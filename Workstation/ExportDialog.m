#import <Cocoa/Cocoa.h>
#include <math.h>
#include <string.h>

// All UI and JSON marshalling belongs to the main thread. The export worker
// owns rendering and output publication; Cocoa only queues user requests.
static const size_t MirrorExportJSONLimit = 65536;

static int MirrorExportError(char *output, size_t capacity, NSString *message) {
    if (output && capacity) {
        const char *bytes = message.UTF8String ?: "native export dialog failed";
        size_t length = MIN(strlen(bytes), capacity - 1);
        memcpy(output, bytes, length);
        output[length] = 0;
    }
    return -1;
}

static BOOL MirrorExportMainThread(char *error, size_t capacity) {
    if ([NSThread isMainThread]) return YES;
    MirrorExportError(error, capacity, @"export dialog must run on the main thread");
    return NO;
}

static NSDictionary *MirrorExportObject(const char *input, char *error, size_t capacity) {
    if (!input) {
        MirrorExportError(error, capacity, @"export dialog JSON is missing");
        return nil;
    }
    size_t length = strnlen(input, MirrorExportJSONLimit + 1);
    if (length > MirrorExportJSONLimit) {
        MirrorExportError(error, capacity, @"export dialog JSON exceeds 65536 bytes");
        return nil;
    }
    NSError *failure = nil;
    id value = [NSJSONSerialization JSONObjectWithData:[NSData dataWithBytes:input length:length]
                                              options:0 error:&failure];
    if (![value isKindOfClass:NSDictionary.class]) {
        MirrorExportError(error, capacity, failure.localizedDescription ?: @"export dialog needs a JSON object");
        return nil;
    }
    return value;
}

static BOOL MirrorExportNumber(NSTextField *field, double *number) {
    NSScanner *scanner = [NSScanner scannerWithString:field.stringValue];
    scanner.locale = [NSLocale localeWithLocaleIdentifier:@"en_US_POSIX"];
    return [scanner scanDouble:number] && scanner.isAtEnd && isfinite(*number);
}

static BOOL MirrorExportFileName(NSString *name) {
    if (!name.length || [name isEqualToString:@"."] || [name isEqualToString:@".."]) return NO;
    if ([name lengthOfBytesUsingEncoding:NSUTF8StringEncoding] > 128) return NO;
    NSCharacterSet *forbidden = [NSCharacterSet characterSetWithCharactersInString:@"/\\:"];
    return [name rangeOfCharacterFromSet:forbidden].location == NSNotFound &&
        [name rangeOfCharacterFromSet:NSCharacterSet.controlCharacterSet].location == NSNotFound &&
        [[name stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet] length] > 0;
}

@interface MirrorExportDialog : NSWindowController <NSWindowDelegate>
@property(nonatomic, strong) NSTextField *startField, *endField, *folderField, *destinationLabel, *statusLabel;
@property(nonatomic, strong) NSPopUpButton *rateMenu;
@property(nonatomic, strong) NSArray<NSTextField *> *stemFields;
@property(nonatomic, strong) NSButton *chooseButton, *exportButton, *cancelButton;
@property(nonatomic, strong) NSProgressIndicator *progress;
@property(nonatomic, strong) NSTextView *details;
@property(nonatomic, strong) NSURL *parentDirectory;
@property(nonatomic, strong) NSOpenPanel *chooser;
@property(nonatomic, strong) NSMutableArray<NSDictionary *> *requests;
@property(nonatomic) BOOL active, cancelRequested, dismissed;
-(void)configure:(NSDictionary *)defaults;
-(void)applyStatus:(NSDictionary *)status;
-(void)requestCancel;
-(void)dismiss;
-(void)closeForApplication;
@end

