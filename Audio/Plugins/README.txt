Desktop plugin hosting

The C++ backends are ported from Cardinal, with independent local dependencies:
VST3 configure/create/native-editor/shutdown and bus processing from
BaseSystem/Vst3System.cpp; CLAP and Audio Unit backends from their corresponding
Systems; Host/Vst3UI.mm; and the isolated VST2 worker/audio/editor bridges.
The old game Host, DawContext, track mutex and global DAW
clock are not dependencies of this port. Microtonal helpers are retained with
the backends; the new document currently provides ordinary MIDI pitches.

VST3 SDK base/pluginterfaces/public.sdk, CLAP headers and nlohmann JSON are
vendored below vendor with their original license notices. build.rs builds the
native CMake target only for macOS. iOS never compiles or links these SDKs,
scanners, native editor code or workers. Its plugin API reports unavailable.

In the desktop workstation, select a document track and open the bottom panel.
Edison occupies Cardinal's FX insert row, centered vertically, with page arrows
and track arrows on the left. Upper arrows select mixer/inserts; lower arrows
select the previous/next eligible document track. The MIDI instrument is first,
then effects in chain order. Sockets represent actual loaded plugins only.
Press and release a socket to toggle its editor; releasing outside cancels.
Drag an effect past another socket to reorder, with a visible insertion marker
and edge scrolling; the instrument cannot move behind effects. Delete removes
the selected insert and owns that key until release, so a held key cannot also
delete a selected arrangement clip or note.
Wheel/trackpad scroll moves the chain continuously, clipped beside navigation.
Native window visibility
controls the plug model, including closing an editor with its title-bar button.
Opening an editor preserves socket position and spacing. Spacing and the scroll
limit reserve the connected GLB's actual horizontal bounds, including its cable;
Cardinal's painted 5-pixel overhang is not a measurement of this 3D assembly.
Face size is capped to the available row height on small/custom panels.
Track arrows are hidden and inert on the mixer page, whose other controls retain
pointer input. Dropping an effect onto navigation or outside the chain cancels
the reorder. Offscreen portions of sliding panels cannot receive input.
The approved rounded
56 mm socket and thick 34 mm plug are used without deforming either mesh.
The packet has no presentation tilt: +Z faces the camera. Cardinal's fake 2D
extrusion offset is retired; real depth comes from the authored geometry.
Selection/hover tint and depressed press feedback distinguish insert gestures.
The left catalog restores Cardinal's All, Components and Plugins columns and
STOCK PLUGINS, VST3 / VST2, USER_CLAP and USER_AUDIOUNIT groups. Press selects
a plugin; release on an existing eligible timeline lane loads it. Instrument
drops require MIDI; effects and U Shaping accept audio/MIDI. Hosted release elsewhere,
Escape, pointer loss or a layout/document change cancels without loading.
U Shaping retains Cardinal's separate selected-lane fallback when its release
has no direct row, including outside the timeline. Selection remains visible
and category scroll positions are retained. The
port-only right-click menu is retired; it has no matching Cardinal workflow.
Search, other library categories, and free vertical track placement remain
outside this incremental port. The original browser-to-insert-row contract does
not exist: preserved plugin drops target timeline tracks. New inserts
start with their editors closed; click the Edison socket to open one. Loading
a MIDI instrument replaces the old instrument only after the replacement
initializes successfully, retaining that track's effects and other tracks.

The control thread scans only the application's USER/USER_VST3, USER/USER_VST2,
USER/USER_CLAP and USER/USER_AUDIOUNIT folders once before native audio activation.
This prevents initial factory discovery from blocking document preparation after
callbacks start. Graphics-only diagnostics retain a first-step fallback. System
and home-library plugin locations, CLAP_PATH and the installed Audio Unit registry
are not discovery sources. Audio Units retain Cardinal's generic parameter editor. VST3/CLAP prefer their own native editors with parameter-editor
fallback. VST3 and CLAP edits enter preallocated atomic latest-value mailboxes;
the final knob position is retained even after more than 256 edits before a block.
Processor parameter delivery is independent of controller display updates.
The native command boundary rejects calls outside its creating control thread;
third-party discovery, factories and UI additionally require macOS main. Stock-only
headless tests can own a host on their test thread. Render-side
CC automation reads metadata prepared by main-thread graph commands and calls
a separate native automation entry point. VST3 controller display updates are
serviced on main; the preparation worker never calls controller/UI methods.
Device-rate changes are prepared on main before worker rendering. A mismatched
render rate fails rather than reconfiguring plugins from the worker.

