// Cardinal's CC parameter menu. Rust owns every hit and mutation; this overlay
// only draws the source bevel, selection colors and exact stb_easy_font quads.
#import <Cocoa/Cocoa.h>
#import <objc/runtime.h>
#include <math.h>
#include "CcStbEasyFont.h"

static BOOL WWCcArray(id value,NSUInteger count){
 if(![value isKindOfClass:NSArray.class]||[value count]!=count)return NO;
 for(id number in value)if(![number isKindOfClass:NSNumber.class]||!isfinite([number doubleValue]))return NO;
 return YES;
}
static NSRect WWCcRect(NSArray *r){return NSMakeRect([r[0] doubleValue],[r[1] doubleValue],[r[2] doubleValue],[r[3] doubleValue]);}
static NSColor *WWCcColor(NSArray *r){return [NSColor colorWithSRGBRed:[r[0] doubleValue] green:[r[1] doubleValue] blue:[r[2] doubleValue] alpha:1.];}
static void WWCcQuad(NSPoint a,NSPoint b,NSPoint c,NSPoint d){
 NSBezierPath *path=[NSBezierPath bezierPath];[path moveToPoint:a];[path lineToPoint:b];[path lineToPoint:c];[path lineToPoint:d];[path closePath];[path fill];
}
@interface MirrorCcParameterPicker:NSView
@property(nonatomic,strong)NSDictionary *payload;
@end
@implementation MirrorCcParameterPicker
-(BOOL)isFlipped{return YES;}
-(BOOL)isOpaque{return NO;}
-(BOOL)acceptsFirstResponder{return NO;}
-(NSView*)hitTest:(NSPoint)point{(void)point;return nil;}
-(void)drawRect:(NSRect)dirty{
 (void)dirty;NSDictionary *scene=self.payload;if(!scene)return;
 [NSGraphicsContext saveGraphicsState];
 NSArray *viewport=scene[@"viewport"];NSAffineTransform *scale=[NSAffineTransform transform];
 [scale scaleXBy:self.bounds.size.width/[viewport[0] doubleValue] yBy:self.bounds.size.height/[viewport[1] doubleValue]];[scale concat];
 NSRect rect=WWCcRect(scene[@"rect"]);double depth=[scene[@"bevel"] doubleValue];NSDictionary *colors=scene[@"colors"];
 NSPoint a=NSMakePoint(NSMinX(rect),NSMinY(rect)),b=NSMakePoint(NSMaxX(rect),NSMinY(rect));
 NSPoint c=NSMakePoint(NSMaxX(rect),NSMaxY(rect)),d=NSMakePoint(NSMinX(rect),NSMaxY(rect));
 [WWCcColor(colors[@"front"])setFill];WWCcQuad(a,b,c,d);
 [WWCcColor(colors[@"top"])setFill];WWCcQuad(a,b,NSMakePoint(b.x-depth,b.y-depth),NSMakePoint(a.x-depth,a.y-depth));
 [WWCcColor(colors[@"side"])setFill];WWCcQuad(a,d,NSMakePoint(d.x-depth,d.y-depth),NSMakePoint(a.x-depth,a.y-depth));
 for(NSDictionary *row in scene[@"rows"]){
  if([row[@"selected"]boolValue]){[WWCcColor(colors[@"selected"])setFill];NSRectFill(WWCcRect(row[@"rect"]));}
  else if([row[@"hovered"]boolValue]){[WWCcColor(colors[@"hover"])setFill];NSRectFill(WWCcRect(row[@"rect"]));}
  [WWCcColor(colors[@"text"])setFill];
  // Same bounded per-label quad buffer and coordinates as Cardinal pushText.
  union {float alignment;unsigned char bytes[99999];} vertices;
  NSArray *origin=row[@"text_origin"];const char *label=[row[@"label"] UTF8String];
  int quads=stb_easy_font_print([origin[0] floatValue],[origin[1] floatValue],(char*)label,NULL,vertices.bytes,sizeof(vertices.bytes));
  const float *data=(const float*)vertices.bytes;
  for(int i=0;i<quads;++i){const float *v=data+i*16;WWCcQuad(NSMakePoint(v[0],v[1]),NSMakePoint(v[4],v[5]),NSMakePoint(v[8],v[9]),NSMakePoint(v[12],v[13]));}
 }
 [NSGraphicsContext restoreGraphicsState];
}
@end

static char ccParameterPickerAssociation;
bool ww_workstation_cc_parameter_picker(void *nativeWindow,const char *payload){
 if(!nativeWindow||!payload)return false;
 NSWindow *window=(__bridge NSWindow*)nativeWindow;
 NSData *bytes=[[NSString stringWithUTF8String:payload]dataUsingEncoding:NSUTF8StringEncoding];if(!bytes)return false;
 id value=[NSJSONSerialization JSONObjectWithData:bytes options:NSJSONReadingFragmentsAllowed error:nil];
 MirrorCcParameterPicker *view=objc_getAssociatedObject(window,&ccParameterPickerAssociation);
 if(value==[NSNull null]){view.hidden=YES;return true;}
 if(![value isKindOfClass:NSDictionary.class]||!WWCcArray(value[@"viewport"],2)||!WWCcArray(value[@"rect"],4)
    ||![value[@"rows"]isKindOfClass:NSArray.class]||![value[@"colors"]isKindOfClass:NSDictionary.class]
    ||![value[@"bevel"]isKindOfClass:NSNumber.class]||!isfinite([value[@"bevel"]doubleValue]))return false;
 if([value[@"viewport"][0]doubleValue]<=0||[value[@"viewport"][1]doubleValue]<=0)return false;
 for(NSString *key in @[@"front",@"top",@"side",@"text",@"selected",@"hover"]){
  NSArray *rgb=value[@"colors"][key];if(!WWCcArray(rgb,3))return false;
  for(NSNumber *channel in rgb)if(channel.doubleValue<0||channel.doubleValue>1)return false;
 }
 for(id row in value[@"rows"])if(![row isKindOfClass:NSDictionary.class]||!WWCcArray(row[@"rect"],4)
    ||!WWCcArray(row[@"text_origin"],2)||![row[@"label"]isKindOfClass:NSString.class])return false;
 if(!view){view=[[MirrorCcParameterPicker alloc]initWithFrame:window.contentView.bounds];
  view.wantsLayer=YES;view.autoresizingMask=NSViewWidthSizable|NSViewHeightSizable;
  [window.contentView addSubview:view positioned:NSWindowAbove relativeTo:nil];
  objc_setAssociatedObject(window,&ccParameterPickerAssociation,view,OBJC_ASSOCIATION_RETAIN_NONATOMIC);
 }
 view.frame=window.contentView.bounds;view.payload=value;view.hidden=NO;view.needsDisplay=YES;return true;
}
