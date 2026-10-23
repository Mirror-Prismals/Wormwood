# Mirror (Prismals!)

Engine: Wormwood.

Rust engine using the author's Salamander framework. Read [DESIGN.txt](DESIGN.txt)
for the design; this file records implementation and verification status.

Current development/verification is macOS only. The new data-driven cue and
contract entry-point changes below are verified on this Mac; Windows/iPhone
results elsewhere in this file describe earlier snapshots and await refresh.

The registered runtime now creates and removes Entities, runs a layer-five cue
through preparation and Ready → Go → Finished, and attributes lifecycle, native
resource, and frame records to the active Audicle. Failed preparation/presentation
uses the declared cleanup path and reports cancellation separately. The first
scheduler supports one active cue and one prepared visual Entity at a time.
These are initial implementation limits, not limits of the design.

Verified on October 22, 2026 on Apple M1 Pro: all twenty-one runtime/contract tests
pass. OpenGL, Vulkan, and WebGPU each presented six native frames at 1280×960,
passed GPU pixel readback, and completed the tracked lifecycle and cleanup.
The three readbacks have matching center/background colors. Actual results are
in [output/graphics-verification.json](output/graphics-verification.json).
Windows graphics are also verified independently on the GTX 1050 Ti: OpenGL,
Vulkan, and WebGPU each rendered six 640×480 frames, passed matching GPU pixel
readbacks, and finished Entity/Audicle cleanup. All seventeen runtime tests passed
on Windows. Evidence is in output/windows-native-bf7ea23/windows-graphics.json.
Windows combined cues are now verified through the Scarlett 18i8 USB using
Focusrite ASIO at 48 kHz / 256 frames (5.33 ms per buffer). OpenGL, Vulkan, and
WebGPU each rendered sixty frames with GPU readback, played all 36,000 prepared
samples, reported zero xruns, and finished Audicle/Entity cleanup. All seventeen
Windows tests passed. Evidence is in output/windows-av-e9514cb. This finite
internal-loopback check does not establish acoustic latency or sustained DAW load.

## Build and run

The current native build is set up for macOS with development headers/libraries
for GLFW, Vulkan, wgpu-native, a macOS SDK/libclang, and glslangValidator available.
Build-time bindgen generates C ABI declarations from the installed headers.
WORMWOOD_NATIVE_PREFIX selects the header prefix (default /opt/homebrew).
WebGPU has macOS Metal and Windows HWND/D3D12 surface paths; both are verified.

```sh
cargo test --offline
cargo run --offline
cargo run --offline -- --triangle OpenGL --frames 120
cargo run --offline -- --triangle Vulkan --frames 120
cargo run --offline -- --triangle WebGPU --frames 120
```

The default invocation runs dependency/JACK diagnostics without opening graphics
windows. Each triangle invocation selects exactly the named API, prepares while
hidden, presents a bounded number of frames, verifies GPU pixel readback, and
cleans up. Native rendering failures are errors, not backend fallback requests.
The hello cue and its Entity data are in Procedures/Cues/hello_triangle.json. Shaders are in
Procedures/Shaders. Readback images and reports are written under output.

Native library overrides are WORMWOOD_GLFW_LIBRARY, WORMWOOD_VULKAN_LIBRARY,
WORMWOOD_WEBGPU_LIBRARY, and WORMWOOD_JACK_LIBRARY. An explicit override failure
is reported without switching to another API. The WebGPU library remains loaded
for the process lifetime so native worker/callback code remains mapped; cue-owned
GPU resources are explicitly released.

## Structure and limits

BaseSystem/LayerOne through LayerFive contain Systems. Systems/ contains JSON
contracts, Procedures/functionregistry.json catalogs hooks and input/output types,
and Procedures/systems.json declares ordering. BaseSystem/registry.json contains
only enabled/disabled booleans. Circuit-breaker controls are in their own file.
BaseEntity.rs contains data, without World or Level membership.

Host checks compiled signatures, payloads, declared calls, disabled Systems, and
the global halt. Native presentation requires Go. Entity removal is rejected while
its graphics resources remain prepared. Lifecycle records are restricted to their
owning registered hooks. This still does not provide complete compiler-enforced
prevention of every arbitrary native/direct-call bypass.

output/execution.jsonl records parent/child invocations and Entity/Audicle/native
lifecycle events with run IDs. File-backed tracing is diagnostic instrumentation;
it must not be used inside a real-time audio callback. The current runtime is a
finite cue runner, not the completed application/audio scheduler.

