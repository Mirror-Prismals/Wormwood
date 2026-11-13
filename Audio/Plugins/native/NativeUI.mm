#import <Cocoa/Cocoa.h>
#import <objc/runtime.h>
#include "Host/Vst3Host.h"

struct Vst3UiWindow {
    NSWindow* window;
    NSView* contentView;
};

@interface Vst3EditorActionTarget : NSObject
@property(nonatomic, assign) Vst3EditorActionFn action;
@property(nonatomic, assign) void* userData;
@end
@implementation Vst3EditorActionTarget
- (void)perform:(id)sender { if (self.action) self.action(self.userData); }
@end

@interface Vst3BitmapView : NSView
@property(nonatomic, strong) NSImage* image;
@property(nonatomic, assign) Vst3EditorGestureFn gesture;
@property(nonatomic, assign) void* userData;
@property(nonatomic, assign) BOOL busy;
@property(nonatomic, assign) BOOL pressed;
@property(nonatomic, assign) NSPoint startPoint;
@end
@implementation Vst3BitmapView
- (BOOL)isFlipped { return YES; }
- (BOOL)acceptsFirstMouse:(NSEvent*)event { return YES; }
- (void)drawRect:(NSRect)rect {
    [self.image drawInRect:self.bounds fromRect:NSZeroRect operation:NSCompositingOperationSourceOver
                 fraction:1.0 respectFlipped:YES hints:nil];
}
- (void)mouseDown:(NSEvent*)event {
    if (self.busy || !self.gesture) return;
    self.startPoint = [self convertPoint:event.locationInWindow fromView:nil];
    self.pressed = YES;
    self.gesture(self.userData, self.startPoint.x, self.startPoint.y, 0);
}
- (void)mouseDragged:(NSEvent*)event {
    if (!self.pressed || !self.gesture) return;
    NSPoint point = [self convertPoint:event.locationInWindow fromView:nil];
    point.x = fmax(0., fmin(point.x, self.bounds.size.width - 1.));
    point.y = fmax(0., fmin(point.y, self.bounds.size.height - 1.));
    self.gesture(self.userData, point.x, point.y, 1);
}
- (void)mouseUp:(NSEvent*)event {
    if (!self.pressed) return;
    self.pressed = NO;
    if (!self.gesture) return;
    NSPoint end = [self convertPoint:event.locationInWindow fromView:nil];
    end.x = fmax(0., fmin(end.x, self.bounds.size.width - 1.));
    end.y = fmax(0., fmin(end.y, self.bounds.size.height - 1.));
    self.gesture(self.userData, end.x, end.y, 2);
}
@end

@interface Vst3DefaultParamTarget : NSObject
@property(nonatomic, assign) Vst3DefaultEditorSetParamFn setParam;
@property(nonatomic, assign) void* userData;
@end

@implementation Vst3DefaultParamTarget
- (void)sliderChanged:(NSSlider*)sender {
    if (!self.setParam) return;
    self.setParam(self.userData, static_cast<int32_t>(sender.tag), sender.doubleValue);
}
@end

static void Vst3UI_EnsureApp() {
    NSApplication* app = [NSApplication sharedApplication];
    if (app && [app activationPolicy] == NSApplicationActivationPolicyProhibited) {
        [app setActivationPolicy:NSApplicationActivationPolicyRegular];
    }
}

Vst3UiWindow* Vst3UI_CreateWindow(const char* title, int width, int height) {
    Vst3UI_EnsureApp();
    NSString* nsTitle = title ? [NSString stringWithUTF8String:title] : @"VST3";
    NSRect rect = NSMakeRect(0, 0, width > 0 ? width : 640, height > 0 ? height : 480);
    NSUInteger style = NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskResizable;
    NSWindow* window = [[NSWindow alloc] initWithContentRect:rect
                                                   styleMask:style
                                                     backing:NSBackingStoreBuffered
                                                       defer:NO];
    [window setTitle:nsTitle];
    [window setReleasedWhenClosed:NO];
    [window center];
    NSView* content = [[NSView alloc] initWithFrame:rect];
    [window setContentView:content];
    auto* handle = new Vst3UiWindow{window, content};
    return handle;
}

