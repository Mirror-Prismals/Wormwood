#import "../../Render/WorkstationControls.m"
#import "../../Render/WorkstationPluginSurface.m"
// Isolated AppKit composition and spring check; never launches the game.
// clang -fobjc-arc -framework Cocoa -framework CoreText -framework QuartzCore
//   BuildSupport/Workstation/check_drag_preview.m -o /tmp/check_drag_preview
// /tmp/check_drag_preview "$PWD" "$PWD/output/checks/drag-preview"
int main(int argc,const char **argv){if(argc!=3)return 1;@autoreleasepool{
 [NSApplication sharedApplication];
 NSString *root=[NSString stringWithUTF8String:argv[1]],*output=[NSString stringWithUTF8String:argv[2]];
 [[NSFileManager defaultManager]createDirectoryAtPath:output withIntermediateDirectories:YES attributes:nil error:nil];
 NSFont *font=[NSFont fontWithName:@"Helvetica" size:21.6];
 for(NSString *kind in @[@"audio",@"comp",@"matrix",@"tape",@"plugin",@"eq"]){
  NSDictionary *payload=[kind isEqual:@"eq"]?[NSJSONSerialization JSONObjectWithData:[@"{\"preview\":\"eq\",\"eq\":{\"gem_surface\":true,\"rect\":[0,36,230,132],\"plot\":[16,66,198,68],\"slopes\":[[10,144,103,18],[117,144,103,18]],\"bypass\":[84,0,28,28],\"remove\":[118,0,28,28],\"scale\":1,\"edges\":[78.98000562299544,179.49000281149773],\"gain_y\":100,\"zero_y\":100,\"title\":\"U SHAPING  +0.0 dB\",\"slope_labels\":[\"180 Hz / 12 dB\",\"6000 Hz / 12 dB\"],\"response\":[[16.0,100],[18.0625,100],[20.125,100],[22.1875,100],[24.25,100],[26.3125,100],[28.375,100],[30.4375,100],[32.5,100],[34.5625,100],[36.625,100],[38.6875,100],[40.75,100],[42.8125,100],[44.875,100],[46.9375,100],[49.0,100],[51.0625,100],[53.125,100],[55.1875,100],[57.25,100],[59.3125,100],[61.375,100],[63.4375,100],[65.5,100],[67.5625,100],[69.625,100],[71.6875,100],[73.75,100],[75.8125,100],[77.875,100],[79.9375,100],[82.0,100],[84.0625,100],[86.125,100],[88.1875,100],[90.25,100],[92.3125,100],[94.375,100],[96.4375,100],[98.5,100],[100.5625,100],[102.625,100],[104.6875,100],[106.75,100],[108.8125,100],[110.875,100],[112.9375,100],[115.0,100],[117.0625,100],[119.125,100],[121.1875,100],[123.25,100],[125.3125,100],[127.375,100],[129.4375,100],[131.5,100],[133.5625,100],[135.625,100],[137.6875,100],[139.75,100],[141.8125,100],[143.875,100],[145.9375,100],[148.0,100],[150.0625,100],[152.125,100],[154.1875,100],[156.25,100],[158.3125,100],[160.375,100],[162.4375,100],[164.5,100],[166.5625,100],[168.625,100],[170.6875,100],[172.75,100],[174.8125,100],[176.875,100],[178.9375,100],[181.0,100],[183.0625,100],[185.125,100],[187.1875,100],[189.25,100],[191.3125,100],[193.375,100],[195.4375,100],[197.5,100],[199.5625,100],[201.625,100],[203.6875,100],[205.75,100],[207.8125,100],[209.875,100],[211.9375,100],[214.0,100]]}}" dataUsingEncoding:NSUTF8StringEncoding] options:0 error:nil]:@{@"preview":kind};NSImage *im=WWDragCompose(root,payload,font);if(!im)return 2;
  NSBitmapImageRep *rep=[NSBitmapImageRep imageRepWithData:im.TIFFRepresentation];
  [[rep representationUsingType:NSBitmapImageFileTypePNG properties:@{}]writeToFile:[output stringByAppendingPathComponent:[kind stringByAppendingString:@".png"]] atomically:YES];
 }
 MirrorWorkstationCursorLabel *view=[MirrorWorkstationCursorLabel new];NSWindow *window=[[NSWindow alloc]initWithContentRect:NSMakeRect(0,0,500,400) styleMask:NSWindowStyleMaskBorderless backing:NSBackingStoreBuffered defer:NO];window.releasedWhenClosed=NO;[window.contentView addSubview:view];view.wantsLayer=YES;
 view.targetAngle=.22;view.lastPointer=CACurrentMediaTime();
 for(int i=0;i<30;i++){view.lastTick=CACurrentMediaTime()-.008;[view tick];}if(view.angle<.05||view.angle>.28)return 3;
 view.targetAngle=0;for(int i=0;i<300;i++){view.lastTick=CACurrentMediaTime()-.008;[view tick];}if(fabs(view.angle)>.001)return 4;if([view hitTest:NSZeroPoint]!=nil||view.acceptsFirstResponder)return 5;
 [view stopMotion];[window close];puts("artwork composition and damped rotation passed");
 }return 0;}
