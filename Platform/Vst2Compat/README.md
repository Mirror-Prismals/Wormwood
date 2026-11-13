# Rosetta-free VST2 compatibility prototype

The goal is general hosting of existing Intel Mac VST2 binaries on Apple Silicon,
without Rosetta, plugin source code, or per-plugin implementations. This directory
contains the first standalone execution experiment, **not finished DAW support**.
No existing DAW formats, scanning, saved projects, or System execution order change.

`macho.py` reads a `.vst` bundle or raw executable, selects its x86_64 slice,
validates its segments and classic dyld bindings, and finds the VST2 entry point.
`runtime.py` maps the image at its preferred guest addresses inside Unicorn and
calls its Intel code through the Mac x86_64 calling convention. Guest pointers
remain guest addresses; no plugin is passed to `dlopen`, launched as an Intel
process, or retried under Rosetta. Native ARM64 Python is required for execution.

The VST2 interface is declared in `Host/Vst2Abi.h`. The Python worker decodes the
64-bit ABI explicitly, including float arguments/returns, rather than treating
guest function pointers as callable native pointers. Python is used here to
validate compatibility machinery offline, not on Cardinal's audio thread.

## Build and check

From the repository root:

```sh
bash BuildSupport/check_vst2_compat.sh
```

That wrapper builds incrementally and installs the pinned dependency into a
workspace-local virtual environment. The individual steps are:

```sh
bash BuildSupport/build_vst2_compat.sh
python3 -m venv build/vst2-compat/venv
build/vst2-compat/venv/bin/python -m pip install -r Platform/Vst2Compat/requirements.txt
build/vst2-compat/venv/bin/python BuildSupport/Tests/test_vst2_compat.py \
  --effect build/vst2-compat/effect.vst --synth build/vst2-compat/synth.vst
```

The incremental fixture build cross-compiles an Intel stereo gain effect, a
MIDI instrument, and an effect using a separate guest library. It never executes
these binaries through macOS. Checks verify
actual emulated audio, parameter changes, note on/off, lifecycle, host callbacks,
and imported allocation/free functions. Passing them does not establish that an
unchanged third-party plugin works or that processing meets realtime deadlines.

## Inspect a real plugin

Inspection needs only Python's standard library and does not execute the plugin:

```sh
python3 Platform/Vst2Compat/probe.py '/path/to/Plugin.vst'
```

The JSON report includes dependencies, imported symbols, entry address, and
unsupported loader features. Attempt bounded offline execution with:

```sh
build/vst2-compat/venv/bin/python Platform/Vst2Compat/probe.py \
  '/path/to/Plugin.vst' --process
```

`--process` defaults to silent input and reports output peak/RMS; silence is normal for an
instrument without notes. It does not open an editor, send audio to a device,
modify the plugin, or claim that a successful probe proves full compatibility.
The Python API accepts MIDI channel events for instrument checks. `--signal sine`
uses a 440 Hz input at 0.125 amplitude. Repeated `--parameter INDEX=VALUE` options
set normalized parameters via the VST API; `--output-wav PATH` writes a new 16-bit
PCM file after successful processing and cleanup (existing files are not replaced).
The report includes parameter round trips and any WAV sample clipping.

To identify the first missing runtime function or data object actually used
during initialization:

```sh
build/vst2-compat/venv/bin/python Platform/Vst2Compat/probe.py \
  '/path/to/Plugin.vst' --trace-imports
```

This explicit diagnostic mode binds unresolved imports to distinct **unmapped**
guest guard regions. Calling a missing function or dereferencing missing data
stops the emulator with its symbol name, initialization phase, and recent calls.
It does not substitute successful no-ops. Initialization tracing stays separate
from normal processing. Explicit `--trace-audio` / `Runtime.diagnose_audio()` now
allows bounded offline blocks with the same unresolved-import guards still active.
An exercised missing import immediately fails the diagnostic. Success establishes
only the tested audio path; it does not validate remaining imports, the editor,
presets, or realtime behavior.
Normal `--process` continues to reject missing imports before executing code.

## Isolated worker integration foundation

`worker.py` now hosts one plugin in a separate native ARM Python process. The
bounded [versioned protocol](PROTOCOL.md) carries metadata, parameter changes,
planar float audio, and bank-chunk/current-program state. State files identify the
exact plugin binary. Invalid requests and guest faults end the worker session.

