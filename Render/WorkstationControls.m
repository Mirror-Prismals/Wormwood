#import <Cocoa/Cocoa.h>
#import <objc/runtime.h>
#import <math.h>

// Activate the actual GLFW child process, not its separate launcher. Run only
// when a window is shown; background rendering must never steal focus.
void ww_native_window_activate(void *nativeWindow) {
    if(!nativeWindow)return;
    NSWindow *window=(__bridge NSWindow *)nativeWindow;
    [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
    if(window.miniaturized)[window deminiaturize:nil];
    [NSRunningApplication.currentApplication activateWithOptions:NSApplicationActivateIgnoringOtherApps];
    [NSApp activateIgnoringOtherApps:YES];
    [window makeKeyAndOrderFront:nil];
    [window makeFirstResponder:window.contentView];
}

// Opt-in preview chrome. It never draws into or samples the gem image.
@interface MirrorWorkstationControls : NSObject <NSToolbarDelegate> {
    uint8_t _pending[5];
}
@property(nonatomic,weak) NSWindow *window;
@property(nonatomic,strong) NSTextField *rate;
-(uint32_t)take;
@end
@implementation MirrorWorkstationControls
-(NSArray<NSToolbarItemIdentifier> *)toolbarAllowedItemIdentifiers:(NSToolbar *)toolbar {
    return @[@"top",@"bottom",@"left",@"right",@"rate",@"quit"];
}
-(NSArray<NSToolbarItemIdentifier> *)toolbarDefaultItemIdentifiers:(NSToolbar *)toolbar {
    return [self toolbarAllowedItemIdentifiers:toolbar];
}
-(NSToolbarItem *)toolbar:(NSToolbar *)toolbar itemForItemIdentifier:(NSToolbarItemIdentifier)identifier willBeInsertedIntoToolbar:(BOOL)insert {
    NSToolbarItem *item=[[NSToolbarItem alloc] initWithItemIdentifier:identifier];
    if ([identifier isEqualToString:@"rate"]) {
        self.rate=[NSTextField labelWithString:@"Draw FPS: starting…"];
        self.rate.font=[NSFont monospacedDigitSystemFontOfSize:12 weight:NSFontWeightMedium];
        self.rate.frame=NSMakeRect(0,0,190,24);
        self.rate.toolTip=@"Frames drawn divided by elapsed wall time, including waits. Idle windows draw zero frames. This is not measured display scanout.";
        item.view=self.rate;item.minSize=NSMakeSize(190,24);item.maxSize=item.minSize;
    } else {
        NSArray *ids=@[@"top",@"bottom",@"left",@"right",@"quit"];
        NSArray *labels=@[@"Toggle Top ↑",@"Toggle Bottom ↓",@"Toggle Left ←",@"Toggle Right →",@"Quit"];
        NSUInteger index=[ids indexOfObject:identifier];if(index==NSNotFound)return nil;
        NSButton *button=[NSButton buttonWithTitle:labels[index] target:self action:@selector(press:)];
        button.tag=index;button.bezelStyle=NSBezelStyleRounded;
        button.toolTip=index<4?@"Open or close this panel. Arrow keys remain available.":@"Close this preview.";
        [button sizeToFit];item.view=button;item.label=labels[index];
        item.minSize=button.frame.size;item.maxSize=item.minSize;
    }
    return item;
}
-(void)press:(NSButton *)sender {
    NSInteger index=sender.tag;if(index<0||index>4)return;
    if(_pending[index]<8)_pending[index]++;
    [NSApp activateIgnoringOtherApps:YES];
    [self.window makeKeyAndOrderFront:nil];
    [self.window makeFirstResponder:self.window.contentView];
}
-(uint32_t)take {
    uint32_t mask=0;for(int i=0;i<5;i++)if(_pending[i]){_pending[i]--;mask|=1u<<i;}return mask;
}
@end
static char controlsAssociation;
void ww_workstation_controls_show(void *nativeWindow) {
    if(!nativeWindow)return;NSWindow *window=(__bridge NSWindow *)nativeWindow;
    // Exclusive fullscreen windows cannot host an AppKit toolbar.
    if((window.styleMask & NSWindowStyleMaskTitled)==0)return;
    if(objc_getAssociatedObject(window,&controlsAssociation))return;
    NSSize originalContent=window.contentView.bounds.size;
    MirrorWorkstationControls *controls=[MirrorWorkstationControls new];controls.window=window;
    objc_setAssociatedObject(window,&controlsAssociation,controls,OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    NSToolbar *toolbar=[[NSToolbar alloc] initWithIdentifier:@"MirrorPreviewPanelControls"];
    toolbar.delegate=controls;toolbar.allowsUserCustomization=NO;toolbar.displayMode=NSToolbarDisplayModeIconOnly;
    window.toolbar=toolbar;window.toolbarStyle=NSWindowToolbarStyleUnifiedCompact;
    [window setContentSize:originalContent];
    // Direct command-line launch did not establish physical key delivery.
    // Explicit activation is confined to this opted-in window's first show.
    [NSApp activateIgnoringOtherApps:YES];[window makeKeyAndOrderFront:nil];
    [window makeFirstResponder:window.contentView];
}
uint32_t ww_workstation_controls_take(void *nativeWindow,uint32_t *status) {
    if(!nativeWindow)return 0;NSWindow *window=(__bridge NSWindow *)nativeWindow;
    MirrorWorkstationControls *controls=objc_getAssociatedObject(window,&controlsAssociation);
    if(status)*status=(NSApp.active?1u:0u)|(window.keyWindow?2u:0u)|(window.firstResponder==window.contentView?4u:0u)
        |(NSApp.activationPolicy==NSApplicationActivationPolicyRegular?8u:0u)|(window.canBecomeKeyWindow?16u:0u)|(window.visible?32u:0u);
    return controls?[controls take]:0;
}
void ww_workstation_controls_rate(void *nativeWindow,double fps,bool focused) {
    if(!nativeWindow)return;NSWindow *window=(__bridge NSWindow *)nativeWindow;
    MirrorWorkstationControls *controls=objc_getAssociatedObject(window,&controlsAssociation);
    if(!controls)return;
    controls.rate.stringValue=[NSString stringWithFormat:@"Draw FPS: %.1f%@",fps,!focused?@" · paused":(fps==0?@" · idle":@"")];
}


// Drag previews compose persistent baked sprites. No optics or scene recapture.
#import <CoreText/CoreText.h>
#import <QuartzCore/QuartzCore.h>
@interface MirrorWorkstationCursorLabel : NSView
@property(nonatomic,strong) NSAttributedString *text;
@property(nonatomic,strong) NSFont *font;
@property(nonatomic,strong) NSImage *artwork;
@property(nonatomic,strong) NSDictionary *identity;
@property(nonatomic,strong) NSTimer *motion;
@property(nonatomic) NSPoint target;
@property(nonatomic) NSPoint previousTarget;
@property(nonatomic) double angle;
@property(nonatomic) double angularVelocity;
@property(nonatomic) double targetAngle;
@property(nonatomic) double lastTick;
@property(nonatomic) double lastPointer;
@end
@implementation MirrorWorkstationCursorLabel
-(NSView *)hitTest:(NSPoint)point {return nil;}
-(BOOL)acceptsFirstResponder {return NO;}
-(BOOL)isOpaque {return NO;}
-(BOOL)isFlipped {return YES;}
-(void)drawRect:(NSRect)dirty {
 if(self.artwork){[self.artwork drawInRect:self.bounds fromRect:NSZeroRect operation:NSCompositingOperationSourceOver fraction:1. respectFlipped:YES hints:nil];}
 else {[self.text drawAtPoint:NSZeroPoint];}
}
-(void)stopMotion {
 [self.motion invalidate];self.motion=nil;self.angle=0;self.angularVelocity=0;self.targetAngle=0;
 self.layer.transform=CATransform3DIdentity;
}
-(void)tick {
 if(self.hidden||!self.window){[self stopMotion];return;}
 double now=CACurrentMediaTime(),dt=fmin(.025,fmax(.001,now-self.lastTick));self.lastTick=now;
 if(now-self.lastPointer>.07)self.targetAngle=0;
 // Damped angular spring: pointer stays exact; only orientation has inertia.
 self.angularVelocity+=(100*(self.targetAngle-self.angle)-12*self.angularVelocity)*dt;
 self.angle+=self.angularVelocity*dt;self.angle=fmin(.28,fmax(-.28,self.angle));
 CATransform3D t=CATransform3DIdentity;t.m34=-1./900.;
 t=CATransform3DRotate(t,self.angle*.6,1,0,0);
 t=CATransform3DRotate(t,-self.angle*.85,0,1,0);
 self.layer.transform=CATransform3DRotate(t,self.angle,0,0,1);
 if(fabs(self.angle)<.0002&&fabs(self.angularVelocity)<.001&&self.targetAngle==0){[self.motion invalidate];self.motion=nil;self.layer.transform=CATransform3DIdentity;}
}
@end
static char cursorLabelAssociation;
static NSImage *WWDragImage(NSString *root,NSString *name){
 static NSCache *cache; if(!cache)cache=[NSCache new];
 NSString *path=[[root stringByAppendingPathComponent:@"Art/AudioWorkstation/Baked/Drag"] stringByAppendingPathComponent:[name stringByAppendingString:@".png"]];
 NSImage *image=[cache objectForKey:path];if(!image){image=[[NSImage alloc]initWithContentsOfFile:path];if(image)[cache setObject:image forKey:path];}return image;
}
static void WWDragDraw(NSImage *image,NSRect r){[image drawInRect:r fromRect:NSZeroRect operation:NSCompositingOperationSourceOver fraction:1. respectFlipped:YES hints:nil];}
// Reuse the stock EQ's original native plot/labels in the cached preview.
extern void ww_workstation_drag_eq(NSFont *font,NSDictionary *graph);
static NSImage *WWDragCompose(NSString *root,NSDictionary *payload,NSFont *font){
 NSString *kind=payload[@"preview"];BOOL track=[@[@"audio",@"midi",@"cc",@"comp"] containsObject:kind];
 BOOL meter=[kind isEqual:@"matrix"]||[kind isEqual:@"mix"],eq=[kind isEqual:@"eq"],tape=[kind isEqual:@"tape"];
 NSString *name=track?([kind isEqual:@"comp"]?@"CompLaneIvory":@"Panel0"):meter?@"GemMeter_Ivory":eq?@"StockEQ0":tape?@"TapeClip_Long":@"Edison";
 NSImage *source=WWDragImage(root,name);if(!source)return nil;
 NSSize size=track?NSMakeSize(24,[kind isEqual:@"audio"]?96:64):meter?NSMakeSize(22,160):eq?NSMakeSize(300,252):tape?NSMakeSize(220,72):NSMakeSize(220,130);
 NSImage *result=[[NSImage alloc]initWithSize:size];[result lockFocusFlipped:YES];
 if(track){
  // Match the timeline's nine-slice ruby/ivory handle rather than stretch bevels.
  CGFloat band=4;NSSize src=source.size;CGFloat sx=src.width*.2,sy=src.height*.2;
  CGFloat xs[4]={0,band,size.width-band,size.width},ys[4]={0,band,size.height-band,size.height};
  CGFloat us[4]={0,sx,src.width-sx,src.width},vs[4]={0,sy,src.height-sy,src.height};
  for(int y=0;y<3;y++)for(int x=0;x<3;x++)[source drawInRect:NSMakeRect(xs[x],ys[y],xs[x+1]-xs[x],ys[y+1]-ys[y]) fromRect:NSMakeRect(us[x],vs[y],us[x+1]-us[x],vs[y+1]-vs[y]) operation:NSCompositingOperationSourceOver fraction:1 respectFlipped:YES hints:nil];
 }else if(meter){
  CGFloat sy[4]={0,source.size.height*12./180.,source.size.height*168./180.,source.size.height};
  CGFloat dy[4]={0,2,158,160};
  for(int i=0;i<3;i++)[source drawInRect:NSMakeRect(0,dy[i],size.width,dy[i+1]-dy[i]) fromRect:NSMakeRect(0,sy[i],source.size.width,sy[i+1]-sy[i]) operation:NSCompositingOperationSourceOver fraction:1 respectFlipped:YES hints:nil];
 }else{WWDragDraw(source,NSMakeRect(0,0,size.width,eq?300.*168./230.:size.height));}
 if(eq){[NSGraphicsContext saveGraphicsState];NSAffineTransform *t=[NSAffineTransform transform];[t scaleBy:300./230.];[t concat];ww_workstation_drag_eq(font,payload[@"eq"]);[NSGraphicsContext restoreGraphicsState];}
 if(tape){
  NSArray *wave=payload[@"wave"];
  [[NSColor colorWithSRGBRed:.88 green:.68 blue:.23 alpha:1]setStroke];NSBezierPath *path=[NSBezierPath bezierPath];path.lineWidth=1;
  if([wave isKindOfClass:NSArray.class])for(NSUInteger i=0;i<wave.count;i++){
   NSArray *v=wave[i];double x=8+(i+.5)*(size.width-16)/wave.count;
   [path moveToPoint:NSMakePoint(x,size.height*.5-[v[1]doubleValue]*22)];[path lineToPoint:NSMakePoint(x,size.height*.5-[v[0]doubleValue]*22)];
  }
  [path stroke];
 }
 [result unlockFocus];return result;
}
bool ww_workstation_cursor_label(void *nativeWindow,const char *caption,double x,double y,const char *fontPath,const char *json){
 if(!nativeWindow)return false;NSWindow *window=(__bridge NSWindow *)nativeWindow;
 MirrorWorkstationCursorLabel *label=objc_getAssociatedObject(window,&cursorLabelAssociation);
 if(!caption||!*caption){label.hidden=YES;[label stopMotion];label.identity=nil;label.artwork=nil;return true;}
 if(!label){
  NSData *data=[NSData dataWithContentsOfFile:[NSString stringWithUTF8String:fontPath]];if(!data)return false;
  CGDataProviderRef provider=CGDataProviderCreateWithCFData((__bridge CFDataRef)data);CGFontRef cgFont=provider?CGFontCreateWithDataProvider(provider):NULL;
  if(provider)CGDataProviderRelease(provider);if(!cgFont)return false;
  CTFontRef font=CTFontCreateWithGraphicsFont(cgFont,21.6,NULL,NULL);CGFontRelease(cgFont);if(!font)return false;
  label=[[MirrorWorkstationCursorLabel alloc]initWithFrame:NSZeroRect];label.font=CFBridgingRelease(font);label.wantsLayer=YES;
  label.autoresizingMask=NSViewMinYMargin;[window.contentView addSubview:label positioned:NSWindowAbove relativeTo:nil];
  objc_setAssociatedObject(window,&cursorLabelAssociation,label,OBJC_ASSOCIATION_RETAIN_NONATOMIC);
 }
 NSDictionary *payload=json?[NSJSONSerialization JSONObjectWithData:[[NSString stringWithUTF8String:json] dataUsingEncoding:NSUTF8StringEncoding] options:0 error:nil]:nil;
 NSMutableDictionary *identity=[payload mutableCopy];[identity removeObjectForKey:@"position"];
 BOOL fresh=label.hidden||!label.identity;
 if(![label.identity isEqual:identity]){
  label.identity=identity;NSString *root=[[[NSString stringWithUTF8String:fontPath]stringByDeletingLastPathComponent]stringByDeletingLastPathComponent];root=[root stringByDeletingLastPathComponent];
  label.artwork=WWDragCompose(root,payload,label.font);
  if(payload[@"preview"]&&!label.artwork)return false;
  label.text=[[NSAttributedString alloc]initWithString:[NSString stringWithUTF8String:caption] attributes:@{NSFontAttributeName:label.font,NSForegroundColorAttributeName:[NSColor colorWithSRGBRed:244./255. green:241./255. blue:232./255. alpha:1.]}];label.needsDisplay=YES;
 }
 double now=CACurrentMediaTime();NSPoint target=NSMakePoint(x,y);
 if(fresh){[label stopMotion];label.previousTarget=target;label.lastPointer=now;}
 else if(!NSEqualPoints(target,label.previousTarget)){
  double dt=fmax(.008,now-label.lastPointer);double vx=(x-label.previousTarget.x)/dt,vy=(y-label.previousTarget.y)/dt;
  label.targetAngle=fmin(.22,fmax(-.22,(vx*.00015+vy*.00006)));label.lastPointer=now;label.previousTarget=target;
 }
 NSSize size=label.artwork?label.artwork.size:label.text.size;
 label.frame=NSMakeRect(x,window.contentView.bounds.size.height-y-size.height,size.width,size.height);label.hidden=NO;
 if(!label.motion){label.lastTick=now;__weak MirrorWorkstationCursorLabel *weak=label;
  label.motion=[NSTimer timerWithTimeInterval:1./120. repeats:YES block:^(NSTimer *timer){MirrorWorkstationCursorLabel *view=weak;if(view)[view tick];else[timer invalidate];}];
  [[NSRunLoop mainRunLoop]addTimer:label.motion forMode:NSRunLoopCommonModes];
 }
 return true;
}

// Cardinal's range fill + two vertical edges. Native pass-through layer avoids
// optical rebaking while selecting. Tape rectangles are cut out so clips stay in front.
@interface MirrorWorkstationRange : NSView
@property(nonatomic,strong) NSDictionary *rangeData;
@end
@implementation MirrorWorkstationRange
-(NSView *)hitTest:(NSPoint)p{return nil;}
-(BOOL)acceptsFirstResponder{return NO;}
-(BOOL)isOpaque{return NO;}
-(BOOL)isFlipped{return YES;}
-(void)drawRect:(NSRect)dirty{
 // Fade display is unlit UI content; never recapture lighting for a drag.
 for(NSDictionary *fade in self.rangeData[@"fades"]){
  NSArray *c=fade[@"clip"];[NSGraphicsContext saveGraphicsState];
  [[NSBezierPath bezierPathWithRect:NSInsetRect(NSMakeRect([c[0]doubleValue],[c[1]doubleValue],[c[2]doubleValue],[c[3]doubleValue]),0.,-5.)]addClip];
  NSBezierPath *line=[NSBezierPath bezierPath];line.lineWidth=1.5;BOOL first=YES;
  for(NSArray *p in fade[@"points"]){NSPoint point=NSMakePoint([p[0]doubleValue],[p[1]doubleValue]);if(first){[line moveToPoint:point];first=NO;}else{[line lineToPoint:point];}}
  [[NSColor colorWithSRGBRed:1. green:.9 blue:.58 alpha:1.]setStroke];[line stroke];
  [[NSColor colorWithSRGBRed:1. green:.9 blue:.58 alpha:1.]setFill];
  for(NSArray *p in fade[@"handles"]){NSRectFill(NSMakeRect([p[0]doubleValue]-4.,[p[1]doubleValue]-4.,8.,8.));}
  [NSGraphicsContext restoreGraphicsState];
 }
 NSArray *a=self.rangeData[@"rect"];if(a.count!=4)return;
 NSRect r=NSMakeRect([a[0] doubleValue],[a[1] doubleValue],[a[2] doubleValue],[a[3] doubleValue]);
 [NSGraphicsContext saveGraphicsState];
 NSBezierPath *clip=[NSBezierPath bezierPathWithRect:NSInsetRect(r,-1.,0.)];clip.windingRule=NSEvenOddWindingRule;
 // Clip exclusions can overlap. Apply each exclusion independently (not XOR).
 [clip addClip];
 for(NSArray *h in self.rangeData[@"holes"]){NSBezierPath *hole=[NSBezierPath bezierPathWithRect:self.bounds];hole.windingRule=NSEvenOddWindingRule;[hole appendBezierPathWithRect:NSMakeRect([h[0] doubleValue],[h[1] doubleValue],[h[2] doubleValue],[h[3] doubleValue])];[hole addClip];}
 [[NSColor colorWithSRGBRed:(143./255.)*.35 green:(184./255.)*.35 blue:.35 alpha:1.]setFill];NSRectFill(r);
 [[NSColor colorWithSRGBRed:143./255. green:184./255. blue:1. alpha:1.]setFill];
 NSRectFill(NSMakeRect(r.origin.x-1.,r.origin.y,2.,r.size.height));NSRectFill(NSMakeRect(NSMaxX(r)-1.,r.origin.y,2.,r.size.height));
 [NSGraphicsContext restoreGraphicsState];
}
@end
static char selectionRangeAssociation;
bool ww_workstation_selection_range(void *nativeWindow,const char *payload){
 if(!nativeWindow||!payload)return false;NSWindow *window=(__bridge NSWindow *)nativeWindow;
 NSData *data=[[NSString stringWithUTF8String:payload]dataUsingEncoding:NSUTF8StringEncoding];
 id value=[NSJSONSerialization JSONObjectWithData:data options:NSJSONReadingFragmentsAllowed error:nil];
 MirrorWorkstationRange *view=objc_getAssociatedObject(window,&selectionRangeAssociation);
 if(value==[NSNull null]){view.hidden=YES;return true;}if(![value isKindOfClass:[NSDictionary class]])return false;
 if(!view){view=[[MirrorWorkstationRange alloc]initWithFrame:window.contentView.bounds];view.autoresizingMask=NSViewWidthSizable|NSViewHeightSizable;view.wantsLayer=YES;[window.contentView addSubview:view positioned:NSWindowBelow relativeTo:nil];objc_setAssociatedObject(window,&selectionRangeAssociation,view,OBJC_ASSOCIATION_RETAIN_NONATOMIC);}
 view.frame=window.contentView.bounds;view.rangeData=value;view.hidden=NO;view.needsDisplay=YES;return true;
}

#import "WorkstationPluginSurface.m"

#include "WorkstationCcParameterPicker.m"

// Modeless sample selection; polled by the workstation control thread.
static NSOpenPanel *wwDrumSamplePanel;
static NSString *wwDrumSamplePath;
static int wwDrumSampleResult;
int ww_drum_sample_open(void) {
    if (![NSThread isMainThread] || wwDrumSamplePanel) return -1;
    wwDrumSamplePath=nil; wwDrumSampleResult=0;
    wwDrumSamplePanel=[NSOpenPanel openPanel];
    wwDrumSamplePanel.canChooseDirectories=NO;
    wwDrumSamplePanel.allowsMultipleSelection=NO;
    wwDrumSamplePanel.allowedFileTypes=@[@"wav",@"wave"];
    wwDrumSamplePanel.title=@"Load drum rack sample";
    [wwDrumSamplePanel beginWithCompletionHandler:^(NSModalResponse response) {
        wwDrumSamplePath=response==NSModalResponseOK?wwDrumSamplePanel.URL.path:nil;
        wwDrumSampleResult=wwDrumSamplePath?1:-1;
        wwDrumSamplePanel=nil;
    }];
    return 0;
}
int ww_drum_sample_poll(char *path,size_t capacity) {
    if (![NSThread isMainThread]) return -1;
    int result=wwDrumSampleResult;
    if(result==1){const char *value=wwDrumSamplePath.UTF8String;
        if(!path||!value||strlen(value)>=capacity)result=-1;
        else memcpy(path,value,strlen(value)+1);
    }
    wwDrumSampleResult=0;wwDrumSamplePath=nil;return result;
}

void ww_drum_sample_cancel(void) {
    if (![NSThread isMainThread]) return;
    [wwDrumSamplePanel cancel:nil];wwDrumSamplePath=nil;wwDrumSampleResult=0;
}


// Unlit, pass-through edge bubble for the unbounded drag cursor.
@interface MirrorVirtualCursor : NSView
@property(nonatomic) double dx;
@property(nonatomic) double dy;
@end
@implementation MirrorVirtualCursor
-(BOOL)isFlipped{return YES;}
-(NSView*)hitTest:(NSPoint)p{return nil;}
-(void)drawRect:(NSRect)dirty {
 [[NSColor colorWithSRGBRed:0.07 green:0.16 blue:0.22 alpha:0.92] setFill];
 NSBezierPath *bubble=[NSBezierPath bezierPathWithOvalInRect:NSInsetRect(self.bounds,2,2)];[bubble fill];
 [[NSColor colorWithSRGBRed:0.55 green:0.88 blue:1.0 alpha:1] setStroke];bubble.lineWidth=2;[bubble stroke];
 NSString *direction=fabs(self.dx)>fabs(self.dy)?(self.dx<0?@"←":@"→"):(self.dy<0?@"↑":@"↓");
 NSString *text=[NSString stringWithFormat:@"%@\n%.0f px",direction,hypot(self.dx,self.dy)];
 NSMutableParagraphStyle *style=[NSMutableParagraphStyle new];style.alignment=NSTextAlignmentCenter;
 [text drawInRect:NSInsetRect(self.bounds,3,12) withAttributes:@{NSFontAttributeName:[NSFont boldSystemFontOfSize:13],NSForegroundColorAttributeName:NSColor.whiteColor,NSParagraphStyleAttributeName:style}];
}
@end
void ww_workstation_virtual_cursor(void *nativeWindow,double x,double y,double dx,double dy,bool visible){
 NSWindow *window=(__bridge NSWindow*)nativeWindow;if(!window)return;
 static char key;MirrorVirtualCursor *bubble=objc_getAssociatedObject(window,&key);
 if(!visible){bubble.hidden=YES;return;}
 if(!bubble){bubble=[[MirrorVirtualCursor alloc]initWithFrame:NSMakeRect(0,0,68,68)];objc_setAssociatedObject(window,&key,bubble,OBJC_ASSOCIATION_RETAIN_NONATOMIC);}
 if(bubble.superview!=window.contentView){[bubble removeFromSuperview];[window.contentView addSubview:bubble positioned:NSWindowAbove relativeTo:nil];}
 NSRect bounds=window.contentView.bounds;
 double bx=fmin(fmax(x-34,0),fmax(0,bounds.size.width-68));
 double by=fmin(fmax(y-34,0),fmax(0,bounds.size.height-68));
 if(!window.contentView.isFlipped)by=bounds.size.height-by-68;
 bubble.frame=NSMakeRect(bx,by,68,68);bubble.dx=dx;bubble.dy=dy;bubble.hidden=NO;bubble.needsDisplay=YES;
}
