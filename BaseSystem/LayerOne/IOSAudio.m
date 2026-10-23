// Apple API client implementation, private to IOSAudioConnectionSystem.
#import <AVFAudio/AVFAudio.h>
#import <AudioToolbox/AudioToolbox.h>
#import <UIKit/UIKit.h>
#include <stdbool.h>
#include <stdio.h>
typedef void (*WwRender)(void *,uint32_t,float *,float *,double,bool);
typedef void (*WwInterrupt)(void *);
typedef struct {WwRender render;void *data;} WwRenderContext;
@interface WwAudio : NSObject
@property(nonatomic,assign) AudioUnit unit;
@property(nonatomic,assign) WwRender render;
@property(nonatomic,assign) WwInterrupt interrupt;
@property(nonatomic,assign) void *data;
@property(nonatomic,assign) void *notificationData;
@property(nonatomic,strong) NSMutableArray *observers;
@end
@implementation WwAudio
@end
static int fail(char *error,size_t capacity,const char *operation,OSStatus status) {
    snprintf(error,capacity,"%s failed (%d)",operation,(int)status);return -1;
}
static void mainSync(dispatch_block_t block) { if(NSThread.isMainThread) block();else dispatch_sync(dispatch_get_main_queue(),block); }
static OSStatus renderAudio(void *ref,AudioUnitRenderActionFlags *flags,const AudioTimeStamp *time,UInt32 bus,UInt32 frames,AudioBufferList *buffers) {
    // No Objective-C messaging, allocation, logging, locks, or dispatch here.
    // The raw context is separate from the Objective-C connection object.
    WwRenderContext *context=ref;
    if(!buffers || buffers->mNumberBuffers!=2) return kAudio_ParamError;
    for(unsigned i=0;i<2;i++) if(buffers->mBuffers[i].mNumberChannels!=1 || buffers->mBuffers[i].mDataByteSize<frames*sizeof(float)) return kAudio_ParamError;
    context->render(context->data,frames,buffers->mBuffers[0].mData,buffers->mBuffers[1].mData,time->mSampleTime,(time->mFlags&kAudioTimeStampSampleTimeValid)!=0);
    return noErr;
}
// A stable native callback context: allocated during preparation and destroyed
// only after AudioComponentInstanceDispose has stopped using the callback.
@interface WwAudioContext : WwAudio
@property(nonatomic,assign) void *context;
@end
@implementation WwAudioContext
@end
void *ww_audio_open(WwRender render,WwInterrupt interrupt,void *data,double *rate,uint32_t *frames,char *error,size_t capacity) {
    @autoreleasepool {
        AVAudioSession *session=AVAudioSession.sharedInstance;NSError *failure=nil;
        if(![session setCategory:AVAudioSessionCategoryPlayback mode:AVAudioSessionModeDefault options:0 error:&failure] || ![session setPreferredSampleRate:48000 error:&failure] || ![session setPreferredIOBufferDuration:256.0/48000 error:&failure] || ![session setActive:YES error:&failure]) {
            snprintf(error,capacity,"AVAudioSession: %s",failure.localizedDescription.UTF8String);return NULL;
        }
        *rate=session.sampleRate;*frames=(uint32_t)llround(session.IOBufferDuration * *rate);
        WwAudioContext *state=[WwAudioContext new];state.render=render;state.interrupt=interrupt;state.data=data;
        AudioComponentDescription desc={.componentType=kAudioUnitType_Output,.componentSubType=kAudioUnitSubType_RemoteIO,.componentManufacturer=kAudioUnitManufacturer_Apple};
        AudioComponent component=AudioComponentFindNext(NULL,&desc);AudioUnit unit=NULL;
        OSStatus status=component?AudioComponentInstanceNew(component,&unit):kAudio_ParamError;
        if(status){fail(error,capacity,"RemoteIO creation",status);[session setActive:NO error:nil];return NULL;}
        state.unit=unit;
        WwRenderContext *context=calloc(1,sizeof(*context));
        if(!context){AudioComponentInstanceDispose(unit);[session setActive:NO error:nil];snprintf(error,capacity,"audio context allocation failed");return NULL;}
        context->render=render;context->data=data;state.context=context;
        AudioStreamBasicDescription format={.mSampleRate=*rate,.mFormatID=kAudioFormatLinearPCM,.mFormatFlags=kAudioFormatFlagIsFloat|kAudioFormatFlagIsPacked|kAudioFormatFlagIsNonInterleaved,.mBytesPerPacket=4,.mFramesPerPacket=1,.mBytesPerFrame=4,.mChannelsPerFrame=2,.mBitsPerChannel=32};
        AURenderCallbackStruct callback={.inputProc=renderAudio,.inputProcRefCon=context};
        status=AudioUnitSetProperty(unit,kAudioUnitProperty_StreamFormat,kAudioUnitScope_Input,0,&format,sizeof(format));
        if(!status)status=AudioUnitSetProperty(unit,kAudioUnitProperty_SetRenderCallback,kAudioUnitScope_Input,0,&callback,sizeof(callback));
        if(!status)status=AudioUnitInitialize(unit);
        if(status){AudioComponentInstanceDispose(unit);free(context);[session setActive:NO error:nil];fail(error,capacity,"RemoteIO preparation",status);return NULL;}
        return (__bridge_retained void *)state;
    }
}
int ww_audio_activate(void *handle,char *error,size_t capacity) {
    WwAudioContext *state=(__bridge WwAudioContext *)handle;
    mainSync(^{
        state.notificationData=state.data;state.observers=[NSMutableArray new];
        __weak WwAudioContext *weakState=state;
        for(NSString *name in @[AVAudioSessionInterruptionNotification,AVAudioSessionRouteChangeNotification,AVAudioSessionMediaServicesWereResetNotification,UIApplicationWillResignActiveNotification]) {
            id token=[NSNotificationCenter.defaultCenter addObserverForName:name object:nil queue:NSOperationQueue.mainQueue usingBlock:^(NSNotification *note){
                // Session category changes made during our own preparation can
                // be delivered after activation; they do not invalidate the route.
                if([note.name isEqualToString:AVAudioSessionRouteChangeNotification] && [note.userInfo[AVAudioSessionRouteChangeReasonKey] unsignedIntegerValue]==AVAudioSessionRouteChangeReasonCategoryChange)return;
                WwAudioContext *strong=weakState;
                if(strong.notificationData) strong.interrupt(strong.notificationData);
            }];
            [state.observers addObject:token];
        }
    });
    OSStatus status=AudioOutputUnitStart(state.unit);return status?fail(error,capacity,"RemoteIO start",status):0;
}
int ww_audio_close(void *handle,char *error,size_t capacity) {
    WwAudioContext *state=(__bridge WwAudioContext *)handle;
    if(state.unit) {
        OSStatus status=AudioOutputUnitStop(state.unit);if(status)return fail(error,capacity,"RemoteIO stop",status);
        status=AudioUnitUninitialize(state.unit);if(status)return fail(error,capacity,"RemoteIO uninitialize",status);
        status=AudioComponentInstanceDispose(state.unit);if(status)return fail(error,capacity,"RemoteIO dispose",status);
        state.unit=NULL;free(state.context);state.context=NULL;
    }
    mainSync(^{state.notificationData=NULL;for(id token in state.observers)[NSNotificationCenter.defaultCenter removeObserver:token];state.observers=nil;});
    NSError *failure=nil;
    if(![AVAudioSession.sharedInstance setActive:NO withOptions:AVAudioSessionSetActiveOptionNotifyOthersOnDeactivation error:&failure]) {
        snprintf(error,capacity,"Audio session deactivation: %s",failure.localizedDescription.UTF8String);return -1;
    }
    CFRelease(handle);return 0;
}