`Host/Vst2Worker.h` and `Platform/Vst2/Worker.cpp` provide Cardinal's native C++
transport, registered in both build source lists. It uses explicit subprocess argv,
framed local sockets, request deadlines, and kill/reap on failure. Its blocking
calls are reserved for a dedicated worker/control thread. `Platform/Vst2/HostedPlugin.cpp` now connects the backend to the existing hosted
insert/parameter presentation container. VST2 keeps its own format and project
identity; it does not load a CLAP binary. Discovery scans `USER/USER_VST2`, and
entries appear under **VST3 / VST2**. Startup runs asynchronously; after validation,
the existing generic parameter editor opens automatically.

This development preview supports stereo effects only. It requires the workspace
Python/Unicorn environment and uses the explicit guest C++ library when available.
Native editors and instruments are rejected/unsupported. The streaming delay is
shown in the editor title and is not yet compensated against other tracks. Export
and freeze are blocked while a VST2 instance exists. Project state stores the
initial/restored bank plus current generic parameter edits, avoiding blocking IPC
during playback saves; this does not snapshot autonomous hidden plugin-state changes.
Restore is applied to the newly loading worker before audio publication.

The native C++ client fixture verifies a real cross-process gain/parameter/state
round trip and failure cleanup. Python worker checks cover parameter and opaque
chunk state, malformed messages, plugin identity mismatch, and stalled-worker
termination. `benchmark.py` measures steady-state processing separately from startup;
the tested CamelCrusher configuration missed no deadlines over 64 measured blocks
at each of 128/256/512/1024 frames (48 kHz). At 256 frames, mean end-to-end time was
3.37 ms and maximum 3.62 ms against a 5.33 ms block duration. Startup took 9.23 s,
excluded from those timings. Its bank chunk restored all exposed parameter values.
See `build/vst2-compat/camelcrusher-worker-benchmark.json`. These short runs do not
qualify live operation under DAW load.

The native stereo `Vst2AudioBridge` now uses preallocated queues and a dedicated
IPC thread, with two-block latency. Callback checks verify no heap allocation,
correct delayed stereo samples, parameter delivery, overload fault latching,
delayed-dry fallback, and stop/restart. A gated protocol fixture verifies recovery
after exactly one missed deadline, including reclaiming late output on ring reuse.
The streaming adapter also verifies
17/73/38-frame chunks against continuous output with three-block total latency,
without callback allocations. The bridge is wired into hosted effect inserts. A cross-track latency compensation
policy and broader live-load qualification remain outstanding.

## Current boundary

### Original-editor investigation and fallback

The generic parameter editor remains the supported editor. Original-editor
experiments must run in a separate disposable process, never in the live audio
worker: a trapped editor call must not stop working audio or parameter control.
Any future original-editor integration must retain access to the generic editor
and fall back to it when editor creation, rendering, or its process fails.

`editor_probe.py` begins that investigation by querying the advertised editor flag
and rectangle. `--attempt-open` additionally calls editor open with a null parent
and, if accepted, idle/close. It records unresolved imports and lifecycle errors.
This is headless diagnosis, not a working native editor: null-parent rejection
does not prove a plugin is incompatible, and successful dispatch does not prove
rendering or input works. No host window pointer is passed into guest code.
`--parent guarded` supplies an unmapped guest token to trace the first view
operation beyond null-parent checks. It does not emulate a parent view; accesses
must trap until an actual view bridge is implemented.
`--parent offscreen` opts the disposable probe into an initial NSView bridge:
guest instances have separate native NSView backing, and `initWithFrame:`
translates the Intel stack-passed rectangle to ARM64's native ABI. It creates no
visible window, registers no Intel implementation with native Objective-C, and
traps on unsupported view methods. The bridge handles view attachment, rectangle
queries, invalidation, drag-type registration, tracking-area ownership, and guest
destructors. CamelCrusher completes open/idle/close with its resources available;
its guest editor view is removed on close, including deferred tracking cleanup.
`--attempt-draw --attempt-open --parent offscreen` additionally enters attached
guest `drawRect:` methods with an Intel stack-passed rectangle. This is a graphics
compatibility probe: lockFocus/unlockFocus select a bounded RGBA bitmap through
NSGraphicsContext, with opaque guest context handles and nested focus restoration.
Guest CoreGraphics drawing operations remain incomplete, and successful
callback dispatch alone does not establish that pixels rendered. Worker-local
mouse down/up/drag events and virtual window responder handling support isolated
input checks; keyboard, menus, and native desktop windows remain unsupported.
The implemented graphics subset covers state save/restore, transforms, clipping,
RGB colors and color spaces, paths/lines, and decoded image drawing. CamelCrusher
completes its original draw callback into a 345x373 bitmap and closes cleanly.
`--bitmap-output path.png` exports those pixels (or partial output after a trap),
converting premultiplied alpha for PNG and honoring the guest view's isFlipped
coordinate convention when creating its drawing context. This is still
an offscreen render; Cardinal presents those pixels in its own native window.
The isolated CamelCrusher probe also toggles Filter On (parameter 3: 0 to 1)
and drags Cutoff (parameter 4: 0 to approximately 0.15), then redraws and closes.
`--click X Y` and `--drag X Y END_X END_Y` use view-local coordinates and never
inject macOS events. These observations establish those interactions for this
plugin, not general editor compatibility.

