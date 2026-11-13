#import <Cocoa/Cocoa.h>
#import <objc/runtime.h>

// Pass-through UI: the overlay must never steal the game's mouse controls.
@interface MirrorMaterialHUD : NSView
@property(nonatomic,strong) NSTextField *label;
@end
@implementation MirrorMaterialHUD
-(NSView *)hitTest:(NSPoint)point { return nil; }
@end
void ww_material_hud(void *nativeWindow,const char *caption,bool visible) {
    if (!nativeWindow || !caption) return;
    NSWindow *window=(__bridge NSWindow *)nativeWindow;
    NSString *text=[NSString stringWithUTF8String:caption];
    static char association;
    MirrorMaterialHUD *hud=objc_getAssociatedObject(window,&association);
    if (!visible || (![text containsString:@"1 Shovel"] && ![text hasPrefix:@"NPC | "])) { hud.hidden=YES; return; }
    if (!hud) {
        NSView *content=window.contentView;
        hud=[[MirrorMaterialHUD alloc] initWithFrame:NSMakeRect(12,content.bounds.size.height-92,content.bounds.size.width-24,80)];
        hud.autoresizingMask=NSViewWidthSizable|NSViewMinYMargin;
        hud.wantsLayer=YES;hud.layer.backgroundColor=[NSColor colorWithWhite:0.02 alpha:0.78].CGColor;hud.layer.cornerRadius=7;
        hud.label=[NSTextField wrappingLabelWithString:@""];
        hud.label.frame=NSInsetRect(hud.bounds,10,7);hud.label.autoresizingMask=NSViewWidthSizable|NSViewHeightSizable;
        hud.label.textColor=NSColor.whiteColor;hud.label.font=[NSFont systemFontOfSize:13 weight:NSFontWeightMedium];
        [hud addSubview:hud.label];[content addSubview:hud positioned:NSWindowAbove relativeTo:nil];
        objc_setAssociatedObject(window,&association,hud,OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    }
    text=[text stringByReplacingOccurrencesOfString:@" · 1 Shovel" withString:@"\n1 Shovel"];
    text=[text stringByReplacingOccurrencesOfString:@" · Hold LMB" withString:@"\nHold left mouse"];
    hud.label.stringValue=text;
    hud.hidden=NO;
}
