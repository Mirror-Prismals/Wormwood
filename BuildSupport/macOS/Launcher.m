// Mirror-only native launcher; the physics workshop is a separate inspection window.
#import <Cocoa/Cocoa.h>
#include <unistd.h>
static NSString *assets(void){return [NSBundle.mainBundle.resourcePath stringByAppendingPathComponent:@"EngineAssets"];}
static NSString *engine(void){return [NSBundle.mainBundle.bundlePath stringByAppendingPathComponent:@"Contents/MacOS/wormwood-engine"];}
static NSString *diagnostics(void){return [NSHomeDirectory() stringByAppendingPathComponent:@"Library/Application Support/MirrorPrismals/Diagnostics"];}
@interface Launcher:NSObject<NSApplicationDelegate,NSWindowDelegate>
@property NSWindow *window,*workshop;
@property NSPopUpButton *backend,*cases,*workshopBackend;
@property NSTextField *status,*workshopStatus,*details;
@property NSButton *play,*simulate,*stop,*workshopStop;
@property NSString *stopFile;
@property NSTask *task;
@property NSArray *catalog;
@property NSWindow *performanceWindow,*modsWindow;
@property NSMutableDictionary<NSString*,NSButton*> *modChoices;
@property NSButton *performanceEnabled,*benchmark,*nativeResolution;
@property NSTextField *performanceReadout;
@property NSTimer *performanceTimer;
@property NSString *sessionDiagnostics;
@property BOOL sessionMeasured;
@end
@implementation Launcher
-(void)prepareFullscreen:(NSWindow*)window{
 // Keep the launcher's controls centred at their designed size in fullscreen.
 NSView *controls=window.contentView;
 NSView *container=[[NSView alloc]initWithFrame:controls.frame];
 window.contentView=container;
 controls.autoresizingMask=NSViewMinXMargin|NSViewMaxXMargin|NSViewMinYMargin|NSViewMaxYMargin;
 [container addSubview:controls];
 window.styleMask|=NSWindowStyleMaskResizable;
 window.collectionBehavior|=NSWindowCollectionBehaviorFullScreenPrimary;
}
-(void)enterFullscreen:(NSWindow*)window{
 dispatch_async(dispatch_get_main_queue(),^{
  if(!(window.styleMask&NSWindowStyleMaskFullScreen))[window toggleFullScreen:nil];
 });
}
-(NSTextField*)label:(NSString*)text rect:(NSRect)rect window:(NSWindow*)window{
 NSTextField *l=[NSTextField wrappingLabelWithString:text];l.frame=rect;[window.contentView addSubview:l];return l;
}
-(NSPopUpButton*)apis:(NSRect)rect window:(NSWindow*)window{
 NSPopUpButton *p=[[NSPopUpButton alloc]initWithFrame:rect pullsDown:NO];[p addItemsWithTitles:@[@"WebGPU",@"Vulkan",@"OpenGL"]];[window.contentView addSubview:p];return p;
}
-(void)applicationDidFinishLaunching:(NSNotification*)n{
 self.window=[[NSWindow alloc]initWithContentRect:NSMakeRect(0,0,530,466) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable|NSWindowStyleMaskMiniaturizable backing:NSBackingStoreBuffered defer:NO];
 self.window.title=@"Mirror";self.window.delegate=self;self.window.releasedWhenClosed=NO;
 NSTextField *title=[self label:@"Mirror" rect:NSMakeRect(25,358,480,36) window:self.window];title.font=[NSFont boldSystemFontOfSize:26];
 [self label:@"Spawn theatre · audio workstation · physical landscape" rect:NSMakeRect(25,323,480,24) window:self.window];
 [self label:@"Graphics API" rect:NSMakeRect(25,276,145,24) window:self.window];self.backend=[self apis:NSMakeRect(170,275,330,28) window:self.window];
 [self label:@"WASD — move     Space — jump / ascend     Shift — sprint / descend\nMouse — look     Double-Space — walk / fly     Ctrl — fast flight\n1 Shovel · 2 Pickaxe · 3 Axe · 4 Bucket\nHold left click for force; release to work · Right click pours\nClick the theatre computer — open the audio workstation\nF — pick up / place phone     R — focus phone / reset (cheats only)\nEscape — release phone focus / pause     Cmd+Q — quit\n\nMaterial changes remain during this session." rect:NSMakeRect(25,109,480,150) window:self.window];
 self.status=[self label:@"Game and audio workstation development build." rect:NSMakeRect(25,64,330,34) window:self.window];self.status.font=[NSFont systemFontOfSize:11];
 self.play=[NSButton buttonWithTitle:@"Play Mirror" target:self action:@selector(playGame:)];self.play.frame=NSMakeRect(365,19,140,34);self.play.keyEquivalent=@"\r";[self.window.contentView addSubview:self.play];
 NSButton *w=[NSButton buttonWithTitle:@"Physics workshop…" target:self action:@selector(showWorkshop:)];w.frame=NSMakeRect(20,19,180,34);[self.window.contentView addSubview:w];
 self.stop=[NSButton buttonWithTitle:@"Stop session" target:self action:@selector(stopSession:)];self.stop.frame=NSMakeRect(365,60,140,30);self.stop.enabled=NO;[self.window.contentView addSubview:self.stop];
 NSButton *logs=[NSButton buttonWithTitle:@"Diagnostics" target:self action:@selector(openLogs:)];logs.frame=NSMakeRect(210,19,130,34);[self.window.contentView addSubview:logs];
 for(NSView *view in self.window.contentView.subviews){NSRect frame=view.frame;frame.origin.y+=46;view.frame=frame;}
 NSButton *audio=[NSButton buttonWithTitle:@"DAW only mode" target:self action:@selector(showWorkstation:)];audio.frame=NSMakeRect(20,19,155,34);[self.window.contentView addSubview:audio];
 self.nativeResolution=[NSButton checkboxWithTitle:@"Retina" target:nil action:nil];self.nativeResolution.frame=NSMakeRect(180,22,78,24);self.nativeResolution.toolTip=@"Use the display’s native pixel density for the workstation";[self.window.contentView addSubview:self.nativeResolution];
 NSButton *mods=[NSButton buttonWithTitle:@"Mods…" target:self action:@selector(showMods:)];mods.frame=NSMakeRect(265,19,90,34);[self.window.contentView addSubview:mods];
 NSButton *performance=[NSButton buttonWithTitle:@"Performance…" target:self action:@selector(showPerformance:)];performance.frame=NSMakeRect(365,19,140,34);[self.window.contentView addSubview:performance];
 NSMenu *menu=[NSMenu new];NSMenuItem *root=[NSMenuItem new];[menu addItem:root];NSMenu *app=[NSMenu new];NSMenuItem *workstation=[app addItemWithTitle:@"DAW only mode" action:@selector(showWorkstation:) keyEquivalent:@""];workstation.target=self;[app addItem:NSMenuItem.separatorItem];[app addItemWithTitle:@"Quit Mirror" action:@selector(terminate:) keyEquivalent:@"q"];root.submenu=app;NSApp.mainMenu=menu;
 [self prepareFullscreen:self.window];
 [self.window center];[self.window makeKeyAndOrderFront:nil];[NSApp activateIgnoringOtherApps:YES];
 [self enterFullscreen:self.window];
}
-(NSString*)userModsDirectory{return [NSHomeDirectory() stringByAppendingPathComponent:@"Library/Application Support/Mirror/Mods"];}
-(void)openModsFolder:(id)sender{
 NSString *path=[self userModsDirectory];NSError *error=nil;
 if(![NSFileManager.defaultManager createDirectoryAtPath:path withIntermediateDirectories:YES attributes:nil error:&error]){[self showStatus:error.localizedDescription];return;}
 [NSWorkspace.sharedWorkspace openURL:[NSURL fileURLWithPath:path]];
}
-(void)showMods:(id)sender{
 if(!self.modsWindow){
  self.modsWindow=[[NSWindow alloc]initWithContentRect:NSMakeRect(0,0,500,320) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable backing:NSBackingStoreBuffered defer:NO];self.modsWindow.title=@"Mirror — Mods";self.modsWindow.releasedWhenClosed=NO;
  [self label:@"Optional packages — select before starting a session" rect:NSMakeRect(20,273,460,28) window:self.modsWindow];
  [self label:@"Native mods run code on your computer. Install packages you trust." rect:NSMakeRect(20,239,460,30) window:self.modsWindow];
  NSScrollView *scroll=[[NSScrollView alloc]initWithFrame:NSMakeRect(20,64,460,170)];scroll.hasVerticalScroller=YES;[self.modsWindow.contentView addSubview:scroll];
  self.modChoices=[NSMutableDictionary new];NSMutableDictionary *installed=[NSMutableDictionary new];
  for(NSString *root in @[[assets() stringByAppendingPathComponent:@"Mods"],[self userModsDirectory]]){
   for(NSString *folder in [NSFileManager.defaultManager contentsOfDirectoryAtPath:root error:nil]){
    NSData *data=[NSData dataWithContentsOfFile:[[root stringByAppendingPathComponent:folder] stringByAppendingPathComponent:@"mod.json"]];
    NSDictionary *manifest=data?[NSJSONSerialization JSONObjectWithData:data options:0 error:nil]:nil;
    if([manifest isKindOfClass:NSDictionary.class]&&[manifest[@"id"] isKindOfClass:NSString.class]&&[manifest[@"name"] isKindOfClass:NSString.class])installed[manifest[@"id"]]=manifest;
   }
  }
  NSArray *ids=[[installed allKeys] sortedArrayUsingSelector:@selector(compare:)];NSView *list=[[NSView alloc]initWithFrame:NSMakeRect(0,0,440,MAX(170,ids.count*36))];scroll.documentView=list;
  NSUInteger index=0;for(NSString *identifier in ids){NSDictionary *manifest=installed[identifier];NSButton *choice=[NSButton checkboxWithTitle:[NSString stringWithFormat:@"%@ (%@)",manifest[@"name"],manifest[@"version"]?:@"?"] target:nil action:nil];choice.frame=NSMakeRect(4,list.frame.size.height-36*(++index),425,30);choice.state=NSControlStateValueOff;[list addSubview:choice];self.modChoices[identifier]=choice;}
  if(!ids.count){NSTextField *empty=[NSTextField labelWithString:@"No packages installed. Add a package folder, then reopen Mirror."];empty.frame=NSMakeRect(4,115,425,40);empty.lineBreakMode=NSLineBreakByWordWrapping;[list addSubview:empty];}
  NSButton *folder=[NSButton buttonWithTitle:@"Open Mods folder" target:self action:@selector(openModsFolder:)];folder.frame=NSMakeRect(20,18,170,32);[self.modsWindow.contentView addSubview:folder];
 }
 [self.modsWindow center];[self.modsWindow makeKeyAndOrderFront:nil];
}
-(void)showWorkshop:(id)sender{
 if(!self.workshop){
  self.workshop=[[NSWindow alloc]initWithContentRect:NSMakeRect(0,0,620,490) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable backing:NSBackingStoreBuffered defer:NO];self.workshop.title=@"Mirror — Physics Workshop";self.workshop.releasedWhenClosed=NO;
  NSTextField *t=[self label:@"OGC material experiments" rect:NSMakeRect(24,431,572,34) window:self.workshop];t.font=[NSFont boldSystemFontOfSize:22];
  [self label:@"Scenario" rect:NSMakeRect(24,385,130,24) window:self.workshop];
  self.cases=[[NSPopUpButton alloc]initWithFrame:NSMakeRect(155,384,440,28) pullsDown:NO];
  NSData *data=[NSData dataWithContentsOfFile:[assets() stringByAppendingPathComponent:@"SimulationWorkshop/author_cases.json"]];NSDictionary *catalog=data?[NSJSONSerialization JSONObjectWithData:data options:0 error:nil]:nil;self.catalog=catalog[@"cases"]?:@[];
  for(NSDictionary *c in self.catalog)[self.cases addItemWithTitle:c[@"description"]];self.cases.target=self;self.cases.action=@selector(caseChanged:);[self.workshop.contentView addSubview:self.cases];
  [self label:@"Graphics API" rect:NSMakeRect(24,340,130,24) window:self.workshop];self.workshopBackend=[self apis:NSMakeRect(155,339,440,28) window:self.workshop];[self.workshopBackend selectItemWithTitle:@"WebGPU"];
  self.details=[self label:@"" rect:NSMakeRect(24,205,572,120) window:self.workshop];
  [self label:@"P — pause / resume     N — one physics frame     R — reset\nWASD + Space / Shift — move camera     Right mouse — look\nEscape — close the simulation\n\nThe window title reports FPS, simulation speed and CPU costs. Simulation 100% means real time. A stopped case keeps its last state; reset remains available." rect:NSMakeRect(24,83,572,115) window:self.workshop];
  self.workshopStatus=[self label:@"CPU material constraints + OGC contact; no audio callback work." rect:NSMakeRect(24,48,572,30) window:self.workshop];self.workshopStatus.font=[NSFont systemFontOfSize:11];
  self.simulate=[NSButton buttonWithTitle:@"Run simulation" target:self action:@selector(runSimulation:)];self.simulate.frame=NSMakeRect(430,12,165,34);[self.workshop.contentView addSubview:self.simulate];
  self.workshopStop=[NSButton buttonWithTitle:@"Stop session" target:self action:@selector(stopSession:)];self.workshopStop.frame=NSMakeRect(230,12,160,34);self.workshopStop.enabled=self.task.running;[self.workshop.contentView addSubview:self.workshopStop];
  NSButton *paper=[NSButton buttonWithTitle:@"Author project / paper" target:self action:@selector(openPaper:)];paper.frame=NSMakeRect(19,12,200,34);[self.workshop.contentView addSubview:paper];
  [self caseChanged:nil];[self prepareFullscreen:self.workshop];[self.workshop center];
 }
 [self.workshop makeKeyAndOrderFront:nil];
 [self enterFullscreen:self.workshop];
}
-(void)showPerformance:(id)sender{
 if(!self.performanceWindow){
  self.performanceWindow=[[NSWindow alloc]initWithContentRect:NSMakeRect(0,0,650,440) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable|NSWindowStyleMaskResizable backing:NSBackingStoreBuffered defer:NO];
  self.performanceWindow.title=@"Mirror — Performance";self.performanceWindow.releasedWhenClosed=NO;
  NSTextField *heading=[self label:@"Performance checks" rect:NSMakeRect(24,384,600,32) window:self.performanceWindow];heading.font=[NSFont boldSystemFontOfSize:22];
  self.performanceEnabled=[NSButton checkboxWithTitle:@"Measure CPU, GPU and audio in the next session" target:nil action:nil];self.performanceEnabled.frame=NSMakeRect(24,345,600,30);[self.performanceWindow.contentView addSubview:self.performanceEnabled];
  [self label:@"The workstation benchmark repeats the same panel motion with four warmup frames and 32 measured frames. It uses the selected graphics API and current fullscreen quality." rect:NSMakeRect(24,279,600,58) window:self.performanceWindow];
  self.performanceReadout=[self label:@"Start a measured session or a workstation benchmark. Reports are saved separately for each run." rect:NSMakeRect(24,72,600,194) window:self.performanceWindow];self.performanceReadout.font=[NSFont monospacedSystemFontOfSize:12 weight:NSFontWeightRegular];
  self.benchmark=[NSButton buttonWithTitle:@"Benchmark workstation" target:self action:@selector(benchmarkWorkstation:)];self.benchmark.frame=NSMakeRect(379,20,247,34);[self.performanceWindow.contentView addSubview:self.benchmark];
  NSButton *report=[NSButton buttonWithTitle:@"Open report folder" target:self action:@selector(openPerformanceLogs:)];report.frame=NSMakeRect(19,20,190,34);[self.performanceWindow.contentView addSubview:report];
  self.performanceTimer=[NSTimer scheduledTimerWithTimeInterval:0.5 target:self selector:@selector(refreshPerformance:) userInfo:nil repeats:YES];
  [self.performanceWindow center];
 }
 [self.performanceWindow makeKeyAndOrderFront:nil];[self refreshPerformance:nil];
}
-(void)benchmarkWorkstation:(id)sender{
 self.performanceEnabled.state=NSControlStateValueOn;
 [self launch:@[@"--benchmark-workstation",self.backend.titleOfSelectedItem]];
}
-(void)openPerformanceLogs:(id)sender{
 NSString *path=self.sessionDiagnostics?:diagnostics();
 [NSFileManager.defaultManager createDirectoryAtPath:path withIntermediateDirectories:YES attributes:nil error:nil];
 [NSWorkspace.sharedWorkspace openURL:[NSURL fileURLWithPath:path]];
}
-(void)refreshPerformance:(id)sender{
 if(!self.performanceReadout||!self.sessionDiagnostics)return;
 if(!self.sessionMeasured){self.performanceReadout.stringValue=@"Performance measurement was disabled for this session. Enable it before launching the next session.";return;}
 NSString *path=[self.sessionDiagnostics stringByAppendingPathComponent:@"performance.json"];
 NSData *data=[NSData dataWithContentsOfFile:path];NSDictionary *report=data?[NSJSONSerialization JSONObjectWithData:data options:0 error:nil]:nil;
 NSMutableString *text=[NSMutableString stringWithString:self.task.running?@"Measuring current session…\n":@"Session measurements\n"];
 if(report){
  // The engine labels unsupported measurements explicitly rather than inferring
  // GPU cost from CPU waits. Detailed counters remain in the JSON report.
  NSDictionary *categories=report[@"categories"];
  NSDictionary *labels=@{@"GameLogic":@"Game logic",@"GameGraphics":@"Game graphics",@"DawGraphics":@"DAW graphics",@"Audio":@"Audio"};
  for(NSString *key in @[@"GameLogic",@"GameGraphics",@"DawGraphics",@"Audio"]){
   id category=categories[key];id cpu=[category isKindOfClass:NSDictionary.class]?category[@"cpu_per_core_percent"]:nil;
   if([cpu isKindOfClass:NSNumber.class])[text appendFormat:@"%@ CPU/core: %.1f%%\n",labels[key],[cpu doubleValue]];
  }
  for(NSString *api in @[@"WebGPU",@"Vulkan",@"OpenGL"]){
   NSDictionary *gpu=report[@"gpu"][api];
   id count=gpu[@"completed_samples"];
   if([count isKindOfClass:NSNumber.class]&&[count unsignedLongLongValue]>0){
    double seconds=0;BOOL valid=NO;
    for(NSDictionary *category in [gpu[@"categories"] allValues]){
     id time=category[@"sampled_gpu_seconds"];
     if([time isKindOfClass:NSNumber.class]){seconds+=[time doubleValue];valid=YES;}
    }
    if(valid)[text appendFormat:@"%@ GPU interval mean %.2f ms · missing %@\n",api,seconds*1000./[count doubleValue],gpu[@"incomplete_samples"]];
   }else if([gpu[@"supported"] isEqual:@NO])[text appendFormat:@"%@ GPU timing unavailable\n",api];
  }
  NSDictionary *audio=report[@"audio"];
  if([audio[@"deadline_percent"] isKindOfClass:NSNumber.class]&&[audio[@"peak_deadline_percent"] isKindOfClass:NSNumber.class])
   [text appendFormat:@"Audio deadline %.1f%% · peak %.1f%% · late %@\n",[audio[@"deadline_percent"] doubleValue],[audio[@"peak_deadline_percent"] doubleValue],audio[@"late_blocks"]];
  else [text appendString:@"Audio deadlines: unavailable / no completed callback\n"];
  if(!categories)[text appendString:@"CPU/audio report available in performance.json\n"];
 }else [text appendString:@"Waiting for the first measurement window…\n"];
 for(NSString *api in @[@"WebGPU",@"Vulkan",@"OpenGL"]){
  NSString *file=[self.sessionDiagnostics stringByAppendingPathComponent:[NSString stringWithFormat:@"workstation-benchmark-%@.json",api]];
  NSData *bytes=[NSData dataWithContentsOfFile:file];NSDictionary *result=bytes?[NSJSONSerialization JSONObjectWithData:bytes options:0 error:nil]:nil;
  NSDictionary *b=result[@"benchmark"];
  if(b){
   [text appendFormat:@"\n%@ benchmark: %@ (%@/%@ frames)\n",api,[b[@"complete"] boolValue]?@"complete":@"incomplete",b[@"measured_frames"],b[@"requested_measured_frames"]];
   if([b[@"frame_ms_p50"] isKindOfClass:NSNumber.class])[text appendFormat:@"Frame median %.2f ms · p95 %.2f ms\n",[b[@"frame_ms_p50"] doubleValue],[b[@"frame_ms_p95"] doubleValue]];
   NSDictionary *gpu=result[@"gpu_measurement"];
   if([gpu[@"gpu_ms_p50"] isKindOfClass:NSNumber.class]&&[gpu[@"gpu_ms_p95"] isKindOfClass:NSNumber.class])[text appendFormat:@"GPU median %.2f ms · p95 %.2f ms (%@ samples, %@)\n",[gpu[@"gpu_ms_p50"] doubleValue],[gpu[@"gpu_ms_p95"] doubleValue],gpu[@"measured_samples"],[gpu[@"complete"] isEqual:@YES]?@"complete":@"incomplete"];
   else [text appendString:@"GPU timing unavailable\n"];
  }
 }
 self.performanceReadout.stringValue=text;
 self.benchmark.enabled=!self.task.running;
}
-(void)caseChanged:(id)sender{
 if(self.cases.indexOfSelectedItem<0)return;
 NSDictionary *c=self.catalog[self.cases.indexOfSelectedItem];
 NSString *origin=c[@"paper_figure"]?[NSString stringWithFormat:@"Paper figure %@",c[@"paper_figure"]]:c[@"origin"]?:@"Mirror experiment";
 self.details.stringValue=[NSString stringWithFormat:@"%@\n%@",origin,c[@"differences"]?:@"Procedural reconstruction of this scenario family."];
}
-(void)stopSession:(id)sender{if(self.task.running){NSError *error=nil;if(![NSData.data writeToFile:self.stopFile options:NSDataWritingAtomic error:&error]){[self showStatus:error.localizedDescription];return;}self.stop.enabled=NO;self.workshopStop.enabled=NO;[self showStatus:@"Closing session and saving state…"];}}
-(void)playGame:(id)sender{[self launch:@[@"--interactive",@"planet_surface",self.backend.titleOfSelectedItem]];}
-(void)showWorkstation:(id)sender{[self launch:@[@"--workstation",self.backend.titleOfSelectedItem]];}
-(void)runSimulation:(id)sender{if(self.cases.indexOfSelectedItem>=0)[self launch:@[@"--workshop",self.catalog[self.cases.indexOfSelectedItem][@"id"],self.workshopBackend.titleOfSelectedItem]];}
-(void)openPaper:(id)sender{[NSWorkspace.sharedWorkspace openURL:[NSURL URLWithString:@"https://ankachan.github.io/Projects/OGC/"]];}
-(void)openLogs:(id)sender{[NSFileManager.defaultManager createDirectoryAtPath:diagnostics() withIntermediateDirectories:YES attributes:nil error:nil];[NSWorkspace.sharedWorkspace openURL:[NSURL fileURLWithPath:diagnostics()]];}
-(void)showStatus:(NSString*)s{self.status.stringValue=s;self.workshopStatus.stringValue=s;}
-(void)launch:(NSArray*)args{
 if(self.task.running){[self showStatus:@"Close the current session before starting another."];return;}
 NSError *error=nil;if(![NSFileManager.defaultManager createDirectoryAtPath:diagnostics() withIntermediateDirectories:YES attributes:nil error:&error]){[self showStatus:error.localizedDescription];return;}
 BOOL measure=self.performanceEnabled.state==NSControlStateValueOn||[args containsObject:@"--benchmark-workstation"];
 self.sessionMeasured=measure;
 self.sessionDiagnostics=measure?[diagnostics() stringByAppendingPathComponent:[@"performance-" stringByAppendingString:NSUUID.UUID.UUIDString]]:diagnostics();
 if(![NSFileManager.defaultManager createDirectoryAtPath:self.sessionDiagnostics withIntermediateDirectories:YES attributes:nil error:&error]){[self showStatus:error.localizedDescription];return;}
 NSString *log=[self.sessionDiagnostics stringByAppendingPathComponent:@"session.log"];if(![NSData.data writeToFile:log options:NSDataWritingAtomic error:&error]){[self showStatus:error.localizedDescription];return;}
 NSFileHandle *handle=[NSFileHandle fileHandleForWritingAtPath:log];NSTask *task=[NSTask new];task.executableURL=[NSURL fileURLWithPath:engine()];task.arguments=args;
 NSMutableDictionary *env=[NSProcessInfo.processInfo.environment mutableCopy];
 NSMutableArray *selected=[NSMutableArray new];for(NSString *identifier in self.modChoices){if(self.modChoices[identifier].state==NSControlStateValueOn)[selected addObject:identifier];}
 env[@"MIRROR_MODS"]=[[selected sortedArrayUsingSelector:@selector(compare:)] componentsJoinedByString:@","];env[@"MIRROR_MOD_PATH"]=[self userModsDirectory];env[@"WORMWOOD_WORKSTATION_RETINA"]=self.nativeResolution.state==NSControlStateValueOn?@"1":@"0";/* Author policy: launcher always uses disk artwork; optics is CLI research only. */env[@"WORMWOOD_WORKSTATION_REFERENCE_OPTICS"]=@"0";env[@"WORMWOOD_ASSET_ROOT"]=assets();BOOL game=[args containsObject:@"--interactive"];env[@"WORMWOOD_FULLSCREEN"]=@"1";if(game)env[@"WORMWOOD_GAME_WINDOW"]=@"1";env[@"WORMWOOD_OUTPUT_ROOT"]=self.sessionDiagnostics;env[@"WORMWOOD_PERFORMANCE"]=measure?@"1":@"0";self.stopFile=[self.sessionDiagnostics stringByAppendingPathComponent:[@"session-stop-" stringByAppendingString:NSUUID.UUID.UUIDString]];env[@"WORMWOOD_STOP_FILE"]=self.stopFile;task.environment=env;task.standardOutput=handle;task.standardError=handle;
 __weak Launcher *weak=self;task.terminationHandler=^(NSTask *finished){dispatch_async(dispatch_get_main_queue(),^{[handle closeFile];Launcher *owner=weak;if(!owner)return;owner.play.enabled=YES;owner.simulate.enabled=YES;owner.task=nil;owner.stop.enabled=NO;owner.workshopStop.enabled=NO;[NSFileManager.defaultManager removeItemAtPath:owner.stopFile error:nil];owner.stopFile=nil;[owner showStatus:finished.terminationStatus==0?@"Session ended; diagnostics saved.":@"Session failed. Open Diagnostics → session.log."];});};
 if(![task launchAndReturnError:&error]){[handle closeFile];[self showStatus:error.localizedDescription];return;}
 self.task=task;self.stop.enabled=YES;self.workshopStop.enabled=YES;self.play.enabled=NO;self.simulate.enabled=NO;[self showStatus:@"Session running. Close its window to return here."];
}
-(NSApplicationTerminateReply)applicationShouldTerminate:(NSApplication*)app{
 if(self.task.running){[self showStatus:@"Close the running session first to save and release its resources."];[self.window makeKeyAndOrderFront:nil];return NSTerminateCancel;}return NSTerminateNow;
}
-(BOOL)windowShouldClose:(NSWindow*)w{[NSApp terminate:nil];return NO;}
@end
int main(int argc,char**argv){@autoreleasepool{
 if(argc==3&&strcmp(argv[1],"--verify-bundle")==0){setenv("WORMWOOD_ASSET_ROOT",assets().fileSystemRepresentation,1);setenv("WORMWOOD_OUTPUT_ROOT",diagnostics().fileSystemRepresentation,1);execl(engine().fileSystemRepresentation,"wormwood","--workshop","cloth_sphere",argv[2],"--frames","8",NULL);return 1;}
 NSApplication *app=NSApplication.sharedApplication;[app setActivationPolicy:NSApplicationActivationPolicyRegular];Launcher *delegate=[Launcher new];app.delegate=delegate;[app run];}return 0;}