Vst3UiWindow* Vst3UI_CreateDefaultEditorWindow(const char* title,
                                               const Vst3DefaultEditorParam* params,
                                               int paramCount,
                                               Vst3DefaultEditorSetParamFn setParam,
                                               void* userData) {
    Vst3UI_EnsureApp();
    const int clampedCount = paramCount > 0 ? paramCount : 0;
    const int width = 520;
    const int rowHeight = 34;
    const int topPad = 48;
    const int bottomPad = 18;
    const int contentHeight = MAX(220, topPad + bottomPad + clampedCount * rowHeight);
    const int windowHeight = MIN(640, contentHeight);

    NSString* nsTitle = title ? [NSString stringWithUTF8String:title] : @"VST3 Parameters";
    NSRect rect = NSMakeRect(0, 0, width, windowHeight);
    NSUInteger style = NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskResizable;
    NSWindow* window = [[NSWindow alloc] initWithContentRect:rect
                                                   styleMask:style
                                                     backing:NSBackingStoreBuffered
                                                       defer:NO];
    [window setTitle:[nsTitle stringByAppendingString:@" Parameters"]];
    [window setReleasedWhenClosed:NO];
    [window center];

    NSScrollView* scroll = [[NSScrollView alloc] initWithFrame:rect];
    [scroll setHasVerticalScroller:YES];
    [scroll setAutohidesScrollers:YES];
    [scroll setAutoresizingMask:NSViewWidthSizable | NSViewHeightSizable];

    NSView* document = [[NSView alloc] initWithFrame:NSMakeRect(0, 0, width, contentHeight)];
    Vst3DefaultParamTarget* target = [[Vst3DefaultParamTarget alloc] init];
    target.setParam = setParam;
    target.userData = userData;
    objc_setAssociatedObject(document, "Vst3DefaultParamTarget", target, OBJC_ASSOCIATION_RETAIN_NONATOMIC);

    NSTextField* header = [NSTextField labelWithString:@"Generic parameter editor"];
    header.identifier = @"Vst3ParameterHeader";
    [header setFrame:NSMakeRect(16, contentHeight - 32, width - 32, 20)];
    [header setFont:[NSFont boldSystemFontOfSize:13.0]];
    [document addSubview:header];

    for (int i = 0; i < clampedCount; ++i) {
        const Vst3DefaultEditorParam& param = params[i];
        const int y = contentHeight - topPad - (i + 1) * rowHeight;
        NSString* paramTitle = param.title[0] ? [NSString stringWithUTF8String:param.title] : @"Parameter";
        if (param.units[0]) {
            paramTitle = [paramTitle stringByAppendingFormat:@" (%s)", param.units];
        }

        NSTextField* label = [NSTextField labelWithString:paramTitle];
        label.identifier = @"Vst3ParameterLabel";
        label.tag = param.id;
        [label setFrame:NSMakeRect(16, y + 8, 190, 18)];
        [label setLineBreakMode:NSLineBreakByTruncatingTail];
        [document addSubview:label];

        NSSlider* slider = [NSSlider sliderWithValue:param.value
                                            minValue:0.0
                                            maxValue:1.0
                                              target:target
                                              action:@selector(sliderChanged:)];
        [slider setTag:param.id];
        [slider setFrame:NSMakeRect(214, y + 5, width - 236, 24)];
        [slider setContinuous:YES];
        [document addSubview:slider];
    }

    [scroll setDocumentView:document];
    [window setContentView:scroll];
    auto* handle = new Vst3UiWindow{window, scroll};
    return handle;
}

void* Vst3UI_GetContentView(Vst3UiWindow* window) {
    if (!window || !window->contentView) return nullptr;
    return (__bridge void*)window->contentView;
}

void Vst3UI_ShowWindow(Vst3UiWindow* window) {
    if (!window || !window->window) return;
    if ([window->window isMiniaturized]) [window->window deminiaturize:nil];
    [window->window makeKeyAndOrderFront:nil];
    [NSApp activateIgnoringOtherApps:YES];
}

void Vst3UI_HideWindow(Vst3UiWindow* window) {
    if (!window || !window->window) return;
    if ([window->window isMiniaturized]) [window->window deminiaturize:nil];
    [window->window orderOut:nil];
}

bool Vst3UI_IsWindowVisible(Vst3UiWindow* window) {
    return window && window->window
        && ([window->window isVisible] || [window->window isMiniaturized]);
}

void Vst3UI_SetWindowSize(Vst3UiWindow* window, int width, int height) {
    if (!window || !window->window) return;
    NSSize size = NSMakeSize(width > 0 ? width : 640, height > 0 ? height : 480);
    [window->window setContentSize:size];
}

void Vst3UI_CloseWindow(Vst3UiWindow* window) {
    if (!window) return;
    if (window->window) {
        [window->window orderOut:nil];
        [window->window close];
    }
    delete window;
}

