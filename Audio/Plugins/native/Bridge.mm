// Desktop-only Cardinal plugin backends adapted to Wormwood track identities.
// All entry points run on the workstation's control/render-ahead thread. The
// native audio callback only consumes its existing prepared PCM queue.
#import <Cocoa/Cocoa.h>
#include "Bridge.h"
#include "Host/Vst3Host.h"
#include "Host/Vst2Host.h"
#include "json.hpp"
#include "InsertAssignment.h"
#include "StockEq.h"
#include "AutomationParameters.h"
#include <map>
#include <limits>
#include <stdexcept>
using Json=nlohmann::json;
namespace {
struct Insert {
    uint64_t id=0;std::string track;std::string chain="playback";Vst3AvailablePlugin descriptor;
    Vst3Plugin* vst=nullptr;ClapPlugin* clap=nullptr;AudioUnitPlugin* au=nullptr;
    bool bendActive=false;
    std::array<float,128> held{};
    std::unique_ptr<LShapingTrackState> stock;
    bool instrument()const{return descriptor.isInstrument;}
    Vst3UiWindow* window()const{return stock?nullptr:vst?vst->uiWindow:clap?clap->uiWindow:au->uiWindow;}
    std::atomic<bool>& bypass(){return stock?stock->bypassed:vst?vst->bypassed:clap?clap->bypassed:au->bypassed;}
};
struct Host {
    const std::thread::id controlThread=std::this_thread::get_id();
    Vst3Context ctx;std::vector<Insert> inserts;uint64_t next=1;std::string reply;
    std::map<std::string,int64_t> nextFrame,continuousFrames;
    std::array<float,2048> a{},b{},c{},d{};
    bool scanned=false;
    ~Host(){for(auto& p:inserts)destroy(p);ClapSystemLogic::Cleanup(ctx);AudioUnitSystemLogic::CleanupBackend(ctx);}
    void destroy(Insert& p){if(p.stock){p.stock.reset();return;}if(p.vst)Vst3SystemLogic::Destroy(ctx,p.vst);else if(p.clap)ClapSystemLogic::DestroyPlugin(ctx,p.clap);else AudioUnitSystemLogic::DestroyPlugin(ctx,p.au);}
    Insert& find(uint64_t id){for(auto& p:inserts)if(p.id==id)return p;throw std::runtime_error("Plugin insert no longer exists");}
    static const char* format(HostedPluginFormat f){switch(f){case HostedPluginFormat::Vst3:return "VST3";case HostedPluginFormat::Clap:return "CLAP";case HostedPluginFormat::AudioUnit:return "Audio Unit";case HostedPluginFormat::Vst2:return "VST2";}return "Unknown";}
    Json status(){Json rows=Json::array(),catalog=Json::array();
        for(auto& p:inserts){
            Json row={{"id",p.id},{"track",p.track},{"chain",p.chain},{"name",p.descriptor.name},{"plugin_id",p.descriptor.uniqueId},{"instrument",p.instrument()},
                {"format",p.stock?"Stock":format(p.descriptor.format)},{"bypassed",p.bypass().load()},
                {"editor_open",p.window()&&Vst3UI_IsWindowVisible(p.window())},{"stock",bool(p.stock)}};
            if(p.stock){
                row["state"]=LShapingSystemLogic::Serialize(*p.stock);
                std::array<float,97> hz{},db{};
                for(size_t i=0;i<hz.size();++i)hz[i]=20*std::pow(1000.f,float(i)/96.f);
                LShapingSystemLogic::ResponseCurveDb(*p.stock,hz.data(),db.data(),97,ctx.sampleRate);
                if(p.stock->bypassed.load(std::memory_order_relaxed))db.fill(0.f);
                row["response_db"]=db;
            }
            // Observe edits made inside a native editor on its control thread.
            // This bounded metadata is never queried by the audio callback.
            if(!p.stock && row["editor_open"].get<bool>()){
                Json values=Json::array();
                const auto add=[&](uint32_t id,double value){
                    if(values.size()<4096 && std::isfinite(value))values.push_back({id,value});
                };
                if(p.vst && p.vst->controller){
                    for(auto id:p.vst->writableParameters)add(id,p.vst->controller->getParamNormalized(id));
                }else if(p.clap){
                    for(const auto& info:p.clap->parameterInfo)add(info.id,ClapSystemLogic::GetNormalizedParam(*p.clap,info.id));
                }else if(p.au){
                    for(const auto& info:p.au->parameterInfo)add(info.id,AudioUnitSystemLogic::GetNormalizedParam(*p.au,info.id));
                }
                row["editor_parameters"]=std::move(values);
            }
            rows.push_back(std::move(row));
        }
        catalog.push_back({{"stock","LShaping"},{"name","U Shaping EQ"},{"format","Stock"},{"instrument",false}});
        for(size_t i=0;i<ctx.availablePlugins.size();++i){auto& p=ctx.availablePlugins[i];catalog.push_back({{"index",i},{"plugin_id",p.uniqueId},{"name",p.name},{"format",format(p.format)},{"instrument",p.isInstrument},{"vendor",p.vendor}});}
        return {{"available",true},{"scanned",scanned},{"inserts",rows},{"catalog",catalog},{"sample_rate",ctx.sampleRate}};
    }
    void open(Insert& p){if(p.stock)return;if(p.vst)Vst3SystemLogic::Vst3_OpenPluginUI(*p.vst);else if(p.clap)ClapSystemLogic::OpenPluginUI(*p.clap);else AudioUnitSystemLogic::OpenPluginUI(*p.au);}
    void hide(Insert& p){
        if(!p.window())return;
        if(p.clap&&p.clap->guiCreated&&p.clap->gui)p.clap->gui->hide(p.clap->plugin);
        Vst3UI_HideWindow(p.window());
        if(p.vst)p.vst->uiVisible=false;else if(p.clap)p.clap->uiVisible=false;else p.au->uiVisible=false;
    }
    void reset(Insert& p){
        // Called between render ranges on seeks/reconfiguration, never concurrently with process.
        p.held.fill(0);
        if(p.vst)p.vst->tuningVoices.clear();if(p.clap)p.clap->tuningVoices.clear();if(p.au)p.au->tuningVoices.clear();
        if(p.stock){LShapingSystemLogic::ResetPath(*p.stock,LShapingSystemLogic::ProcessingPath::Realtime);}
        else if(p.vst){p.vst->processor->setProcessing(false);p.vst->processor->setProcessing(true);p.vst->realtimeResetRequested.store(false);}
        else if(p.clap&&p.clap->plugin&&p.clap->active){
            // A transport reset precedes process(), including the first block
            // after transferring preparation to its independent thread.
            p.clap->audioThreadHash.store(static_cast<uint64_t>(std::hash<std::thread::id>{}(std::this_thread::get_id())),std::memory_order_relaxed);
            p.clap->plugin->reset(p.clap->plugin);
        }
        else if(p.au){ /* AU reset uses the backend adapter below. */ AudioUnitSystemLogic::Reset(*p.au); }
    }
    void rate(double rate){if(rate==ctx.sampleRate)return;
        if(rate<8000||rate>384000)throw std::runtime_error("Invalid plugin sample rate");
        for(auto& p:inserts)if(p.clap&&p.clap->vst2)
            throw std::runtime_error("Reload the VST2 insert after changing the device sample rate");
        // Preserve native state and identities while re-activating at the device/export rate.
        ctx.sampleRate=rate;
        for(auto& p:inserts){
            if(p.stock){}
            else if(p.vst){if(!Vst3SystemLogic::Reconfigure(ctx,*p.vst))throw std::runtime_error("VST3 rejected sample rate");}
            else if(p.clap){p.clap->restartRequested.store(true);}
            else AudioUnitSystemLogic::Reconfigure(ctx,*p.au);
            reset(p);
        }
        ClapSystemLogic::UpdateMainThread(ctx);nextFrame.clear();continuousFrames.clear();
    }
};
}
extern "C" void* ww_plugins_create(){try{return new Host;}catch(...){return nullptr;}}
extern "C" void ww_plugins_destroy(void* p){delete static_cast<Host*>(p);}
extern "C" const char* ww_plugins_command(void* opaque,const char* request){
    auto& h=*static_cast<Host*>(opaque);
    if(std::this_thread::get_id()!=h.controlThread)return "{\"error\":\"Plugin controls require their owning control thread\"}";
    try{
        Json q=Json::parse(request);const auto op=q.at("op").get<std::string>();
        // Pure stock processing tests may own a host on a non-AppKit thread.
        // Native discovery, third-party factories and editor/controller paths
        // require macOS main in addition to the serialized control owner.
        if(![NSThread isMainThread]&&(op=="scan"||(op=="add"&&q.value("stock","")!="LShaping")
            ||op=="editor"||op=="toggle_editor"))throw std::runtime_error("Native plugin discovery and UI require the main thread");
        if(op=="scan"){
            if(!h.scanned || q.value("refresh",false)){const auto root=q.at("root").get<std::string>();
                const auto userRoot=q.value("user_root",root+"/USER");
                h.ctx.availablePlugins.clear();
                Vst3SystemLogic::Discover(h.ctx,userRoot);ClapSystemLogic::Initialize(h.ctx,root,userRoot);AudioUnitSystemLogic::InitializeBackend(h.ctx,userRoot);
                std::sort(h.ctx.availablePlugins.begin(),h.ctx.availablePlugins.end(),[](auto& a,auto& b){return a.name<b.name;});h.scanned=true;}
        }else if(op=="add"){
            h.rate(q.value("sample_rate",double(h.ctx.sampleRate)));
            const auto track=q.at("track").get<std::string>();
            const auto kind=q.value("track_kind","");
            const auto chain=q.value("chain","playback");
            if(chain!="playback"&&chain!="preamp")throw std::runtime_error("Unknown plugin chain");
            Vst3AvailablePlugin descriptor;
            const bool stock=q.value("stock","")=="LShaping";
            if(stock){descriptor.name="U Shaping EQ";descriptor.uniqueId="LShaping";}
            else{auto index=q.at("index").get<size_t>();if(index>=h.ctx.availablePlugins.size())throw std::runtime_error("Invalid catalog selection");descriptor=h.ctx.availablePlugins[index];}
            if(kind!="audio"&&kind!="midi"&&kind!="mix"&&kind!="channel")throw std::runtime_error("Plugins require an audio/MIDI track, channel bus or mix bus");
            if(chain=="preamp"&&(kind!="audio"||descriptor.isInstrument))throw std::runtime_error("Audio capture preamps accept effects only");
            if(descriptor.isInstrument&&kind!="midi")throw std::runtime_error("Instruments require a MIDI track");
            PluginInserts::assign(h.inserts,track,descriptor.isInstrument,128,[&]{
                Insert p;p.id=h.next++;p.track=track;p.chain=chain;p.descriptor=descriptor;
                if(stock){p.stock=std::make_unique<LShapingTrackState>();return p;}
                switch(p.descriptor.format){
                    case HostedPluginFormat::Vst3:p.vst=Vst3SystemLogic::Create(h.ctx,p.descriptor);break;
                    case HostedPluginFormat::AudioUnit:p.au=AudioUnitSystemLogic::CreatePlugin(h.ctx,p.descriptor,p.instrument());break;
                    default:p.clap=ClapSystemLogic::CreatePlugin(h.ctx,p.descriptor,p.instrument());break;
                }
                if(!p.vst&&!p.clap&&!p.au)throw std::runtime_error("Plugin failed to initialize; check architecture and format support");
                return p;
            },[&](Insert& retired){h.destroy(retired);});
            // Cardinal leaves a newly loaded editor closed. The Edison socket
            // is plugged only when the user opens its editor from that socket.
        }else if(op=="hide"){
            if(q.contains("id"))h.hide(h.find(q.at("id")));
            else for(auto& p:h.inserts)h.hide(p);
        }else if(op=="toggle_editor"){
            auto& p=h.find(q.at("id"));
            if(p.window()&&Vst3UI_IsWindowVisible(p.window()))h.hide(p);else h.open(p);
        }else if(op=="editor")h.open(h.find(q.at("id")));
        else if(op=="bypass"){auto& p=h.find(q.at("id"));p.bypass().store(q.at("value").get<bool>());}
        else if(op=="remove"){
            auto id=q.at("id").get<uint64_t>();auto& p=h.find(id);h.destroy(p);
            h.inserts.erase(std::remove_if(h.inserts.begin(),h.inserts.end(),[&](auto& x){return x.id==id;}),h.inserts.end());
        }else if(op=="move"){
            auto& p=h.find(q.at("id"));if(p.instrument())throw std::runtime_error("The instrument stays before its effects");
            auto index=static_cast<size_t>(&p-h.inserts.data());int direction=q.at("direction").get<int>();
            for(int j=int(index)+(direction<0?-1:1);j>=0&&j<int(h.inserts.size());j+=(direction<0?-1:1))
                if(h.inserts[j].track==p.track&&h.inserts[j].chain==p.chain&&!h.inserts[j].instrument()){std::swap(h.inserts[index],h.inserts[j]);break;}
        }else if(op=="transfer"){
            auto& selected=h.find(q.at("id"));
            if(selected.instrument())throw std::runtime_error("Instruments cannot enter the audio preamp");
            const auto chain=q.at("chain").get<std::string>();
            if(chain!="playback"&&chain!="preamp")throw std::runtime_error("Unknown plugin chain");
            if(q.value("track_kind","")!="audio")throw std::runtime_error("Only audio tracks have a preamp");
            // Move the owned instance, never serialize/recreate its DSP or UI.
            // Reserve and allocate before mutation so failure leaves it intact.
            std::vector<size_t> positions;positions.reserve(h.inserts.size());
            for(size_t i=0;i<h.inserts.size();++i)if(h.inserts[i].track==selected.track&&!h.inserts[i].instrument()&&(h.inserts[i].chain==chain||h.inserts[i].id==selected.id))positions.push_back(i);
            size_t from=0;for(size_t i=0;i<positions.size();++i)if(h.inserts[positions[i]].id==selected.id)from=i;
            const size_t to=std::min(q.at("index").get<size_t>(),positions.size()-1);
            selected.chain=chain;
            while(from<to){std::swap(h.inserts[positions[from]],h.inserts[positions[from+1]]);++from;}
            while(from>to){std::swap(h.inserts[positions[from]],h.inserts[positions[from-1]]);--from;}
        }else if(op=="reorder"){
            auto& p=h.find(q.at("id"));if(p.instrument())throw std::runtime_error("The instrument stays before its effects");
            // Reorder the selected track's effect positions without disturbing
            // other tracks, instrument ownership, editor windows or DSP state.
            std::vector<size_t> positions;size_t from=0;
            for(size_t i=0;i<h.inserts.size();++i)if(h.inserts[i].track==p.track&&h.inserts[i].chain==p.chain&&!h.inserts[i].instrument()){
                if(h.inserts[i].id==p.id)from=positions.size();positions.push_back(i);
            }
            const size_t to=std::min(q.at("index").get<size_t>(),positions.size()-1);
            while(from<to){std::swap(h.inserts[positions[from]],h.inserts[positions[from+1]]);++from;}
            while(from>to){std::swap(h.inserts[positions[from]],h.inserts[positions[from-1]]);--from;}
        }else if(op=="stock_parameter"){
            auto& p=h.find(q.at("id"));if(!p.stock)throw std::runtime_error("Not a stock EQ insert");
            const auto key=q.at("parameter").get<std::string>();const double value=q.at("value");
            if(!std::isfinite(value))throw std::runtime_error("Invalid EQ parameter");
            if(key=="gain_db")p.stock->gainDb.store(std::clamp(float(value),-24.f,24.f));
            else if(key=="low_slope"||key=="high_slope"){
                const std::array<int,7> slopes{{6,12,18,24,30,36,48}};
                if(std::find(slopes.begin(),slopes.end(),value)==slopes.end())throw std::runtime_error("Invalid EQ slope");
                p.stock->bands[key=="high_slope"?1:0].slopeDbOct.store(int(value));
            }else if(key=="low_hz"||key=="high_hz"){
                const float upper=std::min(20000.f,h.ctx.sampleRate*.475f);
                const int edge=key=="high_hz"?1:0;
                const float floor=edge?std::min(p.stock->bands[0].frequencyHz.load()*1.01f,upper):20.f;
                const float ceiling=edge?upper:std::min(p.stock->bands[1].frequencyHz.load()/1.01f,upper/1.01f);
                p.stock->bands[edge].frequencyHz.store(std::clamp(float(value),floor,std::max(floor,ceiling)));
            }else throw std::runtime_error("Unknown EQ parameter");
        }else if(op=="automation_catalog"){
            Json devices=Json::array();
            // Chain labels come from status, keeping this independent of the
            // preamp owner's internal representation. Cardinal excludes preamps.
            const auto status=h.status();
            for(auto row:status.at("inserts")){
                if(row.value("stock",false)||row.value("chain",std::string("playback"))!="playback")continue;
                auto& p=h.find(row.at("id"));
                row["parameters"]=CardinalAutomation::parameters(p);
                row["label"]=(p.instrument()?"I:":"")+CardinalAutomation::compact(p.descriptor.name,p.instrument()?8:10);
                row["chain"]="playback";devices.push_back(std::move(row));
            }
            h.reply=Json({{"devices",devices}}).dump();return h.reply.c_str();
        }else if(op=="parameter"){
            CardinalAutomation::set(h.find(q.at("id")),q.at("parameter").get<uint32_t>(),q.at("value").get<double>());
        }else if(op=="save"||op=="restore"){
            auto& p=h.find(q.at("id"));std::vector<uint8_t> data;bool ok=false;
            if(op=="save"){
                if(p.stock){auto state=LShapingSystemLogic::Serialize(*p.stock).dump();data.assign(state.begin(),state.end());ok=true;}
                else ok=p.vst?Vst3SystemLogic::SaveState(*p.vst,data):p.clap?ClapSystemLogic::SaveState(*p.clap,data):AudioUnitSystemLogic::SaveState(*p.au,data);
                if(!ok)throw std::runtime_error("Plugin does not support state save");
                h.reply=Json({{"state",data},{"format",p.stock?"Stock":Host::format(p.descriptor.format)},{"plugin_id",p.descriptor.uniqueId}}).dump();return h.reply.c_str();
            }
            data=q.at("state").get<std::vector<uint8_t>>();if(data.size()>16*1024*1024)throw std::runtime_error("Plugin state exceeds 16 MiB");
            if(p.stock){LShapingSystemLogic::Deserialize(*p.stock,Json::parse(data));ok=true;}
            else ok=p.vst?Vst3SystemLogic::LoadState(*p.vst,data.data(),data.size()):p.clap?ClapSystemLogic::LoadState(*p.clap,data.data(),data.size()):AudioUnitSystemLogic::LoadState(*p.au,data.data(),data.size());
            if(!ok)throw std::runtime_error("Plugin rejected state");h.reset(p);
        }else if(op=="prepare_rate"){
            h.rate(q.at("sample_rate").get<double>());
        }else if(op=="tick"){
            ClapSystemLogic::UpdateMainThread(h.ctx);
            for(auto& p:h.inserts)if(p.vst)
                if(!Vst3SystemLogic::Service(h.ctx,*p.vst))throw std::runtime_error("VST3 restart failed; reload the insert");
        }else if(op!="status")throw std::runtime_error("Unknown plugin operation");
        h.reply=h.status().dump();
    }catch(const std::exception& e){h.reply=Json({{"error",e.what()}}).dump();}
    catch(...){h.reply="{\"error\":\"Native plugin exception\"}";}
    return h.reply.c_str();
}
extern "C" bool ww_plugins_automate(void* opaque,uint64_t insert,uint32_t parameter,double value){
    if(!opaque||!std::isfinite(value)||value<0.||value>1.)return false;
    try{auto& p=static_cast<Host*>(opaque)->find(insert);
        if(p.stock)return false;
        if(p.vst)return Vst3SystemLogic::Automate(*p.vst,parameter,value);
        if(p.clap)return ClapSystemLogic::QueueNormalizedParam(*p.clap,parameter,value);
        if(p.au)return AudioUnitSystemLogic::SetNormalizedParam(*p.au,parameter,value);
    }catch(...){}
    return false;
}
static bool processChain(void* opaque,const char* track,float* left,float* right,int frames,
    double rate,int64_t start,double bpm,double quarters,int numerator,int denominator,const float* desired,const char* chainName,int discontinuity,const double* tunedData,int tunedCount,double bend=0.){
    auto& h=*static_cast<Host*>(opaque);
    try{
        // Device-rate changes belong to main-thread prepare_rate/add commands.
        // A render worker must never call setupProcessing or main-thread CLAP hooks.
        if(frames<1||frames>2048||rate!=h.ctx.sampleRate||!std::isfinite(bend)||std::abs(bend)>2.)return false;
        const std::string chain=chainName?chainName:"playback";
        if(chain!="playback"&&chain!="preamp")return false;
        const std::string clockKey=chain+"\x1f"+track;
        auto next=h.nextFrame.find(clockKey);
        if(discontinuity>0||(discontinuity<0&&next!=h.nextFrame.end()&&next->second!=start))
            for(auto& p:h.inserts)if(p.track==track&&p.chain==chain)h.reset(p);
        std::array<float,128> held{};std::copy_n(desired,128,held.data());
        if(tunedCount<0||tunedCount>128||(tunedCount&&!tunedData))return false;
        std::vector<Microtonal::Voice> tuned;tuned.reserve(tunedCount);
        for(int i=0;i<tunedCount;i++){const double* v=tunedData+i*5;
            if(!std::isfinite(v[0])||!std::isfinite(v[1])||!std::isfinite(v[2])||!std::isfinite(v[3])||!std::isfinite(v[4])||v[1]<0||v[1]>127)return false;
            tuned.push_back({uint64_t(v[0]),int(v[1]),v[2],float(v[3]),uint64_t(v[4])});}
        // Unsupported instruments fail explicitly instead of rounding pitches.
        for(auto& p:h.inserts)if(p.track==track&&p.chain==chain&&p.instrument()&&!tuned.empty()){
            if(p.au)return false;
            if(p.clap&&ClapSystemLogic::TuningOutput(*p.clap,0)==Microtonal::Output::Off)return false;
        }
        if(bend!=0.) for(auto& p:h.inserts)if(p.track==track&&p.chain==chain&&p.instrument()&&p.vst&&p.vst->channelPitchBend==Steinberg::Vst::kNoParamId)return false;
        h.ctx.continuousSamples=h.continuousFrames[clockKey];
        h.ctx.processClockBpm=bpm;h.ctx.processClockQuarterNotes=quarters;
        h.ctx.processClockNumerator=numerator;h.ctx.processClockDenominator=denominator;
        std::copy_n(left,frames,h.a.data());std::copy_n(right,frames,h.b.data());
        float* il=h.a.data();float* ir=h.b.data();float* ol=h.c.data();float* or_=h.d.data();
        for(auto& p:h.inserts){if(p.track!=track||p.chain!=chain)continue;
            h.ctx.processPitchBend = (p.bendActive || bend!=0.) ? std::clamp(0.5+bend/4.,0.,1.) : -1.;
            p.bendActive = bend!=0.;
            std::fill_n(ol,frames,0);std::fill_n(or_,frames,0);bool ok=false;
            if(p.stock){std::copy_n(il,frames,ol);std::copy_n(ir,frames,or_);LShapingSystemLogic::ProcessStereo(*p.stock,ol,or_,frames,rate,LShapingSystemLogic::ProcessingPath::Realtime);ok=true;}
            else if(p.vst)ok=Vst3SystemLogic::Process(h.ctx,*p.vst,il,ir,ol,or_,frames,start,true,held,p.held,&tuned);
            else if(p.clap)ok=p.instrument()?ClapSystemLogic::ProcessInstrument(h.ctx,*p.clap,ol,or_,frames,start,true,held,p.held,&tuned,0)
                :ClapSystemLogic::ProcessEffect(h.ctx,*p.clap,il,ir,ol,or_,frames,start,true);
            else ok=p.instrument()?AudioUnitSystemLogic::ProcessInstrument(h.ctx,*p.au,ol,or_,frames,start,true,held,p.held,&tuned,0)
                :AudioUnitSystemLogic::ProcessEffect(h.ctx,*p.au,il,ir,ol,or_,frames,start,true);
            if(ok||p.instrument()){std::swap(il,ol);std::swap(ir,or_);}
        }
        for(int i=0;i<frames;++i){left[i]=std::isfinite(il[i])?il[i]:0;right[i]=std::isfinite(ir[i])?ir[i]:0;}
        h.nextFrame[clockKey]=start+frames;h.continuousFrames[clockKey]+=frames;return true;
    }catch(...){std::fill_n(left,std::max(0,frames),0);std::fill_n(right,std::max(0,frames),0);return false;}
}


