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
are not established framework rules. Keep the
foundation common to both intended games. The author has authorized separating two game repositories at the shared cube/flight milestone. Keep their subsequent title-specific work independent; do not silently sync features between them.

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
