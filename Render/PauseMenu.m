#import <Cocoa/Cocoa.h>
#import <objc/runtime.h>

// Native controls sit above the selected GL/Metal-backed game view. The engine
// still owns pausing, cheat validation, safe noclip exit and session cleanup.
@interface MirrorPauseStack : NSStackView @end
@implementation MirrorPauseStack
-(BOOL)isFlipped {return YES;}
@end
@interface MirrorPauseClip : NSClipView @end
@implementation MirrorPauseClip
-(BOOL)isFlipped {return YES;}
@end
// Graphics preferences belong to the game, independently of the launcher's
// diagnostics directory. Commit both selections atomically.
static NSURL *graphicsFile(void) {
    // Isolated desktop diagnostics can exercise save/restart without editing
    // the player's preferences.
    NSString *directory=NSProcessInfo.processInfo.environment[@"WORMWOOD_SETTINGS_DIRECTORY"];
    if(directory.isAbsolutePath) return [[NSURL fileURLWithPath:directory isDirectory:YES] URLByAppendingPathComponent:@"graphics.plist"];
    NSURL *support=[[NSFileManager defaultManager] URLsForDirectory:NSApplicationSupportDirectory inDomains:NSUserDomainMask].firstObject;
    return [[support URLByAppendingPathComponent:@"MirrorPrismals" isDirectory:YES] URLByAppendingPathComponent:@"graphics.plist"];
}
static uint32_t cloudScale=1;
static BOOL activeNative, savedNative, restartRequested;
static void loadGraphics(void) {
    static dispatch_once_t once;
    dispatch_once(&once,^{
        NSDictionary *saved=[NSDictionary dictionaryWithContentsOfURL:graphicsFile()];
        NSInteger scale=[saved[@"cloudDivisor"] integerValue];
        cloudScale=(scale==2 || scale==4)?(uint32_t)scale:1;
        activeNative=savedNative=[saved[@"nativePixels"] boolValue];
    });
}
uint32_t ww_cloud_divisor(void) { loadGraphics();return cloudScale; }
bool ww_game_native_pixels(void) { loadGraphics();return activeNative; }
bool ww_game_restart_requested(void) { return restartRequested; }
static BOOL saveGraphics(uint32_t scale,BOOL native,NSError **error) {
    NSURL *file=graphicsFile();
    if(![[NSFileManager defaultManager] createDirectoryAtURL:file.URLByDeletingLastPathComponent withIntermediateDirectories:YES attributes:nil error:error])return NO;
    NSDictionary *values=@{@"cloudDivisor":@(scale),@"nativePixels":@(native)};
    NSData *data=[NSPropertyListSerialization dataWithPropertyList:values format:NSPropertyListBinaryFormat_v1_0 options:0 error:error];
    if(!data || ![data writeToURL:file options:NSDataWritingAtomic error:error])return NO;
    cloudScale=scale;savedNative=native;
    return YES;
}
// Sound preferences are independent of System enablement and graphics settings.
static float soundLevels[7]={1,1,1,1,1,1,1};
static NSArray<NSString *> *soundKeys(void) {return @[@"ambient",@"wind",@"birds",@"crickets",@"water",@"ocean",@"depths"];}
static NSURL *soundFile(void) {return [[graphicsFile() URLByDeletingLastPathComponent] URLByAppendingPathComponent:@"sound.plist"];}
static void loadSound(void) {
    static dispatch_once_t once;
    dispatch_once(&once,^{
        NSDictionary *saved=[NSDictionary dictionaryWithContentsOfURL:soundFile()];
        NSArray *keys=soundKeys();
        for(NSUInteger i=0;i<7;i++) {
            id value=saved[keys[i]];
            if([value isKindOfClass:NSNumber.class] && isfinite([value doubleValue]))soundLevels[i]=fmax(0.,fmin(1.,[value doubleValue]));
        }
    });
}
static BOOL saveSound(NSError **error) {
    NSURL *file=soundFile();
    if(![[NSFileManager defaultManager] createDirectoryAtURL:file.URLByDeletingLastPathComponent withIntermediateDirectories:YES attributes:nil error:error])return NO;
    NSMutableDictionary *values=[NSMutableDictionary new];NSArray *keys=soundKeys();
    for(NSUInteger i=0;i<7;i++)values[keys[i]]=@(soundLevels[i]);
    NSData *data=[NSPropertyListSerialization dataWithPropertyList:values format:NSPropertyListBinaryFormat_v1_0 options:0 error:error];
    return data && [data writeToURL:file options:NSDataWritingAtomic error:error];
}
// Read on the application/control thread only; audio callbacks receive routes.
float ww_environment_sound_gain(uint32_t channel) {
    loadSound();int slot=-1;
    switch(channel){case 2:slot=1;break;case 12:case 13:case 14:case 15:slot=2;break;case 10:slot=3;break;case 8:slot=4;break;case 9:slot=5;break;case 11:slot=6;break;default:return 1.;}
    return soundLevels[0]*soundLevels[slot];
}
@interface MirrorPauseMenu : NSView
@property(nonatomic,strong) NSButton *resumeButton, *cheatsButton, *flightButton, *noclipButton, *speakerButton, *nightVisionButton, *realTimeButton;
@property(nonatomic,strong) NSSlider *timeSlider;
@property(nonatomic,strong) NSTextField *timeLabel;
@property(nonatomic,strong) NSScrollView *scroll;
@property(nonatomic,strong) NSStackView *graphics,*sound;
@property(nonatomic,strong) NSMutableArray<NSSlider *> *soundSliders;
@property(nonatomic,strong) NSMutableArray<NSTextField *> *soundLabels;
@property(nonatomic,strong) NSTextField *soundStatus;
@property(nonatomic,strong) NSPopUpButton *cloudChoice,*displayChoice;
@property(nonatomic,strong) NSButton *restartButton,*applyButton;
@property(nonatomic,strong) NSTextField *graphicsStatus;
@property(nonatomic,strong) id eventMonitor;
@property(nonatomic) uint32_t flags, action;
@property(nonatomic) int32_t minutes;
@end