extern "C" bool ww_plugins_process_chain_timeline(void* p,const char* t,float* l,float* r,int n,double rate,int64_t start,double bpm,double quarters,int numerator,int denominator,const float* held,const char* chain,int discontinuity){
    return processChain(p,t,l,r,n,rate,start,bpm,quarters,numerator,denominator,held,chain,discontinuity,nullptr,0);
}
extern "C" bool ww_plugins_process_tuned_timeline(void* p,const char* t,float* l,float* r,int n,double rate,int64_t start,double bpm,double quarters,int numerator,int denominator,const float* held,int discontinuity,const double* tuned,int count){
    return processChain(p,t,l,r,n,rate,start,bpm,quarters,numerator,denominator,held,"playback",discontinuity,tuned,count);
}

extern "C" bool ww_plugins_process_bent_timeline(void* p,const char* t,float* l,float* r,int n,double rate,int64_t start,double bpm,double quarters,int numerator,int denominator,const float* held,int discontinuity,const double* tuned,int count,double bend){
    return processChain(p,t,l,r,n,rate,start,bpm,quarters,numerator,denominator,held,"playback",discontinuity,tuned,count,bend);
}

// Preserve both original and loop-aware playback ABIs. Capture owns a separate
// chain/clock and never resets or advances playback insert state.
extern "C" bool ww_plugins_process_timeline(void* opaque,const char* track,float* left,float* right,int frames,
    double rate,int64_t start,double bpm,double quarters,int numerator,int denominator,const float* held,int discontinuity){
    return ww_plugins_process_chain_timeline(opaque,track,left,right,frames,rate,start,bpm,quarters,numerator,denominator,held,"playback",discontinuity);
}
extern "C" bool ww_plugins_process(void* opaque,const char* track,float* left,float* right,int frames,
    double rate,int64_t start,double bpm,double quarters,int numerator,int denominator,const float* held){
    return ww_plugins_process_timeline(opaque,track,left,right,frames,rate,start,bpm,quarters,numerator,denominator,held,-1);
}

