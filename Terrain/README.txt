Mirror regional terrain baseline

This module supplies the deterministic, continuous surface shared by presentation
and radial terrain collision queries. It contains no voxel or chunk storage.
height([0, 1, 0]) is exactly zero metres above the 6,371,000 metre reference sphere.
Excavation changes must be applied after this common base height on both paths.

The authored local region uses two curving resistant ridge bands per fold, lower
intervening strata, rolling valley floors, a transverse drainage gap, weathering
variation and metre-scale surface relief. Sandstone exposure follows the resistant
crests. This is an artistic approximation inspired by central Pennsylvania's Ridge
and Valley province, not a mapped reconstruction, erosion solver, soil constitutive
law or geological validation. Chord coordinates extend the function continuously
over the planet but do not give the whole planet a realistic geological history.

The sampler exposes height(normal), sample(normal), surface_normal(normal), and
RADIUS. Sample contains color, material, soil_depth and moisture descriptors. These
descriptors have not been calibrated as mechanical properties. Existing excavation
save stamps remain offsets on the new base generation; old flat-surface coordinates
therefore acquire the new regional relief. No saved edit is erased by this module.

Material-world presentation (October 29, 2026)

The landform/cave sampler supplies occupied shape. Physics/matter/world_field.rs
assigns turf, dirt, fine sand, stone and bedrock to that volume. Water retains its
fluid volume and native optical renderer. Ground atlas selection, cave sprite
materials and fallback grass/tree/clod appearance are retired. Woodland.rs now
supplies tree habitat/density and the retained tree-bark atlas only. The existing
VegetationSystem tree geometry remains selected, with physical trunk refinement.

EnvironmentGeometry renders the material volume with colored surface geometry.
Rooted turf uses the workshop crown/blade builder at three distance resolutions;
it is not the old four-blade placeholder. The same root-mat material is explicit
when disturbed. Interaction samples occupied material and transfers domain
ownership to the material solvers; old spawned excavation debris is not appended
to the game. See the main README for retained physical limitations and status.
The development-grid and atlas-ground shader paths are removed.

References supporting the artistic landform direction:
Pennsylvania DCNR, Geology of Pennsylvania:
https://www.pa.gov/agencies/dcnr/conservation/geology/geology-of-pa
Penn State, Terry Engelder, An Introduction to the Geology of the Nittany Valley:
https://personal.ems.psu.edu/~jte2/references/link108.pdf

No old Cardinal terrain or Minecraft source was read or used for this module.
The formula and vegetation meshes were authored for Mirror's new direction.

Terrain presentation reuses derived world geometry until the camera crosses a
two-metre snapped chart cell, changes altitude class or changes excavation edits.
Looking and body sway within that cell reproject the existing geometry; they do
not resample the regional landscape or rescan vegetation sites. Simulated debris
continues to update independently each frame. Conservative homogeneous frustum
rejection removes wholly invisible terrain triangles before JSON batch conversion.
Intersecting triangles still use native GPU clipping. iOS retains horizontal GPU
clipping because its drawable aspect correction occurs at the native boundary.

mesh_anchor(camera_eye) exposes the same fixed centre used by this cache.
rendered_height(camera_eye, query_normal, sample_height) intersects its exact
near-field triangles and returns their radial elevation. The callback must include
existing excavation offsets. This supports feet against the displayed surface,
including its half-metre sampling, instead of assuming the analytic surface and
its linear display triangles are identical. The helper returns None outside the
eight-metre dense region or above its near-field altitude limit; it does not
replace the continuous height sampler for world generation or remote collision.

Continuous cave source extension (October 26, 2026)

Underground.rs augments the radial baseline with connected continuous voids.
The configuration is data owned by CaveSystem, copied into accepted scene
snapshots. Rendering, radial support/body contact, acoustic material occupancy
and local water collision use this same definition. It is a first bounded,
coarse local surface extraction, not a completed worldwide cave generator.
Cardinal Depths configuration and spawn/material metadata were consulted under
the author's explicit request; old block terrain algorithms were not imported.
Generated voids describe initial geology and do not count as physical excavation.

Brown upper caves and blue Depths (November 7, 2026)

At the author's request, the pre-binary expansive Cardinal cave profile in
WORKSPACE_APR_17/Salamander92 was inspected. The upper cave selection now uses
its broad 64/128-metre Perlin proportions and depth-dependent thresholds over a
1024-metre local radius, with a three-metre surface roof. It overlaps the existing
Depths at -24..-20 m; authored passages preserve the entrance-to-chamber route.
The older bounded value-noise prototype remains selectable as value_noise.
This is continuous geological void generation, not block storage or excavation.

The active Cardinal atlas is atlas_v15_original.png, explicitly selected by its
circuit breaker. Its StoneExternal tile 34 is brown; its Depths wall tile 461 is
blue. The similarly named replacement atlas_v15.png is grey and is not the color
reference. Art/Terrain/Rock contains original Blender substances inspired by the
authored palette, with metric albedo/normal/roughness/height/cavity maps.
Native caves use the new rock maps, not the retired ground atlas. Mechanical
stone classification remains unchanged; Terrain/Rock.rs supplies appearance by
radial elevation. Grey is retained as an unselected future Nightworld palette.

Surface-net windows below -18 m expand from 36 to 72 metres across so the main
28-metre-radius chamber's walls fit. River air scores are capped at the existing
shoreline margin to condition surface extraction near banks without changing
which points are air. The field and mesh remain bounded approximations.
Deep underground views omit exterior terrain, theatre and surface vegetation;
the cave fades to darkness from 24 to 34 metres instead of revealing the outdoor
sky beyond the finite mesh. This is finite visibility, not a complete cave LOD
system. Near-surface cave windows keep their previous dimensions and daylight.
Native normal relief does not displace the collision surface or rock silhouette.