`editor_worker.py` is a separate process entry point for the upcoming host
integration. Its `open`, `frame`, `mouse`, `click`, `drag`, and `close` requests reuse the
framed pipe envelope, with a PNG in the binary response payload. It cannot process
audio. Open accepts an optional matching saved state plus parameter overrides;
frame/input requests accept parameter updates, and gestures return changed
parameter values. An editor error terminates only this worker and reports generic
editor fallback. Cardinal's generic parameter window has an **Original UI**
button to launch this worker asynchronously. The original window has a
**Parameters** button, and the generic window stays available. Button clicks and
knob drags return parameter edits through the existing audio parameter queue;
mouse-down, drag, and mouse-up stream separately, so sound and the bitmap update
while dragging. Pending drag positions coalesce to the newest position; press and
release are preserved, with one worker request in flight at a time. Closing or failing the
editor worker leaves the audio worker running. Keyboard, popup menus, hidden
non-parameter preset changes, and arbitrary plugins' editor APIs remain unverified.
Resources can use the normal macOS installation or the read-only mirror at
`USER/USER_VST2_RESOURCES` (overridden by `CARDINAL_VST2_RESOURCE_ROOT`).
The optional `--resource-root` points at a workspace mirror of absolute resource
paths. Existing resources inside that root override read-only stdio, Carbon
path lookup, and ImageIO file loading; missing paths keep normal lookup behavior.
Overlay symlinks cannot redirect outside the root. This diagnostic option is not
enabled for Cardinal's live worker. It avoids system-wide installation merely
to investigate a plugin's skin assets.
Image loading accepts bridged CoreFoundation dictionaries with standard type
callbacks or null callbacks. Boolean constants and the ImageIO caching key use
opaque guest handles; custom dictionary callbacks still trap explicitly.
The read-only stdio bridge supports bounded `fscanf` string, character, and
decimal integer conversions, field widths, suppression, and integer lengths.
Other conversions fail explicitly. Output pointers use the guest AMD64
variadic ABI, including arguments passed on the stack.
Example (run in a separate process, with any required guest libraries):

```sh
build/vst2-compat/venv/bin/python Platform/Vst2Compat/editor_probe.py path/to/Plugin.vst \
  --attempt-open --output build/vst2-compat/editor-report.json
```

### First unchanged third-party test: CamelCrusher

On 2026-09-16, the VST bundle was extracted from the user's already-mounted
CamelCrusher installer into `build/vst2-compat/real-plugins/camelcrusher/`.
No installer scripts were run, no system plugin folders were modified, and the
binary is not included in source control. Its bundle identifies version 1.01;
the executable contains x86_64, i386, and PowerPC slices. Executable SHA-256:
`0a43999fcbd054105050148b9cb73170a28a685f45934c65b50efcc16f013f9d`.
This records the supplied test artifact, not independent verification of its origin.