JACK now supports a finite combined visual/audio diagnostic through registered
Prepare, Start, Stop, and Release hooks. `--av` adds a sound Entity to the triangle
cue. Preparation activates a silent callback and connects its ports; Go starts a
precomputed, ramped 440 Hz tone at peak amplitude 0.025 for 750 ms. The callback
uses one pass over each supplied buffer and atomic telemetry, without allocation,
locks, file writes, JSON, or Host dispatch. Completion is collected outside it.

Verified October 22, 2026: OpenGL, Vulkan, and WebGPU each presented 60 frames
while JACK delivered 36,000 samples at 48 kHz / 256 frames. An internal JACK
loopback verified signal return. Each run reported zero xruns, completed the
Audicle, removed both Entities, and gracefully stopped its owned server. Twenty-one
unit/contract tests pass; a real missing-library test verified cancellation and
Entity/native cleanup before Ready. Results are in output/av-verification.json.

Run `cargo run --offline -- --av OpenGL --frames 60` (or Vulkan/WebGPU).
After building, `python3 BuildSupport/Audio/verify.py` checks all three paths and
the missing-library failure. It plays three quiet tones. The macOS startup path
uses a separate server named Wormwood, 48 kHz, 256 frames, built-in speakers, and
no capture input. WORMWOOD_JACKD selects the external server executable. An
existing Wormwood server is reused and never stopped; only the spawned child
is owned. This initial device selection is specific to this Mac.

These are finite diagnostics, not a stress test, acoustic output/latency test,
or completed DAW audio engine. Go coordinates audio and visual submissions;
sample-accurate audiovisual synchronization is not implemented. Queue-based
control, sustained scheduling, device changes/recovery, broader device selection,
iOS audio, and ChucK remain unfinished.
No game-specific, terrain, multiplayer, or DAW behavior is implemented.

Wormwood's own code is Apache-2.0; see [LICENSE](LICENSE). Dependencies keep their
own licenses. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and the
[JACK integration document](BuildSupport/Audio/JACK.md).

## Windows source transfer

Run `python3 BuildSupport/Windows/sync_windows.py` to transfer a fingerprinted
source snapshot through the existing `salamander-windows` SSH alias. Each transfer
extracts into a new directory under the Windows user's `WormwoodSync` folder,
checks every source file's SHA-256, and leaves existing checkouts untouched.
Build outputs, Git metadata, and local output are excluded; symlinks are rejected.
Snapshots remain under output/source-packages for reproducibility.

Use `--package-only` to prepare a snapshot without connecting. Optional `--run
build`, `--run test`, or `--run diagnostics` executes Cargo on Windows and retrieves
the log and source-tagged result under output/windows-sync. Cargo uses --locked;
Windows Rust and native development dependencies must already be installed.
This transfer tool does not establish Windows runtime/backend support.

## Windows combined-cue verification

`python3 BuildSupport/Windows/sync_windows.py --run verify` transfers a fresh
source snapshot, installs missing Rust/Clang/GLFW/Vulkan-header/shader tools
through the existing MSYS2 package manager, and retrieves a SHA-256-pinned
wgpu-native v27.0.4.0 GNU SDK into the user's WormwoodTools cache. Setup does not
refresh package databases or request a general system upgrade. Toolchain versions
are recorded in that cache. JACK remains the separately installed JACK2 server
and replaceable client library.

The driver builds/tests through SSH, then uses a temporary interactive scheduled
task to run the three native cue paths on the logged-in desktop. It records the
source fingerprint, session, GPU, audio loopback/xruns, native readback, and
cleanup results. The task is removed afterward. Logs/results return as
windows-results.zip under output/windows-sync. The combined Windows audio/visual
milestone passed on all three backends, as recorded above.

BuildSupport/Windows/environment.ps1 selects the installed SDKs for direct
Windows development. Its current machine profile selects ASIO::Focusrite USB ASIO
at 256 frames and 48 kHz. Override WORMWOOD_JACK_OUTPUT and WORMWOOD_JACK_PERIOD
for an explicit device/period. The 5.33 ms buffer budget is not measured round-trip
latency. Owned Windows servers use JACK synchronous graph scheduling and realtime
priority 92, placing the client at priority 89 in JACK2's MMCSS Pro Audio range.
The default asynchronous mode produced underruns on this machine; the verified
configuration keeps the same audio device, rate, and buffer size.
Windows-owned server shutdown currently uses process termination after closing
the JACK client; macOS uses SIGTERM with a bounded fallback. Sustained scheduling,
recovery and device configuration remain unfinished.

