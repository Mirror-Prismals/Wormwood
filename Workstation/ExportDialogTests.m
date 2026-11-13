// Standalone native regression harness; not part of the application build.
// Compile this file alone (it includes the production implementation), with
// -fobjc-arc -framework Cocoa -mmacosx-version-min=13.0. Run in a macOS GUI
// session. No audio rendering or export files are created by this harness.
#import "ExportDialog.m"
#import <QuartzCore/QuartzCore.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>

static unsigned Checks;
static void Check(BOOL condition, NSString *message) {
    ++Checks;
    if (!condition) {
        fprintf(stderr, "ExportDialog regression failed: %s\n", message.UTF8String);
        exit(1);
    }
}

static NSString *JSON(NSDictionary *value) {
    NSError *error = nil;
    NSData *data = [NSJSONSerialization dataWithJSONObject:value options:0 error:&error];
    Check(data != nil, error.localizedDescription ?: @"encode harness JSON");
    return [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
}

static void Open(NSNumber *rate, double end) {
    char error[4096] = {0};
    NSString *input = JSON(@{@"start_seconds":@0, @"end_seconds":@(end), @"sample_rate":rate});
    Check(ww_export_dialog_open(input.UTF8String, error, sizeof(error)) == 0,
          [NSString stringWithUTF8String:error]);
}

static void Close(void) {
    char error[4096] = {0};
    Check(ww_export_dialog_close(error, sizeof(error)) == 0, [NSString stringWithUTF8String:error]);
}

static id Poll(void) {
    char output[65537] = {0}, error[4096] = {0};
    Check(ww_export_dialog_poll(output, sizeof(output), error, sizeof(error)) == 0,
          [NSString stringWithUTF8String:error]);
    NSError *failure = nil;
    id value = [NSJSONSerialization JSONObjectWithData:[NSData dataWithBytes:output length:strlen(output)]
        options:NSJSONReadingFragmentsAllowed error:&failure];
    Check(value != nil, failure.localizedDescription ?: @"decode native request");
    return value;
}

static void Update(NSDictionary *status) {
    char error[4096] = {0};
    NSString *input = JSON(status);
    Check(ww_export_dialog_update(input.UTF8String, error, sizeof(error)) == 0,
          [NSString stringWithUTF8String:error]);
}

static void Prepare(NSNumber *rate, double end) {
    Close();
    Open(rate, end);
    MirrorExportWindow.parentDirectory = [NSURL fileURLWithPath:NSTemporaryDirectory() isDirectory:YES];
    MirrorExportWindow.folderField.stringValue = [@"mirror-export-dialog-test-" stringByAppendingString:NSUUID.UUID.UUIDString];
    [MirrorExportWindow folderChanged:nil];
    Check(![[NSFileManager defaultManager] fileExistsAtPath:MirrorExportWindow.destination], @"fixture destination must be new");
}

static void Start(void) {
    [MirrorExportWindow start:nil];
    Check(MirrorExportWindow.active, @"start marks the pending request active");
    Check(MirrorExportWindow.requests.count == 1, @"start queues one request");
    Check(!MirrorExportWindow.startField.enabled && !MirrorExportWindow.exportButton.enabled,
          @"settings lock while the request is active");
}

static void ExactRates(void) {
    Prepare(@8000, 1. / 8000.);
    Check([MirrorExportWindow.rateMenu.selectedItem.representedObject isEqualToNumber:@8000], @"preserve 8000 Hz");
    Check(MirrorExportWindow.rateMenu.numberOfItems == 5, @"add one exact source rate");
    Open(@22050, 0.125);
    Check([MirrorExportWindow.rateMenu.selectedItem.representedObject isEqualToNumber:@22050], @"preserve 22050 Hz on reopen");
    Check(MirrorExportWindow.rateMenu.numberOfItems == 5, @"replace the previous extra rate");
    Open(@48000, 0.125);
    Check([MirrorExportWindow.rateMenu.selectedItem.representedObject isEqualToNumber:@48000], @"select a standard source rate");
    Check(MirrorExportWindow.rateMenu.numberOfItems == 4, @"remove obsolete extra rate");
    Start();
    NSDictionary *request = Poll();
    Check([request[@"sample_rate"] isEqualToNumber:@48000], @"marshal the selected rate");
    Check([request[@"end_seconds"] doubleValue] == 0.125, @"preserve fractional range");
    Check([request[@"stem_names"] isEqual:@[@"master_L", @"master_S", @"master_F", @"master_R"]], @"preserve four default stem names");
    Check([request[@"destination"] isEqualToString:MirrorExportWindow.destination], @"marshal the selected absolute destination");
}

static void PendingStartCancellation(void) {
    // A one-frame request could publish in the controller's very first step.
    // Cancellation before polling must therefore eliminate the start entirely.
    Prepare(@8000, 1. / 8000.);
    Start();
    NSString *destination = MirrorExportWindow.destination;
    [MirrorExportWindow cancel:nil];
    Check(Poll() == NSNull.null, @"cancel before polling must not expose a short start");
    Check(!MirrorExportWindow.active && !MirrorExportWindow.cancelRequested, @"pending cancellation settles locally");
    Check(MirrorExportWindow.exportButton.enabled, @"pending cancellation unlocks settings");
    Check(![[NSFileManager defaultManager] fileExistsAtPath:destination], @"cancelled request creates no output");
    Open(@8000, 1. / 8000.);
    Check(Poll() == NSNull.null, @"reopen cannot resurrect the cancelled start");

    Start();
    [MirrorExportWindow windowShouldClose:MirrorExportWindow.window];
    Check(!MirrorExportWindow.window.visible, @"user close hides the panel");
    Check(Poll() == NSNull.null, @"user close discards an unconsumed start");
    Open(@22050, 0.5);
    Check(!MirrorExportWindow.active && MirrorExportWindow.startField.enabled, @"reopen after pending close is editable");
    Check([MirrorExportWindow.rateMenu.selectedItem.representedObject isEqualToNumber:@22050], @"reopen applies new defaults after cancellation");
}

static void RunningCancellation(void) {
    Prepare(@8000, 1.);
    Start();
    Check([Poll()[@"action"] isEqualToString:@"start"], @"application consumes the start");
    [MirrorExportWindow dismiss];
    [MirrorExportWindow requestCancel];
    Check(MirrorExportWindow.requests.count == 1, @"running cancellation is queued exactly once");
    Check(MirrorExportWindow.active && MirrorExportWindow.cancelRequested, @"running cancellation awaits backend acknowledgement");
    Open(@48000, 2.);
    Check([MirrorExportWindow.rateMenu.selectedItem.representedObject isEqualToNumber:@8000], @"reopening a running job preserves its rate");
    Check([Poll()[@"action"] isEqualToString:@"cancel"], @"user close requests cancellation of a consumed start");
    Check(Poll() == NSNull.null, @"no duplicate cancellation follows");
    Update(@{@"phase":@"cancelled", @"progress":@0.25});
    Check(!MirrorExportWindow.active && MirrorExportWindow.exportButton.enabled, @"backend cancellation unlocks the panel");
}

static void ExplicitCloseInvalidatesRequests(void) {
    Prepare(@8000, 1. / 8000.);
    Start();
    Close();
    Check(Poll() == NSNull.null, @"application close invalidates pending start without queuing cancel");
    Check(!MirrorExportWindow.active && !MirrorExportWindow.cancelRequested, @"application close resets request flags");
    Open(@44100, 1.);
    Check(Poll() == NSNull.null, @"another session receives no old request");
    Start();
    (void)Poll();
    [MirrorExportWindow requestCancel];
    Close();
    Check(Poll() == NSNull.null, @"application close also invalidates pending cancellation");
    Check(!MirrorExportWindow.active && !MirrorExportWindow.cancelRequested, @"application close resets running-request UI");
}

static void ValidationErrorRetention(void) {
    Prepare(@48000, 1.);
    Start();
    (void)Poll();
    [MirrorExportWindow requestCancel];
    NSString *message = @"Source sample rate must match export rate; resample the source first.";
    Update(@{@"phase":@"failed", @"error":message, @"progress":@0});
    Check(Poll() == NSNull.null, @"failed request removes its obsolete queued cancellation");
    Check([MirrorExportWindow.details.string isEqualToString:message], @"show the exact validation error");
    [MirrorExportWindow requestCancel];
    Check(Poll() == NSNull.null, @"cancel without an active request queues nothing");
    Check([MirrorExportWindow.details.string isEqualToString:message], @"inactive cancellation preserves the error");
    [MirrorExportWindow dismiss];
    Open(@48000, 1.);
    Check([MirrorExportWindow.details.string isEqualToString:message], @"user close/reopen retains the error");
    Close();
    Open(@48000, 1.);
    Check([MirrorExportWindow.details.string isEqualToString:message], @"application close/reopen retains the error");
    Update(@{@"phase":@"rendering", @"progress":@1});
    Check(![MirrorExportWindow.statusLabel.stringValue isEqualToString:@"Export completed"], @"100 percent rendering alone is not success");
    Update(@{@"phase":@"completed", @"progress":@1, @"destination":@"/completed/example"});
    Check([MirrorExportWindow.statusLabel.stringValue isEqualToString:@"Export completed"], @"only completed acknowledges success");
}

struct ThreadResult { int code; char error[4096]; };
static void *OffMainClose(void *context) {
    struct ThreadResult *result = context;
    result->code = ww_export_dialog_close(result->error, sizeof(result->error));
    return NULL;
}

static void BridgeErrorsDoNotConsumeRequests(void) {
    Prepare(@8000, 1.);
    Start();
    char output[2] = {0}, error[4096] = {0};
    Check(ww_export_dialog_poll(output, sizeof(output), error, sizeof(error)) != 0, @"small output buffer is an error");
    Check(MirrorExportWindow.requests.count == 1, @"buffer failure does not consume the request");
    struct ThreadResult result = {0};
    pthread_t thread;
    Check(pthread_create(&thread, NULL, OffMainClose, &result) == 0, @"create wrong-thread rejection probe");
    Check(pthread_join(thread, NULL) == 0, @"join wrong-thread rejection probe");
    Check(result.code != 0 && strstr(result.error, "main thread") != NULL, @"close returns an explicit main-thread error");
    Check(MirrorExportWindow.active && MirrorExportWindow.requests.count == 1, @"wrong-thread close cannot mutate the main-thread dialog");
    Close();
}

int main(int argc, char **argv) {
    @autoreleasepool {
        Check(NSThread.isMainThread, @"run the native harness on the main thread");
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
        [NSApp finishLaunching];
        if (argc == 3 && strcmp(argv[1], "--capture") == 0) {
            Open(@48000, 120.5);
            NSView *view = MirrorExportWindow.window.contentView;
            // A content-view capture excludes the NSWindow backing surface.
            // Supply that exact native surface color for a readable capture.
            view.wantsLayer = YES;
            view.layer.backgroundColor = MirrorExportWindow.window.backgroundColor.CGColor;
            [MirrorExportWindow.window displayIfNeeded];
            NSBitmapImageRep *bitmap = [view bitmapImageRepForCachingDisplayInRect:view.bounds];
            [view cacheDisplayInRect:view.bounds toBitmapImageRep:bitmap];
            NSData *png = [bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
            Check([png writeToFile:[NSString stringWithUTF8String:argv[2]] atomically:YES], @"write real Cocoa view capture");
            Close();
            printf("Export dialog view captured.\n");
            return 0;
        }
        ExactRates();
        PendingStartCancellation();
        RunningCancellation();
        ExplicitCloseInvalidatesRequests();
        ValidationErrorRetention();
        BridgeErrorsDoNotConsumeRequests();
        Close();
        printf("ExportDialog native lifecycle checks passed (%u assertions).\n", Checks);
    }
    return 0;
}
