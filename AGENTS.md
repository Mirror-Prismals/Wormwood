# Wormwood development boundaries

Read DESIGN.txt and README.md. Preserve the author's Salamander vocabulary.
Keep this workspace independent of the preserved Salamander137 directory.
Do not read/copy old terrain or Minecraft implementations for new engine work.
Do not add voxels, terrain, chunks, or DAW features to the initial diagnostics task.
Use BaseSystem/LayerOne through LayerFive for Systems and matching Systems JSON.
Keep registry.json restricted to System-name booleans. Keep circuit breaker
controls separate; never merge its contents into the System registry.
Catalog named public hooks and validate contracts. Named functions are not Audicles.
Only layer five originates application Audicles. Distinguish invocation tracing
from Audicle lifecycle. Do not claim library inventory means a graphics device,
triangle, JACK connection, VM, or audio output has been tested.
Keep work bounded and report unfinished milestones honestly. No Bevy requirement.

Wormwood is a new engine using Salamander, not a Rust port of Cardinal. Do not
inherit Cardinal concepts, resource formats, or responsibilities merely from
familiar file/folder names. Worlds/Levels and Entity world_id are excluded from
this foundation. Consult DESIGN.txt for the intended engine design; its unresolved details
are not established framework rules. The fork is complete. Mirror and Martin are separate programs; do not edit or synchronize Martin while working here. The author has authorized separating two game repositories at the shared cube/flight milestone. Keep their subsequent title-specific work independent; do not silently sync features between them.

DESIGN.txt is the plain text design document. Keep it about Wormwood itself,
without examples, comparisons, tables, development history, or implementation-status reports.
Keep current implementation status in README.md. Preserve unresolved design details
as unresolved rather than inventing decisions. Layer two is for non-Audicle operations
in Systems, not a mandatory input-reporting path for layer-five controllers.
Named functions exist across every layer and are not themselves Audicle instances.
Do not force calls through all numbered layers. The triangle milestone covers
explicitly selected OpenGL, Vulkan, and WebGPU paths, separately verified; do not
substitute backend inventory or probes for rendered output.

Procedures/systems.json determines startup order only. Each enabled System's
init_steps runs once in that order. entry_steps are separately callable typed
Host entry points. Do not reinstate a global update/cleanup phase scheduler or
reapply the startup list each frame. main remains a small process entry outside
System scope; application flow and ongoing work belong to Systems. Current
validation focus is macOS; defer Windows/iPhone device runs until requested.

The user has explicitly authorized the standalone OGC module under the original
Structures directory for physics work. Do not import its Cardinal System wrapper
or unrelated terrain code. Workshop cases follow the original OGC authors. Keep
our solver, mesh, scale, validation and timing differences explicit.

Intended behavior lives in each System contract's optional intent object. Read
`cargo run --offline --locked -- --inspect-intent` before changing documented
behavior. Preserve behavior IDs when moving source; update source bindings and
verification obligations with the implementation. Explain any change to an
approved expectation separately; do not silently rewrite intent to excuse code.
Record meaningful implementation alternatives and explicit retirement. Runtime
disablement is never retirement; undocumented code is never automatically dead.
The inspector's declared-call checks are structural evidence only. Behavioral
checks remain unverified until actual review/check evidence is obtained. Do not
claim the inspector proves source behavior or authorizes automatic deletion.
Keep inspection and reporting out of real-time audio callbacks.

# Author-approved DAW artwork policy — November 8, 2026

The DAW must use independently composited, persistent baked artwork. Expensive
lighting is an OFFLINE asset-build operation, not application interaction work.
The author explicitly accepts fixed reflections/refraction and illumination.
Do not restore optical freshness, per-frame tracing, content-triggered lighting
captures, settle refreshes, meter refraction, or live shadow/transport resolves
in the production DAW. Moving/resizing panels, pressing buttons, moving faders,
editing clips, zooming, changing labels, and meter/playhead updates only change
transforms, clipping, image selection or inexpensive unlit display content.
Keep comments and executable no-live-optics checks at the renderer boundary.
Ship Baked/manifest.json and its images with the app; never invoke the offline
baker from startup or use live optical calculations as a missing-asset fallback.
The old optics implementation is retained as an explicit research alternative
(WORMWOOD_WORKSTATION_REFERENCE_OPTICS=1), not the production default. Its pixel
identity and optical freshness expectations do not apply to baked production
artwork. A request to improve matching screenshots does not revoke this policy.

For the in-game baked DAW, retain the completed real paused-game background
between observer/world changes. UI edits must not force full world water,
reflection, vegetation and shadow rendering. Water animation holds with this
paused visual image; audio remains active. Camera/world changes, resume and
resize refresh the image. Validate through the real theatre-computer embedded
transition, not a DAW-only substitute. The explicit WORLD_CACHE=0 continuous
world research alternative does not define production UI performance policy.
