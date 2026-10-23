# Third-party notices

Wormwood's own code is Apache-2.0. Third-party components retain their licenses.

## JACK external client library

Wormwood uses the JACK Audio Connection Kit client library, `libjack`, under
LGPL-2.1-or-later. It loads an externally installed shared library and connects
as an external client to a separate JACK server. No JACK library or server
binary or implementation source is bundled in this workspace.

The [LGPL 2.1 text](BuildSupport/Licenses/LGPL-2.1.txt) is included. Users may
modify Wormwood and reverse engineer it for debugging modifications to the
library. A compatible modified library can be selected through
`WORMWOOD_JACK_LIBRARY` without recompiling Wormwood. Preserve this notice
and the LGPL text when distributing this JACK-using application.

JACK remains copyrighted by its upstream contributors. The public client header
credits Paul Davis (2001) and Jack O'Quin (2004); see the [upstream source and
notices](https://github.com/jackaudio/jack2/tree/v1.9.22).

## Rust libraries used by the current macOS build

| Component | Version | Selected license text |
| --- | --- | --- |
| serde_json | 1.0.145 | [MIT](BuildSupport/Licenses/serde_json-1.0.145-MIT.txt) |
| serde_core | 1.0.229 | [MIT](BuildSupport/Licenses/serde_core-1.0.229-MIT.txt) |
| itoa | 1.0.18 | [MIT](BuildSupport/Licenses/itoa-1.0.18-MIT.txt) |
| memchr | 2.8.3 | [MIT](BuildSupport/Licenses/memchr-2.8.3-MIT.txt) |
| ryu | 1.0.23 | [Apache-2.0](BuildSupport/Licenses/ryu-1.0.23-Apache-2.0.txt) |
| libloading | 0.8.9 | [ISC](BuildSupport/Licenses/libloading-0.8.9-ISC.txt) |
| cfg-if | 1.0.5 | [MIT](BuildSupport/Licenses/cfg-if-1.0.5-MIT.txt) |

This list describes the current build, not every optional/target-specific entry
in Cargo.lock. Update it when enabling new targets/features or changing libraries.

## Native graphics interfaces and build tool

The graphics clients load externally installed GLFW (zlib license), Vulkan
loader (primarily Apache-2.0), and wgpu-native (MIT OR Apache-2.0) libraries.
Their binaries are not bundled here. The generated Vulkan interfaces use
headers attributed to The Khronos Group Inc., 2015–2026 (Apache-2.0 OR MIT).
WebGPU header attribution is WebGPU-Native developers, 2019–2023 (BSD-3-Clause).
Build-time bindgen 0.72.1 is BSD-3-Clause and is not linked into the engine.

Included texts:
- [GLFW](BuildSupport/Licenses/glfw-3.4-zlib.txt)
- [wgpu-native](BuildSupport/Licenses/wgpu-native-MIT.txt)
- [WebGPU headers](BuildSupport/Licenses/webgpu-headers-BSD-3-Clause.txt)
- [bindgen](BuildSupport/Licenses/bindgen-0.72.1-BSD-3-Clause.txt)
- [Apache-2.0](LICENSE)

Native API declarations are generated from installed headers at build time;
they are not copied engine implementations. This section records those
interfaces and tools and does not imply that all build-tool transitive packages
are part of a runtime distribution.

## iOS graphics packaging

The iOS device target links the official wgpu-native 27.0.4.0 static library
(MIT OR Apache-2.0) and MoltenVK 1.4.2 static library (Apache-2.0). The pinned
archives and recorded SDK hashes remain in local output/ios-sdk. The iOS target
uses Apple's system frameworks for native surfaces and audio; it does not bundle
or link a JACK server or libjack. Preserve upstream notices from the SDK archives
along with this notice when preparing a distributable application.
