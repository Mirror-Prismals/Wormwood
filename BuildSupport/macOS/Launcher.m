// Native development launcher. Engine behavior remains in contracted Rust Systems.
#import <Cocoa/Cocoa.h>
#include <unistd.h>
static NSString *assets(void) { return [NSBundle.mainBundle.resourcePath stringByAppendingPathComponent:@"EngineAssets"]; }
static NSString *engine(void) { return [NSBundle.mainBundle.bundlePath stringByAppendingPathComponent:@"Contents/MacOS/wormwood-engine"]; }
static NSString *diagnostics(void) { NSString *name=[NSBundle.mainBundle objectForInfoDictionaryKey:@"WormwoodDataDirectory"] ?: @"Wormwood"; return [[NSHomeDirectory() stringByAppendingPathComponent:@"Library/Application Support"] stringByAppendingPathComponent:[name stringByAppendingPathComponent:@"Diagnostics"]]; }
@interface Launcher : NSObject <NSApplicationDelegate, NSWindowDelegate>
@property NSWindow *window;
@property NSPopUpButton *backend;
@property NSButton *start;
@property NSPopUpButton *scene;
@property NSTextField *instructions;
@property NSTextField *status;
@property NSTask *task;
@end
@implementation Launcher
- (NSTextField *)label:(NSString *)text frame:(NSRect)frame {
    NSTextField *label=[NSTextField wrappingLabelWithString:text]; label.frame=frame;
    [self.window.contentView addSubview:label]; return label;
}
- (void)applicationDidFinishLaunching:(NSNotification *)notification {
    self.window=[[NSWindow alloc] initWithContentRect:NSMakeRect(0,0,460,430) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable|NSWindowStyleMaskMiniaturizable backing:NSBackingStoreBuffered defer:NO];
    self.window.title=[NSBundle.mainBundle objectForInfoDictionaryKey:@"CFBundleDisplayName"]; self.window.delegate=self; self.window.releasedWhenClosed=NO;
    NSTextField *title=[self label:self.window.title frame:NSMakeRect(24,378,412,30)];title.font=[NSFont boldSystemFontOfSize:24];
    [self label:@"Flight and audio foundation test" frame:NSMakeRect(24,350,412,22)];
    [self label:@"Graphics API" frame:NSMakeRect(24,308,130,24)];
    self.backend=[[NSPopUpButton alloc] initWithFrame:NSMakeRect(156,307,278,28) pullsDown:NO];
    [self.backend addItemsWithTitles:@[@"OpenGL",@"Vulkan",@"WebGPU"]];[self.window.contentView addSubview:self.backend];
    [self label:@"Test" frame:NSMakeRect(24,266,130,24)];
    self.scene=[[NSPopUpButton alloc] initWithFrame:NSMakeRect(156,265,278,28) pullsDown:NO];
    [self.scene addItemsWithTitles:@[[NSBundle.mainBundle objectForInfoDictionaryKey:@"EnvironmentLabel"],@"Cube + player flight",@"Triangle + audio"]];self.scene.target=self;self.scene.action=@selector(sceneChanged:);[self.window.contentView addSubview:self.scene];
    self.instructions=[self label:@"" frame:NSMakeRect(24,96,412,150)];[self sceneChanged:nil];
    self.status=[self label:@"Audio follows macOS default output. Change it in Control Center → Sound." frame:NSMakeRect(24,55,412,36)];self.status.font=[NSFont systemFontOfSize:11];
    self.start=[NSButton buttonWithTitle:@"Start session" target:self action:@selector(startSession:)];self.start.frame=NSMakeRect(289,16,147,32);self.start.keyEquivalent=@"\r";[self.window.contentView addSubview:self.start];
    NSButton *logs=[NSButton buttonWithTitle:@"Open diagnostics" target:self action:@selector(openLogs:)];logs.frame=NSMakeRect(18,16,158,32);[self.window.contentView addSubview:logs];
    NSMenu *menu=[NSMenu new];NSMenuItem *root=[NSMenuItem new];[menu addItem:root];NSMenu *appMenu=[NSMenu new];
    [appMenu addItemWithTitle:[NSString stringWithFormat:@"Quit %@",self.window.title] action:@selector(terminate:) keyEquivalent:@"q"];root.submenu=appMenu;NSApp.mainMenu=menu;
    [self.window center];[self.window makeKeyAndOrderFront:nil];[NSApp activateIgnoringOtherApps:YES];
}
- (void)sceneChanged:(id)sender {
    if(self.scene.indexOfSelectedItem==0){self.instructions.stringValue=[NSBundle.mainBundle objectForInfoDictionaryKey:@"EnvironmentControls"];return;}
    self.instructions.stringValue=self.scene.indexOfSelectedItem==1?@"W / A / S / D — fly\nSpace / Left Shift — up / down\nHold right mouse — look around\nR — reset camera\nQ or close the cube window — end the session\n\nFree flight: no gravity or collision. This test is silent.":@"A short tone starts automatically.\nSpace — replay once the tone finishes\nEscape — cancel the active tone\nQ or close the triangle window — end the session\n\nHolding Space should not keep replaying.";
}
- (void)openLogs:(id)sender {
    [NSFileManager.defaultManager createDirectoryAtPath:diagnostics() withIntermediateDirectories:YES attributes:nil error:nil];
    [NSWorkspace.sharedWorkspace openURL:[NSURL fileURLWithPath:diagnostics()]];
}
- (void)startSession:(id)sender {
    if (self.task.running) return;
    NSError *error=nil;
    if (![NSFileManager.defaultManager createDirectoryAtPath:diagnostics() withIntermediateDirectories:YES attributes:nil error:&error]) {self.status.stringValue=error.localizedDescription;return;}
    NSString *log=[diagnostics() stringByAppendingPathComponent:@"session.log"];
    if (![NSData.data writeToFile:log options:NSDataWritingAtomic error:&error]) {self.status.stringValue=error.localizedDescription;return;}
    NSFileHandle *handle=[NSFileHandle fileHandleForWritingAtPath:log];
    NSTask *task=[NSTask new];task.executableURL=[NSURL fileURLWithPath:engine()];task.arguments=@[@"--interactive",self.scene.indexOfSelectedItem==0?[NSBundle.mainBundle objectForInfoDictionaryKey:@"DefaultScene"]:(self.scene.indexOfSelectedItem==1?@"hello_cube":@"hello_triangle"),self.backend.titleOfSelectedItem];
    NSMutableDictionary *env=[NSProcessInfo.processInfo.environment mutableCopy];env[@"WORMWOOD_ASSET_ROOT"]=assets();env[@"WORMWOOD_OUTPUT_ROOT"]=diagnostics();task.environment=env;
    task.standardOutput=handle;task.standardError=handle;
    __weak Launcher *weakSelf=self;
    task.terminationHandler=^(NSTask *finished) {
        dispatch_async(dispatch_get_main_queue(),^{
            [handle closeFile];Launcher *owner=weakSelf;if (!owner) return;
            owner.start.enabled=YES;owner.backend.enabled=YES;owner.scene.enabled=YES;owner.task=nil;
            owner.status.stringValue=finished.terminationStatus==0?@"Session ended. Select another API or start again.":@"Session failed. Open diagnostics to read session.log.";
            [owner.window makeKeyAndOrderFront:nil];[NSApp activateIgnoringOtherApps:YES];
        });
    };
    if (![task launchAndReturnError:&error]) {[handle closeFile];self.status.stringValue=error.localizedDescription;return;}
    self.task=task;self.start.enabled=NO;self.backend.enabled=NO;self.scene.enabled=NO;
    self.status.stringValue=@"Session running. Use the test window. Close it before quitting Wormwood.";
}
- (NSApplicationTerminateReply)applicationShouldTerminate:(NSApplication *)sender {
    if (self.task.running) {
        [self.window makeKeyAndOrderFront:nil];
        self.status.stringValue=@"Close the test window first so its audio and graphics resources can be released.";
        return NSTerminateCancel;
    }
    return NSTerminateNow;
}
- (BOOL)windowShouldClose:(NSWindow *)sender { [NSApp terminate:nil];return NO; }
@end
int main(int argc,char **argv) {
    @autoreleasepool {
        // Exercises the packaged runtime and asset/output paths without UI automation.
        if (argc==3 && strcmp(argv[1],"--verify-bundle")==0) {
            setenv("WORMWOOD_ASSET_ROOT",assets().fileSystemRepresentation,1);
            setenv("WORMWOOD_OUTPUT_ROOT",diagnostics().fileSystemRepresentation,1);
            execl(engine().fileSystemRepresentation,"wormwood","--interactive","hello_triangle",argv[2],"--verify-session",NULL);
            perror("Wormwood packaged engine");return 1;
        }
        NSApplication *app=NSApplication.sharedApplication;[app setActivationPolicy:NSApplicationActivationPolicyRegular];
        Launcher *delegate=[Launcher new];app.delegate=delegate;[app run];
    }
    return 0;
}
