// Native iOS graphics client, private to GraphicsConnectionSystem.
#import <UIKit/UIKit.h>
#import <QuartzCore/CAEAGLLayer.h>
#import <OpenGLES/ES3/gl.h>
#include <stdio.h>
#include <string.h>
static __weak UIViewController *hostController;
static BOOL hostActive=YES;
extern void ww_ios_bring_controls(void);
extern bool ww_ios_stop_requested(void);
void ww_ios_attach(void *controller) { hostController=(__bridge UIViewController *)controller; }
void ww_ios_active(bool active) {hostActive=active;}
static void onMain(dispatch_block_t block) {if(NSThread.isMainThread)block();else dispatch_sync(dispatch_get_main_queue(),block);}
@interface WwGLView : UIView
@end
@implementation WwGLView
+ (Class)layerClass {return CAEAGLLayer.class;}
@end
@interface WwGL : NSObject { float _modelData[32]; }
@property(nonatomic,strong) WwGLView *view;
@property(nonatomic,strong) EAGLContext *context;
@property(nonatomic,assign) GLuint framebuffer,renderbuffer,program,vbo,vao,playerTexture,woodlandTexture,cloudTexture;
@property(nonatomic,strong) NSData *cloudFrame;
@property(nonatomic,assign) GLint width,height;
@property(nonatomic,assign) BOOL captured;
@property(nonatomic,assign) GLuint depth,modelProgram,modelVao,modelVbo;
@property(nonatomic,assign) uint32_t vertexCount,bones,shell;
- (float *)modelFrame;
@end
@implementation WwGL
- (float *)modelFrame{return _modelData;}
@end
typedef struct {uint32_t width,height;uint8_t center[4],corner[4];char renderer[256];bool readback;} WwGraphicsInfo;
static GLuint compile(GLenum type,const char *source,char *error,size_t capacity) {
    GLuint shader=glCreateShader(type);glShaderSource(shader,1,&source,NULL);glCompileShader(shader);
    GLint success=0;glGetShaderiv(shader,GL_COMPILE_STATUS,&success);
    if(!success){glGetShaderInfoLog(shader,(GLsizei)capacity,NULL,error);glDeleteShader(shader);return 0;}return shader;
}
static void destroy(WwGL *state) {
    [EAGLContext setCurrentContext:state.context];glFinish();
    if(state.cloudTexture)glDeleteTextures(1,&(GLuint){state.cloudTexture});
    if(state.woodlandTexture)glDeleteTextures(1,&(GLuint){state.woodlandTexture});
    if(state.playerTexture)glDeleteTextures(1,&(GLuint){state.playerTexture});
    if(state.depth)glDeleteRenderbuffers(1,&(GLuint){state.depth});
    if(state.modelProgram)glDeleteProgram(state.modelProgram);
    if(state.modelVbo)glDeleteBuffers(1,&(GLuint){state.modelVbo});
    if(state.modelVao)glDeleteVertexArrays(1,&(GLuint){state.modelVao});
    if(state.vbo)glDeleteBuffers(1,&(GLuint){state.vbo});
    if(state.vao)glDeleteVertexArrays(1,&(GLuint){state.vao});
    if(state.program)glDeleteProgram(state.program);
    if(state.framebuffer)glDeleteFramebuffers(1,&(GLuint){state.framebuffer});
    if(state.renderbuffer)glDeleteRenderbuffers(1,&(GLuint){state.renderbuffer});
    [state.view removeFromSuperview];[EAGLContext setCurrentContext:nil];
}
void *ww_graphics_open(const float *vertices,const char *vertex,const char *fragment,WwGraphicsInfo *info,char *error,size_t capacity) {
    __block WwGL *state=nil;
    onMain(^{@autoreleasepool {
        if(!hostController || !hostActive){snprintf(error,capacity,"iOS surface is inactive");return;}
        WwGL *s=[WwGL new];s.context=[[EAGLContext alloc]initWithAPI:kEAGLRenderingAPIOpenGLES3];
        if(!s.context || ![EAGLContext setCurrentContext:s.context]){snprintf(error,capacity,"OpenGL ES 3 context unavailable");return;}
        s.view=[[WwGLView alloc]initWithFrame:hostController.view.bounds];s.view.contentScaleFactor=hostController.view.window.screen.scale;s.view.autoresizingMask=UIViewAutoresizingFlexibleWidth|UIViewAutoresizingFlexibleHeight;s.view.userInteractionEnabled=NO;
        s.view.hidden=YES;s.view.autoresizingMask=UIViewAutoresizingFlexibleWidth|UIViewAutoresizingFlexibleHeight;
        CAEAGLLayer *layer=(CAEAGLLayer *)s.view.layer;layer.opaque=YES;
        layer.drawableProperties=@{kEAGLDrawablePropertyRetainedBacking:@NO,kEAGLDrawablePropertyColorFormat:kEAGLColorFormatRGBA8};
        [hostController.view addSubview:s.view];
        GLuint rb=0,fb=0;glGenRenderbuffers(1,&rb);s.renderbuffer=rb;glBindRenderbuffer(GL_RENDERBUFFER,rb);
        if(![s.context renderbufferStorage:GL_RENDERBUFFER fromDrawable:layer]){snprintf(error,capacity,"iOS drawable allocation failed");destroy(s);return;}
        GLint width=0,height=0;glGetRenderbufferParameteriv(GL_RENDERBUFFER,GL_RENDERBUFFER_WIDTH,&width);glGetRenderbufferParameteriv(GL_RENDERBUFFER,GL_RENDERBUFFER_HEIGHT,&height);s.width=width;s.height=height;
        glGenFramebuffers(1,&fb);s.framebuffer=fb;glBindFramebuffer(GL_FRAMEBUFFER,fb);glFramebufferRenderbuffer(GL_FRAMEBUFFER,GL_COLOR_ATTACHMENT0,GL_RENDERBUFFER,rb);
        if(width<=0 || height<=0 || glCheckFramebufferStatus(GL_FRAMEBUFFER)!=GL_FRAMEBUFFER_COMPLETE){snprintf(error,capacity,"iOS framebuffer incomplete");destroy(s);return;}
        GLuint vs=compile(GL_VERTEX_SHADER,vertex,error,capacity);if(!vs){destroy(s);return;}
        GLuint fs=compile(GL_FRAGMENT_SHADER,fragment,error,capacity);if(!fs){glDeleteShader(vs);destroy(s);return;}
        s.program=glCreateProgram();glAttachShader(s.program,vs);glAttachShader(s.program,fs);glLinkProgram(s.program);glDeleteShader(vs);glDeleteShader(fs);
        GLint success=0;glGetProgramiv(s.program,GL_LINK_STATUS,&success);
        if(!success){glGetProgramInfoLog(s.program,(GLsizei)capacity,NULL,error);destroy(s);return;}
        GLuint vao=0,vbo=0;glGenVertexArrays(1,&vao);s.vao=vao;glBindVertexArray(vao);glGenBuffers(1,&vbo);s.vbo=vbo;glBindBuffer(GL_ARRAY_BUFFER,vbo);glBufferData(GL_ARRAY_BUFFER,18*sizeof(float),vertices,GL_STATIC_DRAW);
        glEnableVertexAttribArray(0);glVertexAttribPointer(0,3,GL_FLOAT,GL_FALSE,6*sizeof(float),(const void *)0);
        glEnableVertexAttribArray(1);glVertexAttribPointer(1,3,GL_FLOAT,GL_FALSE,6*sizeof(float),(const void *)(3*sizeof(float)));
        if(glGetError()!=GL_NO_ERROR){snprintf(error,capacity,"OpenGL ES preparation error");destroy(s);return;}
        info->width=width;info->height=height;info->readback=false;snprintf(info->renderer,sizeof(info->renderer),"%s",glGetString(GL_RENDERER));state=s;
    }});
    return state?(__bridge_retained void *)state:NULL;
}
int ww_graphics_frame(void *handle,WwGraphicsInfo *info,char *error,size_t capacity) {
    __block int result=-1;
    onMain(^{@autoreleasepool {
        WwGL *s=(__bridge WwGL *)handle;
        if(!hostActive || !s.view.window){snprintf(error,capacity,"iOS presentation interrupted (%s)",!hostActive?"scene inactive":"surface detached");return;}
        if(![EAGLContext setCurrentContext:s.context]){snprintf(error,capacity,"iOS graphics context lost");return;}
        s.view.hidden=NO;glBindFramebuffer(GL_FRAMEBUFFER,s.framebuffer);glBindRenderbuffer(GL_RENDERBUFFER,s.renderbuffer);
        glViewport(0,0,s.width,s.height);glClearColor(.035,.045,.065,1);glClear(GL_COLOR_BUFFER_BIT);glUseProgram(s.program);glBindVertexArray(s.vao);glDrawArrays(GL_TRIANGLES,0,3);
        if(!s.captured){
            glReadPixels(s.width/2,s.height/2,1,1,GL_RGBA,GL_UNSIGNED_BYTE,info->center);glReadPixels(0,0,1,1,GL_RGBA,GL_UNSIGNED_BYTE,info->corner);
            const unsigned char center[]={101,120,166},corner[]={9,11,17};
            for(unsigned i=0;i<3;i++)if(abs((int)info->center[i]-center[i])>6 || abs((int)info->corner[i]-corner[i])>3){snprintf(error,capacity,"OpenGL ES pixel readback mismatch");return;}
            info->readback=true;s.captured=YES;
        }
        glFinish();if(glGetError()!=GL_NO_ERROR){snprintf(error,capacity,"OpenGL ES frame error");return;}
        if(![s.context presentRenderbuffer:GL_RENDERBUFFER]){snprintf(error,capacity,"iOS renderbuffer presentation failed");return;}result=0;
    }});return result;
}
int ww_graphics_close(void *handle,char *error,size_t capacity) {
    __block int result=-1;
    onMain(^{WwGL *s=(__bridge WwGL *)handle;
        if(![EAGLContext setCurrentContext:s.context]){snprintf(error,capacity,"cannot release iOS context");return;}
        destroy(s);CFRelease(handle);result=0;
    });return result;
}