@implementation MirrorExportDialog
-(NSTextField *)label:(NSString *)text frame:(NSRect)frame {
    NSTextField *label = [NSTextField wrappingLabelWithString:text];
    label.frame = frame;
    label.selectable = YES;
    [self.window.contentView addSubview:label];
    return label;
}
-(NSTextField *)field:(NSString *)text frame:(NSRect)frame {
    NSTextField *field = [[NSTextField alloc] initWithFrame:frame];
    field.stringValue = text;
    field.font = [NSFont monospacedDigitSystemFontOfSize:13 weight:NSFontWeightRegular];
    [self.window.contentView addSubview:field];
    return field;
}
-(NSButton *)button:(NSString *)title frame:(NSRect)frame action:(SEL)action {
    NSButton *button = [NSButton buttonWithTitle:title target:self action:action];
    button.frame = frame;
    button.bezelStyle = NSBezelStyleRounded;
    [self.window.contentView addSubview:button];
    return button;
}
-(instancetype)init {
    NSWindow *panel = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, 640, 720)
        styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable
        backing:NSBackingStoreBuffered defer:NO];
    if (!(self = [super initWithWindow:panel])) return nil;
    panel.title = @"Export audio";
    panel.releasedWhenClosed = NO;
    panel.hidesOnDeactivate = NO;
    panel.delegate = self;
    self.requests = [NSMutableArray array];
    [self label:@"Four mono WAV files: Left, Sub, Front Fill and Right.\nExport is before the listening master and its volume control."
          frame:NSMakeRect(20, 674, 600, 36)];
    [self label:@"Start (seconds)" frame:NSMakeRect(20, 647, 170, 18)];
    [self label:@"End (seconds)" frame:NSMakeRect(220, 647, 170, 18)];
    [self label:@"Sample rate" frame:NSMakeRect(420, 647, 180, 18)];
    self.startField = [self field:@"0" frame:NSMakeRect(20, 618, 180, 25)];
    self.endField = [self field:@"0" frame:NSMakeRect(220, 618, 180, 25)];
    self.rateMenu = [[NSPopUpButton alloc] initWithFrame:NSMakeRect(420, 617, 200, 27) pullsDown:NO];
    for (NSNumber *rate in @[@44100, @48000, @96000, @192000]) {
        [self.rateMenu addItemWithTitle:[NSString stringWithFormat:@"%@ Hz", rate]];
        self.rateMenu.lastItem.representedObject = rate;
    }
    [self.window.contentView addSubview:self.rateMenu];
    [self label:@"File names (without .wav)" frame:NSMakeRect(20, 587, 600, 20)];
    NSArray *labels = @[@"Left", @"Sub", @"Front Fill", @"Right"];
    NSArray *names = @[@"master_L", @"master_S", @"master_F", @"master_R"];
    NSMutableArray *fields = [NSMutableArray array];
    for (NSUInteger i = 0; i < labels.count; ++i) {
        CGFloat y = 556 - 34 * i;
        [self label:labels[i] frame:NSMakeRect(20, y + 2, 130, 22)];
        [fields addObject:[self field:names[i] frame:NSMakeRect(160, y, 460, 25)]];
    }
    self.stemFields = fields;
    [self label:@"New folder name" frame:NSMakeRect(20, 414, 130, 22)];
    self.folderField = [self field:@"Mirror export" frame:NSMakeRect(160, 412, 460, 25)];
    self.folderField.target = self;
    self.folderField.action = @selector(folderChanged:);
    self.chooseButton = [self button:@"Choose parent folder…" frame:NSMakeRect(16, 370, 215, 30)
                             action:@selector(chooseParent:)];
    self.destinationLabel = [self label:@"Choose where the new export folder will be created."
                                  frame:NSMakeRect(20, 328, 600, 37)];
    self.progress = [[NSProgressIndicator alloc] initWithFrame:NSMakeRect(20, 305, 600, 16)];
    self.progress.indeterminate = NO;
    self.progress.minValue = 0;
    self.progress.maxValue = 1;
    [self.window.contentView addSubview:self.progress];
    self.statusLabel = [self label:@"Ready to export" frame:NSMakeRect(20, 273, 600, 24)];
    self.statusLabel.font = [NSFont systemFontOfSize:14 weight:NSFontWeightSemibold];
    NSScrollView *scroll = [[NSScrollView alloc] initWithFrame:NSMakeRect(20, 80, 600, 183)];
    scroll.hasVerticalScroller = YES;
    scroll.borderType = NSBezelBorder;
    self.details = [[NSTextView alloc] initWithFrame:NSMakeRect(0, 0, 580, 183)];
    self.details.editable = NO;
    self.details.selectable = YES;
    self.details.richText = NO;
    self.details.font = [NSFont systemFontOfSize:12];
    self.details.textContainerInset = NSMakeSize(7, 7);
    self.details.verticallyResizable = YES;
    self.details.horizontallyResizable = NO;
    self.details.minSize = NSMakeSize(0, 183);
    self.details.maxSize = NSMakeSize(CGFLOAT_MAX, CGFLOAT_MAX);
    self.details.autoresizingMask = NSViewWidthSizable;
    self.details.textContainer.containerSize = NSMakeSize(580, CGFLOAT_MAX);
    self.details.textContainer.widthTracksTextView = YES;
    scroll.documentView = self.details;
    [self.window.contentView addSubview:scroll];
    self.cancelButton = [self button:@"Close" frame:NSMakeRect(400, 24, 100, 32) action:@selector(cancel:)];
    self.exportButton = [self button:@"Export" frame:NSMakeRect(510, 24, 110, 32) action:@selector(start:)];
    self.exportButton.keyEquivalent = @"\r";
    [self.window center];
    return self;
}
-(void)setControlsEnabled:(BOOL)enabled {
    self.startField.enabled = enabled;
    self.endField.enabled = enabled;
    self.rateMenu.enabled = enabled;
    self.folderField.enabled = enabled;
    for (NSTextField *field in self.stemFields) field.enabled = enabled;
    self.chooseButton.enabled = enabled && !self.chooser;
    self.exportButton.enabled = enabled && !self.chooser;
    self.cancelButton.enabled = !self.cancelRequested;
    self.cancelButton.title = self.active ? @"Cancel export" : @"Close";
}
-(NSString *)destination {
    if (!self.parentDirectory) return nil;
    return [[self.parentDirectory URLByAppendingPathComponent:self.folderField.stringValue isDirectory:YES] path];
}
-(void)folderChanged:(id)sender {
    (void)sender;
    self.destinationLabel.stringValue = self.destination ?: @"Choose where the new export folder will be created.";
}
-(void)configure:(NSDictionary *)defaults {
    self.dismissed = NO;
    if (!self.active) {
        for (NSString *key in @[@"start_seconds", @"end_seconds"]) {
            id value = defaults[key];
            if ([value isKindOfClass:NSNumber.class] && isfinite([value doubleValue]) && [value doubleValue] >= 0) {
                NSTextField *field = [key isEqualToString:@"start_seconds"] ? self.startField : self.endField;
                field.stringValue = [NSString stringWithFormat:@"%.17g", [value doubleValue]];
            }
        }
        NSNumber *rate = @48000;
        id suppliedRate = defaults[@"sample_rate"];
        if ([suppliedRate isKindOfClass:NSNumber.class]) {
            double value = [suppliedRate doubleValue];
            if (isfinite(value) && value >= 8000 && value <= 192000 && value == floor(value)) rate = @((NSInteger)value);
        }
        // The renderer does not resample. Retain an exact valid source rate,
        // including rates outside the four common choices, on every opening.
        while (self.rateMenu.numberOfItems > 4) [self.rateMenu removeItemAtIndex:4];
        NSInteger selected = -1;
        for (NSInteger i = 0; i < self.rateMenu.numberOfItems; ++i) {
            if ([[self.rateMenu itemAtIndex:i].representedObject isEqualToNumber:rate]) selected = i;
        }
        if (selected < 0) {
            [self.rateMenu addItemWithTitle:[NSString stringWithFormat:@"%@ Hz", rate]];
            self.rateMenu.lastItem.representedObject = rate;
            selected = self.rateMenu.numberOfItems - 1;
        }
        [self.rateMenu selectItemAtIndex:selected];
        [self setControlsEnabled:YES];
    }
    [self showWindow:nil];
    [self.window makeKeyAndOrderFront:nil];
}
-(void)chooseParent:(id)sender {
    (void)sender;
    if (self.active || self.chooser) return;
    NSOpenPanel *chooser = [NSOpenPanel openPanel];
    chooser.title = @"Choose the parent folder for your export";
    chooser.prompt = @"Choose folder";
    chooser.canChooseDirectories = YES;
    chooser.canChooseFiles = NO;
    chooser.allowsMultipleSelection = NO;
    chooser.canCreateDirectories = YES;
    if (self.parentDirectory) chooser.directoryURL = self.parentDirectory;
    self.chooser = chooser;
    [self setControlsEnabled:YES];
    __weak MirrorExportDialog *weakSelf = self;
    [chooser beginSheetModalForWindow:self.window completionHandler:^(NSModalResponse response) {
        MirrorExportDialog *dialog = weakSelf;
        if (!dialog) return;
        if (response == NSModalResponseOK && !dialog.dismissed) {
            dialog.parentDirectory = chooser.URL;
            [dialog folderChanged:nil];
        }
        dialog.chooser = nil;
        [dialog setControlsEnabled:!dialog.active];
        // Return to export settings after either choosing or cancelling. The
        // main GLFW window can otherwise take focus as the sheet disappears.
        if (!dialog.dismissed) [dialog.window makeKeyAndOrderFront:nil];
    }];
}
-(void)showError:(NSString *)message {
    self.statusLabel.stringValue = @"Export needs attention";
    self.details.string = message;
}
-(void)start:(id)sender {
    (void)sender;
    if (self.active || self.chooser || self.requests.count) return;
    [self.window makeFirstResponder:nil];
    double start = 0, end = 0;
    if (!MirrorExportNumber(self.startField, &start) || !MirrorExportNumber(self.endField, &end) || start < 0 || end <= start) {
        [self showError:@"Enter a finite start at or after zero and an end later than the start, in seconds."];
        return;
    }
    NSMutableArray<NSString *> *names = [NSMutableArray array];
    NSMutableSet<NSString *> *unique = [NSMutableSet set];
    for (NSTextField *field in self.stemFields) {
        NSString *name = field.stringValue;
        NSString *key = name.precomposedStringWithCanonicalMapping.lowercaseString;
        if (!MirrorExportFileName(name) || [unique containsObject:key]) {
            [self showError:@"Use four distinct file names, each at most 128 UTF-8 bytes, without slashes, colons or control characters."];
            return;
        }
        [names addObject:name];
        [unique addObject:key];
    }
    if (!MirrorExportFileName(self.folderField.stringValue)) {
        [self showError:@"Enter a new folder name, at most 128 UTF-8 bytes, without slashes, colons or control characters."];
        return;
    }
    NSString *destination = self.destination;
    if (!destination.isAbsolutePath) {
        [self showError:@"Choose a parent folder for the export."];
        return;
    }
    if ([[NSFileManager defaultManager] fileExistsAtPath:destination]) {
        [self showError:[NSString stringWithFormat:@"This destination already exists. Choose a new folder name.\n%@", destination]];
        return;
    }
    [self.requests addObject:@{@"action":@"start", @"start_seconds":@(start), @"end_seconds":@(end),
        @"sample_rate":self.rateMenu.selectedItem.representedObject, @"stem_names":names, @"destination":destination}];
    self.active = YES;
    self.cancelRequested = NO;
    self.progress.doubleValue = 0;
    self.statusLabel.stringValue = @"Preparing export…";
    self.details.string = destination;
    [self folderChanged:nil];
    [self setControlsEnabled:NO];
}
-(void)requestCancel {
    if (!self.active || self.cancelRequested) return;
    BOOL pendingStart = NO;
    for (NSDictionary *request in self.requests) {
        if ([request[@"action"] isEqualToString:@"start"]) pendingStart = YES;
    }
    if (pendingStart) {
        // Nothing has reached the application. Sending start then cancel could
        // publish a short export before the next poll observes cancellation.
        [self.requests removeAllObjects];
        self.active = NO;
        self.cancelRequested = NO;
        self.progress.doubleValue = 0.;
        self.statusLabel.stringValue = @"Export cancelled";
        [self setControlsEnabled:YES];
        return;
    }
    [self.requests addObject:@{@"action":@"cancel"}];
    self.cancelRequested = YES;
    self.statusLabel.stringValue = @"Cancelling export…";
    [self setControlsEnabled:NO];
}
-(void)cancel:(id)sender {
    (void)sender;
    if (self.active) [self requestCancel];
    else [self dismiss];
}
-(void)dismiss {
    self.dismissed = YES;
    [self requestCancel];
    [self.chooser cancel:nil];
    [self.window orderOut:nil];
}
-(void)closeForApplication {
    // The controller has already cancelled its owned job. No queued action may
    // survive into another workstation session. Preserve the last visible
    // error/result so reopening does not replace it with an invented success.
    self.dismissed = YES;
    [self.requests removeAllObjects];
    self.active = NO;
    self.cancelRequested = NO;
    [self.chooser cancel:nil];
    [self setControlsEnabled:YES];
    [self.window orderOut:nil];
}
-(BOOL)windowShouldClose:(NSWindow *)sender {
    (void)sender;
    [self dismiss];
    return NO;
}
-(void)applyStatus:(NSDictionary *)status {
    NSString *phase = [status[@"phase"] isKindOfClass:NSString.class] ? status[@"phase"] : @"";
    BOOL completed = [phase isEqualToString:@"completed"];
    BOOL failed = [phase isEqualToString:@"failed"];
    BOOL cancelled = [phase isEqualToString:@"cancelled"];
    BOOL terminal = completed || failed || cancelled;
    if ([phase isEqualToString:@"rendering"] || [phase isEqualToString:@"preparing"]) self.active = YES;
    if (terminal) {
        self.active = NO;
        self.cancelRequested = NO;
        // A late queued cancel no longer belongs to a running request. Leaving
        // it queued could replace a validation failure with an older job status.
        NSIndexSet *obsolete = [self.requests indexesOfObjectsPassingTest:^BOOL(NSDictionary *request, NSUInteger index, BOOL *stop) {
            (void)index; (void)stop;
            return [request[@"action"] isEqualToString:@"cancel"];
        }];
        [self.requests removeObjectsAtIndexes:obsolete];
    }
    id fraction = status[@"progress"];
    if ([fraction isKindOfClass:NSNumber.class] && isfinite([fraction doubleValue])) {
        self.progress.doubleValue = MAX(0., MIN(1., [fraction doubleValue]));
    }
    if (completed) {
        self.progress.doubleValue = 1.;
        self.statusLabel.stringValue = @"Export completed";
    } else if (failed) self.statusLabel.stringValue = @"Export failed";
    else if (cancelled) self.statusLabel.stringValue = @"Export cancelled";
    else if (self.cancelRequested) self.statusLabel.stringValue = @"Cancelling export…";
    else if ([phase isEqualToString:@"rendering"]) {
        self.statusLabel.stringValue = [NSString stringWithFormat:@"Exporting audio — %.0f%%", self.progress.doubleValue * 100.];
    } else if ([phase isEqualToString:@"preparing"]) self.statusLabel.stringValue = @"Preparing export…";
    NSMutableArray<NSString *> *lines = [NSMutableArray array];
    if ([status[@"error"] isKindOfClass:NSString.class] && [status[@"error"] length]) [lines addObject:status[@"error"]];
    if ([status[@"destination"] isKindOfClass:NSString.class]) [lines addObject:status[@"destination"]];
    if ([status[@"files"] isKindOfClass:NSArray.class]) {
        for (id file in status[@"files"]) {
            if ([file isKindOfClass:NSString.class]) [lines addObject:file];
            else if ([file isKindOfClass:NSDictionary.class] && [file[@"path"] isKindOfClass:NSString.class]) [lines addObject:file[@"path"]];
        }
    }
    if (lines.count) self.details.string = [lines componentsJoinedByString:@"\n"];
    [self setControlsEnabled:!self.active];
}
@end

