#import <UIKit/UIKit.h>
extern void ww_ios_attach(void *controller);
extern void ww_ios_active(bool active);
extern int wormwood_ios_run_cues(const char *resources,const char *output);
extern int wormwood_ios_bootstrap(const char *resources, const char *output);
@interface WormwoodScene : UIResponder <UIWindowSceneDelegate>
@property(nonatomic,strong) UIWindow *window;
@end
@implementation WormwoodScene
- (void)scene:(UIScene *)scene willConnectToSession:(UISceneSession *)session options:(UISceneConnectionOptions *)options {
    if (![scene isKindOfClass:UIWindowScene.class]) return;
    self.window=[[UIWindow alloc] initWithWindowScene:(UIWindowScene *)scene];
    UIViewController *controller=[UIViewController new];
    controller.view.backgroundColor=UIColor.blackColor;
    self.window.rootViewController=controller;
    [self.window makeKeyAndVisible];
    NSString *resources=[NSBundle.mainBundle.resourcePath stringByAppendingPathComponent:@"EngineAssets"];
    NSURL *documents=[NSFileManager.defaultManager URLsForDirectory:NSDocumentDirectory inDomains:NSUserDomainMask].firstObject;
    NSString *output=[[documents URLByAppendingPathComponent:@"Wormwood" isDirectory:YES] path];
    ww_ios_attach((__bridge void *)controller);
    BOOL previousIdleTimer=UIApplication.sharedApplication.idleTimerDisabled;
    UIApplication.sharedApplication.idleTimerDisabled=YES;
    // Keep UIKit responsive. Connection Systems marshal only graphics work
    // onto the main queue; Rust orchestration and report I/O remain here.
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW,NSEC_PER_SEC),dispatch_get_global_queue(QOS_CLASS_USER_INITIATED,0),^{@autoreleasepool {
        int result=wormwood_ios_bootstrap(resources.UTF8String,output.UTF8String);
        if(result==0) result=wormwood_ios_run_cues(resources.UTF8String,output.UTF8String);
        NSLog(@"Wormwood tracked iOS diagnostics returned %d",result);
        dispatch_async(dispatch_get_main_queue(),^{UIApplication.sharedApplication.idleTimerDisabled=previousIdleTimer;});
    }});
}
- (void)sceneWillResignActive:(UIScene *)scene {ww_ios_active(false);}
- (void)sceneDidBecomeActive:(UIScene *)scene {ww_ios_active(true);}
@end
@interface WormwoodApp : UIResponder <UIApplicationDelegate>
@end
@implementation WormwoodApp
- (BOOL)application:(UIApplication *)application didFinishLaunchingWithOptions:(NSDictionary *)options { return YES; }
- (UISceneConfiguration *)application:(UIApplication *)application configurationForConnectingSceneSession:(UISceneSession *)session options:(UISceneConnectionOptions *)options {
    UISceneConfiguration *configuration=[[UISceneConfiguration alloc] initWithName:@"Wormwood" sessionRole:session.role];
    configuration.delegateClass=WormwoodScene.class;
    return configuration;
}
@end
int main(int argc,char **argv){@autoreleasepool{return UIApplicationMain(argc,argv,nil,NSStringFromClass(WormwoodApp.class));}}
