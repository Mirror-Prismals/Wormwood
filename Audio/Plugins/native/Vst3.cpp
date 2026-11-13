// Ported configure/create/editor/shutdown from Cardinal BaseSystem/Vst3System.cpp.
#include "Host/Vst3Host.h"
#include "pluginterfaces/vst/ivstnoteexpression.h"
#include "pluginterfaces/vst/ivstmidicontrollers.h"
#include "base/source/fstreamer.h"
#include "public.sdk/source/common/memorystream.h"
#include <algorithm>
#include <cstring>
#include <cstdio>
#include <filesystem>
#include <iostream>
namespace Vst3SystemLogic {
namespace {
using PluginDescriptor = std::pair<VST3::Hosting::Module::Ptr,VST3::Hosting::ClassInfo>;
        bool configurePlugin(Vst3Context& ctx, Vst3Plugin& plugin) {
            if (!plugin.component || !plugin.processor) return false;
            if (plugin.processor->canProcessSampleSize(Steinberg::Vst::kSample32)
                != Steinberg::kResultTrue) return false;

            plugin.inputBusses = plugin.component->getBusCount(Steinberg::Vst::kAudio, Steinberg::Vst::kInput);
            plugin.outputBusses = plugin.component->getBusCount(Steinberg::Vst::kAudio, Steinberg::Vst::kOutput);
            if (plugin.outputBusses <= 0) {
                std::cerr << "VST3: " << plugin.name << " has no output buses." << std::endl;
                return false;
            }
            if (!plugin.isInstrument && plugin.inputBusses <= 0) {
                std::cerr << "VST3: " << plugin.name << " has no input buses." << std::endl;
                return false;
            }

            Steinberg::Vst::BusInfo info{};
            if (plugin.inputBusses > 0 && plugin.component->getBusInfo(Steinberg::Vst::kAudio,
                                                                       Steinberg::Vst::kInput, 0, info) == Steinberg::kResultOk) {
                plugin.inputChannels = std::min(info.channelCount, 2);
                if (info.channelCount > 2) {
                    std::cout << "VST3: " << plugin.name << " input channels clamped to 2." << std::endl;
                }
            }
            if (plugin.component->getBusInfo(Steinberg::Vst::kAudio, Steinberg::Vst::kOutput, 0, info) == Steinberg::kResultOk) {
                plugin.outputChannels = std::min(info.channelCount, 2);
                if (info.channelCount > 2) {
                    std::cout << "VST3: " << plugin.name << " output channels clamped to 2." << std::endl;
                }
            }

            for (int i = 0; i < plugin.inputBusses; ++i) {
                plugin.component->activateBus(Steinberg::Vst::kAudio, Steinberg::Vst::kInput, i, true);
            }
            for (int i = 0; i < plugin.outputBusses; ++i) {
                plugin.component->activateBus(Steinberg::Vst::kAudio, Steinberg::Vst::kOutput, i, true);
            }

            plugin.eventList.setMaxSize(256);
            std::vector<uint32_t> parameterIds;
            plugin.writableParameters.clear();
            if (plugin.controller) {
                const auto count = plugin.controller->getParameterCount();
                if (count < 0 || count > 4096) return false;
                for (int32_t i = 0; i < count; ++i) {
                    Steinberg::Vst::ParameterInfo parameter{};
                    if (plugin.controller->getParameterInfo(i, parameter) == Steinberg::kResultOk) {
                        parameterIds.push_back(parameter.id);
                        if (!(parameter.flags & Steinberg::Vst::ParameterInfo::kIsReadOnly))
                            plugin.writableParameters.push_back(parameter.id);
                    }
                }
            }
            plugin.pending.configure(parameterIds);
            plugin.controllerUpdates.configure(parameterIds);
            plugin.inputParameterChanges.setMaxParameters(static_cast<int32_t>(parameterIds.size()));
            if (!plugin.processData.prepare(*plugin.component, 0, Steinberg::Vst::kSample32))
                return false;
            plugin.safeBufferFrames=ctx.blockSize;
            size_t channels=0;
            for(int bus=0;bus<plugin.processData.numInputs;++bus)channels+=plugin.processData.inputs[bus].numChannels;
            for(int bus=0;bus<plugin.processData.numOutputs;++bus)channels+=plugin.processData.outputs[bus].numChannels;
            plugin.safeBusBuffers.resize(channels);
            size_t channel=0;
            auto bind=[&](Steinberg::Vst::AudioBusBuffers* buses,int count){
                for(int bus=0;bus<count;++bus)for(int c=0;c<buses[bus].numChannels;++c){
                    auto& buffer=plugin.safeBusBuffers[channel++];buffer.assign(ctx.blockSize,0.0f);
                    buses[bus].channelBuffers32[c]=buffer.data();
                }
            };
            bind(plugin.processData.inputs,plugin.processData.numInputs);
            bind(plugin.processData.outputs,plugin.processData.numOutputs);
            Steinberg::Vst::ProcessSetup setup{Steinberg::Vst::kRealtime, Steinberg::Vst::kSample32,
                                               ctx.blockSize, ctx.sampleRate};
            if (plugin.processor->setupProcessing(setup) != Steinberg::kResultOk) {
                std::cerr << "VST3: setupProcessing failed for " << plugin.name << std::endl;
                return false;
            }
            if (plugin.component->setActive(true) != Steinberg::kResultOk) {
                std::cerr << "VST3: setActive failed for " << plugin.name << std::endl;
                return false;
            }
            // kNotImplemented is permitted for processors with no processing
            // state to change; any real failure must leave the instance inactive.
            const auto processing = plugin.processor->setProcessing(true);
            if (processing != Steinberg::kResultOk && processing != Steinberg::kNotImplemented) {
                plugin.component->setActive(false);
                return false;
            }
            plugin.active = true;
            return true;
        }