static MirrorExportDialog *MirrorExportWindow;

int ww_export_dialog_open(const char *input, char *error, size_t capacity) {
    @autoreleasepool {
        if (!MirrorExportMainThread(error, capacity)) return -1;
        NSDictionary *defaults = MirrorExportObject(input, error, capacity);
        if (!defaults) return -1;
        if (!MirrorExportWindow) MirrorExportWindow = [MirrorExportDialog new];
        if (!MirrorExportWindow) return MirrorExportError(error, capacity, @"could not create the export window");
        [MirrorExportWindow configure:defaults];
        return 0;
    }
}

int ww_export_dialog_poll(char *output, size_t outputCapacity, char *error, size_t errorCapacity) {
    @autoreleasepool {
        if (!MirrorExportMainThread(error, errorCapacity)) return -1;
        id request = MirrorExportWindow.requests.firstObject ?: NSNull.null;
        NSError *failure = nil;
        NSData *data = [NSJSONSerialization dataWithJSONObject:request options:NSJSONWritingFragmentsAllowed error:&failure];
        if (!data) return MirrorExportError(error, errorCapacity, failure.localizedDescription ?: @"could not encode export request");
        if (!output || data.length >= outputCapacity) return MirrorExportError(error, errorCapacity, @"export request does not fit the output buffer");
        memcpy(output, data.bytes, data.length);
        output[data.length] = 0;
        if (MirrorExportWindow.requests.count) [MirrorExportWindow.requests removeObjectAtIndex:0];
        return 0;
    }
}