void Vst3UI_SetEditorAction(Vst3UiWindow* handle, const char* label, bool enabled,
                          Vst3EditorActionFn action, void* userData) {
    if (!handle || ![handle->contentView isKindOfClass:[NSScrollView class]]) return;
    NSView* document = [(NSScrollView*)handle->contentView documentView];
    NSButton* button = objc_getAssociatedObject(document, "Vst3EditorActionButton");
    if (!button) {
        Vst3EditorActionTarget* target = [Vst3EditorActionTarget new];
        button = [NSButton buttonWithTitle:@"Original UI" target:target action:@selector(perform:)];
        [button setFrame:NSMakeRect(295, document.frame.size.height - 35, 205, 26)];
        [button setAutoresizingMask:NSViewMinXMargin | NSViewMinYMargin];
        objc_setAssociatedObject(button, "Vst3EditorActionTarget", target, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
        objc_setAssociatedObject(document, "Vst3EditorActionButton", button, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
        [document addSubview:button];
    }
    Vst3EditorActionTarget* target = objc_getAssociatedObject(button, "Vst3EditorActionTarget");
    target.action = action; target.userData = userData;
    button.title = label ? [NSString stringWithUTF8String:label] : @"Original UI";
    button.enabled = enabled;
}

void Vst3UI_UpdateDefaultParameter(Vst3UiWindow* handle, int32_t id, double value) {
    if (!handle || ![handle->contentView isKindOfClass:[NSScrollView class]]) return;
    NSView* document = [(NSScrollView*)handle->contentView documentView];
    for (NSView* view in document.subviews)
        if ([view isKindOfClass:[NSSlider class]] && view.tag == id) [(NSSlider*)view setDoubleValue:value];
}

void Vst3UI_UpdateDefaultParameterText(Vst3UiWindow* handle, int32_t id, const char* text) {
    if (!handle || ![handle->contentView isKindOfClass:[NSScrollView class]]) return;
    NSView* document = [(NSScrollView*)handle->contentView documentView];
    for (NSView* view in document.subviews) {
        if ([view isKindOfClass:[NSTextField class]] && view.tag == id &&
            [view.identifier isEqualToString:@"Vst3ParameterLabel"]) {
            NSTextField* label = (NSTextField*)view;
            NSString* value = text ? [NSString stringWithUTF8String:text] : @"";
            label.stringValue = value ?: @"";
            label.toolTip = label.stringValue;
            if (label.frame.size.height < 32) {
                NSRect frame = label.frame; frame.origin.y -= 7; frame.size.height = 32;
                label.frame = frame;
            }
            label.font = [NSFont systemFontOfSize:11];
            label.maximumNumberOfLines = 2;
        }
    }
}

Vst3UiWindow* Vst3UI_CreateBitmapEditor(const char* title, Vst3EditorGestureFn gesture,
                                      Vst3EditorActionFn parameters, void* userData) {
    auto* handle = Vst3UI_CreateWindow(title, 345, 413);
    if (!handle) return nullptr;
    handle->window.styleMask &= ~NSWindowStyleMaskResizable;
    Vst3BitmapView* bitmap = [[Vst3BitmapView alloc] initWithFrame:NSMakeRect(0, 0, 345, 373)];
    bitmap.gesture = gesture; bitmap.userData = userData;
    bitmap.toolTip = @"Drag knobs to adjust parameters.";
    [handle->contentView addSubview:bitmap];
    objc_setAssociatedObject(handle->contentView, "Vst3BitmapView", bitmap, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    Vst3EditorActionTarget* target = [Vst3EditorActionTarget new];
    target.action = parameters; target.userData = userData;
    NSButton* button = [NSButton buttonWithTitle:@"Parameters" target:target action:@selector(perform:)];
    button.frame = NSMakeRect(12, 379, 130, 28);
    button.autoresizingMask = NSViewMinYMargin;
    objc_setAssociatedObject(button, "Vst3EditorActionTarget", target, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    [handle->contentView addSubview:button];
    return handle;
}

bool Vst3UI_UpdateBitmapEditor(Vst3UiWindow* handle, const uint8_t* bytes, size_t size) {
    if (!handle || !bytes || !size || size > 1024 * 1024) return false;
    Vst3BitmapView* bitmap = objc_getAssociatedObject(handle->contentView, "Vst3BitmapView");
    if (!bitmap) return false;
    NSData* data = [NSData dataWithBytes:bytes length:size];
    NSBitmapImageRep* rep = [[NSBitmapImageRep alloc] initWithData:data];
    if (!rep || rep.pixelsWide < 1 || rep.pixelsHigh < 1 || rep.pixelsWide > 4096 || rep.pixelsHigh > 4096
        || rep.pixelsWide * rep.pixelsHigh > 4 * 1024 * 1024) return false;
    NSSize dimensions = NSMakeSize(rep.pixelsWide, rep.pixelsHigh);
    NSImage* image = [[NSImage alloc] initWithSize:dimensions];
    [image addRepresentation:rep];
    if (!NSEqualSizes(bitmap.frame.size, dimensions)) {
        [handle->window setContentSize:NSMakeSize(dimensions.width, dimensions.height + 40)];
        bitmap.frame = NSMakeRect(0, 0, dimensions.width, dimensions.height);
    }
    bitmap.image = image;
    bitmap.needsDisplay = YES;
    return true;
}

void Vst3UI_SetBitmapEditorBusy(Vst3UiWindow* handle, bool busy) {
    if (!handle) return;
    Vst3BitmapView* bitmap = objc_getAssociatedObject(handle->contentView, "Vst3BitmapView");
    bitmap.busy = busy;
}

@interface Vst3EditorMenuTarget : NSObject
@property(nonatomic, assign) uint32_t selection;
- (void)choose:(NSMenuItem*)sender;
@end
@implementation Vst3EditorMenuTarget
- (void)choose:(NSMenuItem*)sender { self.selection = static_cast<uint32_t>(sender.tag); }
@end

static NSMenu* buildEditorMenu(const std::vector<Vst3EditorMenuItem>& entries, Vst3EditorMenuTarget* target) {
    NSMenu* menu = [[NSMenu alloc] initWithTitle:@""];
    menu.autoenablesItems = NO;
    for (const auto& entry : entries) {
        if (entry.separator) { [menu addItem:[NSMenuItem separatorItem]]; continue; }
        NSString* title = [NSString stringWithUTF8String:entry.title.c_str()] ?: @"";
        NSMenuItem* item = [[NSMenuItem alloc] initWithTitle:title action:@selector(choose:) keyEquivalent:@""];
        item.target = target;
        item.tag = entry.selection;
        item.enabled = entry.enabled;
        item.state = entry.checked ? NSControlStateValueOn : NSControlStateValueOff;
        if (!entry.children.empty()) item.submenu = buildEditorMenu(entry.children, target);
        [menu addItem:item];
    }
    return menu;
}

uint32_t Vst3UI_ShowBitmapEditorMenu(Vst3UiWindow* handle, const std::vector<Vst3EditorMenuItem>& entries,
                                    double x, double y) {
    if (!handle) return 0;
    Vst3BitmapView* bitmap = objc_getAssociatedObject(handle->contentView, "Vst3BitmapView");
    if (!bitmap) return 0;
    bitmap.pressed = NO;
    Vst3EditorMenuTarget* target = [Vst3EditorMenuTarget new];
    NSMenu* menu = buildEditorMenu(entries, target);
    [menu popUpMenuPositioningItem:nil atLocation:NSMakePoint(x, y) inView:bitmap];
    return target.selection;
}

void Vst3UI_SetDefaultEditorError(Vst3UiWindow* handle, const char* message) {
    if (!handle || !message || !*message || ![handle->contentView isKindOfClass:[NSScrollView class]]) return;
    NSScrollView* scroll = (NSScrollView*)handle->contentView;
    NSView* document = scroll.documentView;
    NSTextField* label = objc_getAssociatedObject(document, "Vst3EditorError");
    if (!label) {
        const CGFloat extra = 120;
        const CGFloat oldHeight = document.frame.size.height;
        NSRect frame = document.frame; frame.size.height += extra; document.frame = frame;
        for (NSView* view in document.subviews) {
            if ([view.identifier isEqualToString:@"Vst3ParameterHeader"]) {
                NSRect headerFrame = view.frame; headerFrame.origin.y += extra; view.frame = headerFrame;
            }
        }
        label = [NSTextField wrappingLabelWithString:@""];
        label.frame = NSMakeRect(16, oldHeight - 28, document.frame.size.width - 32, extra - 12);
        label.autoresizingMask = NSViewWidthSizable | NSViewMinYMargin;
        label.selectable = YES;
        label.textColor = [NSColor systemRedColor];
        label.font = [NSFont systemFontOfSize:12];
        [document addSubview:label];
        objc_setAssociatedObject(document, "Vst3EditorError", label, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
        [document scrollPoint:NSMakePoint(0, MAX(0, document.frame.size.height - scroll.contentView.bounds.size.height))];
    }
    label.stringValue = [NSString stringWithUTF8String:message] ?: @"Plugin worker stopped.";
    label.toolTip = label.stringValue;
}
