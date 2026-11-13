// UIKit counterpart of Render/WorkstationPluginSurface.m and WorkstationControls.m.
// UIOnly: the existing Rust controllers own every hit and mutation. No gem or
// optical state, input recognizers, native buttons or audio callbacks live here.
#import <UIKit/UIKit.h>
#import <CoreText/CoreText.h>
#import <objc/runtime.h>
#include <math.h>
#include <string.h>
#include "WorkstationOverlay.h"
#include "../../Render/CcStbEasyFont.h"

static NSArray *WWList(id x){return [x isKindOfClass:NSArray.class]?x:@[];}
static BOOL WWFlag(id x){return [x isKindOfClass:NSNumber.class]&&[x boolValue];}
static BOOL WWNumber(id x){return [x isKindOfClass:NSNumber.class]&&isfinite([x doubleValue]);}
static BOOL WWArray(id x,NSUInteger n){if(![x isKindOfClass:NSArray.class]||[x count]!=n)return NO;for(id v in x)if(!WWNumber(v))return NO;return YES;}
static BOOL WWRect(id x){return WWArray(x,4)&&[x[2]doubleValue]>=0.&&[x[3]doubleValue]>=0.;}
static BOOL WWText(id x){return [x isKindOfClass:NSString.class];}
static BOOL WWOptionalArray(id x){return !x||x==NSNull.null||[x isKindOfClass:NSArray.class];}
static CGRect WWPluginRect(NSArray *r){return CGRectMake([r[0]doubleValue],[r[1]doubleValue],[r[2]doubleValue],[r[3]doubleValue]);}
static UIColor *WWColor(NSArray *rgb,double divisor,double alpha){return [UIColor colorWithRed:[rgb[0]doubleValue]/divisor green:[rgb[1]doubleValue]/divisor blue:[rgb[2]doubleValue]/divisor alpha:alpha];}
static UIColor *WWBlend(UIColor *a,double t,UIColor *b){CGFloat ar,ag,ab,aa,br,bg,bb,ba;[a getRed:&ar green:&ag blue:&ab alpha:&aa];[b getRed:&br green:&bg blue:&bb alpha:&ba];return [UIColor colorWithRed:ar+(br-ar)*t green:ag+(bg-ag)*t blue:ab+(bb-ab)*t alpha:aa+(ba-aa)*t];}
static BOOL WWValid(uint32_t kind,id p){
 if(p==NSNull.null)return YES;if(![p isKindOfClass:NSDictionary.class])return NO;
 if(kind==WWWorkstationCursorLabel)return WWText(p[@"text"])&&WWArray(p[@"position"],2);
 if(kind==WWWorkstationSelectionRange){
  if(p[@"rect"]){if(!WWRect(p[@"rect"])||![p[@"holes"]isKindOfClass:NSArray.class])return NO;for(id h in p[@"holes"])if(!WWRect(h))return NO;}
  if(p[@"fades"]){if(![p[@"fades"]isKindOfClass:NSArray.class]||[p[@"fades"]count]>128)return NO;for(id f in p[@"fades"]){if(!WWRect(f[@"clip"])||![f[@"points"]isKindOfClass:NSArray.class]||[f[@"points"]count]>2050||![f[@"handles"]isKindOfClass:NSArray.class]||[f[@"handles"]count]>2048)return NO;for(id q in f[@"points"])if(!WWArray(q,2))return NO;for(id q in f[@"handles"])if(!WWArray(q,2))return NO;}}
  return YES;}
 if(kind==WWWorkstationCcParameterPicker){
  if(!WWArray(p[@"viewport"],2)||[p[@"viewport"][0]doubleValue]<=0.||[p[@"viewport"][1]doubleValue]<=0.||!WWRect(p[@"rect"])||!WWNumber(p[@"bevel"])||![p[@"rows"]isKindOfClass:NSArray.class]||![p[@"colors"]isKindOfClass:NSDictionary.class])return NO;
  for(NSString *k in @[@"front",@"top",@"side",@"text",@"selected",@"hover"]){id rgb=p[@"colors"][k];if(!WWArray(rgb,3))return NO;for(NSNumber *v in rgb)if(v.doubleValue<0.||v.doubleValue>1.)return NO;}
  for(id row in p[@"rows"])if(![row isKindOfClass:NSDictionary.class]||!WWRect(row[@"rect"])||!WWArray(row[@"text_origin"],2)||!WWText(row[@"label"]))return NO;
  return YES;
 }
 if(kind!=WWWorkstationPluginSurface||![p[@"groups"]isKindOfClass:NSArray.class]||[p[@"groups"]count]>8)return NO;
 for(id g in p[@"groups"]){
  if(![g isKindOfClass:NSDictionary.class]||!WWRect(g[@"clip"])||![g[@"labels"]isKindOfClass:NSArray.class]||[g[@"labels"]count]>256||!WWOptionalArray(g[@"browser_faces"])||!WWOptionalArray(g[@"graphs"]))return NO;
  for(id l in g[@"labels"])if(![l isKindOfClass:NSDictionary.class]||!WWText(l[@"text"])||!WWRect(l[@"rect"])||!WWNumber(l[@"size"])||[l[@"size"]doubleValue]<=0.||[l[@"size"]doubleValue]>128.||!WWArray(l[@"color"],3)||(l[@"icon"]&&l[@"icon"]!=NSNull.null&&!WWRect(l[@"icon"])))return NO;
  if(WWList(g[@"browser_faces"]).count>8192||WWList(g[@"graphs"]).count>16)return NO;
  for(id f in WWList(g[@"browser_faces"]))if(![f isKindOfClass:NSDictionary.class]||!WWRect(f[@"rect"])||!WWArray(f[@"color"],3)||!WWNumber(f[@"radius"])||[f[@"radius"]doubleValue]<0.||[f[@"radius"]doubleValue]>128.||!WWNumber(f[@"alpha"])||[f[@"alpha"]doubleValue]<0.||[f[@"alpha"]doubleValue]>1.)return NO;
  for(id a in WWList(g[@"graphs"])){
   if(![a isKindOfClass:NSDictionary.class])return NO;
   for(NSString *k in @[@"rect",@"plot",@"bypass",@"remove"])if(!WWRect(a[k]))return NO;
   if(![a[@"slopes"]isKindOfClass:NSArray.class]||[a[@"slopes"]count]!=2||!WWRect(a[@"slopes"][0])||!WWRect(a[@"slopes"][1])||!WWArray(a[@"edges"],2)||!WWText(a[@"title"])||![a[@"slope_labels"]isKindOfClass:NSArray.class]||[a[@"slope_labels"]count]!=2||!WWText(a[@"slope_labels"][0])||!WWText(a[@"slope_labels"][1])||![a[@"response"]isKindOfClass:NSArray.class]||[a[@"response"]count]>97)return NO;
   for(NSString *k in @[@"gain_y",@"zero_y",@"scale"])if(!WWNumber(a[k]))return NO;
   for(id point in a[@"response"])if(!WWArray(point,2))return NO;
  }
 }
 return YES;
}
static id WWLoadFont(NSString *path,double size){
 NSData *data=[NSData dataWithContentsOfFile:path];if(!data)return nil;
 CGDataProviderRef provider=CGDataProviderCreateWithCFData((__bridge CFDataRef)data);
 CGFontRef font=provider?CGFontCreateWithDataProvider(provider):NULL;if(provider)CGDataProviderRelease(provider);if(!font)return nil;
 CTFontRef text=CTFontCreateWithGraphicsFont(font,size,NULL,NULL);CGFontRelease(font);return CFBridgingRelease(text);
}
static CTLineRef WWLine(NSString *text,id font,UIColor *color){
 NSAttributedString *s=[[NSAttributedString alloc]initWithString:text attributes:@{(__bridge id)kCTFontAttributeName:font,(__bridge id)kCTForegroundColorAttributeName:(__bridge id)color.CGColor}];
 return CTLineCreateWithAttributedString((__bridge CFAttributedStringRef)s);
}
static CGSize WWMeasure(NSString *text,id font){CGFloat a,d,l;CTLineRef line=WWLine(text,font,UIColor.whiteColor);double width=CTLineGetTypographicBounds(line,&a,&d,&l);CFRelease(line);return CGSizeMake(width,a+d+l);}
static void WWDrawText(NSString *text,id font,UIColor *color,CGPoint top){
 if(!text.length)return;CTLineRef line=WWLine(text,font,color);CGFloat ascent;CTLineGetTypographicBounds(line,&ascent,NULL,NULL);
 CGContextRef c=UIGraphicsGetCurrentContext();CGContextSaveGState(c);CGContextSetTextMatrix(c,CGAffineTransformIdentity);
 CGContextTranslateCTM(c,top.x,top.y+ascent);CGContextScaleCTM(c,1.,-1.);CGContextSetTextPosition(c,0.,0.);CTLineDraw(line,c);CGContextRestoreGState(c);CFRelease(line);
}
static void WWQuad(CGPoint a,CGPoint b,CGPoint c,CGPoint d){UIBezierPath *p=[UIBezierPath bezierPath];[p moveToPoint:a];[p addLineToPoint:b];[p addLineToPoint:c];[p addLineToPoint:d];[p closePath];[p fill];}