        template <size_t N, typename TString>
        void copyVstString(char (&out)[N], const TString& in) {
            static_assert(N > 0, "output buffer must not be empty");
            size_t i = 0;
            for (; i + 1 < N; ++i) {
                const auto ch = in[i];
                if (ch == 0) break;
                out[i] = (ch >= 32 && ch < 127) ? static_cast<char>(ch) : '?';
            }
            out[i] = '\0';
        }

        std::vector<Vst3DefaultEditorParam> collectDefaultEditorParams(Vst3Plugin& plugin) {
            std::vector<Vst3DefaultEditorParam> params;
            if (!plugin.controller) return params;

            const int32_t count = plugin.controller->getParameterCount();
            if (count <= 0) return params;
            const int32_t maxParams = std::min<int32_t>(count, 128);
            params.reserve(static_cast<size_t>(maxParams));

            for (int32_t i = 0; i < maxParams; ++i) {
                Steinberg::Vst::ParameterInfo info{};
                if (plugin.controller->getParameterInfo(i, info) != Steinberg::kResultOk) continue;

                Vst3DefaultEditorParam param{};
                param.id = static_cast<int32_t>(info.id);
                param.value = std::clamp(plugin.controller->getParamNormalized(info.id), 0.0, 1.0);
                copyVstString(param.title, info.title);
                copyVstString(param.units, info.units);
                if (param.title[0] == '\0') {
                    std::snprintf(param.title, sizeof(param.title), "Param %d", static_cast<int>(i + 1));
                }
                params.push_back(param);
            }
            return params;
        }

        void setDefaultEditorParam(void* userData, int32_t paramID, double value) {
            auto* plugin = static_cast<Vst3Plugin*>(userData);
            if (!plugin || !plugin->controller) return;
            const Steinberg::Vst::ParamID id = static_cast<Steinberg::Vst::ParamID>(paramID);
            const double normalized = std::clamp(value, 0.0, 1.0);
            Parameter(*plugin, id, normalized);
        }

        bool openDefaultPluginUI(Vst3Plugin& plugin) {
            std::vector<Vst3DefaultEditorParam> params = collectDefaultEditorParams(plugin);
            plugin.uiWindow = Vst3UI_CreateDefaultEditorWindow(
                plugin.name.c_str(),
                params.empty() ? nullptr : params.data(),
                static_cast<int>(params.size()),
                setDefaultEditorParam,
                &plugin
            );
            if (!plugin.uiWindow) return false;
            Vst3UI_ShowWindow(plugin.uiWindow);
            plugin.uiVisible = true;
            plugin.usingDefaultEditor = true;
            return true;
        }