int ww_export_dialog_update(const char *input, char *error, size_t capacity) {
    @autoreleasepool {
        if (!MirrorExportMainThread(error, capacity)) return -1;
        NSDictionary *status = MirrorExportObject(input, error, capacity);
        if (!status) return -1;
        [MirrorExportWindow applyStatus:status];
        return 0;
    }
}

int ww_export_dialog_close(char *error, size_t capacity) {
    @autoreleasepool {
        if (!MirrorExportMainThread(error, capacity)) return -1;
        [MirrorExportWindow closeForApplication];
        return 0;
    }
}

// Track freezing has a separate provisional-completion window. Closing it is
// always Cancel; only OK publishes rendered audio to the document.
@interface MirrorFreezeDialog : NSWindowController <NSWindowDelegate>
@property(nonatomic,strong) NSTextField *label;
@property(nonatomic,strong) NSProgressIndicator *progress;
@property(nonatomic,strong) NSButton *accept;
@property(nonatomic) int action;
@end
@implementation MirrorFreezeDialog
-(instancetype)init {
 NSPanel *window=[[NSPanel alloc] initWithContentRect:NSMakeRect(0,0,420,155) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable backing:NSBackingStoreBuffered defer:NO];
 if((self=[super initWithWindow:window])){
  window.title=@"Freeze Track";window.delegate=self;window.releasedWhenClosed=NO;
  self.label=[NSTextField wrappingLabelWithString:@"Freezing track…"];
  self.label.frame=NSMakeRect(20,100,380,35);[window.contentView addSubview:self.label];
  self.progress=[[NSProgressIndicator alloc] initWithFrame:NSMakeRect(20,76,380,18)];self.progress.indeterminate=NO;self.progress.minValue=0;self.progress.maxValue=1;[window.contentView addSubview:self.progress];
  NSButton *cancel=[NSButton buttonWithTitle:@"Cancel" target:self action:@selector(cancel:)];cancel.frame=NSMakeRect(215,20,90,32);cancel.keyEquivalent=@"\033";[window.contentView addSubview:cancel];
  self.accept=[NSButton buttonWithTitle:@"OK" target:self action:@selector(accept:)];self.accept.frame=NSMakeRect(310,20,90,32);self.accept.enabled=NO;self.accept.keyEquivalent=@"\r";[window.contentView addSubview:self.accept];
 }return self;
}
-(void)cancel:(id)sender{self.action=2;}
-(void)accept:(id)sender{if(self.accept.enabled)self.action=1;}
-(BOOL)windowShouldClose:(NSWindow *)sender{self.action=2;return NO;}
@end
static MirrorFreezeDialog *MirrorFreezeWindow;
int ww_track_freeze_dialog(int op,double progress,const char *message,int finished){
 if(![NSThread isMainThread])return -1;
 if(op==0){if(!MirrorFreezeWindow)MirrorFreezeWindow=[[MirrorFreezeDialog alloc] init];MirrorFreezeWindow.action=0;MirrorFreezeWindow.accept.enabled=NO;MirrorFreezeWindow.progress.doubleValue=0;MirrorFreezeWindow.label.stringValue=@"Freezing track…";[MirrorFreezeWindow.window center];[MirrorFreezeWindow showWindow:nil];return 0;}
 if(op==1){int action=MirrorFreezeWindow.action;MirrorFreezeWindow.action=0;return action;}
 if(op==2){MirrorFreezeWindow.progress.doubleValue=progress;MirrorFreezeWindow.label.stringValue=message?[NSString stringWithUTF8String:message]:@"Freezing track…";MirrorFreezeWindow.accept.enabled=finished;return 0;}
 if(op==3){[MirrorFreezeWindow.window orderOut:nil];MirrorFreezeWindow=nil;return 0;}
 return -1;
}

#include "AudioImportDialog.m"
