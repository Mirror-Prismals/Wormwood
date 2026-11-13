#pragma once
// Cardinal AutomationTrackSystem parameter enumeration. This runs on the
// existing control/render-ahead owner, never the device callback.
#include "Host/Vst3Host.h"
#include "pluginterfaces/base/ustring.h"
#include "json.hpp"
#include <algorithm>
#include <cctype>
#include <cmath>
#include <stdexcept>

namespace CardinalAutomation {
using Json=nlohmann::json;
inline std::string compact(const std::string& raw,size_t limit=10){
    std::string result;
    for(unsigned char byte:raw){
        char c=static_cast<char>(byte);
        if(c=='\n'||c=='\r'||c=='\t')c=' ';
        if(c==' '&&!result.empty()&&result.back()==' ')continue;
        result.push_back(static_cast<char>(std::toupper(static_cast<unsigned char>(c))));
        if(result.size()>=limit)break;
    }
    while(!result.empty()&&result.back()==' ')result.pop_back();
    return result.empty()?"NONE":result;
}
inline std::string title(const Steinberg::Vst::TChar* text,int size){
    char ascii[256]{};Steinberg::UString(const_cast<Steinberg::Vst::TChar*>(text),size).toAscii(ascii,sizeof(ascii));
    std::string result(ascii);
    while(!result.empty()&&std::isspace(static_cast<unsigned char>(result.front())))result.erase(result.begin());
    while(!result.empty()&&std::isspace(static_cast<unsigned char>(result.back())))result.pop_back();
    return result;
}
inline double finiteNormalized(double value){return std::isfinite(value)?std::clamp(value,0.,1.):0.;}
template<class Insert> Json parameters(Insert& insert){
    Json out=Json::array();if(insert.stock)return out;
    auto append=[&](uint32_t id,std::string label,double value,bool writable){
        if(out.size()>=4096)throw std::runtime_error("Automation parameter count exceeds 4096");
        out.push_back({{"id",id},{"label",compact(label)},{"value",finiteNormalized(value)},{"writable",writable}});
    };
    if(insert.au){
        for(size_t i=0;i<insert.au->parameterInfo.size();++i){const auto& info=insert.au->parameterInfo[i];
            if(info.writable)append(info.id,info.name.empty()?"P"+std::to_string(i+1):info.name,AudioUnitSystemLogic::GetNormalizedParam(*insert.au,info.id),true);
        }
    }else if(insert.clap){
        for(size_t i=0;i<insert.clap->parameterInfo.size();++i){const auto& info=insert.clap->parameterInfo[i];
            if(info.flags&(CLAP_PARAM_IS_HIDDEN|CLAP_PARAM_IS_READONLY))continue;
            append(info.id,info.name.empty()?"P"+std::to_string(i+1):info.name,ClapSystemLogic::GetNormalizedParam(*insert.clap,info.id),true);
        }
    }else if(insert.vst&&insert.vst->controller){
        const auto count=insert.vst->controller->getParameterCount();
        if(count>4096)throw std::runtime_error("Automation parameter count exceeds 4096");
        for(Steinberg::int32 i=0;i<count;++i){Steinberg::Vst::ParameterInfo info{};
            if(insert.vst->controller->getParameterInfo(i,info)!=Steinberg::kResultOk)continue;
            auto label=title(info.title,int(std::size(info.title)));
            if(label.empty())label=title(info.shortTitle,int(std::size(info.shortTitle)));
            if(label.empty())label="P"+std::to_string(i+1);
            append(info.id,label,insert.vst->controller->getParamNormalized(info.id),!(info.flags&Steinberg::Vst::ParameterInfo::kIsReadOnly));
        }
    }
    return out;
}
template<class Insert> void set(Insert& insert,uint32_t id,double value){
    if(!std::isfinite(value)||value<0.||value>1.)throw std::runtime_error("Parameter must be normalized 0..1");
    bool writable=false;
    if(insert.stock)throw std::runtime_error("Stock inserts are not Cardinal automation targets");
    if(insert.au){for(const auto& info:insert.au->parameterInfo)if(info.id==id){writable=info.writable;break;}}
    else if(insert.clap){for(const auto& info:insert.clap->parameterInfo)if(info.id==id){writable=!(info.flags&(CLAP_PARAM_IS_HIDDEN|CLAP_PARAM_IS_READONLY));break;}}
    else if(insert.vst&&insert.vst->controller){
        const auto count=insert.vst->controller->getParameterCount();
        if(count>4096)throw std::runtime_error("Automation parameter count exceeds 4096");
        for(Steinberg::int32 i=0;i<count;++i){Steinberg::Vst::ParameterInfo info{};
            if(insert.vst->controller->getParameterInfo(i,info)==Steinberg::kResultOk&&info.id==id){writable=!(info.flags&Steinberg::Vst::ParameterInfo::kIsReadOnly);break;}}
    }
    if(!writable)throw std::runtime_error("Automation parameter is unavailable or read-only");
    if(insert.vst){
        auto& p=*insert.vst;
        if(!p.handler)throw std::runtime_error("VST3 parameter handler is disconnected");
        if(p.controller->setParamNormalized(id,value)!=Steinberg::kResultOk||p.handler->performEdit(id,value)!=Steinberg::kResultOk)
            throw std::runtime_error("VST3 rejected automation parameter");
    }else if(insert.clap){
        if(!ClapSystemLogic::QueueNormalizedParam(*insert.clap,id,value))throw std::runtime_error("CLAP/VST2 parameter queue rejected automation");
    }else if(insert.au){
        if(!AudioUnitSystemLogic::SetNormalizedParam(*insert.au,id,value))throw std::runtime_error("Audio Unit rejected automation parameter");
    }
}
}
