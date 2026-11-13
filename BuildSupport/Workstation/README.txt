Five-mode native benchmark suite

Build a fresh app, then run:
  python3 BuildSupport/macOS/build_app.py --output-dir output/macOS-benchmark
  python3 BuildSupport/Workstation/benchmark_modes.py --app output/macOS-benchmark/MirrorPrismals.app

Default: three serial passes per mode, fullscreen WebGPU, 120 warmup frames,
at least 30 measured seconds, rotating mode order. --mode selects computer,
vr, ar, game or daw-only; repeat it for a subset. --repeats 1..5 and --seconds
1..120 bound the run. Game uses 2400 finite frames; --game-frames can increase
that up to 3600 if the requested measured duration would otherwise be short.
No install, preference edits, quality/optics overrides or synthetic stress project.

Computer uses the physical perspective-mapped theatre screen from the spawn
observer (about 4.7 metres away), not a close-up computer view. AR uses its
frozen player observer. VR uses the real pair and normal player movement/look,
without ray drawings or security-camera switches. These are game modes on the
Mac display, not headset hardware benchmarks. DAW-only uses the plain backdrop.
All four use real-time left-panel key open/close/reversal replay, without a
motion-bank override. Game-only keeps the spawn observer still while the world,
rendering and environmental audio run; it is not a traversal benchmark.

Reports separate complete-loop FPS from moving-frame loop intervals. A moving
frame has changed panel extension relative to the preceding frame; its interval
runs from this loop start to the next, including reporting/pacing. The last
motion interval is excluded because it has no following replay record. Tail
latencies, GPU query coverage, observer identity, audio counters and raw native
reports are retained. Missing/wrong observer modes, fixed-step animation,
missing draws, nonmoving VR navigation, incomplete query/counter coverage and
cleanup failures make the run fail. Results of other modes still run and remain
reviewable. Each process has a 300-second outer deadline.

Game GPU coverage is the primary opaque render pass only; its scope excludes
reflection, vegetation, water and presentation, so it is not total world GPU cost.
Game audio reports lifetime totals including startup/cleanup, explicitly not
measured-window underrun deltas. DAW-only without audio stress may also lack a
measured counter window; null is not zero. Application loop rate is not physical
scanout or event-to-effect latency. An empty default document is not a loaded
plugin/recording/music-project benchmark. Hardware/activity/power snapshots and
executable/assets identity are included. No thermal steady-state claim.

Pure coverage-gate checks:
  python3 -m unittest discover -s BuildSupport/Workstation -p test_benchmark_modes.py

Headless canonical workstation compute probe

profile_optics.py orchestrates bounded query-equivalence checks, generation,
focused tests, native build, before/after captures and headless compute probes.
Launch it through the normal process-job workflow. It records relevant input
hashes and runs both variants with one frozen executable and copied runtime
inputs. Concurrent checkout edits are recorded; changes to the copied inputs
invalidate the measurement. Authoring-only shaders are not runtime inputs.
Use --reuse-build path/to/validated-run --asset-root path/to/private-snapshot
to resume measurements after a successful build. Recorded runtime hashes must
match before reuse; the snapshot can preserve matching shaders while other
agents continue editing the checkout. This does not rebuild or package the app.
Pin the function catalog, System contracts, startup schedule and registry with
the executable too. The harness checks actual startup before drawing; intent
inspection alone cannot detect a registration missing from compiled code.
Its private region/cost-isolation variants are diagnostics, never app assets.
The production candidate keeps original geometry and optical quality settings.

Native WebGPU GPU-pass timing is opt-in with
WORMWOOD_WORKSTATION_GPU_TIMING=1. Results are written to
workstation-gpu-timing-WebGPU.json in the output directory. Unsupported adapters
report supported=false; CPU time is never relabelled as GPU time. The feature
is otherwise disabled, with no query buffers or readbacks in the ordinary app.

This diagnostic compiles the unchanged canonical workstation WGSL plus a small
8 x 8 compute entry. It does not replace the renderer or change geometry,
materials, ray samples, path budgets, resolution, lighting or optical buffers.
It tests compute dispatch cost separately from a fragment-pipeline draw.

