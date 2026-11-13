// Cardinal UIOnly lettering and original U-Shaping plot. All hit handling lives
// in Rust; this view is pass-through and never modifies the authored gem image.
#import <Cocoa/Cocoa.h>
#import <CoreText/CoreText.h>
#import <objc/runtime.h>
#import <math.h>
static NSRect WWPluginRect(NSArray *r){return NSMakeRect([r[0] doubleValue],[r[1] doubleValue],[r[2] doubleValue],[r[3] doubleValue]);}
@interface MirrorPluginSurface:NSView
@property(nonatomic,strong) NSDictionary *payload;
@property(nonatomic,strong) NSFont *font;
@property(nonatomic,strong) NSFont *iconFont;
@end
@implementation MirrorPluginSurface
-(BOOL)isFlipped{return YES;}
-(BOOL)isOpaque{return NO;}
-(BOOL)acceptsFirstResponder{return NO;}
-(NSView*)hitTest:(NSPoint)p{return nil;}
-(void)label:(NSString*)text rect:(NSRect)r size:(double)size centered:(BOOL)center color:(NSColor*)color{
 NSFont *font=CFBridgingRelease(CTFontCreateCopyWithAttributes((__bridge CTFontRef)self.font,size,NULL,NULL));
 NSDictionary *attributes=@{NSFontAttributeName:font,NSForegroundColorAttributeName:color};
 NSString *prefix=text;BOOL truncated=NO;
 while(prefix.length&&[(truncated?[prefix stringByAppendingString:@"..."]:prefix) sizeWithAttributes:attributes].width>r.size.width){
  NSRange last=[prefix rangeOfComposedCharacterSequenceAtIndex:prefix.length-1];prefix=[prefix substringToIndex:last.location];truncated=YES;
 }
 NSString *shown=truncated?[prefix stringByAppendingString:@"..."]:prefix;NSSize s=[shown sizeWithAttributes:attributes];
 [shown drawAtPoint:NSMakePoint(center?NSMidX(r)-s.width*.5:r.origin.x,NSMidY(r)-s.height*.5) withAttributes:attributes];
}
-(void)lineFrom:(NSPoint)a to:(NSPoint)b width:(double)width color:(NSColor*)color{
 [color setStroke];NSBezierPath *path=[NSBezierPath bezierPath];path.lineWidth=width;[path moveToPoint:a];[path lineToPoint:b];[path stroke];
}
-(void)stock:(NSDictionary*)g{
 BOOL gem=[g[@"gem_surface"] boolValue];
 double c=[g[@"scale"] doubleValue];NSRect r=WWPluginRect(g[@"rect"]),plot=WWPluginRect(g[@"plot"]);
 NSColor *front=[NSColor colorWithSRGBRed:230./255. green:224./255. blue:212./255. alpha:.85];
 NSColor *top=[NSColor colorWithSRGBRed:244./255. green:241./255. blue:232./255. alpha:1.];
 NSColor *side=[NSColor colorWithSRGBRed:207./255. green:198./255. blue:183./255. alpha:1.];
 NSColor *glyph=[NSColor colorWithSRGBRed:.15 green:.15 blue:.15 alpha:1.];
 NSColor *selected=[front blendedColorWithFraction:.45 ofColor:[NSColor colorWithSRGBRed:.18 green:.52 blue:.95 alpha:1.]];
 if(!gem){[side setFill];NSRectFill(NSOffsetRect(r,-8*c,-8*c));[([g[@"selected"] boolValue]?selected:front) setFill];NSRectFill(r);}
 [[NSColor colorWithSRGBRed:(207./255.)*.72 green:(198./255.)*.72 blue:(183./255.)*.72 alpha:1.]setFill];NSRectFill(NSInsetRect(plot,-6*c,-6*c));
 double low=[g[@"edges"][0] doubleValue],high=[g[@"edges"][1] doubleValue],gain=[g[@"gain_y"] doubleValue],zero=[g[@"zero_y"] doubleValue];
 [self lineFrom:NSMakePoint(NSMinX(plot),zero) to:NSMakePoint(NSMaxX(plot),zero) width:c color:side];
 NSColor *curve=[g[@"bypassed"] boolValue]?top:[NSColor colorWithSRGBRed:244./255. green:241./255. blue:232./255. alpha:1.];
 for(int i=0;i<2;i++){double x=i?high:low;[self lineFrom:NSMakePoint(x,NSMinY(plot)) to:NSMakePoint(x,NSMaxY(plot)) width:c color:top];
  [self lineFrom:NSMakePoint(x-4*c,(gain+zero)*.5) to:NSMakePoint(x+4*c,(gain+zero)*.5) width:5*c color:curve];
  NSRect button=WWPluginRect(g[@"slopes"][i]);if(!gem){[front setFill];NSRectFill(button);}[self label:g[@"slope_labels"][i] rect:button size:9*c centered:YES color:glyph];}
 [self lineFrom:NSMakePoint(low,gain) to:NSMakePoint(high,gain) width:c color:top];
 NSBezierPath *diamond=[NSBezierPath bezierPath];double middle=(low+high)*.5;[diamond moveToPoint:NSMakePoint(middle,gain-5*c)];[diamond lineToPoint:NSMakePoint(middle+5*c,gain)];[diamond lineToPoint:NSMakePoint(middle,gain+5*c)];[diamond lineToPoint:NSMakePoint(middle-5*c,gain)];[diamond closePath];[curve setFill];[diamond fill];
 NSArray *response=g[@"response"];if(response.count){NSBezierPath *path=[NSBezierPath bezierPath];path.lineWidth=2*c;for(NSUInteger i=0;i<response.count;i++){NSPoint p=NSMakePoint([response[i][0] doubleValue],[response[i][1] doubleValue]);if(i)[path lineToPoint:p];else[path moveToPoint:p];}[curve setStroke];[path stroke];}
 for(NSString *key in @[@"bypass",@"remove"]){NSRect b=WWPluginRect(g[key]);BOOL bypass=[key isEqualToString:@"bypass"];NSColor *fill=bypass&&[g[@"bypassed"] boolValue]?top:front;
  if([g[@"selected"]boolValue])fill=[fill blendedColorWithFraction:.45 ofColor:[NSColor colorWithSRGBRed:.18 green:.52 blue:.95 alpha:1.]];
  if([g[bypass?@"bypass_pressed":@"remove_pressed"]boolValue])fill=[fill blendedColorWithFraction:.12 ofColor:NSColor.blackColor];if(!gem){[fill setFill];NSRectFill(b);}[self label:[key isEqualToString:@"bypass"]?@"B":@"X" rect:b size:12*c centered:YES color:glyph];}
 [self label:g[@"title"] rect:NSMakeRect(r.origin.x,r.origin.y,r.size.width,22*c) size:11*c centered:YES color:glyph];
 [self label:@"Drag edges / middle; click slopes (dB/oct)" rect:NSMakeRect(r.origin.x,NSMaxY(r),r.size.width,24*c) size:9*c centered:YES color:glyph];
}
// Preserve Cardinal's five straight segments per rounded corner, rather than
// replacing its row outline with a different native control shape.
-(void)browserFace:(NSDictionary*)face{
 NSRect r=WWPluginRect(face[@"rect"]);if(r.size.width<=0||r.size.height<=0)return;
 double radius=fmin([face[@"radius"]doubleValue],fmin(r.size.width,r.size.height)*.5);
 NSArray *rgb=face[@"color"];[[NSColor colorWithSRGBRed:[rgb[0]doubleValue]/255. green:[rgb[1]doubleValue]/255. blue:[rgb[2]doubleValue]/255. alpha:[face[@"alpha"]doubleValue]]setFill];
 const double halfPi=1.57079632679,pi=3.14159265359;
 double corners[4][3]={{NSMaxX(r)-radius,NSMinY(r)+radius,-halfPi},{NSMaxX(r)-radius,NSMaxY(r)-radius,0.},{NSMinX(r)+radius,NSMaxY(r)-radius,halfPi},{NSMinX(r)+radius,NSMinY(r)+radius,pi}};
 NSBezierPath *path=[NSBezierPath bezierPath];
 for(int corner=0;corner<4;corner++)for(int segment=0;segment<=5;segment++){
  double angle=corners[corner][2]+segment*(halfPi/5.);NSPoint point=NSMakePoint(corners[corner][0]+cos(angle)*radius,corners[corner][1]+sin(angle)*radius);
  if(corner==0&&segment==0)[path moveToPoint:point];else[path lineToPoint:point];
 }
 [path closePath];[path fill];
}
-(void)drawRect:(NSRect)dirty{
 for(NSDictionary *g in self.payload[@"groups"]){
  [NSGraphicsContext saveGraphicsState];NSRectClip(WWPluginRect(g[@"clip"]));
  for(NSDictionary *face in g[@"browser_faces"])[self browserFace:face];
  for(NSDictionary *graph in g[@"graphs"])[self stock:graph];
  for(NSDictionary *l in g[@"labels"]){NSArray *rgb=l[@"color"];NSColor *color=[NSColor colorWithSRGBRed:[rgb[0] doubleValue]/255. green:[rgb[1] doubleValue]/255. blue:[rgb[2] doubleValue]/255. alpha:1.];
   if([l[@"selected"] boolValue])color=[NSColor colorWithSRGBRed:143./255. green:184./255. blue:1. alpha:1.];
   if([l[@"icon"] isKindOfClass:NSArray.class]){NSRect icon=WWPluginRect(l[@"icon"]);NSFont *font=CFBridgingRelease(CTFontCreateCopyWithAttributes((__bridge CTFontRef)self.iconFont,icon.size.height,NULL,NULL));
    NSAttributedString *glyph=[[NSAttributedString alloc]initWithString:@"\uF102" attributes:@{NSFontAttributeName:font,NSForegroundColorAttributeName:color}];[glyph drawAtPoint:NSMakePoint(icon.origin.x,NSMidY(icon)-glyph.size.height*.5)];}
   [self label:l[@"text"] rect:WWPluginRect(l[@"rect"]) size:[l[@"size"] doubleValue] centered:[l[@"center"] boolValue] color:color];}
  [NSGraphicsContext restoreGraphicsState];
 }
}
@end
static char pluginSurfaceAssociation;
bool ww_workstation_plugin_surface(void *nativeWindow,const char *payload,const char *fontPath){
 if(!nativeWindow||!payload)return false;NSWindow *window=(__bridge NSWindow*)nativeWindow;
 id value=[NSJSONSerialization JSONObjectWithData:[[NSString stringWithUTF8String:payload] dataUsingEncoding:NSUTF8StringEncoding] options:NSJSONReadingFragmentsAllowed error:nil];
 MirrorPluginSurface *view=objc_getAssociatedObject(window,&pluginSurfaceAssociation);
 if(value==[NSNull null]){view.hidden=YES;return true;}
 if(![value isKindOfClass:NSDictionary.class])return false;
 if(!view){NSData *data=[NSData dataWithContentsOfFile:[NSString stringWithUTF8String:fontPath]];if(!data)return false;
  CGDataProviderRef provider=CGDataProviderCreateWithCFData((__bridge CFDataRef)data);CGFontRef font=provider?CGFontCreateWithDataProvider(provider):NULL;if(provider)CGDataProviderRelease(provider);if(!font)return false;
  CTFontRef ct=CTFontCreateWithGraphicsFont(font,16,NULL,NULL);CGFontRelease(font);if(!ct)return false;
  view=[[MirrorPluginSurface alloc]initWithFrame:window.contentView.bounds];view.font=CFBridgingRelease(ct);
  NSString *iconPath=[[[NSString stringWithUTF8String:fontPath]stringByDeletingLastPathComponent]stringByAppendingPathComponent:@"../MaterialDesign/MaterialIconsRound-Regular.otf"];
  NSData *iconData=[NSData dataWithContentsOfFile:iconPath];if(!iconData)return false;
  CGDataProviderRef ip=CGDataProviderCreateWithCFData((__bridge CFDataRef)iconData);CGFontRef ig=ip?CGFontCreateWithDataProvider(ip):NULL;if(ip)CGDataProviderRelease(ip);if(!ig)return false;
  CTFontRef it=CTFontCreateWithGraphicsFont(ig,21.6,NULL,NULL);CGFontRelease(ig);if(!it)return false;view.iconFont=CFBridgingRelease(it);view.wantsLayer=YES;view.autoresizingMask=NSViewWidthSizable|NSViewHeightSizable;
  [window.contentView addSubview:view positioned:NSWindowBelow relativeTo:nil];objc_setAssociatedObject(window,&pluginSurfaceAssociation,view,OBJC_ASSOCIATION_RETAIN_NONATOMIC);}
 view.frame=window.contentView.bounds;view.payload=value;view.hidden=NO;view.needsDisplay=YES;return true;
}

// Cached drag artwork uses the identical plot and labels as the inserted EQ.
void ww_workstation_drag_eq(NSFont *font,NSDictionary *graph){
 MirrorPluginSurface *surface=[MirrorPluginSurface new];surface.font=font;[surface stock:graph];
}
