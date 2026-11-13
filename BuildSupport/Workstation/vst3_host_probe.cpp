// Native, no-speaker-output compatibility probe. Does not open plugin editors.
#include "Host/Vst3Host.h"
#include "Bridge.h"
#include "json.hpp"
#include <array>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <stdexcept>
#include <thread>

using Json=nlohmann::json;
static void require(bool ok,const char* message){if(!ok)throw std::runtime_error(message);}
int main(int argc,char** argv) {
    try {
        require(argc==4,"usage: vst3_host_probe <bundle> <class name> <Wormwood root>");
        Vst3Context ctx;
        ctx.hostApp=Steinberg::owned(new Steinberg::Vst::HostApplication());
        Steinberg::Vst::PluginContextFactory::instance().setPluginContext(ctx.hostApp);
        std::string error;
        auto module=VST3::Hosting::Module::create(argv[1],error);
        require(bool(module),error.c_str());
        Vst3AvailablePlugin descriptor;
        bool found=false;
        for(const auto& info:module->getFactory().classInfos())if(info.name()==argv[2]&&info.category()==kVstAudioEffectClass){
            descriptor.module=module;descriptor.classInfo=info;descriptor.name=info.name();found=true;break;
        }
        require(found,"plugin class missing");
        auto* p=Vst3SystemLogic::Create(ctx,descriptor);
        require(p&&p->controller&&p->handler,"plugin initialization/handler failed");
        std::vector<uint32_t> parameters;
        for(int i=0;i<p->controller->getParameterCount();++i){Steinberg::Vst::ParameterInfo info{};
            if(p->controller->getParameterInfo(i,info)==Steinberg::kResultOk&&!(info.flags&Steinberg::Vst::ParameterInfo::kIsReadOnly))parameters.push_back(info.id);
        }
        require(!parameters.empty(),"no writable parameters");
        std::array<float,2048> left{},right{},outLeft{},outRight{};
        std::array<float,128> desired{},held{};
        auto process=[&](int n,int64_t frame){
            for(int i=0;i<n;++i){left[i]=.05f*std::sin((frame+i)*.0576);right[i]=.05f*std::cos((frame+i)*.0321);}
            const bool ok=Vst3SystemLogic::Process(ctx,*p,left.data(),right.data(),outLeft.data(),outRight.data(),n,frame,true,desired,held);
            if(ok)for(int i=0;i<n;++i)require(std::isfinite(outLeft[i])&&std::isfinite(outRight[i]),"nonfinite plugin output");
            return ok;
        };
        p->handler->restartComponent(Steinberg::Vst::kParamTitlesChanged);
        require(process(128,0),"metadata notification interrupted DSP");
        require(Vst3SystemLogic::Service(ctx,*p),"metadata service failed");
        // Send only through the host handler. Plugins with a private
        // controller-to-processor channel cannot hide a broken host queue here.
        const auto parameter=parameters.front();
        p->bypassed.store(true);
        for(int i=0;i<10000;++i)require(p->handler->performEdit(parameter,(i%101)/100.)==Steinberg::kResultOk,"editor publication dropped");
        require(p->handler->performEdit(parameter,.731)==Steinberg::kResultOk,"final publication dropped");
        p->bypassed.store(false);require(process(128,128),"burst processing failed");
        auto checkChange=[&](double expected){
            bool matched=false;
            for(int i=0;i<p->inputParameterChanges.getParameterCount();++i){auto* queue=p->inputParameterChanges.getParameterData(i);
                if(queue->getParameterId()!=parameter)continue;Steinberg::int32 offset=0;double v=0;
                require(queue->getPoint(queue->getPointCount()-1,offset,v)==Steinberg::kResultOk,"parameter point missing");
                matched=v==expected&&offset==0;
            }
            if(!matched)throw std::runtime_error("final editor value did not reach processor input (expected "+std::to_string(expected)+")");
        };
        checkChange(.731);
        const auto beforeAutomation=p->controller->getParamNormalized(parameter);
        require(Vst3SystemLogic::Automate(*p,parameter,.923),"preparation-thread automation rejected");
        require(p->controller->getParamNormalized(parameter)==beforeAutomation,"automation called controller synchronously");
        require(process(128,256),"automation processing failed");checkChange(.923);
        require(Vst3SystemLogic::Service(ctx,*p),"automation display refresh failed");
        p->controller->setParamNormalized(parameter,.321);
        const auto refreshed=p->controller->getParamNormalized(parameter);
        p->handler->restartComponent(Steinberg::Vst::kParamValuesChanged);
        require(!p->realtimeResetRequested.load(),"parameter refresh requested DSP reset");
        require(Vst3SystemLogic::Service(ctx,*p)&&process(128,256),"parameter refresh failed");checkChange(refreshed);
        p->handler->restartComponent(Steinberg::Vst::kIoChanged);
        require(!process(128,384),"processing used stale I/O configuration");
        require(Vst3SystemLogic::Service(ctx,*p)&&process(128,384),"I/O restart failed");

        // UI-thread edits against independent DSP processing, as in the DAW.
        std::atomic<bool> workerFailed{false};
        std::thread audio([&]{try{for(int i=0;i<3000;++i)require(process(128,512+128*i),"worker process failed");}catch(...){workerFailed.store(true);}});
        for(int i=0;i<10000;++i){const auto id=parameters[i%parameters.size()];const double value=(i%101)/100.;
            p->controller->setParamNormalized(id,value);
            require(p->handler->performEdit(id,value)==Steinberg::kResultOk,"concurrent edit rejected");
        }
        audio.join();require(!workerFailed.load(),"concurrent processing failed");
        Vst3SystemLogic::Destroy(ctx,p);

        // Exercise the public C bridge using its actual shared declaration.
        auto* host=ww_plugins_create();require(host,"bridge allocation failed");
        auto command=[&](Json q){auto result=Json::parse(ww_plugins_command(host,q.dump().c_str()));
            if(result.contains("error"))throw std::runtime_error(result.dump());return result;};
        auto status=command({{"op","scan"},{"root",argv[3]}});int index=-1;
        for(const auto& row:status["catalog"])if(row["name"]==argv[2]&&row["format"]=="VST3")index=row["index"];
        require(index>=0,"bridge catalog plugin missing");
        auto added=command({{"op","add"},{"index",index},{"track","probe"},{"track_kind","audio"},{"sample_rate",48000}});
        const auto id=added["inserts"][0]["id"];
        auto catalog=command({{"op","automation_catalog"}})["devices"][0]["parameters"];
        bool commandRejected=false;
        std::thread forbiddenControl([&]{const auto result=Json::parse(ww_plugins_command(host,"{\"op\":\"status\"}"));commandRejected=result.contains("error");});
        forbiddenControl.join();require(commandRejected,"native control API accepted a worker-thread call");
        int64_t frame=0;double maximumMs=0;
        for(int block=0;block<3000;++block){
            const uint32_t target=catalog[block%catalog.size()]["id"];
            if(block%2==0)command({{"op","parameter"},{"id",id},{"parameter",target},{"value",(block%101)/100.}});
            else require(ww_plugins_automate(host,id.get<uint64_t>(),target,(block%101)/100.),"bridge automation failed");
            const int sizes[]={1,64,128,256,2048};const int n=sizes[block%5];
            for(int i=0;i<n;++i){left[i]=.05f*std::sin((frame+i)*.0576);right[i]=.05f*std::cos((frame+i)*.0321);}
            const auto begin=std::chrono::steady_clock::now();
            require(ww_plugins_process(host,"probe",left.data(),right.data(),n,48000.,frame,120.,frame/24000.,4,4,desired.data()),"bridge process failed");
            maximumMs=std::max(maximumMs,std::chrono::duration<double,std::milli>(std::chrono::steady_clock::now()-begin).count());
            for(int i=0;i<n;++i)require(std::isfinite(left[i])&&std::isfinite(right[i]),"bridge nonfinite output");
            frame+=n;if(block%100==0)command({{"op","tick"}});
        }
        command({{"op","remove"},{"id",id}});ww_plugins_destroy(host);
        std::printf("PASS: metadata/parameter/I/O restart, 10000 burst edits, 10000 concurrent UI-thread edits with 3000 DSP blocks; bridge 3000 edits, %lld frames, max block %.3f ms. No editor or audible-output verification.\n",static_cast<long long>(frame),maximumMs);
    } catch(const std::exception& error){std::fprintf(stderr,"FAIL: %s\n",error.what());return 1;}
}
