#import <UIKit/UIKit.h>
#import "WorkstationOverlay.h"
#import "../../BuildSupport/iOS/SessionLimits.h"
#include <pthread.h>
#include <stdbool.h>
#include <stdint.h>
#include <math.h>
extern void ww_ios_attach(void *controller);
extern void ww_ios_active(bool active);
extern int wormwood_ios_run_game(const char *,const char *,const char *);
extern int wormwood_ios_run_soil_workshop(const char *,const char *,const char *);
extern int wormwood_ios_run_material_workshop(const char *,const char *,const char *,const char *);
extern void ww_workshop_action(uint32_t action);
extern void ww_workstation_panel_action(uint32_t edge);
extern void ww_workstation_return_to_game(void);
extern void ww_touch_event(uint64_t,uint32_t,double,double,double,double);
extern void ww_mobile_workstation_touch(uint64_t,uint32_t,double,double,double,double);
extern void ww_touch_reset(void);
extern void ww_touch_begin_session(void);
extern void ww_touch_pause_request(void);
extern void ww_touch_cheats(uint32_t flags);
extern void ww_touch_time_override(int32_t minutes);
extern void ww_touch_work(bool held);
extern void ww_touch_put_down(void);
static BOOL active=YES,stopRequested=NO;
static __weak UIView *controls;
static void onMain(dispatch_block_t b){if(NSThread.isMainThread)b();else dispatch_sync(dispatch_get_main_queue(),b);}
bool ww_ios_is_active(void){__block BOOL value;onMain(^{value=active;});return value;}
bool ww_ios_stop_requested(void){__block BOOL value;onMain(^{value=stopRequested;});return value;}
void ww_ios_request_close(void){onMain(^{stopRequested=YES;ww_touch_reset();});}
void ww_ios_bring_controls(void){if(controls)[controls.superview bringSubviewToFront:controls];}
void ww_ios_caption(const char *text);

