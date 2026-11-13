// C ABI only. All operations belong to the non-real-time VM worker.
#include "chuck/chuck.h"
#include "chuck/chuck_vm.h"
#include <cstdint>
#include <cstring>
#include <memory>
#include <vector>
#include <exception>
struct MirrorChuck {ChucK vm;};
static void error(char* p,size_t n,const char* s) {if(p&&n){std::strncpy(p,s,n-1);p[n-1]=0;}}
extern "C" {
void* mirror_chuck_open(uint32_t rate,char* e,size_t n) noexcept {
    try {auto c=std::make_unique<MirrorChuck>();
        c->vm.setParam(CHUCK_PARAM_SAMPLE_RATE,static_cast<t_CKINT>(rate));
        c->vm.setParam(CHUCK_PARAM_INPUT_CHANNELS,static_cast<t_CKINT>(0));
        c->vm.setParam(CHUCK_PARAM_OUTPUT_CHANNELS,static_cast<t_CKINT>(18));
        c->vm.setParam(CHUCK_PARAM_VM_HALT,static_cast<t_CKINT>(FALSE));
        c->vm.setParam(CHUCK_PARAM_CHUGIN_ENABLE,static_cast<t_CKINT>(FALSE));
        c->vm.setParam(CHUCK_PARAM_IS_REALTIME_AUDIO_HINT,static_cast<t_CKINT>(TRUE));
        if(!c->vm.init()||!c->vm.start()){error(e,n,"ChucK VM initialization failed");return nullptr;}
        return c.release();
    }catch(const std::exception& x){error(e,n,x.what());}catch(...){error(e,n,"ChucK initialization exception");}return nullptr;
}
uint64_t mirror_chuck_compile(void* p,const char* path,const char* args,char* e,size_t n) noexcept {
    try {if(!p||!path||!args){error(e,n,"invalid ChucK compile arguments");return 0;}std::vector<t_CKUINT> ids;
        if(!static_cast<MirrorChuck*>(p)->vm.compileFile(path,args,1,FALSE,&ids)||ids.empty()){error(e,n,"ChucK script compilation failed");return 0;}return uint64_t(ids.front());
    }catch(const std::exception& x){error(e,n,x.what());}catch(...){error(e,n,"ChucK compilation exception");}return 0;
}
int mirror_chuck_run(void* p,float* output,uint32_t frames) noexcept {
    if(!p||!output||frames>256)return 0;
    try {static_cast<MirrorChuck*>(p)->vm.run(nullptr,output,frames);return 1;}catch(...){return 0;}
}
int mirror_chuck_remove(void* p,uint64_t id) noexcept {
    try {
        if(!p || !id || uint64_t(static_cast<t_CKUINT>(id)) != id)return 0;
        auto* vm=static_cast<MirrorChuck*>(p)->vm.vm();
        if(!vm->shreduler()->lookup(static_cast<t_CKUINT>(id)))return 1;
        // Scheduler removal alone leaks the shred and leaves its UGens linked.
        // The synchronous VM command frees it and its child shreds on this worker.
        auto* msg=new Chuck_Msg;
        msg->type=CK_MSG_REMOVE;
        msg->param=static_cast<t_CKUINT>(id);
        return vm->process_msg(msg) == static_cast<t_CKUINT>(id) ? 1 : 0;
    }catch(...){return 0;}
}
int mirror_chuck_active(void* p,uint64_t id) noexcept {
    try {return p && static_cast<MirrorChuck*>(p)->vm.vm()->shreduler()->lookup(static_cast<t_CKUINT>(id)) ? 1 : 0;}catch(...){return -1;}
}
void mirror_chuck_close(void* p) noexcept {try {delete static_cast<MirrorChuck*>(p);}catch(...) {}}
}