Inspection successfully selects x86_64 and finds its entry point. It reports
275 imported symbols and 12 library/framework dependencies. Legacy x86_64
unsigned pointer relocation support now validates all 5,260 local relocations
and decodes 657 external/indirect pointer bindings, including symbols defined
inside the plugin. Local pointers retain their preferred addresses at zero slide.
The normal execution probe reaches runtime import resolution and stops there.
With diagnostic import tracing, the unchanged Intel binary **completes its
first initializer**, including string allocation/copying and C++ destructor
registration. Without a guest C++ library, it stops in initializer 2 of 4 at
`__ZNSt8ios_base4InitC1Ev` (GNU `std::ios_base::Init::Init()`). With the experimental
guest library below, all 31 library initializers complete and that constructor
executes Intel code. Guest standard streams, pthread-once callbacks, memory
zeroing, and default byte/wide-character conversion now allow C++ stream and
locale initialization to finish. A narrow native-ARM Foundation bridge then
handles autorelease-pool and mutable-string initialization. The guest Objective-C
registry now lets VSTGUI register four classes, 48 Intel methods, and four ivars.
**All 35 initializers complete (31 library plus four plugin), and CamelCrusher
returns a valid AEffect, opens/configures, and processes offline stereo audio.**
The tested plugin exposes 17 parameters. With no installed resource bank, its
initial parameter values were all zero, including master volume (-infinity dB),
so the first sine-input run returned silence. Setting master and distortion
parameters through the standard VST API produced finite, nonzero output across
1,024 frames at 48 kHz: peak 0.1441375464 and RMS 0.0920085572. The parameter
round trips matched their requested values. Startup and cleanup both completed.
This is a diagnostic audio-path result, not general plugin or realtime support.
A longer run processed 98,304 stereo frames (2.048 seconds at 48 kHz), completed
cleanup, and exported 16-bit PCM without clipping. Peak was 0.1441375464 and RMS
0.1198161197; the output differs from the input. Listening artifacts are
`build/vst2-compat/camelcrusher-listening-input.wav` and
`build/vst2-compat/camelcrusher-listening-output.wav`, with measurements in
`build/vst2-compat/camelcrusher-listening-report.json`. This verifies the render
and file contents, not a subjective listening assessment.
Read-only folder lookup, FSRef/CFURL/CFString conversion, scalar ARM libm calls,
and worker-local random state now let entry proceed through DSP table setup.
With `--startup-seconds 15`, that setup takes about nine seconds in this prototype.
The plugin then tries reading its MIDI configuration through `fopen(..., "rt")`;
the actual missing file returns ENOENT. Flat directory enumeration is implemented,
and an invalid FSRef after the missing resource-directory lookup returns
`errFSBadFSRef`. Native `TickCount`, guest-only dyld image count/header/name/slide,
and owned ImageIO resources are now bridged. The real run finds no installed
background image; editor/resource-loaded startup remains unverified. Saving MIDI
settings during close returns EROFS because the worker exposes read-only files.
No plugin-specific behavior was added.
The startup budget defaults to two seconds and can be set to 0.1–30 seconds per
call; the instruction cap scales with it. Audio and cleanup retain their two-second
wall-clock budgets. Audio skips Unicorn's expensive per-instruction counter;
initialization, control, and cleanup retain instruction limits. A runaway-DSP
regression verifies the audio timeout with a longer startup budget configured.
These limits do not themselves guarantee realtime performance.
Reports are `build/vst2-compat/camelcrusher-inspection.json` and
`build/vst2-compat/camelcrusher-compatibility.json`; the initialization trace is
`build/vst2-compat/camelcrusher-initialization.json`. The two-image trace is
`build/vst2-compat/camelcrusher-runtime-trace.json`; the audio-path report is
`build/vst2-compat/camelcrusher-audio-trace.json`.
Native object and file cleanup runs on initialization failure.

The fourteen parser/relocation checks and the Intel effect/MIDI fixture checks pass.
An additional execution check uses the effect's legacy indirect symbol table
instead of compressed dyld bindings and verifies its processed samples.
Runtime checks also cover C strings, key-manager state, static initialization
guards, nested guest finalizer callbacks, destructor ordering/one-time execution,
registration during finalization, and missing function/data guard faults.
The shared-library effect verifies dependency initialization order, a strong
plugin data definition overriding a weak library definition, cross-image audio,
guest symbol lookup, and rejection of overlapping mappings/ambiguous strong
exports. Default mutex checks cover ownership, contention, destruction, and
rejection of a recursive lock that would deadlock.
Additional checks exercise Intel inline getc/putc and Darwin FILE layout,
captured output/EOF/error flags, once-callback failure recovery and recursion,
native pool ordering/cleanup, UTF-16 constant-string conversion, mutable strings,
and retain/autorelease ownership. Registry checks verify class/metaclass lookup,
registration visibility, aligned and inherited ivars, duplicate methods,
integer/float Intel method dispatch, superclass calls, class disposal, and
rejection of unsupported native view creation. Formatting checks exercise
register/stack variadic arguments, floating-point arguments, truncation and
length reporting. Native Foundation is invoked only through the
explicit bridge described below; the plugin itself remains emulated.

Next work is resource-complete startup, more audio-path coverage, and worker integration.
The import list includes editor/framework APIs; it does not establish that every
import must execute during offline audio processing. No CamelCrusher-specific
code path has been added.

### Legacy C++ runtime build experiment

