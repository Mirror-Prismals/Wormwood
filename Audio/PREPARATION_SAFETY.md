# Audio preparation and callback ownership

`EnvironmentAudioSystem.Begin` starts one `mirror-audio-prepare` thread after
native output attachment and Audicle Go, then waits for its readiness handshake
before activating native callbacks. The wait holds no worker or document lock.
Failed activation joins the worker and recovers its owned state before native
cleanup. This avoids accumulating captured input before its PCM producer exists.
The worker owns input-capture draining,
document rendering and native plugin processing independently of graphics and
window event handling. A preparation pass renders at most two native-sized
blocks. The thread polls the existing queues at one-millisecond intervals; it
is a non-real-time preparation thread, not a hard-real-time scheduler.

Before native activation, Begin initializes the thirty-second retrospective
history ring at the actual device rate. Capture epoch changes and clock rewinds
reuse its storage: a checked monotonic serial base invalidates earlier cells
without clearing the whole ring. This preserves the existing per-frame memory
footprint and prevents old samples from appearing in later capture gaps. The
first accepted block still establishes history time; preallocation creates no
synthetic recorded samples. Focused tests cover allocation reuse, forward and
backward epochs, MIDI clearing, missing-frame rejection and recall chronology.

The native JACK/RemoteIO callback continues to use the preallocated capture
and output SPSC queues. This change adds no callback mutex, allocation, JSON,
file access, editor call or thread wake operation. Rendering the current
allocating document implementation directly inside that callback is explicitly
excluded.

One serialized document owner prevents simultaneous edits and processing. A
worker pass holds that owner across its revision read, capture acceptance and
rendering, so an edit cannot invalidate a partially accepted block. Main-thread
UI/native plugin lifecycle commands use the same owner; graphics receives owned
snapshots and releases document ownership before rendering. Environment frame
submission also releases preparation ownership before `AppearanceSystem.View`.
Long document edits or native plugin control operations can still delay
preparation. This architecture isolates graphics stalls; it does not establish
hard-real-time deadlines for every document/plugin operation.

Worker document calls retain the registered `EnvironmentAudioSystem.Pump`
attribution and initiating Audicle. Their journal has a fixed 512-event capacity
and reports omitted events. The application flushes it; the callback never
observes or writes tracing data. Revision reads use the typed Snapshot projection
without reconstructing clips or querying plugin controller metadata.

Close joins preparation before releasing native output. Host teardown also joins
before releasing the document and native editors. The worker returns its context
to the joining thread so native plugin destruction remains on the main thread.
Preparation errors and panics stop playback and recording and remain visible in
transport status. Process crashes inside third-party native plugins are not
isolated by this worker.

The two main-thread-stall regressions use the real document renderer and
transport. Each consumes 12,800 exact source frames while the control thread
is blocked: one exercises playback, and the other exercises the production
capture branch with asymmetric stereo input. Both check for underruns; the
capture case also checks input drops and the stopped playhead. These, two startup-readiness/failure-ownership checks, and ten existing
capture/observation tests cover the worker lifecycle. Separate capture tests cover accepted
samples, looped recording, stopped monitoring and capture shutdown. Physical-device latency,
OS scheduling under load and native plugin-editor stability require native
verification; passing queue tests alone does not establish them.

## Dry stereo signal integrity

`Workstation/SignalIntegrityTests.rs` exercises document `RenderRange` output
through the native four-bus transport and stereo monitor fold. Three tests pass:
asymmetric channel impulses retain their timing, polarity and values above
unity; independent wideband samples remain exactly equal at 44.1, 48 and 96 kHz
across different render and callback block sizes; stopped live stereo monitoring
retains opposing-phase channels without cancellation. These tests select L/R
routing explicitly and do not insert a plugin. They establish that this dry
route adds no clipping, filtering or channel crossfeed, not that hardware output
or arbitrary plugins match another host.

The existing FF/FF track default remains a mono sum followed by the stereo
monitor's half-gain fold. World speaker monitoring deliberately adds cabinet
EQ, propagation delay and spatial summing. Neither route is the transparent
L/R path tested here. Playback currently rejects source/device sample-rate
mismatches; these tests do not establish sample-rate conversion quality.

## Native verification

The final 2026-11-02 macOS output-only check used real speakers at 48 kHz
with 256-frame JACK periods while the WebGPU panel benchmark ran. The native
trace confirms installed plugin discovery completed before native Start. Before
that change, its approximately 37 ms document lock produced a persistent
2,048-frame output queue. After moving discovery and retrospective allocation
before activation, the final queue was 256 frames (5.33 ms). This is software
queue depth at observation, not physical input-to-output latency.

The final measured 10.67-second window had one new DAW transport underrun and
zero ChucK VM underruns; one DAW underrun preceded that window. Capture reported
no queue drops, and shutdown released all Entities. The longest preparation
pass was 7.056 ms; some RenderRange spans exceeded the 5.33 ms native period.
The real-time deadline gap under heavy graphics load remains unresolved. Earlier
runs with no measured DAW underruns retained approximately 41–43 ms of queued
audio; they must not be substituted for this final result.

Both Apple worker QoS requests were accepted. Corrected master gain produced
peak output 0.002799177 for the diagnostic's requested gain of 0.05. The ordinary
duplex check failed when JACK/CoreAudio tried to create the microphone/speaker
aggregate device. The output-only override was confined to the diagnostic;
normal device selection is unchanged. Hardware round-trip latency and parity
with Ableton remain unmeasured.

The production capture path cannot obtain a second period of future live input
by changing a render-ahead target. Maintaining an intentional two-period margin
requires explicit transport priming/output-delay state and capture-clock
verification through Play, Seek, Loop and Record, or separate playback and live
monitor pipelines. No extra renders, fabricated live input, or discarded
recorded samples were introduced to hide the remaining underrun.

A registered transport regression additionally checks that capture-connected
Play/Pause apply master gain, monitor mode and speaker mute. Requested gain is
verified on actual queued PCM rather than only by inspecting status values.
Verification records are in output/checks/audio-host-safety-20261102.

On macOS/iOS, both the document preparation worker and ChucK VM worker request
user-interactive QoS once at thread startup. Existing transport/VM telemetry
reports whether the OS accepted that request and its error code otherwise.
This changes worker priority classification only; it is not a real-time
scheduling guarantee and introduces no callback work.

The capture path preserves every accepted input frame. Its queue can therefore
retain pipeline delay accumulated during startup or a scheduling stall even
when subsequent capture and playback rates match. Improving priority can reduce
future stalls; it does not remove already queued capture audio. This change does
not drop captured samples, change recording clocks, or alter buffering targets.

## macOS duplex startup follow-up

The separated microphone/speaker aggregate failure above is now addressed by
LayerOne's MacAudioAggregate.m and MacAudioRoute.rs ownership. Managed JACK uses
one device created through AudioHardwareCreateAggregateDevice, with explicit
capture-channel offsets when the speaker interface also has input channels.
Single-device, output-only, and externally managed JACK paths remain separate.
The owned aggregate is released after the JACK server stops, including failed
startup. Only generated aggregate UIDs with demonstrably dead process owners
are reclaimed after crashes. No OS default input/output is changed.

A native 12-frame Mirror launch on 2026-11-02 opened MacBook microphone capture
and speakers at 48 kHz / 256 frames, returned success and released all Entities.
Device inventory matched before and after both normal close and an injected
JACK spawn failure. This is launch/routing/cleanup evidence, not a latency or
underrun improvement claim. Logs: output/checks/mirror-launch-duplex-20261102.