        void openPluginUIInternal(Vst3Plugin& plugin) {
#if defined(_WIN32)
            const char* platformType = Steinberg::kPlatformTypeHWND;
#else
            const char* platformType = Steinberg::kPlatformTypeNSView;
#endif
            if (!plugin.controller) return;
            plugin.view = plugin.controller->createView(Steinberg::Vst::ViewType::kEditor);
            if (!plugin.view) {
                (void)openDefaultPluginUI(plugin);
                return;
            }
            if (plugin.view->isPlatformTypeSupported(platformType) != Steinberg::kResultTrue) {
                plugin.view.reset();
                (void)openDefaultPluginUI(plugin);
                return;
            }
            Steinberg::ViewRect size;
            if (plugin.view->getSize(&size) != Steinberg::kResultTrue) {
                size = Steinberg::ViewRect(0, 0, 640, 480);
            }
            plugin.uiWindow = Vst3UI_CreateWindow(plugin.name.c_str(), size.getWidth(), size.getHeight());
            if (!plugin.uiWindow) {
                plugin.view.reset();
                (void)openDefaultPluginUI(plugin);
                return;
            }
            void* parent = Vst3UI_GetContentView(plugin.uiWindow);
            if (!parent || plugin.view->attached(parent, platformType) != Steinberg::kResultTrue) {
                Vst3UI_CloseWindow(plugin.uiWindow);
                plugin.uiWindow = nullptr;
                plugin.view.reset();
                (void)openDefaultPluginUI(plugin);
                return;
            }
            Vst3UI_ShowWindow(plugin.uiWindow);
            plugin.uiVisible = true;
            plugin.usingDefaultEditor = false;
        }

        Vst3Plugin* createPlugin(Vst3Context& ctx, const PluginDescriptor& desc, bool instrument) {
            if constexpr (!SalamanderPlatform::desktopPluginHosting) return nullptr;
            auto plugin = std::make_unique<Vst3Plugin>();
            plugin->module = desc.first;
            plugin->classInfo = desc.second;
            plugin->name = desc.second.name();
            plugin->isInstrument = instrument;
            auto factory = plugin->module->getFactory();
            plugin->provider = Steinberg::owned(new Steinberg::Vst::PlugProvider(factory, plugin->classInfo, true));
            if (!plugin->provider->initialize()) {
                return nullptr;
            }
            plugin->component = Steinberg::owned(plugin->provider->getComponent());
            plugin->controller = Steinberg::owned(plugin->provider->getController());
            if (plugin->controller) {
                Steinberg::FUnknownPtr<Steinberg::Vst::IMidiMapping> mapping(plugin->controller);
                if(mapping) mapping->getMidiControllerAssignment(0,0,Steinberg::Vst::kPitchBend,plugin->channelPitchBend);
                plugin->supportsMpeMapping = static_cast<bool>(mapping);
                if (mapping) for (int channel = 1; channel <= 15; ++channel) {
                    Steinberg::Vst::ParamID parameter = 0;
                    if (mapping->getMidiControllerAssignment(0, channel, Steinberg::Vst::kPitchBend, parameter)
                        != Steinberg::kResultOk) plugin->supportsMpeMapping = false;
                }
            }
            if (plugin->controller) {
                Steinberg::FUnknownPtr<Steinberg::Vst::INoteExpressionController> expression(plugin->controller);
                if (expression) for (int i = 0; i < expression->getNoteExpressionCount(0, 0); ++i) {
                    Steinberg::Vst::NoteExpressionTypeInfo info{};
                    if (expression->getNoteExpressionInfo(0, 0, i, info) == Steinberg::kResultOk
                        && info.typeId == Steinberg::Vst::kTuningTypeID) plugin->supportsNoteTuning = true;
                }
            }
            plugin->processor = Steinberg::FUnknownPtr<Steinberg::Vst::IAudioProcessor>(plugin->component);
            if (!configurePlugin(ctx, *plugin)) {
                return nullptr;
            }
            ctx.plugins.push_back(std::move(plugin));
            return ctx.plugins.back().get();
        }

        void shutdownPlugin(Vst3Plugin& plugin) {
            if (plugin.view) {
                plugin.view->removed();
                plugin.view.reset();
            }
        if (plugin.uiWindow) {
            Vst3UI_CloseWindow(plugin.uiWindow);
            plugin.uiWindow = nullptr;
        }
        plugin.uiVisible = false;
        plugin.usingDefaultEditor = false;
            if (plugin.processor) {
                plugin.processor->setProcessing(false);
            }
            if (plugin.component) {
                plugin.component->setActive(false);
            }
            plugin.processData.unprepare();
            plugin.active = false;
        }
    } // namespace