## iOS native host

The iOS target is a UIKit app linked to Wormwood's shared Rust static library.
The app packages the actual registries/contracts and invokes the registered
DiagnosticsSystem.ValidateRuntime hook through the same Host. App resources are
read from the signed bundle; traces and validation reports are written into its
Documents/Wormwood directory. Source fingerprints identify the tested build.

Run `python3 BuildSupport/iOS/build_device.py --team TEAM --device DEVICE` to
build, provision, sign, install, launch, and retrieve on-device Rust validation.
The local output/ios-signing.json supplies defaults on this Mac. Logs and final
verification are in output/ios-device-build. Signing uses the existing development
team; code-sign verification and provisioning-device membership are checked.

Verified October 22, 2026 on the connected iPhone: the app built, passed strict
code-sign verification and provisioning membership checks, installed, launched,
and returned its matching source-fingerprinted Rust validation report and execution
trace. See output/ios-device-build/verification.json for that bootstrap milestone.

Native cue source is now implemented for iOS OpenGL ES 3, Vulkan through MoltenVK,
and WebGPU through wgpu-native/Metal. The two latter paths reuse the shared native
renderers with UIKit CAMetalLayer surfaces. Each selected API has its own resources,
readback, and result; there is no fallback. Signed resources and writable readback
output have distinct paths. UIKit work is marshalled to its main queue while the
registered Rust cue runs on a worker, keeping the app event loop responsive.

IOSAudioConnectionSystem is a separate registered layer-one client of RemoteIO.
Its preparation configures AVAudioSession, observes the actual rate/period,
precomputes the finite ramped tone, and activates a silent callback before Ready.
Go arms the sound. The callback uses raw output buffers, bounded sample processing,
and atomic telemetry; it never dispatches Host calls, allocates, locks, or writes
files. Interruption, route changes, media-service reset, and app deactivation
cancel the diagnostic instead of resuming it implicitly. Native disposal precedes
freeing callback state. The returned IOSAudioReport records produced samples,
output-buffer peak, and callback sample-timestamp discontinuities. RemoteIO has
no JACK-style xrun counter, and output-buffer verification is not acoustic or
hardware-loopback verification.

The device build script downloads SHA-256-pinned official iOS SDKs (wgpu-native
27.0.4.0 and MoltenVK 1.4.2), then builds/signs/deploys and retrieves ios-cues.json
with a matching source fingerprint. Native source syntax and the Rust iOS target
check pass; all twenty-one host/callback tests pass. The native app has linked,
passed code-sign/provisioning verification, installed over the network, and run
on the A18 Pro iPhone. In the first observed run, OpenGL ES presented four frames
and RemoteIO reached Go before presentation was interrupted. The trace confirmed
cancellation and removal of all prepared audio/graphics/Entity resources. This
is interruption-cleanup evidence, not a successful full cue or a pass for all
three APIs. See output/ios-interruption-a3b04c1.

The updated native app temporarily disables automatic screen sleep during its
finite diagnostic and restores the previous setting afterward. Explicit scene
inactivity still cancels presentation. This updated snapshot is linked and waiting
for macOS's protected development-key signing approval; its device results remain
pending. The phone must be unlocked to launch. Once signed and installed,
`python3 BuildSupport/DeviceChecks/verify_ios.py` can retry launch and retrieve
source-matched reports without rebuilding. The verifier checks every frame,
Audicle stage, Entity creation/removal, and audio lifecycle against that run's
trace. It distinguishes native output-buffer verification from acoustic output.

## Paced desktop callback/render verification

`cargo run --offline -- --soak OpenGL --seconds 5 --repeat 2` runs repeated
finite audio/visual cues in one Host/process, using an explicit 16,667 us delay
between submitted frames. Vulkan and WebGPU use the same diagnostic entry point.
This diagnostic pacing is not a display/audio clock synchronization design.
Each cue plays 750 ms of prepared samples, then continues processing silence
while rendering runs. Between cues, the Host verifies that no Entity, active
Audicle, or prepared native resource remains owned.

`python3 BuildSupport/Audio/stress.py --rounds 2 --seconds 5` checks the three
macOS paths and their lifecycle traces. Verified October 22: six cues passed,
with 6.9–7.1 seconds of callbacks per cue, zero active xruns, native GPU readback,
and complete cleanup. Evidence is in output/callback-render-soak/verification.json.