`BuildSupport/build_vst2_legacy_cpp.py` cross-compiles Apple's legacy GNU C++
runtime source for guest x86_64 execution. It requires an unmodified checkout of
`apple-oss-distributions/libstdcxx` at commit
`dad66dd62ed2aa2162c2c53778ff94336f49db95` under
`build/vst2-compat/upstream/libstdcxx`. Run with native ARM64 Python:

```sh
python3 BuildSupport/build_vst2_legacy_cpp.py
```

The incremental build uses the upstream GNU headers, SDK C/ABI declarations,
and four compiler tasks. It writes per-source diagnostics and `build-report.json`
under `build/vst2-compat/legacy-cpp`. A successful link produces a guest dylib at
a separate preferred address; unresolved runtime imports remain explicit.
The build neither rebuilds the plugin nor executes or installs the resulting
library. The worker can now explicitly load it into guest memory:

```sh
build/vst2-compat/venv/bin/python Platform/Vst2Compat/probe.py \
  '/path/to/Plugin.vst' --trace-imports \
  --guest-library build/vst2-compat/legacy-cpp/cardinal-legacy-cpp.dylib
```

The build compiled all 65 source files and linked successfully. The real-plugin
trace above verifies library mapping, binding, and initialization; it does not
establish complete C++ runtime or plugin compatibility. Additional libraries must
be supplied explicitly in dependency initialization order and have nonoverlapping
preferred addresses. There is no automatic dependency discovery or image sliding.
Symbol lookup uses unique explicit exports and coalesces weak definitions; it is
not a complete implementation of dyld's per-library namespace/search rules.
Upstream source and license notices remain in the local checkout; this experiment
does not vendor or package that source or its binary for distribution.

### Image metadata and resources

Dyld queries enumerate the guest plugin and explicitly supplied guest libraries,
return their mapped headers and stable guest copies of resolved binary paths,
and report zero slides. Native process images are never exposed.

`image_shims.py` bridges local-file ImageIO sources, decoded images, property
dictionaries, and basic image dimensions. Native objects stay behind owned CF
handles. Non-file URL schemes and custom decoding options are rejected. Drawing,
bitmap contexts, image-provider callbacks, and plugin windows remain unsupported.
Decoded images are not governed by the guest heap limit; this remains a trusted-
plugin prototype rather than a hardened media decoder sandbox.

### Platform streams and Foundation bridge

`stdio_shims.py` provides guest-only stdin/stdout/stderr data objects with Darwin's
x86_64 FILE layout. Stdin is empty by default; output is captured in memory with a
64 KiB limit per stream. Reads, writes, EOF/error state, pushback, and unbuffered
inline stdio paths are supported. Read-only `fopen` accepts regular files in `r`,
`rb`, or `rt` mode; it uses virtual descriptors and guest FILE storage, with no
native FILE pointers exposed. At most 64 files may be open at once; transfers are
bounded to 4 MiB. Seeking, descriptor reads, FILE pushback, and close/worker cleanup
are supported. Real OS errors are returned for missing files. Write modes return EROFS; wide-oriented streams and buffered modes remain unsupported. This bridge
can read files accessible to the worker and is not a filesystem security sandbox.

`filesystem_shims.py` bridges read-only folder/path queries through opaque 80-byte
FSRef tokens. `cf_shims.py` bridges explicit string and URL conversions using owned
guest handles, copied buffers, and native object cleanup. Flat directory iteration
supports bounded batches (up to 256 entries), catalog metadata, UTF-16 names, and
opaque FSRef results. Security-object fields and recursive iteration are rejected.
Catalog layouts are checked against the SDK for ARM64 and x86_64; the fixture
retains compile-time assertions for the Intel layout. Iterator handles are released
on close and worker teardown. `math_shims.py` bridges explicit scalar float/double libm
functions through XMM registers; floating exception flags and NaN payload fidelity
are not modeled. `TickCount`, `time`, `rand`, and `rand_r` are supported; random state belongs
to the worker and does not reseed the native process.

`objc_shims.py` bridges a small explicit set of NSAutoreleasePool, NSString, and
NSMutableString messages to native ARM Foundation. Guest objects use guest handles;
host object pointers stay in the bridge. Guest character data is copied with bounds,
including legacy ASCII/UTF-16 constant-string arguments. Pool initialization and
drain order are tracked, as are string ownership and pending autoreleases. No
arbitrary native method invocation, AppKit windows,
or Intel-to-native callback registration is enabled. Unsupported messages stop
with the receiver and selector. This is partial platform support, not a complete
Objective-C runtime or general Cocoa compatibility.

