// Isolated native controls/persistence and layout check. Compile with Cocoa.
#import <Cocoa/Cocoa.h>
#import "../../Render/PauseMenu.m"
static void require(BOOL condition,NSString *message) {if(!condition){NSLog(@"FAIL: %@",message);exit(1);}}
int main(int argc,const char **argv) {@autoreleasepool {
    require(NSProcessInfo.processInfo.environment[@"WORMWOOD_SETTINGS_DIRECTORY"].isAbsolutePath,@"isolated preferences required");
    [NSApplication sharedApplication];
    if(argc>1 && strcmp(argv[1],"--reload")==0) {
        require(fabs(ww_environment_sound_gain(2))<1e-6,@"wind mute survives process restart");
        require(fabs(ww_environment_sound_gain(12)-0.125)<1e-6,@"master and bird levels survive process restart");
        require(fabs(ww_environment_sound_gain(8)-0.35)<1e-6,@"water level survives process restart");
        puts("Sound preference reload passed.");return 0;
    }
    NSWindow *window=[[NSWindow alloc]initWithContentRect:NSMakeRect(0,0,640,780) styleMask:NSWindowStyleMaskTitled backing:NSBackingStoreBuffered defer:NO];
    ww_pause_menu((__bridge void *)window,true,0,-1);
    MirrorPauseMenu *menu=objc_getAssociatedObject(window,&pauseAssociation);
    require(menu!=nil && menu.sound.hidden,@"sound section starts collapsed");
    NSButton *open=nil;
    for(NSView *view in ((NSStackView *)menu.scroll.documentView).arrangedSubviews)if([view isKindOfClass:NSButton.class] && [((NSButton *)view).title isEqualToString:@"Sound settings"])open=(NSButton *)view;
    require(open!=nil,@"sound settings menu button exists");[open performClick:nil];
    require(!menu.sound.hidden,@"sound section opens");
    require(menu.soundSliders.count==7,@"all seven controls exist");
    NSButton *reset=nil;
    for(NSView *view in menu.sound.arrangedSubviews)if([view isKindOfClass:NSButton.class])reset=(NSButton *)view;
    require(reset!=nil,@"reset button exists");[reset performClick:nil];
    for(NSUInteger i=0;i<7;i++)require(menu.soundSliders[i].doubleValue==1.,@"reset restores previous levels");
    NSArray *values=@[@0.5,@0.,@0.25,@1.,@0.7,@1.,@1.];
    for(NSUInteger i=0;i<7;i++) {NSSlider *slider=menu.soundSliders[i];slider.doubleValue=[values[i] doubleValue];[slider sendAction:slider.action to:slider.target];}
    require(ww_environment_sound_gain(2)==0.,@"wind mute");
    for(uint32_t ch=12;ch<=15;ch++)require(fabs(ww_environment_sound_gain(ch)-0.125)<1e-6,@"all four birds share one level");
    require(fabs(ww_environment_sound_gain(8)-0.35)<1e-6,@"water and master multiply");
    require(ww_environment_sound_gain(17)==1.,@"workstation feedback stays independent");
    require(ww_environment_sound_gain(9)==0.5 && ww_environment_sound_gain(11)==0.5,@"ocean and depths master");
    [menu layoutSubtreeIfNeeded];[menu.scroll.documentView layoutSubtreeIfNeeded];
    require(menu.sound.bounds.size.width>300,@"usable slider width");
    for(NSView *view in menu.sound.arrangedSubviews)require(view.frame.size.width>300 && view.frame.size.height>0,@"controls have visible dimensions");
    NSString *output=NSProcessInfo.processInfo.environment[@"WORMWOOD_SOUND_SCREENSHOT"];
    if(output.length) {
        NSBitmapImageRep *image=[menu bitmapImageRepForCachingDisplayInRect:menu.bounds];
        [menu cacheDisplayInRect:menu.bounds toBitmapImageRep:image];
        require([[image representationUsingType:NSBitmapImageFileTypePNG properties:@{}] writeToFile:output atomically:YES],@"write panel screenshot");
    }
    ww_pause_menu((__bridge void *)window,false,0,-1);
    ww_pause_menu((__bridge void *)window,true,0,-1);
    menu=objc_getAssociatedObject(window,&pauseAssociation);
    require(menu.soundSliders[1].doubleValue==0 && menu.soundSliders[2].doubleValue==0.25,@"reopening menu retains values");
    ww_pause_menu((__bridge void *)window,false,0,-1);
    puts("Native sound controls, group gains, reset, menu reopen and layout passed.");
}return 0;}
