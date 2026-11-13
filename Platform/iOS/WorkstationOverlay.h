// Native UI-only drawing. The engine retains all hit testing and interaction.
#pragma once
#include <stdbool.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
enum WWWorkstationOverlayKind {
 WWWorkstationPluginSurface = 0,
 WWWorkstationSelectionRange = 1,
 WWWorkstationCursorLabel = 2,
 WWWorkstationCcParameterPicker = 3
};
// Call on the main thread with the full logical-viewport UIView. Reattach on
// every workstation entry: hide disables publication until the next attach.
// Inserts above rendered content but below existing native controls.
bool ww_ios_workstation_overlay_attach(void *parentView);
// Safe on the graphics worker or main thread; never call from an audio callback.
// Copies JSON/font path before returning and coalesces each of four channels.
// Coordinates are the unchanged desktop payload's top-left logical coordinates.
// width/height are the source viewport used to construct that payload (not DPI).
// CC picker uses its own `viewport`; other channels use width/height. `null`
// clears only this channel and requires no font. Fonts are loaded synchronously
// on first publication; UIKit drawing is asynchronous on the main thread.
// false means malformed JSON/payload, missing font, or no attached live session.
bool ww_ios_workstation_overlay_publish(uint32_t kind,const char *json,
 const char *fontPath,double width,double height);
// Clears all payloads and invalidates pending publications. Safe on any thread.
void ww_ios_workstation_overlay_hide(void);
#ifdef __cplusplus
}
#endif