@interface WWWorkstationOverlay:UIView
@property(nonatomic,strong)NSMutableDictionary<NSNumber *,NSDictionary *> *channels;
@end
@implementation WWWorkstationOverlay
-(instancetype)initWithFrame:(CGRect)frame{if((self=[super initWithFrame:frame])){self.channels=[NSMutableDictionary new];self.opaque=NO;self.backgroundColor=UIColor.clearColor;self.userInteractionEnabled=NO;self.isAccessibilityElement=NO;self.accessibilityElementsHidden=YES;self.clipsToBounds=YES;self.autoresizingMask=UIViewAutoresizingFlexibleWidth|UIViewAutoresizingFlexibleHeight;self.contentMode=UIViewContentModeRedraw;}return self;}
-(UIView *)hitTest:(CGPoint)point withEvent:(UIEvent *)event{(void)point;(void)event;return nil;}
-(BOOL)canBecomeFirstResponder{return NO;}
-(void)label:(NSString *)text font:(id)base rect:(CGRect)r size:(double)size centered:(BOOL)center color:(UIColor *)color{
 if(r.size.width<=0.||r.size.height<=0.)return;
 id font=CFBridgingRelease(CTFontCreateCopyWithAttributes((__bridge CTFontRef)base,size,NULL,NULL));
 NSString *prefix=text;BOOL truncated=NO;
 while(prefix.length&&WWMeasure(truncated?[prefix stringByAppendingString:@"..."]:prefix,font).width>r.size.width){NSRange last=[prefix rangeOfComposedCharacterSequenceAtIndex:prefix.length-1];prefix=[prefix substringToIndex:last.location];truncated=YES;}
 NSString *shown=truncated?[prefix stringByAppendingString:@"..."]:prefix;CGSize s=WWMeasure(shown,font);
 WWDrawText(shown,font,color,CGPointMake(center?CGRectGetMidX(r)-s.width*.5:r.origin.x,CGRectGetMidY(r)-s.height*.5));
}
// The stock renderer calls the same label helper with the current surface font.
-(void)label:(NSString *)text rect:(CGRect)r size:(double)size centered:(BOOL)center color:(UIColor *)color{
 [self label:text font:self.channels[@(WWWorkstationPluginSurface)][@"fonts"][0] rect:r size:size centered:center color:color];
}
-(void)lineFrom:(CGPoint)a to:(CGPoint)b width:(double)width color:(UIColor *)color{[color setStroke];UIBezierPath *path=[UIBezierPath bezierPath];path.lineWidth=width;[path moveToPoint:a];[path addLineToPoint:b];[path stroke];}
-(void)stock:(NSDictionary*)g{
 BOOL gem=WWFlag(g[@"gem_surface"]);
 double c=[g[@"scale"] doubleValue];CGRect r=WWPluginRect(g[@"rect"]),plot=WWPluginRect(g[@"plot"]);
 UIColor *front=[UIColor colorWithRed:230./255. green:224./255. blue:212./255. alpha:.85];
 UIColor *top=[UIColor colorWithRed:244./255. green:241./255. blue:232./255. alpha:1.];
 UIColor *side=[UIColor colorWithRed:207./255. green:198./255. blue:183./255. alpha:1.];
 UIColor *glyph=[UIColor colorWithRed:.15 green:.15 blue:.15 alpha:1.];
 UIColor *selected=WWBlend(front,.45,[UIColor colorWithRed:.18 green:.52 blue:.95 alpha:1.]);
 if(!gem){[side setFill];UIRectFill(CGRectOffset(r,-8*c,-8*c));[(WWFlag(g[@"selected"])?selected:front) setFill];UIRectFill(r);}
 [[UIColor colorWithRed:(207./255.)*.72 green:(198./255.)*.72 blue:(183./255.)*.72 alpha:1.]setFill];UIRectFill(CGRectInset(plot,-6*c,-6*c));
 double low=[g[@"edges"][0] doubleValue],high=[g[@"edges"][1] doubleValue],gain=[g[@"gain_y"] doubleValue],zero=[g[@"zero_y"] doubleValue];
 [self lineFrom:CGPointMake(CGRectGetMinX(plot),zero) to:CGPointMake(CGRectGetMaxX(plot),zero) width:c color:side];
 UIColor *curve=WWFlag(g[@"bypassed"])?top:[UIColor colorWithRed:244./255. green:241./255. blue:232./255. alpha:1.];
 for(int i=0;i<2;i++){double x=i?high:low;[self lineFrom:CGPointMake(x,CGRectGetMinY(plot)) to:CGPointMake(x,CGRectGetMaxY(plot)) width:c color:top];
  [self lineFrom:CGPointMake(x-4*c,(gain+zero)*.5) to:CGPointMake(x+4*c,(gain+zero)*.5) width:5*c color:curve];
  CGRect button=WWPluginRect(g[@"slopes"][i]);if(!gem){[front setFill];UIRectFill(button);}[self label:g[@"slope_labels"][i] rect:button size:9*c centered:YES color:glyph];}
 [self lineFrom:CGPointMake(low,gain) to:CGPointMake(high,gain) width:c color:top];
 UIBezierPath *diamond=[UIBezierPath bezierPath];double middle=(low+high)*.5;[diamond moveToPoint:CGPointMake(middle,gain-5*c)];[diamond addLineToPoint:CGPointMake(middle+5*c,gain)];[diamond addLineToPoint:CGPointMake(middle,gain+5*c)];[diamond addLineToPoint:CGPointMake(middle-5*c,gain)];[diamond closePath];[curve setFill];[diamond fill];
 NSArray *response=g[@"response"];if(response.count){UIBezierPath *path=[UIBezierPath bezierPath];path.lineWidth=2*c;for(NSUInteger i=0;i<response.count;i++){CGPoint p=CGPointMake([response[i][0] doubleValue],[response[i][1] doubleValue]);if(i)[path addLineToPoint:p];else[path moveToPoint:p];}[curve setStroke];[path stroke];}
 for(NSString *key in @[@"bypass",@"remove"]){CGRect b=WWPluginRect(g[key]);BOOL bypass=[key isEqualToString:@"bypass"];UIColor *fill=bypass&&WWFlag(g[@"bypassed"])?top:front;
  if(WWFlag(g[@"selected"]))fill=WWBlend(fill,.45,[UIColor colorWithRed:.18 green:.52 blue:.95 alpha:1.]);
  if(WWFlag(g[bypass?@"bypass_pressed":@"remove_pressed"]))fill=WWBlend(fill,.12,UIColor.blackColor);if(!gem){[fill setFill];UIRectFill(b);}[self label:[key isEqualToString:@"bypass"]?@"B":@"X" rect:b size:12*c centered:YES color:glyph];}
 [self label:g[@"title"] rect:CGRectMake(r.origin.x,r.origin.y,r.size.width,22*c) size:11*c centered:YES color:glyph];
 [self label:@"Drag edges / middle; click slopes (dB/oct)" rect:CGRectMake(r.origin.x,CGRectGetMaxY(r),r.size.width,24*c) size:9*c centered:YES color:glyph];
}
// Preserve Cardinal's five straight segments per rounded corner, rather than
// replacing its row outline with a different native control shape.
-(void)browserFace:(NSDictionary*)face{
 CGRect r=WWPluginRect(face[@"rect"]);if(r.size.width<=0||r.size.height<=0)return;
 double radius=fmin([face[@"radius"]doubleValue],fmin(r.size.width,r.size.height)*.5);
 NSArray *rgb=face[@"color"];[[UIColor colorWithRed:[rgb[0]doubleValue]/255. green:[rgb[1]doubleValue]/255. blue:[rgb[2]doubleValue]/255. alpha:[face[@"alpha"]doubleValue]]setFill];
 const double halfPi=1.57079632679,pi=3.14159265359;
 double corners[4][3]={{CGRectGetMaxX(r)-radius,CGRectGetMinY(r)+radius,-halfPi},{CGRectGetMaxX(r)-radius,CGRectGetMaxY(r)-radius,0.},{CGRectGetMinX(r)+radius,CGRectGetMaxY(r)-radius,halfPi},{CGRectGetMinX(r)+radius,CGRectGetMinY(r)+radius,pi}};
 UIBezierPath *path=[UIBezierPath bezierPath];
 for(int corner=0;corner<4;corner++)for(int segment=0;segment<=5;segment++){
  double angle=corners[corner][2]+segment*(halfPi/5.);CGPoint point=CGPointMake(corners[corner][0]+cos(angle)*radius,corners[corner][1]+sin(angle)*radius);
  if(corner==0&&segment==0)[path moveToPoint:point];else[path addLineToPoint:point];
 }
 [path closePath];[path fill];
}
-(void)surface:(NSDictionary *)p fonts:(NSArray *)fonts{
 CGContextRef c=UIGraphicsGetCurrentContext();
 for(NSDictionary *g in p[@"groups"]){
  CGContextSaveGState(c);CGContextClipToRect(c,WWPluginRect(g[@"clip"]));
  for(NSDictionary *face in WWList(g[@"browser_faces"]))[self browserFace:face];
  for(NSDictionary *graph in WWList(g[@"graphs"]))[self stock:graph];
  for(NSDictionary *l in g[@"labels"]){
   UIColor *color=WWFlag(l[@"selected"])?[UIColor colorWithRed:143./255. green:184./255. blue:1. alpha:1.]:WWColor(l[@"color"],255.,1.);
   if([l[@"icon"]isKindOfClass:NSArray.class]){
    CGRect icon=WWPluginRect(l[@"icon"]);if(icon.size.height>0.){
     id font=CFBridgingRelease(CTFontCreateCopyWithAttributes((__bridge CTFontRef)fonts[1],icon.size.height,NULL,NULL));CGSize size=WWMeasure(@"\uF102",font);
     WWDrawText(@"\uF102",font,color,CGPointMake(icon.origin.x,CGRectGetMidY(icon)-size.height*.5));
    }
   }
   [self label:l[@"text"] font:fonts[0] rect:WWPluginRect(l[@"rect"]) size:[l[@"size"]doubleValue] centered:WWFlag(l[@"center"]) color:color];
  }
  CGContextRestoreGState(c);
 }
}
-(void)range:(NSDictionary *)p viewport:(CGSize)viewport{
 for(NSDictionary *fade in p[@"fades"]){CGContextRef ctx=UIGraphicsGetCurrentContext();CGContextSaveGState(ctx);CGContextClipToRect(ctx,CGRectInset(WWPluginRect(fade[@"clip"]),0.,-5.));
  UIBezierPath *line=[UIBezierPath bezierPath];line.lineWidth=1.5;BOOL first=YES;
  for(NSArray *q in fade[@"points"]){CGPoint point=CGPointMake([q[0]doubleValue],[q[1]doubleValue]);if(first){[line moveToPoint:point];first=NO;}else{[line addLineToPoint:point];}}
  [[UIColor colorWithRed:1. green:.9 blue:.58 alpha:1.]setStroke];[line stroke];[[UIColor colorWithRed:1. green:.9 blue:.58 alpha:1.]setFill];for(NSArray *q in fade[@"handles"])UIRectFill(CGRectMake([q[0]doubleValue]-4.,[q[1]doubleValue]-4.,8.,8.));CGContextRestoreGState(ctx);
 }
 if(!p[@"rect"])return;
 CGRect r=WWPluginRect(p[@"rect"]);CGContextRef c=UIGraphicsGetCurrentContext();CGContextSaveGState(c);
 CGContextClipToRect(c,CGRectInset(r,-1.,0.));
 // Apply each exclusion separately: overlapping clips remain cut out, not XOR.
 for(NSArray *hole in p[@"holes"]){UIBezierPath *clip=[UIBezierPath bezierPathWithRect:CGRectMake(0.,0.,viewport.width,viewport.height)];clip.usesEvenOddFillRule=YES;[clip appendPath:[UIBezierPath bezierPathWithRect:WWPluginRect(hole)]];[clip addClip];}
 [[UIColor colorWithRed:(143./255.)*.35 green:(184./255.)*.35 blue:.35 alpha:1.]setFill];UIRectFill(r);
 [[UIColor colorWithRed:143./255. green:184./255. blue:1. alpha:1.]setFill];
 UIRectFill(CGRectMake(r.origin.x-1.,r.origin.y,2.,r.size.height));UIRectFill(CGRectMake(CGRectGetMaxX(r)-1.,r.origin.y,2.,r.size.height));
 CGContextRestoreGState(c);
}
-(void)cursor:(NSDictionary *)p fonts:(NSArray *)fonts{
 id font=CFBridgingRelease(CTFontCreateCopyWithAttributes((__bridge CTFontRef)fonts[0],21.6,NULL,NULL));
 CGSize size=WWMeasure(p[@"text"],font);double x=[p[@"position"][0]doubleValue],y=[p[@"position"][1]doubleValue];
 WWDrawText(p[@"text"],font,[UIColor colorWithRed:244./255. green:241./255. blue:232./255. alpha:1.],CGPointMake(x,y-(ceil(size.height)+1.)*.5));
}
-(void)picker:(NSDictionary *)p{
 CGRect r=WWPluginRect(p[@"rect"]);double depth=[p[@"bevel"]doubleValue];NSDictionary *colors=p[@"colors"];
 CGPoint a=CGPointMake(CGRectGetMinX(r),CGRectGetMinY(r)),b=CGPointMake(CGRectGetMaxX(r),CGRectGetMinY(r)),c=CGPointMake(CGRectGetMaxX(r),CGRectGetMaxY(r)),d=CGPointMake(CGRectGetMinX(r),CGRectGetMaxY(r));
 [WWColor(colors[@"front"],1.,1.)setFill];WWQuad(a,b,c,d);
 [WWColor(colors[@"top"],1.,1.)setFill];WWQuad(a,b,CGPointMake(b.x-depth,b.y-depth),CGPointMake(a.x-depth,a.y-depth));
 [WWColor(colors[@"side"],1.,1.)setFill];WWQuad(a,d,CGPointMake(d.x-depth,d.y-depth),CGPointMake(a.x-depth,a.y-depth));
 for(NSDictionary *row in p[@"rows"]){
  if(WWFlag(row[@"selected"])){[WWColor(colors[@"selected"],1.,1.)setFill];UIRectFill(WWPluginRect(row[@"rect"]));}
  else if(WWFlag(row[@"hovered"])){[WWColor(colors[@"hover"],1.,1.)setFill];UIRectFill(WWPluginRect(row[@"rect"]));}
  [WWColor(colors[@"text"],1.,1.)setFill];
  // Same per-label bounded storage and exact glyph quads as desktop/Cardinal.
  union{float alignment;unsigned char bytes[99999];}vertices;
  NSArray *origin=row[@"text_origin"];const char *label=[row[@"label"]UTF8String];
  int quads=stb_easy_font_print([origin[0]floatValue],[origin[1]floatValue],(char *)label,NULL,vertices.bytes,sizeof(vertices.bytes));
  const float *data=(const float *)vertices.bytes;
  for(int i=0;i<quads;++i){const float *v=data+i*16;WWQuad(CGPointMake(v[0],v[1]),CGPointMake(v[4],v[5]),CGPointMake(v[8],v[9]),CGPointMake(v[12],v[13]));}
 }
}
-(void)drawRect:(CGRect)dirty{
 (void)dirty;CGContextRef c=UIGraphicsGetCurrentContext();
 // Range is behind text; the existing CC popup is above all other UIOnly layers.
 for(NSNumber *key in @[@(WWWorkstationSelectionRange),@(WWWorkstationPluginSurface),@(WWWorkstationCursorLabel),@(WWWorkstationCcParameterPicker)]){
  NSDictionary *frame=self.channels[key];if(!frame)continue;
  NSArray *v=frame[@"viewport"];CGSize viewport=CGSizeMake([v[0]doubleValue],[v[1]doubleValue]);
  CGContextSaveGState(c);CGContextTranslateCTM(c,self.bounds.origin.x,self.bounds.origin.y);CGContextScaleCTM(c,self.bounds.size.width/viewport.width,self.bounds.size.height/viewport.height);
  switch(key.unsignedIntValue){
   case WWWorkstationPluginSurface:[self surface:frame[@"payload"] fonts:frame[@"fonts"]];break;
   case WWWorkstationSelectionRange:[self range:frame[@"payload"] viewport:viewport];break;
   case WWWorkstationCursorLabel:[self cursor:frame[@"payload"] fonts:frame[@"fonts"]];break;
   case WWWorkstationCcParameterPicker:[self picker:frame[@"payload"]];break;
  }
  CGContextRestoreGState(c);
 }
}
@end