`python3 BuildSupport/Windows/sync_windows.py --run verify --soak-seconds 5
--soak-repeats 2` adds the equivalent paced same-process checks after the normal
native verification. Verified October 22 on the Focusrite/GTX 1050 Ti: all 21
tests, all three normal cues, and two paced cues per API passed. Paced cues ran
5.5–5.8 seconds of callbacks each, with zero active xruns and matching lifecycle
and cleanup traces. Evidence is in output/windows-soak-ae076bd.

These are finite preallocated callback/render checks. They do not establish DAW
synthesis/streaming capacity, acoustic latency, general memory-leak freedom,
physical device unplug/recovery, or complete iOS background/device recovery.

## Data-driven cue composition

CueSystem.Run accepts a ProcedureRef containing a procedure name. Its declared
call to CueProcedureSystem.Load reads Procedures/Cues/<name>.json and validates
schema version 1 before any Entity is created. The definition contains one
required visual Entity specification and one optional sound Entity specification.
Systems execute the same tracked preparation, Ready, Go, completion/cancellation,
and native/Entity cleanup operations. Cue data cannot introduce arbitrary function
calls or bypass the contracts. Registry booleans and circuit-breaker controls
retain their separate responsibilities.

Run `cargo run --offline -- --cue hello_triangle OpenGL --frames 60` to select the
procedure explicitly. Vulkan and WebGPU use the same command with the named API.
The existing --triangle, --av, and --soak commands select hello_triangle; --triangle
suppresses its optional sound. The new command writes the existing av-<API>.json
report, now with mode "cue" and a procedure identifier. The initial native
connections still support a three-vertex vertex-color visual and the prepared
diagnostic sine sound; this change adds composition data, not additional renderers
or sound sources. Procedure loading is bounded to 256 KiB and confined to its cue
directory. Names are restricted to 1–64 ASCII letters, digits, underscores, or hyphens.
Unknown cue/Entity fields and unsupported schema versions are rejected.

System contracts can list entry_steps for explicitly callable Host entry points
with typed input. These are separate from startup init_steps, which require Unit input. The diagnostic command runs startup without launching a cue. All entry points and cross-System routes remain
validated against compiled signatures and the function registry.

Verified on macOS: 24 tests pass, including authored data reaching Entity creation,
rejection before Ready/Go, and caller/path checks. Native combined cues passed
OpenGL, Vulkan, and WebGPU with zero active xruns and preparation-failure cleanup.
Windows and iPhone validation of this snapshot is deferred.

## Process entry and System startup

main.rs remains seven lines. The library entry delegates desktop argument parsing
and process orchestration to Platform/Desktop.rs, outside the System boundary.
Arguments are validated before startup. Every desktop command loads the Host,
runs startup once, and then dispatches its requested operation. The no-argument
command performs startup diagnostics and exits; this remains a finite command
host rather than a completed interactive application.

Procedures/systems.json is only the ordered System startup list. The Host invokes
each enabled System's init_steps in that order. Contracts separately declare
entry_steps for callable operations with typed input. The unused update_steps
and cleanup_steps scheduling fields have been removed; the Host does not use a
universal phase/frame schedule. Normal execution follows registered calls and
callbacks. Cue cleanup continues through the existing contracted cleanup calls.

Startup records its plan, completed hooks, and any failure in output/startup.json
and host_startup trace events. A failure stops subsequent startup hooks. Startup
cannot be attempted twice on the same Host, and running another cue does not
repeat it. Diagnostic file writing now belongs to DiagnosticsSystem rather than
a System-specific switch in the Host.

Mac verification: all 27 tests pass. Tests cover startup-list ordering, separate
entry points, one-time startup across repeated cues, and stopping after failure.
OpenGL, Vulkan, and WebGPU combined cues passed with zero active xruns and native
cleanup, including the actual missing-library failure path. Windows/iPhone
validation of these changes remains deferred.

## Mac live cue mode

`cargo run --offline -- --live hello_triangle OpenGL` starts the usual ordered
startup and enters the registered cue. Select Vulkan or WebGPU explicitly to
use that connection. The presentation System keeps rendering until its native
window closes, with an explicit 16,667 us pacing delay. The prepared sound plays
once and then returns to silence while the visual remains. main does not gain
an application/frame scheduler or replay the System startup list.

Presentation returns its end reason to layer five. Window closure cancels the
Audicle; it is not reported as normal completion. Cancellation releases the audio
client promptly rather than waiting for the whole sound, then releases graphics
and removes both Entities through the existing declared routes. The resulting
report is output/live-<API>.json with stage Cancelled and reason window_closed.
Native failures remain errors. The finite cue/soak paths continue to require
normal completion in their verifiers; a cancelled soak is a failure.