@interface WWPluginMenuTarget : NSObject
@property NSInteger selected;
-(void)choose:(NSMenuItem*)item;
@end
@implementation WWPluginMenuTarget
-(void)choose:(NSMenuItem*)item{self.selected=item.tag;}
@end
// OS popup only; all sockets and plugs remain actual Blender geometry.
extern "C" int ww_plugins_menu(const char* rows){@autoreleasepool{
    try{auto entries=Json::parse(rows);NSMenu* menu=[[NSMenu alloc] initWithTitle:@"Track plugins"];
        WWPluginMenuTarget* target=[WWPluginMenuTarget new];target.selected=-1;
        for(auto& entry:entries){NSMenuItem* item=[[NSMenuItem alloc] initWithTitle:[NSString stringWithUTF8String:entry.at("label").get<std::string>().c_str()] action:@selector(choose:) keyEquivalent:@""];
            item.target=target;item.tag=entry.at("value").get<int>();item.enabled=entry.value("enabled",true);[menu addItem:item];}
        menu.autoenablesItems=NO;[menu popUpMenuPositioningItem:nil atLocation:[NSEvent mouseLocation] inView:nil];return int(target.selected);
    }catch(...){return -1;}
}}

// Text entry for the existing audio-lane Rename action; Return accepts and
// Escape cancels without mutating the document. UI/control-thread only.
extern "C" const char* ww_plugins_text_entry(const char* request){@autoreleasepool{
    static thread_local std::string reply;
    try{
        auto q=Json::parse(request);NSAlert* alert=[NSAlert new];
        alert.messageText=[NSString stringWithUTF8String:q.value("title","RENAME LANE").c_str()];
        NSTextField* field=[[NSTextField alloc] initWithFrame:NSMakeRect(0,0,252,25)];
        field.stringValue=[NSString stringWithUTF8String:q.value("value","").c_str()];
        alert.accessoryView=field;[alert addButtonWithTitle:@"Save"];[alert addButtonWithTitle:@"Cancel"];
        [alert.window setInitialFirstResponder:field];
        if([alert runModal]==NSAlertFirstButtonReturn)reply=Json({{"name",std::string(field.stringValue.UTF8String?:"")}}).dump();
        else reply=Json({{"cancelled",true}}).dump();
    }catch(...){reply=Json({{"cancelled",true}}).dump();}
    return reply.c_str();
}}