Build with the installed wgpu-native 27 headers/library:
clang++ -std=c++17 -O2 -I/opt/homebrew/include
  BuildSupport/Workstation/compute_probe.cpp -L/opt/homebrew/lib -lwgpu_native
  -Wl,-rpath,/opt/homebrew/lib -o output/workstation-compute-probe
(Join those lines into one shell command.)

Invocation:
output/workstation-compute-probe shader.wgsl frame.bin width height output-prefix iterations

shader.wgsl is the ordinary generated canonical workstation shader, including
its existing vertex/fragment entries. The probe appends its compute entry itself.
frame.bin is the complete little-endian RGBA32F optical buffer, including the
control geometry, SSLE density and AgX LUT. Row 8 width/height must exactly match
the supplied dimensions; the probe refuses to silently change ray sampling.

Metal is required explicitly. Timestamp queries are enabled if the adapter
supports them; otherwise GPU times are reported as null and CPU completion
times remain available. Shader/pipeline creation and initial uploads happen
before measurements. One unmeasured warmup precedes the requested fixed count
of measured dispatches. Each dispatch waits for its own queue completion. GPU
timestamps bracket the compute pass, excluding query resolution and copies.
CPU times include command encoding, submission, timestamp resolution/copy and
completion wait, excluding query mapping, final image readback and compilation.
All async waits have a 60-second deadline. The process is finite and foreground;
the calling task should launch builds/runs with its normal process-job workflow.

Output-prefix files:
  .compute.wgsl : exact combined shader used
  .json        : adapter, feature support, CPU/GPU timings and output statistics
  .rgba32f     : tightly packed RGBA32F, top-left origin, display-encoded color
  .ppm         : RGB preview, clamped and rounded to 8-bit display color
  .error.json  : written only when an operation fails

Bindings: 0 unused appearance storage buffer, 1 complete optical storage buffer,
2 writable RGBA32F output. Pixel coordinates and stratified samples are the
same as the fragment entry. The scene still uses the original canonical ray
transport; this is not a cached picture or lower-quality substitute. No surface
or window is created, and the launcher is not started.

Private analytic-control candidates

analytic_controls.py reads a complete exported frame and canonical WGSL, then
writes private copies with appended acceleration descriptors. Example:
python3 BuildSupport/Workstation/analytic_controls.py --shader path/workstation.wgsl
  --frame path/complete.bin --output output/checks/private-control-candidate
  --mode exact-facets --rays 256
(Join those lines into one shell command.)

Modes:
  exact-facets: group original flat-shaded triangles into conservative plane
    slabs/AABBs, then refine only surviving facets with the original triangle
    equations, shading normals, material and traversal-order tie preference.
  analytic-construction: intersect the original authored 26-plane gem directly;
    recessed housings subtract the 10-plane chamfered cutter prism specified by
    the Blender authoring script. This is mathematically the authored model,
    but differs slightly from its quantized/triangulated GLB snapshot. Matching
    authored optical normals are retained where the construction face survives.

Both candidates replace only control-interior exit queries. Exterior/scene
queries, lighting, opacity, sample count and path budgets stay canonical. The
existing analytic meter window remains untouched. Unsupported construction
shapes fall back to the original function. No production asset or shader changes.

The generator records deterministic ray comparisons for each shape. Exact-facet
comparisons must match the original triangle query; construction comparisons
report distance, normal and miss differences without claiming equivalence.
Construction acceptance requires source dimensions/transforms and each original
triangle to lie within 20 micrometres of one expected construction face. The
reported maximum is a triangle-to-plane residual, not a global Hausdorff or
radiance-error bound. Pixel comparisons, critical-angle behavior and timing must
be reviewed before any candidate can be promoted.

Outputs: <prefix>.wgsl, <prefix>.bin, <prefix>.json. The original optical frame is
byte-identical as the prefix of the new buffer, so all existing ABI/LUT/density
offsets remain valid. Run compute_probe on the copied shader and frame separately.