The --close-after-frames N diagnostic option (1..600) requests closure through
GLFW's native close flag after N frames. It is labelled diagnostic_request in the
presentation report, distinct from window_event. `python3
BuildSupport/macOS/verify_live.py` verified this native close/cancellation path
on OpenGL, Vulkan, and WebGPU, including native GPU readback, both Entity removals,
audio release without claiming completion, and owned JACK server shutdown.
Evidence is in output/live-verification/verification.json. The native UI automation
service timed out when attempting human-style window controls, so a physical
window-button click is not claimed as an automated pass. The interrupted initial
test and its owned JACK server were explicitly stopped and recorded as aborted.

All 29 tests pass. They include cancellation before/after the first frame and
subsequent cue reuse. Normal three-API combined cues still pass with zero active
xruns and preparation-failure cleanup. Live mode remains a diagnostic: it retains
file-backed frame tracing, has no general input binding/replay controls, and does
not yet supply sample-scheduled audio cancellation/fades or a production recorder.
Windows/iPhone verification remains deferred.

## Persistent Mac application

`cargo run --offline -- --interactive hello_triangle OpenGL` opens one persistent
window. Select Vulkan or WebGPU explicitly for those paths. The first cue starts
automatically. Space triggers another cue while idle; Escape cancels the active
cue; Q or the native close control ends the application. Held Space does not
retrigger. The prepared visual Entity stays visible between events, and is removed
when the application closes. The diagnostic sound is prepared anew for each cue.
Only one active cue and one persistent visual are supported by this first slice.

ApplicationSystem (layer five) owns the application loop and interprets input.
CueSystem exposes Begin, Advance, Cancel, Status, and Close through contracts;
Begin returns after preparation/Go, and Advance polls an atomic completion flag
through JackConnectionSystem.Complete. It does not wait for the sound duration.
AppearanceSystem.Pump presents one frame of the persistent appearance and obtains
native window input through GraphicsConnectionSystem. Drawing an appearance left
in place is tracked by Entity even when no Audicle is active. The seven-line main
and startup-only systems.json remain unchanged in responsibility.

Preparation, native audio client/server teardown, rendering, and trace file writes
still run synchronously on the application thread. This is not yet a continuously
running DAW audio service or asynchronous asset-loading implementation. The JACK
callback retains its allocation-free, lock-free processing path. Audio cancellation
releases the client; sample-scheduled cancellation fades remain future work.

Run `python3 BuildSupport/macOS/verify_interactive.py` after building for the bounded
native check. `--verify-session` injects labelled application commands to finish,
replay, cancel, replay, finish, and close in one window. It does not synthesize
physical keyboard input. Results and invocation/resource traces are saved under
output/interactive-verification. Windows/iPhone runs remain deferred.

Verified on this Mac on October 22, 2026: all 32 Rust tests pass. The persistent
application check passed independently on OpenGL, Vulkan, and WebGPU with GPU
pixel readback, two completed sounds with zero active JACK xruns, one cancelled
sound, one native graphics preparation/release, and all four Entities removed.
A real missing-JACK-library failure cancelled preparation and released both
Entities and the native graphics session. Evidence is in
[output/interactive-verification/verification.json](output/interactive-verification/verification.json).

## Double-clickable Mac test app

Build with `python3 BuildSupport/macOS/build_app.py`. Open
output/macOS/Wormwood.app in Finder. The native launcher shows the controls and
lets you select OpenGL, Vulkan, or WebGPU and start another session after closing
the previous one. This is a local development bundle using the Mac's installed
JACK, GLFW, MoltenVK, and wgpu-native; external libraries and the JACK server are
not copied into it. It is locally ad hoc signed, not a notarized distribution.

Contracts, registries, cue procedures and shaders are packaged inside the app.
Writable reports, traces, captures and session.log go to
~/Library/Application Support/Wormwood/Diagnostics; the launcher has an Open
diagnostics button. No writes to the app bundle are required. The launcher is
platform UI outside System scope; engine execution still enters the contracted
ApplicationSystem. No terminal command is required for hands-on use.

`python3 BuildSupport/macOS/verify_app.py` relocates the bundle into a temporary
folder, exercises all three native paths with labelled diagnostic commands, checks
cue cleanup and verifies that the signed app contents were not modified at runtime.