// One latest payload per channel and at most one queued main-thread drain.
// Session generations discard outstanding work on hide/reattach. No UIView is
// touched off the main thread; no synchronous main-thread dispatch is used.
static char WWOverlayAssociation;
static __weak WWWorkstationOverlay *WWActiveOverlay;
static uint64_t WWGeneration;
static BOOL WWEnabled=NO,WWScheduled=NO;
static NSMutableDictionary<NSNumber *,NSDictionary *> *WWPending;
static NSString *WWFontPath;
static NSArray *WWFonts;
static NSArray *WWGetFonts(NSString *path){
 @synchronized(WWWorkstationOverlay.class){if([WWFontPath isEqualToString:path])return WWFonts;}
 id text=WWLoadFont(path,16.);
 NSString *iconPath=[[path stringByDeletingLastPathComponent]stringByAppendingPathComponent:@"../MaterialDesign/MaterialIconsRound-Regular.otf"];
 id icon=WWLoadFont(iconPath,21.6);if(!text||!icon)return nil;
 NSArray *fonts=@[text,icon];
 @synchronized(WWWorkstationOverlay.class){WWFontPath=[path copy];WWFonts=fonts;}return fonts;
}
static void WWDrain(uint64_t generation){
 NSCAssert(NSThread.isMainThread,@"UIOnly drain belongs to the main thread");NSDictionary *pending;
 @synchronized(WWWorkstationOverlay.class){
  if(generation!=WWGeneration||!WWEnabled)return;
  pending=[WWPending copy];[WWPending removeAllObjects];WWScheduled=NO;
 }
 WWWorkstationOverlay *view=WWActiveOverlay;if(!view)return;
 for(NSNumber *key in pending){NSDictionary *frame=pending[key];if(frame[@"payload"]==NSNull.null)[view.channels removeObjectForKey:key];else view.channels[key]=frame;}
 view.hidden=view.channels.count==0;[view setNeedsDisplay];
}
bool ww_ios_workstation_overlay_attach(void *parentView){
 if(!NSThread.isMainThread||!parentView)return false;
 UIView *parent=(__bridge UIView *)parentView;if(![parent isKindOfClass:UIView.class])return false;
 WWWorkstationOverlay *view=objc_getAssociatedObject(parent,&WWOverlayAssociation);
 if(!view){view=[[WWWorkstationOverlay alloc]initWithFrame:parent.bounds];[parent insertSubview:view atIndex:0];objc_setAssociatedObject(parent,&WWOverlayAssociation,view,OBJC_ASSOCIATION_RETAIN_NONATOMIC);}
 if(WWActiveOverlay&&WWActiveOverlay!=view){[WWActiveOverlay.channels removeAllObjects];WWActiveOverlay.hidden=YES;}
 view.frame=parent.bounds;view.bounds=CGRectMake(0.,0.,parent.bounds.size.width,parent.bounds.size.height);[view.channels removeAllObjects];view.hidden=YES;[view setNeedsDisplay];WWActiveOverlay=view;
 @synchronized(WWWorkstationOverlay.class){++WWGeneration;WWEnabled=YES;WWScheduled=NO;WWPending=[NSMutableDictionary new];}
 return true;
}
bool ww_ios_workstation_overlay_publish(uint32_t kind,const char *json,const char *fontPath,double width,double height){
 @autoreleasepool{
  if(kind>WWWorkstationCcParameterPicker||!json)return false;uint64_t generation;
  @synchronized(WWWorkstationOverlay.class){if(!WWEnabled)return false;generation=WWGeneration;}
  NSData *data=[NSData dataWithBytes:json length:strlen(json)];
  id payload=[NSJSONSerialization JSONObjectWithData:data options:NSJSONReadingFragmentsAllowed error:NULL];
  if(!WWValid(kind,payload))return false;
  if(kind==WWWorkstationCursorLabel&&payload!=NSNull.null&&[payload[@"text"]length]==0)payload=NSNull.null;
  if(kind==WWWorkstationCcParameterPicker&&payload!=NSNull.null){width=[payload[@"viewport"][0]doubleValue];height=[payload[@"viewport"][1]doubleValue];}
  if(payload!=NSNull.null&&(!isfinite(width)||!isfinite(height)||width<=0.||height<=0.))return false;
  NSArray *fonts=@[];
  if(payload!=NSNull.null&&(kind==WWWorkstationPluginSurface||kind==WWWorkstationCursorLabel)){
   if(!fontPath)return false;NSString *path=[NSString stringWithUTF8String:fontPath];if(!path)return false;
   fonts=WWGetFonts(path);if(!fonts)return false;
  }
  NSDictionary *frame=@{@"payload":payload,@"viewport":@[@(width),@(height)],@"fonts":fonts};BOOL schedule=NO;
  @synchronized(WWWorkstationOverlay.class){
   if(!WWEnabled||WWGeneration!=generation)return false;
   WWPending[@(kind)]=frame;if(!WWScheduled){WWScheduled=YES;schedule=YES;}
  }
  if(schedule)dispatch_async(dispatch_get_main_queue(),^{WWDrain(generation);});return true;
 }
}
void ww_ios_workstation_overlay_hide(void){
 __block uint64_t generation;
 @synchronized(WWWorkstationOverlay.class){generation=++WWGeneration;WWEnabled=NO;WWScheduled=NO;[WWPending removeAllObjects];}
 void(^clear)(void)=^{
  @synchronized(WWWorkstationOverlay.class){if(generation!=WWGeneration)return;}
  WWWorkstationOverlay *view=WWActiveOverlay;[view.channels removeAllObjects];view.hidden=YES;[view setNeedsDisplay];
 };
 if(NSThread.isMainThread)clear();else dispatch_async(dispatch_get_main_queue(),clear);
}