    void Vst3_OpenPluginUI(Vst3Plugin& plugin) {
        if (plugin.uiWindow) {
            if (!Vst3UI_IsWindowVisible(plugin.uiWindow)) {
                Vst3UI_ShowWindow(plugin.uiWindow);
            }
            plugin.uiVisible = true;
            return;
        }
        openPluginUIInternal(plugin);
    }


namespace {
// UI-to-DSP handoff. Native editor edits must reach inputParameterChanges, not
// just the controller's display value (the old generic editor missed this).
class Handler final : public Steinberg::Vst::IComponentHandler {
    std::atomic<Steinberg::uint32> refs{1}; Vst3Plugin& plugin;
public:
    explicit Handler(Vst3Plugin& p):plugin(p){}
    Steinberg::tresult PLUGIN_API queryInterface(const Steinberg::TUID id,void** out) override {
        if(Steinberg::FUnknownPrivate::iidEqual(id,Steinberg::FUnknown::iid)
            ||Steinberg::FUnknownPrivate::iidEqual(id,Steinberg::Vst::IComponentHandler::iid)) {
            *out=static_cast<Steinberg::Vst::IComponentHandler*>(this);addRef();return Steinberg::kResultOk;}
        *out=nullptr;return Steinberg::kNoInterface;
    }
    Steinberg::uint32 PLUGIN_API addRef() override{return ++refs;}
    Steinberg::uint32 PLUGIN_API release() override{auto n=--refs;if(!n)delete this;return n;}
    Steinberg::tresult PLUGIN_API beginEdit(Steinberg::Vst::ParamID) override{return Steinberg::kResultOk;}
    Steinberg::tresult PLUGIN_API endEdit(Steinberg::Vst::ParamID) override{return Steinberg::kResultOk;}
    Steinberg::tresult PLUGIN_API performEdit(Steinberg::Vst::ParamID id,double v) override{
        return plugin.pending.publish(id,v)?Steinberg::kResultOk:Steinberg::kResultFalse;
    }
    Steinberg::tresult PLUGIN_API restartComponent(Steinberg::int32 flags) override{
        // A title/value notification is not an instruction to reset DSP. Real
        // I/O changes are serviced with processing stopped before buffers change.
        if (flags & Steinberg::Vst::kReloadComponent) return Steinberg::kNotImplemented;
        plugin.restartFlags.fetch_or(flags,std::memory_order_release);
        if (flags & (Steinberg::Vst::kIoChanged | Steinberg::Vst::kLatencyChanged))
            plugin.realtimeResetRequested.store(true,std::memory_order_release);
        return Steinberg::kResultOk;
    }
};
}
void Parameter(Vst3Plugin& p,uint32_t id,double value) {
    if (!std::isfinite(value) || value < 0. || value > 1.) return;
    if(p.controller)p.controller->setParamNormalized(id,value);
    if(p.handler)p.handler->performEdit(id,value);
}
void Discover(Vst3Context& ctx,const std::string& root) {
    if(!ctx.hostApp) {
        ctx.hostApp=Steinberg::owned(new Steinberg::Vst::HostApplication());
        Steinberg::Vst::PluginContextFactory::instance().setPluginContext(ctx.hostApp);
    }
    const std::vector<std::filesystem::path> paths={std::filesystem::path(root)/"USER_VST3"};
    for(const auto& dir:paths) {
        std::error_code ec;
        for(std::filesystem::directory_iterator it(dir,ec),end;!ec&&it!=end;it.increment(ec)) {
            if(it->path().extension()!=".vst3")continue;
            std::string error;VST3::Hosting::Module::Ptr module;
            for (const auto& loaded : ctx.modules) if (loaded && loaded->getPath()==it->path().string()) { module=loaded; break; }
            if(!module) { module=VST3::Hosting::Module::create(it->path().string(),error); if(module)ctx.modules.push_back(module); }
            if(!module)continue;
            for(const auto& info:module->getFactory().classInfos()) {
                if(info.category()!=kVstAudioEffectClass)continue;
                Vst3AvailablePlugin p;p.name=info.name();p.vendor=info.vendor();p.uniqueId=info.ID().toString();
                p.modulePath=it->path().string();p.module=module;p.classInfo=info;
                for(const auto& sub:info.subCategories())if(sub=="Instrument")p.isInstrument=true;
                ctx.availablePlugins.push_back(p);
            }
        }
    }
}
Vst3Plugin* Create(Vst3Context& ctx,const Vst3AvailablePlugin& a) {
    auto* p=createPlugin(ctx,{a.module,a.classInfo},a.isInstrument);
    if(p&&p->controller){p->handler=Steinberg::owned(new Handler(*p));p->controller->setComponentHandler(p->handler);}
    return p;
}
void Destroy(Vst3Context& ctx,Vst3Plugin* p) {
    for(auto it=ctx.plugins.begin();it!=ctx.plugins.end();++it)if(it->get()==p){
        if(p->controller)p->controller->setComponentHandler(nullptr);
        shutdownPlugin(*p);ctx.plugins.erase(it);return;
    }
}
// Same bus-buffer and ProcessContext setup as Cardinal, with stereo preserved.
// Called for bounded spans ending exactly at MIDI events by the new transport.
bool Process(Vst3Context& ctx,Vst3Plugin& p,const float* il,const float* ir,float* ol,float* or_,
    int n,int64_t offset,bool playing,const std::array<float,128>& desired,std::array<float,128>& held,const std::vector<Microtonal::Voice>* tuned) {
    if(!p.active||n<=0||n>p.safeBufferFrames)return false;
    const bool bypass=p.bypassed.load(std::memory_order_relaxed);
    if(bypass&&!p.isInstrument)return false;
    if(p.realtimeResetRequested.load(std::memory_order_acquire)
        ||(p.restartFlags.load(std::memory_order_acquire)&(Steinberg::Vst::kIoChanged|Steinberg::Vst::kLatencyChanged)))return false;
    auto& data=p.processData;
    // Restore every bus to its owned storage; never leave pointers to a prior
    // callback's transient output arrays in a silent auxiliary bus.
    size_t channel=0;
    for(auto pair:{std::make_pair(data.inputs,data.numInputs),std::make_pair(data.outputs,data.numOutputs)})
        for(int b=0;b<pair.second;++b){pair.first[b].silenceFlags=0;
            for(int c=0;c<pair.first[b].numChannels;++c){auto& buf=p.safeBusBuffers[channel++];
                std::fill_n(buf.data(),n,0.f);pair.first[b].channelBuffers32[c]=buf.data();}}
    if(data.numInputs>0)for(int c=0;c<std::min(2,data.inputs[0].numChannels);++c)
        std::copy_n(c==0?il:ir,n,data.inputs[0].channelBuffers32[c]);
    p.eventList.clear();p.inputParameterChanges.clearQueue();
    p.pending.consume([&](uint32_t id,double value){Steinberg::int32 index=0,point=0;
        if(auto* q=p.inputParameterChanges.addParameterData(id,index))q->addPoint(0,value,point);});
    if(p.isInstrument && ctx.processPitchBend >= 0.) {
        if(p.channelPitchBend==Steinberg::Vst::kNoParamId && ctx.processPitchBend!=0.5)return false;
        if(p.channelPitchBend!=Steinberg::Vst::kNoParamId){Steinberg::int32 index=0,point=0;
            if(auto* q=p.inputParameterChanges.addParameterData(p.channelPitchBend,index))q->addPoint(0,ctx.processPitchBend,point);}
    }
    if(p.isInstrument)for(int key=0;key<128;++key){float next=bypass?0.f:desired[key];
        if((next>0)!=(held[key]>0)){Steinberg::Vst::Event event{};event.busIndex=0;event.sampleOffset=0;
            if(next>0){event.type=Steinberg::Vst::Event::kNoteOnEvent;event.noteOn.pitch=key;event.noteOn.velocity=next;event.noteOn.noteId=key+1;}
            else{event.type=Steinberg::Vst::Event::kNoteOffEvent;event.noteOff.pitch=key;event.noteOff.noteId=key+1;}
            p.eventList.addEvent(event);}
        held[key]=next;
    }
    if(p.isInstrument)Microtonal::Dispatch(p.tuningVoices,p.nextTuningVoiceId,tuned,Microtonal::Output::Native,bypass,
        [&](bool on,const Microtonal::ActiveVoice& v){
            Steinberg::Vst::Event event{};event.busIndex=0;event.sampleOffset=0;
            if(on){event.type=Steinberg::Vst::Event::kNoteOnEvent;event.noteOn.pitch=v.key;event.noteOn.tuning=v.cents;event.noteOn.velocity=v.velocity;event.noteOn.noteId=v.eventId;}
            else{event.type=Steinberg::Vst::Event::kNoteOffEvent;event.noteOff.pitch=v.key;event.noteOff.noteId=v.eventId;}
            p.eventList.addEvent(event);
        },[](int,int,int){});
    p.processContext={};p.processContext.sampleRate=ctx.sampleRate;p.processContext.projectTimeSamples=offset;
    p.processContext.continousTimeSamples=ctx.continuousSamples;p.processContext.tempo=ctx.processClockBpm;
    p.processContext.projectTimeMusic=ctx.processClockQuarterNotes;p.processContext.timeSigNumerator=ctx.processClockNumerator;
    p.processContext.timeSigDenominator=ctx.processClockDenominator;
    using PC=Steinberg::Vst::ProcessContext;
    p.processContext.state=PC::kContTimeValid|PC::kTempoValid|PC::kProjectTimeMusicValid|PC::kTimeSigValid|(playing?PC::kPlaying:0);
    data.processMode=Steinberg::Vst::kRealtime;data.symbolicSampleSize=Steinberg::Vst::kSample32;data.numSamples=n;
    data.inputEvents=&p.eventList;data.inputParameterChanges=&p.inputParameterChanges;data.processContext=&p.processContext;
    data.outputEvents=nullptr;data.outputParameterChanges=nullptr;
    if(p.processor->process(data)!=Steinberg::kResultOk)return false;
    if(data.numOutputs<1||data.outputs[0].numChannels<1)return false;
    auto* left=data.outputs[0].channelBuffers32[0];auto* right=data.outputs[0].channelBuffers32[data.outputs[0].numChannels>1?1:0];
    for(int i=0;i<n;++i)if(!std::isfinite(left[i])||!std::isfinite(right[i])){p.realtimeResetRequested.store(true);return false;}
    for(int i=0;i<n;++i){ol[i]=bypass?0:left[i];or_[i]=bypass?0:right[i];}return true;
}
bool SaveState(Vst3Plugin& p,std::vector<uint8_t>& out){
    Steinberg::MemoryStream stream;
    if(p.component->getState(&stream)!=Steinberg::kResultOk)return false;
    if(stream.getSize()<0||stream.getSize()>16*1024*1024)return false;
    if(stream.getSize()==0){out.clear();return true;}
    const auto* data=reinterpret_cast<const uint8_t*>(stream.getData());out.assign(data,data+stream.getSize());return true;
}
bool LoadState(Vst3Plugin& p,const uint8_t* data,size_t n){
    if(n>16*1024*1024)return false;
    Steinberg::MemoryStream stream(const_cast<uint8_t*>(data),static_cast<Steinberg::TSize>(n));
    if(p.component->setState(&stream)!=Steinberg::kResultOk)return false;
    stream.seek(0,Steinberg::IBStream::kIBSeekSet,nullptr);if(p.controller)p.controller->setComponentState(&stream);return true;
}
bool Automate(Vst3Plugin& p,uint32_t id,double value) {
    if(std::find(p.writableParameters.begin(),p.writableParameters.end(),id)==p.writableParameters.end())return false;
    if(!p.pending.publish(id,value))return false;
    return p.controllerUpdates.publish(id,value);
}
bool Reconfigure(Vst3Context& ctx,Vst3Plugin& p) {
    p.processor->setProcessing(false);
    p.component->setActive(false);
    p.active=false;
    p.realtimeResetRequested.store(false,std::memory_order_release);
    p.processData.unprepare();
    return configurePlugin(ctx,p);
}
bool Service(Vst3Context& ctx,Vst3Plugin& p) {
    // Automation reaches DSP on the preparation thread; its display/controller
    // update is delivered here on the UI thread (SDK connection proxies require it).
    p.controllerUpdates.consume([&](uint32_t id,double value){if(p.controller)p.controller->setParamNormalized(id,value);});
    const auto flags=p.restartFlags.exchange(0,std::memory_order_acq_rel);
    if (flags & (Steinberg::Vst::kIoChanged | Steinberg::Vst::kLatencyChanged)) {
        if (!Reconfigure(ctx,p)) return false;
    } else if (p.realtimeResetRequested.exchange(false,std::memory_order_acq_rel)) {
        p.processor->setProcessing(false);
        p.processor->setProcessing(true);
    }
    if ((flags & (Steinberg::Vst::kParamValuesChanged | Steinberg::Vst::kIoChanged
                  | Steinberg::Vst::kLatencyChanged)) && p.controller) {
        const auto count=p.controller->getParameterCount();
        if(count<0||count>4096)return false;
        for(int32_t i=0;i<count;++i){Steinberg::Vst::ParameterInfo info{};
            if(p.controller->getParameterInfo(i,info)==Steinberg::kResultOk)
                p.pending.publish(info.id,p.controller->getParamNormalized(info.id));
        }
    }
    return true;
}
} // namespace Vst3SystemLogic
