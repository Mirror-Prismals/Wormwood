# Isolated worker contract (version 1)

One worker process owns one plugin. Requests are serialized by one control thread.
The C++ transport in `Host/Vst2Worker.h` is blocking and **must not run in Cardinal's
realtime audio callback**. Cardinal's browser and stereo effect chain use this backend.
`Host/Vst2AudioBridge.h` now provides the fixed-block stereo-effect queue:
preallocated slots, a dedicated IPC thread, and two blocks of latency. Its callback
uses lock-free atomics and copies only; it neither allocates nor performs IPC.
Initial output is silent for two blocks. A missed output deadline returns dry
audio at the same delay for that block and increments LateBlocks; the worker
continues so later blocks can recover. Stale completed slots are reclaimed on
ring reuse. A full queue or worker error latches a fault with FailureReason,
returning delayed dry audio, never late/stale output.
The host must show the fault and explicitly stop/restart; audio callbacks must be
quiescent during those lifecycle operations. Parameter updates coalesce in atomic
slots. Project state requests require stopping the bridge and using the returned
worker handle, then restarting. `ProcessStream` accepts irregular sub-blocks up to
the configured block size, aggregates them, and adds one block of buffering;
`StreamLatencyFrames()` therefore reports three blocks total. Do not mix fixed
and streaming entry points in one started session. Instruments, latency
compensation across Cardinal tracks, and live transport synchronization remain
unimplemented. Cardinal supplies a generic parameter editor; original plugin
editor windows and their UI callbacks remain unsupported.

Each packet starts with two little-endian uint32 lengths: JSON bytes, audio bytes.
JSON is an object, UTF-8, at most 8 MiB; audio is at most 1 MiB. Requests carry
`version: 1`, an integer `id`, and `op`. Responses echo version/id, then `ok` and
`result`, or `error`. Audio is planar little-endian float32, channel-major. Plugin
native logs go to stderr; stdout is exclusively the framed response channel.

Operations:

- `open`: plugin path, explicit `libraries`, sample rate, maximum block size,
  startup budget, and explicit `experimental` guarded-import mode. Returns
  channels, parameters, binary identity, state capability, latency, and startup time.
  `architecture` defaults to `"x86_64"`; explicit `"i386"` selects the 32-bit
  audio runtime. The i386 path accepts a read-only `resource_root` mirror and
  rejects explicit shared guest libraries until that loader is implemented.
  Its response includes `architecture: "i386"` and disables original-editor
  support. Cardinal sends `"auto"`, which inspects bounded Mach-O headers and
  prefers x86_64 when both Intel slices exist. `optional_x86_64_libraries` are
  added to `libraries` only for the 64-bit runtime; explicit `libraries` are
  never silently discarded. Both architectures accept `resource_root`.
- `parameters`: parameter IDs, names, labels, displays, and normalized values.
- `set_parameter`: `index`, normalized `value`; returns the plugin's readback.
- `process`: `frames`, optional MIDI tuples and parameter pairs, plus input audio.
  With `parameter_metadata: true`, parameter changes schedule a refresh of
  dependent names and display text. At most one parameter record is returned
  per audio response as `parameter_info`, spreading metadata work across blocks.
  Cardinal's IPC thread collects these records for main-thread label updates;
  the audio callback performs no metadata work or locking.
  Returns output audio and processing duration. Uses the existing stopped-transport
  offline host context; live transport synchronization is not implemented yet.
- `save_state` / `load_state`: versioned plugin state. Plugins advertising chunks
  supply a bank chunk (at most 4 MiB, base64); others use current program/parameters.
  The binary SHA-256 must match before loading. Parameter fallback does not claim
  to preserve hidden DSP history. Chunk setter return conventions vary; callers
  verify meaningful readback rather than treating a return code as proof.
- `close`: stop processing and release plugin/native resources before acknowledging.

A malformed request, unsupported API, or guest failure is terminal: the process
returns a framed error when possible, cleans up, and exits. Clients reject wrong
IDs/versions/oversized responses and kill/reap the worker on a deadline or failed
exchange. C++ spawn uses an explicit Python executable/script argv, without a shell.
The plugin never executes natively and never launches through Rosetta. This is
process fault isolation, not a security sandbox or a realtime-performance promise.

`benchmark.py` excludes startup, warms each block size, and records worker and
client round-trip p50/p95/max plus deadline misses. Python control-client timing is
not a substitute for a production audio scheduler benchmark. `realtime_qualified`
remains false until the actual Cardinal transport/scheduler is measured.