@implementation MirrorPauseMenu
-(NSButton *)button:(NSString *)title action:(SEL)action {
    NSButton *button=[NSButton buttonWithTitle:title target:self action:action];
    button.bezelStyle=NSBezelStyleRounded;
    button.controlSize=NSControlSizeLarge;
    button.font=[NSFont systemFontOfSize:16 weight:NSFontWeightMedium];
    [button.heightAnchor constraintGreaterThanOrEqualToConstant:34].active=YES;
    return button;
}
-(NSTextField *)label:(NSString *)text {
    NSTextField *label=[NSTextField wrappingLabelWithString:text];
    label.textColor=NSColor.labelColor;
    label.font=[NSFont systemFontOfSize:14];
    return label;
}
-(instancetype)initWithFrame:(NSRect)frame {
    if(!(self=[super initWithFrame:frame]))return nil;
    self.autoresizingMask=NSViewWidthSizable|NSViewHeightSizable;
    self.wantsLayer=YES;
    self.layer.backgroundColor=[NSColor colorWithWhite:0.025 alpha:0.95].CGColor;
    self.appearance=[NSAppearance appearanceNamed:NSAppearanceNameDarkAqua];

    self.scroll=[NSScrollView new];
    self.scroll.contentView=[MirrorPauseClip new];
    self.scroll.drawsBackground=NO;
    self.scroll.hasVerticalScroller=YES;
    self.scroll.autohidesScrollers=YES;
    [self addSubview:self.scroll];
    NSStackView *stack=[MirrorPauseStack new];
    stack.orientation=NSUserInterfaceLayoutOrientationVertical;
    stack.alignment=NSLayoutAttributeLeading;
    stack.spacing=10;
    stack.edgeInsets=NSEdgeInsetsMake(16,20,20,20);
    self.scroll.documentView=stack;
    stack.translatesAutoresizingMaskIntoConstraints=NO;
    [stack.widthAnchor constraintEqualToAnchor:self.scroll.contentView.widthAnchor].active=YES;
    [stack.topAnchor constraintEqualToAnchor:self.scroll.contentView.topAnchor].active=YES;
    [stack.leadingAnchor constraintEqualToAnchor:self.scroll.contentView.leadingAnchor].active=YES;

    NSTextField *title=[self label:@"Paused"];
    title.font=[NSFont systemFontOfSize:28 weight:NSFontWeightSemibold];
    [stack addArrangedSubview:title];
    NSButton *resume=[self button:@"Resume  ·  Esc" action:@selector(resume:)];
    self.resumeButton=resume;resume.keyEquivalent=@"\r";
    [stack addArrangedSubview:resume];
    [stack addArrangedSubview:[self button:@"Graphics settings" action:@selector(toggleGraphics:)]];
    self.graphics=[NSStackView new];
    self.graphics.orientation=NSUserInterfaceLayoutOrientationVertical;
    self.graphics.alignment=NSLayoutAttributeLeading;
    self.graphics.spacing=6;
    [stack addArrangedSubview:self.graphics];
    [self.graphics addArrangedSubview:[self label:@"Cloud resolution"]];
    self.cloudChoice=[NSPopUpButton new];
    [self.cloudChoice addItemsWithTitles:@[@"High · full resolution",@"Medium · half resolution",@"Low · quarter resolution"]];
    self.cloudChoice.accessibilityLabel=@"Cloud resolution";
    [self.graphics addArrangedSubview:self.cloudChoice];
    NSArray *args=NSProcessInfo.processInfo.arguments;
    BOOL webgpu=[args containsObject:@"WebGPU"];
    self.cloudChoice.enabled=webgpu;
    if(!webgpu)[self.graphics addArrangedSubview:[self label:@"Cloud resolution presets are available on WebGPU."]];
    [self.graphics addArrangedSubview:[self label:@"Fullscreen rendering resolution"]];
    self.displayChoice=[NSPopUpButton new];
    [self.displayChoice addItemsWithTitles:@[@"Standard · display points",@"Native · Retina pixels"]];
    self.displayChoice.accessibilityLabel=@"Fullscreen rendering resolution";
    [self.graphics addArrangedSubview:self.displayChoice];
    self.applyButton=[self button:@"Apply" action:@selector(applyGraphics:)];
    [self.graphics addArrangedSubview:self.applyButton];
    self.graphicsStatus=[self label:@""];
    [self.graphics addArrangedSubview:self.graphicsStatus];
    self.restartButton=[self button:@"Restart game now" action:@selector(restartGame:)];
    [self.graphics addArrangedSubview:self.restartButton];
    for(NSView *v in self.graphics.arrangedSubviews){
        [v.widthAnchor constraintEqualToAnchor:self.graphics.widthAnchor].active=YES;
    }
    loadGraphics();
    [self.cloudChoice selectItemAtIndex:cloudScale==4?2:cloudScale==2?1:0];
    [self.displayChoice selectItemAtIndex:savedNative?1:0];
    for(NSPopUpButton *choice in @[self.cloudChoice,self.displayChoice]){
        choice.target=self;choice.action=@selector(graphicsChanged:);
    }
    [self graphicsChanged:nil];
    self.graphics.hidden=YES;
    [stack addArrangedSubview:[self button:@"Sound settings" action:@selector(toggleSound:)]];
    self.sound=[NSStackView new];self.sound.orientation=NSUserInterfaceLayoutOrientationVertical;
    self.sound.alignment=NSLayoutAttributeLeading;self.sound.spacing=6;
    [stack addArrangedSubview:self.sound];
    self.soundSliders=[NSMutableArray new];self.soundLabels=[NSMutableArray new];
    loadSound();
    NSArray *soundNames=@[@"Ambient master",@"Wind",@"Birds",@"Insects / crickets",@"Flowing water",@"Ocean",@"Underwater ambience"];
    for(NSUInteger i=0;i<7;i++) {
        NSTextField *label=[self label:[NSString stringWithFormat:@"%@: %.0f%%",soundNames[i],100*soundLevels[i]]];
        [self.soundLabels addObject:label];[self.sound addArrangedSubview:label];
        NSSlider *slider=[NSSlider sliderWithValue:soundLevels[i] minValue:0 maxValue:1 target:self action:@selector(changeSound:)];
        slider.tag=i;slider.continuous=YES;slider.accessibilityLabel=soundNames[i];
        [self.soundSliders addObject:slider];[self.sound addArrangedSubview:slider];
    }
    [self.sound addArrangedSubview:[self button:@"Reset sound levels" action:@selector(resetSound:)]];
    self.soundStatus=[self label:@"Changes apply immediately and save for your next launch."];
    [self.sound addArrangedSubview:self.soundStatus];
    for(NSView *v in self.sound.arrangedSubviews)[v.widthAnchor constraintEqualToAnchor:self.sound.widthAnchor].active=YES;
    self.sound.hidden=YES;
    self.cheatsButton=[self button:@"Cheats: Off" action:@selector(toggleCheats:)];
    self.flightButton=[self button:@"Flight: Off" action:@selector(toggleFlight:)];
    self.noclipButton=[self button:@"Noclip: Off" action:@selector(toggleNoclip:)];
    self.speakerButton=[self button:@"Sound speakers: Off" action:@selector(toggleSpeakers:)];
    self.nightVisionButton=[self button:@"Night vision: Off" action:@selector(toggleNightVision:)];
    for(NSButton *b in @[self.cheatsButton,self.flightButton,self.noclipButton,self.speakerButton,self.nightVisionButton]) [stack addArrangedSubview:b];
    self.timeLabel=[self label:@"Time of day: Real time"];
    [stack addArrangedSubview:self.timeLabel];
    self.timeSlider=[NSSlider sliderWithValue:720 minValue:0 maxValue:1439 target:self action:@selector(changeTime:)];
    self.timeSlider.continuous=YES;
    self.timeSlider.accessibilityLabel=@"Time of day";
    [stack addArrangedSubview:self.timeSlider];
    self.realTimeButton=[self button:@"Use real time" action:@selector(useRealTime:)];
    [stack addArrangedSubview:self.realTimeButton];
    [stack addArrangedSubview:[self label:@"Changes apply when you resume. Noclip requires flight; disabling it inside solid ground returns you to a safe position."]];
    NSTextField *controls=[self label:@"Controls\nWASD — move    Mouse — look\nSpace — jump / fly up    Shift — sprint / fly down\nCtrl — fast flight    Double-Space — toggle flight with cheats\n1 Shovel · 2 Pickaxe · 3 Axe · 4 Bucket\nLeft mouse — dig with shovel or pickaxe; hold and release axe or bucket.\nRight mouse — refill a cut or pour    R — focus phone / reset session (cheats only)\nF8 — cheats    F9 — flight    F10 — noclip\nEsc — pause / resume    Cmd+Q — quit"];
    [stack addArrangedSubview:controls];
    [stack addArrangedSubview:[self button:@"Return to title" action:@selector(returnToTitle:)]];
    for(NSView *v in stack.arrangedSubviews) {
        [v.widthAnchor constraintEqualToAnchor:stack.widthAnchor constant:-40].active=YES;
    }
    return self;
}
-(void)layout {
    [super layout];
    CGFloat width=MIN(500.,MAX(0.,self.bounds.size.width-24.));
    self.scroll.frame=NSMakeRect((self.bounds.size.width-width)*0.5,12,width,MAX(0.,self.bounds.size.height-24.));
}
-(void)refresh {
    self.cheatsButton.title=(self.flags&1)?@"Cheats: On":@"Cheats: Off";
    self.flightButton.title=(self.flags&2)?@"Flight: On":@"Flight: Off";
    self.noclipButton.title=(self.flags&4)?@"Noclip: On":@"Noclip: Off";
    self.speakerButton.title=(self.flags&8)?@"Sound speakers: On":@"Sound speakers: Off";
    BOOL enabled=(self.flags&1)!=0;
    self.speakerButton.enabled=enabled;
    self.nightVisionButton.title=(self.flags&16)?@"Night vision: On":@"Night vision: Off";
    self.nightVisionButton.enabled=enabled;
    self.flightButton.enabled=self.noclipButton.enabled=enabled;
    self.timeSlider.enabled=self.realTimeButton.enabled=enabled;
    if(self.minutes<0) self.timeLabel.stringValue=@"Time of day: Real time";
    else {
        int h=self.minutes/60,m=self.minutes%60;
        self.timeLabel.stringValue=[NSString stringWithFormat:@"Time of day: %d:%02d %@",h%12?h%12:12,m,h<12?@"AM":@"PM"];
    }
}
-(void)toggleSound:(id)sender {self.sound.hidden=!self.sound.hidden;}
-(void)updateSoundLabels {
    for(NSUInteger i=0;i<7;i++) {
        NSString *name=self.soundSliders[i].accessibilityLabel;
        self.soundLabels[i].stringValue=[NSString stringWithFormat:@"%@: %.0f%%",name,100*soundLevels[i]];
    }
    NSError *error=nil;
    self.soundStatus.stringValue=saveSound(&error)?@"Changes apply immediately and save for your next launch.":[NSString stringWithFormat:@"Applied for this session. Could not save: %@",error.localizedDescription];
}
-(void)changeSound:(NSSlider *)slider {
    if(slider.tag<0 || slider.tag>=7)return;
    soundLevels[slider.tag]=fmax(0.,fmin(1.,slider.doubleValue));[self updateSoundLabels];
}
-(void)resetSound:(id)sender {
    for(NSUInteger i=0;i<7;i++){soundLevels[i]=1.;self.soundSliders[i].doubleValue=1.;}
    [self updateSoundLabels];
}
-(void)toggleGraphics:(id)sender { self.graphics.hidden=!self.graphics.hidden; }
-(uint32_t)selectedScale { return (uint32_t)(1u<<self.cloudChoice.indexOfSelectedItem); }
-(void)graphicsChanged:(id)sender {
    BOOL native=self.displayChoice.indexOfSelectedItem==1;
    BOOL changed=self.selectedScale!=cloudScale || native!=savedNative;
    self.applyButton.enabled=changed;
    self.restartButton.hidden=changed || savedNative==activeNative;
    if(changed && native!=activeNative) self.graphicsStatus.stringValue=@"Changing fullscreen resolution requires restarting the game.";
    else if(!changed && savedNative!=activeNative) self.graphicsStatus.stringValue=@"Settings saved. Restart now or on your next launch. Restarting starts a new game session and clears session terrain edits.";
    else self.graphicsStatus.stringValue=@"";
}
-(void)applyGraphics:(id)sender {
    NSError *error=nil;
    if(!saveGraphics(self.selectedScale,self.displayChoice.indexOfSelectedItem==1,&error)){
        self.graphicsStatus.stringValue=error.localizedDescription;return;
    }
    [self graphicsChanged:nil];
    if(savedNative==activeNative)self.graphicsStatus.stringValue=@"Settings applied.";
}
-(void)restartGame:(id)sender {
    if(savedNative==activeNative || !self.applyButton || self.applyButton.enabled)return;
    restartRequested=YES;self.action=2;
}
-(void)toggleCheats:(id)sender {
    self.flags=(self.flags&1)?0:(self.flags|1);
    if(!(self.flags&1))self.minutes=-1;
    [self refresh];
}
-(void)toggleFlight:(id)sender {self.flags=(self.flags&2)?(self.flags&~6u):(self.flags|2);[self refresh];}
-(void)toggleNightVision:(id)sender {self.flags^=16;[self refresh];}
-(void)toggleSpeakers:(id)sender {self.flags^=8;[self refresh];}
-(void)toggleNoclip:(id)sender {self.flags=(self.flags&4)?(self.flags&~4u):(self.flags|6);[self refresh];}
-(void)changeTime:(NSSlider *)slider {self.minutes=(int32_t)lround(slider.doubleValue);[self refresh];}
-(void)useRealTime:(id)sender {self.minutes=-1;[self refresh];}
-(void)resume:(id)sender {self.action=1;}
-(void)returnToTitle:(id)sender {self.action=2;}
-(void)stopMonitoring {
    if(self.eventMonitor){[NSEvent removeMonitor:self.eventMonitor];self.eventMonitor=nil;}
}
-(void)viewWillMoveToWindow:(NSWindow *)window {
    [self stopMonitoring];
    [super viewWillMoveToWindow:window];
    if(!window)return;
    __weak MirrorPauseMenu *weakSelf=self;
    self.eventMonitor=[NSEvent addLocalMonitorForEventsMatchingMask:
        NSEventMaskKeyDown|NSEventMaskKeyUp|NSEventMaskLeftMouseUp|NSEventMaskRightMouseUp
        handler:^NSEvent *(NSEvent *event) {
            MirrorPauseMenu *menu=weakSelf;
            if(!menu||event.window!=menu.window)return event;
            if(event.type==NSEventTypeKeyDown&&event.keyCode==53){
                if(!event.isARepeat)menu.action=1;
                return nil;
            }
            if(event.type==NSEventTypeKeyDown&&event.keyCode==12&&(event.modifierFlags&NSEventModifierFlagCommand)){
                menu.action=2;return nil;
            }
            // Release keys/buttons that were held when the GLFW view lost the
            // responder. Menu controls still receive their own mouse-up events.
            if(event.type==NSEventTypeKeyUp){[menu.window.contentView keyUp:event];return nil;}
            if(event.type==NSEventTypeLeftMouseUp)[menu.window.contentView mouseUp:event];
            if(event.type==NSEventTypeRightMouseUp)[menu.window.contentView rightMouseUp:event];
            return event;
        }];
}
-(void)dealloc { [self stopMonitoring]; }
@end

