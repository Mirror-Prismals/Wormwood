# Native graphics verification

Run `python3 BuildSupport/Graphics/verify.py` from the workspace. It checks
formatting, builds/tests offline, and launches one bounded triangle cue per
backend. Each cue uses a real window, native rendering API, and GPU pixel
readback. OpenGL, Vulkan, and WebGPU are tested independently, with no fallback.
The script checks trace ordering and cleans out stale images before testing.

Results: output/graphics-verification.json. Per-backend images, reports, and
stdout/stderr logs are saved alongside it. A successful mock runtime test is
not sufficient for a native backend to pass. Native verification requires a
macOS GUI session and the installed development dependencies listed in README.md.

The initial renderers support the registered vertex-color triangle format.
One frame is in flight at a time; this is correctness scaffolding, not a
performance benchmark. Vulkan and WebGPU wait for submitted work before readback
and teardown. The native clients remain private to the layer-one graphics System.
Generated bindings live in Cargo's build output and use installed C headers.
