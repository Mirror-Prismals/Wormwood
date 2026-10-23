# JACK integration plan

Wormwood's own code is Apache-2.0. JACK keeps its own licenses.
The connection diagnostic remains a passive check of the default server.
The combined cue additionally implements explicit preparation, playback,
completion, and release through the external shared-library boundary.
See output/av-verification.json for verified combined-cue results.

## Client boundary

The layer-one JackConnectionSystem uses the public client API through an
externally installed, replaceable shared `libjack` library. Wormwood will run as
an external client of the separate JACK server process. The backend/server is
not itself a Salamander System.

Use `libjack`, not `libjackserver`. Do not statically link JACK or embed the
server in Wormwood for this implementation. Do not copy JACK implementation
code into Wormwood. Any Rust binding dependency needs its own license entry.
These are project integration choices, not claims that all other approaches
are forbidden by JACK's licenses.

Passive connection diagnostics use JackNoStartServer. Combined-cue preparation
selects the separate named Wormwood server using JackNoStartServer and
JackServerName, then explicitly spawns an external jackd if unavailable. On this
Mac it opens built-in speakers at 48 kHz / 256 frames, with no capture input.
Preparation owns server/client/port resources until registered release; only a
server process spawned by this session is stopped. Shutdown requests SIGTERM
and waits, with a bounded forced-exit fallback. Existing servers are left running.
This startup choice is operational, not a license requirement. Native callbacks must retain tracked ownership while avoiding
allocation, blocking, or trace-file writes in the real-time callback.

## License basis and distribution

JACK2's client API is LGPL-2.1-or-later; the server is GPL-licensed.
The LGPL permits an application using the library to retain its own license
under section 6. Our selected path is the replaceable shared-library mechanism
in section 6(b), compatible with an interface-compatible modified library.
Wormwood's Apache license does not relicense JACK.

When distributing a JACK-linked application, include the required prominent
JACK/library notice and LGPL text, preserve applicable notices, and permit
modification and reverse engineering for debugging library modifications.
Dynamic linking does not remove these obligations.

The initial package will not redistribute JACK binaries. Bundling a separate
server is possible in principle and does not by itself make Wormwood GPL;
redistributed server/library copies carry their respective license and source
obligations. Any later bundling or library modification needs matching license
texts, notices, and compliant source provision for those components. Static
linking would require a different relinking/compliance arrangement.

This plan covers JACK only. ChucK and other dependencies need separate license
and integration decisions before inclusion.

## Primary references

- [JACK2 v1.9.22 client header and license](https://github.com/jackaudio/jack2/blob/v1.9.22/common/jack/jack.h)
- [JACK2 release: LGPL clarification for libjack](https://jackaudio.org/news/2019/10/06/jack2-v1-9-13-release.html)
- [LGPL 2.1 text, especially section 6](https://spdx.org/licenses/LGPL-2.1-only.html)

The load path can be overridden with `WORMWOOD_JACK_LIBRARY`. The override is
authoritative, and an invalid library is reported without silent fallback.
Third-party license texts and notices are in BuildSupport/Licenses and
THIRD_PARTY_NOTICES.md.

The finite diagnostic precomputes a ramped quiet tone before Ready. Its active
callback writes silence before Go, plays prepared samples after Go, and returns
to silence. A JACK input port receives the client's output for signal verification;
it does not record microphone input. Only scalar atomic telemetry crosses back
to the owning connection. JSON, tracing, polling for completion, client closure,
and server shutdown execute outside the real-time callback. Completion telemetry
retains Entity/Audicle IDs. This is not yet the general DAW scheduler, command
queue, streaming engine, or a sample-accurate audio/graphics clock design.

Windows Focusrite verification uses the external JACK2 PortAudio ASIO driver,
48 kHz / 256 frames, synchronous graph mode (-S), and server realtime priority
92 (-R -P 92 before the backend arguments). JACK2 maps Windows priorities 89..92
to MMCSS Pro Audio; its client priority is the server priority minus three.
The report queries JACK's realtime state and client priorities outside the callback.
The asynchronous mode produced 12–16 active xruns in short tests despite low
reported callback CPU load. Synchronous mode passed all three native graphics
paths with zero total and active xruns at the unchanged device/rate/period.
This does not establish sustained-load performance or end-to-end acoustic latency.

Scheduling references: [JACK2 Windows thread scheduling](https://github.com/jackaudio/jack2/blob/v1.9.22/windows/JackWinThread.cpp),
[MMCSS priority constants](https://github.com/jackaudio/jack2/blob/v1.9.22/windows/JackMMCSS.h),
and [engine/client priority relationship](https://github.com/jackaudio/jack2/blob/v1.9.22/common/JackEngineControl.h).
