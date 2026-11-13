#import <Cocoa/Cocoa.h>
#include <string.h>
#include <stdio.h>
#include <sys/stdio.h>

// Modeless chooser, owned and polled only by the workstation's main thread.
static NSOpenPanel *WWAudioChooser;
static NSData *WWAudioSelection;
int ww_audio_import_open(int plugins) {
    if (![NSThread isMainThread]) return -1;
    if (WWAudioChooser) { [WWAudioChooser makeKeyAndOrderFront:nil]; return 0; }
    WWAudioSelection = nil;
    WWAudioChooser = [NSOpenPanel openPanel];
    WWAudioChooser.title = plugins ? @"Import Plugins into User Library" : @"Import Audio into User Samples";
    WWAudioChooser.prompt = @"Import";
    WWAudioChooser.canChooseDirectories = NO;
    WWAudioChooser.canChooseFiles = YES;
    WWAudioChooser.allowsMultipleSelection = YES;
    WWAudioChooser.treatsFilePackagesAsDirectories = NO;
    WWAudioChooser.allowedFileTypes = plugins ? @[@"vst3", @"vst", @"clap", @"component"] : @[@"wav", @"wave", @"aif", @"aiff", @"mp3", @"m4a", @"aac", @"caf", @"flac"];
    NSOpenPanel *panel = WWAudioChooser;
    [panel beginWithCompletionHandler:^(NSModalResponse response) {
        if (WWAudioChooser != panel) return;
        NSMutableArray *paths = [NSMutableArray array];
        if (response == NSModalResponseOK) {
            for (NSURL *url in panel.URLs) if (url.isFileURL) [paths addObject:url.path];
        }
        WWAudioSelection = [NSJSONSerialization dataWithJSONObject:paths options:0 error:nil];
        WWAudioChooser = nil;
    }];
    return 0;
}
int ww_audio_import_poll(char *buffer, size_t capacity) {
    if (![NSThread isMainThread]) return -1;
    if (!WWAudioSelection) return 0;
    if (WWAudioSelection.length >= capacity) { WWAudioSelection = nil; return -1; }
    memcpy(buffer, WWAudioSelection.bytes, WWAudioSelection.length);
    buffer[WWAudioSelection.length] = 0;
    WWAudioSelection = nil;
    return 1;
}
void ww_audio_import_close(void) {
    if (![NSThread isMainThread]) return;
    NSOpenPanel *panel = WWAudioChooser;
    WWAudioChooser = nil;
    [panel cancel:nil];
    WWAudioSelection = nil;
}

int ww_import_publish_bundle(const char *source, const char *destination) {
    return renamex_np(source, destination, RENAME_EXCL);
}