#import <Metal/Metal.h>
#import <QuartzCore/CAMetalLayer.h>
@interface WwMetalView : UIView
@end
@implementation WwMetalView
+ (Class)layerClass{return CAMetalLayer.class;}
@end
void *ww_metal_open(bool workstation) {
    __block WwMetalView *view=nil;
    onMain(^{
        if(!hostActive || !hostController)return;
        view=[[WwMetalView alloc]initWithFrame:hostController.view.bounds];view.hidden=YES;
        // Workstation optics render one pixel per logical point, at the same
        // UIKit point size. Each new game surface keeps the device screen scale.
        view.contentScaleFactor=workstation?1.0:hostController.view.window.screen.scale;view.autoresizingMask=UIViewAutoresizingFlexibleWidth|UIViewAutoresizingFlexibleHeight;view.userInteractionEnabled=NO;
        CAMetalLayer *layer=(CAMetalLayer *)view.layer;layer.device=MTLCreateSystemDefaultDevice();layer.pixelFormat=MTLPixelFormatBGRA8Unorm;layer.framebufferOnly=NO;
        layer.contentsScale=view.contentScaleFactor;
        layer.drawableSize=CGSizeMake(view.bounds.size.width*view.contentScaleFactor,view.bounds.size.height*view.contentScaleFactor);
        [hostController.view addSubview:view];
    });return view?(__bridge_retained void *)view:NULL;
}
void *ww_metal_layer(void *handle){__block void *layer=NULL;onMain(^{layer=(__bridge void *)((__bridge WwMetalView *)handle).layer;});return layer;}
void ww_logical_dimensions(void *handle,double *width,double *height){onMain(^{UIView *v=(__bridge UIView *)handle;*width=v.bounds.size.width;*height=v.bounds.size.height;});}
void ww_metal_dimensions(void *handle,uint32_t *width,uint32_t *height){onMain(^{UIView *v=(__bridge UIView *)handle;CGSize size=CGSizeMake(v.bounds.size.width*v.contentScaleFactor,v.bounds.size.height*v.contentScaleFactor);*width=size.width;*height=size.height;});}
void ww_metal_show(void *handle){onMain(^{((__bridge WwMetalView *)handle).hidden=NO;ww_ios_bring_controls();});}
bool ww_metal_closed(void *handle){__block BOOL closed=YES;onMain(^{closed=ww_ios_stop_requested() || !((__bridge WwMetalView *)handle).window;});return closed;}
void ww_metal_close(void *handle){onMain(^{[((__bridge WwMetalView *)handle) removeFromSuperview];CFRelease(handle);});}