@class GameController;
static __weak GameController *currentController;
extern void ww_touch_tool(uint32_t slot);
@interface TutorialOverlay:UIView
@end
@implementation TutorialOverlay
// Instruction text and highlights do not swallow movement/look touches.
- (UIView *)hitTest:(CGPoint)point withEvent:(UIEvent *)event {
 UIView *hit=[super hitTest:point withEvent:event];
 for(UIView *v=hit;v&&v!=self;v=v.superview)if([v isKindOfClass:UIButton.class])return hit;
 return nil;
}
@end
@interface MobileControls:UIView
@property(nonatomic,strong)NSMutableDictionary<NSNumber *,NSMutableDictionary *> *gestures;
@end
@interface GameController:UIViewController
@property(nonatomic)BOOL running,paused,replayAfterResume;
@property(nonatomic)BOOL workshopMode,workshopPaused,workstationMode;
@property(nonatomic)BOOL embeddedWorkstation;
@property(nonatomic)BOOL phoneHeld,phoneFocused;
@property(nonatomic)CGPoint workstationPanStart;
@property(nonatomic)NSUInteger workstationPanTouches;
@property(nonatomic)CGFloat workstationScale,workstationPinchStart;
@property(nonatomic)uint32_t controlFlags;
@property(nonatomic)NSInteger tutorialStep;
@property(nonatomic)NSInteger timeOverrideMinutes;
@property(nonatomic,strong)UIView *menu,*pausePanel,*highlight;
@property(nonatomic,strong)MobileControls *input;
@property(nonatomic,strong)UILabel *status;
@property(nonatomic,strong)TutorialOverlay *tutorial;
@property(nonatomic,strong)UIButton *cheatsButton,*flightButton,*noclipButton;
@property(nonatomic,strong)UIButton *realTimeButton;
@property(nonatomic,strong)UISlider *timeSlider;
@property(nonatomic,strong)UILabel *timeLabel;
@property(nonatomic,strong)UIStackView *toolPanel;
@property(nonatomic,strong)UILabel *digStatus;
@property(nonatomic,strong)UIButton *workButton,*putDownButton;
@property(nonatomic,strong)UISegmentedControl *materialTools;
@property(nonatomic,strong)UIStackView *workshopPanel;
@property(nonatomic,strong)UISegmentedControl *workshopBackend;
@property(nonatomic,strong)UISegmentedControl *workshopForceMode;
@property(nonatomic,strong)UILabel *workshopStatus;
@property(nonatomic,strong)UIButton *workshopPause,*workshopStep;
- (void)noteControlAction:(NSInteger)action;
- (void)applyPause:(BOOL)paused;
- (void)receiveControlState:(uint32_t)flags;
- (void)receivePhoneHeld:(BOOL)held focused:(BOOL)focused;
@end
@implementation MobileControls
- (instancetype)initWithFrame:(CGRect)frame {if((self=[super initWithFrame:frame]))self.gestures=[NSMutableDictionary new];return self;}
- (void)send:(NSSet<UITouch *> *)touches phase:(uint32_t)phase {
 if(currentController.workstationMode){
  for(UITouch *t in touches){CGPoint p=[t locationInView:self];ww_mobile_workstation_touch((uint64_t)(uintptr_t)(__bridge void *)t,phase,p.x,p.y,self.bounds.size.width,self.bounds.size.height);}
  return;
 }
 for(UITouch *t in touches){
  CGPoint p=[t locationInView:self];NSNumber *key=@((uintptr_t)(__bridge void *)t);
  if(phase==0)self.gestures[key]=[@{@"x":@(p.x),@"y":@(p.y),@"time":@(t.timestamp),@"action":@(-1)} mutableCopy];
  NSMutableDictionary *start=self.gestures[key];
  ww_touch_event(key.unsignedLongLongValue,phase,p.x,p.y,self.bounds.size.width,self.bounds.size.height);
  if(!currentController.paused&&start){
   CGFloat dx=p.x-[start[@"x"] doubleValue],dy=p.y-[start[@"y"] doubleValue];
   CGFloat corner=MIN(80.,MIN(self.bounds.size.width,self.bounds.size.height)*.18);
   BOOL jump=[start[@"x"] doubleValue]>self.bounds.size.width-corner&&[start[@"y"] doubleValue]>self.bounds.size.height-corner;
   BOOL pause=[start[@"x"] doubleValue]>self.bounds.size.width-corner&&[start[@"y"] doubleValue]<corner;
   if(phase==1&&!jump&&!pause&&hypot(dx,dy)>28.){
    NSInteger action=[start[@"action"] integerValue];
    if(action<0){action=[start[@"x"] doubleValue]<self.bounds.size.width*.5?0:1;start[@"action"]=@(action);}
    [currentController noteControlAction:action];
   }
   if(phase==2&&jump&&hypot(dx,dy)<12.)[currentController noteControlAction:2];
  }
  if(phase>=2)[self.gestures removeObjectForKey:key];
 }
}
- (void)touchesBegan:(NSSet *)t withEvent:(UIEvent *)e{[self send:t phase:0];}
- (void)touchesMoved:(NSSet *)t withEvent:(UIEvent *)e{[self send:t phase:1];}
- (void)touchesEnded:(NSSet *)t withEvent:(UIEvent *)e{[self send:t phase:2];}
- (void)touchesCancelled:(NSSet *)t withEvent:(UIEvent *)e{[self send:t phase:3];}
@end
@implementation GameController
- (BOOL)prefersStatusBarHidden{return YES;}
- (UIRectEdge)preferredScreenEdgesDeferringSystemGestures{return UIRectEdgeAll;}
- (UIButton *)button:(NSString *)title action:(SEL)action {
 UIButton *b=[UIButton buttonWithType:UIButtonTypeSystem];[b setTitle:title forState:UIControlStateNormal];
 b.titleLabel.font=[UIFont preferredFontForTextStyle:UIFontTextStyleBody];b.titleLabel.numberOfLines=0;
 [b.heightAnchor constraintGreaterThanOrEqualToConstant:44].active=YES;
 [b addTarget:self action:action forControlEvents:UIControlEventTouchUpInside];return b;
}
- (UILabel *)label:(NSString *)text {
 UILabel *l=[UILabel new];l.text=text;l.numberOfLines=0;l.textColor=UIColor.whiteColor;
 l.font=[UIFont preferredFontForTextStyle:UIFontTextStyleBody];l.adjustsFontForContentSizeCategory=YES;return l;
}
- (void)viewDidLoad {
 [super viewDidLoad];currentController=self;self.view.backgroundColor=UIColor.blackColor;ww_ios_attach((__bridge void *)self);
 self.menu=[UIView new];self.menu.translatesAutoresizingMaskIntoConstraints=NO;[self.view addSubview:self.menu];
 [NSLayoutConstraint activateConstraints:@[[self.menu.leadingAnchor constraintEqualToAnchor:self.view.safeAreaLayoutGuide.leadingAnchor],[self.menu.trailingAnchor constraintEqualToAnchor:self.view.safeAreaLayoutGuide.trailingAnchor],[self.menu.topAnchor constraintEqualToAnchor:self.view.safeAreaLayoutGuide.topAnchor],[self.menu.bottomAnchor constraintEqualToAnchor:self.view.safeAreaLayoutGuide.bottomAnchor]]];
 UIScrollView *menuScroll=[UIScrollView new];menuScroll.translatesAutoresizingMaskIntoConstraints=NO;[self.menu addSubview:menuScroll];
 [NSLayoutConstraint activateConstraints:@[[menuScroll.leadingAnchor constraintEqualToAnchor:self.menu.leadingAnchor],[menuScroll.trailingAnchor constraintEqualToAnchor:self.menu.trailingAnchor],[menuScroll.topAnchor constraintEqualToAnchor:self.menu.topAnchor],[menuScroll.bottomAnchor constraintEqualToAnchor:self.menu.bottomAnchor]]];
 UIStackView *stack=[UIStackView new];stack.axis=UILayoutConstraintAxisVertical;stack.spacing=8;stack.translatesAutoresizingMaskIntoConstraints=NO;[menuScroll addSubview:stack];
 [NSLayoutConstraint activateConstraints:@[[stack.centerXAnchor constraintEqualToAnchor:menuScroll.contentLayoutGuide.centerXAnchor],[stack.widthAnchor constraintEqualToAnchor:menuScroll.frameLayoutGuide.widthAnchor multiplier:.9],[stack.topAnchor constraintEqualToAnchor:menuScroll.contentLayoutGuide.topAnchor constant:8],[stack.bottomAnchor constraintEqualToAnchor:menuScroll.contentLayoutGuide.bottomAnchor constant:-8],[menuScroll.contentLayoutGuide.widthAnchor constraintEqualToAnchor:menuScroll.frameLayoutGuide.widthAnchor]]];
 self.status=[self label:[self version]];self.status.textAlignment=NSTextAlignmentCenter;[stack addArrangedSubview:self.status];
 for(NSString *api in @[@"WebGPU",@"Vulkan",@"OpenGL"]){UIButton *b=[self button:[@"Play — " stringByAppendingString:api] action:@selector(play:)];b.accessibilityIdentifier=api;[stack addArrangedSubview:b];}
 UILabel *materials=[self label:@"Material workshop · automatic demonstrations"];materials.textAlignment=NSTextAlignmentCenter;materials.font=[UIFont preferredFontForTextStyle:UIFontTextStyleHeadline];[stack addArrangedSubview:materials];
 self.workshopBackend=[[UISegmentedControl alloc]initWithItems:@[@"WebGPU",@"Vulkan",@"OpenGL"]];self.workshopBackend.selectedSegmentIndex=0;self.workshopBackend.accessibilityLabel=@"Workshop graphics API";[stack addArrangedSubview:self.workshopBackend];
 UIButton *panels=[self button:@"DAW only mode" action:@selector(openWorkstation:)];[stack addArrangedSubview:panels];
 UILabel *panelHelp=[self label:@"From the timeline, swipe down/up/right/left anywhere to open the top/bottom/left/right panel. Swipe only the opposite direction to close it and return to the timeline before opening another panel. Hold briefly before dragging controls. Two fingers scroll or pinch to zoom; two-finger double tap exits."];panelHelp.font=[UIFont preferredFontForTextStyle:UIFontTextStyleFootnote];panelHelp.textAlignment=NSTextAlignmentCenter;[stack addArrangedSubview:panelHelp];
 self.workshopForceMode=[[UISegmentedControl alloc]initWithItems:@[@"Demonstration",@"Compare forces"]];self.workshopForceMode.selectedSegmentIndex=0;self.workshopForceMode.accessibilityLabel=@"Workshop force comparison";[stack addArrangedSubview:self.workshopForceMode];
 UILabel *forceHelp=[self label:@"Compare forces runs light, medium and strong on fresh samples. The layered ground scene has its own longer digging sequence."];forceHelp.font=[UIFont preferredFontForTextStyle:UIFontTextStyleFootnote];forceHelp.textAlignment=NSTextAlignmentCenter;[stack addArrangedSubview:forceHelp];
 NSArray *materialTitles=@[@"Worker · Dig a ground section",@"Worker · Chop a standing tree",@"Worker · Shovel snow",@"Ice · Fracture, float and melt",@"Ice · Melt in air",@"Soil · Spade and clumps",@"Turf · Root mat and spade",@"Fine sand · Bucket scoop",@"Pebbles · Bucket scoop",@"Wood · Axe wedge and split",@"Wood · Chop across grain",@"Stone · Pickaxe",@"Loose stones · Shovel",@"Stone · Blunt bucket comparison",@"Layered ground · Turf → soil → stone"];
 NSArray *materialCases=@[@"matter_worker_dig",@"matter_worker_axe",@"matter_snow",@"matter_ice",@"matter_ice_air",@"matter_dirt",@"matter_grass",@"matter_sand",@"matter_pebbles",@"matter_wood",@"matter_wood_chop",@"matter_stone",@"matter_rubble",@"matter_stone_blunt",@"matter_layers"];
 for(NSUInteger i=0;i<materialTitles.count;i++){UIButton *workshop=[self button:materialTitles[i] action:@selector(openWorkshop:)];workshop.accessibilityIdentifier=materialCases[i];[stack addArrangedSubview:workshop];}
 UILabel *help=[self label:@"Drag left to move · Drag right to look\nTap the top-right corner to pause, open cheats, or replay controls.\nA short tutorial appears each time you start playing."];help.font=[UIFont preferredFontForTextStyle:UIFontTextStyleFootnote];help.textAlignment=NSTextAlignmentCenter;help.textColor=UIColor.lightGrayColor;[stack addArrangedSubview:help];
 self.input=[[MobileControls alloc]initWithFrame:self.view.bounds];self.input.autoresizingMask=UIViewAutoresizingFlexibleWidth|UIViewAutoresizingFlexibleHeight;self.input.multipleTouchEnabled=YES;self.input.hidden=YES;[self.view addSubview:self.input];controls=self.input;
 UITapGestureRecognizer *exitPanels=[[UITapGestureRecognizer alloc]initWithTarget:self action:@selector(workstationExit:)];exitPanels.numberOfTapsRequired=2;exitPanels.numberOfTouchesRequired=2;exitPanels.enabled=NO;[self.input addGestureRecognizer:exitPanels];
 self.toolPanel=[UIStackView new];self.toolPanel.axis=UILayoutConstraintAxisVertical;self.toolPanel.spacing=6;self.toolPanel.translatesAutoresizingMaskIntoConstraints=NO;self.toolPanel.hidden=YES;
 self.digStatus=[self label:@"Aim at nearby soil"];self.digStatus.font=[UIFont preferredFontForTextStyle:UIFontTextStyleFootnote];[self.toolPanel addArrangedSubview:self.digStatus];
 UIButton *dig=[self button:@"Hold · release to work" action:@selector(endDig)];self.workButton=dig;dig.backgroundColor=[UIColor.blackColor colorWithAlphaComponent:.7];dig.layer.cornerRadius=12;[dig addTarget:self action:@selector(beginDig) forControlEvents:UIControlEventTouchDown];[dig addTarget:self action:@selector(endDig) forControlEvents:UIControlEventTouchUpOutside|UIControlEventTouchCancel];[self.toolPanel addArrangedSubview:dig];
 for(NSLayoutConstraint *constraint in dig.constraints)if(constraint.firstAttribute==NSLayoutAttributeHeight)constraint.priority=UILayoutPriorityDefaultHigh;
 UISegmentedControl *tools=[[UISegmentedControl alloc]initWithItems:@[@"Spade",@"Pick",@"Axe",@"Bucket"]];self.materialTools=tools;tools.selectedSegmentIndex=0;[tools addTarget:self action:@selector(selectMaterialTool:) forControlEvents:UIControlEventValueChanged];[self.toolPanel addArrangedSubview:tools];
 self.putDownButton=[self button:@"Empty / pour" action:@selector(putDown)];[self.toolPanel addArrangedSubview:self.putDownButton];[self.input addSubview:self.toolPanel];
 [NSLayoutConstraint activateConstraints:@[[self.toolPanel.trailingAnchor constraintEqualToAnchor:self.input.safeAreaLayoutGuide.trailingAnchor constant:-20],[self.toolPanel.bottomAnchor constraintEqualToAnchor:self.input.safeAreaLayoutGuide.bottomAnchor constant:-105],[self.toolPanel.widthAnchor constraintEqualToConstant:260]]];
 self.workshopPanel=[UIStackView new];self.workshopPanel.axis=UILayoutConstraintAxisVertical;self.workshopPanel.spacing=4;self.workshopPanel.translatesAutoresizingMaskIntoConstraints=NO;self.workshopPanel.hidden=YES;
 self.workshopStatus=[self label:@"Preparing material workshop…"];self.workshopStatus.font=[UIFont preferredFontForTextStyle:UIFontTextStyleFootnote];self.workshopStatus.backgroundColor=[UIColor.blackColor colorWithAlphaComponent:.65];[self.workshopPanel addArrangedSubview:self.workshopStatus];
 UIStackView *actions=[UIStackView new];actions.axis=UILayoutConstraintAxisHorizontal;actions.spacing=4;actions.distribution=UIStackViewDistributionFillEqually;
 self.workshopPause=[self button:@"Pause" action:@selector(workshopTogglePause)];self.workshopStep=[self button:@"Step" action:@selector(workshopSingleStep)];
 for(UIButton *b in @[self.workshopPause,self.workshopStep,[self button:@"Reset" action:@selector(workshopReset)],[self button:@"Exit" action:@selector(returnToTitle)]]){b.backgroundColor=[UIColor.blackColor colorWithAlphaComponent:.75];[actions addArrangedSubview:b];}
 [self.workshopPanel addArrangedSubview:actions];[self.input addSubview:self.workshopPanel];
 [NSLayoutConstraint activateConstraints:@[[self.workshopPanel.leadingAnchor constraintEqualToAnchor:self.input.safeAreaLayoutGuide.leadingAnchor constant:12],[self.workshopPanel.trailingAnchor constraintEqualToAnchor:self.input.safeAreaLayoutGuide.trailingAnchor constant:-12],[self.workshopPanel.topAnchor constraintEqualToAnchor:self.input.safeAreaLayoutGuide.topAnchor constant:8]]];
 self.tutorialStep=-1;
}
- (NSString *)version{return [NSString stringWithFormat:@"%@ · %@",NSBundle.mainBundle.infoDictionary[@"CFBundleDisplayName"],NSBundle.mainBundle.infoDictionary[@"CFBundleVersion"]];}
- (void)viewDidLayoutSubviews{[super viewDidLayoutSubviews];[self layoutHighlight];}
- (void)layoutHighlight {
 if(!self.highlight)return;CGFloat w=self.input.bounds.size.width,h=self.input.bounds.size.height;
 CGFloat corner=MIN(80.,MIN(w,h)*.18);self.highlight.hidden=self.tutorialStep==3;
 if(self.tutorialStep==0)self.highlight.frame=CGRectMake(16,h*.35,w*.44,h*.55);
 else if(self.tutorialStep==2)self.highlight.frame=CGRectMake(w-corner,h-corner,corner,corner);
 else if(self.tutorialStep==4||self.tutorialStep==5)self.highlight.frame=CGRectInset(self.toolPanel.frame,-4,-4);
 else if(self.tutorialStep==6)self.highlight.frame=CGRectMake(w-corner,0,corner,corner);
 else self.highlight.frame=CGRectMake(w*.54,h*.35,w*.44-16,h*.55);
}
- (void)startTutorial {
 self.tutorialStep=0;[self showTutorial];
}
- (void)showTutorial {
 [self.tutorial removeFromSuperview];self.tutorial=nil;self.highlight=nil;
 if(!self.running||self.paused||self.tutorialStep<0)return;
 NSArray *titles=@[@"Move",@"Look around",@"Jump",@"Choose a tool",@"Work the material",@"Carry and pour",@"Pause and test with cheats"];
 NSArray *lessons=@[@"Drag on the left half of the screen. Drag farther to move faster. Double-tap the left side to toggle sprint.",@"Drag on the right half to look. You can move and look with two fingers at the same time.",@"Tap the bottom-right corner to jump. With cheats off, double-tapping still only jumps.",@"Choose Spade, Pick, Axe or Bucket in the tool panel. Your starter tools are available immediately.",@"Aim at nearby turf, soil, sand, rock or a tree. Hold Work to build force, then release for one physical stroke. A short press is light; a longer hold is stronger. Drag right to look.",@"Use the bucket to scoop loose sand or water. Walk while carrying it and tap Empty / pour to tip it. Keep the loaded bucket selected until you have poured.",@"Tap the top-right corner to pause. Enable cheats to use flight, noclip and the time-of-day slider. Use real time restores the clock. In flight, hold bottom-right to rise and bottom-left to descend. Noclip lets you pass through rock. You can also replay these controls here."];
 self.tutorial=[[TutorialOverlay alloc]initWithFrame:self.input.bounds];self.tutorial.autoresizingMask=UIViewAutoresizingFlexibleWidth|UIViewAutoresizingFlexibleHeight;[self.input addSubview:self.tutorial];
 self.highlight=[UIView new];self.highlight.userInteractionEnabled=NO;self.highlight.layer.borderColor=UIColor.systemYellowColor.CGColor;self.highlight.layer.borderWidth=2;self.highlight.layer.cornerRadius=16;[self.tutorial addSubview:self.highlight];
 UIView *card=[UIView new];card.backgroundColor=[UIColor.blackColor colorWithAlphaComponent:.78];card.layer.cornerRadius=14;card.translatesAutoresizingMaskIntoConstraints=NO;[self.tutorial addSubview:card];
 UIStackView *stack=[UIStackView new];stack.axis=UILayoutConstraintAxisVertical;stack.spacing=8;stack.translatesAutoresizingMaskIntoConstraints=NO;[card addSubview:stack];
 UILabel *title=[self label:[NSString stringWithFormat:@"Controls %ld of 7 · %@",(long)self.tutorialStep+1,titles[self.tutorialStep]]];title.font=[UIFont preferredFontForTextStyle:UIFontTextStyleHeadline];[stack addArrangedSubview:title];[stack addArrangedSubview:[self label:lessons[self.tutorialStep]]];
 UIStackView *buttons=[UIStackView new];buttons.axis=UILayoutConstraintAxisHorizontal;buttons.distribution=UIStackViewDistributionFillEqually;[buttons addArrangedSubview:[self button:@"Skip tutorial" action:@selector(finishTutorial)]];[buttons addArrangedSubview:[self button:self.tutorialStep==6?@"Done":@"Next" action:@selector(nextTutorial)]];[stack addArrangedSubview:buttons];
 [NSLayoutConstraint activateConstraints:@[[card.topAnchor constraintEqualToAnchor:self.tutorial.safeAreaLayoutGuide.topAnchor constant:8],[card.centerXAnchor constraintEqualToAnchor:self.tutorial.centerXAnchor],[card.widthAnchor constraintLessThanOrEqualToAnchor:self.tutorial.safeAreaLayoutGuide.widthAnchor constant:-24],[card.widthAnchor constraintLessThanOrEqualToConstant:560],[stack.leadingAnchor constraintEqualToAnchor:card.leadingAnchor constant:14],[stack.trailingAnchor constraintEqualToAnchor:card.trailingAnchor constant:-14],[stack.topAnchor constraintEqualToAnchor:card.topAnchor constant:10],[stack.bottomAnchor constraintEqualToAnchor:card.bottomAnchor constant:-6]]];
 [self layoutHighlight];UIAccessibilityPostNotification(UIAccessibilityAnnouncementNotification,title.text);
}
- (void)noteControlAction:(NSInteger)action{if(self.tutorialStep==action)[self nextTutorial];}
- (void)nextTutorial{if(self.tutorialStep<0)return;if(++self.tutorialStep>=7)[self finishTutorial];else [self showTutorial];}
- (void)finishTutorial {
 self.tutorialStep=-1;[self.tutorial removeFromSuperview];self.tutorial=nil;self.highlight=nil;
}
- (void)selectMaterialTool:(UISegmentedControl *)sender {if(!self.phoneHeld)ww_touch_tool((uint32_t)sender.selectedSegmentIndex);}
- (void)beginDig { if(self.phoneHeld)return;ww_touch_work(true);[self noteControlAction:4]; }
- (void)endDig { ww_touch_work(false); }
- (void)putDown { ww_touch_put_down(); }
- (void)receivePhoneHeld:(BOOL)held focused:(BOOL)focused {
 held=held&&self.running&&!stopRequested;focused=held&&focused;
 if(self.phoneHeld==held&&self.phoneFocused==focused)return;
 BOOL acquired=held&&!self.phoneHeld;self.phoneHeld=held;self.phoneFocused=focused;
 if(acquired)ww_touch_work(false);
 self.digStatus.hidden=self.workButton.hidden=self.materialTools.hidden=held;
 self.workButton.enabled=self.materialTools.enabled=!held;
 [self.putDownButton setTitle:held?(focused?@"Lower phone":@"Put down phone"):@"Empty / pour" forState:UIControlStateNormal];
 // Pause, workstation focus and acknowledged control flags own panel visibility.
}
- (void)digFeedback:(uint32_t)status {
 NSArray *messages=@[@"Aim at nearby soil",@"Ready to dig",@"Positioning shovel",@"Pushing into soil",@"Levering",@"Lifting",@"Blocked — release to stop",@"No soil reached",@"Soil lifted",@"Release to try again",@"Empty shovel — release to retry"];
 self.digStatus.text=messages[MIN(status,(uint32_t)messages.count-1)];
}
- (void)refreshCheatButtons {
 [self.cheatsButton setTitle:(self.controlFlags&1)?@"Cheats: On":@"Cheats: Off" forState:UIControlStateNormal];
 [self.flightButton setTitle:(self.controlFlags&2)?@"Flight: On":@"Flight: Off" forState:UIControlStateNormal];
 [self.noclipButton setTitle:(self.controlFlags&4)?@"Noclip: On":@"Noclip: Off" forState:UIControlStateNormal];
 self.flightButton.enabled=self.noclipButton.enabled=(self.controlFlags&1)!=0;
 self.timeSlider.enabled=self.realTimeButton.enabled=(self.controlFlags&1)!=0;
 if(self.timeOverrideMinutes<0) self.timeLabel.text=@"Time of day: Real time";
 else { NSInteger h=self.timeOverrideMinutes/60,m=self.timeOverrideMinutes%60; self.timeLabel.text=[NSString stringWithFormat:@"Time of day: %ld:%02ld %@",(long)(h%12?h%12:12),(long)m,h<12?@"AM":@"PM"]; }
}
- (void)toggleCheats{self.controlFlags=(self.controlFlags&1)?(self.controlFlags&8):(self.controlFlags|1);if(!(self.controlFlags&1))self.timeOverrideMinutes=-1;[self requestCheats];}
- (void)toggleFlight{self.controlFlags=(self.controlFlags&2)?(self.controlFlags&~6u):(self.controlFlags|2);[self requestCheats];}
- (void)toggleNoclip{self.controlFlags=(self.controlFlags&4)?(self.controlFlags&~4u):(self.controlFlags|6);[self requestCheats];}
- (void)requestCheats{ww_touch_cheats(self.controlFlags&7);[self refreshCheatButtons];}
- (void)changeTime:(UISlider *)slider { self.timeOverrideMinutes=(NSInteger)lroundf(slider.value);ww_touch_time_override((int32_t)self.timeOverrideMinutes);[self refreshCheatButtons]; }
- (void)useRealTime { self.timeOverrideMinutes=-1;ww_touch_time_override(-1);[self refreshCheatButtons]; }
- (void)resume{ww_touch_pause_request();}
- (void)replayTutorial{self.replayAfterResume=YES;ww_touch_pause_request();}
- (void)returnToTitle{ww_ios_request_close();}
- (void)receiveControlState:(uint32_t)flags {
 if(self.workstationMode)return;
 BOOL hadTool=(self.controlFlags&8)!=0,hasTool=(flags&8)!=0;
 self.controlFlags=self.paused?((self.controlFlags&7)|(flags&8)):flags;
 self.toolPanel.hidden=!hasTool||self.paused;
 [self refreshCheatButtons];
 if(!hadTool&&hasTool)[self noteControlAction:3];
 if(hadTool&&!hasTool)[self noteControlAction:5];
}
- (void)applyPause:(BOOL)paused {
 if(self.workstationMode)return;
 self.toolPanel.hidden=paused||(self.controlFlags&8)==0;
 if(!self.running)return;if(self.paused==paused&&((paused&&self.pausePanel)||!paused))return;
 self.paused=paused;[self.input.gestures removeAllObjects];[self.pausePanel removeFromSuperview];self.pausePanel=nil;
 if(!paused){if(self.replayAfterResume){self.replayAfterResume=NO;[self startTutorial];}else [self showTutorial];return;}
 [self noteControlAction:6];[self.tutorial removeFromSuperview];self.tutorial=nil;
 self.pausePanel=[[UIView alloc]initWithFrame:self.input.bounds];self.pausePanel.autoresizingMask=UIViewAutoresizingFlexibleWidth|UIViewAutoresizingFlexibleHeight;self.pausePanel.backgroundColor=[UIColor.blackColor colorWithAlphaComponent:.92];[self.input addSubview:self.pausePanel];
 UIScrollView *scroll=[UIScrollView new];scroll.translatesAutoresizingMaskIntoConstraints=NO;[self.pausePanel addSubview:scroll];
 UIStackView *stack=[UIStackView new];stack.axis=UILayoutConstraintAxisVertical;stack.spacing=8;stack.translatesAutoresizingMaskIntoConstraints=NO;[scroll addSubview:stack];
 UILabel *title=[self label:@"Paused"];title.font=[UIFont preferredFontForTextStyle:UIFontTextStyleTitle2];[stack addArrangedSubview:title];
 self.cheatsButton=[self button:@"Cheats: Off" action:@selector(toggleCheats)];self.flightButton=[self button:@"Flight: Off" action:@selector(toggleFlight)];self.noclipButton=[self button:@"Noclip: Off" action:@selector(toggleNoclip)];
 for(UIButton *b in @[self.cheatsButton,self.flightButton,self.noclipButton])[stack addArrangedSubview:b];
 self.timeLabel=[self label:@"Time of day: Real time"];[stack addArrangedSubview:self.timeLabel];
 self.timeSlider=[UISlider new];self.timeSlider.minimumValue=0;self.timeSlider.maximumValue=1439;self.timeSlider.value=self.timeOverrideMinutes<0?720:self.timeOverrideMinutes;self.timeSlider.accessibilityLabel=@"Time of day";[self.timeSlider addTarget:self action:@selector(changeTime:) forControlEvents:UIControlEventValueChanged];[stack addArrangedSubview:self.timeSlider];
 self.realTimeButton=[self button:@"Use real time" action:@selector(useRealTime)];[stack addArrangedSubview:self.realTimeButton];
 UILabel *help=[self label:@"Changes apply when you resume.\nFlight: move/look normally; hold bottom-right to rise, bottom-left to descend. Double-tap left to toggle fast movement. Noclip follows your look direction and passes through ground and cave walls.\nTurning noclip off in solid rock returns you to a safe position."];help.font=[UIFont preferredFontForTextStyle:UIFontTextStyleFootnote];[stack addArrangedSubview:help];
 [stack addArrangedSubview:[self button:@"Resume" action:@selector(resume)]];[stack addArrangedSubview:[self button:@"Replay controls tutorial" action:@selector(replayTutorial)]];[stack addArrangedSubview:[self button:@"Return to title" action:@selector(returnToTitle)]];
 [NSLayoutConstraint activateConstraints:@[[scroll.topAnchor constraintEqualToAnchor:self.pausePanel.safeAreaLayoutGuide.topAnchor constant:12],[scroll.bottomAnchor constraintEqualToAnchor:self.pausePanel.safeAreaLayoutGuide.bottomAnchor constant:-12],[scroll.centerXAnchor constraintEqualToAnchor:self.pausePanel.centerXAnchor],[scroll.widthAnchor constraintEqualToAnchor:self.pausePanel.safeAreaLayoutGuide.widthAnchor multiplier:.9],[stack.topAnchor constraintEqualToAnchor:scroll.contentLayoutGuide.topAnchor],[stack.bottomAnchor constraintEqualToAnchor:scroll.contentLayoutGuide.bottomAnchor],[stack.leadingAnchor constraintEqualToAnchor:scroll.contentLayoutGuide.leadingAnchor],[stack.trailingAnchor constraintEqualToAnchor:scroll.contentLayoutGuide.trailingAnchor],[stack.widthAnchor constraintEqualToAnchor:scroll.frameLayoutGuide.widthAnchor]]];
 [self refreshCheatButtons];UIAccessibilityPostNotification(UIAccessibilityScreenChangedNotification,title);
}
- (void)workshopTogglePause {self.workshopPaused=!self.workshopPaused;[self.workshopPause setTitle:self.workshopPaused?@"Resume":@"Pause" forState:UIControlStateNormal];self.workshopStep.enabled=self.workshopPaused;ww_workshop_action(1);}
- (void)workshopSingleStep {if(self.workshopPaused)ww_workshop_action(2);}
- (void)workshopReset {ww_workshop_action(4);}
- (void)play:(UIButton *)sender {self.workshopMode=NO;self.workstationMode=NO;[self startSession:sender];}
- (void)openWorkshop:(UIButton *)sender {self.workshopMode=YES;self.workstationMode=NO;[self startSession:sender];}
- (void)openWorkstation:(UIButton *)sender {self.workstationScale=1.;self.workshopMode=YES;self.workstationMode=YES;sender.accessibilityIdentifier=@"workstation_panels";[self startSession:sender];}
- (void)workstationExit:(UITapGestureRecognizer *)tap {if(self.workstationMode){if(self.embeddedWorkstation)ww_workstation_return_to_game();else ww_ios_request_close();}}
- (void)startSession:(UIButton *)sender {
 if(self.running)return;self.running=YES;self.paused=NO;self.controlFlags=0;self.timeOverrideMinutes=-1;self.replayAfterResume=NO;stopRequested=NO;
 [self receivePhoneHeld:NO focused:NO];
 ww_touch_begin_session();if(self.workstationMode)ww_ios_workstation_overlay_attach((__bridge void *)self.input);else ww_ios_workstation_overlay_hide();self.menu.hidden=YES;self.input.hidden=NO;self.toolPanel.hidden=YES;UIApplication.sharedApplication.idleTimerDisabled=YES;
 self.workshopPaused=NO;self.workshopPanel.hidden=!self.workshopMode||self.workstationMode;self.workshopStep.enabled=NO;[self.workshopPause setTitle:@"Pause" forState:UIControlStateNormal];
 for(UIGestureRecognizer *gesture in self.input.gestureRecognizers)gesture.enabled=self.workstationMode;
 if(!self.workshopMode)[self startTutorial];
 BOOL workshopMode=self.workshopMode;NSString *workshopCase=workshopMode?sender.accessibilityIdentifier:nil;NSString *api=workshopMode?[@[@"WebGPU",@"Vulkan",@"OpenGL"] objectAtIndex:self.workshopBackend.selectedSegmentIndex]:sender.accessibilityIdentifier;NSString *resources=[NSBundle.mainBundle.resourcePath stringByAppendingPathComponent:@"EngineAssets"];NSString *folder=NSBundle.mainBundle.bundleIdentifier;NSURL *documents=[NSFileManager.defaultManager URLsForDirectory:NSDocumentDirectory inDomains:NSUserDomainMask].firstObject;NSString *output=[[documents URLByAppendingPathComponent:folder isDirectory:YES]path];
 if(workshopMode && !self.workstationMode && self.workshopForceMode.selectedSegmentIndex==1 && ![workshopCase isEqualToString:@"matter_layers"] && ![workshopCase hasPrefix:@"matter_worker"] && ![workshopCase hasPrefix:@"matter_ice"] && ![workshopCase isEqualToString:@"matter_snow"])workshopCase=[workshopCase stringByAppendingString:@"_compare"];
 // Host construction and session entry each need more than 1 MiB of stack.
 // GCD chooses a smaller worker stack; reserve our session budget explicitly.
 NSThread *sessionThread=[[NSThread alloc]initWithBlock:^{@autoreleasepool{
  NSUInteger stackBytes=pthread_get_stacksize_np(pthread_self());
  NSDictionary *startup=@{@"mode":self.workstationMode?@"daw":(workshopMode?@"workshop":@"game"),@"backend":api,@"stack_bytes":@(stackBytes),@"build":NSBundle.mainBundle.infoDictionary[@"CFBundleVersion"]?:@"unknown"};
  [NSFileManager.defaultManager createDirectoryAtPath:output withIntermediateDirectories:YES attributes:nil error:nil];
  [[NSJSONSerialization dataWithJSONObject:startup options:NSJSONWritingPrettyPrinted error:nil] writeToFile:[output stringByAppendingPathComponent:@"mobile-session-startup.json"] atomically:YES];
  int result=-1;
  if(stackBytes<WW_IOS_SESSION_STACK_MIB*1024u*1024u){
   NSString *failure=[NSString stringWithFormat:@"Session thread stack is too small (%lu bytes).",(unsigned long)stackBytes];
   [failure writeToFile:[output stringByAppendingPathComponent:@"session-error.txt"] atomically:YES encoding:NSUTF8StringEncoding error:nil];
  }else{
   result=workshopMode?wormwood_ios_run_material_workshop(resources.UTF8String,output.UTF8String,api.UTF8String,workshopCase.UTF8String):wormwood_ios_run_game(resources.UTF8String,output.UTF8String,api.UTF8String);
  }
  NSString *failure=[NSString stringWithContentsOfFile:[output stringByAppendingPathComponent:@"session-error.txt"] encoding:NSUTF8StringEncoding error:nil];if(failure.length>600)failure=[failure substringToIndex:600];
  dispatch_async(dispatch_get_main_queue(),^{ww_ios_workstation_overlay_hide();self.running=NO;[self receivePhoneHeld:NO focused:NO];self.workshopPanel.hidden=YES;self.workshopMode=NO;self.workstationMode=NO;for(UIGestureRecognizer *gesture in self.input.gestureRecognizers)gesture.enabled=NO;self.input.hidden=YES;self.menu.hidden=NO;[self.pausePanel removeFromSuperview];self.pausePanel=nil;[self.tutorial removeFromSuperview];self.tutorial=nil;self.tutorialStep=-1;[self.view bringSubviewToFront:self.menu];UIApplication.sharedApplication.idleTimerDisabled=NO;self.status.text=result==0?[self version]:[NSString stringWithFormat:@"%@\nSession failed: %@",[self version],failure?:@"See app diagnostics in Files."];});
 }}];
 sessionThread.name=@"Mirror session";
 sessionThread.qualityOfService=NSQualityOfServiceUserInitiated;
 sessionThread.stackSize=WW_IOS_SESSION_STACK_MIB*1024u*1024u;
 [sessionThread start];
}
@end
// Focus changes use the existing input surface without creating a second app
// session or activating the gameplay pause overlay.
void ww_ios_workstation_focus(bool enabled){onMain(^{
 GameController *controller=currentController;
 controller.embeddedWorkstation=enabled;controller.workstationMode=enabled;
 if(enabled)ww_ios_workstation_overlay_attach((__bridge void *)controller.input);else ww_ios_workstation_overlay_hide();
 [controller.input.gestures removeAllObjects];ww_touch_reset();
 for(UIGestureRecognizer *gesture in controller.input.gestureRecognizers)gesture.enabled=enabled;
 controller.toolPanel.hidden=enabled||(controller.controlFlags&8)==0;
 if(enabled){[controller.tutorial removeFromSuperview];controller.tutorial=nil;}
});}
void ww_ios_caption(const char *text){if(!text)return;NSString *caption=[NSString stringWithUTF8String:text];if(!caption)return;dispatch_async(dispatch_get_main_queue(),^{if(currentController.workshopMode)currentController.workshopStatus.text=caption;else currentController.digStatus.text=caption;});}
// UI is derived from acknowledged engine state; these never mutate game physics.
void ww_ios_controls_state(uint32_t flags){dispatch_async(dispatch_get_main_queue(),^{[currentController receiveControlState:flags];});}
void ww_ios_phone_state(bool held,bool focused){dispatch_async(dispatch_get_main_queue(),^{[currentController receivePhoneHeld:held focused:focused];});}
void ww_ios_pause_state(bool paused){dispatch_async(dispatch_get_main_queue(),^{[currentController applyPause:paused];});}
void ww_ios_dig_feedback(uint32_t status){dispatch_async(dispatch_get_main_queue(),^{[currentController digFeedback:status];});}
@interface GameScene:UIResponder<UIWindowSceneDelegate>
@property(nonatomic,strong)UIWindow *window;
@end
@implementation GameScene
- (void)scene:(UIScene *)scene willConnectToSession:(UISceneSession *)session options:(UISceneConnectionOptions *)options{if(![scene isKindOfClass:UIWindowScene.class])return;self.window=[[UIWindow alloc]initWithWindowScene:(UIWindowScene *)scene];self.window.rootViewController=[GameController new];[self.window makeKeyAndVisible];}
- (void)sceneWillResignActive:(UIScene *)scene{active=NO;ww_touch_reset();ww_ios_active(false);}
- (void)sceneDidBecomeActive:(UIScene *)scene{active=YES;ww_ios_active(true);}
- (void)sceneDidDisconnect:(UIScene *)scene{ww_ios_request_close();}
@end
@interface GameApp:UIResponder<UIApplicationDelegate>
@end
@implementation GameApp
- (BOOL)application:(UIApplication *)application didFinishLaunchingWithOptions:(NSDictionary *)options{return YES;}
- (UISceneConfiguration *)application:(UIApplication *)application configurationForConnectingSceneSession:(UISceneSession *)session options:(UISceneConnectionOptions *)options{UISceneConfiguration *c=[[UISceneConfiguration alloc]initWithName:@"Game" sessionRole:session.role];c.delegateClass=GameScene.class;return c;}
@end
int main(int argc,char **argv){@autoreleasepool{return UIApplicationMain(argc,argv,nil,NSStringFromClass(GameApp.class));}}