`objc_registry.py` adds worker-local dynamic class registration. System frameworks
provide read-only class names, superclass relationships and instance sizes;
plugin class definitions, selectors, ivars and Intel methods are held in the
guest registry. Registered Intel methods can dispatch inside Unicorn, preserving
the caller's integer/floating-point arguments and return registers. Simple
NSObject-derived guest objects are supported; native AppKit instances, native UI
callbacks, and guest `+initialize`/`-dealloc` callback lifecycles remain unsupported.
Registration does not install any Intel method pointer into the ARM runtime.

`format_shims.py` implements a bounded subset of sprintf/snprintf and their
va_list forms using the AMD64 variadic calling convention. Unsupported formats
(including `%n`, positional arguments, and long double) fail explicitly.

### Implemented and pending

Implemented: x86_64 thin/universal Mach-O images; classic dyld pointer bindings;
explicit additional guest dylibs, pointer weak binding and strong/weak export
selection, including unsigned backward binding offsets;
legacy unsigned 64-bit relocations and indirect symbol pointer tables at zero
slide; image initializers; VST2 open/configure/activate/process/close; normalized
parameters; MIDI channel messages; basic stopped transport information; planar
float processing; bounded malloc/calloc/free and memory/string imports.
Additional shared runtime support includes realloc/strdup, basic C++ new/delete,
single-threaded GNU reference-count updates and Darwin key-manager bookkeeping,
C++ static guards, default single-threaded Darwin mutexes, pthread-once callbacks,
and atexit/finalize callbacks; guest standard streams and default C-locale byte
conversion; log2/log2f; bounded variadic formatting; the limited native ARM
Foundation bridge and guest class registration/method dispatch described above.
A guest-only virtual libSystem
handle exposes implemented exports through emulated dlopen/dlsym; it never loads
a native dylib and does not claim to implement the whole library.

Not implemented: general Cocoa/Carbon/Objective-C, native AppKit instances and UI
callback bridging, arbitrary framework loading, complete GNU C++ runtime dependencies
(including full stdio/locale, RTTI, and exceptions),
threads/TLS, general filesystem writes/APIs, authorization services, 32-bit plugins, shell plugin
enumeration, export-trie-only entry discovery, full dyld namespace/weak-import semantics, chained fixups,
prebound images, section relocations, non-pointer legacy relocation forms,
legacy accumulating/double processing, project state/presets, native editors,
DAW automation integration, or realtime IPC. Unknown imports are reported before
execution in normal mode, never silently replaced with successful no-ops. Unsupported host
callbacks return the VST2 unsupported value and are included in the probe report.

This is not a hardened security sandbox. Execution has guest memory, instruction,
and time limits, but Unicorn is a native dependency; use trusted plugins. A
production worker also needs OS process restrictions and an external watchdog.

Next: use reports from unchanged real plugins to implement shared missing API
behavior and verify audio. Then add the production worker protocol and Cardinal
System contract/integration. A real plugin is a compatibility test, not a special
case to hardcode. The emulator backend can be replaced if another translator
offers better compatibility or realtime performance.

## References and dependency provenance