// Interactive scene path. Resources remain owned by GraphicsConnectionSystem.
void *ww_scene_open(const char *vertex,const char *fragment,char *error,size_t capacity,bool workstation) {
 __block WwGL *state=nil;
 onMain(^{@autoreleasepool{
  if(!hostController || !hostActive){snprintf(error,capacity,"iOS surface inactive");return;}
  WwGL *s=[WwGL new];s.context=[[EAGLContext alloc]initWithAPI:kEAGLRenderingAPIOpenGLES3];
  if(!s.context || ![EAGLContext setCurrentContext:s.context]){snprintf(error,capacity,"EAGL unavailable");return;}
  // Set before drawable/depth allocation so GLES reports the same one-pixel-per-
  // logical-point dimensions as Metal. Point geometry and shaders are unchanged.
  s.view=[[WwGLView alloc]initWithFrame:hostController.view.bounds];s.view.contentScaleFactor=workstation?1.0:hostController.view.window.screen.scale;s.view.autoresizingMask=UIViewAutoresizingFlexibleWidth|UIViewAutoresizingFlexibleHeight;s.view.userInteractionEnabled=NO;
  CAEAGLLayer *layer=(CAEAGLLayer *)s.view.layer;layer.contentsScale=s.view.contentScaleFactor;layer.opaque=YES;layer.drawableProperties=@{kEAGLDrawablePropertyRetainedBacking:@NO,kEAGLDrawablePropertyColorFormat:kEAGLColorFormatRGBA8};[hostController.view addSubview:s.view];
  GLuint rb,fb,depth;glGenRenderbuffers(1,&rb);s.renderbuffer=rb;glBindRenderbuffer(GL_RENDERBUFFER,rb);
  if(![s.context renderbufferStorage:GL_RENDERBUFFER fromDrawable:layer]){snprintf(error,capacity,"EAGL drawable unavailable");destroy(s);return;}
  GLint w,h;glGetRenderbufferParameteriv(GL_RENDERBUFFER,GL_RENDERBUFFER_WIDTH,&w);glGetRenderbufferParameteriv(GL_RENDERBUFFER,GL_RENDERBUFFER_HEIGHT,&h);s.width=w;s.height=h;
  glGenFramebuffers(1,&fb);s.framebuffer=fb;glBindFramebuffer(GL_FRAMEBUFFER,fb);glFramebufferRenderbuffer(GL_FRAMEBUFFER,GL_COLOR_ATTACHMENT0,GL_RENDERBUFFER,rb);
  glGenRenderbuffers(1,&depth);s.depth=depth;glBindRenderbuffer(GL_RENDERBUFFER,depth);glRenderbufferStorage(GL_RENDERBUFFER,GL_DEPTH_COMPONENT24,w,h);glFramebufferRenderbuffer(GL_FRAMEBUFFER,GL_DEPTH_ATTACHMENT,GL_RENDERBUFFER,depth);
  if(glCheckFramebufferStatus(GL_FRAMEBUFFER)!=GL_FRAMEBUFFER_COMPLETE){snprintf(error,capacity,"EAGL framebuffer incomplete");destroy(s);return;}
  GLuint vs=compile(GL_VERTEX_SHADER,vertex,error,capacity),fs=compile(GL_FRAGMENT_SHADER,fragment,error,capacity);
  if(!vs||!fs){if(vs)glDeleteShader(vs);if(fs)glDeleteShader(fs);destroy(s);return;}
  s.program=glCreateProgram();glAttachShader(s.program,vs);glAttachShader(s.program,fs);glLinkProgram(s.program);glDeleteShader(vs);glDeleteShader(fs);GLint ok;glGetProgramiv(s.program,GL_LINK_STATUS,&ok);
  if(!ok){glGetProgramInfoLog(s.program,(GLsizei)capacity,NULL,error);destroy(s);return;}
  GLuint vao,vbo;glGenVertexArrays(1,&vao);s.vao=vao;glBindVertexArray(vao);glGenBuffers(1,&vbo);s.vbo=vbo;glBindBuffer(GL_ARRAY_BUFFER,vbo);
  glEnableVertexAttribArray(0);glVertexAttribPointer(0,4,GL_FLOAT,GL_FALSE,32,0);glEnableVertexAttribArray(1);glVertexAttribPointer(1,4,GL_FLOAT,GL_FALSE,32,(void *)16);state=s;
 }});return state?(__bridge_retained void *)state:NULL;
}
void *ww_scene_view(void *handle){__block void *view=NULL;onMain(^{view=(__bridge_retained void *)((__bridge WwGL *)handle).view;});return view;}
void ww_scene_cloud_noise(void *handle,uint32_t width,uint32_t height,const uint8_t *pixels){onMain(^{WwGL *s=(__bridge WwGL *)handle;[EAGLContext setCurrentContext:s.context];if(!s.cloudTexture){GLuint t;glGenTextures(1,&t);s.cloudTexture=t;}glActiveTexture(GL_TEXTURE6);glBindTexture(GL_TEXTURE_2D,s.cloudTexture);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_NEAREST);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_NEAREST);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_S,GL_CLAMP_TO_EDGE);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_T,GL_CLAMP_TO_EDGE);glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA8,width,height,0,GL_RGBA,GL_UNSIGNED_BYTE,pixels);glActiveTexture(GL_TEXTURE0);});}
void ww_scene_cloud_frame(void *handle,const float *frame){NSData *copy=[NSData dataWithBytes:frame length:192];onMain(^{WwGL *s=(__bridge WwGL *)handle;s.cloudFrame=copy;});}
void ww_scene_woodland(void *handle,uint32_t width,uint32_t height,const uint8_t *pixels){onMain(^{WwGL *s=(__bridge WwGL *)handle;[EAGLContext setCurrentContext:s.context];if(!s.woodlandTexture){GLuint t;glGenTextures(1,&t);s.woodlandTexture=t;}glActiveTexture(GL_TEXTURE5);glBindTexture(GL_TEXTURE_2D,s.woodlandTexture);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_NEAREST);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_NEAREST);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_S,GL_CLAMP_TO_EDGE);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_T,GL_CLAMP_TO_EDGE);glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA8,width,height,0,GL_RGBA,GL_UNSIGNED_BYTE,pixels);glActiveTexture(GL_TEXTURE0);});}
static void bindWoodland(WwGL *s){glUniform1i(glGetUniformLocation(s.program,"cloudNoiseAtlas"),6);glActiveTexture(GL_TEXTURE6);glBindTexture(GL_TEXTURE_2D,s.cloudTexture);glBindSampler(6,0);if(s.cloudFrame.length==192)glUniform4fv(glGetUniformLocation(s.program,"cloudFrame"),12,(const GLfloat *)s.cloudFrame.bytes);glUniform1i(glGetUniformLocation(s.program,"woodlandAtlas"),5);glActiveTexture(GL_TEXTURE5);glBindTexture(GL_TEXTURE_2D,s.woodlandTexture);glBindSampler(5,0);glActiveTexture(GL_TEXTURE0);}
void ww_scene_texture(void *handle,uint32_t width,uint32_t height,const uint8_t *pixels){onMain(^{WwGL *s=(__bridge WwGL *)handle;[EAGLContext setCurrentContext:s.context];if(!s.playerTexture){GLuint t;glGenTextures(1,&t);s.playerTexture=t;}glBindTexture(GL_TEXTURE_2D,s.playerTexture);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_LINEAR);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_LINEAR);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_S,GL_CLAMP_TO_EDGE);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_T,GL_CLAMP_TO_EDGE);glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA8,width,height,0,GL_RGBA,GL_UNSIGNED_BYTE,pixels);});}
// RGBA32F optical constants plus the authored Blender display LUT. The frame
// updates only the first row; static color-transform data stays on the GPU.
void ww_scene_workstation(void *handle,const float *rows,uint32_t count){onMain(^{
 WwGL *s=(__bridge WwGL *)handle;[EAGLContext setCurrentContext:s.context];
 if(!s.playerTexture){GLuint t;glGenTextures(1,&t);s.playerTexture=t;}
 glActiveTexture(GL_TEXTURE0);glBindTexture(GL_TEXTURE_2D,s.playerTexture);
 glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_NEAREST);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_NEAREST);
 glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_S,GL_CLAMP_TO_EDGE);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_T,GL_CLAMP_TO_EDGE);
 if(count==49408)glTexSubImage2D(GL_TEXTURE_2D,0,0,0,256,count/256,GL_RGBA,GL_FLOAT,rows);
 else glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA32F,256,count/256,0,GL_RGBA,GL_FLOAT,rows);
 glUseProgram(s.program);glUniform1i(glGetUniformLocation(s.program,"workstationData"),0);
});}
void ww_scene_batch(void *handle,const float *vertices,uint32_t count){onMain(^{WwGL *s=(__bridge WwGL *)handle;[EAGLContext setCurrentContext:s.context];glBindBuffer(GL_ARRAY_BUFFER,s.vbo);glBufferData(GL_ARRAY_BUFFER,(size_t)count*32,vertices,GL_DYNAMIC_DRAW);s.vertexCount=count;});}
int ww_scene_model(void *handle,const float *vertices,uint32_t bones,uint32_t shell,const char *vertex,const char *fragment,char *error,size_t capacity){
 __block int result=-1;onMain(^{WwGL *s=(__bridge WwGL *)handle;[EAGLContext setCurrentContext:s.context];
 if(!s.modelProgram){GLuint vs=compile(GL_VERTEX_SHADER,vertex,error,capacity),fs=compile(GL_FRAGMENT_SHADER,fragment,error,capacity);if(!vs||!fs){if(vs)glDeleteShader(vs);if(fs)glDeleteShader(fs);return;}GLuint p=glCreateProgram();glAttachShader(p,vs);glAttachShader(p,fs);glLinkProgram(p);glDeleteShader(vs);glDeleteShader(fs);GLint ok;glGetProgramiv(p,GL_LINK_STATUS,&ok);if(!ok){glGetProgramInfoLog(p,(GLsizei)capacity,NULL,error);glDeleteProgram(p);return;}s.modelProgram=p;
 GLuint vao,vbo;glGenVertexArrays(1,&vao);s.modelVao=vao;glBindVertexArray(vao);glGenBuffers(1,&vbo);s.modelVbo=vbo;glBindBuffer(GL_ARRAY_BUFFER,vbo);glBufferData(GL_ARRAY_BUFFER,((size_t)bones+shell)*28,vertices,GL_STATIC_DRAW);
 for(int i=0;i<3;i++){glEnableVertexAttribArray(i);glVertexAttribPointer(i,i==2?1:3,GL_FLOAT,GL_FALSE,28,(void *)(uintptr_t)(i==0?0:i==1?12:24));}s.bones=bones;s.shell=shell;
 }result=0;});return result;
}
void ww_scene_model_frame(void *handle,const float *frame){onMain(^{WwGL *s=(__bridge WwGL *)handle;memcpy(s.modelFrame,frame,128);});}
void ww_scene_opaque(void *handle){WwGL *s=(__bridge WwGL *)handle;glEnable(GL_DEPTH_TEST);glDepthFunc(GL_LEQUAL);glDepthMask(GL_TRUE);glEnable(GL_BLEND);glBlendFunc(GL_SRC_ALPHA,GL_ONE_MINUS_SRC_ALPHA);glDisable(GL_CULL_FACE);glUseProgram(s.program);bindWoodland(s);glActiveTexture(GL_TEXTURE0);glBindTexture(GL_TEXTURE_2D,s.playerTexture);glBindVertexArray(s.vao);glDrawArrays(GL_TRIANGLES,0,s.vertexCount);}
int ww_scene_with_context(void *handle,int (*callback)(void *,uint32_t,uint32_t,uint32_t),void *context){__block int result=-1;onMain(^{WwGL *s=(__bridge WwGL *)handle;[EAGLContext setCurrentContext:s.context];result=callback(context,s.framebuffer,s.width,s.height);});return result;}
int ww_scene_draw(void *handle,const float *clear,int (*begin)(void *,uint32_t,uint32_t,uint32_t),int (*overlay)(void *,uint32_t,uint32_t,uint32_t),void *context,bool validate,uint8_t *center,uint8_t *corner,char *error,size_t capacity){
 __block int result=-1;onMain(^{@autoreleasepool{WwGL *s=(__bridge WwGL *)handle;if(!hostActive||!s.view.window){snprintf(error,capacity,"iOS scene inactive");return;}[EAGLContext setCurrentContext:s.context];s.view.hidden=NO;glBindFramebuffer(GL_FRAMEBUFFER,s.framebuffer);glViewport(0,0,s.width,s.height);if(begin&&begin(context,s.framebuffer,s.width,s.height)!=0){snprintf(error,capacity,"scene setup failed");return;}glClearColor(clear[0],clear[1],clear[2],clear[3]);glClear(GL_COLOR_BUFFER_BIT|GL_DEPTH_BUFFER_BIT);glEnable(GL_DEPTH_TEST);glDepthFunc(GL_LEQUAL);glDepthMask(GL_TRUE);glEnable(GL_BLEND);glBlendFunc(GL_SRC_ALPHA,GL_ONE_MINUS_SRC_ALPHA);glDisable(GL_CULL_FACE);glUseProgram(s.program);bindWoodland(s);glActiveTexture(GL_TEXTURE0);glBindTexture(GL_TEXTURE_2D,s.playerTexture);glBindVertexArray(s.vao);glDrawArrays(GL_TRIANGLES,0,s.vertexCount);
 if(s.modelProgram){s.modelFrame[1]=(float)s.width/s.height;glUseProgram(s.modelProgram);glBindVertexArray(s.modelVao);glUniform4fv(glGetUniformLocation(s.modelProgram,"frame[0]"),8,s.modelFrame);glDrawArrays(GL_TRIANGLES,0,s.bones);glEnable(GL_BLEND);glBlendFuncSeparate(GL_SRC_ALPHA,GL_ONE,GL_ONE,GL_ZERO);glDepthMask(GL_FALSE);glDrawArrays(GL_TRIANGLES,s.bones,s.shell);glDepthMask(GL_TRUE);glDisable(GL_BLEND);}
 if(overlay&&overlay(context,s.framebuffer,s.width,s.height)!=0){snprintf(error,capacity,"scene overlay failed");return;}
 if(validate){glReadPixels(s.width/2,s.height/2,1,1,GL_RGBA,GL_UNSIGNED_BYTE,center);glReadPixels(0,0,1,1,GL_RGBA,GL_UNSIGNED_BYTE,corner);if(center[0]<25&&center[1]<25&&center[2]<25){snprintf(error,capacity,"EAGL triangle readback empty");return;}}
 if(glGetError()!=GL_NO_ERROR){snprintf(error,capacity,"EAGL draw failed");return;}glBindRenderbuffer(GL_RENDERBUFFER,s.renderbuffer);if(![s.context presentRenderbuffer:GL_RENDERBUFFER]){snprintf(error,capacity,"EAGL present failed");return;}ww_ios_bring_controls();result=0;
 }});return result;
}

// Same authored table camera/light uniforms as the desktop scene backend.
void ww_scene_table_eye(void *handle,const float *eye,const float *light){onMain(^{
 WwGL *s=(__bridge WwGL *)handle;[EAGLContext setCurrentContext:s.context];glUseProgram(s.program);
 glUniform4fv(glGetUniformLocation(s.program,"tableEye"),1,eye);
 glUniform4fv(glGetUniformLocation(s.program,"exteriorLight"),2,light);
});}