## macOS output-device changes

Managed Mac JACK connections now select the current CoreAudio default output UID
when each cue prepares, instead of hard-coding the built-in speaker. Each engine
process uses its own owned server name; an old external Wormwood server is not
silently reused. Explicit WORMWOOD_JACK_OUTPUT/WORMWOOD_JACK_SERVER overrides keep
their explicit routing policy.

During an active interactive cue, JackConnectionSystem.RouteChanged queries the
system default at most every 100 ms on the application thread. A changed or missing
default emits audio_route_changed and cancels the current cue through its declared
cleanup route. The triangle/window remains alive. The next Space-triggered cue
selects the latest default, so changing macOS output or disconnecting Bluetooth
does not require restarting the app. The interrupted tone is not seamlessly moved
or resumed; continuous-stream migration/fades are still future work. No CoreAudio
query, allocation or logging was added to the real-time callback.

Output configuration records now include output_device_uid and
follows_system_default. Output policy/disconnection tests and a native default-UID
query pass on this Mac. Physical Bluetooth disconnection still needs a hands-on
check; the packaged native checks do not toggle Bluetooth or change OS settings.

The revised device-following build passes all 34 Rust tests. Packaged native checks
verify that each prepared sound records a nonempty CoreAudio output UID, follows
the system default, and owns its isolated JACK server. Subsequent preparation
errors are recorded as application_audio_error and leave an existing scene alive
for another trigger. Initial startup failure still reports an error to the launcher.


## Shared cube and flight baseline

The Mac launcher now defaults to Cube + player flight and retains Triangle + audio
as a separate test. WASD flies relative to the view, Space/Left Shift moves up/down,
holding right mouse captures the pointer for looking, and releasing it restores
the pointer. R resets the camera; Q or native close ends the session. No gravity,
collision, terrain, DAW or title-specific mechanics are added. This cube test is
silent, so Space controls ascent independently of sound-cue replay.

FlightSystem is a layer-five System. Movement/look/reset activation creates a
tracked Audicle, and returning to idle finishes it. Camera updates use
AppearanceSystem.View and GraphicsConnectionSystem.SetView through declared typed
contracts. main and the System startup list retain their existing responsibilities.
Time-based movement normalizes diagonal input, clamps frame stalls and pitch, and
cleans up the active flight Audicle before releasing the visual Entity.

This bounded twelve-triangle renderer presents one closed convex cube using CPU
back-face elimination and GPU perspective clipping. It does not yet supply a
scene depth buffer for multiple/intersecting meshes. OpenGL/WebGPU use bounded
uniform batches, while Vulkan keeps portable 96-byte push constants and refreshes
its command buffers for a changed view. The underlying APIs remain explicit.

`python3 BuildSupport/macOS/verify_flight.py` checks all three native cube paths,
scripted movement/look/reset, Audicle attribution, and resource cleanup. Evidence
is saved in output/flight-verification. Physical mouse and keyboard use still
needs the user's hands-on check. Project.json supplies independent app identity
and writable diagnostics directory for each game repository.

The shared cube baseline passes 39 Rust tests. All three Mac native cube checks
pass; the triangle/audio bundle checks still pass on all three APIs.


## Current game environment

Project.json is the authority for this copy's game and engine identity. The native
launcher now opens the game environment by default, with cube flight and triangle
audio retained as separate diagnostic choices. The two Git repositories are local
and independent; the source fork manifest records the earlier shared baseline.

The environment is a kinematic starter. The planet is an analytic Earth-radius
sphere presented through a camera-relative patch and is not destructible yet.
The water universe is homogeneous, unbounded, neutrally buoyant swimming with
thrust, drag, and an independently rotating personal frame. It has no floor or
water surface. Visual specks only provide movement/visibility cues. Neither scene
implements fluid dynamics, OGC, soil, excavation, compaction or building.

WASD moves; hold right mouse to look; Space/Shift moves up/down; Q/E rolls the
personal frame; R resets; Escape closes. On the planet, up/down follows the radial
direction, and Control boosts altitude-dependent flight speed. In water, up/down
follows the player's chosen up direction. These scenes are silent.

All 41 Rust tests pass for each copy. Both packaged game environments passed on
OpenGL, Vulkan and WebGPU with tracked navigation and resource cleanup. Evidence
is under output/environment-verification. Physical control feel still needs a
hands-on check. SimulationWorkshop/author_cases.json records the authors' original
OGC scenarios; those physics simulations and their UI are not implemented yet.
