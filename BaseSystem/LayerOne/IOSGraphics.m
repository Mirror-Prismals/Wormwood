// Native iOS graphics client, private to GraphicsConnectionSystem.
#import <UIKit/UIKit.h>
#import <QuartzCore/CAEAGLLayer.h>
#import <OpenGLES/ES3/gl.h>
#include <stdio.h>
#include <string.h>
static __weak UIViewController *hostController;
static BOOL hostActive=YES;
void ww_ios_attach(void *controller) { hostController=(__bridge UIViewController *)controller; }
void ww_ios_active(bool active) {hostActive=active;}
static void onMain(dispatch_block_t block) {if(NSThread.isMainThread)block();else dispatch_sync(dispatch_get_main_queue(),block);}
@interface WwGLView : UIView
@end
@implementation WwGLView
+ (Class)layerClass {return CAEAGLLayer.class;}
@end
@interface WwGL : NSObject
@property(nonatomic,strong) WwGLView *view;
@property(nonatomic,strong) EAGLContext *context;
@property(nonatomic,assign) GLuint framebuffer,renderbuffer,program,vbo,vao;
@property(nonatomic,assign) GLint width,height;
@property(nonatomic,assign) BOOL captured;
@end
@implementation WwGL
@end
typedef struct {uint32_t width,height;uint8_t center[4],corner[4];char renderer[256];bool readback;} WwGraphicsInfo;
static GLuint compile(GLenum type,const char *source,char *error,size_t capacity) {
    GLuint shader=glCreateShader(type);glShaderSource(shader,1,&source,NULL);glCompileShader(shader);
    GLint success=0;glGetShaderiv(shader,GL_COMPILE_STATUS,&success);
    if(!success){glGetShaderInfoLog(shader,(GLsizei)capacity,NULL,error);glDeleteShader(shader);return 0;}return shader;
}
static void destroy(WwGL *state) {
    [EAGLContext setCurrentContext:state.context];glFinish();
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
        s.view=[[WwGLView alloc]initWithFrame:hostController.view.bounds];s.view.contentScaleFactor=hostController.view.window.screen.scale;
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
void *ww_metal_open(void) {
    __block WwMetalView *view=nil;
    onMain(^{
        if(!hostActive || !hostController)return;
        view=[[WwMetalView alloc]initWithFrame:hostController.view.bounds];view.hidden=YES;
        view.contentScaleFactor=hostController.view.window.screen.scale;
        CAMetalLayer *layer=(CAMetalLayer *)view.layer;layer.device=MTLCreateSystemDefaultDevice();layer.pixelFormat=MTLPixelFormatBGRA8Unorm;layer.framebufferOnly=NO;
        layer.drawableSize=CGSizeMake(view.bounds.size.width*view.contentScaleFactor,view.bounds.size.height*view.contentScaleFactor);
        [hostController.view addSubview:view];
    });return view?(__bridge_retained void *)view:NULL;
}
void *ww_metal_layer(void *handle){__block void *layer=NULL;onMain(^{layer=(__bridge void *)((__bridge WwMetalView *)handle).layer;});return layer;}
void ww_metal_dimensions(void *handle,uint32_t *width,uint32_t *height){onMain(^{CGSize size=((CAMetalLayer *)((__bridge WwMetalView *)handle).layer).drawableSize;*width=size.width;*height=size.height;});}
void ww_metal_show(void *handle){onMain(^{((__bridge WwMetalView *)handle).hidden=NO;});}
bool ww_metal_closed(void *handle){__block BOOL closed=YES;onMain(^{closed=!hostActive || !((__bridge WwMetalView *)handle).window;});return closed;}
void ww_metal_close(void *handle){onMain(^{[((__bridge WwMetalView *)handle) removeFromSuperview];CFRelease(handle);});}