- [Unicorn](https://www.unicorn-engine.org/) provides CPU emulation. Its Python
  package is pinned separately; no emulator or Ardour implementation is vendored.
- [Apple Mach-O definitions](https://github.com/apple-oss-distributions/xnu/blob/main/EXTERNAL_HEADERS/mach-o/loader.h)
  describe the binary container and load commands.
- [C++ ABI runtime interfaces](https://itanium-cxx-abi.github.io/cxx-abi/abi.html#dso-dtor-runtime-api)
  describe destructor registration/finalization and initialization guards.
- [Apple's libstdcxx source](https://github.com/apple-oss-distributions/libstdcxx)
  supplies the experimental legacy C++ guest runtime described above.
  Modern native libc++ is not a drop-in
  replacement for an Intel plugin's GNU C++ object layout and exported symbols.
- [Apple's dyld binding decoder](https://github.com/apple-oss-distributions/dyld/blob/main/common/MachOAnalyzer.cpp)
  documents pointer binding streams and weak/strong-definition announcements.
- Darwin FILE and pthread layouts are checked against the installed SDK headers.
  [Apple's default C-locale implementation](https://github.com/apple-oss-distributions/Libc/blob/main/locale/FreeBSD/none.c)
  specifies its one-byte character conversion behavior.
- [Apple's Objective-C ABI definitions](https://github.com/opensource-apple/objc4/blob/master/runtime/objc-abi.h)
  describe legacy message references;
  [Clang's constant-string emission](https://github.com/llvm/llvm-project/blob/main/clang/lib/CodeGen/CodeGenModule.cpp)
  defines the guest literal layout used by the string bridge.
- The fixture and compatibility code here are newly authored. Distribution of
  a future bundled emulator must retain its applicable license notices.

### First Cardinal listening check

The existing extracted CamelCrusher bundle is linked at
`USER/USER_VST2/CamelCrusher.vst`; no system plugin installation is performed.
After a successful fast build, launch the workspace `./cardinal`, locate
`CamelCrusher [VST2]` under **VST3 / VST2**, and add it to an audio track.
Allow roughly ten seconds for first startup; the generic parameter editor opens
when ready. Because the installer support bank is not installed, all initial
parameters can be zero. For the previously verified sound, set MasterOn=1,
MasterMix=1, MasterVolume=0.5, DistOn=1, DistMech=0.5, and DistTube=0.5.
Listen on a solo track first; inspect playback, parameter response, bypass,
remove/reload, and save/reopen. Native editor support is a separate next phase.

### Python startup audit (2026-09-17)

The VST2 audio worker is launched only when creating a VST2 instance; the editor
worker is launched by Original UI. Discovery does not launch Python. Removing an
instance stops its audio worker; editor shutdown is asynchronous and bounded by
its in-flight request timeout.

A local CamelCrusher phase-timing comparison measured initialization at 7.462 s
before and 5.154 s after the audit, and editor initialization/open at 7.495 s
before and 5.175 s after. Parameter metadata took about 0.008 s in both cases;
the first bitmap took about 0.030 s. These are individual same-machine phase
measurements, not a general latency guarantee or end-to-end cold process timings.

Changes resolve import handlers once, cache scalar math ABI setup, avoid writing
an unused integer return register for floating results, and skip unused parameter
metadata when restoring the separate editor. Native libm calls, errno handling,
and emulation time/instruction limits remain intact. Compatibility, float/domain
edge cases, graphics, and real-plugin streaming drag checks passed. Remaining
startup cost is dominated by repeated emulated math calls and the Python
bindings' generic register conversion. Further work there is a distinct bridge
optimization, rather than parameter-list or resource-loading optimization.

### 32-bit Intel Mac foundation (2026-09-17, offline only)

`macho32.py` and `runtime32.py` isolate the new i386 work from the established
x86_64 audio/editor workers. Cardinal does not route plugins into this path yet.
The initial runtime implements bounded cdecl calls, basic memory imports, and
guarded unresolved imports, not full VST audio or Carbon editor hosting. It runs
Intel instructions in native ARM64 Unicorn without Rosetta. PowerPC and Windows
binaries are outside this Mac path.

`probe.py --architecture i386 BUNDLE` inspects a plugin without execution.
`probe32.py BUNDLE --initialize` additionally attempts its static initializers;
a `status: blocked` report identifies a compatibility stop, even when the probe
command exits successfully. It does not establish audio compatibility.

The user-supplied LFX-1310 1.4 Mac disk image was extracted read-only into the
workspace without running its installer. Its VST bundle contains i386 and PPC
slices. The i386 loader recognizes 7,164 local relocations, 217 pointer bindings,
375 jump stubs, and three initializers. Carbon/QuickTime dependencies remain
separate compatibility work. Resources and installers stay in ignored build
artifacts and are not redistributed with Cardinal.

Initial execution reached LFX's first static initializer. The current i386
bridges provide an explicit unavailable ColorSync profile result, owned native
RGB color-space tokens, bundle discovery with guest-only function gates, and
worker-local legacy key-manager state. Guest-only libSystem lookup, C++ exit
registration/finalization, and Carbon critical regions now allow all three LFX
initializers to complete, including cleanup. `probe32.py BUNDLE --open` also
attempts VST entry/open.
The shared printf formatter now has separate AMD64 and i386 argument readers.
Darwin i386 streams now use 88-byte FILE objects, captured stdout/stderr, signed
32-bit seeks, and the shared read-only file layer. The path bridge reuses only
the FSRef operations whose guest layouts are architecture-independent. Wide
strings use Darwin's default NONE/C encoding. Scalar libm calls use i386 stack
arguments and an emulated FLD instruction to return through x87 ST(0).

LFX now reaches its DSP table construction. Initializer/entry/open calls have
an explicit 0.1..30-second startup allowance (15 by default); other calls and
the complete cleanup phase retain two-second deadlines. Instruction ceilings
remain enabled.

LFX now opens and returns all 31 parameter records (128 programs, stereo I/O).
`probe32.py BUNDLE --parameters --resource-root MIRROR` exercises that path.
Float parameter returns are consumed with guest x87 FSTP, and setters receive
32-bit stack arguments. Bundles without classic resource files get an explicit
missing-resource result. Modal alerts are recorded and return userCanceledErr;
no native UI opens. Preference-directory writes return EROFS, and LFX shutdown
completes with its save-error alert declined.

`probe32.py BUNDLE --process --resource-root MIRROR` now renders 16 offline
blocks through 32-bit channel-pointer tables. Both replacing and legacy
accumulating processors are supported; output buffers are cleared each block.
Sample-rate/block-size setup, mains/start/stop callbacks, finite audio checks,
and sample-position reporting are covered by the i386 ABI checks.

An offline LFX check passed dry-through comparison, parameter readback, a
64-block Gate Reverb wet render, and shutdown. This establishes audio execution
for that plugin, not compatibility with every i386 plugin. The measured wet
render initially took about 0.89 seconds for 0.34 seconds of audio. Disabling
Unicorn's per-instruction counter only during DSP (matching the 64-bit path)
reduced it to about 0.15 seconds with identical measured output. Wall-clock
deadlines remain active, but Unicorn's timeout can miss tight translated loops.
The independent parent-worker deadline is essential; an infinite-loop i386 DSP
fixture now verifies that the parent kills and reaps the stuck worker.
This offline measurement does not establish in-app real-time reliability.

The audio worker accepts an explicit `architecture: "i386"` open request and
uses the same protocol for metadata, parameters, planar audio, and project
state. LFX passed a worker chunk-state save/change/restore round trip and stereo
processing. MIDI channel-event tables use four-byte pointers and are covered by
an ABI fixture; no real i386 instrument has been verified. Cardinal's host now
selects the guest architecture automatically, preferring x86_64 in universal
bundles. 32-bit editor support remains unfinished. i386
sessions advertise no original editor so the generic parameter UI stays available.

The first offscreen i386 editor probe now reaches Carbon custom-control creation.
Bounded CF strings (including i386 formatted varargs, ownership, and output
conversion) and guest-only Toolbox class registration are implemented. Class
callbacks and event type lists stay in guest memory/records; no native Intel
callback or native Carbon window is created. LFX's pre-open editor rectangle is
empty, so dimensions must be queried again after successful construction.
Custom-control creation now invokes the registered guest initialization handler
through deferred emulator callbacks and propagates its status. Event class/kind
and bounded typed parameter reads/writes, handler registration, and a composited
offscreen HIView tree are implemented. LFX reaches drag-handler registration next.
The API layouts follow Apple's archived 10.6 CarbonEvents.h, Controls.h, and
HIView.h; the window-content identifier is the platform's `('wind', 1)` constant.
Drawing and mouse delivery remain pending; the original-editor action stays
disabled until that path produces usable frames.

The editor probe additionally enables a host-first dyld image list: image zero
is a synthetic guest host header, and image one is the plugin. LFX's VSTGUI
resource discovery explicitly skips image zero; the audio-only prototype's
single-image list had therefore left its bundle reference unset and editor size
zero. This correction is currently opt-in (`editor_support=True`) while resource
APIs are built, preserving the established audio path. Read-only file URLs,
resource-only bundle creation, bounded bundle resource lookup, and FSRef output
now reach the plugin's real bitmap files. The next missing API is
`FSGetCatalogInfo`, on the legacy image-import path. Drag handler registration
and legacy version queries are also implemented; drag-and-drop delivery is not.

The FSSpec-only catalog query and graphics-importer lifecycle now resolve legacy
file identities to read-only ImageIO decoding. Images have opaque guest tokens,
reference counts, file-size/dimension/aggregate-pixel limits, and explicit native
cleanup. Source rectangles and retained CGImage outputs are implemented.

Offscreen 32-bit XRGB QuickDraw worlds now support pixel-map handles, accessors,
locking, image-import drawing, and disposal. A two-row exact-color BMP fixture
checks byte order, top-to-bottom row orientation, and released guest allocations.
LFX loads 17 bitmap surfaces and its editor opens at 745 x 75 pixels. An offscreen
diagnostic control-draw event reaches `CGContextSaveGState`; guest Core Graphics
context drawing is the next unimplemented stage. No editor frame has rendered,
and no 32-bit original UI is advertised by Cardinal yet.
