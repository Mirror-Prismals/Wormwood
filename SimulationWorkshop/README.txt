Mirror Physics Workshop

The current mobile Material workshop has ten automatic scenes: dirt/spade,
established rooted turf, fine sand/bucket, pebbles/bucket, wood splitting along
grain, wood chopping across grain, stone/pickaxe, loose stone/shovel,
broad-bucket stone contact, and coupled turf/soil/bedrock excavation.
The prior seed/germination scene is retained on desktop as matter_seedlings.
See MATTER.txt for their implementation, research references and current limits.
Mobile has a graphics selector: WebGPU (default), Vulkan, or OpenGL.
Desktop example: cargo run --offline --release -- --workshop matter_dirt WebGPU
Compare forces runs the selected single-material scene on fresh specimens with
45, 240 and 800 N actuator limits, 17 simulated seconds each. Results stay in the
caption/report. On desktop select the corresponding force-comparison entry or
use: cargo run --offline --release -- --workshop matter_stone_compare WebGPU
The longer layered section is matter_layers. It retains all matter while
switching between turf cutting, soil scoops, shovel/rock contact and a pick.
The pick stage requires measured exposed rock; unsuccessful excavation stays
visible and is reported. The force selector does not reset this layered scene.
The older soil cases documented below are retained alternatives.

Open MirrorPrismals.app and select Physics workshop. The workshop has its own
window, author-scenario chooser and graphics API selector. It runs only Mirror.
Martin/lacunavi dawii has not been modified or linked into this launcher.

P pauses or resumes, N advances one 1/60-second physics frame while paused,
R resets the scene, Escape closes it. WASD and Space/Shift move the camera;
hold right mouse to look. The caption shows the actual selected API, wall-clock
FPS, simulation speed (100% means real time), CPU physics, mesh, and render/present
milliseconds. Render timing includes submission and presentation wait, not a GPU
timestamp. A halted case keeps the last valid geometry visible and can be reset.
Apple contact queries use independent worker buffers and a deterministic merge.
The application owns the fixed-step clock, with at most four steps of catch-up;
overload is reported as reduced simulation speed, never hidden by skipping physics.
Paused frames do not rebuild the mesh. Suspension clears accumulated clock debt.

Performance measurements (release builds):
  cargo build --offline --locked --release
  target/release/wormwood --simulate matter_dirt --frames 900
  target/release/wormwood --workshop matter_dirt WebGPU --frames 300
  target/release/wormwood --workshop matter_dirt Vulkan --frames 300
  target/release/wormwood --workshop matter_dirt OpenGL --frames 300
Reports are written under output/simulations; set WORMWOOD_OUTPUT_ROOT to retain
separate runs. Headless reports measure physics throughput, not displayed FPS.
Bounded --workshop runs use one fixed physics advance between presented frames
so every API receives the same workload. Their report marks this clock mode.
Interactive/mobile runs instead use wall time and bounded catch-up.
Both modes report wall/simulation time, mean, median, p95 and worst frame cost.
Material reports also separate adaptation, material solve, contact broad/narrow
phases, and implicit skin reconstruction. The most recent 4096 frame samples
are retained for percentiles; all frames contribute to means and worst time.

Command-line use:
  cargo run --offline -- --workshop cloth_sphere OpenGL
  cargo run --offline -- --simulate cloth_sphere --frames 60
  python3 BuildSupport/macOS/verify_physics.py

Automatic Mirror excavation showcase (not an OGC paper case):
  cargo run --offline --release -- --workshop soil_spade_fracture OpenGL

The iOS Soil workshop button selects this case. No player operation of the
spade is needed. The force-driven stroke approaches, presses, levers, lifts,
holds and tips, then leaves the result visible until Reset. The close oblique
camera and visible shaft/grip make the stroke readable. Pause, Step, Reset,
Exit and optional camera movement remain available.

108 pre-existing elastic soil cells are connected by cohesive interfaces.
Tensile/shear loading accumulates damage; broken interfaces no longer hold
their cells together. OGC contact continues for every cell, the tapered blade
and the receiving tray. Detached material is never attached to the spade or
deleted. Connectivity to the anchored bottom layer determines detachment;
lift counts only detached cells whose centroids rise at least 4 cm.
The caption's motion phase is a requested action, not a success assertion.
Mass is retained; the tiny initial gaps are numerical contact clearance.
Cracks follow prepared cell interfaces, not arbitrary cut planes. Strengths
are uncalibrated. Turf, shaft and grip are visual, without separate physics.
Runtime behavior is unverified for this revision. Per user direction, only
iOS compilation was checked; the user will perform the visual playtest.

Retained contact-only baseline:
  cargo run --offline --release -- --workshop soil_spade_contact OpenGL
  cargo run --offline --release -- --simulate soil_spade_contact --frames 180

This isolated case drives a compliant spade into an intact, fixed-base,
tetrahedral soil patch and then holds the target. The motor applies limited
acceleration; OGC constrains contact. The report records the target and
accepted movement, force on the blade, soil displacement and contact count.
It is a contact baseline only: fracture, disconnected soil, compaction, roots
and player controls are not implemented. The material and drive parameters
are uncalibrated. The desktop launcher lists the case after the app is rebuilt.
Build 2492.19.27 exposed this contact-only baseline on iOS and was observed by
the user. The source now selects the separate fracture showcase above instead;
this old baseline and its regression expectation remain retained on desktop.

The catalog follows the original OGC authors' published scenario families:
50 cloth layers/cylinder, knot tightening, eight-turn cloth twisting,
stitched garment/body, robot shirt manipulation, yarn stretching,
five-turn yarn twisting/release, three cloth layers/sphere, cloth/sphere,
and a volumetric mat. Default mesh resolution is reduced. Opposite twist
handles contribute half of the stated relative angle each.

These are procedural fixtures with CPU XPBD material constraints and the
explicitly authorized standalone contact module. They are not the paper's
VBD implementation or full benchmark assets. The garment/body and robot/shirt
cases are labelled proxies. Extended runs currently halt before completing the full cloth and yarn twist
sequences, at about 3.72s and 7.32s respectively. The volumetric mat halts on a
cell inversion at about 1.91s. All ten default fixtures pass a one-second run.
Friction/thread pull-through correctness remains unverified. No hair, fur, granular soil,
fluid, or compaction model is supplied by this workshop.

Original sources:
https://ankachan.github.io/Projects/OGC/
https://graphics.cs.utah.edu/research/projects/ogc/Offset_Geometric_Contact-SIGGRAPH2025.pdf

JSON reports record actual simulated seconds, average/worst wall step time,
contact demand, overflow, retries and failures. Discrete triangle intersection
and tetrahedral inversion checks reject invalid candidates. They do not prove
continuous collision safety. A failed stress run is a failure, even when its
rejection/rollback mechanism works correctly.

The contact wrapper conditions both material target displacement and contact
repulsion with the local Hessian. A regression checks that repeated inward
material targets retain positive clearance against a fixed surface. The mat
uses a conforming 24-by-6-by-1-cell tetrahedral lattice, ten substeps and eight
constraint/contact iterations per substep; other default fixtures use four
substeps and four iterations. Reports expose the actual counts and debug build
profile. This CPU workshop can run much slower than real time.

Twist reports distinguish commanded rotations, measured handle rotations,
handle target error and release state. A completed clock interval by itself
is not proof that an actuator completed its requested physical motion.