static char pauseAssociation;
void ww_pause_menu(void *nativeWindow,bool visible,uint32_t flags,int32_t minutes) {
    if(!nativeWindow)return;
    NSWindow *window=(__bridge NSWindow *)nativeWindow;
    MirrorPauseMenu *menu=objc_getAssociatedObject(window,&pauseAssociation);
    if(!visible){
        if(menu){[menu removeFromSuperview];objc_setAssociatedObject(window,&pauseAssociation,nil,OBJC_ASSOCIATION_RETAIN_NONATOMIC);[window makeFirstResponder:window.contentView];}
        return;
    }
    if(menu)return;
    menu=[[MirrorPauseMenu alloc]initWithFrame:window.contentView.bounds];
    menu.flags=flags&15;menu.minutes=minutes;
    menu.timeSlider.doubleValue=minutes<0?720:minutes;
    [menu refresh];
    objc_setAssociatedObject(window,&pauseAssociation,menu,OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    [window.contentView addSubview:menu positioned:NSWindowAbove relativeTo:nil];
    [menu layoutSubtreeIfNeeded];
    [menu.scroll.documentView scrollPoint:NSZeroPoint];
    [window makeFirstResponder:menu.resumeButton];
}
bool ww_pause_menu_input(void *nativeWindow,uint32_t *action,uint32_t *flags,int32_t *minutes) {
    if(!nativeWindow)return false;
    MirrorPauseMenu *menu=objc_getAssociatedObject((__bridge NSWindow *)nativeWindow,&pauseAssociation);
    if(!menu)return false;
    *action=menu.action;*flags=menu.flags;*minutes=menu.minutes;
    menu.action=0;
    return true;
}
