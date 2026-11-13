#import <Cocoa/Cocoa.h>
#import <CoreGraphics/CoreGraphics.h>
// Called only by the owning window's main thread, never by an audio callback.
// Read metadata without requesting a new display mode or refresh rate.
void ww_display_cadence(void *nativeWindow, double *values, uint32_t *displayID) {
    if (!nativeWindow || !values || !displayID) return;
    NSWindow *window=(__bridge NSWindow *)nativeWindow;
    NSScreen *screen=window.screen;
    if (!screen) return;
    NSNumber *number=screen.deviceDescription[@"NSScreenNumber"];
    *displayID=number.unsignedIntValue;
    CGDisplayModeRef mode=CGDisplayCopyDisplayMode(*displayID);
    if (mode) {values[0]=CGDisplayModeGetRefreshRate(mode);CGDisplayModeRelease(mode);}
    values[1]=screen.maximumFramesPerSecond;
    values[2]=screen.minimumRefreshInterval;
    values[3]=screen.maximumRefreshInterval;
}