VST3 metadata notifications no longer reset DSP. Parameter-value notifications
refresh values; I/O and latency notifications quiesce processing until owned bus
buffers are rebuilt. Component-reload requests remain explicitly unsupported.
This does not add plugin-delay compensation or process isolation for VST3.

Native safety checks: BuildSupport/Workstation/verify_native_plugin_safety.py
--plugin <bundle> --class-name <class> --output <directory> [--asan]. The probe
uses Bridge.h shared with the implementation, exercises concurrent control/DSP,
parameter bursts and restart semantics without opening editors or audio devices.
An older ad-hoc /tmp stress probe used an obsolete C declaration; that earlier
result is not valid evidence for the current bridge ABI.

The workstation's existing render-ahead producer runs instruments/effects in
bounded stereo spans split at MIDI note and clip-clock boundaries. Hosted MIDI
instruments replace the built-in sine on their track. Effects run before track
gain, CC gain automation and L/S/FF/R routing. Consecutive ranges retain DSP
history; a discontinuous range resets voices and processing. The audio callback
only consumes prepared PCM. Loading/scanning/editors/JSON/allocations never run
inside JACK. Hosted CC parameter lanes use cached metadata and the processing
parameter API. Plugin-delay compensation, external MIDI devices, sidechain
routing and automatic project-file persistence remain outside this integration.
Command exposes parameter and opaque state save/restore. Native editor changes
pass through the native-sized audio preparation queue.
Same-pitch overlapping notes use the retained held-note model.

Export currently rejects a document with plugin inserts. Its frozen export
snapshot would otherwise lose the plugin graph, and reusing playback instances
would disturb their history. Independent export instances are a subsequent step.
Do not describe this port as the entire old DAW or as audibly/device verified.

VST2 retains Cardinal's experimental Apple Silicon Intel compatibility worker,
not an in-process native loader. The old implementation accepts effects only.
Platform/Vst2Compat holds its sources and pinned Unicorn requirement; install
its optional runtime with BuildSupport/setup_vst2_worker.sh. The native host
reports a missing runtime immediately instead of pretending the plugin loaded.
VST3, CLAP and Audio Units do not need Python. Legacy C++ guest library support
remains optional and is not bundled. Standard native formats are the first
playtest path. No worker or Python environment is included in the iOS package.

Current native verification: the installed Color Distortion VST3 passed the
corrected bridge probe under AddressSanitizer: 10,000 concurrent parameter
edits with 3,000 DSP blocks, plus 3,000 mixed control/worker edits across
1,498,200 bridge frames. No host memory errors were detected. This probe
opens no editor or audio device; the originally reported editor crash and
audible comparison still need a playtest. CLAP runtime behavior is unverified.

U Shaping EQ is the original LShaping stock implementation: two Butterworth
shelves, seven slopes, cutoff/gain ramps, bypass smoothing, independent realtime
and offline state, and response measurement. The inline 230x132 source graph
uses edge-frequency, middle-gain, slope, B/X and title-drag gestures. Stock EQ
does not create an Edison socket or native editor. The 3-D Edison meshes retain
their authored geometry and display real hosted plugin names above the face.

The graph and catalog use a pass-through native surface with Cardinal fonts and
source geometry. The five stock gem surfaces are real closed 26-plane gems
using the existing approved bottom-panel material and optical renderer. Their
composite control instances retain selection, bypass and physical B/X press.
Original row hover/selection faces and acknowledged successful-drop sparkles
are restored. Legacy per-surface theme overrides are not imported; the current
approved default requires no new theme setting. Installed third-party scanning/windows, native visual appearance,
and device playback remain unverified. Private checks cover registered Step to
stock loading/graph/Close, the actual native command/PCM bridge, and exact DSP
parity against the preserved source. They do not establish production readiness.

USER-folder discovery correction, November 7: removed standard VST3/CLAP roots,
CLAP_PATH expansion and global Audio Unit enumeration. VST2 already uses
USER/USER_VST2. The native CMake host build passed. A linked bridge scan against
an empty USER tree with CLAP_PATH set returned only the built-in Stock EQ and
zero third-party entries. Source review confirms each retained scanner uses its
format-specific USER folder. Intent inspection reports zero structural mismatches.
The full cargo build failed while writing its archive because the disk was full;
a rebuilt application and native DAW UI walkthrough remain unverified.
