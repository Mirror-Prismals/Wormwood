Native startup focus and deferred browser access — November 13, 2026

The actual GLFW engine process explicitly activates its Cocoa application and
makes its window key/front when shown. This is independent of preview chrome;
ordinary redraws and hidden background work do not request focus. Borderless
fullscreen windows skip the optional AppKit toolbar.

The macOS left panel starts greyed out with an Enable Browser button. A complete
click enables the browser for that application session, then permits USER audio
and plug-in discovery. Startup/prewarm and audio preparation no longer scan
those folders before this click. This author-requested runtime gate replaces
the previous unconditional pre-audio discovery policy; discovery remains on
the control thread, outside audio callbacks. Permission errors remain visible
through the existing browser error reporting. The disabled tint reuses baked
artwork, with no new lighting or optical refresh.

Validation: 41 focused checks pass. Two bounded native fullscreen sessions
showed the paused/enabled browser states and acquired application/key-window
focus by the second event pump without keyboard task switching. The enabled
session listed a temporary WAV fixture; both sessions released all Entities.
The signed app is output/macOS-startup-focus-fix-20261113/Mirror.app, with
reports and images in the sibling verification directory. Existing OS file
permission grants were not reset or modified for these checks.

Streaming WAV audio import — November 13, 2026

The library importer and WAV preview reader now accept streaming WAV headers
whose RIFF or data length is UINT32_MAX, using the actual file end for those
unknown lengths. Explicit lengths still reject truncated files. WAV imports
remain byte-for-byte copies in USER/USER_SAMPLES; duplicate names retain both
files and publication remains atomic and outside the audio callback.

The supplied glass beach and Last Christmas WAVs both use these unknown-length
headers. The opt-in selected_audio_import_fixtures check imported both originals,
verified identical bytes, and found both through the library scanner. Synthetic
regressions cover streaming headers with metadata, preview decoding, explicit
truncation, duplicate names, cancellation, invalid audio and large-file import.
Existing timeline decoded-source memory limits remain unchanged.

Waveform timeline and disabled spectral editor — November 13, 2026

Timeline audio clips use waveforms regardless of a saved spectral-display
preference. The clip-display preference is hidden and the bottom Spectral tab
is disabled for now. Spectral processing and existing document edits remain
retained; this is a temporary UI disablement, not retirement.
The signed release app is output/macOS-waveform-20261113/Mirror.app.
All 39 focused settings, bottom-panel and timeline tests pass. Intent inspection
reports zero active structural mismatches. The packaged WebGPU check with three
audio clips completed four frames with no arrangement error and zero remaining
Entities. The broader bottom-panel Host test fails at its initial clip selection
before reaching the spectral assertion; that integration check remains unverified.
JACK was unavailable, so audio playback was not checked. Build/test logs and the
native capture are in the sibling verification directory.

Shift overview scroll ownership — November 13, 2026

Whole-workstation Shift-wheel navigation now consumes every scroll tick,
including repeated ticks after reaching either overview limit. Previously only
the tick that changed the layout was consumed; later ticks panned the timeline,
moving clips out of view while playback continued. Timeline bar gestures retain
their separate input ownership. No audio or baked-artwork behavior changes.

The repeated-tick regression fails before the fix. All 23 focused navigation,
timeline bar and overview clip-presentation checks pass after it. A finite native
reproduction is available with WORMWOOD_WORKSTATION_OVERVIEW_CHECK=1 and
WORMWOOD_WORKSTATION_CAPTURE_DT=0.5 on --check-arrangement WebGPU; add
WORMWOOD_WORKSTATION_FREE_CANVAS_CHECK=1 for generated audio clips.
The packaged WebGPU check completed four frames with repeated Shift-scroll
input, retained a timeline start of zero, and visibly rendered its three audio
clips in overview. Entity cleanup completed. The signed local app is
output/macOS-clip-zoom-fix-20261113/Mirror.app; evidence is in its sibling
verification directory. This is a bounded rendering check, not a listening test.

Deferred DAW upload lifetime fix — November 12, 2026

Unfocused or minimized WebGPU DAWs continued receiving queue writes while
surface drawing was deferred. These writes retained Metal staging allocations
until a queue submission. Deferred Pump now submits an empty command list and
waits for upload completion without acquiring a drawable, presenting a frame,
acknowledging redraw debt, or evaluating optics.

A bounded old-bundle reproduction reached 1,607 MiB RSS in 5.4 seconds and was
terminated at the safety cutoff. The fixed bundle ran for 45 seconds with late
RSS of 743–772 MiB while normal draws were deferred, and 605–606 MiB under
continuous rendering. The continuous run exited cleanly. The background run
reported a capture-clock discontinuity during cleanup. The final repeat exited
cleanly with late RSS of 752–765 MiB; its browser labels were visually checked. These are short native memory regressions, not long-run proof.

The browser/import/arrangement changes passed 118 targeted tests. Host cases
used RUST_MIN_STACK=16777216. Release signing and all 348 checked packaged assets
verify; source intent inspection reports zero structural mismatches.
Current test bundle: output/macOS-browser-tracks-memory-final-20261112/MirrorPrismals.app.
Evidence: output/macOS-browser-tracks-memory-final-20261112/verification/report.json.

Browser, song import and track placement — November 12, 2026

The left browser now uses one full-width content page. Back opens the All,
Components, Plugins and Audio choices; All combines the catalog. Import Audio
and Add Plugins are fixed at the bottom of this panel. Search follows Cardinal's
case-insensitive substring matching and owns typing until a click elsewhere;
Escape clears the query. Smaller category labels no longer compete with a
second column. All controls retain the production baked-artwork policy.

Audio imports publish to USER/USER_SAMPLES, then reveal the refreshed Audio
page. Validation streams the complete WAV instead of loading it with the drum
sampler's 8 MiB limit. Preview/timeline loads have a separate bounded reader;
the existing 16,777,216 decoded-value document budget and rate requirements
still apply. Drum samples retain their original limit. Conversion, validation
and copying remain outside audio callbacks.

The right I/O footer cycles TRACK TOP, TRACK BOTTOM and TRACK FREE. Free stores
per-track vertical placement for the session; horizontal handle drags still
resize tracks. Drawing and picking share the same row coordinates. Output
selectors use the existing white ruby when OFF. Track gestures tolerate up to
0.25 logical points of panel settling; meaningful layout changes still cancel.
The native offscreen pointer works without holding a mouse button. Resizing
uses actual unbounded pointer deltas rather than synthetic edge velocity.

Five-mode benchmark suite and fresh measurements — November 12, 2026

BuildSupport/Workstation/benchmark_modes.py runs computer, VR, AR, game and
DAW-only cases serially from one signed app. --mode selects a case; default is
three passes each. Every accepted run has at least 30 measured seconds after
120 warmup frames. Fullscreen WebGPU at 1920x1200 on Apple M1 Pro, empty documents,
normal elapsed-time left-panel key open/close/reversal input, no motion-bank or
live-optics override. Game is a stationary spawn observer with the world active;
VR adds ordinary forward/back navigation and a look sweep; AR freezes its observer.
Computer uses the actual perspective-mapped monitor from the spawn viewpoint,
about 4.7 metres away, not close-up computer use. These are Mac game wearable
modes, not headset hardware or physical-input latency measurements.

Accepted complete-loop FPS ranges: computer 87.2–88.3, VR 6.05–6.17, AR 63.6–63.9,
game 17.6–17.8, DAW-only 84.8–86.0. Game fresh-scene rates are 13.3–13.4 FPS.
Loop p99 ranges are respectively 26.7–27.2, 201.0–203.2, 31.8–32.0, 80.0–82.8,
and 14.4–14.5 milliseconds. Moving-only timing and worst intervals are retained
separately in each run. All accepted embedded windows add zero transport and
ChucK VM underruns. Game audio has lifetime counters only and standalone has no
measured audio counter window; neither is reported as zero measured dropouts.

Nine report-gate tests pass and intent inspection has zero structural mismatches;
behavioral obligations outside the native evidence remain unverified. All 15
accepted native runs pass observer identity, actual motion, GPU query coverage
and zero-Entity cleanup checks. Executable and artwork/shader hashes are unchanged.
An initial computer run overlapping an inspector build is excluded and replaced
by a complete clean run. A replacement with unresolved relative output paths
is also excluded; the suite now resolves native output/assets/settings paths.
Raw attempts, reasons and the accepted results are preserved in
output/benchmarks/five-modes-20261112-203402/report.json and summary.md.
The fresh signed app is output/macOS-five-mode-benchmark-20261112/MirrorPrismals.app.

Run after building an app:
  python3 BuildSupport/Workstation/benchmark_modes.py --app PATH/TO/MirrorPrismals.app

Embedded benchmark observer setup — November 12, 2026

The explicit full-screen combined benchmark now enables its required camera
pass-through after resident Resume. Standalone prewarm and physical-computer
priming can leave the resident settings on a plain backdrop; previously the
benchmark then rejected its own observer before collecting any measurements.
The change is process-local to the finite benchmark and does not save settings
or change ordinary application entry. Physical-screen fixtures retain their
separate observer setup.

Release build and intent inspection complete (zero structural mismatches).
Three fullscreen 1920x1200 WebGPU runs use MOTION_INPUT_TEST=motion, normal
elapsed-time left-panel key replay, no motion-bank override, and no physical-screen
override. Moving-frame loop intervals average 63.1–64.0 FPS, with p99 31.7–33.1 ms
and worst intervals 73.9–131.9 ms. One measured run adds two transport underruns;
the others add zero, and all add zero ChucK VM underrun periods. Each run completes
with zero remaining Entities. These are finite empty-document observer runs,
not physical-input latency or loaded-project performance claims. Evidence and
interval-selection method: output/checks/game-modes-20261112-201046/motion-summary.json.

Trackless audio and Channel Bus components — November 12, 2026

WAV audio can be dropped onto empty timeline space without creating an audio
track. Free audio rows have no track handle, fader, recording controls or insert
chain. Clips remain movable and editable; source offsets, gain, fades, warp and
spectral edits retain their existing behavior. Dropping onto a real audio lane
still assigns the clip to that track. Main audio clips can move between a real
track and free rows; alternate comp takes retain their existing lane rules.

For an unassigned audio clip, Waveform exposes a TO button cycling OFF, the
available Matrix buses and Channel Buses. Matrix destinations use their existing
four routing-gem colors. The button is absent for assigned clips and other editor
tabs. New imports default to the available FF matrix, or the first available
matrix when FF is absent; no matrix means OFF. Direct routing targets a Matrix
Bus identity, so another matrix reading the same stem does not duplicate it.

Channel Bus is a DAW Component dropped into the bottom panel. It has stereo
input/output, a mixer fader, mute, meters and a playback effects strip, with no
timeline lane. Incoming track/channel sends sum before its effects and gain.
The I/O destination control can send tracks and channels to another channel,
an audio/MIDI track or a matrix. Empty track output retains existing L/S/FF/R
routing; OFF disconnects. Channel outputs initially select an available matrix.
Feedback routes are rejected. Clips never own plugin inserts; route through a
Channel Bus to process free clips. Channel FX are included in isolated preview;
stem export with live channel plugins rejects rather than silently omitting FX.
Content Undo/Redo includes routing, placement and bus edits. Missing/deleted
destinations are silent until rerouted or restored. Artwork selection remains
baked, and routing preparation stays outside the real-time audio callback.

Validation: 732 workstation tests passed, two were ignored. The pre-existing
shader-literal assertion named in output/channel-bus-validation/scope.txt was
excluded. Registered routing tests passed (6 selected checks), as did both
registered WAV import checks. Intent inspection reports zero structural
mismatches; behavioral obligations outside these checks remain unverified.

Native appearance, device listening and installed-app packaging for this change
have not been verified. Automated validation evidence is recorded under
output/channel-bus-validation.

Piano-roll controls and spectrum selection — November 12, 2026

The piano room uses square persistent gem buttons with bold captions above them,
responsive rows, and the same TimelineBars zoom/pan, movable zoom rail, loop
handles and transport seek behavior as the arrangement. Loop timing remains
transport seconds; the note view projects it through the selected clip clock.
SPECTRUM SOURCE cycles Mix Bus and individual tracks; SPECTRUM TYPE cycles Audio,
MIDI and Both. The piano default is Mix Bus / Audio. Analysis filters an isolated
copy and never changes live mute/solo/routing. Individual tracks use their
post-fader, pre-master stereo signal. The timeline Master Mix still observes Both.
DEFAULT SOUND toggles the selected MIDI track fallback sine independently,
preserving MIDI events, assigned instruments, drum sources and other tracks.
It is revision-checked and undoable. Artwork stays baked; no live optics added.

Validation: 29 focused piano workspace, scene, spectrum, shared-bar and builtin
instrument tests pass. Generated OpenGL/iOS/Vulkan fragment shaders validate.
The signed packaged WebGPU piano diagnostic passes and its capture was visually
reviewed. Intent inspection reports zero structural mismatches; behavioral
obligations outside these checks remain unverified. The updated test app is
output/macOS/MirrorPrismals.app; its predecessor is retained beside it. Evidence:
output/piano-redesign-validation and output/piano-redesign-native/piano-roll.png.

Nightworld startup repair — November 12, 2026

Rebuilt the authored Nightworld asset for the current waterpark radial datum
(14.773903843946755 metres). The stale asset previously stopped both Play Mirror
and DAW Only at TheatreSystem.Configure. The loader validation remains intact.
Nine Nightworld tests pass; the signed packaged WebGPU game and DAW each complete
six native frames. All 362 packaged assets match source. The corrected local app
is output/macOS/MirrorPrismals.app. This bounded startup check is not a sustained
audio or gameplay test. Evidence: output/nightworld-launch-check-20261112.

Southern ocean, polar island and jungle continent — November 12, 2026

The southern coast is 16 km from town. A 40 km sea leads to a rounded
100 by 100 km snowy island, followed by another 100 km sea and the jungle
coast at 256 km. The jungle continent has 4,000 by 4,000 km chart extents,
sandy shores and evergreen broadleaf vegetation. This preserves the requested
distances on the existing radius-6,371-km planet; the snowy island is not the
geometric planetary pole. The separate northern work remains intact.

A bounded procedural futuristic city sits 1 km inland. Cyan towers and roof
gardens surround raised lava channels with clear, framed walkways. Independent
solid panes provide contact; edge seals and reflection streaks leave the lava
visible. Lava is emissive scenery, not simulated fluid or heat damage. North
and south access ramps join the longitudinal walkway to the ground. Geometry
is generated relative to the city before f32 conversion. Jungle near trees
use a new evergreen procedural broadleaf profile; distant crowns reuse the
existing broadleaf atlas at their tropical scale. Snow uses the existing Snow
substance and excludes generated grass. Southern original-surface colors use
biome materials directly, avoiding the old cave color-tile reconstruction for
offshore vertices. Cave geometry and excavation remain separate.

Validation: 12 regional terrain tests passed (one offline export ignored by
default); three regional-city tests passed. Four southern-focused tests pass,
covering geography, tree identities/evergreen foliage, material sampling and
city bounds/solid panes. Explicit globe export passes. Intent inspection has
zero structural mismatches; its pending behavior checks are not proof of
behavior. Native WebGPU captures show the city, lava walkways and snow close-up.
These are finite renderer checks, not a manual playthrough or installed-app
packaging. See output/southern-world for imagery and native capture reports.

The whole-globe and close-up maps are offline cartographic renders from actual
terrain samples. Labels and graticules are overlays. The reverse hemisphere is
explicitly labelled as the current engine's Nightworld chart; no unseen
continents were invented for the illustration. Reproduce with:
cargo test --offline --locked export_southern_globe -- --ignored
python3 BuildSupport/World/render_globe.py
The Python renderer needs Pillow and NumPy.

Silver spider crab — November 12, 2026

A single hostile silver spider crab now discovers connected cave air below
-22 metres. It pursues at up to 4.5 m/s, follows the shared cave field and
contact normal on land and walls, and moves directly through occupied water.
Bounded swept samples reject rock penetration; this is a kinematic body proxy,
not physical collision for every individual leg or pincer. Spawns search a
bounded 8–16 metre ring and up to 32 metres downward for support.

Close contact gives a sudden camera-facing crab grab. Movement, tools and
look are held while 24 fresh W/A/S/D presses build escape progress; holding a
key or all four keys does not repeatedly score. The prompt reports progress.
Escape releases the player, causes five seconds of retreat at up to 6 m/s,
and then leaves the crab passive until approached within 2.8 metres. Pause
freezes the encounter; stop/abort/reset release the owned visual and Audicles.
Noclip and laboratory interactions prevent new grabs. The encounter has no
failure timer or newly invented health/death rules.

Art/Wildlife/SilverSpiderCrab/SilverSpiderCrab.blend is the editable Blender
model with metallic silver materials, eight jointed legs and two pincers.
The 3,772-triangle export contains sixteen scuttle frames. Runtime interpolates
those frames and rotates the model into the contact frame; water shares that
animation. Dedicated silver facet shading makes it visible in the dark cave
without depending on outdoor sky exposure. It does not reproduce the studio
render's metal reflections. The scene instance bound increases from 13 to 14.
The existing LaboratorySystem entry missing from startup order was restored
so the catalog, registry and schedule account for the same Systems.

Validation: executable build and intent inspection pass with zero structural
mismatches. Four crab tests pass, including actual cave spawn/motion, grab,
escape and resource cleanup. Existing deer behavior and the shared instance
bound tests pass. Tests use RUST_MIN_STACK=67108864 for the large Host fixture.
The older combined bird/deer scene test fails before crab presentation because
its bird fixture produces zero birds rather than four; this was not changed.
The Blender studio render was inspected. Native in-game appearance, wall and
water traversal, jump-scare framing and sustained performance remain unverified.

Zen monastery and burning incense — November 12, 2026

A terrain-supported Zen monastery now stands west of the residential lane,
at theatre-local X=130, Z=-470. Its open meditation hall has shoji bays,
cushions, an altar and a veranda. An enclosed garden adds raked gravel,
grouped stones, moss islands, stone lanterns and a gate. A stone path joins
the existing lane; no existing suburban parcel is replaced.

Aim at the altar lighter and press F to pick it up. Hold F on the incense tip
for two seconds to light it. The 14 cm stick then smolders for 30 real minutes,
shortening to an ash deposit with a glowing ember and rising smoke. F replaces
a spent stick; F at the lighter's place returns it. Burn time continues while
away, paused or in the embedded DAW; the DAW's retained world image remains
paused until returning. Closing the game resets this session-local state.

Ignition and burning have an explicit Audicle lifecycle. Smoke uses a bounded
analytic plume with translucent, depth-tested puffs, not a fluid/chemical
simulation. Millimetre-scale props are packed relative to the camera before
conversion to f32. Details and review command: Art/Town/MONASTERY_README.txt.

Validation: eight focused tests cover ignition cancellation, elapsed-time fuel
consumption, ash/smoke completion, registered input/lifecycle cleanup, small
prop precision, parcel separation, entrance contact and complete-venue bounds.
The original two planned-town regressions also pass. The full venue contains
228,930 triangles against its 229,376 limit. Native visual results are recorded
in output/monastery-20261112; automated input checks are not a manual playtest.
Installed-app packaging has not been performed for this change.

Town laboratory and Prismasqueek — November 12, 2026

A new laboratory stands east of Wren Lane, with a separate temporary Prismal
zoo nearby. Both use authored Blender geometry, surveyed foundations and graded
connections to the town roads. The new sites have their own bounded visual
caches; the main town mesh limit and final scene limit remain unchanged.
Opaque walls, floors and furniture retain matching physical/acoustic solids.

Entering the lab starts a roughly 31-second introduction. The placeholder
researcher gives the player Prismasqueek, explains that he will keep him in the
zoo until the player has an enclosure, and sends him through a red-yarn portal.
Controls are held for the scene and pause freezes its progression. The gift is
saved atomically at the zoo-transfer checkpoint in output/Saves/laboratory.json
(or the selected writable output root), so returning does not grant duplicates.
The personal-enclosure building/claiming system is not implemented by this scene.

Prismasqueek uses the authored /Users/mira/Prismasqueek/Prismasqueek.blend crystal
mouse, copied into Art/Prismasqueek, not Cardinal's image-to-mesh GLB. The 7,216
triangle export retains the quartz silhouette, rose paws/tail and obsidian eyes.
Its source is unrigged; runtime animation uses small paw steps and bounded
following inside the zoo. Quartz has an analytic real-time reflection/Fresnel
approximation, not Blender's path-traced transmission. The original material
and editable scene remain intact in the copied Blender source.

The portal is a timber installation of 64 straight, taut red strands tangent to
a clear circular aperture. It shows a clipped view of the destination site and
its actors. Same-world point/direction mapping and traversal rearming are
adapted from Cardinal's portal behavior without importing world/terrain data.
Both directions check body clearance; the destination view does not recurse or
render distant town scenery beyond the destination site.

Validation: six focused geometry/lifecycle/persistence/portal checks and three
existing NPC movement checks pass. Native WebGPU review uses the real exported
meshes and shared projection paths; its six corrected captures were visually reviewed, including readable signage,
front-facing Prismasqueek and both aperture views. They are not an interactive
playthrough. The workspace's pre-existing Nightworld/waterpark datum mismatch
still blocks normal TheatreSystem startup, so full-game entrance, pause/resume
and traversal remain unverified. No installed application was replaced. Evidence is in
output/checks/laboratory-20261112-final/validation.json.

Native review command:
WORMWOOD_CHECK_LABORATORY=1 WORMWOOD_OUTPUT_ROOT=output/checks/laboratory-20261112-final \
  target/debug/wormwood --check-theatre-exterior WebGPU

Blue-white celestial night sky — November 12, 2026

Night now has a dense fictional celestial sphere: several star brightness and
size populations, a broad distant blue-white galactic band, irregular nebula
filaments and dark dust lanes. There is no foreground galaxy or oversized
planet. Direction-space sampling avoids a longitude seam; the sphere rotates
once per existing daily cycle. Twilight and horizon extinction hide it naturally.
Point filtering limits undersampling. The original sky palettes, sun/moon,
cloud equations, weather, scene lighting and audio are unchanged. Celestial
light is composed behind the original clouds, not into their illumination.
The existing cloud-quality setting also controls sky resolution; no preference
is changed. Lower settings soften the starfield along with the rest of the sky.

Source: Procedures/Atmosphere/celestial.glsl, embedded for all four scene shader
variants by BuildSupport/generate_sky_shaders.py. AppearanceSystem retains its
existing day/night behavior ID with this author-requested extension and an
explicit celestial review obligation. No other project is changed.

Validation: normal offline executable build; four atmosphere tests, one cloud
frame test and the production scene shader/camera test pass. GL/GLES compile,
Vulkan SPIR-V generation and WGSL parse/validation pass. Generator is idempotent.
Native WebGPU captures were reviewed at midnight, east, zenith, through existing
clouds and at dawn; noon is byte-identical to the baseline shader without the
celestial contribution. Evidence: output/checks/night-sky-20261112. The old
isolated grass validation harness has unrelated missing-module/packed-size
errors; the normal production shader test passes. Other native backends, iPhone,
installed-app packaging and sustained frame-time/motion tests remain unverified.

Expanded suburban neighborhoods — November 12, 2026

The town plan now has 54 suburban homes (previously 20), six connected named
streets, six house palettes with varied heights, bays and dormers, porch gardens
and paired clipped topiary. Wren and Meadow Play Greens provide swings, slides,
benches and community beds; Linden Common adds pergolas and a topiary garden.
Orchard Square is a separate neighborhood gathering space. Continuous sidewalks
and five pedestrian shortcuts connect the streets through gaps between lots.
The existing farm, football pitch and golf course are retained. Playground and
community facilities are authored scenery, without new gameplay rules.

The editable TownPlan.blend and runtime plan.json are regenerated together.
Preview ground and road ribbons are Blender-only; the engine builds terrain-
supported roads, sidewalks and walking links. Six-metre road tessellation and
omission of hidden thin-window faces keep the complete town at 227,880 triangles
against its 229,376 venue limit. There is limited room for additional geometry.

Validation: both planned_town engine tests pass, covering complete-town budget,
road contact, grades and graph connectivity. validate_suburbs.py passes for
54 homes, 62 nonoverlapping parcels, 94 road/walk edges, unobstructed pedestrian
lot gaps and mesh bounds. The executable builds offline and intent inspection
reports zero structural mismatches. Offline previews are not native visual validation;
installed-app packaging and in-game traversal/performance remain unverified.

Soft DAW keyboard feedback — November 12, 2026

The keyswitch cue now uses a quieter linear black-switch style: low-pass
filtered plastic contact and a short damped 320 Hz body replace the bright
noise burst and 10 kHz click. Keyboard and arrow-key triggers, feedback routing
and the DAW SFX volume control are preserved. ChucK silent compilation/execution
passed; native listening and installed-app packaging remain unverified.

DAW-only Plain background — November 12, 2026

DAW-only startup selects Plain even when a saved preference selected passthrough.
The baked compositor now draws the existing studio Plain background, using the
same authored ground/ambient color and display transform as the piano room.
Physical computer screens retain their dark backing; camera passthrough and
selected themes retain their existing backgrounds. No artwork is rebaked.

Regional geography and two cities — November 12, 2026

The default surface now places town in treeless plains. In metres relative to
the town (X=125, Z=-125), +X east: forest +1,000 to +21,000, desert to a
locally indented coast near +28,000, and a stained-glass city at +24,500.
A north–south ridge around -8,000 reaches roughly 1.55 km above the reference
sphere; the western city is at -11,000 in the plains beyond it. Block distances
in this request are interpreted as metres. Existing forest species and density
are retained inside the eastern band. Nightworld remains independent.

The cities use bounded procedural exterior architecture, shared ground samples,
building contact/tracing and the existing venue renderer. Colored glass panels
have inexpensive luminous fill; this is not optical glass transmission or
interior architecture. The western city adds white terraces, roof gardens and
sky bridges. City meshes are selected within 6 km rather than added to every
town frame. The original theatre furniture remains local to town. Desert and
coastal material is sand, and the eastern coast has explicit ocean connectivity
outside the old drainage-map bounds. Ocean rendering is enabled with 2.2 m
wave amplitude and a 2x shallow-water gain.

Validation: 7 regional/city tests, 30 vegetation tests (one ignored), 10 terrain
rendering tests, 3 water-mesh tests, the water-cache test and the Nightworld-water test passed. Intent
inspection reported zero structural mismatches. Native WebGPU captures were
reviewed for the plains, forest edge, western ridge, both cities and coast.
The native review caught and fixed remote material-camera bounds and overlapping
glass/wall faces. A bounded ocean horizon mesh now extends the coastal view
beyond the local physical-water patch without changing collision occupancy.
Evidence: output/regional-geography-20261112. Cold terrain material generation
was slow in the debug review; no sustained frame-rate claim is made. Native
walking/listening checks and installed-app packaging remain unperformed.

Cardinal 2D theme alternative — November 12, 2026

The 3D gem ruby remains the main/default DAW theme. The desktop WebGPU
compositor now accepts Cardinal's saved 2D theme JSON as an explicit alternative,
selected through Settings > Workspace > Theme, --theme or WORMWOOD_THEME.
Apply persists the choice; Cancel preserves the current one. The chooser retains
Wormwood's document, controls and audio. Cardinal's 18 preset files are copied
unchanged without importing its previously selected theme.

The adapter supports original bevel colors, jelly surfaces, offline images of
the original crystal shader, color maps, anchor/warp gradients, grain, emission,
MIDI colors and three meter palettes. Crystal images are generated only by the
explicit offline bake_cardinal_themes.py tool; stale/missing artwork is an error.
The 2D path skips the 3D gem atlas and bubble animation pixels/redraws. No hardware
speedup is claimed without measurement. Custom GLB styles remain separate.

See Workstation/CardinalThemes.txt for commands and compatibility limits.
This is a presentation adapter, not a completed port of Cardinal's entire
quick-paint editor, font/outline system, cursor effects or animated backdrops.
Specular/gloss settings remain preserved data.

Validation: offline build and focused importer, atlas, shader/no-live-optics and
Settings checks pass. Standalone flat, Cardinal crystal and main 3D ruby native
previews render. The temporary preview fixture omits TheatreSystem.Configure:
the current workspace fails that startup step because Nightworld and waterpark
datums differ. Full embedded-world testing, listening (JACK unavailable),
performance measurements and installed-app packaging remain unverified.
Evidence: output/cardinal-theme-port-20261112.

Matrix physical-output patch bay — November 12, 2026

Right-panel page one now keeps only input and mute for Matrix Buses. Its
ARRAY/OFF and CH controls are retired. Settings has an Audio Routing tab with
matrix-output ports, physical device/channel ports and cables. Drag a source
port to a destination, or click each port; X disconnects. The matrix list
scrolls when more than six buses exist. The real catalog currently exposes
Flown Array L/S/FF/R; no OS device enumeration is implied.

Routes change immediately through the existing revision-checked SetBus and
content Undo path. Close Settings to use content Undo. Workspace Apply/Cancel
continues to govern only preferences. Existing default routes, matrix input,
mute, monitor Mix Buses, stem export and audio processing are unchanged.
The patch bay uses unlit display geometry with no artwork baking or live optics.
Native appearance, device/listening checks and app packaging remain unverified.
Validation: 10 Settings/routing tests, 7 bus signal/Undo tests and the registered
Host bus-controls test passed in the compiled test executable. Intent inspection
reported zero structural mismatches. The static settings layout was rendered
and reviewed; this is not native application or hardware listening evidence.

Full-screen drum workspace — November 12, 2026

Tab now cycles Timeline, Piano roll, Drum rack, then Timeline. The existing
DRUM RACK control and explicit open entry also select the rack. Escape or
TIMELINE returns to the arrangement. The piano clip and its viewport survive
switching. Hidden timeline and panel controls do not receive rack input.

The draggable gem popup is replaced by a full-screen transparent field using
the piano roll's existing baked bubble artwork for enabled steps. Empty steps
are invisible but remain clickable; enabling a step creates a small bubble
(the former inactive size). Preview highlights enabled steps only. Only
the rack controls retain gem artwork. There is no rack background panel or
runtime lighting refresh. The field resizes with the viewport and retains the
current backdrop/passthrough setting. Audio, samples, tempo and preview remain
independent of workspace navigation. To place a row pattern, drag ::, press
Tab while holding it, then release on its linked MIDI lane. Releasing inside
the rack does not edit the hidden arrangement.

Validation: 27 focused drum-rack tests pass, including registered Host Tab
cycling, preserved document state, shared picking geometry and pattern-drop
ownership. The baked shader validates and its no-live-optics check passes.
The structural intent check passes. Evidence: output/drum-workspace-20261112.
Native visual/listening review and installed-app packaging remain unperformed.

Master mix preview and piano transcription — November 12, 2026

Properties lists Master Mix as a read-only display track. It starts unchecked
in Global and in new viewports. Enabling it shows the complete monitor mix on
the existing stereo tape waveform. It is a synthetic display row: selecting,
dragging, splitting, deleting, recording, comp-lane and plugin drops do not
edit it or create a document track. Multiple monitor Mix Buses are summed, as
in playback; Matrix Buses retain their hardware-only routing.

The piano room automatically shows this mix's spectrum behind the notes.
Horizontal placement follows the open clip's start and local quarter-note
clock; vertical placement follows the physical keyboard and its EDO tuning.
Pan and zoom resample cached analysis. Harmonics remain visible; this is a
transcription reference, not automatic note detection or MIDI generation.

MixPreview owns an isolated playback snapshot with separate playback plugins.
It renders sequential blocks with the existing clip/gain/fade/warp/automation,
instrument and Mix Bus paths, never through live plugin instances. Each UI
poll processes at most 32 blocks, checking a four-millisecond preparation
budget between blocks. An 8192-point Hann-window stereo-power FFT supplies
half-semitone frequency samples; spectral history is bounded to about 1024
time slices, while all rendered samples contribute to 128 stereo peak columns.
The display grid is cached separately. Rendering pauses while both consumers
are hidden; a valid completed cache is retained for reopening. No file writes, FFT, locks or allocation
are added to audio callbacks; no new lighting or baked artwork is needed.

Document revisions and plugin-status changes invalidate the job. Stale results
are cleared while the replacement builds. Completed generations cross the
Host boundary once and are reused thereafter. Unsupported isolated plugin
state or source sample-rate mismatch is reported as unavailable rather than
showing a misleading dry mix. Existing offline-plugin limits (including VST2)
apply. Live input is excluded. Two seconds of plugin tail or 120 ms of builtin
release are included; file/project persistence of this disposable cache is not
needed.

Validation: seven focused mix-preview tests passed, including registered Host
routing, hidden work suppression, cache reuse, stereo mix mute/Undo, pitch/time
projection and isolated warp preparation. Surrounding checks passed: 10 viewport,
52 arrangement, three piano workspace, three piano geometry, six bus and six
freeze tests, plus dynamic glyph-address and bend-row storage checks. The baked
shader validates with Naga and cannot reach live optics; generated Vulkan
shaders compile. Intent inspection reports zero active structural mismatches.
Native packaged-app verification: the WebGPU piano-room diagnostic completed
184 frames (180 measured), showed the completed spectrum behind the note
bubbles, reported no editor errors, and released its Entities. The four-note
fixture's last 20 cached preview calls had a 0.007 ms median; this is not a
large-project or third-party-plugin performance claim. JACK was not running,
so device listening remains unverified. Signed staged app:
output/macOS-mix-preview-20261112/MirrorPrismals.app. Validation logs, source
hashes and the completed native capture are in
output/mix-preview-validation-20261112.


Waterpark placeholder NPCs — November 12, 2026

NpcSystem introduces three named town visitors using small, differently colored
self-lit spheres. Nori swims laps in the shallow pool, Pip wades back and forth
with rests, and Jun loops around the lazy river. Routes use the installed park's
pool bounds; movement is continuous and repeats for the game session. These are
scripted aquatic routines, not general town pathfinding or slide riding.

Look toward a visitor within 4.5 metres and press T to talk. Repeated presses
cycle three individual lines; holding T does not repeat. A macOS HUD and window
caption show the name/prompt and reply. Replies expire or close when you leave.
Normal gameplay advances visitors; pause and the DAW's deferred observer view
hold their routines. Stop/abort release the owned Entities and Audicles.

The unlit_surface appearance uses a fixed vertex-color gradient to suggest
roundness. It skips per-vertex sky exposure and scene light tint/brightness and
adds no lights, shadows, ray tracing, assets or audio work. The scene budget now
allows the prior eight instances plus three visitors. Native visual/play review
and app packaging remain unverified.

Validation: six focused tests pass (continuous loops and dwell, 3,000 seconds
of bounded independent routines, zero-time hold, scene capacity, intent bindings,
and registered NPC dialogue/Entity/Audicle lifecycle). The lifecycle fixture
loads the real NPC, Entity, Audicle and Theatre contracts with authored assets
and a minimal navigation caller. The macOS HUD passes clang syntax checking.
Full workspace startup was blocked during validation by a separate unfinished
WorkstationDataSystem.MixPreview declaration with no compiled hook. No unrelated
workstation code was changed to bypass that registration.

Timeline viewports — November 12, 2026

The left panel now has Browser and Properties pages. Properties shows the
complete document track list above a separate Viewports list. Checkboxes select
the tracks visible on the timeline for the selected viewport. Global is the
initial selection and cannot be deleted; it starts with every track checked.
The + control creates and selects an empty numbered viewport, with every track
listed but unchecked. Choices remain independent when switching views. New
tracks are checked in Global and unchecked in custom views. Expanded comp lanes
follow their parent track. Both property lists scroll independently.

Visibility changes are session-local UI state. They do not mute tracks, change
routing, alter the audio document or enter content Undo. Track insertion and
reordering preserve the full document order, including hidden tracks. New labels
and selection faces use the native unlit overlay over existing baked panel art;
there is no new lighting or offline asset build. Project-file save/load is not
added. Native appearance and listening validation remain separate checks.

Validation: the viewport filter passes 9 tests, the arrangement suite passes
52 tests, and the browser filter passes 11 tests. Host checks use
RUST_MIN_STACK=16777216. These cover control/data behavior and packet validity;
native visual and listening review remain unperformed.

Signed macOS build: output/macOS-viewports-20261112/MirrorPrismals.app.
Intent inspection reports zero structural mismatches. Logs and results are in
output/viewports-validation-20261112.

Workstation interaction review — November 11, 2026

The physical theatre computer defaults to a plain opaque screen backing. Its
surrounding real theatre observer remains visible, but is no longer transmitted
through the screen and its transparent artwork. In-canvas camera control hints
are removed. VR observer movement preserves the paused world clock, water and
wildlife, skips settled idle navigation, and yields pointer ownership to controls.
Moving VR still needs fresh full-scene rendering; frozen AR has a lower cost.

Shift alone no longer changes the view or triggers a navigation sound.
Shift-scroll down enters persistent 0.61 overview; Shift-scroll up returns.
An edge arrow shrinks an expanded/full-view panel before closing it. Drum-rack
and bottom-panel input consume queued press/release events. Mixer track headers,
housings and fader caps select ordinary tracks; Delete removes a selected track
and its clips/comp lanes/drum row in one undoable revision. Source samples and
inactive plugin chains remain available for Undo. Recording/frozen-content guards
remain. Right and bottom control labels use brighter unlit lettering above their
buttons, with additional space on stacked rows. Theatre subs now meet the floor,
and their fill cabinets move down with them.

Verification: 692 workstation tests pass with two ignored and one unrelated old
128-control shader-string assertion excluded. Five focused navigation, VR,
shader and registered fader-route checks pass. Native packaged WebGPU checks
complete the real computer transition (12 frames), VR walking/camera switching
(13 frames), and normal-track selection/gain replay (11 distinct values). Source
Systems/Procedures match the signed bundle. The legacy six-specimen native meter
assertion still fails; current track faders are verified through their real replay.
Manual drum loading/listening, subjective readability and moving-VR responsiveness
remain user checks. Results and logs: output/workstation-review-20261111.
Current signed test bundle: output/macOS/MirrorPrismals.app.

Town expansion and connected road plan — November 11, 2026

The northern civic quarter adds a walk-in courthouse, pawn shop, thrift store
and eight-shop mall arcade. The commissary now displays 21 Blender-authored
product types, including new produce, pantry and household stock. Packed Blender
surface tiles are sampled offline into the existing vertex-colour assets; these
are baked surface colours, not a new runtime UV-image shader. The original houses
have the same texture pipeline. Twenty more suburban homes form two crescents
around a green, with porches, garages, paths and gardens. A 105 x 68 metre soccer
pitch, compact nine-hole golf course, planted vegetable/orchard farm and farmhouse
join the explicit connected road graph. Sports/scoring, commerce and cultivation
gameplay are not introduced by these scenery additions.

Theatre/Plan.rs surveys real terrain and finished site levels, solves common
junction elevations and builds smooth curved asphalt roads, entrance spurs and
terrain-reaching shoulders. The original northern esplanade is surfaced as a
main street with pavement edges and markings. Both the historical park approach
and later town approaches now have earth shoulders to the actual ground. Site
foundations stay below the authored turf/floor to avoid coincident-face flicker;
landscaped skirts point outward. Collision coverage includes the full new district.
The road diagram is Art/Town/road-plan.svg; metric controls and editable Blender
sources are under BuildSupport/Town and Art/Town.

The marsh boardwalk is a larger connected trail network with crossing routes,
branches and four roofed bird hides. Its rail gaps preserve physical junctions.
The waterpark adds a circulating four-reach lazy river and the Sunspin inner-tube
funnel. The riding tube uses a Blender mesh, wider contact and damped pool buoyancy.
The funnel has a continuous enclosed discharge with a transported section frame,
three-metre-radius elbow and an open catch-pool inlet. Its free gravity/contact
check reaches the receiving water; the two original deep descents still pass.

Static opaque faces are spatially ordered for the existing hierarchy. The renderer
keeps the union of primary and actual water-reflection frusta and rejects enclosed
auditorium content only when opaque walls/ceiling block it from every relevant
camera. Interior and potential front-portal views retain full detail. Unknown
reflection metadata retains the conservative fallback. No dynamic LOD, water-ray,
material, audio or global scene-buffer budget reduction was used. The authored
venue share is now 229,376 of the unchanged 262,144 scene slots; actual composed
frames are still validated. The raw assembled venue contains 214,412 triangles.

Two final packaged native WebGPU 1,200-frame matched-route runs measured
46.55 and 47.22 loop FPS, versus 46.26 before the asset expansion. Median frame
time changed from 19.00 ms to 18.32 / 17.97 ms. Fresh-scene delivery was
38.94 / 38.56 per second versus 38.35. These are diagnostic throughput figures,
not physical scanout FPS or proof of long-session stability. The separate daylight
town route also completed. All finished with zero remaining Entities. Sixteen
focused checks pass and nine established native captures match byte for byte.
Seven packaged town/park assets match source hashes. Intent inspection reports
zero structural mismatches; unperformed behavior obligations remain pending.
Native manual input/riding and sustained audio listening remain separate checks.
Evidence, hashes and complete benchmark limits: output/town-expansion/RESULTS.txt
and comparison.json. The ad-hoc signed app is
output/macOS-town-expansion-20261111/MirrorPrismals.app.

Plugin target gem row — November 11, 2026

Bottom Plugins page replaces its previous/next track arrows with a second row
of matching numbered mini gems. Click selects an audio track, MIDI track or Mix
Bus directly; selected gems use the same pressed white ruby artwork as the page
row. Wheel over the target row reveals further groups of three. All 49 bottom-panel
checks and the baked shader/no-live-optics check pass. The normal executable
builds and intent inspection has zero structural mismatches. Native appearance
and installed-app packaging remain unverified.

Wind doorway placement and speaker inspection — November 11, 2026

Outdoor wind placement now samples 96 upper-hemisphere direct escape directions
at most five times per second on the acoustic worker, using the receiver's local
up. A candidate must also have unobstructed sky above its far endpoint, avoiding
false openings in long roofed rooms. Opening solid angle lowers the source and
its dominant direction moves it toward the opening. A 0.35-second position ease
replaces the old threshold jump/roof-edge hold. With no escape, the target clears
the highest roof. Player and virtual microphone keep separate placement state.
The existing twenty-ray/six-bounce acoustic propagation batch is unchanged.

On Mac, Escape → Cheats: On → Sound speakers: On exposes the published acoustic
emitter positions using a new Blender-authored spherical omni speaker with
aluminium grille rings and an amber indicator. This control is independent of
ray ribbons and glasses. Disabling cheats, reset and new-session configuration
clear the selection. The runtime mesh is compiled into the executable; Blender
source and its exporter are in Art/Audio/OmniSpeaker and BuildSupport/Blender.

Five wind-placement checks, four acoustic regression checks, two worker checks,
one cheat-gating check and one mesh check pass. Intent inspection reports zero
structural mismatches. The Blender model preview has been rendered and reviewed.
The signed release bundle is output/macOS-wind-doorways-20261111/MirrorPrismals.app.
Its 35 executable sections match the release build and all eight baked artwork
files match source. An eight-frame packaged WebGPU game run exits cleanly with
zero remaining Entities and no ChucK render errors. That startup run reports
28 transport underruns (zero ChucK VM underrun periods); this is not evidence
of glitch-free sustained audio. Native doorway listening and in-game model
inspection remain manual checks.
Validation and package results are recorded in output/wind-*.log and
output/speaker-*.log.

Waveform sample detail — November 11, 2026

Timeline Command-wheel zoom now reaches a 10-microsecond visible interval.
At 100 milliseconds and closer, visible audio clips request signed stereo
min/max values from the original PCM for their visible source interval. The
existing 128-column GPU packet displays this interval rather than stretching
full-clip absolute peaks. Wider views retain the rainbow overview. Clip offsets,
comp takes and drag previews use the same source mapping. Detail is cached until
the source document, visible clips or source interval changes.

The bottom Waveform editor regenerates its preview for the visible source
interval and viewport width, up to 2048 columns. Command-wheel zooms about the
pointer down to one source frame; ordinary wheel pans. The overview bar retains
range zoom and click-to-fit. Selection, trim, split and loop coordinates remain
relative to the full clip. The former 0.5-percent display floor and cached
240-column magnification are replaced. Preview ranges preserve sample polarity
and scan all contributing frames; neither zoom path edits PCM or document
revision. This is unlit display work over persistent baked artwork, never a
lighting refresh or audio-callback operation.

Validation: nine waveform-focused tests (including a 440 Hz sine, individual
samples, offset PCM, stereo polarity, timeline GPU packet mapping, zoom/pan and
unchanged revisions), 48 bottom-panel tests, 49 arrangement tests and 22 timeline
tests pass. The production baked shader validates and its no-live-optics check
passes. Normal executable builds; intent inspection has zero structural
mismatches. Native interaction/appearance review and installed-app packaging
remain unverified.

Timeline selection and handle resizing — November 11, 2026

Selected timeline track handles and clips now use Cardinal's blue display
palette. Selection travels in the existing instance packet and tints persistent
baked artwork while preserving facet brightness, tape detail, clipping and depth.
Normal ruby and ivory comp handles return when deselected. No live lighting,
asset captures, startup baking or audio-callback work is introduced.

Horizontal handle resizing continues outward at viewport edges at 420 logical
points per second, matching Cardinal's gesture. Reversing at a size limit responds
immediately; release retains the height, and existing cancellation restores it.
Vertical dragging retains track reorder ownership. Track selection alone never
resizes a lane.

Validation: 49 arrangement checks, 8 timeline scene checks and the composed
WGSL validation/no-live-optics call-graph check pass. Intent inspection reports
zero structural mismatches. The release app is staged and signature-verified at
output/macOS-selection-feedback/MirrorPrismals.app. Native appearance and drag
feel remain for manual review. Two invalid focused fields in the existing
TimelineBarIntegrationTests input literals were removed to compile the test suite;
these input values have no focus field or replacement behavior.

Routing gem channel colors — November 11, 2026

Matrix input selectors and normal track left/right output selectors now use
blue crystal for L, cyan for FF and orange for S/Sub. R and OFF retain the
original ruby; matrix hardware channel selectors retain their existing artwork.
BuildSupport/Workstation/bake_channel_gems.py generates persistent hue variants
from the mini ruby, retaining source saturation, value, alpha and auxiliary
depth. The main offline baker regenerates these variants after its normal bake.
Interaction selects shipped images without lighting calculations.

The focused routing packet test passes for all channel states and confirms
unrelated controls retain their palette. Offline asset checks confirm alpha,
auxiliary depth and facet brightness preservation. Native visual review and
installed-app packaging remain unverified.

Artwork drag previews — November 11, 2026

At the author’s request, macOS browser drags replace the former word-only ghost
with the component’s baked artwork: vertical track/comp handles, Matrix/Mix Bus
meters, the stock EQ’s gem GUI and existing graph/labels, Edison hardware for
hosted plugins, and tape with a source envelope for WAV samples. Pointer velocity
drives bounded damped pitch/yaw/roll, settling when stopped. Position remains
anchored to the pointer. Physical theatre-computer input projects back to the
native overlay; drop behavior remains revision checked and unchanged.

`BuildSupport/Workstation/export_drag_art.py` converts existing baked radiance
through the original AgX tetrahedral LUT into shipped transparent PNG sprites.
It is an offline build step, never startup or drag work. Native previews compose
once per payload and reuse the image while moving. WAV envelopes stream on a
cancellable worker within the existing 16,777,216-source-value budget. Unsupported
or unreadable samples show tape without a fabricated waveform. No optics, scene
recapture, document edits or audio-callback work are introduced. iOS retains its
existing text preview. The former label-only expectation is explicitly replaced
in the retained `cardinal_cursor_ghost` behavior.

Validation: 31 browser, 3 audio-library/envelope, 3 computer-screen projection
and 4 cursor routing/request tests pass (Host tests use a 16 MiB stack). The
isolated AppKit composition, pass-through and damped-settling check passes;
composed preview images are in output/checks/drag-preview. Release builds and
intent inspection reports zero structural mismatches. The separate signed app
is output/macOS-drag-preview/MirrorPrismals.app; its six preview PNGs match the source assets,
and all executable/data sections match the release binary. The usual app bundle is unchanged.
Manual real-window drag feel and full embedded interaction review remain pending.

## Workstation settings — November 11, 2026

The lightbox editor has been retired from SETTINGS at the author's request.
The simple rectangular dialog has Workspace and Controls tabs. Workspace offers
plain/passthrough background and left/right or top/bottom panel placement.
Apply saves preferences; Cancel/Escape discards changes. Reset restores the
workspace defaults in the draft. AR retains its locked real-view background.
The Controls tab documents existing navigation shortcuts and touch gestures.
Version-one settings remain readable, and saved lighting is preserved without
exposing lighting edits or adding runtime optics. Panel placement uses the
existing layout and hit-testing routes after Apply.

All seven focused settings tests pass, including input capture, viewport layout,
cancellation and persistence with retained legacy lighting.
Native visual review and installed-app packaging remain unverified.

Browser drag retention — November 11, 2026

Component, sample and plugin drags now use the logical viewport and canvas
scale for coordinate-space invalidation, rather than animated timeline bounds.
Panel movement and presentation refreshes retain the held payload; drop targets
still use current geometry. Document changes, focus/pointer loss, Escape,
modal ownership and actual viewport/scale changes retain cancellation behavior.
Validation: all 30 browser-filtered tests pass with RUST_MIN_STACK=16777216,
including component/sample drops across changed panel bounds and cancellation
on window resize. Normal executable builds; intent inspection reports zero
structural mismatches. Native interaction review and app packaging remain pending.

Browser component double-click insertion — November 11, 2026

Double-click Audio, MIDI or CC Track in the left browser to insert before the
selected track, or append when none is selected. Double-click Comp Lane to add
a take to the selected audio track; selecting its lane or handle supplies the
same target. Matrix and Mix Bus double-clicks create their existing bottom-panel
components. Drag insertion remains available. Clicks must stay within 3 logical
points and complete within 350 ms; cancellation and document changes break pairs.
The lane gutter now selects its track like the handle, and double-clicking either
opens the channel strip. Audio-clip double-click still opens Waveform.
Validation: all 10 browser and 48 arrangement tests pass. The normal executable
builds and intent inspection reports zero structural mismatches. Native
interaction review and installed-app packaging remain pending.

## Audio editor presentation — November 11, 2026

The Clip Editor hides clip-edit controls when no audio clip is selected.
Its controls reuse the transport mini ruby artwork; active tabs and enabled
Warp select the existing white ruby variant. The Waveform zoom and loop bars
reuse the timeline's baked ruby image, with the selected range above its track.
Waveform and Warp displays have transparent backgrounds. Warp shows the source
envelope, transient lines and marker handles; the selected marker turns white.
Interior marker drag preview and commit use the same inset coordinates as the
visible waveform. Endpoints remain fixed; click the waveform and Add Mark to
insert an interior marker. All artwork remains persistent with no live optics.

Validation: 49 focused bottom-panel/editor tests pass, including marker gestures,
backend warp behavior, empty controls, transparent marker display and white
selected-state/bar packets. The production shader parses/validates and its
no-live-lighting reachability check passes. Running-app visual review and
installed-app packaging have not been performed for this change.

## Right-panel pages — November 11, 2026

Drum Rack, Rays and optional speaker/headset controls now live on page two.
Page one starts I/O near the top. Three numbered mini gems directly select
pages, using the bottom panel's colorless ruby selected artwork. Existing
baked assets are reused with no runtime lighting. Four navigation, sixteen I/O
and six control-geometry checks pass. Native visual review and app-bundle
rebuild remain pending.

## Meter casing height — November 11, 2026

The baked mixer keeps the LED spectrum at its existing height and fits the
casing to it with 2 mm end rims in place of the authored 12 mm rims. Only the
end artwork is compressed; the aperture, level mapping and LED height remain
unchanged for track and output meters. This is persistent image sampling with
no live lighting. Native visual review remains pending.

## Resident DAW before gameplay — November 11, 2026

At the author's request, the macOS baked WebGPU game completes DAW loading
before accepting gameplay input. Prewarm loads the panel kits and plugin catalog,
creates the resident DAW Entity and renders its first image offscreen. PrimeWorld
prepares the actual embedded theatre/world pipelines and completes the native
window's first presentation during startup. This last startup view deliberately
replaces hidden-only world priming so its first-show hitch occurs before spawn.
Prewarm itself remains hidden. WORMWOOD_WORKSTATION_PREWARM=0 retains the explicit
lazy-preparation comparison; other backends retain their existing lifecycle.

Computer and AR/VR entry reuse the same DAW Entity, renderer, panel assets,
document and layout. Departure parks the DAW and restores the retained game
renderer/texture ownership. While gameplay owns input, a bounded 10 Hz offscreen
baked composition updates the physical monitor, including the playhead; it never
shows/polls the DAW window, rebuilds world pipelines, or performs optical lighting.
The existing game audio session continues. Final shutdown releases both scenes.
Parked native windows explicitly restore on reentry. GPU report snapshots retain
the live query stream; benchmark draw offsets exclude startup and earlier entries.

Validation: 17 focused lifetime, recovery, wearable, redraw, completion, GPU-offset
and baked-boundary checks pass (Host tests use a 16 MiB stack). Source intent
inspection reports zero structural mismatches. The final signed release completed
three real embedded entries each for computer, AR and VR: all nine frame/GPU
reports complete, no rejected/failed calls, and zero Entities after each process.
Each process loaded panel assets once, created one DAW Entity and prepared two
native clients total. First entry frames measured roughly 100–215 ms, including
native window presentation; this is not a claim of zero latency. AR/VR Resume
setup is about 2–3 ms; physical screen setup about 24–31 ms. Full initialization
and initial presentation are paid during startup. See
output/resident-daw-release-verification.json and output/resident-daw-release-*.
An earlier AR verification process was externally terminated before embedded
startup without an application error; its retry and the final three-mode run
passed. Manual click/E interaction feel and extended device listening remain
separate from these finite production-transition checks.

The usual output/macOS/MirrorPrismals.app is updated; its previous bundle is
preserved beside it. Restart the app to load this build.

## Independent theatre office chair — November 11, 2026

The front-of-house chair is an independent upholstered office chair with armrests,
a telescoping lift, a five-spoke base and rotating caster wheels. Walk into it to
roll it across the level FOH deck. Look at the chair within three metres and scroll
to raise/lower the seat between 0.65 and 1.45 metres. The chair stops at the deck
edges and solid fixtures; it does not roll down the stairs. Its small changing
mesh has a separate projection cache from the immutable theatre architecture.

Clicking the computer freezes the current observer position and orientation.
E/Escape releases focus at the same location. Computer use never moves the player
to the chair or aims their view at the monitor. Perspective image/input mapping
uses the retained actual view. Wearable and standalone entry retain their own views.
This changes the physical_computer_display expectation at the author's request.

Validation: two chair geometry/height/rolling checks, seven computer focus,
projection and recovery checks, and the authored theatre geometry check pass.
The Host focus regression verifies the actual computer-screen Resume/Suspend
path retains the exact oblique observer pose. Host tests require
RUST_MIN_STACK=16777216. Source intent inspection has zero structural mismatches.
The signed release completed eight measured native WebGPU frames through the real
theatre-computer embedded transition. The usual output/macOS bundle is updated,
with the previous bundle preserved beside it. Native chair appearance, push feel
and wheel gestures remain manual checks.

## Physical computer startup validation repair — November 11, 2026

The shared WorkstationEntry validator now accepts the optional boolean
computer_screen field forwarded by embedded startup. Previously every computer
entry failed at Resume with invalid input after game graphics suspension,
causing a renderer blink and leaving the monitor dark. Unknown fields and
nonboolean screen selections remain rejected.

Validation: the production-request admission regression and both perspective
image/input tests pass. The optimized signed package completed eight frames
through the real theatre-computer embedded WebGPU transition with
WORMWOOD_COMPUTER_SCREEN_VERIFY=1; RunEmbedded finished without error and the
benchmark reports complete. Manual oblique clicking/dragging remains unverified.
The packaged source inspector cannot resolve omitted authoring source files;
source-checkout inspection is the relevant structural check.

## Loop placement and user sample lettering — November 10, 2026

LOOP now lives below LEARN on right-panel page two, with the existing managed
release and transport toggle. Its top-panel face and input target are removed.
The embedded browser lettering now includes user WAV entries, matching the
catalog used by hit testing and drag/drop. Both changes use existing baked
artwork and inexpensive display content. Cargo check and ten focused control,
ruler and browser checks pass; intent inspection reports zero structural
mismatches. Native visual review and installed-app packaging remain unverified.

## Physical computer startup repair — November 10, 2026

The shared embedded startup now forwards the computer_screen selection to
Resume. Previously the theatre click requested it but run_mode discarded it,
so the physical display and inverse mouse mapping were never activated.
The player holds position and the existing visible desktop cursor operates
controls on the projected screen; E/Escape returns to gameplay.

Validation: embedded startup selection regression and both physical-screen
projection/event mapping tests pass; the debug executable builds. Intent
inspection reports zero structural mismatches. Native transition verification
is pending: the debug benchmark encountered a main-thread stack overflow
following unavailable JACK diagnostics; a larger-stack retry reached desktop
service connection failures and was stopped. Installed-app packaging is unchanged.

## Timeline panel keys after piano roll — November 10, 2026

Desktop panel arrows and Shift overview now follow the visible piano workspace,
rather than whether its MIDI editor retains a loaded clip. Returning with Tab
or Escape restores timeline panel navigation while preserving the clip and its
viewport for reopening. Space remains the shared audio transport.

Validation: the retained-clip navigation regression passes; the executable builds
and intent inspection reports zero structural mismatches. Native interactive
review and installed-app packaging remain unverified.

## Learn button placement — November 10, 2026

LEARN is a ruby button on the second right-panel page. It toggles the existing
lesson card and preserves session progress. The separate top-right control is
removed. It reuses persistent baked ruby artwork with unlit lettering.
Validation: cargo check passes; six tutorial checks pass with the existing
16 MiB Host-test stack requirement. Intent inspection reports zero structural
mismatches. Native visual review and installed-app packaging remain unverified.

## Cardinal meter colors — November 10, 2026

Production baked DAW meter fills use Cardinal's direct display palette, including
the 25% inactive fill, without the previous 1.85 brightness multiplier or AgX
display mapping. The baked casing and unlit composition policy remain intact.
Mix bus meters select the third palette by their MX bus identity rather than
their position after the first four meters.
Native visual review remains pending.

## Right-panel lettering — November 10, 2026

Compact/right panel labels use bright ivory ink, stronger engraved coverage and
fractional-pixel stroke expansion. Font size, spacing and authored fracture
texture stay unchanged. This is unlit display composition over persistent baked
artwork. Native screenshot review remains pending.

## Nightworld floor portals, dunes and parkour shaft — November 10, 2026

Nightworld is a reflected radial coordinate chart. Floor portals retain the
entry's X/Z and reverse up through a movement-holding loading card. Ordinary
floor iris sites are spaced eight metres apart; arrivals use a five-metre
bottom gallery with mineable rock overhead. Nightworld's surface is dry sand,
with asymmetric wind-shaped ridges, smaller dunes and fine ripple variation.
Its live day/night clock is offset twelve hours from the overworld.

At the waterpark depths pools, the east/south dry promenade has a narrow secret
stair passage to the floor iris. Its destination is a finite native-water basin
inside a green/magenta spiral metal tube. The 222 separated platforms rise
0.72 metres each and finish above the tube rim. Their real Blender triangles
provide body contact; there is no hidden climbing ramp. Nightworld uses no
Pennsylvania vegetation or overworld underground river water.

The Blender master, GLB, procedural sand material, 2K sand albedo and build
instructions are in Art/Nightworld. Runtime ships Procedures/Assets/Nightworld;
it does not launch Blender. See Art/Nightworld/README.txt and manifest.json.

Validation: 13 focused Nightworld checks and 39 terrain regression checks pass.
Registered transition checks cover the loading hold, exact X/Z and inverted up;
other checks cover contact, geological clearance, full platform route spacing,
rim clearance, water orientation, clock phase, scene storage and cache boundaries.
Native WebGPU/Metal captures use the production scene builder and finite-basin
water pass at the landing, shaft, dunes and loading card. Intent inspection
reports zero structural mismatches. These are finite scene/route checks, not a
complete player-controlled waterpark traversal or parkour ascent. Installed-app
packaging and device listening remain unverified.

## Waterpark physics and deep cavern pools — November 10, 2026

The 50.6-metre launch deck replaces the previous 24.5-metre tower. Both
courses have aerial and underground corkscrews, dropping approximately 138
metres to the Depths pool cavern, roughly 72 metres below the reference
surface and above bedrock. Lime is enclosed throughout; magenta keeps an
open launch and encloses its fast turns. The splash garden adds shallow
water, mushroom cascades, spray arches, parcel jets and stepping stones.
A broad pool promenade connects to 24 timbered switchback mine flights.
Matching geological voids clear the tubes, cavern and complete stair route.
An authored inner blue limestone envelope encloses the large room beyond
the local terrain mesh window. Real slab openings clear both slide portals.

Z toggles sitting, WASD steers and Space jumps. Seated movement preserves
momentum, uses bounded substeps and resolves actual triangle contact.
Automatic centerline capture, minimum speed and velocity clearing are
explicitly retired at the author's request. Paths describe the artwork and
water film only. O near a launch toggles the nearest inlet; conservative
film mass/advected momentum evolve under gravity, friction and pressure,
then drain when closed. Jets use existing conserved parcels with nearby
park-shell collision. Water coupling happens once, before contact, rather
than adding a second unconstrained translation after the surf solve.
The retained workstation baked-artwork/paused-world policy is unchanged.

The editable Blender master, GLB, overview.png and depths.png are in
Art/Waterpark. The runtime export has 49,149 visible and 39,808 collision
triangles, indexed locally for contact. Immutable acoustic snapshots allow
65,536 triangles to preserve the enlarged shell. The complete shared venue
also includes concurrently developed town assets; its current bounded
allocation is three quarters of the renderer's existing scene capacity.
Four zero-area town import faces are excluded from acoustic geometry.

Validation: all seven waterpark checks pass, including both complete
unsteered physical descents, tower/mine stair traversal, gate drainage,
geological clearance, receiving-water occupancy and registered production
configuration. All four player-controller checks pass, including sitting
foot position, held-key behavior, momentum and reset. Intent inspection
reports zero structural mismatches; it does not prove behavior. Evidence is
in output/checks/waterpark-upgrade. Six native WebGPU views (including cavern and splash garden) were captured
and visually reviewed. The optimized, signed Mac app is staged at
output/macOS/waterpark-upgrade-20261110/MirrorPrismals.app. Its actual game
started and closed cleanly after four frames; packaged park/cave/contracts
match the source assets. That cold-start run reported 29 audio underruns
under concurrent build/capture load, so it does not establish steady-state
audio performance. Manual full-game play feel,
steering/jumping through the courses, audio-device listening and iOS remain
unverified.

## Church and greenhouse commissary — November 10, 2026

The town now includes a 304 × 240 m vegan commissary/outdoor general store
(approximately four theatre circular floor areas), with produce and pantry
fixtures, camping displays, clothing, toys, checkout counters and a 152 × 96 m
north-east greenhouse wing. Nine reusable starter goods have editable Blender
models. The catalog explicitly separates modeled displays from planned stock;
purchase, currency, item ownership and persistence remain unfinished. No animal
products or firearms are stocked.

The 104 × 184 m experimental dark organic Gothic church has curved asymmetric
101/119 m spires, branching ribs, colored lancets and rose window, a main nave,
carpeted U balcony with two staircases and protective rails, altar and suspended
plain cross before a full-wall Progress Pride banner. Sanctuary and gallery
use the theatre's existing red carpet material. Both buildings and goods have
editable Blender masters under Art/Town; builders live in BuildSupport/Town.
The civic foundation survey, physical supports, northern connecting roads,
vegetation reservations and acoustic shell load through TheatreSystem. Static
interior fill extends the venue's existing cached lighting treatment. The shared
venue allocation is now 196,608 triangles within the unchanged 262,144 triangle
scene cap; remaining scene capacity is reserved for terrain, actors and overlays.
Civic acoustics use volumetric architectural envelopes and roof surfaces rather
than thousands of decorative/merchandise triangles.

Validation: both civic scale/budget and physical stairs/railing/store-door tests
pass against the combined town, including the independently added neighborhood
and marsh (190,657 triangles in the final test). Six final native WebGPU views
were captured and visually reviewed: exterior, sanctuary, balcony, commissary,
food hall and greenhouse, under output/checks/civic-20261110. Intent inspection
reports zero structural mismatches; this does not establish all behavior checks.
Blender previews are separate from game-renderer evidence. The greenhouse uses
open glazing bays and spaced tinted louvers in the opaque architecture pipeline.
Full-game manual traversal, installed-app packaging and other graphics backends
remain unverified.

## Authored marsh nature center — November 10, 2026

A fixed nature center north of the civic esplanade adds a walk-in cedar pavilion,
interpretation tables, contained still pools, cattail/sedge islands and a raised
loop boardwalk with two viewing decks. Follow the north branch at theatre-local
X=240 from the esplanade. Offline Blender assets and physical walking envelopes
use the existing town venue. This adds no procedural biomes. Pools currently
use opaque still-water artwork. See Art/Town/MARSH_README.txt for asset sources
and scope. Cargo check and the collision test pass, including pavilion access,
full-loop traversal and deck support. The assembled town uses 190,595 of
196,608 venue triangles; intent inspection reports zero structural mismatches.
Native game review and installed-app packaging remain unverified.

## Outdoor wind above cover — November 10, 2026

Wind sources now carry an outdoor-wind marker into the terrain acoustic adapter.
Each receiver probes its accepted geometry downward from 128 metres above its
local up axis at five Hz. Wind clears the highest detected cover by one metre,
with a two-metre minimum above the receiver. A half-second edge hold and eased
outdoor return limit doorway chatter. Roofs, stacked floors and underground
cover use the same query; overhead vegetation may also provide cover. Cover
beyond the bounded probe is not detected. Existing material acoustics determine
audibility; no indoor mute or gain preset is introduced. The microphone owns
independent cover state. All queries run on the acoustic control/worker side.

This explicitly extends continuous_ambient_layers intent from a listener-relative
wind anchor to an outdoor anchor above cover. Other ambient layers are unchanged.
Three new wind checks passed in an isolated harness using the production helper,
vector functions and acoustic Snapshot geometry; two existing geometry checks
also passed. Full-crate intent inspection and tests were blocked by concurrent,
unrelated theatre compilation errors (initially duplicate town definitions and
missing clearings, later missing Theatre/Civic.rs). Production Surface integration,
native listening and installed-app packaging remain unverified.

## Matrix and parallel Mix Bus components — November 10, 2026

The DAW now owns output buses separately from timeline tracks. Defaults retain
four L/S/FF/R Matrix Buses to the Flown Array and one clean Mix Bus, with an
empty timeline. Drag Matrix Bus or Mix Bus from DAW Components into the bottom
panel. Click a bus meter to select it; Delete removes it. First-page right I/O
controls patch matrix stem input, physical array destination/channel and mute.
Mix buses have separate monitor FX chains, output gating, and mute; their
outputs add without normalization. Bottom page 2 accepts effects on the selected
Mix Bus. Matrix buses have no FX. Physical array and virtual microphone remain
pre-mix; four-stem export remains upstream of monitor processing.

This extends the prior fixed stereo-master expectation: each Mix Bus begins
with the same stereo fold and owns its own processing. Bus edits participate in
content Undo; deleted-bus inserts stay dormant for restoration, and their IDs
cannot be reassigned to new components. The physical-output catalog currently
contains the engine's Flown Array, rather than an OS audio-device selection UI.

Validation: six focused bus PCM tests, two registered Host/controller tests,
and related browser, mixer, I/O, playback, export, undo, native stock-FX and
transport regressions pass. Intent inspection reports no active structural
mismatches. Native visual/listening review and installed-app packaging remain
unverified. See Workstation/README.txt for controls and retained limitations.

## Bottom mixer alignment and output palettes — November 10, 2026

Fader cap layout and pointer dragging now share the authored slider travel
(-43.5..65.5 mm), scaled with the 180 mm housing. Track meters reserve the
same bottom readout band as their adjacent fader housings. Baked meter display
content is clipped to the saved protective-window material mask so it cannot
cover casing bevels. Persistent artwork and offline lighting remain unchanged.

The four main LSFFR outputs use Cardinal's inverted master palette; the stereo
2-bus uses its teal/navy/black/red/orange/teal monitor palette. Track meters keep
the standard spectrum. Palette selection lives in the existing per-strip packet;
audio observations and routing are unchanged.

Validation: 44 bottom-panel tests passed, including resized-panel slider bounds;
the generated compositor shader validates and its no-live-optics reachability
check passed. A native WebGPU capture rendered the six colored output meters
and was visually reviewed. The legacy --check-workstation-controls command
reports incomplete because its six static specimen poses are hidden; its four
frames did render. Native track-fader dragging and installed-app packaging remain
unverified.

## Numbered bottom-panel pages — November 10, 2026

Three equal mini gem buttons replace the previous/next arrows: 1 Mixer,
2 Plugins, 3 Clip Editor. Direct selection presses the current page in ivory
using the mute palette and persistent baked artwork. Buffer remains hidden.
Validation: all 12 bottom-panel tests pass; intent inspection reports zero
structural mismatches. Native visual review and installed-app packaging remain
unverified.

## Hidden bottom Buffer page — November 10, 2026

Bottom-panel arrows now navigate Mixer, Plugins and Clip Editor, skipping the
explicit Buffer/Loopback page in both directions. Capture/history functionality
and stable internal page IDs remain available for arm-button tracking.
Native UI review remains unverified.

## Bottom mixer output layout — November 10, 2026

The first bottom-panel page spaces the six main/monitor meter spectra at 40
logical points so their 30-point gem casings have clearance. Output casings
reserve the same 22-point bottom readout band as the faders instead of filling
the full content height. The soundtrack-next and speaker-next buttons are
removed from this page. Native visual review remains unverified.

## Night water reflection correction — November 10, 2026

Scene preparation now preserves the configured sky-reflection blend weight as
the authored sky palette changes through the day. Night brightness scales the
neutral fallback instead; darkness no longer selects a nearly white reflection.
The focused regression passes for sky weights 0, 0.4 and 1. This changes the
shared water packets for all native backends; nighttime gameplay/readback and
installed-app packaging remain unverified.

## Cave entrances, dripstone and minerals — November 10, 2026

Three new surface routes reconnect to the existing descent: a western rift,
eastern gallery and northern sinkhole. A low grotto, broad mineral gallery and
vertical rift add distinct chamber proportions. Existing upper brown rock and
blue Depths rock retain their materials. Deterministic floor/ceiling probes add
115 Blender-authored formations and inclusions, with a clear corridor around
route centerlines. Upper stalagmites contain ruby or silver; the Depths add
cyan-blue and magenta crystal clusters matching Cardinal's selected original
atlas palette. Each crystal root probes its own floor to prevent floating.

Art/Terrain/CaveFormations contains the editable .blend, seven centered GLBs,
inspected Preview.png, offline generator and embedded runtime convex meshes.
Fine visible faces and collision half-spaces share those meshes; geological
surface-net spacing remains unchanged. Accepted excavation domains remove both
formation collision and faces. Pickaxe refinement samples mineral identity,
mass and tint into existing bonded stone cells, preserving them as physical
pieces move and fracture. Reports expose mineral_mass_kg. These are physical
drops, with existing stone mechanics and density, rather than inventory items.
Independent mineral mechanics and calibrated hardness are not implemented.

Validation: all 35 terrain tests and all 23 cave-filtered checks pass (overlapping
sets), including mineral refinement/mass/color, accepted removal, entrances,
collision, mesh budgets and the deep-cave rendering regression. All seven GLB
headers and centered export nodes validate; the Blender preview was inspected.
The normal executable builds with debug information disabled, and intent
inspection reports zero structural mismatches. Evidence:
output/checks/cave-expansion. Native gameplay, actual pickaxe/drop
interaction, scene lighting and frame cost remain unverified. The installed app
has not been repackaged. Existing terrain/session edits are retained; the new
geological branches alter resting cave shape at their configured locations.

## Pennsylvania woodland expansion — November 10, 2026

Original Blender assets add sugar maple, black cherry and yellow birch to the
existing four trees. Witch-hazel, wood sorrel, Pennsylvania sedge, sandstone,
shale and small stream stones each have three editable .blend/.glb variations
and three bounded runtime mesh levels. Art/Trees/Pennsylvania_Hardwoods.png and
Art/Understory/Woodland_Expansion.png are inspected Blender asset previews.
These are artistic approximations; bark uses existing atlas proxies.

Candidate tree occupancy rises from .95 to 1.25 times habitat suitability;
roots and stable IDs retain their 14-metre grid. New habitat weighting changes
some generated species and corresponding saved branch-damage interpretation.
Ground colony acceptance rises .58 to .86; original colonies retain their
species, additional colonies supply the expansion. Small colonies increase
from four to six individuals. Existing water/venue filters and total scene
triangle budgets remain. Stones are decorative without physical collision.
Seven-species desktop baked silhouettes retain the actual production tree
shapes and prevent the new species exhausting distant forest coverage.

Validation: 29 vegetation checks passed, one offline baker ignored in the
ordinary suite; the explicit seven-species offline bake passed separately.
All 21 new GLB headers and matching Blender files validated. OpenGL color and
Vulkan color/shadow shaders compile. Evidence: output/checks/pa-woodland-expansion/.
Native review was attempted but the application archive build failed with
No space left on device. Native screenshots, traversal, frame cost and installed
app packaging remain unverified. One incidental syntax correction restores
Theatre/Waterpark.rs tuple access `.0`; no waterpark behavior was changed.

## Virtual microphone — November 9, 2026

Validation: 57 selected checks pass, including a full 32,767-frame VM1/physical
queue drained at a 128-frame preparation quantum with exact PCM replay. The
native WebGPU embedded game/DAW check completes 12 measured frames and clean
capture shutdown. Intent inspection reports zero structural mismatches. The
debug run retains source/transport underruns; this is not listening verification.
Native device take recording and manual listening remain unverified. Evidence:
output/checks/virtual-microphone/.

A Blender-authored walnut/brass microphone stands at stage-local [0,5,24].
Its capsule at [0,6.55,24] faces the audience (-Z). Select VM1 on an audio track,
arm it and record through the existing preamp/take workflow. The microphone
uses its own terrain capture, acoustic worker, cardioid routes and ambient
source positions. A separate cabinet receiver captures physical theatre speaker
buses. Microphone PCM never enters the player output implicitly; explicit track
monitoring and recorded clip playback retain their normal behavior.

The model source, GLB and inspected studio preview are in
Art/AudioWorkstation/VirtualMicrophone. Runtime triangles ship under Procedures.
One stationary microphone is implemented; custom placement is a future extension.
The combined theatre/park/microphone allowance uses half of the existing shared
262,144-triangle scene capacity, leaving the other half for world contributors.
No GPU storage limit is increased. Capture shutdown drains the detached queue
using an input-count bound instead of the old six passes, preserving complete
takes at small native block sizes.
Float-literal syntax corrections in the concurrent waterpark code also allow
the integrated build to compile.

## Audio preamp and Edison transfers — November 9, 2026

Validation: 656 workstation tests pass (two ignored), along with 21 overlapping
plugin regressions. Intent inspection reports zero structural mismatches. Native
WebGPU benchmark and fixed-panel runs each completed 12 frames and released all
Entities; the split image was inspected. Evidence: output/checks/preamp-split/.
Manual native Z/drag gestures and third-party microphone recording/listening
remain separate validation obligations. The installed app was not repackaged.

On the Plugins page, audio tracks reveal a small bottom-left preamp gem with
its own scroll position. Its width matches the left browser; the normal bottom
panel continues to the right. The split eases open and closed when switching
between audio and MIDI tracks or leaving the Plugins page.

Select an effect Edison and press unmodified Z to move it between playback and
preamp, keeping its plugin instance, settings and editor. Select an Edison before
dragging to reorder it or move it across the split. The held object lifts and
follows smoothly; neighbors slide around the proposed insertion gap. Release
commits once. Escape, blur and other modal input ownership cancel the transfer.
Scroll either chain independently; dragging near an edge scrolls that chain.
Instruments cannot enter a preamp, and MIDI tracks do not have preamps.

Preamp effects print into new audio recorded from that track's input. Imported
and dragged-in audio retains its source PCM and playback uses the ordinary
insert chain. Artwork remains persistent image composition: the small gem reuses
the baked bottom-panel image with its own transform, without runtime optics.

## Wearable DAW views and acoustic visualization — November 9, 2026

The right panel's track-I/O page includes RAYS ON/OFF. The session choice stays
in effect after leaving the workstation: ordinary walking hides audio rays,
while worn AR or VR glasses reveal them. This is acoustic diagnostic artwork,
separate from the DAW's offline baked optical artwork. Actual VM channel peaks,
cabinet peaks, source coverage probes and existing reflection/visibility paths
supply the visualization; silence produces no source nets. Microphone paths use
violet. The existing budgets bound geometry and probes; no extra acoustic solver
rays or callback-side allocation, locking, file access or geometry generation is
introduced. Per-channel callback observations use fixed-size atomics.

E opens/closes the workstation through worn glasses. AR stops the player and
locks the camera to the current game view, used as the backdrop. AR cannot select
plain color or security cameras. VR opens on the player's live view and retains
WASD walking and pointer movement for looking. Its right-panel PLAYER VIEW /
CAMERAS button, or C, switches between player and remote views. In camera mode,
WASD pans the selected security camera and the player stops; V selects the next
security camera. VR also permits the plain backdrop in Settings. MIDI keyboard,
focus, modal and active editing guards retain input priority. Computer entry
keeps its existing independent camera behavior.

The baked WebGPU workstation composites animated unlit ray geometry after the
completed world color image, using the matching saved world depth, before the
baked UI. Ray animation and UI edits do not recapture AR's paused world or run
live gem optics. VR player movement changes the observer and refreshes the real
world normally. Other graphics backends use their existing scene composition.

Finite native verification uses WORMWOOD_WEARABLE_VERIFY=ar or vr with
--benchmark-game-workstation WebGPU --frames 12. It wears the real pair for the
bounded fixture, enters through the shared production transition, enables rays,
and records player/camera poses in benchmark.wearable_verification. The VR fixture
requests walking and pointer look, switches to cameras at frame 4 and returns to
player view at frame 8. This is deterministic input replay, not manual gameplay.

Validation: optimized macOS build and the native AR/VR checks completed. AR kept
one player/camera pose across 36 samples and reused its paused world image after
the initial size change. VR moved in player view, kept the player fixed during
four changing remote-camera samples, and resumed walking afterward. The corrected
interleaved ray upload was inspected in a native capture. Focused wearable,
glasses, ray geometry, vertex ABI and retained-world checks pass. The broader
track-I/O filter has 18 passes and one failing mobile hidden-piano transport test;
this is not a full mobile validation. Evidence: output/checks/wearable-daw/.

## Piano-roll pitch bends — November 9, 2026

Validation: 650 workstation tests pass (two ignored), all five new bend checks
pass, and the baked shader validates without live-optics calls. A four-frame
native WebGPU capture shows opposing bends. Intent inspection reports zero
structural mismatches. Evidence: output/checks/note-bends/. The separate Host
mobile transport integration test fails its button-pressure assertion; native
input and third-party plugin listening remain unverified. Concurrent Origami
and headset changes were preserved, with small compile-only fixes to their
status construction, JSON array and owned view values.

Option/Alt-drag a soap bubble vertically to bend its ending pitch up or down.
The film follows a smooth curve over the note's duration. Option/Alt+Shift-click
clears the bend. Release commits one undoable edit; Escape cancels. Bend amounts
are saved as `bend_semitones`, defaulting to zero for existing notes, with a
range of -2 to +2 semitones. Move and resize retain the bend.

Plugin playback uses standard channel-1 pitch bend (VST3 MIDI mapping, MIDI
wheel events for AU/VST2/MIDI CLAP, and tuning expressions for native CLAP).
MPE is not required. Set the instrument's pitch-wheel range to +/-2 semitones.
A channel bend affects the whole chord; if bent notes overlap, the latest bent
note onset controls the channel until it ends, then any older held bend resumes.
The wheel recenters when no bent note remains. The built-in sine instrument
supports independent bends with continuous, seek-deterministic phase.

The production renderer deforms persistent baked film images using a cubic
coordinate transform. No runtime lighting, refraction or artwork baking is added.
`WORMWOOD_PIANO_CHECK_BEND=2 --check-bubble-notes WebGPU` selects opposing bends
in the existing finite native diagnostic. This source change does not install
or repackage the application. Third-party plugin response and listening remain
separate validation obligations.

## Fixed theatre lights and CC control — November 9, 2026

Validation: 15 lighting-related tests, 20 theatre-related tests, 27 focused CC/
automation checks and the native 2048-point overlay-capacity regression pass
(these selections overlap). Intent inspection reports zero structural mismatches.
Native WebGPU red/blue/blackout readbacks and both model reviews were inspected.
The real theatre-computer embedded transition completed 32 measured frames with
35 distinct lighting states over 36 samples, averaging 32.2 ms/frame (p95 35.7 ms).
Measured transport and ambient-VM underrun deltas were zero; startup lifetime
underruns were nonzero. This finite silent CC timeline is not a sustained song,
plugin-load or acoustic-listening test. Evidence: output/checks/stage-lighting/.
A separate optimized app is at output/macOS/stage-lighting/MirrorPrismals.app;
its ad-hoc signature and packaged lighting/contract hashes verify. The former
installed app bundle remains available. Windows/iPhone runs were not performed.


Four fixed LED spots and two LED wash bars now inhabit the theatre. Their Blender
models use the PA's graphite/steel hardware palette with separate optical cells,
yokes, heat sinks, fittings, connectors and fixed mounts. Sources, GLBs and review
renders live in Art/Theatre/StageLighting; the separate packaged runtime packet
loads alongside the existing room. The deliberate venue limit is now80,000
triangles, including half-metre sampling along the original64 stage boards.
Acoustic/contact geometry is unchanged.

CC target WORKSPACE exposes each named LIGHT and INTENSITY/RED/GREEN/BLUE.
Existing point editing drives seek-safe linear curves at the audible transport
cursor. Lighting is control-side visual work and never part of PCM rendering or
native callbacks. See Workstation/README.txt for controls and gap/overlap behavior.

The theatre camera now has an explicitly selected rehearsal path over the actual
venue and shared native materials. Lighting changes refresh that room pass without
outdoor terrain/water/vegetation/reflections; unchanged room/observer packets retain
the image. Outdoor cameras and persistent baked DAW artwork retain their policies.
The former full-scene theatre path is retained when rehearsal is not selected.

Native lighting uses cached fixed receiving weights and interpolated vertex color,
not Blender's Cycles/PBR result. Receivers cover the deck/backing; moving occluder
shadows, haze, gobos, moving heads and physical lighting-device output are excluded.
This is a targeted stage-lighting port, not Cardinal voxel/skylight propagation.

## Track freeze and flatten — November 9, 2026

Select a main timeline track handle (with no clip selected), then press F to
freeze its audio or MIDI. The separate progress popup keeps Cancel available
after Finished; OK accepts the rendered audio. Cancel or closing the popup
discards the provisional result without replacing original clips. Select the
accepted frozen track handle and press F again to flatten it into editable audio.

Freezing renders clip treatments and playback instruments/effects through an
isolated native host, before track gain, gain automation and LSFFR routing.
Those mix controls stay live. Effects receive a two-second tail; the built-in
MIDI instrument receives its release tail. Frozen content and inserts are locked,
recording/monitoring on that track is disabled, and flattening removes playback
inserts and retained original clip treatments. Shared source assets and comp-take
storage remain in the document. VST2 freeze is unavailable. Existing source
memory and matching sample-rate limits apply. Rendering advances in bounded
control-side blocks; no popup or rendering work enters the device callback.

Validation: 641 workstation tests pass (two ignored); eight freeze, buffered-key,
MIDI, stock-plugin and Host-route tests pass. Native popup state checks pass;
intent inspection reports zero structural mismatches. Evidence:
output/checks/track-freeze-20261109/report.json. Native
F-key interaction and third-party plugin/device listening still require review;
the installed application has not been repackaged for this source change.

## Clip gain and waveform controls — November 9, 2026

The bottom Clip Editor now opens Waveform, with per-clip gain in one-dB steps,
unity reset, source-range selection, trim and split. Its Warp tab connects
source-anchor add/drag/remove and semitone pitch controls to the existing
Cardinal map/worker. Spectral repair remains on its own tab. Raw source PCM
stays unchanged; gain precedes track effects/routing and is frozen in export.
See Workstation/README.txt for controls and the explicit split/warp restrictions.
Native UI appearance, interaction and device listening remain unverified.

## Bottom-panel waveform editor — November 9, 2026

The existing fourth bottom-panel page exposes clip gain in 1 dB steps with a
unity reset, waveform range trim/split, and general-warp marker and pitch edits.
Double-click an audio tape to open it. Invalid endpoint splits, warp-enabled
trim/split and marker actions without an editable selection are now disabled.
See Workstation/README.txt for gestures and supported warp scope.

Validation: 42 focused bottom-panel/controller/backend/scene tests pass,
including rendered clip-gain scaling and trim preservation. Intent inspection
reports zero structural mismatches. Native visual/device listening review and
installed-app repackaging were not performed for this change.


Clip editing validation: 632 workstation tests passed, two ignored; 38 clip-focused
checks passed, including gain-scaled PCM, frozen WAV gain and registered Host
routes. The normal executable builds and intent inspection reports zero
structural mismatches. Native UI/device listening remains unverified.

## Audio clip lip editing — November 9, 2026

Audio clips now reserve their top and bottom lips for movement; the center
selects and drags a time range. Audio lanes default to 96 logical points; MIDI and CC retain 64.
Command-E splits selected audio at the edit range anchor or playhead through
a revision-checked control-side edit. See Workstation/README.txt for treatment
and fade restrictions. Native appearance and interaction have not been reviewed.

## Attached manual comp lanes — November 9, 2026

Drag **Comp Lane** from DAW COMPONENTS onto an audio track (or one of its
existing takes) to append a silent take row directly below that parent. Take
handles use the same analytic gem cut as the main handle, with the original
ivory fader-housing material baked offline. Audio tapes and their source-derived
rainbow waveforms use the normal timeline presentation. Drag a handle sideways
to resize that row independently. Reordering the parent carries its take rows.
Right-click the parent or a take handle once to hide/show the parent's takes;
hiding does not discard audio or change the recording destinations.

Recording starts in the first existing take lane. Each loop pass advances to
the next lane, then stays on the final lane and punches later passes over it.
No lanes are created automatically. With no take lanes, recording uses main.
The parent main arrangement stays audible; take rows are storage/edit targets.
Option-click an audio tape to slice it manually, then drag pieces between the
parent and its takes. There is no automatic section selection or comp assembly.
Take tapes support horizontal movement, deletion and clip-local fade handles;
WAV imports can target a take row. Native clip-treatment editors remain main-lane
features; the take context notice directs users to slicing and manual transfer.

This explicitly replaces the previous handle menu and explicit-playlist-target
recording expectation. AudioPlaylist's legacy selection/record-target metadata
and the old nested menu implementation are retained as research alternatives;
production recording always starts at the first comp lane. Capture changes and
manual edits run outside audio callbacks. All existing bounded capture/source
limits remain in force. CompLaneIvory is shipped in Baked/manifest.json and
images.rgba32f; startup never invokes its offline baker.

Validation: 622 workstation tests pass (two ignored); eleven baked renderer/asset
checks pass (one explicit export ignored), including WGSL no-live-optics reachability
and transparent cut coverage. Intent inspection has zero active structural
mismatches. A four-frame real WebGPU arrangement capture was visually reviewed
with main plus two ivory take rows. Live microphone recording and device listening
remain unverified; the installed app was not repackaged. Evidence:
output/checks/comp-lanes-20261109/report.json.

## Edison insert selection and bypass — November 9, 2026

Click an Edison outlet to select it (blue highlight), then click it again to
open or close its plugin editor. Delete removes the selected insert. Right-click
the selected outlet to toggle bypass; bypassed outlets darken. Held right-click
and Delete act once per press. Existing drag-to-reorder remains available.
Selection and bypass tint the persistent baked artwork without optical refresh.

Validation: focused selection/editor-request regression and the real Host
browser/stock-plugin integration pass, including held-right bypass and held-Delete
removal. The integration requires RUST_MIN_STACK=33554432 (default test stack
overflowed). Baked WGSL validates and its no-live-optics reachability check passes.
Native third-party window/audio and visual appearance remain unverified; the
installed application has not been repackaged for this source change.

## Red wool theatre carpet, November 9, 2026

Integrated the custom Blender carpet on the ten terrace tops and central, FOH
and lobby ramps. Tags 117..120 preserve local positions and ramp tangent frames;
filtered albedo, normal, roughness, height and cavity maps append after existing
materials. Shared scene/workstation-camera shaders use shallow relief and an
approximate wool sheen. Geometry, collision, acoustics and non-carpet payloads
are unchanged, as checked during export. Current interior/exterior Blender masters
and the venue GLB/packet carry the assignment; generators reproduce it.

Validation: two focused carpet checks, fourteen venue checks, the cached venue
projection equivalence check and four isolated production camera-split tests
passed. Existing scene-asset checks passed. Desktop GLSL/SPIR-V and scene/camera
WGSL validate. The signed app at output/red-carpet/MirrorPrismals.app passed
packaged map/mesh/shader comparisons and nine-view WebGPU, OpenGL and Vulkan
native reviews. WebGPU close-up, central ramp and room captures were visually
reviewed; exact Blender lighting parity and iOS device appearance are unverified.
Intent inspection reports zero structural mismatches. Evidence:
output/checks/red-carpet/report.json.

## Pennsylvania understory colonies — November 9, 2026

The live vegetation packet now includes original Blender-authored Christmas
ferns, mountain laurel and mayapples. Three variations per species have three
runtime mesh levels (109–652 triangles per plant), exported from the editable
Art/Understory Blender sources. Authored colors and modeled leaf/flower outlines
use the existing native foliage lighting and shadow path, without tree cutout
masks or new texture bindings.

Stable six-metre cube-face colony seeds produce patches within 40 metres, using
woodland density, soil and moisture descriptors. Continuous terrain heights set
roots; water and theatre/venue clearings are excluded. Ground flora uses at most
one quarter of the existing 16,384 live vegetation triangle budget; trees and
distant woodland share the remainder. Mayapples appear on days 85–230; fern and
laurel remain evergreen. Laurel flowers are currently fixed artwork. These are
artistic habitat/season approximations, with abrupt mesh/range transitions.
Plants are decorative: no collision, gathering, wind, cutting or persistent
plant ecology; saved excavation does not yet uproot or re-seat them.

Validation: 29 vegetation tests pass, zero failures, one ignored benchmark.
Native WebGPU review rendered all three species through production colony
packets alongside trees (15,132–15,561 total triangles; 4,009–4,070 flora
triangles, 8–15 plants). Images were inspected; this isolated vegetation review
does not render the terrain, so ground contact still needs an outdoor full-scene
walkthrough. The signed output/pa-understory/MirrorPrismals.app completed a
30-frame real-game startup and cleanup. Intent inspection reports zero structural
mismatches; these checks do not establish frame-rate improvement or OpenGL/iOS
visual parity. Evidence: output/checks/pa-understory/final-WebGPU/report.json and
output/checks/pa-understory/final-game/interactive-WebGPU.json. The Blender
preview is not an in-game screenshot.

## Panel movement morph sound, November 9, 2026

All arrow-key panel actions share Procedures/chuck/daw/panel_motion.ck. The
previous bell is replaced by the author's three-facet sine morph, fixed to
version 1 (0.75 pitch) for every panel. The authored timing, partials, envelopes,
JCRev mix and 0.045 output gain are preserved, with the existing feedback gain
argument and channel-17 workstation bus. The panel_motion cue gain is enabled
at 1 instead of 0. Reopen the workstation to reload cached cue gains.

ChucK silent execution accepts the script. Native listening remains unverified;
the installed app has not been repackaged for this source change.

## Physical tree-felling hinge, November 9, 2026

The existing axe/material transfer path now prepares a compliant cross-grain
band around the initial strike. This replaces rigid merging across that band:
remaining bonded wood supports and bends under crown weight and tool contact.
Damage reduces tensile/shear stiffness while compression retains support.
Local edge tension/shear supplies the bending fracture criterion; opposing
tractions no longer cancel out of its stress calculation. Stump anchors,
retained crown mass/geometry and stiff fibres outside the band remain.
There is no hit-count, timer or commanded fall angle.

Use tool 3 (axe), aim at reachable lower trunk, hold primary to charge and
release to cut; repeat against the exposed wood. Existing wood/contact work
still supplies cutting damage. Upper branches remain a rigid crown, cut
resolution is prepared around the first strike, and the reduced material
parameters are uncalibrated. No native visual or end-to-end axe-felling result
is claimed from the solver tests. The installed app has not been repackaged
for this source change.

Validation uses `cargo test --offline --locked physics::matter::` including
`felling_checks`: intact support with damage updates enabled, partial-cut
compliance, bending stress without net-force cancellation, and gravity-driven
fall of a deliberately severed off-center crown with anchored stump and
conserved mass. Prescribing the severed test interface isolates the result of
a completed cut; it does not establish successful axe work or ground contact.
The material suite passes: 15 passed, 0 failed, 1 opt-in benchmark ignored.
Intent inspection reports zero structural mismatches; native weak/strong
strokes, completed felling and fallen-tree collision remain to be playtested.

## Wearable VR and AR workstation access, November 9, 2026

New independent Blender glasses match the phone/computer graphite and warm-metal
palette. Both pairs start beside the theatre computer. F picks up an aimed pair
within 3.25m or places the carried pair on a supporting surface; B wears/removes
it; E while worn opens the same embedded DAW as the computer. Both retain the
normal world view. No Cardinal dimension, portal or rendering modes are imported.
Held pairs suspend the tool and supply two IK grips; wearing frees the hands.
Phone focus, pause, lost focus, loaded tools and held-key edges are arbitrated.
Equipment is session-local; dedicated mobile glasses controls are not implemented.

Art/Glasses/Glasses.blend, separate GLBs and the Cycles preview are authored by
Art/Glasses/make_glasses.py. The game uses their matching runtime mesh packet,
3008 VR and 4000 AR triangles. Blender retains glass/metal/textile materials;
the existing runtime geometry pass uses diffuse facet shading. See the asset
README.txt for regeneration and limitations. Worn hardware is omitted from the
wearer's view and retained at the tracked head pose for external camera views.

Four focused regressions passed: mesh range/gap/behind rejection; both pairs'
wear/open edge and focus guards; metric exports/shared depth projection; and
real theatre pickup/supporting-surface placement including a straight-down ray.
The contract inspector reports zero structural mismatches. A separate signed
app is staged at output/glasses/MirrorPrismals.app. The signed final package completed eight WebGPU frames with zero remaining
Entities. Both pairs were visually confirmed on the desk in the native capture.
Runtime assets match the checkout. Evidence is in output/checks/glasses; the
full manual F/B/E-to-DAW-and-return interaction has not been exercised.

## Baked mixer meter animation repair, November 9, 2026

The baked aperture records the protective window (material 131), so testing its
frontmost material against the LED roles (9–130) never applied live levels.
The compositor now projects the two authored 5 mm by 156 mm spectrum lanes
through the fixed window artwork and reuses the existing channel palette and
level calculation. Housing artwork, clipping and depth remain baked; no live
optical work is added.

Validation: inspected the shipped baked aperture (10,624 samples, all window
material 131). The composed WGSL parses and validates, and the interactive-entry
call-graph test passes with no optical lighting reachable. Native playback and
visual animation still require verification.

## Clear water composition correction, November 9, 2026

The Cardinal91 low-alpha adaptation made water effectively invisible in native
use. Retire that adaptation: the native water layer already holds the complete
transmitted-scene/reflection result, so its compositor must apply that result at
full coverage. Applying the archived material alpha again suppressed waves,
refraction and highlights along with the tint. Remove the final neutral-tint
mix and low alpha from all four water fragment variants and the CPU reference.
Keep zero murkiness, actual sky colors, neutral fallback colors, and the removal
of the forced-blue reflection filter. This changes the prior low-opacity intent
explicitly in response to the author's native visual feedback.

GL/ES validation, Vulkan SPIR-V compilation and WGSL Naga validation passed.
The reference checks cover warm transmitted scene colors and a visible grazing
reflection, which the low-alpha adaptation suppressed. Earlier historical
Cardinal91 adaptation notes below are superseded by this correction.
Release package: output/water-visible-clear/MirrorPrismals.app. Signature and
packaged shader checks passed; intent reports zero structural mismatches.
Native WebGPU startup completed 180 frames successfully. Water appearance
matching remains unverified by native visual comparison.

## Cardinal91 clear-water appearance, November 9, 2026

Use the neutral surface/side tint endpoints and low opacity ranges found in
WORKSPACE_APR_8_9pm/Salamander91/91/Procedures/Shaders/Face.frag.wgsl.
Surface opacity interpolates 0.012–0.040, bounded to 0.010–0.045;
side opacity interpolates 0.04–0.12, bounded to 0.035–0.14. Existing water-layer
composition blends this material over the opaque scene instead of replacing
it with an opaque optical result. Refraction, continuous geometry, waves,
occlusion and reflection resource handling remain present. Sky colors no
longer receive the fixed blue fallback wash. Zero murkiness remains the default.
The CPU reference includes the same final scene/layer composition.

This is an explicit presentation expectation change from opaque Cardinal137
water optics. Continuous optical depth normalized by 24 metres replaces the
old block AO/variation proxy, and existing optical sampling supplies layer
content; historical terrain, block rendering, foam atlas and wireframe are not
ported. This is a visual adaptation, not a pixel-identical shader restoration.
The clear archive also exists in 91 copy 3 and 91 copy 4 and packaged Cardinal
apps; neighboring 91 copy and copy 2 retain the older saturated blue palette.

Validation: all GLSL fragment variants passed validation, Vulkan SPIR-V compiled,
and Naga parsed/validated the WGSL shader. The optical reference regression
preserves a warm underlying scene even beneath a blue sky at maximum surface
depth. Release package: output/water-cardinal91-clear/MirrorPrismals.app. Signature
verification passed, packaged shaders match source, and packaged intent reports
zero structural mismatches. A native WebGPU startup completed 180 frames and
exited successfully. Native visual matching to the archive remains unverified.

## Deferred scene publication after pause, November 9, 2026

Fixed the launcher session failure "presentation requires an active Audicle in
Go" reproduced after PlanetNavigationSystem.Pause while CPU scene work remained
in flight. Worker requests/results now retain the requesting Audicle identity.
A finished request is discarded; a live publication uses its original identity
and restores the caller context even if publication fails. The last displayed
image remains available while paused. Runtime lifecycle guards stay enforced.

Validation: 31 scene-filtered tests passed, including finished-event discard and
queued scene/Audicle coherence. The release app at
output/water-clear-startup-fixed/MirrorPrismals.app passed signature verification
and packaged intent inspection (zero structural mismatches). Play Mirror/WebGPU
was exercised through the native launcher and remained running until Stop
session, which completed normally. Earlier direct bounded startup runs did not
reproduce the launcher pause failure. Clear-water settings remain packaged.

## Clear water default, November 9, 2026

Further archive comparison found that Cardinal131 defaulted WaterMurkiness to
zero, while Cardinal132 changed the fallback to one alongside the blue tint.
Restore zero in the active water settings and default optical settings. Zero
removes absorption and its scattering contribution, preserving refraction,
reflection, waves and highlights. The CPU reference now also omits the retired
forced-blue reflection filter, matching the four native shader variants.
Earlier versions 92–128 had hardcoded absorption; version131's zero-murkiness
setting is the specific historical clear-water reference for this change.

Validation: release package output/water-clear-restored/MirrorPrismals.app
passed signature verification. Packaged settings use zero murkiness; all four
water shader assets match source. Packaged intent inspection reports zero
structural mismatches. Native visual verification remains pending.

## Water reflection colors, November 9, 2026

Restored the reflection color treatment from before Cardinal Salamander132.
Removed the forced red/green attenuation and blue boost from the GL, ES,
Vulkan and WebGPU water fragment shaders. Reflected scene and sky colors now
pass through to the existing Fresnel blend without that tint. Refraction,
depth absorption, waves and reflection selection retain their existing behavior.
This explicitly retires the inherited forced-blue reflection treatment at the
author's request; it does not roll back other water improvements.

Validation: GL and ES fragment validation and Vulkan SPIR-V compilation passed.
Intent inspection reported zero structural mismatches (behavior checks remain
pending). Release packaging completed at output/water-reflection-restored/MirrorPrismals.app.
Code-signature verification passed; all four packaged shaders match source, and
the packaged engine reports zero structural intent mismatches. Native visual
comparison remains pending.

## Track-height handles, November 9, 2026

Timeline handles now distinguish horizontal resizing from vertical reordering
using Cardinal's four-point dominant-axis gesture. Sizes are view state keyed
by stable track identity, bounded to 32–256 logical points, and retained for the
workstation session. Release accepts; Escape, pointer loss, changed panel bounds
or changed document cancels. Reversing at a size limit responds immediately.
Clips, fade/range overlays, drop/picking bounds and input monitors use variable
lane heights. Production transforms existing baked images without lighting work.
Cardinal's screen-edge virtual continuation and free vertical placement are not
ported. Project-file persistence is not added.

Validation: 681 workstation-filtered tests passed, four ignored, using a 16 MiB
test-thread stack after the default stack overflowed in a Host integration case.
Seven timeline geometry tests passed separately, including variable handle/tape
packing. Packaged intent inspection reports zero structural mismatches.
The signed staged app is output/track-height-handles/MirrorPrismals.app.
Native visual and mouse-interaction verification remain pending; no app was
launched and the previously installed app was not replaced.

Deer suspended bound, November 8: escape speed is now capped at 15 m/s with
12 m/s² acceleration. A 7.6 m stride separates hind push, fore support and an
all-hooves-free flight arc calculated with 9.81 m/s² gravity. IK releases world
anchors during flight, tucks swing legs and replants on contact. Terrain and
obstacle acceptance still follow the conservative ground route; this is a
procedural ballistic animation, not a free rigid-body quadruped simulation.
Alert watching and forward-facing escape remain separate states.

## Grass/dirt continuity in distant terrain, November 8

Intentional dirt patches from the detailed ground now supply the base cover of
coarse exterior terrain too. Previously the coarse mesh classified only the
undisturbed landform, so it stayed green until the detailed surface loaded.
World-anchored 24 m tiles reconstruct the same 3 m surface-net sampling phase
and interpolate its material colors onto the coarse mesh. Detailed geometry,
collision, excavation and grass/dirt texture assets are unchanged. This is
material sampling during geometry construction, outside audio callbacks;
1024 cached tiles bound retained memory and are invalidated on field changes.

Regression checks compare actual detailed dirt and grass against coarse cover
and shared points in 0.5/4/8/32 m faces. The actual distant ring build contains
both materials (21171 dirt and 126303 grass vertices inside 320 m in the pinned
spawn view). These counts are geometry evidence, not a pixel-equivalence claim.
Very small patches remain limited by distant vertex density; texture detail
still filters with distance. Surface cover currently follows the selected
surface-net outdoor range; other meshing alternatives retain their own path.

The final signed macOS app is updated; its previous package is retained beside
it. The optimized native WebGPU outdoor check completed eight frames and clean
cleanup. The cold Appearance.View took 2.94 seconds; subsequent stationary views
took 26.5–27.6 ms. These are bounded CPU observations, not a moving-frame or
audio stress claim. Evidence and verification.json are under
output/checks/terrain-cover-20261108/. Intent has zero structural mismatches.

## Transparent baked panel corners, November 8

The offline panel integrator returns a valid panel ID with negative hit distance
when a ray misses the clipped convex silhouette. The baker previously checked
only the ID, writing background radiance and opaque coverage at cut-off corners.
It now requires a positive bounded distance before saving either color or
auxiliary coverage. Misses contain zero RGBA in both halves of the image; the
runtime compositor reveals the game behind the facets. No color-key removal
of legitimate white gem highlights and no live optical work is introduced.

The new panel_cut_corners_are_transparent_and_centers_covered regression fails
on the prior disk assets and passes after the explicit offline rebuild. It
checks all four corners, covered centers, every miss's color, and covered depth
bounds across all four panels. The masks retain 1,140–1,380 transparent pixels
per 512x512 panel. The prior engraving repairs are retained. Evidence and native
visual/performance reports live in output/checks/daw-corner-alpha-repair-20261108.
Native in-game expanded/smaller captures confirm transparent cut corners. Covered
panel color pixels match the prior artwork exactly. The 256-frame in-game motion
run completed at 54.12 loop FPS (GPU envelope 0.186 ms mean), with zero live
optical bakes/resolves, missing art, new transport/VM underruns or remaining
Entities. The signed output/macOS package is updated; the prior app is retained
beside it. Intent inspection has zero active structural mismatches.

## Baked DAW engraving correction, November 8

Transport symbols disappeared because the offline bake used the whole-DAW
camera footprint when filtering tiny isolated controls. Camera rows 2/3 now
match each artwork's bounds, restoring the original density-pattern symbols.
All 41 art variants are regenerated. The old image compositor also directly
summed three displaced interior lettering layers; live labels now sample one
primary front density layer with reduced opacity gain, retaining the original
font/fracture atlas without stacked screen-space copies. No live optics return.

Native real-game before/after captures were visually reviewed at expanded and
smaller panel sizes. Rewind, stop, record and play symbols are present and
browser/control captions have a single baseline. The separate 256-frame in-game
motion run completed at 54.42 loop FPS, GPU envelope 0.180 ms mean, with no new
transport/VM underruns, missing art, live optical bakes/resolves or leaked
Entities. This is a bounded current-checkout result, not a guarantee of 60 FPS
or a populated plugin/audio stress test. The screenshot runs include readback
and are excluded from the performance claim. Shader no-live-optics and disk
bounds checks pass; intent inspection has zero active structural mismatches.

The signed output/macOS/MirrorPrismals.app is updated and its prior complete
package retained beside it. Comparison images, old/new art and reports are in
output/checks/daw-engraving-repair-20261108. The offline authoring instructions
and compositor comments protect against restoring the two faulty assumptions.

Deer state correction, November 8: stationary alert deer watch the player;
fleeing deer smoothly release head aim and look forward along a direct outward
escape route. This explicitly replaces the earlier running look-back behavior.
Hoof IK and startle/recovery thresholds are retained. The earlier look-back GIF
and validation below document the superseded behavior.

## Tree LOD silhouette continuity, November 8

The author rejected the solid triangular hemlock and unrelated simple whole-crown
LOD in the screenshot. Those desktop representations are explicitly retired.
Desktop forest fallback now uses sixteen-view silhouettes and surface normals
baked offline from each species' actual detailed production mesh, with separate
wood/foliage coverage for seasonal changes. Each tree uses a camera-facing quad
at its existing root, actual growth scale and yaw. Damaged/moving trees retain
surviving geometry; an undamaged baked image cannot restore a cut branch.
Healthy authored species no longer insert an inflated Far spray crown before
this fallback when detailed meshes exceed the reserved budget.

Assets, format, offline rebuild and limitations are in Art/Trees/LOD/README.txt.
The atlas adds 24 MiB of immutable native material storage without changing the
vertex ABI or the existing scene atlas offsets. No baking runs in gameplay or
audio callbacks. Original Near/Middle models, identities, simulation and the
16384-triangle scene budget remain. iOS retains geometric Far fallback pending
its separate texture integration; Vulkan shaders compile but device rendering
was not run. Sixteen horizontal views, planar shadows, static distant wind and
discrete LOD/view handovers remain approximations, especially overhead.

All 26 vegetation tests pass, including baked wood/foliage, branch alignment,
damage and forest budgets. Optimized WebGPU/OpenGL each render 35 diagnostic
views: summer/winter Near/Middle/Far/baked comparisons and real forest selection
at 24/48/80m. Those forest packets contain 13926/14396/14514 triangles and omit no
cells. Native comparisons were reviewed; the hemlock's summer silhouette has
0.93 intersection-over-union at 8-pixel occupancy-cell scale in the pinned view.
This is one view comparison, not a universal equivalence claim. The packaged
game completed 120 normal interactive WebGPU frames and clean cleanup. Source
intent inspection reports zero structural mismatches, not behavior proof.

The usual and separate tree apps are updated. Previous complete apps and evidence
are under output/checks/tree-lod-20261108/. Long traversal, overhead appearance,
shadow fidelity and measured frame-time impact remain separate review work.

## Play Mirror bird-perch failure repair, November 8

The reported session ended with `invalid perch branch_id` after a bird occupied
an authored branch above ID30. QueryPerches emitted the correct identity, but
occupied-perch refresh still enforced the former 31-branch tree limit. The
DAW benchmark entered after two game frames and did not reliably reach this
later, habitat-dependent bird transition. That benchmark was insufficient as a
normal-game longevity check.

Perch parsing now derives its bound from the authored graphs. Damage, saved
broken-branch state and physical-wood restore use compatible authored bounds;
actual branch lookup still distinguishes removal from an unresolved site.
This intentionally supersedes the generic-tree assumption that ID31 is always
invalid; species-specific out-of-range IDs remain rejected without mutation.
No bird/audio behavior or baked DAW policy is disabled. Protective comments
explain why a fixed 31-branch bound must not return.

All 23 vegetation tests pass. The new regression refreshes up to 384 actual habitat
candidates across 32 seeds, including IDs above30, then checks damage, saved-state
roundtrip, removed-perch reporting and rejection of out-of-range IDs. Native
normal-game and embedded-game results are recorded separately in
output/checks/play-perch-repair-20261108.
The repaired signed package completed 600 normal gameplay frames (29.95 simulated
audio seconds) and 288 embedded DAW frames, with 428 successful perch queries, no
failed calls and zero remaining Entities after both sessions. Diagnostic command
injection was off in the normal game. The usual output/macOS app is replaced;
the previous complete app is preserved beside it.

## In-game DAW performance repair, November 8

The packaged game now uses the baked DAW through the real theatre-computer
Suspend / RunEmbedded / Resume transition, with the existing game audio active.
The original in-game test was 7.27 FPS even after baking gems: each UI frame
rebuilt and rendered the entire paused world. The production path now retains
the actual completed game background until observer/world/resize changes, then
composites independently baked DAW components over it. Water animation holds
with the paused visual; audio continues. Ordinary gameplay rendering resumes
normally. This supersedes the earlier DAW-only validation and game blocker below.

Measured macOS WebGPU FIFO on Apple M1 Pro at 1920x1200, 32 warmup plus 256
measured frames: panel motion 75.61 loop FPS, continuous real mixer fader dragging
with normal transport playback 82.10 FPS. GPU envelopes average 0.224 / 0.197 ms.
Loop p95 is 20.77 / 19.57 ms: these are averages, not a claim of every frame
meeting 60 FPS. Both runs validate 288 embedded passthrough frames, two world
captures and 285 retained-image draws, zero live gem bakes/resolves, zero new
transport/VM underruns, and complete cleanup with no remaining Entities.
Full camera panning/cycling deliberately refreshes the actual world: 18.64 FPS
in its separate 128-frame run. No claim of 60 FPS while rebuilding that world.
A separate screenshot run confirms the riverbank image changes to the mountain
camera, with the real world visible behind the panels; captures are excluded
from the performance comparisons.

The audio control path borrows immutable geometry rather than copying thousands
of branch cells each frame. Exact visibility queries reuse results only for the
same geometry revision/endpoints/distance; the ray budget, source movement,
head response and callback remain active. Four acoustic regression tests pass,
as do cache-key/shader, embedded prepared-scene reuse, full authored wood bound,
and baked shader no-live-optics/disk-asset checks. Intent inspection has zero
active structural mismatches; that is not audible or behavioral proof.
The conifer wood bound is derived from all 128 accepted tree sites and authored
branch counts; no branches were dropped to make audio accept the scene.

Evidence: output/checks/in-game-daw-20261108/report.json and per-run reports/logs.
The signed output/macOS/MirrorPrismals.app contains the verified executable and
baked artwork. The previous app is preserved beside it. Measurements cover an
empty unarmed track and game ambience, not populated plugin/recording stress or
physical scanout. Protective comments, AGENTS.md and DESIGN.txt prohibit
restoring live gem optics or per-UI-frame paused-world rebuilding.

## Play Mirror launch repair, November 8

The tree update introduced needle-spray/envelope material IDs 3, 4 and 5, but the
numeric and JSON packet validators still accepted only IDs 0 through 2. Full
game startup therefore failed with "invalid bounded numeric vegetation packet".
Both validators now accept the six defined integer IDs and continue rejecting
unknown/fractional tags. TreeCheck now validates packets before rendering so its
isolated review cannot silently bypass this boundary again.

All 22 vegetation tests pass, including numeric and JSON material acceptance and
unknown-tag rejection. The repaired optimized packaged app completed six actual
interactive planet_surface WebGPU frames with diagnostic command injection off,
then closed cleanly. This supersedes the earlier full-game startup blocker below.
Both the usual output/macOS/MirrorPrismals.app and the separate tree build are
repaired; their previous complete apps and evidence are preserved under
output/checks/trees-20261108/play-repair/. This is a bounded startup check, not a
long gameplay or audio-performance result.

## Persistent baked DAW artwork — author policy, November 8

The production macOS WebGPU DAW now draws independent image quads from disk
artwork instead of refreshing optical lighting. Fixed reflections, refraction
and illumination are explicitly accepted by the author. Panel motion, resizing,
button pressure, changing labels, faders, meter LEDs, clips, zoom and settling
must not invoke gem lighting, optical transport, floor-light resolves or shadow
calculations. The previous optical-freshness requirements are superseded for
production; the older implementation remains an explicit research alternative
selected only with WORMWOOD_WORKSTATION_REFERENCE_OPTICS=1. The launcher selects
baked artwork and no longer offers the old live Panel detail toggle.

Art/AudioWorkstation/Baked contains 41 independent artwork variants with original
scene-linear lighting, depth, coverage and material data (79,036,416 image
bytes). The app loads this once; live instances update placement, scale, clipping,
image selection and unlit display data. Browser glyphs are independent sprites.
The offline asset build and fixed authoring fixture are documented in
BuildSupport/Workstation/BAKED_ARTWORK.txt. The package ships finished images;
startup never calls the baker. Missing disk files fail explicitly rather than
selecting live optics. Game/world lighting remains a separate renderer.

Native WebGPU FIFO, 1920x1200, eight warmup and 96 measured frames: canonical
panel-motion replay reaches 117.39 loop FPS; actual mixer fader pointer dragging
reaches 117.90. Both report zero runtime optical bakes/resolves and no missing
art. These are bounded empty/single-track graphics measurements, not scanout or
audio/plugin stress claims. Content/lighting-change and settling replay, populated
arrangement, note and sphere captures also run without optical bakes/resolves.
Images were visually reviewed. The generated shader validation and transitive
entry-call checks pass; ten existing zoom/presentation regression checks pass.
Structural intent inspection reports zero active mismatches. Detailed evidence
is in output/checks/daw-baked-assets-20261108. The signed local
output/macOS/MirrorPrismals.app was updated at that stage. See the in-game
validation above for the current shipped result.
The previous complete app is preserved beside it.

Historical blocker (resolved above): the first full-game test stopped at the
vegetation packet validator before entering the DAW. The tree validator repair
and complete authored wood budget now allow actual game/audio benchmark runs.
The legacy controls diagnostic still targets disabled old instances; the current
mixer is exercised through real pointer input by the in-game fader benchmark.

## Central Pennsylvania tree species, November 8

Generated woodland now mixes white oak, red maple, eastern white pine and eastern
hemlock. Both discovery and exact-ID resolution use the same deterministic,
artistic soil/moisture weighting. Existing tree roots and habitat density remain.
The former generic architectural profiles are retained for explicit callers.
This deliberately changes generated species/silhouettes; previous saved branch
IDs apply to the new graphs, while serialized physical bodies retain their stored
branches. No old save is rewritten. This is not a calibrated ecological survey.

Original Blender masters, GLBs, summer preview and generator live in Art/Trees/.
The live renderer embeds the exported normalized branch graphs and leaf outlines,
then builds its existing bounded LOD meshes rather than loading high-resolution
GLBs. This keeps structural wind, damage and perching attached to the same graph.
Oak/maple lose leaves and change autumn colors; pine/hemlock stay green in winter.
Native needle-spray and distant conical-hemlock masks match color/depth sources.
Oak/maple bark reuse approximate existing atlas textures; see Art/Trees/README.txt.

All 21 vegetation tests pass. The optimized native WebGPU and OpenGL diagnostics
each capture 24 single-tree views across summer/winter and near/middle/far detail.
Representative frames were reviewed, and the Blender lineup and individual scenes
were inspected. The largest single-tree diagnostic mesh is 7,504 triangles;
whole-scene budgets continue to select coarser meshes when needed. Structural
source intent inspection reports zero mismatches, not behavioral proof.
The separate signed macOS app passes packaged startup/contract validation. JACK
was unavailable during that startup; no audible device claim follows.

Evidence and the separate playable app are under output/checks/trees-20261108/.
The existing output/macOS app is retained. Use --check-trees WebGPU or OpenGL to
repeat isolated native review. Full landscape composition, traversal, live cutting,
LOD handover and frame-time impact still need in-game review; iPhone and Vulkan
device rendering have not been checked. Blender previews are higher-resolution
authoring models, not captured game frames. Packaged intent inspection cannot
resolve source bindings because the app intentionally contains no source tree;
source inspection and packaged startup validation are separate checks.

## DAW content capture regression, November 8

Panel detail no longer disables partial content captures. Content keys now
select their owning panel or the merged stationary timeline image, including
control changes and clip/pan/zoom data. Browser glyph metadata belongs only to
its panel. Unchanged panel RGB, bare-body and ownership images remain resident;
existing two-sample atlas migration and the full optical settle refresh remain.
Malformed or unsupported inputs retain complete capture fallback. No optical
quality setting, sample count or resolution is reduced.

A native 960x600 WebGPU FIFO empty-DAW canonical motion replay, Panel detail on,
eight warmup and 64 measured frames, improves from 5.02 FPS (repeat 4.96) to
18.08 FPS. Total captured groups fall from 426 to 76 across 71 captures; 70
patched captures refresh only the left browser group. No timing readbacks or
audio/plugin load were used. Browser lettering still recaptures during motion;
full settle work and live clip/zoom interaction feel remain unverified. This
is a measured regression improvement, not a 60 FPS or all-interactions claim.
Evidence: output/checks/daw-lag-20261108/report.json and adjacent native logs.

## Phone flashlight coverage and aiming, November 8

The user reported a dim beam, insufficient coverage and illumination away from
the aimed area. The phone lamp now follows the camera optical axis; its former
12cm downward offset is retired. Range increases from 22 to 28 metres, inner/outer
angles from 18/38 to 22/42 degrees, intensity from 1.15 to 2, recovered dark
reflectance floor from .24 to .4 and reflection scale from .72 to .8.
This explicitly supersedes the original Cardinal settings for Wormwood.

Ordinary RGB and packed-soil faces now gain bounded samples toward half-metre
edges. Large faces can illuminate inside the cone even when every original
corner lies outside it. Longest-edge refinement stops at 14 levels and at most
16,384 extra triangles, further limited by available native packet capacity.
Position, homogeneous depth, baked color and decoded soil UVs interpolate at
split edges. Visibility is sampled per vertex rather than rejecting a complete
parent face for one shadowed sample. The 17x17 trace budget remains fixed.

Shader coordinate payloads are explicitly excluded from RGB relighting; the old
positive-tag test could corrupt these material coordinates. Textured theatre and
rock, vegetation, water and player shaders still require separate lamp support.
Coarse shadows and interpolated lighting remain approximations, especially at
small obstacles and after refinement limits are reached.

Nineteen phone regressions pass, including interior coverage, near/rotated aim,
partial/full shadows, unchanged off/material packets, soil UVs and both budgets.
Structural intent inspection has zero mismatches, which does not prove behavior.
The optimized native WebGPU and OpenGL diagnostics captured off, level aim,
downward aim and partial shadow scenes; images were reviewed. Four source faces
become 3,156–4,064 samples; measured light-pass CPU times are .48–.69ms on this
M1 Pro. This synthetic scene does not establish full-game frame cost, traversal,
audio stress, Vulkan or iOS device behavior. Reproduce with
`--check-flashlight WebGPU` or `--check-flashlight OpenGL`.

Evidence and the complete previous app are under
`output/checks/flashlight-20261108/`. The local macOS app has the updated engine
and phone rendering contract; restart it to load the change. Deep/strict signing
verification passed. Its real planet-scene phone check captured all five apps
with successful cleanup. The audio session reported 14 underruns; this validation
does not establish glitch-free audio.

## Watchful deer with hoof IK, November 8

Deer now aim their neck and head toward the player independently of body travel.
Alert animals turn their bodies only enough to keep a comfortable gaze. Fleeing
animals favor a slightly diagonal outward route and hold a stable look-back
shoulder, with smoothed yaw/pitch and a 140-degree total head/neck yaw bound.
The muzzle, eyes and antlers remain rigid with the head; upper-neck weights blend
the turn into the body. This explicitly replaces the former direct-away heading
and baked-only head pose, while retaining the 14-metre startle and 34-metre /
four-second recovery conditions. Attention remains proximity-driven.

The existing InverseKinematicsSystem now exposes stateless SolveChains batches
using its retained FABRIK/pole solver. Deer own their four limb seeds and world
hoof anchors separately from the player's control skeleton. Standing and stance
hooves plant; swing feet follow the authored gait over sampled support. A turning
standing body lifts and replants one hoof at a time. Bounded body lowering and
level hoof skin keep support within reach without changing bone lengths.
Unreachable targets report their error. Disabling IK explicitly selects baked
legs with gaze tracking and clears hoof caches before later re-enablement.
Skinning, terrain queries and inspection remain outside audio callbacks.

The original 3,686-triangle deer is retained. deer_rig.json adds evaluated joints,
source skin influences and rigid-face membership alongside the embedded vertex
clips. Per-bone correction matrices are computed once per pose rather than
repeating angle solves per vertex. Neither original Blender model is modified.

Thirteen focused checks pass, covering outward escape plus look-back, rear-seam
stability, terrain movement without audio, IK disable/re-enable, fixed limb
lengths, planted contact stability, actual hoof/face skin, eight-instance
composition, generic reachable/unreachable batches and the existing player
working-stance regression. The low-debug-symbol scene test needed a 16 MiB Rust
test-thread stack; its default small-stack run overflowed. Native release
validation is separate. Structural intent inspection has zero mismatches.

Forty actual registered deer updates were exported and their evaluated IK/gaze
skin rendered in Blender, including the corrected muzzle and backward glance.
Art/Wildlife/Deer/Runtime_Look_Back_IK.gif is that body-frame pose preview, not a
captured game viewport. A native macOS WebGPU run completed 120 scene updates,
240 registered IK batches and normal cleanup with zero failed invocations or
remaining Entities. The audio transport reported 13 underruns; no glitch-free
performance claim is made. Live outdoor encounter feel and iOS device rendering
still need play review.

The signed local output/macOS/MirrorPrismals.app is updated; restart to load it.
The prior complete app, final test logs, runtime pose data and native
summary are under output/deer-ik-validation-20261108/.

## Independent gem control composition, November 8

The author accepts transforming cached gem images during motion. The macOS
WebGPU default now separates bottom/right gem controls from their panel bodies.
Each control retains its own captured pose and uniform scale; panels can change
size without stretching embedded faders. Original optical samples supply the
control RGB and ownership/depth images, and the bare panel image fills the
vacated area. Transport and existing fader-cap images retain their separate
paths. Changed browser lettering refreshes its own group; unchanged groups
survive atlas repacking. Live meters retain the canonical shader. On settling,
a synchronous original render refreshes body, color and ownership together.
Whole-image caching still covers every gem shader family; unsupported component
layouts and views retain their existing optical path. This supersedes the
unfinished motion result below, without claiming every element has its own
allocation or that changing content can always avoid capture.

Matched 160-frame empty-DAW benchmarks, 32 warmup frames, non-Retina WebGPU,
windowed FIFO, no validation readbacks or audio/plugin load: 960x600 motion
improves from 11.05 to 23.11 FPS (repeat 23.15); 1440x900 from 8.70 to 20.32.
Mean GPU pass envelope falls from 77.84 to 31.01 ms and 101.73 to 36.62 ms,
respectively. Settled 960x600 reaches 117.83 loop FPS with 0.09 ms GPU envelope;
this is loop throughput, not a scanout claim. Browser glyph changes still cause
captures, so motion remains below 60 FPS.

The motion-then-settle replay's final image and every sampled image from frame
361 onward match the full-render reference byte for byte. The moving compositor
comparison has zero changed pixels over 64 submissions using the same retained
inputs; it does not establish identity to fresh optical tracing during motion.
Moving captures were reviewed for control proportions. Focused component pose and generated shader checks pass; additional native
cache checks are recorded separately from the benchmark summary in
output/checks/gem-motion-transforms-20261108. Concurrent disk cleanup removed
the raw timing logs, captures and staged app after validation; report.json retains
the observed results and explicitly records that evidence limitation. Other backends, large/Retina
fallbacks and audio/plugin stress are not newly verified. The earlier rejected
grouped prototype is retained as an alternative; its bundled-control stretching
is replaced by independent control mappings here.

## Theatre exterior limestone, November 8

The outward theatre shell now uses the approved Blender dressed-limestone
material in the game. Tag 112 preserves local positions for cylindrical metric
courses, normal/roughness/AO maps and bounded height relief under current outdoor
lighting. Flat coloring is retired only on the 192 outward shell triangles.
Vertex positions, other material payloads, collision and acoustics are unchanged.
The appended material maps preserve all prior relative texture offsets. Desktop
scene and workstation-camera paths share the material; iOS/device appearance is
unverified. The approved editable source is Art/Theatre/TheatreExterior.blend;
export_exterior.py republishes its architecture without rebuilding other hardware.

Ten scene-material checks, six theatre checks and the embedded-camera WGSL check
pass. Desktop scene/workstation GLSL validation passes. Native WebGPU, OpenGL and
Vulkan day/night, near/far and interior captures complete; the WebGPU entrance,
close stone and interior were visually reviewed. The signed packaged app passes
its exterior capture and a six-frame normal WebGPU gameplay session, ending with
zero Entities. The intent inspector reports zero active structural mismatches;
behavior obligations remain distinct. No performance or iOS claim is made.

output/macOS/MirrorPrismals.app is updated and its signature verifies. The previous
app and validation report are preserved in output/checks/exterior-limestone/.
A missing serde derive dependency required by the concurrent deer rig was added
to unblock the build. Disposable debug incremental cache was removed after a
disk-full build; no source or saved user data was removed.

## User audio library, November 8

The macOS desktop left browser now includes an Audio category and USER_SAMPLES
rows under All. It discovers WAV files directly in USER and USER/USER_SAMPLES,
refreshing at most once every two seconds. Local packaged apps use this checkout's
editable USER folder; WORMWOOD_USER_ROOT overrides that location. The browser
preserves existing component/plugin rows, scrolling, drag labels and cancellation.

Create an Audio Track, choose Audio in the left browser, and drag a WAV onto its
lane at the desired elapsed time. An accepted import atomically creates the
source and full-length clip, selects it and uses normal transport/mixing/routing.
Files decode on the control thread only. Limits: 8 MiB per WAV, mono/stereo
PCM8/16/24/32 or float32, 8–192 kHz; sample rate must match active playback.
Rate mismatches report the required conversion explicitly. No MP3/FLAC decoding,
resampling, sample preview or mobile file browser is added in this pass.

The generated 440 Hz test tone is now USER/USER_SAMPLES/sine_440Hz_10s_48k_mono.wav.
Validation results are recorded in output/checks/audio-library-port/report.txt.

## Terrain transition correction, November 8

Ground detail now fades over 192–320 metres rather than 64–128, with UV
eligibility to 512 metres. Distant radial steps remain four metres through the
384-metre base ring. Thirty-one intermediate sampled rings replace the wide
near/distant connector. Both sides share the exact boundary and original
landform normals, so coarse faces do not introduce a separate lighting band.
The half-metre player support patch and physical terrain are unchanged.

The water surface retains its central 1.5-metre samples and adds six-metre
outer samples out to 240 metres per chart axis, previously 48. Outside the
central patch, missing accepted triangle coverage may use generated river/lake
levels and the analytic drainage bed for presentation only. Local unknown water
coverage remains unknown to physics. Distant edits, narrow streams below the
outer sampling scale, ocean horizon coverage and native visual/performance
verification remain limitations. This change does not establish complete
planet-wide material or water LOD correctness. Validation: eight focused terrain tests and eleven water physics tests pass.
The application bundle has not been replaced by this source change.

## Theatre monitor image placement, November 8

The captured DAW image is now centered on the authored display glass at local
y=.175 and placed at z=.059 before the monitor's 180-degree rotation. The
previous z=.054 plane was behind the opaque glass front at z=.058. The image
now has 1mm of clearance. Packaged mesh bounds and f32 depth ordering were
checked. Native visual validation and an updated app remain pending: the
build is blocked by the existing private WetPoint import in Water/surface_mesh.rs.

## Exact complete-image gem composition, November 8

The standalone macOS WebGPU renderer now renders all optical output, including
fallback piano/bubble and sphere views, into a native-format retained image and
presents its texels without filtering. Identical completed packets/resources
reuse the entire image. Changed inputs still render through the original shader
programs. The existing panel images and meter ledger remain enabled. Optical
materials, sample counts, exposure and AgX are unchanged. This is a whole-image
cache; independent caches for every gem element are not implemented by this pass.

Optical uploads now compare exact 128-row blocks in the packed address ranges;
asset writes invalidate the upload shadow. Deferred floor resolution uses those
same packed ranges. Atlas slots retain capacity while rectangles shrink, bounded
to 256 MiB of reserved allocation before falling back to current exact packing.
The capture rectangles/samples remain unchanged. Existing content invalidation
and immediate populated-timeline refresh rules are preserved.

30 focused cache, upload, content, defaults and atlas checks pass. Native
controls, arrangement, piano/bubble and sphere images match the saved old binary
byte for byte. The control diagnostic's mixer-visibility assertion fails in both
builds; this pass does not fix that pre-existing diagnostic. The separate disabled
primary-group-gates experiment still rejects its guarded source contracts and
is not selected. Final moving-frame validation compares 96 submissions with the
original compositor using identical current inputs, with zero changed pixels.
These validation readbacks are excluded from performance runs.

Matched 160-frame runs on the M1 Pro use 32 warmup frames, an empty DAW, WebGPU,
non-Retina windowed FIFO presentation and no audio/plugin load. Settled GPU pass
envelope falls from 7.301 to 0.088 ms; both loops remain about 60 FPS. Repeated
960x600 motion improves only from 10.46 to 10.63 loop FPS; 1440x900 motion improves
from 8.09 to 8.50. At 960x600, atlas reallocations fall from 133 to 16, but 153
optical captures still occur in the 192-frame session. Changing browser glyphs
still causes synchronous broad recaptures. Motion lag is not solved, and the
requested independent per-element conversion remains unfinished. No other
backend, mobile, interactive resize, plugin or audio stress validation is claimed.

A faster grouped-lettering capture prototype was evaluated and rejected: moving
panel images stretched their bundled controls, visibly changing fader proportions.
Its source and captures are preserved under rejected-layer-prototype in the
check directory, and it is not compiled into the selected implementation.
Preserving the requested appearance requires separate control images before
that optimization can be enabled.

The signed candidate is staged separately; output/macOS is unchanged. Results,
source snapshots, native captures and the candidate app are under
output/checks/gem-composition-20261108/.

## Independent deer wildlife, November 7

DeerSystem now owns up to two whitetail buck appearances independently of audio.
It is registered as a layer-five System with explicit Step, Snapshot and Close
hooks, separate lifetime and flee Audicles, and PlanetNavigation-owned updates.
Paused gameplay freezes the animals; Stop and Abort close them. Nearby animals
are never moved by respawning. Animals farther than 180 metres can be released,
with outdoor replacements discovered 40–70 metres from the player.

The deer notice the player within 24 metres and flee inside 14 metres, with
bounded acceleration to 7 metres/second. They steer away and remain startled
until the player has stayed beyond 34 metres for four simulation seconds, then
decelerate. Local route checks reject steep support, cliff steps, sampled water
or unknown water coverage, nearby trunk proxies and accepted material body
obstructions. A 100-metre theatre exclusion keeps deer out of the building and
approach. Blocked animals stop and retry rather than teleporting. This is bounded
local kinematic steering, not global pathfinding or a physical quadruped solver.

Art/Wildlife/Deer/Whitetail_Rigged.blend is an independently editable 14-bone rig
of the existing Retro_Deer derivative. The original derivative and high-poly buck
remain untouched. The 3,686-triangle mesh retains its authored palette, antlers,
white markings, ears, eyes and tail. Standing, alert and running actions are
baked to an embedded indexed skin. Runtime running phase follows actual accepted
travel distance; a blocked animal stops cycling its legs. Ground support supplies
pitch and roll; there is no individual hoof IK. No wildlife work runs in audio
callbacks. The scene instance bound increases from six to eight to accommodate
two tools, four birds and two deer; native triangle limits remain unchanged.

Eight focused checks pass: behavior thresholds/recovery, blocked routes,
cliff/steering/local-up handling, grounded looping skin, instance bounds,
registered movement on terrain without audio, and eight-instance composition
with cleanup. Structural intent inspection reports zero mismatches. Idle, alert
and four running poses were rendered and visually reviewed in Blender.
A native macOS WebGPU check completed 120 application frames and 37 scene
updates, created both deer, reached 100,918 scene triangles, reported no failed
invocations and removed all Entities on close. The audio transport reported
19 underruns and the VM one underrun period; this is not a glitch-free audio
or gameplay-performance claim. Outdoor encounter appearance/feel and iOS device
rendering still need play review. Evidence is under output/checks/deer/.

The local output/macOS/MirrorPrismals.app is updated and its signature verifies.
Restart it to load the deer. The previous complete app is retained under
output/checks/deer/previous-app/. Blender previews and a 2.4x-slow running GIF are
under Art/Wildlife/Deer/.

## Consistent terrain material across LOD, November 7

Terrain base color now samples the material field at every vertex for both near
and distant meshes. The previous whole-face majority color and edge-size blend
could assign a shared turf point its neighbors' soil color at one resolution,
then restore green when that face became coarse. That resolution-dependent
color policy is removed. Natural root-mat exposure now uses the existing fixed
metre-scale terrain gradient instead of the rendered triangle's changing slope.
Exposed turf side walls retain their root color. Fine grain still fades when
unresolved. The grass/loam textures, material strata, collision and mesh layout
are unchanged by this material fix. Gradient/color samples reuse the existing
per-build shared-vertex cache; there is no new per-frame terrain sampling pass.

The optimized build and separately staged signed app pass. A packaged WebGPU
run completes 48 scripted frames and closes with zero remaining Entities.
Full-workspace intent inspection reports zero structural mismatches.

Seven focused EnvironmentGeometry tests pass, including shared strata colors
across 0.5/4/8/32 metre faces, exposed root walls, deep-cave boundaries, numeric
projection and the independently authored forward-cone cache/ring checks.
The tests establish base-color consistency at shared samples; very small patches
can still be undersampled by coarse geometry. Exact near/far pixel coverage and
walking transitions at the user's location remain visual review obligations.
Evidence and the staged candidate app are under output/checks/terrain-material-lod/.

## Forward terrain detail cone, November 7

Low-altitude terrain now follows the camera's tangent heading in a broad forward
cone: full extension through 30 degrees, tapering to the original range at 90.
The eight-metre, half-metre support patch stays exact. Two-metre radial steps
outside it and up to 512 angular samples extend the near region from 64 to about
124 metres straight ahead. The shared distant boundary extends from 128 to
256 metres ahead; the radial mapping rejoins the original mesh at 2048 metres.
Side/rear reach is retained. The near mesh consumes the distant cache's exact
boundary vertices. Turns over about 11 degrees rebuild both adjoining caches;
smaller turns retain the selected cone. Explicit camera axes are supported and
vertical views retain the last useful tangent heading. Cave and high-altitude
selection retain their previous paths. This changes presentation sampling only.

Ground UV eligibility extends from 64 to 192 metres. Diffuse detail fades from
64 to 128 metres in GL, Vulkan, WebGPU and iOS source, retaining footprint/mipmap
filtering and the existing bounded UV packing. Large faces that cannot fit the
packed UV range remain untextured. The gap between near and distant rings still
uses a coarse connecting annulus; this is a longer detailed range, not uniformly
dense ground out to 256 metres. Tree LOD distances are unchanged.

All seven terrain tests, eight production material tests and four shader paths
pass. Intent inspection reports zero structural mismatches. The optimized staged
macOS app is signed and completes a 16-frame native WebGPU run with zero remaining
Entities. Its reviewed capture shows the theatre spawn; outdoor cone appearance,
turning/remesh cost and comparative frame time remain unverified. The installed
app has not been replaced. Evidence and the candidate app:
output/checks/forward-terrain-cone/.

## Left browser engraving, November 7

The macOS left catalog now mounts its live category, component and plugin names
in the same three-layer SSLE shader as the right SETTINGS label. Existing browser
layout supplies the baselines, row positions and width limits; category and scroll
changes invalidate retained content. Hover faces, icons and drag targets retain
their existing layout. The bounded label packet has a separate 4096-row dynamic
tail, preserving authored geometry offsets. Twenty focused engraving, browser,
retained-content and shader checks pass; cargo check passes. A signed optimized
app is staged at output/checks/browser-engraving/MirrorPrismals.app. The installed
app remains unchanged, and native appearance review is pending.

## Outdoor theatre approach skirt, November 7

Ramp side faces now end below the lower ramp endpoint instead of at a fixed
theatre-floor height. This removes the raised wall around the lower outdoor
approach from both visible and acoustic geometry. The walking surface is unchanged.
A regression check covers the approach vertices. An independent geometry check
passes at outside heights -100, -5, -0.5 and 0 metres. Rust validation and native
visual review remain blocked by the existing GPU_BASE assertion in
Workstation/PianoScene.rs; the packaged app has not been updated.

## Fuller tree foliage, November 7

Tree twigs now distribute multi-leaf sprays through a larger crown volume in
varied orientations. Nearby individual leaves sit alongside those sprays;
middle and far twig meshes retain 12 and 6 sprays per twig respectively.
The distant forest uses two upright crossed full-crown cards instead of two
horizontal tip fans, with consistent full-quad UVs. Its six triangles per tree
replace the former eight, keeping the existing overall geometry budgets.
Seasonal shedding, stable plant identities and branch damage remain in effect.

All 19 focused vegetation tests pass, including two-axis projected foliage
coverage, seasonal branch preservation, detail reduction and camera-travel
forest budget checks. All six GLSL color/shadow shaders validate. An optimized native WebGPU outdoor
capture completed with zero remaining Entities and was visually inspected at
night. Fine-leaf spray masks use jittered leaf placement and irregular edges,
shared by color/shadow paths and the CPU coverage reference. Daylight appearance
and frame-time comparisons remain unverified. The signed local macOS app is
updated; rollback is output/macOS/MirrorPrismals.before-fuller-foliage-20261107.app.
Evidence is under output/checks/fuller-foliage/.

## Fine loam for exposed soil, November 7

Brown near-ground dirt and steep turf/root-mat faces now select an original
Blender loam material with fine aggregates, grit, small pebbles and fine roots.
Art/Terrain/Dirt/ holds the editable blend, original procedural generator,
1024-square authoring maps and reviewed Blender preview. Two 512-square runtime
maps pack color/AO and normal/height with ten mip levels, adding 2796208 bytes.
The shared native material tail preserves the wood, rock and grass offsets.
The shader uses bounded diffuse normal/cavity shading and eight texel reads;
no triangles or collision changes are added. Grass remains on its existing path.
Fine sand retains its separate appearance. The existing color-based selector and
approximate planet-up normal basis remain, including their slope/lighting limits.
The sharp edge in the supplied screenshot is a mesh/material boundary; this
texture pass does not smooth that outline or change the steep-root-face rule.

All four shader variants validate and eight production SceneAssets tests pass.
Shader regeneration is stable. The optimized build and a signed separately staged
macOS app pass. A native WebGPU run completes 48 scripted movement/digging frames,
closes with zero Entities and reports zero ChucK render errors. Its reviewed
capture shows the theatre spawn, so close-up soil appearance at the user's
screenshot location remains a playtest obligation. No dirt-specific performance
comparison is claimed. Evidence and the candidate app: output/checks/dirt-material/.
The existing installed local app is not replaced by this candidate.

## Physical piano-key alignment and separate cameras, November 7

White-key fronts now have equal heights using the Blender octave's physical
proportions; raised black keys remain narrower. Each bubble matches the height
and center of its corresponding key. Drawing, picking, pitch zoom and keyboard
audition use the same projection; musical pitch and local note timing stay intact.

The piano room uses the existing plain background or game-camera pass-through
instead of a fixed gray backing. Piano roll and timeline retain independent
in-session cameras and pan, initially mountain and riverbank respectively.
Camera composition through bubble film uses a bounded SDR approximation.

The optimized signed local app is updated, with its prior version preserved at
output/macOS/MirrorPrismals.before-key-alignment-20261107.app. Thirty-two focused
layout/editor tests, the independent camera state test and all shader frontend
checks pass. Preview captures were interrupted, and the app was left closed at
after the user closed it; a fresh native visual check remains pending. The evidence
report at output/checks/piano-key-alignment/ also records an existing separate
mobile hidden-piano/transport workflow failure. Phone workflows are unverified.

## More visible transparent bubbles, November 7

The piano room now has broad warm/cool studio reflections and a soft sky
reflection. Bubbles show translucent bodies and iridescent highlights against
the same plain gray background. Their mesh, 450 nm film, Fresnel reflection,
transparent transmission and timing/pitch geometry are unchanged. The timeline
lighting is unchanged. No opaque fill or emissive outline is used.

GL/GLES/Vulkan shader validation and the optimized release build pass. Fresh
native WebGPU captures of the 12-EDO candidate and packaged 19-EDO view each
rendered four notes in four frames without editor errors or remaining Entities.
Both were visually reviewed. The local signed app is updated, with rollback at
output/macOS/MirrorPrismals.before-bubble-visibility-20261107.app. Evidence and
the new screenshot are under output/checks/bubble-visibility/.

## Separate bubble piano room, November 7

Double-click a MIDI clip to enter its own full-window piano room. The four ruby
panels and timeline tapes stay in the arrangement. The room has a plain background,
the authored bubble notes and a new beveled ivory/ebony keyboard with walnut case.
The editable Blender model, GLB and exact native bevel-plane export live under
Art/AudioWorkstation/PianoKeyboard/. Escape, Tab or < TIMELINE returns to tracks.

The local toolbar controls Draw/Paint, snap, BPM, meter, EDO, scale and root.
BPM and meter belong to the selected clip; no new project tempo or audio grid is
introduced. Cardinal's tuning presets and custom EDO values are available.
Physical white keys show tuning-degree colors in microtone mode; raised keys
remain black. Notes carry canonical MIDI anchors plus cents into the built-in
renderer, recording and export. VST3/CLAP per-note tuning adapters are connected;
unsupported instrument tuning rejects explicitly. AudioUnit microtone tuning
and real third-party instrument validation remain outside the verified results.
See Workstation/BUBBLE_PIANO_ROLL.txt for controls and tuning semantics.

The broad workstation check passed 652 tests, 0 failed, 4 existing ignored. Separate
Host lower-field/hidden-panel and empty-room retention checks each passed. The
production PCM check measures the actual 19-EDO synthesized frequency and verifies
that changing one clip's BPM leaves another clip unchanged. Existing timeline
fixtures now account for the drum-rack toolbar reservation. The former nonzero-
tuning rejection test now checks invalid out-of-range tuning; valid tuning is
intentionally accepted by this requested change.

GL, GLES and Vulkan fragment validation pass. The optimized signed macOS app
completed separate native WebGPU 12-EDO and 19-EDO captures: each rendered four
frames, reported no editor error and released all Entities. Both images were
visually reviewed. The installed app also completed its four-panel timeline
capture with the default retained-rendering path enabled. Piano-room geometry bypasses the timeline's retained furniture
cache, including when empty; ordered meter-ledger instrumentation includes the
new opaque-key radiance site. Source intent inspection reports zero active
structural mismatches; this is structural evidence, not behavioral proof.
Native acoustic output, Windows and phone device runs are not claimed.

The local MirrorPrismals.app is updated. Its previous version is preserved as
output/macOS/MirrorPrismals.before-piano-room-20261107.app. Captures, build/check
logs and the installation report are under output/checks/piano-room-port/.

## Input-monitor housing pose, November 7

The workstation traffic lights now use a fixed -37 degree yaw followed by a
-12 degree pitch, approximating Cardinal's side/downward presentation with
real 3D geometry. The runtime exporter rotates positions and normals after
axis conversion and rebuilds the BVH and bounds. The original GLBs and three
lamp palettes remain the source assets. Native appearance awaits visual review.

## Rigged cardinal birds, November 7

All four planet bird appearances now use the author's northern-cardinal model
rather than colored spheres. Art/Birds/Cardinal/Cardinal_Rigged.blend retains an
editable eight-bone skin and perch, flight and calling actions. The original
/Users/mira/Northern_Cardinal_HighPoly.blend is untouched; export.json records
its hash. The game derivative has 5,354 triangles and 4,164 indexed vertices.
Dense microplumage, hidden draft layers and the display branch are omitted;
current crest, wings, tail, bill, eyes and feet remain. Exported base material
colors replace the original Blender surface shading.

BirdSystem retains the four independent original species calls and habitat
behavior. It supplies local planetary up, travel heading, staggered animation
phases, smooth flight transitions and calling activity. AppearanceSystem blends
the armature-baked frames on the presentation thread through the existing typed
Entity/Get boundary. The skin is embedded in the engine binary; no disk reads,
skinning or mesh JSON round trips occur in the audio callback. Calling moves the
bill while a request is active; it does not track individual audio syllables.
All four visual slots deliberately share the cardinal model in this first pass.

Animation topology/finite/loop/orientation checks, seven bird behavior/audio
checks and the six-instance scene composition/cleanup check pass. The four
Blender pose renders were visually inspected. Structural intent inspection has
zero mismatches. A bounded native macOS WebGPU run created four 5,354-triangle
bird Entities, completed 120 frames with zero failed invocations, and removed
all Entities on close. Its exit view is inside the theatre, so that screenshot
is not a visual review of birds outside. The native audio transport reported
underruns; this does not establish glitch-free audio or gameplay performance.
Outdoor appearance, flight feel and iOS device rendering still need play review.
The local output/macOS/MirrorPrismals.app engine, bird contract and runtime
asset are updated and signature verification passes. Restart it to load this
version. Previous affected files are retained under
output/checks/cardinal/previous-app-files/.
Evidence is under output/checks/cardinal/. Blender previews are under
Art/Birds/Cardinal/; Flight_Preview_Slow.gif plays the flight cycle at 4x slow.

## Cardinal drum rack, November 7

The timeline now has a DRUMS gem opening the four-channel, sixteen-step Cardinal
rack. Each row creates a linked MIDI lane, loads a WAV through a modeless macOS
chooser, paints/erases steps, controls mute/volume and drags patterns onto its
lane. Its local tempo and preview use the existing document, producer renderer,
LSFFR mixer and frozen export path. All controls reuse authored gem meshes.
Leaving the workstation stops preview. See Workstation/DRUM_RACK.txt for controls,
API, source references, format limits and verification evidence.

## Blender meadow grass material, November 7

Green near-ground turf now uses an original tileable Blender meadow material
instead of the previous grass color-noise branch. The half-metre tile combines
live green leaves, dry thatch and gaps. Two 256-square runtime maps contain color/
occlusion and normal/height, with nine mip levels and 699056 bytes including
alignment. Existing geometric crowns and terrain/collision meshes remain. The
runtime uses bounded diffuse normal relief and height-based cavity shading, an
approximate dominant-plane frame and distance filtering; it is not displacement,
parallax occlusion or a full PBR grass shader. Roughness remains in the editable
Blender material. Art/Terrain/Grass/ contains the blend, PNGs, preview and generator.

GL, GLES and Vulkan fragment validation and WGSL validation pass. Seven isolated
production SceneAssets tests pass, including existing wood/rock/monitor layout,
normalized mip normals and both grass upload paths. Shader regeneration is stable.
Reproduce with python3 BuildSupport/validate_grass_material.py; logs are under
output/checks/grass-material/. The harness substitutes only the unrelated camera
vector JSON parser. Blender preview is visually reviewed. Full project compilation
is blocked by concurrent workstation changes (the latest check reports missing
panel_motion::State::rack_snapshot); earlier checks also reported drum-rack Default
implementations. No packaged app was replaced. Native appearance, inclined/polar
mapping, walking shimmer and GPU frame cost remain unverified.

## Pause-menu sound settings, November 7

The macOS game pause menu now has a Sound settings section with an ambient
master and individual wind, birds, insects/crickets, flowing-water, ocean and
underwater-ambience sliders. Each runs from mute to 100%; defaults preserve the
existing mix. Reset sound levels restores those defaults. Values save atomically
to sound.plist beside the game's graphics preferences, independently of System
enablement. WORMWOOD_SETTINGS_DIRECTORY also isolates sound preferences for
native checks. Save failures leave the session change applied and display the
failure in the panel.

The game refreshes the last unscaled ChucK routes while paused, so sliders apply
without resuming simulation. Normal spatial routing also uses the saved levels.
The ambient master multiplies the selected category; all four bird buses share
the bird slider. Workstation feedback and document/export gains are independent.
Preference reads, persistence and route scaling stay outside audio callbacks.

The native Cocoa harness checks the real Sound settings and Reset buttons,
slider actions, category mapping, master multiplication, mute, menu reopen and
layout. A fresh process separately checks saved values. The rendered panel was
visually inspected. Three focused ambient regressions and structural intent
inspection pass with zero mismatches. The signed release app completed two
120-frame native WebGPU/JACK runs: output was nonzero at 100% ambient master
and exactly zero at 0%, with no invocation errors or remaining Entities. Both
runs reported zero VM render errors/underrun periods and seven DAW transport
underruns; this does not establish glitch-free performance. The local macOS
app is updated; previous affected files are in previous-app-files/ under the
sound-settings evidence directory. Evidence and the native harness are under
output/checks/sound-settings/ and BuildSupport/macOS/verify_sound_settings.m.

## Continuous ambient audio, November 7

The planet environment now loads the existing authored wind, insect/cricket,
flowing-water, ocean and depths scripts into its embedded ChucK VM once per
session, alongside the four bird buses. Wind retains its authored 12-second
fade-in. Wind and insects follow the listener; flow and surf use nearby accepted
water-source positions. Still lakes and unknown/dry ground do not invent running
water or ocean noise. Depths is audible only while submerged; above-water
ambience is attenuated and low-pass filtered underwater. Standalone DAW mode
does not start these environmental layers. Requests and sound Entities are
owned by the existing environment Audicle and removed at close; script failures
are reported rather than silently losing a layer.

The four focused checks pass: five-script embedded-VM compilation and finite
nonzero PCM on every dedicated bus, dry/submerged source selection, registered
session reuse/cleanup, and no inferred water in unknown terrain. A release build
and app signature verification pass. A native WebGPU/JACK run completed 120
frames, loaded all five ambient sound Entities, produced 264960 native samples
with nonzero output, and closed with zero remaining Entities and zero invocation
errors. ChucK reported zero render errors and VM underrun periods; the DAW
transport reported eight underruns, so this is not a glitch-free performance
claim. Coast/river transitions, underwater listening and final sound balance
still need listening review. Evidence: output/checks/ambient-audio/.

The local output/macOS/MirrorPrismals.app executable, ambient scripts and matching
contracts are updated. Previous affected files are retained under
output/checks/ambient-audio/previous-app-files/. Restart the app to load them.

## Live world behind workstation cameras, November 7

Embedded security cameras now change the observer over the same live player
scene. The DAW no longer reconstructs a scenic planet with empty edits, coarse
water, baked sky colors or remapped depth. AppearanceSystem's shared scene
builder supplies the current terrain/material state, theatre, detailed vegetation,
clouds and native water passes. Native OpenGL, Vulkan and WebGPU draw the world
first and composite the optical workstation after water. Scene assets and optical
rows have separate native storage. Turning passthrough off clears world passes.
The existing riverbank and mountain sites, camera cycle and per-page aim remain.

Camera daylight inherits the navigation time override. Without an override it
uses the same Eastern wall-clock lighting as gameplay. Water continues through
the existing WaterSystem using the camera observer rather than the suspended
player pose. The original player's position is retained for returning to gameplay.
The FOH computer now permits its current 5660-triangle Blender mesh within an
explicit 6000-triangle object limit; the 48000-triangle theatre limit is unchanged.

The full scene overlay is implemented for desktop OpenGL, Vulkan and WebGPU.
iOS OpenGL ES composition remains unintegrated and rejects this mode explicitly;
iOS device validation is deferred. Native GPU timing's primary scene interval
does not measure the later optical overlay; complete-loop timings include it.
The 12 focused camera/theatre checks pass, with zero structural intent
mismatches. Bounded native OpenGL, Vulkan and WebGPU runs complete. A packaged
WebGPU run captures 94 frames, cycles to the unchanged mountain site and retains
midday lighting; river captures contain native water and detailed vegetation.
Validation evidence and the separately staged macOS app are under
output/checks/live-camera-world/. Native desktop checks verify scene submission
and presentation, not device listening or every gameplay interaction.

## Live timeline navigation, November 7

Changed timeline geometry now invalidates the retained foreground immediately,
including clip positions, track handles, ruler ticks and waveform windows.
Previously these rows were absent from the urgent content key, so incremental
capture could keep showing old clips during continuous zoom-bar input. Timeline
changes now use the existing synchronous content refresh; playhead-only changes
retain the live compositor path. This trades capture work during navigation for
current content; native drag smoothness and frame rate remain unverified.

The 10 retained-content checks, zoom-bar regression and four timeline-scene
checks pass. Intent inspection reports zero structural mismatches. This is
source/test evidence, not native visual confirmation.

## Mixer meter housing ends, November 7

Track and master/monitor meter spectrum rectangles now reserve the authored
12 mm top and bottom rims of the 180 mm housing inside the mixer content clip,
with one logical pixel of outer clearance. Previously the 156 mm spectrum was
stretched across the entire content height, clipping both housing ends. This
intentionally shortens the visible spectrum; meter levels and audio are unchanged.
The existing mixer label/scroll test passes, and transformed exported mesh bounds
fit content heights from 2 to 1000 logical pixels. Native appearance and the
installed app remain unverified; this change is in source.

## Terrain detail and stable forest transitions, November 7

Ground diffuse detail now remains eligible through 64 metres and fades to the
untextured material between 24 and 48 metres in OpenGL, Vulkan, WebGPU and the
iOS source shader. This replaces the triangle-wide 18-metre cutoff, including
its unfaded low-frequency mottling. Four-metre radial steps outside the dense
support patch keep the packed world-space UVs usable through that fade region.
The half-metre support rings and continuous terrain/collision field are retained.

Forest cells now sample nested dyadic world sites. Parent representatives remain
in their children instead of being exchanged for unrelated centre samples; the
same individual crown width and two-tier silhouette is used at every forest
level. Enlarged stand-in canopy patches and the cell-centre habitat veto are
retired; each representative uses its own habitat membership. Individual cells
are retained through 192 metres before adaptive aggregation, beyond the existing
160-metre detailed-tree range. Larger hierarchy roots allow the distant budget
to coarsen without deleting nearby individuals. More trees can still become
visible as distant groups refine; silhouettes still switch to branch meshes.
This is reduced popping, not a complete cross-faded forest or every distant tree.

Focused tests cover ground texture past the former cutoff, material projection,
nested parent/child site identities, and four camera poses within the reserved
8192-triangle forest budget with no omitted cells. Release build and staged app signing pass; OpenGL/iOS GLSL validation passes.
Native appearance and frame rate remain unverified: normal startup rejects the
existing theatre computer mesh budget, and a separate fixture omitting theatre
initialization aborts without a renderer diagnostic. The installed app is not
replaced. Evidence and the staged build: output/checks/terrain-lod-continuity/.

## Theatre camera monitoring

The embedded workstation's right Track IO panel has a wide gem labelled
SPEAKERS OFF / SPEAKERS ON. Enabling it switches the existing L/S/FF/R buses
from direct monitoring to the theatre cabinets, enables camera passthrough for
this session, and selects an interior camera near the rear of the auditorium.
Disabling it restores direct monitoring. The control is absent in DAW-only mode
and when the theatre, security-camera System or audio transport is unavailable.

C cycles riverbank, mountain and theatre; WASD pans. Each editor page retains
its own camera and aim. The active feed supplies audio listener position and
orientation, including cabinet distance, directionality, visibility and delay.
An outdoor camera can therefore hear little or no theatre audio. MIDI keyboard
ownership still blocks camera keys, but does not block the speaker gem's camera
selection. If passthrough is later disabled, the last listener pose is retained
until world navigation or an active feed updates it. Camera and routing work
remain outside the audio callback. Export buses are unchanged.

Validation: offline cargo check/build, focused camera/control/scene-submission
and cabinet/transport checks, and structural intent inspection. Live device
listening and native gem/camera appearance remain unverified. Evidence:
output/checks/theatre-camera-monitoring/report.json.

## Track I/O click cancellation after panel easing

Track I/O now tolerates residual panel movement of at most 0.001 logical
pixels per coordinate between samples. The panel integrator approaches its
target asymptotically; the former exact rectangle comparison continued to
cancel pedestrian-signal presses/releases after the panel was visibly settled.
Page changes and larger layout movement still cancel captured/pending clicks.
A 60 Hz close/reopen regression failed before the fix and passes afterward.
All 16 focused Track I/O model, input and scene tests and four registered
workstation arming integration tests pass. The optimized build passes; the local
macOS app executable is updated and its bundle signature verifies. The previous
executable is retained in output/checks/arming-easing-fix/. Native visual
confirmation remains pending.

## Full-view gem quality and motion, November 7

Expanded desktop gems keep their normal cut thickness and face margins. The
full-viewport capture padding no longer disables the retained renderer, avoiding
its coarse fallback when large. Retained sampling interpolates within the same
surface and stretches the interior separately from the bevel bands. Full-detail
capture retains the selected launcher setting. Expansion now uses a finite
300ms smoothstep; the ordinary panel slide/tap integrator is unchanged.

The full-view cover hides the retained editor directly, independent of studio
lighting. Parked controls and pose-only changes do not force a synchronous
rebake at animation endpoints. Large full-detail background captures use128x16
pixel jobs; their optical refresh converges more slowly instead of dispatching
viewport-width stripes. Moving cached imagery remains an explicit approximation
until the new optical image is published.

The optimized build and focused geometry, motion, content, shader, tile coverage,
lighting and mobile regressions pass. Native full-detail WebGPU/Metal captures at
960x600 show the corrected sharpness and thin bevels. In112-frame expansion/return
replays from settled normal layouts, moving-frame medians were16.6/18.7/16.6ms
for left/bottom/right; worst moving frames were33.0/34.3/32.8ms. Each run used one
initial bake, zero synchronous content refreshes and closed with zero Entities.
An approximately1002ms idle right-panel frame remains, so uninterrupted60FPS is
not claimed. The short replay does not prove full background convergence or
long-held-view behavior. Cocoa overlay text, phone and other backends need their
own visual checks. Evidence and the uninstalled app: output/checks/full-gem-quality/.

## Full gem panel views

Panel cycle update, November 12: all four Option/Alt-arrow shortcuts now cycle
normal -> three-quarter extended -> fullscreen -> normal. The 14 fullscreen
checks and 28 panel motion/layout checks pass (overlapping groups); source-tree
intent inspection reports zero structural mismatches. The signed release app's top
fullscreen view completed four native WebGPU frames using bundled assets,
with zero Entities remaining. Its GPU capture was visually reviewed; native
Cocoa text and manual interaction remain separate from that capture. Evidence:
output/panel-cycle-native-top-release. The updated local app is
output/macOS/MirrorPrismals.app.

At native desktop zoom, Option/Alt plus any arrow cycles its physical panel
through normal, three-quarter extended, fullscreen and back to normal. Side
panels extend to 75 percent of viewport width; top and bottom panels extend to
75 percent of viewport height. Press Escape to return directly to normal. Opening the bottom full view selects its existing
Mixer page; the other bottom pages remain available. The top transport uses the same cycle. Neighboring panels park outside the viewport, retaining
open/closed and modest-size preferences for the return. The same parametric gem
cut and shared content/hit bounds follow the changing size. Pointer editing is
cancelled during this transition and the covered timeline is suspended.

The former modest left/bottom Option shortcut is replaced by full-view expansion;
explicit SetPanel expanded requests retain the old size preference. SetPanel
also accepts fullscreen:true/false for all four panels. Mobile navigation
keeps its existing page workflow. Browser plugin GUI embedding is deferred.
Validation, November 7: optimized engine and staged signed macOS app builds pass.
The packaged app also passes a four-frame bottom-panel render using only bundled
assets, with zero Entities remaining; codesign verification passes.
All 24 panel motion/layout regressions, the registered shortcut/visibility/Escape
integration check, and four mobile workflow regressions pass. Intent inspection
reports zero structural mismatches. Four-frame native WebGPU/Metal captures for
left, bottom and right finish with zero Entities remaining and only the selected
gem admitted. Captured geometry fills each view. Native Cocoa labels are not in
GPU captures; computer-use attachment to the isolated app timed out, so complete
on-screen text/interaction appearance remains unverified. Device audio is outside
this check. Evidence and the uninstalled app: output/checks/full-gem-panels/.

Repeatable capture: set WORMWOOD_WORKSTATION_CAPTURE_PANEL=left (or top/bottom/right)
and WORMWOOD_WORKSTATION_CAPTURE_DT=1 with --daw-only WebGPU --frames 4. This
fixture requires standalone finite capture mode and rejects top, interactive,
embedded and benchmark use.

## Outdoor DAW security cameras

The original scenic reconstruction is retired by “Live world behind workstation
cameras” above. Riverbank and mountain poses and per-page controls are retained.
Daylight is no longer quantized for a projected-packet cache; detailed vegetation,
clouds and reflective water now use the normal player scene and native passes.
Standalone DAW mode starts with the PLAIN studio backdrop, including when saved
settings select PASSTHROUGH. This session choice does not overwrite saved settings.
Standalone DAW mode retains its passthrough restriction.

## Crosswalk arming input and playback

Track IO now preserves queued mouse press/release events, so a quick click
between optical frames cycles Dormant -> Tracking -> Armed -> Dormant exactly
once after the existing managed-button delay. After an I/O edit, the spatial
display reads the new document snapshot instead of reusing pre-edit playback
measurements. This fixes the session exit reported as
`source spheres: stale visualization revision` while playback is running.

Four registered Step regressions pass, including consumed in-memory transport
audio and all three submitted crosswalk palettes. Another 40 focused Track IO,
transport-control and spatial-mix checks pass. These checks verify input,
document and graphics-packet behavior; native visual confirmation is pending.

## Plugin discovery folders

The DAW discovers third-party plugins only under the application's USER directory:
USER_VST3, USER_VST2, USER_CLAP and USER_AUDIOUNIT. System and home-library plugin
folders, CLAP_PATH and global Audio Unit enumeration are excluded, restoring the
Cardinal folder policy. Discovery still finishes before native audio activation.

## Track handle parity with Cardinal

The arrangement now uses unlabelled vertical handles:13px faces spanning the
lane height,12px before the lane, with19px pick width. The handle uses the
same optical cut and material as the ruler. The former name/M/S/gain buttons
and their arrangement mix actions are explicitly retired at the author’s
request. Mute/solo remain in right-panel Track IO page one; volume remains in
the bottom panel or enabled track faders. Names remain document data.
Selection, handle reorder, right-click track menus and handle-column scrolling
remain available.101 focused CPU checks pass, including arrangement/geometry,
Track IO, bottom-panel and browser controls, and WGSL shader validation. Native
visual verification is pending.

## In-game graphics settings on macOS

Press Esc, open Graphics settings, choose a value, then click Apply. On
WebGPU, High draws the original volumetric sky at full resolution. Medium and
Low draw it at half and quarter width and height and upsample it behind the
scene. Each frame uses the current weather, light and camera. Water reflections
keep their original cloud pass. Cloud resolution defaults to High.

Fullscreen rendering resolution selects display points or native Retina pixels.
Apply saves this setting for the next game launch. Restart game now closes the
current session normally and opens a new one with the selected resolution.
Terrain edits are session-only. Settings are saved under MirrorPrismals in
Application Support.

## Terrain controls

In Mirror, select the shovel (1) for turf, soil and sand, or the pickaxe (2)
for exposed rock. Left click removes one existing tetrahedral terrain piece within
4.5 m. The world-fixed tiling uses shared corners and triangular faces with small
deterministic vertex variations. Aim selects a piece; it does not stamp a new
shape. Geological surface/cave boundaries trim the pieces. Repeated digs reveal
the adjoining pieces, and display, aim and collision share the same occupancy.
The intact outer surface retains its current geological presentation; this first
pass makes the solid terrain topology implicit and evaluates pieces near a dig,
without allocating or rendering every tetrahedron across the planet.
Right click an exposed cut face to restore that piece. Step clear before filling
a hole at your feet. Cuts last for the session. Axe and bucket retain their strokes.
Aim-oriented pyramid stamps are retired from normal digging. Legacy box/pyramid
edits remain supported. Trees, buildings and other objects are not tiled yet.

Tiling validation: four production tiling checks and two remove/refill checks
pass, covering shared faces, nondegenerate pieces, unique sampled ownership,
planet-scale identity, internal-face removal, aim independence, duplicate digs
and restored collision. Seven isolated geometry checks also pass. Full offline
cargo check passes. Native appearance/gameplay and updated app packaging remain
unverified.

## Panel compositing and preparation before gameplay

Normal desktop motion uses continuous Origami transforms and the retained
WebGPU panel-image compositor. Panel images carry their captured illumination
while moving; settled changes use the existing incremental optical refresh.
Full-detail panel capture, when selected in the launcher, remains enabled.

The previous default canonical 32-pose foreground bank is now an explicit
comparison alternative: WORMWOOD_WORKSTATION_MOTION_INTERACTIVE=bank enables it,
reference selects its uncached exact-pose comparison, and 0 selects ordinary
continuous composition. Its full-packet/resource validation is preserved.
It is no longer selected by default because unknown poses force synchronous
full optical captures, causing roughly190–205ms stalls in the measured960x600
full-detail embedded motion replay. Continuous composition measured67.3FPS,
21.3ms p99 and27.4ms worst in that same finite workload, with one initial bake.

Before starting the planet game on macOS, AudioWorkstationSystem.Prewarm loads
panel assets and retains a hidden prepared WebGPU client, including immutable
GPU data and pipelines. First workstation Prepare takes ownership of that client.
No Entity, Audicle, visible DAW window or pointer capture is created by prewarm.
Fullscreen activation is deferred until the workstation is shown. GLFW lifetime
is shared by the hidden client and game window. WORMWOOD_WORKSTATION_PREWARM=0
retains lazy preparation for measurement. Other graphics backends remain lazy.
Initial optical capture still occurs on entry, and subsequent DAW recreation
retains its existing behavior. Prewarm shifts preparation into startup; it is
not a reduction in cold shader-compilation cost.

Native checks on M1Pro/Metal passed the game-to-DAW-to-game transition and the
packaged app's input-driven reversal/content/light fixture, with zero remaining
Entities. The1920x1200 left-motion replay measured34.3FPS; the fullscreen
all-panel cycle measured48.1FPS. This does not establish steady60FPS. The former
averaged26.5ms GPU work, so remaining motion cost is still GPU-bound. The adapter
cannot separately timestamp meter versus compositor work inside their shared
render pass. One fullscreen attempt failed during audio capture shutdown; its
repeat completed. These empty-document checks do not establish loaded plugin
performance. Evidence: output/checks/panel-motion-preload-20261107.

Mirror launch: separated macOS microphone/speaker devices now use an owned
CoreAudio aggregate created through the supported API. This avoids JACK2's
failing legacy aggregate lookup. OS defaults remain unchanged; shutdown and
failed startup release the session device. A 12-frame native Play Mirror check
passed with microphone capture and speakers, zero remaining Entities, and no
leftover aggregate. Evidence: output/checks/mirror-launch-duplex-20261102.

Audio processing and safety: the DAW now prepares audio on a dedicated worker,
with native-sized blocks, serialized document/plugin ownership and bounded
diagnostics. Initial plugin discovery finishes before native audio activation.
Graphics submission releases audio preparation ownership. VST3
parameter updates retain the final knob value and service restart requests by
their actual flags. The callback still consumes preallocated queues. See the
[current audio safety contract](Audio/PREPARATION_SAFETY.md) and
[native plugin host verification](Audio/Plugins/README.txt) for tests and limits.
The final speaker-only stress check reduced retained queued audio from 2,048
to 256 frames at 48 kHz. One measured DAW underrun remains under heavy graphics
load; hardware round-trip latency and the original editor crash are unverified.

Normal Mirror gameplay has no HUD. The tool/help overlay is shown only with cheats enabled; disabling cheats immediately hides both its text and background.

Desktop game-to-DAW handoff releases cursor capture on the outgoing window before graphics suspension. Window destruction also releases capture on error/exit paths; returning to the game restores its explicit capture mode.

Desktop workstation plugin restoration: Cardinal's VST3, CLAP, Audio Unit and
isolated VST2 backends now have independent local sources/dependencies. Edison
is the upright bottom selected-track insert row, with page/track navigation;
click toggles an editor, drag reorders effects, and Delete removes the selected
insert. Right-click manages plugins/tracks; its plug follows editor visibility. Hosted audio
runs through the existing render-ahead transport. iOS excludes the native host.
Native-plugin export remains unavailable; see
[hosting details](Audio/Plugins/README.txt). Compile checks do not establish
third-party plugin compatibility or audible playback.

# Mirror (Prismals!)

Engine: Wormwood.

Rust engine using the author's Salamander framework. Read [DESIGN.txt](DESIGN.txt)
for the design; this file records implementation and verification status.


## Settings: Theme / Lightbox

On macOS, use the right-panel page arrows to reach Settings, then click the
engraved SETTINGS label. The first Cardinal Settings port opens Theme / Lightbox
with the original orbit-editor interaction and light parameter controls adapted
to the three actual authored area lights. Angle/rotation values are offsets from
the existing studio, colors and aperture sizes are multipliers, and aim uses
studio metres. The default cool fill orbits away from the face of the right ruby to clear its broad white reflection. The panel uses the existing
native UI-only overlay; gem materials and optical shaders are preserved.

Edits preview immediately. Apply saves to
~/Library/Application Support/MirrorPrismals/workstation-settings.json;
WORMWOOD_SETTINGS_PATH can select a separate diagnostic preferences file.
Cancel, X, Escape and workstation teardown discard the draft. Reset lightbox
previews the default rig; Apply is required to save it. Save/load errors remain
visible in the dialog. Lighting changes invalidate the existing illumination
cache before the next resolved appearance. Frequent edits may require costly
optical refreshes; this is not a performance improvement.

This is the lightbox portion, not the complete old Settings port. Theme libraries,
material/color editors, gobos, adding/removing fixtures, Audio, DAW SFX and General
remain outstanding. Existing studio capacity is three area emitters. Seventeen focused settings/lightbox/hit-target tests pass, and the macOS app
build and signature verification pass. The native overlay was rendered offscreen
and visually reviewed. Live opening, glare movement and relaunch persistence
still require verification in the DAW; these are not claimed by the CPU tests.


## Theatre security camera passthrough

In the embedded DAW, Settings → Theme / Lightbox → Backdrop: PASSTHROUGH
shows the live game scene behind the gem panels. Apply saves the preference;
Cancel restores it. DAW-only mode retains its passthrough restriction.
C cycles riverbank, mountain and the theatre interior; A/D pans and W/S tilts
at 84/66 degrees per second with a ±70-degree tilt limit. Timeline, mixer and
piano roll retain separate session views. Modal dialogs, editing gestures,
lost focus and the computer MIDI keyboard block movement.

All cameras now use the complete shared scene and native world depth; the old
material-90 conversion, depth remap and projected-packet cache are retired.
Row 9.z still controls optical backdrop coverage. This change retains the
existing studio lighting inside the gems; it does not add world reflections
or refraction through the glass. Camera selection/aim remains session state.
See the live-world entry above for desktop validation and platform limits.


## macOS JACK launch correction

The CoreAudio startup command uses JACK's default input-channel count when a
capture device is selected. Passing `-i -1` caused the installed JACK2 parser
to treat the negative value as another option and exit before game startup.
Output-only configurations still explicitly request zero input channels.

## Standalone DAW audio startup failures

A cleanly handled audio startup failure now leaves the DAW editing window open
and reports **Audio unavailable** in its title/status. Playback stays unavailable
until audio is started on a later app launch; no SETTINGS/retry UI is added.
Failed native cleanup is still reported as fatal. Existing owned-server shutdown
and external servers are unchanged. Startup errors name the actual
`jack-server.log` path and include its failure reason. Graphics and Cardinal
editing controls are unchanged; actual GUI/audio verification remains manual.

## Cardinal left-panel workflow correction

The left gem now lists **DAW COMPONENTS**, **Audio Track**, **Midi Track** and
**CC Track**, using Cardinal's component names and order. The invented NEW CLIP
and EDIT CLIP controls and their hit routes are removed. Drag a component into
the visible timeline body and release to insert its track; an invalid drop cancels.
The existing cursor label follows the drag without an optical rebake.

Drag empty timeline space to select a time-and-track range, then press
**Command–Shift–M** on macOS (**Control–Shift–M** on Windows) to create empty
MIDI/CC clips in that range. The existing per-kind overlap checks and free-time
seconds grid remain. Command-modified Shift does not trigger panel overview.
Double-click an existing MIDI tape (second unmoved release within0.35s) to open
its piano roll. Enter places notes only inside the open editor. The invented
N-to-fixed-length-clip path and timeline Enter-to-open route are removed.
The explicit OpenPianoRoll API opens an existing ID or selected clip only;
it never creates a track or clip as a side effect.

Source: preserved Cardinal Vst3BrowserSystem.cpp component construction/drop,
DawLaneShortcutSystem.cpp:838–847, MidiLaneSystem.cpp:866–884. This is the existing
three-component subset on the approved gem surface, not a full browser port.
Other component engines, file-library/search/category navigation, original row
highlight/icons and free vertical lane placement remain outside this correction.
The current timeline still uses seconds and local clip clocks rather than a
shared musical tempo map. Native interaction verification is manual.

## Existing right-panel ORIGAMI control

Click and release the engraved **ORIGAMI** label to cycle the original Cardinal
modes: Off, On, Fixed Top. The initial Fixed Top setting preserves the approved
layout; the first click selects Off. The window title reports the original mode
text after a change. All panel geometry, attached controls and picking use that
same mode. Release outside or lose focus to cancel a press.

This connects the existing layout implementation to its placeholder control.
LOAD/SAVE still need session persistence, SETTINGS still lacks its editor, and
the left component browser still differs from Cardinal's drag-and-drop flow;
those are not replaced with guessed actions in this batch. Source references:
DawTransportSystem.cpp (DawOrigamiPanels branch) and DawUiSystem.cpp
(updateOrigamiPanelButtonVisuals) in the preserved Cardinal tree. Build and focused
CPU tests precede manual verification; no GUI or benchmark is launched.

## Existing top transport controls: Cardinal behavior

The four existing top gem models now use the original Cardinal managed-button
press/release behavior. Hold to depress; release inside to activate, or outside
to cancel. Play stays engaged and always starts/continues playback. Stop holds
the current position; Rewind stops, returns to zero and brings the timeline home.
The original 150ms pressure ramp drives actual inward Z travel of the authored
models together with Cardinal's five-pixel leftward full press. The former
screen-only displacement/tint simulation is retired. The retained renderer
preserves original RGB and transforms captured depth to the current physical
pose without an optical rebuild for ordinary presses. Native callbacks buffer
short left-button presses/releases even when both occur between updates.

Record still lacks the audio-capture backend and reports that limitation rather
than pretending to record. The left component browser uses its Cardinal rows;
remaining right/bottom ports are separate work. Non-retained backend fallbacks
trace the same physically moved button geometry. Source
references are Cardinal's ButtonSystem.cpp, DawTransportSystem.cpp,
DawUiSystem.cpp and DawGemMaterialSystem.cpp in the preserved Salamander137 tree.
Compilation and focused CPU/shader tests precede manual app verification; no
GUI automation, autolaunch or benchmark is run for this change.

## Approved macOS workstation renderer

The normal WebGPU workstation now uses the retained gem appearance and plain
background accepted in iteration26. Gem optics, engraving and panel animation
are preserved; the directly visible floor reflection/shadow pass is omitted.
No experimental flags are needed. Open `output/macOS/MirrorPrismals.app` and
choose **DAW only mode** with **WebGPU**, or run
`target/release/wormwood --workstation WebGPU`. The standalone macOS window starts fullscreen at the primary monitor resolution
with panel buttons, drawn FPS and Quit. This replaces the previously approved
960x600 windowed startup at the user’s request. Direct CLI diagnostics can use
WORMWOOD_FULLSCREEN=0 or an explicit WORMWOOD_WORKSTATION_SIZE to stay windowed. Normal launches have no test
watchdog, forced motion or automatic three-minute exit.

The user reported roughly 100 FPS while manually approving this version; final
integration checks cover compilation, shader/input tests and packaging, not a
new performance run. Foreground appearance refresh and semantic edits can still
cost time. Other graphics backends retain their previous paths. Prior preview
artifacts remain available, and graphics optimization is paused at this approved
version. `python3 BuildSupport/macOS/build_app.py` rebuilds the ordinary app.

## User workstation style packages

Explicit --style PATH (or WORMWOOD_STYLE) selects an external GLB/WGSL package;
--check-style PATH validates it without opening a graphics device. Gem remains
the default. User panels can be open, zero-thickness surfaces or solid meshes.
Authors select stretch, contain or snapped target layouts, and optional bounded
WGSL supplies procedural surface color/cutout and the scene background. Thin
unlit models can appear as flat UI. Imported content surfaces also position
controls and their hit targets. Geometry uploads once; pose updates remain small.

Workstation/StyleAuthoring.txt describes the actual profile and its limits.
Art/AudioWorkstation/StyleExamples contains opt-in carved-metal, wood and flat-ui
packages generated by BuildSupport/Workstation/make_style_samples.py. These are
external test packs, not additional built-in styles. No image-texture bindings,
alpha blending, arbitrary GPU displacement, skinning or animation import is
claimed. Shader alpha does not change CPU control hit targets.

The corrected frozen validation passed22 focused tests (12 importer/layout,
5 shader,4 browser and1 engraving), the desktop build, structural checks, and
15 native captures across WebGPU, Vulkan and OpenGL. All captures were visually
reviewed, including portrait, contain, stretch, snap and the gem baseline. The
wood self-shadow pixel regression passes on all tested backends; equivalent
landscape panel pose rows match exactly. Source fixes reconstruct hits on their
triangles, align the center canvas with visible custom bounds and select
contrasting label ink. Evidence: output/checks/user-styles-20261030-103121,
with separate visual-review.json. Only an unrelated plain-text piano-roll
document changed during this run; evidence identifies the frozen binary/assets.
A subsequent shared Render/window.rs edit is outside that validated snapshot.

The examples are authoring demonstrations: pale wood/metal labels remain finer
and weaker than flat UI labels, and contain mode makes a wide model tiny in a
tall side role. Use a quieter content surface or role-specific models. Live
keyboard/gesture operation was attempted through the native UI tool without
observable panel changes, so physical input delivery remains unverified. Panel
fit policies are implemented; desktop window-edge dragging remains disabled
by the existing host. WebGPU/Vulkan need dynamic target/swapchain recreation
before window resizability can safely be enabled.
No distribution packaging, iPhone device build or upload is included in this pass.

## First functional track arrangement

The left gem now creates audio, MIDI and CC tracks and creates/opens selected
MIDI/CC clips. The default canvas displays document track rows with authored
gem handles and tape meshes. Handles select/reorder tracks and expose mute,
solo and gain; tapes select/move/delete clips. Double-click opens the
existing piano roll, Escape returns to tracks, and Tab selects the spatial
mixer. Blank arrangement clicks no longer create notes. The native playhead
and ruler seek share the existing transport; no project BPM was introduced.

The three fixed traffic-light specimens are now opt-in diagnostics. Concurrent
monitor presentation work places Off indicators beside eligible document lanes;
input-monitor routing remains unconnected. This supersedes the fixed-preview
placement reported below. See Workstation/ARRANGEMENT.txt for controls, packet bounds and
remaining DAW work. The isolated final arrangement/piano-roll run passed27 tests. The complete
validation job passed177 workstation tests (one ignored), the desktop build,
OpenGL/iOS/Vulkan shader compilation and a finite WebGPU capture, with zero
structural contract mismatches and no monitored source changes during the job.
The 640x480 capture was visually inspected; native interaction and audible output
were not exercised. A subsequent PianoRoll.rs change is outside this validation
snapshot. Evidence: output/checks/genma-arrangement-20261030-100542-859255.

## Spatial audio spheres

Tab switches from the arrangement into the spatial audio view. During playback,
measured source tracks appear as glossy metallic spheres on the existing plain
background. Source amplitude and effective track gain
both control apparent distance; stereo balance controls horizontal placement,
stereo width stretches the sphere and frequency content controls its height.
Dragging up/back lowers the actual track gain; down/near raises it. The wheel
offers finer gain adjustment. Active gain automation retains control.

Measurements come from already rendered playback blocks and follow the native
consumed-sample clock. The UI does not re-run plugins. Unknown sources have no placeholder spheres;
observed quiet sources retain small selectable handles. This is source-level
visualization, with scheduling/buffering latency, not hardware loopback.
See Workstation/SPATIAL_AUDIO.txt for behavior and limits. The desktop build,
47 focused tests, GL/GLES/Vulkan shader validation and native WebGPU/Vulkan/OpenGL
captures passed. All three captures were visually reviewed for the glossy
spheres, stereo oval and level/frequency placement on the gray backdrop.
Evidence is in output/checks/spatial-audio-20261030-100002, including source
hashes because other workspace features changed during validation. Live
playback timing, mouse feel and phone behavior remain unverified; no app release
is part of this change.

## Compressed rendered states — October 30

A completed offline workstation experiment captures 81 combinations of four
panel extensions and 16 unseen combinations at 640x480. The built-in gem art,
shaders, camera, opening order, empty document and playhead stay fixed. Five
repeated states in a separate native WebGPU process match both pixels and optical
packets exactly. This samples one restricted state family, not the entire UI.

All 97 RGB8 frames reconstruct byte-for-byte from an 8.70 MB shared tile/XOR pack,
including its dictionary, indexes and provenance. Raw pixels occupy 89.40 MB;
independent PNGs occupy 11.45 MB. Median random CPU decode is 2.32 ms for the pack,
3.48 ms for PNGs and 0.86 ms for independent zlib images (13.40 MB). These are
64-request decoder measurements with warm filesystem caches; GPU upload,
presentation and application FPS are not measured. The pack decoder process
peaks at 54.85 MB RSS including Python, NumPy and Pillow.

Training-only PCA models of ranks 4, 8 and 16 predict unseen positions by
interpolating stored coefficients. They visibly ghost panel edges, lettering and
highlights; rank 16 averages 32.17 RGB8 channel levels of error on the 16 holdouts.
This particular approximation fails the unchanged-art requirement. It is not a
lower bound on compressed state representations. The exact cache covers stored
states only; neither approach is enabled in production rendering.

Reproducible capture, codec and dependency-analysis tools are under
BuildSupport/Workstation. Evidence and the exact tool copies are in
output/checks/mita-state-representation-20261030-v2, with results.txt,
representation/summary.json, visual-review.json and frozen source/asset hashes.

## CPU transport and body collision — October 30

Workstation optical frames now pass their 49,408 numeric rows directly through
the registered GraphicsConnectionSystem.SetView route. The JSON adapter remains
available. Both paths enforce finite frame dimensions, prepared Entity ownership,
route permissions and the active Audicle; capture and native uploads are shared.
This removes the repeated JSON allocation, cloning and conversion during motion.

Game body queries use cached collision-only bounds for whole material regions and
ordered blocks of 32 triangles. Collision replacement rebuilds the bounds;
invalid data falls back to the original queries. Candidate contacts keep their
original order and calculations. Art, shaders, optical budgets, solver rates and
ray queries are unchanged by this pass.

Validation completed against isolated macOS release snapshots: 47 focused tests
passed, all 270 authored asset files and 45 shader files were byte-identical, and
deterministic optical packets and native capture pixels matched exactly on WebGPU,
Vulkan and OpenGL. The registered route, Audicle attribution and rejection paths
were exercised alongside contact-response equivalence, refreshed bounds and
degenerate-triangle fallback.

The existing workstation benchmark ran in baseline/candidate/candidate/baseline
order, with 32 measured frames per run at the same 640x480 viewport. Median moving
update wall time improved as follows:

| Renderer | Before | After | Reduction |
| --- | ---: | ---: | ---: |
| WebGPU | 25.20 ms | 14.43 ms | 43% |
| Vulkan | 25.00 ms | 13.61 ms | 46% |
| OpenGL | 23.83 ms | 10.10 ms | 58% |

GPU timestamps remained dominant. Mean total frame time was 385.94 → 373.97 ms
(WebGPU), 286.44 → 295.90 ms (Vulkan), and 360.54 → 370.49 ms (OpenGL), so this
comparison does not establish a consistent rendered-FPS improvement. The isolated
packet microbenchmark measured 12.136 → 0.082 ms with identical frame bits; it
excludes native upload and rendering.

The mining fixture now finishes each accepted dig and pour at its own site before
moving. Its previous 90-frame visits could leave strokes active and create only
two of the requested eight regions. Replay version 2 retains the measured input
sequence and reports extra settling separately. Each of four completed runs used
eight sites plus revisits, water snapshots/index updates and CPU presentation:
1,440 measured frames and 5,280 actual physics frames per run. All physical
diagnostics matched exactly across the four runs after excluding timing fields;
full-versus-culled presentation checks passed.

Mean body-query wall time fell from 1.303 to 0.0255 ms (98%); worst observed queries
were 40.37 and 2.21 ms. Mean GameStep wall time, including activation and idle
samples, was 29.14 → 27.76 ms. Summed measured CPU-stage wall time was 38.42 →
35.35 ms (8% lower), excluding untimed setup and GPU rendering. Solver tail
latency remains: live-step worst samples were 164.90 → 230.41 ms. These are local
wall-time observations, not thread CPU time, game FPS or phone measurements.

BuildSupport/benchmark_runtime_paths.py preserves the frozen sources, separate
builds, original failed-fixture evidence and completed native results. Full
reports, hashes and the aggregation script are in
[output/checks/mita-performance-20261030](output/checks/mita-performance-20261030);
[summary.json](output/checks/mita-performance-20261030/summary.json) contains the
combined results. The transport and collision implementation remains present in
the shared checkout; newer concurrent presentation and tool-suspension features
are outside these frozen comparisons. No application package or phone release
was produced by this pass.

## Rounded Edison socket demo

The Edison asset now has a 56 mm cube housing with 7 mm edge rounds on both
front and back, replacing the earlier 5.5 mm plate. The unequal blade slots,
arched ground opening and mating origin remain aligned with the existing plug.
The plug also has a full 34 mm body depth, replacing the flattened 10.2 mm shell;
its grip recesses, relief boot and cable junction follow the fuller body. The
connected studio preview uses the same socket; its cable descends from the
deeper housing. The editable Blender master, both GLBs and previews are rebuilt.

A tilted socket specimen appears beside the workstation's bottom mixer demos.
It retains the 2,650 authored triangles and materials. Its 340 KB geometry packet
is uploaded once; moving panels update only its pose. The mixer fits uniformly
with the added specimen, preserving meter/fader spacing. The traffic lights are
separate timeline previews and do not belong to this bottom-panel group. This
is a model demo; socket actions and plug-in hosting are not connected. Mac/iOS packaging
includes its runtime assets. Native appearance and mobile interaction remain
for playtesting; no app package or TestFlight upload is part of this revision.
Mac cargo check passed, including the Vulkan shader build. Blender authoring
checks passed for closed geometry, cavity depth and plug clearance. The intent
inspector reports zero structural mismatches; this is not a native visual test.

## Input-monitor traffic-light appearance

The existing Blender traffic lights follow the actual ordered Audio/MIDI rows
in the central timeline's right 57 px gutter. The arrangement shares that
reserve for clip drawing and picking. CC rows keep their spacing without a
light; scrolling shows at most 32 fully visible rows. Each real track displays
the red Off palette as appearance only. Null or empty timelines clear the
signals. No clicks, monitoring state or audio routing are added.

This integration uses Wormwood's 64 px rows, distinct from Cardinal's inherited
60 px and newly added 180 px rows. The old three-state Off/Auto/In preview remains
available only through explicit monitor_preview. The full models stay uniformly
scaled and front-facing; Cardinal's sheared 2D projection is not reproduced.
The monitor group remains separate from bottom-panel mixer and Edison controls.

The native renderer shares all 128,450 authored triangles between instances
and retains the three material palettes, individual LEDs, emission strength and
metal/clearcoat settings. Geometry uploads once at preparation; the dynamic frame
size is unchanged. Bounded direct-light GGX approximates the Blender materials;
full Cycles lighting and control shadows on the cached floor are not reproduced.
Mac and iOS packaging include the small manifest and shared geometry packet.
Native WebGPU, Vulkan and OpenGL captures at 1920x1200 show the signal aligned
with the MIDI lane in the right gutter, with no signal on the CC lane. The
captures were visually reviewed; 55 workstation, 14 arrangement and 3 timeline
tests passed (one explicit CPU benchmark was ignored). Evidence and limits are
in output/checks/input-monitor-20261030-060431/visual-review.json. The validation
job stopped after those checks because shared files changed during the run;
it did not reach Mac packaging and does not certify the entire evolving checkout.
Earlier bottom-panel captures are not placement evidence. Phone appearance and
sustained performance remain unverified.

## Mac Escape pause menu

Planet play opens a native scrollable pause menu on Escape or focus loss on
WebGPU, Vulkan and OpenGL. Resume/Escape restores mouse capture; clicking settings
keeps the simulation paused. The menu includes desktop controls, Cheats/Flight/
Noclip, a cheats-gated time slider, Use real time and Return to title. Settings
apply through the existing navigation controller on resume, including safe noclip
exit. Disabling cheats restores real time. Calendar, climate and seasons keep
their real clock. Native menu input is isolated from workshop/workstation clients.

The menu replaces the Mac click-anywhere-to-resume behavior. Native controls
release held GLFW keys/buttons and restore the game responder when dismissed.
Mac Rust/Objective-C compilation and executable linking passed; the intent
inspector reports zero structural mismatches. Actual menu layout, focus
transitions and gameplay remain for user playtesting.

## Landscape visibility and CPU rendering — October 30

Game vegetation now reaches 6 km instead of 600 m. Cached detailed trees near the
player transition through branch/spray meshes into sparse trunks with forked
branches and separate seasonal leaf fans. Distant hills use broader, overlapping
treetop patches rooted at sampled tree sites. These meshes use the existing
habitat, age, damage and calendar colors. Meadow and water corridors are excluded
from aggregated canopy. The live budget is 16,384 triangles, divided between
detailed trees and distant coverage; adaptation coarsens the distant representation
instead of exhausting the budget on nearby trees. Water clipping has room to
double that packet within existing vegetation buffers.

The live scene now transports numeric geometry through the existing named System
routes, avoiding full JSON vertex serialization and reparsing every frame. Rest
tree meshes are cached by identity/detail/revision/season. Analytic branch wind is
sampled once per tree; leaves retain their curved geometry and follow that bend,
but independent per-leaf flutter is currently frozen at the cached rest pose.
Moving physical trees invalidate their cached shape. Distant tree silhouettes use static geometry.

Terrain gains 128/256-sector outer rings and tighter radial spacing through the
nearby hills, retaining the exact 64-sector support surface underfoot. Beyond
128 m it uses a separate 32 m world-anchor cache, stitched to the moving near
patch. Offscreen batches are rejected before lighting/projection; repeated grass
support samples are reused. Projection constants are computed once per batch,
and reflection clipping uses stack storage rather than per-face allocations.
Vulkan/WebGPU reflection storage now matches the 131,072-triangle scene limit.
The sky, atmosphere, material colors and ground-atlas retirement are unchanged.

Native compilation and the aarch64-apple-ios cargo check passed. The intent
inspector reports zero structural mismatches. Appearance, transition quality,
sustained FPS, movement spikes and memory on Mac/iPhone still need playtesting;
no frame-rate improvement is claimed from compilation or source inspection.
No app packaging or TestFlight upload is part of this change.

## Fullscreen Mac launch

The launcher opens in native macOS fullscreen with its controls centred. The
physics workshop selector also opens fullscreen. All sessions started from the
launcher, including the audio workstation, receive WORMWOOD_FULLSCREEN=1.
Explicit command-line diagnostics can still select windowed rendering.
The updated Objective-C launcher compiled and the local app bundle was re-signed
and passed deep/strict signature verification; no engine rebuild was needed.

## Workstation responsiveness

The next performance pass adds opt-in measurement adapted from Cardinal's
PerformanceTelemetry/DebugHud functionality. The Mac launcher's Performance
window can enable collection for the next session or start a fixed-input
workstation benchmark. Command-line equivalents are `--performance` before a
normal command, or `--benchmark-workstation BACKEND [--frames 8..600]`.
The latter defaults to four warmup frames and 32 measured frames, uses a fixed
panel-motion replay, and stops with an incomplete report after 120 seconds or an
early close. It forces real draws, excludes warmup/idle/sleep from frame samples,
and preserves current resolution and optical quality. Normal sessions retain
their existing input and rendering behavior.

`performance.json` reports actual instrumented thread CPU time, with nested
work counted exclusively, and callback deadline/peak/late/xrun observations.
Uninstrumented worker CPU is outside its coverage. OpenGL/WebGPU use bounded
asynchronous GPU query rings; Vulkan reuses its existing completion wait.
`performance-gpu-BACKEND.json` retains draw-indexed timestamp durations and
explicit unsupported/incomplete results. These intervals are not shader
occupancy or an overlap-unioned GPU utilization measurement. The benchmark
report separates frame wall times from GPU durations and excludes warmup by
draw index even when queries complete later. Audio counters allocate no memory
or files in callbacks; absent callbacks/xrun sources remain unobserved.
Launcher measurements use separate report folders for each run.

The meter-band candidate passed 6,660 intersection comparisons, 31 focused
workstation tests and six telemetry tests, with pixel-identical native captures.
However, the frozen 640x480 WebGPU comparison measured about304ms with the
original BVH and341ms with meter bands. Production retains the original BVH;
the slower candidate remains an opt-in research export. A separate headless
compute experiment measured about176ms with the original BVH, but has small
image differences, does not present a window and is not integrated into the app.
These results do not establish smooth fullscreen performance.

The public benchmark completed on all three desktop backends. Its WebGPU
cross-encoder timestamps underreported GPU work; render-pass begin/end queries
now pass the native runtime check with all eight measured samples complete.
The short run measured268ms mean GPU time and280ms mean frame wall time at640x480;
the newer plugin-page scene prevents a direct comparison with the earlier
fader-scene timings. Vulkan retains its measured
values with raw query diagnostics because a short outlier remains unexplained.
Wall time and GPU measurements stay separate. Evidence, frozen executable/asset
hashes and candidate comparisons: output/checks/workstation-profile-20261030-052838.
Corrected timestamp evidence: output/checks/control-engraving-20261030-060953.

The desktop workstation now retains its last rendered image while panels are
stationary and continues polling input. Geometry changes, captures and window
refresh/restore trigger drawing. Finite diagnostics still force actual frames.
This replaces continuous idle raytracing; other scenes keep continuous rendering.

Control interiors dispatch directly to their known instance/material. Triangle
edges are precomputed and identical flat normals need only one fetch; rejected
BVH boxes skip metadata. The authored meshes, glass settings, engraving and
optical sample/depth/event limits are preserved. All25 focused tests and2,240
control-query comparisons passed. Native WebGPU/Vulkan/OpenGL renders passed;
before/after captures differ in only1–4 color channels by one8-bit level.

The1920x1200 fullscreen WebGPU idle run skipped1,014 unchanged draws and polled
input in a median0.404ms over its final60 ticks. Vulkan/OpenGL idle checks also
passed and all three sessions closed cleanly. Short640x480 warm Pump medians
changed333→294ms WebGPU,241→224ms Vulkan and342→336ms OpenGL. These are CPU
submission/wait timings, not isolated GPU timestamps; OpenGL is asynchronous.
Active animation is still too slow. This revision fixes repeated idle work and
provides a modest shader improvement, not smooth fullscreen animation.

The local Mac app is rebuilt and signed, with deep/strict verification passing.
Evidence: output/checks/workstation-lag-20261030-040930. No phone build or upload.

## Subsurface laser-engraved text example

The workstation has decorative engraved browser text on the left and settings
text on the right. The original Alegreya Sans font, advances, example strings,
logical-pixel sizes and baselines are preserved. A0.008em inward outline inset,
finer fracture points, stable variation and faint displaced depth layers create
a lighter engraved style. Panel layout, glass materials and controls are unchanged.
There are no browser/settings actions or text overlays.

Art/AudioWorkstation/LaserEngraving/GemPanels_engraved.blend contains editable FONT
sources, the packed original font/license,59,586 actual fracture points and
parametric dimension drivers. The original panel blends remain unchanged.
Native glyph poses remain separate from the glass, allowing later text changes
without recutting gems. Printable ASCII is supported; no editing UI is exposed.

Schema3 includes an immutable mip-filtered density atlas, analytically integrated
from the authored point geometry. It replaces repeated per-ray Gaussian/hash
calculations. Native shading samples that field inside the existing refracted
3D paths with glass attenuation and scattered light. This approximates microscopic
geometry/lighting and does not claim Cycles pixel parity.

Blender generation passed six full-mesh containment checks. The focused native
placement test passed across portrait, landscape, overview, swapped sides and
partial/closed panels. All three desktop renderers passed at640x480 and1280x960;
visual review and readback comparisons confirm matching text and controls.
The new text is substantially clearer and thinner; its sparkle and depth wisps
are subtler than the supplied image-generator reference, particularly at1x.
The existing bright studio reflection still covers part of the right specimen.

The verified Mac package contains the regenerated assets and passes code-signature
verification. Evidence: output/checks/laser-engraving-20261030-032714.
No phone build or upload. Normal1x backing remains the default because the
bounded WebGPU comparison measured about322ms/pump at1x versus755ms atRetina.
These are short submission/wait measurements, not isolated GPU timestamps.
Baking the field did not materially improve overall frame time; the existing
optical scene remains slow. WORMWOOD_WORKSTATION_RETINA=1 is an explicit diagnostic
option for examining the finer grain while preserving logical layout.

## Decorative workstation controls

The requested SSLE revision adds separate engraved Blender kits for the four
transport symbols and fader tracks. It replaces the raised dark glyphs and well
liners with actual internal microfracture geometry in the authoring files and
matching mip-filtered density in the native glass renderer. Fader recesses,
slider components, meter assets and outer crystal geometry stay intact. The
five pattern fields share the panel lettering's three-layer fracture style;
the long fader track uses a rectangular density map. Original kits are preserved.
Generation passed six containment checks covering1,617,192 fracture vertices,
plus12 checks on unchanged components. The packed geometry and density hashes,
material mappings and mip tables agree. The asset integration passed55 focused
workstation tests and a native release build. Startup passes with the complete
asset set; the validation harness includes nested startup assets. Native mixer
captures passed visual review on WebGPU, Vulkan and OpenGL, plus Retina WebGPU.
All ten submitted control poses remain visible, with settled panels and clean
closure. Blender and native captures show internal frost; fine grain is clearer
at Retina than1x. Slider caps and recessed rims remain intact.
Blender previews: output/checks/control-engraving-20261030-055643.
The rebuilt local output/macOS/MirrorPrismals.app contains the verified controls,
passes deep/strict signature verification and matches235 packaged runtime hashes.
Package evidence: output/checks/control-engraving-package-20261030-064749.
The previous app is preserved in that evidence directory. No phone build/upload.

`--check-workstation-controls BACKEND` selects the existing mixer page for four
settled frames and captures the final frame. Its report verifies the six actual
meter/fader poses are visible and restores the previous page afterward. Normal
startup is unchanged. The default plugin page hides the faders; right-click the
bottom panel and choose Show mixer controls to display them interactively.
`BuildSupport/Blender/package_control_engraving.py` stages the complete Mac app
and checks these captures before visual review and install.

The top Origami panel carries the authored rewind, stop, record and play models.
The bottom carries three example ivory fader/stereo-meter pairs, meters on the
left as in Cardinal. All components, symbols, caps and fixed meter levels come
from the Blender GLBs. Uniformly fitted groups follow the panel rectangles.
The engraving revision changes appearance only and adds no control/audio wiring.
The native optical renderer intersects their original triangles using per-model
BVHs; opaque pieces use area lighting and glass uses the existing dielectric
transport. Floor shadow caching still covers the four large panels only.
Twenty-one focused workstation checks passed before the group visibility optimization.
The subsequent desktop build and WebGPU/OpenGL/Vulkan captures passed and were
visually reviewed (output/checks/decorative-controls-20261029-233935). The short
640x480 captures reported roughly 0.3–0.8 seconds per drawn frame on WebGPU/Vulkan;
this is substantially heavier than the blank panels. Phone appearance and speed
remain unverified.

The October 30 control-rendering optimization preserves the authored triangles,
normals, materials and optical sampling limits. It adds surface-area BVH splits,
separate transparent-material search trees, a verified convex exit path for the
meter window, and earlier rejection of missed triangles and blocked terminal rays.
All 1,280 deterministic interior-exit comparisons pass against the source triangles.
The latest 640x480 WebGPU and OpenGL captures are pixel-identical to their original
renderers. Short warm CPU pump medians changed from 624 to 303 ms on WebGPU and
523 to 279 ms on OpenGL; these are not sustained frame-rate or GPU timestamp
measurements, and OpenGL queues work asynchronously. Motion remains too slow.
Vulkan's current render passes, but its before/after timing comparison is invalid:
its shader is embedded in the executable, so the asset-root override did not select
the baseline shader. The benchmark now excludes that comparison. Evidence is in
output/checks/control-acceleration-20261030-000718. The Mac app was rebuilt and
signed with these assets and a visible Audio workstation launcher button. No new
phone build or upload was made during this performance pass.

## Mobile workstation panel swipes

The blank 3D workstation accepts swipes across the visible panel faces, in addition
to Cardinal's screen-edge and inner-rim gestures. Swipe outward to close a panel
and inward from its screen edge to reopen it through the Origami slide animation.
Two-finger left/bottom swipes expand; pinch changes overview. Face selection uses
the animated layout, including overview and swapped edges. Twelve focused layout,
motion and geometry checks pass; phone interaction still needs user playtesting.

## Worker and winter workshops

Five additional automatic scenes show a third-person worker digging a layered
ground section, chopping a standing game tree, shoveling snow, and ice fracturing
and melting in warm water or in air. They are available from the desktop and iOS
workshop menus with the existing graphics backend choices. The visible worker
uses the player mesh and two-hand IK with the same finite-mass tools and OGC
material contact as gameplay. The shared hand actuator now has activation,
force/length and force/velocity response, fatigue and timed effort. Its supported
body motor is prescribed; this is not a full dynamic anatomical human.

Snow has compressive plasticity and hardening. Ice has brittle interfaces,
submerged-volume buoyancy and heat/latent-heat accounting. Melted matter retains
its mass as water. Thermal demonstrations explicitly accelerate heating; game
water uses normal time. An authored game air temperature follows season, hour,
elevation and weather-front phase. Snow and ice specimens are currently in the
workshops, without seasonal snow coverage or refreezing in the world.

Implementation and limits: [worker and winter notes](SimulationWorkshop/WORKER_AND_WINTER.txt).
The next construction direction, based on the preserved theatre, is documented
in [construction notes](SimulationWorkshop/CONSTRUCTION.txt): material planks,
load-bearing fasteners, a supported floor, a shack, then a theatre. Construction
has not been implemented in this revision. These scenes require user playtesting;
compilation cannot establish their physical or visual result.

## Held tools and tree cutting

The October 29 held-tool revision replaces the game's detached target-centered
stroke controller. PlayerRigSystem.Plan supplies the current shoulders and bind
lengths before physics; Advance attaches the wrists to the accepted tool without
advancing locomotion a second time. Wrist offsets and hand orientation place the
palms around the handle instead of driving the wrist through its centre.
Tool forces act at two authored handle grips,
with bounded force and wrist torque. OGC solves contact and the arm reach limits
together. Handles/shafts now have collision geometry. Moving away from a lodged
tool is constrained by the two grips rather than a 2.55 m head-distance tether.
Existing out-of-reach errors from changing posture may recover but cannot grow
inside the tool solve; the player is still an IK/capsule character, not a fully
dynamic articulated body.

Carry and work share the same accepted rigid tool; transferring between regions
preserves its pose and velocity. Empty-air carry uses the same hand drive. Tool
paths interpolate rigid poses continuously through approach, work and recovery.
The spade enters edge-first, levers toward the player with the bowl turning up,
and lifts. The axe requests an inward cross-grain chop with its haft toward the
hands. Targets outside anatomical reach are rejected before refinement.

Game trees use the workshop's bonded wood law with finer axial stations around
the first strike height. These begin intact; contact pressure/work must break
them. The crown is supported by all upper fibres rather than one arbitrary
strand. Upper branches remain one rigid assembly; arbitrary later cut heights
still share this initial partition. The previous target-path game controller is
superseded; automatic workshop trajectories remain selected only by workshops.
Native and iOS compilation pass; the intent inspector reports no structural
mismatches. This revision needs user playtesting; compilation is not evidence of successful
shoveling, felling, or on-device performance. It has not been uploaded.

## Material world

The 2026-10-29 refactor replaces the selected atlas-ground renderer and the
atlas-ground-plus-workshop-specimen approach. Landform and cave functions now
provide the **shape of occupied volume**, while `Physics/matter/world_field.rs`
provides its substance: rooted turf, dirt, fine sand, stone, bedrock and water.
The existing trees and their seasonal foliage remain, with material trunk physics.

- Ground is colored material geometry on WebGPU, Vulkan, OpenGL and GLES.
  The ground atlas shader and ground texture selection are removed. Near turf
  uses the same crown/blade builder as deforming workshop turf. Cave crystal,
  ore, moss, lava and lily-pad atlas decorations and old decorative clods are
  removed from this game presentation. Cave voids and drainage shapes remain;
  their enclosing material is stone. Bedrock remains the unexcavatable -96 m floor.
- Tool interaction refines the existing material volume. It no longer calls
  `World::new(workshop_case)` to insert a specimen. Dirt and fine-sand parcels
  sample occupied volume; stone is clipped to the occupied envelope before
  partitioning into matching bonded interfaces. Cave air and already-owned
  material are excluded. Turf has its own structural root mat above cohesive
  dirt and fractured stone, rather than sprouts attached to dirt particles.
- Resting and refined matter share material definitions and colors. Refinement
  transfers ownership only after OGC configuration succeeds; accepted material
  drives visible geometry, player support and water collision. Internal ownership
  walls are suppressed where neighbouring domains overlap. There is no twelve-
  site gameplay quota (a 4096-region storage safety bound remains).
- The old spawned digging debris and TerrainToolSystem surface-height routes
  are removed from navigation. Their source remains an unselected experimental
  alternative for prior fixtures. The current game uses PhysicsSystem.GameStep.
- Fine sand uses the workshop APIC/frictional law; water uses its distinct
  pressure/viscosity law. Existing native water optics display its accepted skin.
  Trees transfer their original geometry into the bonded wood solver, retaining
  the crown and branch contact proxies when the lower trunk fractures. Bark
  atlas sampling remains for those original trees; the shared native storage
  layout keeps legacy atlas bytes for tree/cloud ABI compatibility, not ground.

Desktop controls: **1 shovel, 2 pickaxe, 3 axe, 4 bucket**. Aim within your arms and tool reach,
hold left mouse to charge, release to work, and right click to empty. WASD and
look remain independent. Mobile uses tool selection, hold/release Work, and
Empty / pour, with separate movement/look zones. Accepted physical tool poses
drive hand IK. R resets the session.

The world uses implicit resting material and explicit local refinement, not a
planet-sized particle solve. Boundary sampling is a mesoscopic approximation.
Separate refined regions still lack two-way material coupling; water source
transfers do not yet have regional refill/return-volume feedback. Turf currently
needs a non-overlapping refinement envelope, whereas soil, sand and rock can
exclude previously owned volume. Upper tree branches follow the crown bundle
and do not independently fracture. Material edits remain session-local.

The October 29 mining performance pass replaces whole-column surface activation
with a 0.45 m local layer, keeping deeper material implicit. The turf root mat
and its current soil contact remain physical; underlying stone is instantiated
only when that local section reaches its geological interface. The workshop's
full layered specimen is unchanged. Soil under turf keeps approximately the
previous full-column parcel size (8 cm minimum); direct dirt, sand and water
retain their own resolutions. This does not resolve the regional coupling and
boundary limitations above.

Native OGC contexts now stay with their regions. Conservative surface-pair
candidate caches include fractured/self-colliding surfaces, invalidate on motion
and filter/topology changes, and fall back to complete streaming when bounded
storage fills. Fixed/fixed pairs do no solve work, while fixed faces continue
supporting moving material. Soil neighbor bins and active contact multipliers
avoid repeated hash/allocation work. Solver rates remain four substeps and five
coupled iterations per selected-region frame.

Water collision and visible material skins now travel as numeric batches through
the existing registered System routes, with invocation tracing and validation.
The JSON adapters remain available. Entities share immutable accepted skins;
offscreen regions are rejected before presentation conversion, without culling
their physics or collision. Water retains its indexed terrain and unchanged
spatial-index prefix when debris moves.

The explicit release benchmark is `Host/MiningBenchmark.rs`. Run with
`MIRROR_MINING_LABEL=name MIRROR_MINING_SITES=16 MIRROR_MINING_FRAMES=180
MIRROR_MINING_REVISITS=1 MIRROR_MINING_WATER=1 MIRROR_MINING_RENDER=1 cargo test
--lib --offline --locked --release mining_stress -- --ignored --nocapture`.
Reports are under `output/benchmarks/mining-20261029/`. It drives real GameStep
activation, revisits, mesh transport, body collision, water snapshots/indexing,
and optional CPU terrain presentation. It checks simulation failures and coarse
visibility against full-mesh projection. It does not run GPU presentation or
measure iPhone FPS. The existing scheduler advances the selected region and at
most one awake background region per frame; retained-site counts do not mean
all those regions advance at 60 Hz simultaneously.

Measured on the local Mac in release mode: the first patch's live GameStep mean
fell from 92.50 to 16.93 ms (p95 113.40 to 25.05 ms); activation fell from 169.65
to 53.93 ms. At eight retained sites, mesh transport fell from 17.59 to 0.21 ms
and water collision preparation from 58.28 to 2.54 ms. The final 16-site,
32-visit, 5,760-frame run passed, including targeted-region advancement and
full-versus-culled presentation checks. Its final visit still averaged 30.25 ms
for GameStep and 9.05 ms for CPU terrain presentation; region-entry remesh spikes
remain. This is not a 60 FPS/device-performance claim. Mac release and iOS target
compilation, ten focused Rust regressions and native cache/streaming equivalence
passed. Details and workload limits are in
`output/benchmarks/mining-20261029/RESULTS.txt` and `summary.json`. No app packaging
or TestFlight upload was performed in this performance pass.

The mobile workshop now offers ten **automatic material scenes**: dirt with
a spade, established turf, fine sand, pebbles, wood splitting along the grain,
wood chopping across it, stone with a pickaxe, loose stones with a shovel,
a broad bucket against the same stone, and a layered turf/soil/bedrock section.
These use the material paths under [Physics/matter](Physics/matter), separate from the earlier
square-cell experiment and the game's existing digging alternatives.

The latest source revision adds **Compare forces** on mobile and matching
`_compare` cases on desktop. Light, medium and strong trials use fresh
deterministic specimens, the same requested motion and tool mass, and
45/240/800 N actuator limits. Each runs for 17 simulated seconds. Captions and
reports retain measured contact force, energy/work, breakage and peak lifting
or bucket containment. An actuator setting is not measured impact force.

The former sand model is preserved as `matter_pebbles`. `matter_sand` now uses
fine-sand material parcels, APIC transfer and a pressure-dependent frictional
plastic return map with no tensile cohesion. A continuous skin presents the
bulk deposit. Computational parcels represent many fine grains; this bounded
experiment does not yet provide a whole beach or explicit microscopic grains.

The axe has a curved honed edge, thick splitting cheeks and a transverse haft.
Its swing aims the edge into the wood, holds/rocks the wedge, and withdraws.
Crack-front connectivity persists in material space as the opening moves.
Growth-ring direction changes cleavage resistance; finite contact work advances
the most loaded admissible crack before distributing any excess. Split shape
still follows prepared interfaces. Complete separation in this revision awaits
observation rather than being inferred from the new fracture code.

`matter_layers` couples turf, cohesive soil and brittle bedrock in one OGC
configuration. Root anchors join turf to the soil beneath. The shovel works
the turf, then makes repeated soil scoops and deposits material beside the hole.
A measured clear bedrock patch enables a shovel-resistance stage and a pickaxe
exchange outside the specimen. Stage changes never erase material or force
exposure; insufficient excavation is reported. The sequence takes approximately
one to two simulated minutes. Soil resolution is fixed in this bounded section.
Combined rendering reserves space within the existing mesh budget, using two
blades per crown and a coarser soil skin when necessary; physical matter remains.

This source revision has not been uploaded or observed in gameplay. The preceding
turf/wood revision was uploaded as **2493.6.13** and received user phone feedback.

The structural revision replaces particle/metaball wood and rock with closed,
shape-preserving irregular cells. A capped section of the game's generated
broadleaf trunk provides the wood geometry; elongated cells and anisotropic
interface strength make splitting along fibres easier than cutting across them.
Rock keeps angular fracture faces and conserved piece volume. Interfaces use
traction and fracture work. Estimated contact pressure comes from the actual
tool mesh's projected tributary area; there is no tool-name fracture switch.
Automatic strikes progress from a light tap to stronger blows.

Dirt retains adaptive meshfree samples with cohesive shear-preserving clusters
and compressive plasticity. Only stand-alone dirt refines/coarsens near a tool. Pebbles keep
fixed-size discrete grains and persistent static-friction constraints. Both
material paths now retain a fixed inertial prediction throughout each substep;
the tool drive preserves momentum after applying its finite-force acceleration
limit. The hollow bucket turns its opening into the mound before curling up.
Material is not attached to tools or spawned to fake excavation.

The October 29 turf revision gives `matter_grass` its own deformable volume
mesh, soil cohesion and tensile root network in `Physics/matter/turf.rs`.
It starts established: 384 crowns and 1,536 attached blades over 96 irregular
triangular prisms. Root bundles bridge adjacent tissue at four depths; they
carry tensile load and can be cut by local edge work or torn by overload.
The matrix and roots fail independently. Broken connections never heal, and
each torn root end follows its own fragment. The spade follows a shallow cut,
under-cut, lift and release trajectory through OGC. Foliage follows the deformed
crowns; individual leaf dynamics and biology are not simulated in this case.
The former sparse seed/growth demonstration is retained as `matter_seedlings`
in the desktop chooser, with its reserves, growth and mechanical leaf chains.

Wood's continuous fibre bundles are now projected as stiff pieces with aggregated
mass/inertia. The matrix between bundles retains brittle cohesive connections
with finite critical opening derived from toughness and strength. Failed grain
interfaces permanently split connectivity instead of softening into rubber-like
tethers. The cut log rests on the table, freeing separated fibres from the old
fixed-base fixture. Contact work can propagate a crack along
an existing grain plane; transverse cuts require the local cutting edge and
the higher cross-grain strength/toughness. The axe plane directs fracture work.
`matter_wood_chop` supplies a sideways
axe stroke on the same specimen. The prepared interfaces still limit cut shape.
That preceding revision was uploaded as 2493.6.13. The new changes above follow
the user's phone feedback on that build.

October 29 native checks before the latest sand/force/layer changes: established turf remains intact before contact. Its
13-second cut/lift/release completes with 38 cut and 19 torn root bundles, three
connected pieces and one detached element still lifted at the end; mass is
unchanged and volume differs by under 0.04%. Physics averages 9.4 ms/frame on
this Mac (not displayed FPS or phone performance). The wood split run produces
34 longitudinal interface failures; the transverse stroke produces two actual
cross-grain failures. Both logs remain one connected component, so these results
establish persistent directional cuts, not complete log separation or convincing
detached splinters. That larger separation still needs work. Native and iOS
compilation pass; two focused OGC regressions match parallel contact to serial
results. A one-frame WebGPU capture verifies the initial turf appearance and
mesh budget. Evidence: `output/benchmarks/turf-grain-20261029`.

OGC supplies tool/material and separated-solid contact. Soil's implicit display
surface approximates its coarser contact proxies. Solid fracture currently follows
prepartitioned interfaces, with temporary exclusions while coincident crack faces
open. This is an uncalibrated reduced material model, not continuum FEM or atomic
simulation. Water dynamics remain outside these scenes. The first workshops were
uploaded in **2492.22.23**. Performance build **2493.0.19** was uploaded; the user
then reported fluid-like solids and ineffective tools. This structural revision
was uploaded as **2493.5.14**. The user liked its stone fracture on the phone,
then reported rubbery wood and rejected the sparse grass as a turf model.

October 28 Mac observations (before the turf/stiff-wood revision): the pick separates the rock into three connected
pieces while the broad bucket leaves it intact. The bucket holds five coarse
sand grains during the lift. The grass scene grows 14 plants with three leaves
each. Wood holds its trunk shape and develops visible grain-aligned cracks,
but the final three-strike sequence still leaves one connected trunk; convincing
detached wood splinters remain unfinished. These observations do not establish
phone performance or physical calibration. Captures and reports are under
`output/benchmarks/material-structure-20261028`.

The retained performance implementation provides an explicit mobile backend selector
(WebGPU by default, plus Vulkan and OpenGL), measured FPS, simulation speed,
and CPU physics/mesh/render timings. It removes JSON transport from iterative
contact responses, prunes excluded collision searches, reuses conservative
candidate lists, and uses independent contact-query workers on Apple platforms.
Material contact pairs stream from grouped spatial bins without allocating
the complete pair list. Local refinement is restricted to the approaching tool region.
The physical timestep and five coupled iterations remain unchanged.
Desktop benchmark evidence lives under `output/benchmarks/workshop-20261028`.
On this M1 Pro, the 120-step dirt opening fell from 191.1 to 12.9 ms per
physics frame. Local refinement changed, so this compares the same scenario
and simulated interval rather than identical material trajectories. The full
15-second dirt sequence still averages 40.4 ms per physics frame. Matched
300-frame dirt renders measured 26.5 FPS on WebGPU, 23.6 on Vulkan, and 25.4 on
OpenGL (39–44% of real-time simulation speed). Dense material remains CPU-bound;
the API selector and major overhead reductions do not yet make it real time.
Those measurements precede the structural revision and are not its benchmarks.
Mac timings are not phone timings.
Implementation, limits, retained alternatives and research sources are in
[SimulationWorkshop/MATTER.txt](SimulationWorkshop/MATTER.txt).

The earlier uploaded build **2492.20.8** was the square-cell soil workshop
the user rejected. That retained `soil_spade_fracture` case has 108 elastic
soil cells joined by breakable, co-rotating cohesive interfaces. A tapered spade
uses a finite-force actuator to press, lever, lift, hold and tip. Interface
damage comes from tensile/shear load; detached cells retain mass and contact
surfaces. Nothing is spawned, deleted or attached to the spade during the stroke.
The caption distinguishes the requested stroke phase from measured broken
interfaces, detached cells and cells actually lifted at least 4 cm.
The camera is closer and oblique; the shaft, grip and short turf follow accepted
geometry. Shaft and turf are visual details, not additional contact bodies.

This is a prepartitioned fracture experiment with numerical contact gaps and
uncalibrated material strengths, not arbitrary cutting/remeshing or compaction.
The user observed that version on the phone and rejected its material model.
Build 2492.19.27 before it was the original contact-only version.

The original contact baseline is preserved as `soil_spade_contact` in
Physics/soil_contact_baseline.rs, with its previous regression obligation intact.
It is a separate selectable case, not evidence that the fracture showcase works.
The game's existing digging experiments below remain separate and unfinished.
See [SimulationWorkshop/README.txt](SimulationWorkshop/README.txt) for commands.

The selected `polyhedral_cutting` experiment prepares a bounded, occupied soil
volume before contact. It divides material around the blade without deleting
volume, tracks original material through subdivision, and uses an affine elastic
model with cohesive connections and tension-limited roots. Accepted blade motion
and contact work can damage nearby interfaces. An uncalibrated fracture zone
extends up to 1.5 mm ahead of an edge that is actually moving under load; it
cannot operate on a stationary request. This zone follows the blade's inward
cutting direction under opposing contact force, rather than the direction of
small lateral drift. Extraction and side-only loading do not extend it. OGC handles contact with
the tool and external surfaces; a separate, experimental constraint solve handles
closely spaced soil pieces. Visual cut surfaces follow the damaged area and
grass follows the soil. These laws and parameters are not calibrated.
Native contact now defaults to `internal_contact_mesh: clipped`, triangulating
only the damaged portion and transferring its contact forces and bounds to the
parent soil body. Its vertices are welded within each body.
Accepted crack masks are stored as a disjoint polygon union, with convex pieces
merged only when their union adds no area, keeping stroke history from creating
extra internal contact seams. Gaps, concavities and holes remain intact. The explicit
`full_face` diagnostic exposes an entire internal polygon after any damage;
that can obstruct the blade ahead of the actual crack and is no longer selected.
The separate crossing monitor still checks whole polyhedral faces. Earlier
clipped native digging checks failed after 30 seconds on the Mac, and the
mobile-control check exceeded its 90-second test limit. Both reached the
128-piece limit; selecting the corrected collision geometry alone does not
establish successful digging.
The exposed-face triangulation is now cached until material topology or damage
changes; those timings predate this cache and do not measure its effect.
The cached native check still failed, taking about 27 seconds with 169 rejected
steps; the cached mobile check has not been measured.
A trial that prepared subdivisions from accumulated accepted edge movement was
rejected: it stalled with no cut and 1,155 rejected steps. Subdivision only
prepares topology and does not itself damage or remove material.

This is not yet a working end-to-end digging system. The retained native shovel
test makes a small cut but does not break roots or lift soil. After fixing
rounding at planted and touching vertices, the latest October 28 run rejected
one candidate step but reached the 128-piece limit and spent about 15.9 seconds
in 260 frame advances on the development Mac. The held-mobile-button regression
still stops as `blocked` during insertion. These are test results on the Mac, not phone
performance measurements. Drop and re-dig have not been verified. Focused
material, contact-mask, fracture-zone and failure-rollback tests pass; they do
not establish playability. The iOS Rust target checks successfully; the changes
have not been run on a phone. Evidence is in
[soil-polyhedral-20261028/status.json](output/verification/soil-polyhedral-20261028/status.json).
Grass blades are visual; the player grip is kinematically constrained,
not a dynamic whole-body OGC simulation. No updated phone playtest is claimed.

The retained alternative porous-tetrahedron experiment has a physical sod skin across numerical gaps between
surface elements. Contact, rendering and ground queries use the same skin,
which tears with its supporting cohesive connections. It carries forces through
existing soil nodes; it does not spawn clods or erase earth. Material interfaces
rotate their rest separation with the earth, and distinguish tensile/shear failure
from compression. Strength uses reference face area and compression-dependent
friction. These are explicit, uncalibrated material parameters.

A dig tracks stable material identities, mass and current centroids from the
start of that stroke. Completion requires newly detached earth lifted at least
5 cm; old debris and historical peak lift cannot count. The blade must actually
enter 10 cm before levering. Pressing against the surface without entering now
reports blocked insertion. The actual supplied aiming ray is honored.

Previous porous-tetrahedron verification: **157 library tests passed; 3 failed**. The held-mobile-button test
stops at blocked insertion. Both retained native and manual-player trajectories
fail their root-rupture requirements under the new material/skin representation.
Those obligations remain required. The tests expose the unresolved bulk cutting
model; they have not been weakened or retired. iOS Rust checking and UIKit syntax
checking pass. No phone gameplay or performance result is claimed.
Evidence: output/verification/soil-ground-20261027.

That alternative uses a bounded 48-element porous tetrahedral region. It does not yet
provide adequate fracture/remeshing for a sharp blade to cut through arbitrary
soil, worldwide persistent material, or a dynamic whole-body OGC grip solve.
Increasing to 108 elements did not repair the retained native stroke and was
slower, so that trial was rejected. Passing the new material-law and intact-sod
checks is not evidence that the digging loop works.

The registered runtime now creates and removes Entities, runs a layer-five cue
through preparation and Ready → Go → Finished, and attributes lifecycle, native
resource, and frame records to the active Audicle. Failed preparation/presentation
uses the declared cleanup path and reports cancellation separately. The first
scheduler supports one active cue and one prepared visual Entity at a time.
These are initial implementation limits, not limits of the design.

Verified on October 22, 2026 on Apple M1 Pro: all twenty-one runtime/contract tests
pass. OpenGL, Vulkan, and WebGPU each presented six native frames at 1280×960,
passed GPU pixel readback, and completed the tracked lifecycle and cleanup.
The three readbacks have matching center/background colors. Actual results are
in [output/graphics-verification.json](output/graphics-verification.json).
Windows graphics are also verified independently on the GTX 1050 Ti: OpenGL,
Vulkan, and WebGPU each rendered six 640×480 frames, passed matching GPU pixel
readbacks, and finished Entity/Audicle cleanup. All seventeen runtime tests passed
on Windows. Evidence is in output/windows-native-bf7ea23/windows-graphics.json.
Windows combined cues are now verified through the Scarlett 18i8 USB using
Focusrite ASIO at 48 kHz / 256 frames (5.33 ms per buffer). OpenGL, Vulkan, and
WebGPU each rendered sixty frames with GPU readback, played all 36,000 prepared
samples, reported zero xruns, and finished Audicle/Entity cleanup. All seventeen
Windows tests passed. Evidence is in output/windows-av-e9514cb. This finite
internal-loopback check does not establish acoustic latency or sustained DAW load.

## Build and run

The current native build is set up for macOS with development headers/libraries
for GLFW, Vulkan, wgpu-native, GLM, a macOS SDK/libclang, a C++17 compiler,
and glslangValidator available.
Build-time bindgen generates C ABI declarations from the installed headers.
WORMWOOD_NATIVE_PREFIX selects the header prefix (default /opt/homebrew).
WebGPU has macOS Metal and Windows HWND/D3D12 surface paths; both are verified.

```sh
cargo test --offline
cargo run --offline
cargo run --offline -- --triangle OpenGL --frames 120
cargo run --offline -- --triangle Vulkan --frames 120
cargo run --offline -- --triangle WebGPU --frames 120
```

The default invocation runs dependency/JACK diagnostics without opening graphics
windows. Each triangle invocation selects exactly the named API, prepares while
hidden, presents a bounded number of frames, verifies GPU pixel readback, and
cleans up. Native rendering failures are errors, not backend fallback requests.
The hello cue and its Entity data are in Procedures/Cues/hello_triangle.json. Shaders are in
Procedures/Shaders. Readback images and reports are written under output.

Native library overrides are WORMWOOD_GLFW_LIBRARY, WORMWOOD_VULKAN_LIBRARY,
WORMWOOD_WEBGPU_LIBRARY, and WORMWOOD_JACK_LIBRARY. An explicit override failure
is reported without switching to another API. The WebGPU library remains loaded
for the process lifetime so native worker/callback code remains mapped; cue-owned
GPU resources are explicitly released.

## Structure and limits

BaseSystem/LayerOne through LayerFive contain Systems. Systems/ contains JSON
contracts, Procedures/functionregistry.json catalogs hooks and input/output types,
and Procedures/systems.json declares ordering. BaseSystem/registry.json contains
only enabled/disabled booleans. Circuit-breaker controls are in their own file.
BaseEntity.rs contains data, without World or Level membership.

Host checks compiled signatures, payloads, declared calls, disabled Systems, and
the global halt. Native presentation requires Go. Entity removal is rejected while
its graphics resources remain prepared. Lifecycle records are restricted to their
owning registered hooks. This still does not provide complete compiler-enforced
prevention of every arbitrary native/direct-call bypass.

output/execution.jsonl records parent/child invocations and Entity/Audicle/native
lifecycle events with run IDs. File-backed tracing is diagnostic instrumentation;
it must not be used inside a real-time audio callback. The current runtime is a
finite cue runner, not the completed application/audio scheduler.

JACK now supports a finite combined visual/audio diagnostic through registered
Prepare, Start, Stop, and Release hooks. `--av` adds a sound Entity to the triangle
cue. Preparation activates a silent callback and connects its ports; Go starts a
precomputed, ramped 440 Hz tone at peak amplitude 0.025 for 750 ms. The callback
uses one pass over each supplied buffer and atomic telemetry, without allocation,
locks, file writes, JSON, or Host dispatch. Completion is collected outside it.

Verified October 22, 2026: OpenGL, Vulkan, and WebGPU each presented 60 frames
while JACK delivered 36,000 samples at 48 kHz / 256 frames. An internal JACK
loopback verified signal return. Each run reported zero xruns, completed the
Audicle, removed both Entities, and gracefully stopped its owned server. Twenty-one
unit/contract tests pass; a real missing-library test verified cancellation and
Entity/native cleanup before Ready. Results are in output/av-verification.json.

Run `cargo run --offline -- --av OpenGL --frames 60` (or Vulkan/WebGPU).
After building, `python3 BuildSupport/Audio/verify.py` checks all three paths and
the missing-library failure. It plays three quiet tones. The macOS startup path
uses a separate server named Wormwood, 48 kHz, 256 frames, built-in speakers, and
no capture input. WORMWOOD_JACKD selects the external server executable. An
existing Wormwood server is reused and never stopped; only the spawned child
is owned. This initial device selection is specific to this Mac.

These are finite diagnostics, not a stress test, acoustic output/latency test,
or completed DAW audio engine. Go coordinates audio and visual submissions;
sample-accurate audiovisual synchronization is not implemented. Queue-based
control, sustained scheduling, device changes/recovery, broader device selection,
iOS audio, and ChucK remain unfinished.
No game-specific, terrain, multiplayer, or DAW behavior is implemented.

Wormwood's own code is Apache-2.0; see [LICENSE](LICENSE). Dependencies keep their
own licenses. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and the
[JACK integration document](BuildSupport/Audio/JACK.md).

## Windows source transfer

Run `python3 BuildSupport/Windows/sync_windows.py` to transfer a fingerprinted
source snapshot through the existing `salamander-windows` SSH alias. Each transfer
extracts into a new directory under the Windows user's `WormwoodSync` folder,
checks every source file's SHA-256, and leaves existing checkouts untouched.
Build outputs, Git metadata, and local output are excluded; symlinks are rejected.
Snapshots remain under output/source-packages for reproducibility.

Use `--package-only` to prepare a snapshot without connecting. Optional `--run
build`, `--run test`, or `--run diagnostics` executes Cargo on Windows and retrieves
the log and source-tagged result under output/windows-sync. Cargo uses --locked;
Windows Rust and native development dependencies must already be installed.
This transfer tool does not establish Windows runtime/backend support.

## Windows combined-cue verification

`python3 BuildSupport/Windows/sync_windows.py --run verify` transfers a fresh
source snapshot, installs missing Rust/Clang/GLFW/Vulkan-header/shader tools
through the existing MSYS2 package manager, and retrieves a SHA-256-pinned
wgpu-native v27.0.4.0 GNU SDK into the user's WormwoodTools cache. Setup does not
refresh package databases or request a general system upgrade. Toolchain versions
are recorded in that cache. JACK remains the separately installed JACK2 server
and replaceable client library.

The driver builds/tests through SSH, then uses a temporary interactive scheduled
task to run the three native cue paths on the logged-in desktop. It records the
source fingerprint, session, GPU, audio loopback/xruns, native readback, and
cleanup results. The task is removed afterward. Logs/results return as
windows-results.zip under output/windows-sync. The combined Windows audio/visual
milestone passed on all three backends, as recorded above.

BuildSupport/Windows/environment.ps1 selects the installed SDKs for direct
Windows development. Its current machine profile selects ASIO::Focusrite USB ASIO
at 256 frames and 48 kHz. Override WORMWOOD_JACK_OUTPUT and WORMWOOD_JACK_PERIOD
for an explicit device/period. The 5.33 ms buffer budget is not measured round-trip
latency. Owned Windows servers use JACK synchronous graph scheduling and realtime
priority 92, placing the client at priority 89 in JACK2's MMCSS Pro Audio range.
The default asynchronous mode produced underruns on this machine; the verified
configuration keeps the same audio device, rate, and buffer size.
Windows-owned server shutdown currently uses process termination after closing
the JACK client; macOS uses SIGTERM with a bounded fallback. Sustained scheduling,
recovery and device configuration remain unfinished.

## iOS native host

The iOS target is a UIKit app linked to Wormwood's shared Rust static library.
The app packages the actual registries/contracts and invokes the registered
DiagnosticsSystem.ValidateRuntime hook through the same Host. App resources are
read from the signed bundle; traces and validation reports are written into its
Documents/Wormwood directory. Source fingerprints identify the tested build.

Run `python3 BuildSupport/iOS/build_device.py --team TEAM --device DEVICE` to
build, provision, sign, install, launch, and retrieve on-device Rust validation.
The local output/ios-signing.json supplies defaults on this Mac. Logs and final
verification are in output/ios-device-build. Signing uses the existing development
team; code-sign verification and provisioning-device membership are checked.

Verified October 22, 2026 on the connected iPhone: the app built, passed strict
code-sign verification and provisioning membership checks, installed, launched,
and returned its matching source-fingerprinted Rust validation report and execution
trace. See output/ios-device-build/verification.json for that bootstrap milestone.

Native cue source is now implemented for iOS OpenGL ES 3, Vulkan through MoltenVK,
and WebGPU through wgpu-native/Metal. The two latter paths reuse the shared native
renderers with UIKit CAMetalLayer surfaces. Each selected API has its own resources,
readback, and result; there is no fallback. Signed resources and writable readback
output have distinct paths. UIKit work is marshalled to its main queue while the
registered Rust cue runs on a worker, keeping the app event loop responsive.

IOSAudioConnectionSystem is a separate registered layer-one client of RemoteIO.
Its preparation configures AVAudioSession, observes the actual rate/period,
precomputes the finite ramped tone, and activates a silent callback before Ready.
Go arms the sound. The callback uses raw output buffers, bounded sample processing,
and atomic telemetry; it never dispatches Host calls, allocates, locks, or writes
files. Interruption, route changes, media-service reset, and app deactivation
cancel the diagnostic instead of resuming it implicitly. Native disposal precedes
freeing callback state. The returned IOSAudioReport records produced samples,
output-buffer peak, and callback sample-timestamp discontinuities. RemoteIO has
no JACK-style xrun counter, and output-buffer verification is not acoustic or
hardware-loopback verification.

The device build script downloads SHA-256-pinned official iOS SDKs (wgpu-native
27.0.4.0 and MoltenVK 1.4.2), then builds/signs/deploys and retrieves ios-cues.json
with a matching source fingerprint. Native source syntax and the Rust iOS target
check pass; all twenty-one host/callback tests pass. The native app has linked,
passed code-sign/provisioning verification, installed over the network, and run
on the A18 Pro iPhone. In the first observed run, OpenGL ES presented four frames
and RemoteIO reached Go before presentation was interrupted. The trace confirmed
cancellation and removal of all prepared audio/graphics/Entity resources. This
is interruption-cleanup evidence, not a successful full cue or a pass for all
three APIs. See output/ios-interruption-a3b04c1.

The updated native app temporarily disables automatic screen sleep during its
finite diagnostic and restores the previous setting afterward. Explicit scene
inactivity still cancels presentation. This updated snapshot is linked and waiting
for macOS's protected development-key signing approval; its device results remain
pending. The phone must be unlocked to launch. Once signed and installed,
`python3 BuildSupport/DeviceChecks/verify_ios.py` can retry launch and retrieve
source-matched reports without rebuilding. The verifier checks every frame,
Audicle stage, Entity creation/removal, and audio lifecycle against that run's
trace. It distinguishes native output-buffer verification from acoustic output.

## Paced desktop callback/render verification

`cargo run --offline -- --soak OpenGL --seconds 5 --repeat 2` runs repeated
finite audio/visual cues in one Host/process, using an explicit 16,667 us delay
between submitted frames. Vulkan and WebGPU use the same diagnostic entry point.
This diagnostic pacing is not a display/audio clock synchronization design.
Each cue plays 750 ms of prepared samples, then continues processing silence
while rendering runs. Between cues, the Host verifies that no Entity, active
Audicle, or prepared native resource remains owned.

`python3 BuildSupport/Audio/stress.py --rounds 2 --seconds 5` checks the three
macOS paths and their lifecycle traces. Verified October 22: six cues passed,
with 6.9–7.1 seconds of callbacks per cue, zero active xruns, native GPU readback,
and complete cleanup. Evidence is in output/callback-render-soak/verification.json.

`python3 BuildSupport/Windows/sync_windows.py --run verify --soak-seconds 5
--soak-repeats 2` adds the equivalent paced same-process checks after the normal
native verification. Verified October 22 on the Focusrite/GTX 1050 Ti: all 21
tests, all three normal cues, and two paced cues per API passed. Paced cues ran
5.5–5.8 seconds of callbacks each, with zero active xruns and matching lifecycle
and cleanup traces. Evidence is in output/windows-soak-ae076bd.

These are finite preallocated callback/render checks. They do not establish DAW
synthesis/streaming capacity, acoustic latency, general memory-leak freedom,
physical device unplug/recovery, or complete iOS background/device recovery.

## Data-driven cue composition

CueSystem.Run accepts a ProcedureRef containing a procedure name. Its declared
call to CueProcedureSystem.Load reads Procedures/Cues/<name>.json and validates
schema version 1 before any Entity is created. The definition contains one
required visual Entity specification and one optional sound Entity specification.
Systems execute the same tracked preparation, Ready, Go, completion/cancellation,
and native/Entity cleanup operations. Cue data cannot introduce arbitrary function
calls or bypass the contracts. Registry booleans and circuit-breaker controls
retain their separate responsibilities.

Run `cargo run --offline -- --cue hello_triangle OpenGL --frames 60` to select the
procedure explicitly. Vulkan and WebGPU use the same command with the named API.
The existing --triangle, --av, and --soak commands select hello_triangle; --triangle
suppresses its optional sound. The new command writes the existing av-<API>.json
report, now with mode "cue" and a procedure identifier. The initial native
connections still support a three-vertex vertex-color visual and the prepared
diagnostic sine sound; this change adds composition data, not additional renderers
or sound sources. Procedure loading is bounded to 256 KiB and confined to its cue
directory. Names are restricted to 1–64 ASCII letters, digits, underscores, or hyphens.
Unknown cue/Entity fields and unsupported schema versions are rejected.

System contracts can list entry_steps for explicitly callable Host entry points
with typed input. These are separate from startup init_steps, which require Unit input. The diagnostic command runs startup without launching a cue. All entry points and cross-System routes remain
validated against compiled signatures and the function registry.

Verified on macOS: 24 tests pass, including authored data reaching Entity creation,
rejection before Ready/Go, and caller/path checks. Native combined cues passed
OpenGL, Vulkan, and WebGPU with zero active xruns and preparation-failure cleanup.
Windows and iPhone validation of this snapshot is deferred.

## Process entry and System startup

main.rs remains seven lines. The library entry delegates desktop argument parsing
and process orchestration to Platform/Desktop.rs, outside the System boundary.
Arguments are validated before startup. Every desktop command loads the Host,
runs startup once, and then dispatches its requested operation. The no-argument
command performs startup diagnostics and exits; this remains a finite command
host rather than a completed interactive application.

Procedures/systems.json is only the ordered System startup list. The Host invokes
each enabled System's init_steps in that order. Contracts separately declare
entry_steps for callable operations with typed input. The unused update_steps
and cleanup_steps scheduling fields have been removed; the Host does not use a
universal phase/frame schedule. Normal execution follows registered calls and
callbacks. Cue cleanup continues through the existing contracted cleanup calls.

Startup records its plan, completed hooks, and any failure in output/startup.json
and host_startup trace events. A failure stops subsequent startup hooks. Startup
cannot be attempted twice on the same Host, and running another cue does not
repeat it. Diagnostic file writing now belongs to DiagnosticsSystem rather than
a System-specific switch in the Host.

Mac verification: all 27 tests pass. Tests cover startup-list ordering, separate
entry points, one-time startup across repeated cues, and stopping after failure.
OpenGL, Vulkan, and WebGPU combined cues passed with zero active xruns and native
cleanup, including the actual missing-library failure path. Windows/iPhone
validation of these changes remains deferred.

## Mac live cue mode

`cargo run --offline -- --live hello_triangle OpenGL` starts the usual ordered
startup and enters the registered cue. Select Vulkan or WebGPU explicitly to
use that connection. The presentation System keeps rendering until its native
window closes, with an explicit 16,667 us pacing delay. The prepared sound plays
once and then returns to silence while the visual remains. main does not gain
an application/frame scheduler or replay the System startup list.

Presentation returns its end reason to layer five. Window closure cancels the
Audicle; it is not reported as normal completion. Cancellation releases the audio
client promptly rather than waiting for the whole sound, then releases graphics
and removes both Entities through the existing declared routes. The resulting
report is output/live-<API>.json with stage Cancelled and reason window_closed.
Native failures remain errors. The finite cue/soak paths continue to require
normal completion in their verifiers; a cancelled soak is a failure.

The --close-after-frames N diagnostic option (1..600) requests closure through
GLFW's native close flag after N frames. It is labelled diagnostic_request in the
presentation report, distinct from window_event. `python3
BuildSupport/macOS/verify_live.py` verified this native close/cancellation path
on OpenGL, Vulkan, and WebGPU, including native GPU readback, both Entity removals,
audio release without claiming completion, and owned JACK server shutdown.
Evidence is in output/live-verification/verification.json. The native UI automation
service timed out when attempting human-style window controls, so a physical
window-button click is not claimed as an automated pass. The interrupted initial
test and its owned JACK server were explicitly stopped and recorded as aborted.

All 29 tests pass. They include cancellation before/after the first frame and
subsequent cue reuse. Normal three-API combined cues still pass with zero active
xruns and preparation-failure cleanup. Live mode remains a diagnostic: it retains
file-backed frame tracing, has no general input binding/replay controls, and does
not yet supply sample-scheduled audio cancellation/fades or a production recorder.
Windows/iPhone verification remains deferred.

## Persistent Mac application

`cargo run --offline -- --interactive hello_triangle OpenGL` opens one persistent
window. Select Vulkan or WebGPU explicitly for those paths. The first cue starts
automatically. Space triggers another cue while idle; Escape cancels the active
cue; Q or the native close control ends the application. Held Space does not
retrigger. The prepared visual Entity stays visible between events, and is removed
when the application closes. The diagnostic sound is prepared anew for each cue.
Only one active cue and one persistent visual are supported by this first slice.

ApplicationSystem (layer five) owns the application loop and interprets input.
CueSystem exposes Begin, Advance, Cancel, Status, and Close through contracts;
Begin returns after preparation/Go, and Advance polls an atomic completion flag
through JackConnectionSystem.Complete. It does not wait for the sound duration.
AppearanceSystem.Pump presents one frame of the persistent appearance and obtains
native window input through GraphicsConnectionSystem. Drawing an appearance left
in place is tracked by Entity even when no Audicle is active. The seven-line main
and startup-only systems.json remain unchanged in responsibility.

Preparation, native audio client/server teardown, rendering, and trace file writes
still run synchronously on the application thread. This is not yet a continuously
running DAW audio service or asynchronous asset-loading implementation. The JACK
callback retains its allocation-free, lock-free processing path. Audio cancellation
releases the client; sample-scheduled cancellation fades remain future work.

Run `python3 BuildSupport/macOS/verify_interactive.py` after building for the bounded
native check. `--verify-session` injects labelled application commands to finish,
replay, cancel, replay, finish, and close in one window. It does not synthesize
physical keyboard input. Results and invocation/resource traces are saved under
output/interactive-verification. Windows/iPhone runs remain deferred.

Verified on this Mac on October 22, 2026: all 32 Rust tests pass. The persistent
application check passed independently on OpenGL, Vulkan, and WebGPU with GPU
pixel readback, two completed sounds with zero active JACK xruns, one cancelled
sound, one native graphics preparation/release, and all four Entities removed.
A real missing-JACK-library failure cancelled preparation and released both
Entities and the native graphics session. Evidence is in
[output/interactive-verification/verification.json](output/interactive-verification/verification.json).

## Double-clickable Mac test app

Build with `python3 BuildSupport/macOS/build_app.py`. Open
output/macOS/MirrorPrismals.app in Finder. The native launcher shows the controls and
lets you select OpenGL, Vulkan, or WebGPU and start another session after closing
the previous one. This is a local development bundle using the Mac's installed
JACK, GLFW, MoltenVK, and wgpu-native; external libraries and the JACK server are
not copied into it. It is locally ad hoc signed, not a notarized distribution.

Contracts, registries, cue procedures and shaders are packaged inside the app.
Writable reports, traces, captures and session.log go to
~/Library/Application Support/MirrorPrismals/Diagnostics; the launcher has an Open
diagnostics button. No writes to the app bundle are required. The launcher is
platform UI outside System scope; engine execution still enters the contracted
ApplicationSystem. No terminal command is required for hands-on use.

`python3 BuildSupport/macOS/build_combined.py` collects a fresh combined build,
focused regressions and packaged native checks in a separate evidence directory.
It keeps the existing local app until the new package is reviewed. The older
`verify_app.py` still describes the original cue diagnostic and is not the current
game/workstation build gate.

## macOS output-device changes

Managed Mac JACK connections now select the current CoreAudio default output UID
when each cue prepares, instead of hard-coding the built-in speaker. Each engine
process uses its own owned server name; an old external Wormwood server is not
silently reused. Explicit WORMWOOD_JACK_OUTPUT/WORMWOOD_JACK_SERVER overrides keep
their explicit routing policy.

During an active interactive cue, JackConnectionSystem.RouteChanged queries the
system default at most every 100 ms on the application thread. A changed or missing
default emits audio_route_changed and cancels the current cue through its declared
cleanup route. The triangle/window remains alive. The next Space-triggered cue
selects the latest default, so changing macOS output or disconnecting Bluetooth
does not require restarting the app. The interrupted tone is not seamlessly moved
or resumed; continuous-stream migration/fades are still future work. No CoreAudio
query, allocation or logging was added to the real-time callback.

Output configuration records now include output_device_uid and
follows_system_default. Output policy/disconnection tests and a native default-UID
query pass on this Mac. Physical Bluetooth disconnection still needs a hands-on
check; the packaged native checks do not toggle Bluetooth or change OS settings.

The revised device-following build passes all 34 Rust tests. Packaged native checks
verify that each prepared sound records a nonempty CoreAudio output UID, follows
the system default, and owns its isolated JACK server. Subsequent preparation
errors are recorded as application_audio_error and leave an existing scene alive
for another trigger. Initial startup failure still reports an error to the launcher.


## Shared cube and flight baseline

The Mac launcher now defaults to Cube + player flight and retains Triangle + audio
as a separate test. WASD flies relative to the view, Space/Left Shift moves up/down,
holding right mouse captures the pointer for looking, and releasing it restores
the pointer. R resets the camera; Q or native close ends the session. No gravity,
collision, terrain, DAW or title-specific mechanics are added. This cube test is
silent, so Space controls ascent independently of sound-cue replay.

FlightSystem is a layer-five System. Movement/look/reset activation creates a
tracked Audicle, and returning to idle finishes it. Camera updates use
AppearanceSystem.View and GraphicsConnectionSystem.SetView through declared typed
contracts. main and the System startup list retain their existing responsibilities.
Time-based movement normalizes diagonal input, clamps frame stalls and pitch, and
cleans up the active flight Audicle before releasing the visual Entity.

The original cube diagnostic presents a closed convex mesh using CPU back-face
elimination and GPU perspective clipping. The current material renderer also
supplies depth testing and larger bounded batches, described below. OpenGL/WebGPU use bounded
uniform batches, while Vulkan keeps portable 96-byte push constants and refreshes
its command buffers for a changed view. The underlying APIs remain explicit.

`python3 BuildSupport/macOS/verify_flight.py` checks all three native cube paths,
scripted movement/look/reset, Audicle attribution, and resource cleanup. Evidence
is saved in output/flight-verification. Physical mouse and keyboard use still
needs the user's hands-on check. Project.json supplies independent app identity
and writable diagnostics directory for each game repository.

The shared cube baseline passes 39 Rust tests. All three Mac native cube checks
pass; the triangle/audio bundle checks still pass on all three APIs.


## Current Mirror game and workshop

Mirror/Wormwood and lacunavi dawii/Martin are independent Git repositories and
programs. This repository's launcher contains only Mirror and its own physics
workshop. No changes are synchronized to Martin.

The planet is an Earth-radius radial surface with camera-relative rendering,
persistent parabolic excavation stamps and collision queries against the same
height field. G cuts where the camera aims within 100 metres. Each accepted cut
has radius 2m and depth 0.8m and transfers its nominal removed volume/mass into
eight coarse tetrahedral fragments. Positions, velocities, mass and edits are
saved atomically under the output directory's planet/surface.json. R resets the
camera without erasing excavation. A failed save or placement rolls back the cut.

The initial contact patch is 16m across with a 0.5m collision mesh. Further cuts
are limited to within 2m of the first cut, at most 64 retained fragments and 128
stamps. This is an explicit bounded destruction prototype, not planet-wide soil
simulation. Fragments have distance/volume constraints, gravity and OGC contact;
they are coarse pieces, not granular dirt. The initial material run sleeps when
motion settles; it freezes with a reported halt after its ten-second step budget
or a failed validation. No compaction, shovelling, fracture continuum, building,
caves, overhangs or topology cutting is implemented.

WASD moves with captured mouse-look. Double-Space switches walking/flight;
Space/Shift supplies jump/sprint or flight ascent/descent. G or click excavates;
R resets and Escape pauses. See the current first-person controls section below.
The game remains silent. Audio infrastructure is retained for later DAW work.

The native launcher has a separate Physics Workshop window. All ten published
OGC scenario families can be run with reduced procedural geometry. Two are
explicit garment/body and robot/shirt proxies with original assets missing.
See SimulationWorkshop/README.txt and author_cases.json for the precise scope.
P pauses/resumes, N steps, R resets, Escape closes. The simulation title reports
actual time, contact counts, cost and halts. This is a CPU XPBD material solver
coupled to the retained standalone OGC contact adaptation, not the original
paper's VBD solver or benchmark performance.

OpenGL, Vulkan and WebGPU now have depth buffers and bounded mesh batches up to
8192 triangles. Dynamic mesh changes remain tracked Entity updates under a Go
Audicle, with declared System contracts. No material work enters an audio callback.

```sh
python3 BuildSupport/macOS/build_app.py
cargo run --offline -- --interactive planet_surface OpenGL
cargo run --offline -- --workshop cloth_sphere Vulkan
cargo run --offline -- --simulate cloth_twist --frames 60
python3 BuildSupport/macOS/verify_physics.py
```

Current verification results are recorded in SimulationWorkshop/verification.json
and output/physics-verification. A validation halt remains a failed physics stress
run; restoring the last valid state does not turn it into a successful benchmark.
Windows/iPhone validation of these new physics changes remains deferred.

The current source passes 51 Rust tests, including a contact-collapse regression,
procedural Entity boundaries, excavation mass/save/reload and rejected-cut
rollback. All ten default material fixtures pass sixty physics frames (one
simulated second). Extended probes still halt on cloth intersection at about
3.72s, yarn intersection at 7.32s, and mat cell inversion at 1.91s. Their previous
valid states remain intact. These full stress sequences are unfinished.

The contact wrapper now solves the material target and repulsion together using
the contact Hessian. The mat uses a conforming anisotropic tetrahedral mesh.
Reports identify debug builds, actual solver budgets, commanded/measured twist
progress, target error and rejected-step geometry diagnostics. The CPU workshop
can run much slower than real time. It is not a paper-scale performance result.

The launcher’s Stop session button follows normal System cleanup and save paths.
The packaged workshop was opened, launched and stopped through its native UI.
No Martin source or assets were changed.


## Native iPhone gameplay and remote builds (October 25, 2026)

The iOS host now enters this game's registered ApplicationSystem rather than
running the triangle diagnostics at launch. The native menu explicitly selects
WebGPU, Vulkan or OpenGL ES. Scene batches, camera changes and native resource
ownership follow the same contracts as desktop. UIKit input is adapted in Rust
from Cardinal's original touch design: left drag moves, right drag looks, taps
interact; platform controls supply altitude, roll, reset and title-specific actions.
Input is cleared on interruption. The application suspends simulation/drawing
while inactive and returns to its menu through normal System cleanup.
These controls do not create Audicles or call gameplay hooks from UIKit.

BuildSupport/iOS/build_ios.py builds an optimized arm64 iPhone app, packages the
current Procedures/Systems and licenses, and signs using the selected Apple team.
Each game has an independent ios_bundle_id in Project.json and independent build
numbers/output. Nothing synchronizes the game repositories. No tests run from
this script. Local build and remote TestFlight upload are distinct results.

    python3 BuildSupport/iOS/build_ios.py --team TEAM
    python3 BuildSupport/iOS/build_ios.py --team TEAM --device DEVICE --install --launch
    python3 BuildSupport/iOS/build_ios.py --team TEAM --testflight --upload

The last command needs the paid team signed in to Xcode, distribution signing
available, and a matching App Store Connect app record. Optional authentication
key arguments accept a path, key ID and issuer ID; never place private keys in
this repository or chat. By default Xcode uses its configured Apple account.
An upload succeeds only if export.log and build-report.json confirm it; it does
not imply Apple has finished processing or assigned the build to internal testers.
After initial app-record/internal-group setup, builds can be uploaded from the
home Mac and installed through TestFlight over any Internet connection.

Current gameplay on iPhone, interruption/resume, performance and TestFlight
processing still need a real device check. Compilation is not a gameplay pass.
Diagnostics are available through Files because the app exposes its Documents
folder; per-session errors are inside its bundle-ID subfolder/session-error.txt.

October 25 compiler status: both game libraries pass cargo check for macOS and
aarch64-apple-ios. UIKit, the ES scene adapter, and retained Vulkan/WebGPU API
symbols compile against the iPhone SDK; Dawii's ES manatee shaders compile.
The current iPhone gameplay has not been run or claimed as verified. Optimized
app packaging is a separate build step. On October 25, Apple confirmed the paid
membership through October 25, 2027, and Xcode recognized the Developer Team
8KR5Z7L6F6. This game now has App Store Connect app record 6826952495.
The Apple Distribution certificate is installed and local codesign use is verified.
The first upload attempt compiled successfully but produced a generic archive
without the application. The archive target now installs into /Applications,
and the build script checks the archived app identity and signature before export.
The corrected archive uploaded as version 0.1.0, build 2490.0.10. The owner
installed it through TestFlight and reported OpenGL working, but Vulkan and
WebGPU failed at startup. The Vulkan screenshot reports missing vkCreateInstance.
The Personal Testing internal group contains the owner account only and has
automatic distribution enabled for future uploaded Xcode builds.

## Intended behavior inspection

System contracts can now contain an `intent` object with `schema_version: 1`
and a `behaviors` array. Each behavior declares `id`, `description`, `retention`
(`supported`, `experimental`, or `retired`), `selection`, `implements` (local
named functions), `sources` (relative file paths), and `checks`. Behavior IDs
remain stable when source moves. IDs are qualified by their owning System.

A `declared_calls` check supplies an ID, expectation and qualified `targets`;
inspection compares these with the transitive permitted-call graph. A `behavior`
check supplies an ID, expectation and plain-language verification `procedure`.
Behavior checks are reported as `not_checked`: declarations do not count as proof
of source behavior, and this command does not run tests or invoke an AI reviewer.

```sh
cargo run --offline --locked -- --inspect-intent
cargo run --offline --locked -- --inspect-intent --json
```

Inspection reads all registered System contracts, including disabled Systems,
without startup, native hosts, graphics, audio or output-file writes. It reports
missing source bindings, mismatched declared routes, verification obligations,
unmapped named functions and unmapped source files. Source inventory covers
BaseSystem, Render, Host, Platform, Structures, Procedures/Shaders and root Rust
entry/build files; it is not a whole-repository or third-party asset audit.
Undocumented functions are conservatively retained. Only explicitly retired hooks
outside declared retained routes are pruning-review candidates; declared startup
and entry hooks remain protected even when disabled. Source bindings never
authorize deleting whole files.

Host loading validates the shape and hook references of any supplied intent
records, without requiring source files to be shipped in application bundles.
Existing contracts without intent remain valid and are visible as documentation
gaps. Initial descriptions cover established Entity/Audicle rules, all three
graphics choices and selected game behaviors; coverage is deliberately partial.
The command does not prove implementation equivalence, discover hidden direct
calls, import past test results, or automatically perform source-level AI review.
No gameplay or audio callback behavior changes in this addition.

## Fullscreen and first-person controls

The Mac launcher starts the planet game in native monitor fullscreen on the
selected graphics API. The workshop keeps its own window behavior. Fullscreen
projection uses the actual desktop framebuffer aspect.

Player controls are adapted from the author's Cardinal MouseInputSystem,
UAVSystem and the core walking input/speed rules in WalkModeSystem. Planet
surface collision and radial gravity remain this engine's own operations.
Mouse-look is captured during play, uses 0.1 degrees per pixel, and clamps pitch
to +/-89 degrees around radial up. WASD stays horizontal regardless of pitch;
Q/E no longer banks the camera. Start in creative flight at 5 units/second;
Space/Shift ascends/descends and Ctrl gives 3x flight speed. Double-tap Space
within 0.32 seconds to switch flight/walking. Walking supplies gravity, an
8-unit/second jump, Shift sprint charge and movement-key double-tap full speed.
Tuning and initial mode live in Procedures/player_controls.json. The iOS adapter
uses Walk/Fly mode buttons in place of the retired roll buttons; device validation
is deferred.

Escape pauses and releases the cursor. Click or Escape resumes; focus loss also
pauses. Cmd+Q or the launcher Stop session closes and saves. G or a captured left
click excavates; the click used to resume does not excavate. R resets the player.
The controller and pause are layer-five operations with tracked movement
Audicles; InputMode is a contracted layer-one graphics operation.

This ports the core controls. Advanced Cardinal locomotion, including prone
slides, bunny-hop chaining, wall climbing and vehicle-specific controls, remains
unported. No gameplay tests are claimed for this change.

## Prismals body and ground grid

The planet now uses Cardinal's black 24-by-24 material grid per metre, with
its 0.03-cell line threshold and distance filtering. Green ground and brown
excavations retain their colors. This is a surface pattern, not triangle edges.

The original rigged Prismals player GLB and base-color texture are included in
Procedures/Assets/Player. PlayerModelSystem.Load reads GLB skin/accessor data;
InverseKinematicsSystem supplies Cardinal's cached and smoothed control skeleton;
PlayerRigSystem retargets that pose onto the GLB and skins a separate tracked Entity. AppearanceSystem composes it
with the planet. The body shares navigation's Go event; no competing Audicle is
created by lower layers. Head-dominant triangles are hidden in first person,
with torso, arms and legs retained. Look down to inspect the body. Double-tap
Space to walk and see planted-foot locomotion; flight keeps the body suspended.

The eye height intentionally changes from 2.00 to 1.84 metres to fit this
approximately 1.98-metre model. Existing control bindings remain in effect.
This port covers the core loader, skinning and locomotion IK; Cardinal's
vehicle, prone and held-object poses are outside this first body milestone.

The 24-float triangle connection payload retains three clip-space positions
and three vec4 appearance records. A positive fourth component means RGB;
-1 means green grid, -3 brown grid (xy fixed-surface UV, z illumination), and
-2 textured player (xy GLB UV). Each triangle has one material discriminator.
OpenGL/GLES samples the RGBA texture; Vulkan/WebGPU use a bounded read-only
RGBA storage buffer with equivalent bilinear clamp sampling. Texture ownership
is released before the body Entity. Mesh submission is capped at 16384 triangles.

Compilation and contract inspection are separate from playing the game. This
change has no automated gameplay or device-validation claim; its new behavioral
checks remain pending for manual play. See the asset PROVENANCE.txt for the
original workspace path, unchanged GLB hash and the precise adapted sources.

The player body now uses Cardinal's configured 1.15 scale, procedural skeleton
proportions, cached FABRIK limbs, speed-dependent smoothing, square-root stride
and lift curves, and step-count-based arm phase. Walking begins a new step from
held movement input; releasing input completes the current step. Only forward
input aligns body yaw directly with the camera. Mesh retargeting preserves bind
lengths, leaves feet level, carries forearm rotation into wrists, and lets
unmapped spine/clavicle/toe joints inherit their driven parents. This replaces
the earlier direct-bind solver, separate arm timer and reflected body basis.

Cardinal's head hinge, horizontal camera boom and stride-linked sway are routed
through the same pose result used for first-person body presentation. Aiming
uses that actual eye position. The neutral collision/control eye height remains
1.84 metres. Tuning lives in Procedures/player_body.json. Manual gait-feel and
visual comparison remain pending; compilation is not evidence of matching feel.

## iOS release graphics symbol fix, 2026-10-25

Inspection of the uploaded release archive found no exported Vulkan or WebGPU
entry points. Retaining the function addresses did not preserve their names
through Xcode's executable stripping, so the runtime loaders could not find them.
CMake now explicitly exports this game's retained graphics APIs and uses the
non-global strip style. Desktop loaders and API selection remain unchanged.

The iOS packaging script checks the final app's dynamic exports against every
graphics function requested by its generated loaders before export/upload. This
check rejects missing symbols; it does not establish successful device rendering.
The replacement TestFlight build and on-device Vulkan/WebGPU results are pending.

## Pennsylvania terrain direction, October 25, 2026

[TERRAIN_DESIGN.txt](TERRAIN_DESIGN.txt) records the author's new regional and
physical excavation direction, primary references, unresolved representation
choices and the proposed first tool/material milestone. The existing height-field
digging and coarse fragments remain the playable prototype. No new mechanical
model, generated Pennsylvania landscape or phone build is claimed by this entry.

## Mobile gestures and world interaction, October 25, 2026

Gameplay has no HUD, text, virtual buttons, captions, joystick drawings or staged
clod/cradle. The backend-selection launcher remains separate from gameplay.

Mobile controls follow Cardinal's touch grammar in UIKit points: left drag is
analog movement, right drag is look, and the first 12 points of a camera drag
are a dead zone. Double-tap on the left toggles sprint. The invisible bottom-right
corner jumps; double-tap there toggles flight, and holding it sustains flight
ascent. The invisible top-right corner pauses; tap to resume. Holding the far
right edge while paused returns to the launcher. Backgrounding or changing
orientation clears held touches. Walking is the initial player mode.

Tap the actual shovel mesh within three metres to pick it up. It then follows
the player rather than staying at a fixed patch. Two world grip targets drive
the existing arm IK. Tapping empty space requests the normal jump. Touch aiming
uses the actual screen aspect and leaves the camera stationary. Desktop G/click
can still equip the nearby shovel. R resets the player and shovel.

ColorEmotionSystem is a contracted layer-four presentation System driven by
layer-five interactions. Its appearance is a tracked BaseEntity. The port keeps
the authored vignette radius, pulse, smoothing and full-charge split/spin, with
only the requested palettes: lime/pink for digging intent and orange/green for
pickup. All three graphics paths use the same effect equations and alpha blend.
The color is charge feedback; it is not evidence that terrain was excavated.

Procedures/terrain_session.json defaults persist_excavation to false. An old
surface.json is moved to a uniquely named archived-surface file on first load;
its contents are not deleted. Old holes and debris are not restored and fresh
sessions do not write excavation saves. The landscape is deterministic and
starts unmodified each session.

Equipped shovel strokes now cut the actual terrain by the original blade outline
and its contact clearance. The renderer and native ground collider split their
triangles at that polygon, including the new floor and walls. Hold/release sets
a 4–16 cm cut depth; a tap makes a shallow scoop. The removed prism becomes a
cohesive tetrahedral soil mesh with matching rest volume and density-derived
mass, rather than a pre-spawned fixture. OGC handles blade/soil/ground contact;
XPBD maintains the soil's elastic shape and volume. The 2.3-second stroke lowers,
cuts, lifts, tips and returns the shovel. The rendered tool and hand grips follow
the accepted rigid contact pose, including blocked movement. Cuts remain for the
current session and clear on a fresh session. The local native contact patch follows new digging locations. Soil farther than
3.6 metres from the new patch retains its world geometry, mass and velocity as
dormant state and resumes when near a later patch. The session retains up to
64 scoops without silently discarding old material. Reset clears both current
terrain cuts and tool material.

Soil separation is geometric blade cutting followed by remeshing. It is not a
continuum fracture/yield law, granular flow or plastic compaction simulation. The prior pre-separated-clod/cradle demo was removed from the
planet game at the author's request. Its bounded OGC experiment remains in
Physics/shovel.rs and ShovelSoilSystem for research. It is not described as a
working excavation mechanic. A carried shovel now performs real blade-shaped terrain cuts.

Art/Tools/Shovel.blend is the editable original Blender shovel; its source,
exported appearance/collision shell and provenance remain under Art/Tools and
Procedures/Assets. The player model, IK, Pennsylvania-inspired regional terrain,
vegetation appearance, native graphics batching and optimized Mac packaging are
retained. Native phone behavior and all selected graphics paths still require
hands-on verification after the replacement TestFlight update.

The bounded native regression passes through the production shovel hooks, completes a
stroke, compares the hole height with its blade floor, checks volume/mass and
verifies actual OGC contact and contact-driven soil lift. The checked stroke
removed 0.01249 cubic metres and lifted soil 18.4 cm through contact, with rest
volume matching removed volume within 0.5 percent. This headless evidence
does not verify the phone's presentation or physical touch delivery. The iPhone
launcher has a visible build number; in-game HUD and controls remain absent.

## Direct spade controls test, October 25, 2026

The regular straight spade uses Art/Tools/StraightSpade.obj and make_spade.py.
The original round-point Blender source is preserved as historical art. No
scripted lowering/lifting stroke, charge-selected hole, or staged clod runs in
this selection. Procedures/soil_patch.json enabled=false selects a rigid spade
against immovable terrain so mobile controls can be tested independently.

Desktop G equips a nearby spade and releases a held one. Left-drag translates
the tool; right-drag levers it. Mobile taps the actual spade to equip it. A
stationary right hold of 0.18 seconds followed by drag maneuvers it; a second
right contact supplies levering, twist and reach. A quick two-right-finger tap
releases it. Ordinary right drag remains camera look. These tool gestures are
reserved only while equipped. Movement remains on the left. No HUD is added.

Both hand targets are transformed from the accepted rigid physical pose. The
supporting hand slides along the shaft, and requested motion respects reach
measured from the actual player model. The production-hook/actual-IK native
check passes pickup, carry, manipulation, release, retained free tool and
repickup. Measured carry errors are below one millimetre for both hands. Manual lowering
reaches native terrain contact with both hands reachable; the checked contact
errors are below 0.03 mm. Evidence is in output/verification/spade-controls-2490.4.1.json.
The iOS target check includes the actual Touch.rs implementation. This evidence
does not establish how the gestures feel on the author's phone.

That controls-only release intentionally left rooted soil disabled. The following
section describes the current source selection and its separate acceptance.

## Bounded physical digging, October 26, 2026

The October 27 interaction supersedes the mobile gesture descriptions below.
Right drag always looks. While equipped, Hold to dig and Put down buttons expose
work and release; releasing work cancels the attempt. A green ground ring marks
reachable simulated soil, and an orange ring marks an unavailable target. Work
locks the aimed world point, positions the spade, requests insertion, then levers
and lifts through accepted contact/grasp constraints. Completion of lifting now
requires actual accepted position, rather than elapsed time alone.
Blade contact effort sums vertex force magnitudes; opposing pressures no longer
cancel the contact detector. The net force still supplies directional feedback.
The working stance can lower the pelvis by up to 25 cm. Leg lengths and planted
foot support are retained, with the same lowering in shoulder reach and camera.

The bounded soil domain is prepared at session start, in front of the player,
independently of blade contact. Advance never creates material. This remains one
52 by 72 by 24 cm work site, not planet-wide excavatable soil. Top faces carry
grass and sod color; intact cohesive interior faces are hidden only in the visual
mesh. Collision retains every tetrahedron. The finite-element clearance remains a material approximation. Trials with tighter
clearance or a perturbed second interior plane regressed fracture/lift checks and
were discarded; those alignment artifacts remain unresolved.

ShovelSystem.ConstrainMovement clips attempted player translation at the upper
hand's reach from the accepted physical spade. Tool contact can consequently
restrict walking; putting down the spade removes the restriction. This is a
kinematic locomotion/grasp coupling, not an OGC mass-and-impulse solve of the
entire player body. The previous one-way body-to-tool relationship and mobile
timing/pinch gestures are replaced; desktop manual manipulation remains.
Phone feel, reliable scooping and body contact outside that reach constraint
still require further work and hands-on verification.
Current acceptance is incomplete: 154 library checks pass, but the production
soil/root/lift test fails its retained 15 mm lift requirement (9.85 mm observed).
Its fracture, mass, idle and hand-reach checks pass before that failure. iOS Rust
checking and UIKit syntax checking pass. This candidate has not been uploaded;
evidence is in output/verification/mobile-physical-dig-20261027.

An experimental guided motion can be requested while holding the spade: tap
the right side on mobile or press E on desktop. It lowers the tool until force
is measured on it with the soil patch prepared, then requests levering and
lifting through the existing physical tool and hand-reach constraints. It stops
if contact is not detected, feedback is stale, or blocked motion stalls. Manual
manipulation or release cancels it. The force does not identify which part of
the tool or which surface produced contact. Soil changes only
through contact and material response; this initial motion is not yet validated
as a reliable scoop or tested on a phone.

Procedures/soil_patch.json now enables a 52 × 72 × 24 cm occupied material patch
beneath the working spade before contact. The patch contains 48 cohesive
finite elements and 12 tension-limited root fibres. Static terrain becomes a
physical representation of that same occupied domain; preparing it does not
excavate a hole. No input charge, timer, prerecorded stroke or polygon cutout
creates detached soil. Existing material deforms, cohesive/root constraints
rupture under load, and detached material retains mass and volume constraints.
Every distinct tetrahedron remains collidable. Invalid crossings and inverted
volume candidates cannot commit material failure.

One experimental visible grass tuft follows a top face of a soil tetrahedron,
including if that element separates. Its blades are presentation geometry,
not individually simulated grass or contacts, and do not enter water collision.
The rest of the landscape's visible grass is still separate. This attachment is
not yet verified visually in a playable build.

The full-spade mass/inertia, bounded hand wrench, contact Hessian and conservative
vertex displacement bounds feed one rigid solve. ToolMotion.control_point is
model-space: omitted/null means the model origin, while the player declares the
upper hand grip. Motion limits apply at that reference and reconstruct the body
origin from the same rotation. Optional grasp data supplies the two anatomical
reach ranges and the support hand's shaft interval. The solver enforces those
alongside contact; an infeasible request holds/restricts movement and reports
resistance. Releasing the tool removes the hand joint.

Left drag moves the spade vertically and laterally. Right drag levers about the
upper grip. Option/Alt with left drag controls independent reach and twist.
On iPhone the existing held-right gesture maneuvers the tool and the second
contact supplies rotation, pinch/reach and twist. Vertical motion no longer
silently extends reach. The torso bends within its existing limit, and the
final displayed hands are solved onto the accepted physical grip positions
without stretching bones. Gameplay has no HUD.

Seven focused native checks pass, including both motion-reference invariants,
held/released/blocked joints, both soil trajectories and retained pickup/drop/
repickup. The player check includes two seconds of idle prepared soil and checks
anatomical reach on all 600 frames; latched hand error stays below 7.8 mm.
It breaks four roots, accounts for 28.24 kg of detached material and records
1.88 cm peak lift of a detached element. Detached mass does not mean all of that
material was lifted together. The independent native trajectory breaks three
roots and records 2.33 cm peak lift. Both retain an anchored remainder, accounted
mass/rest volume, physical release and cleanup. Evidence is in
output/verification/rooted-soil; immutable release snapshots live in output/releases.

This is a single coarse material experiment, not planet-wide excavation or a
calibrated soil, root or human-force model. The material and rigid-body constants
are engineering approximations. Physics uses a two-substep frame budget and
reports simulated and discarded wall time explicitly. Native checks establish
these bounded behaviors; compilation does not establish iPhone playability,
gesture feel or device performance.

## Environment audio source completion

The planet application connects the four unchanged original bird scripts to the
embedded ChucK VM, native JACK/RemoteIO output, bird appearances and spatial audio.
Source includes original 35% ray panning, head/distance filtering, finite call
lifecycle, wind-timed perch refresh, callback buffer clearing, retained attachment
ownership and partial-startup cleanup. Worker lifecycle messages use bounded
backpressure rather than silently dropping failures or cancellations.

Acoustic snapshots combine continuous terrain, accepted soil/debris tetrahedra
and closed tapered wood from accepted branches with shared wind-deformed endpoints.
Wood covers up to 128 nearby tree sites in the declared 128-metre query radius;
terrain crossings retain a 5 cm sampling limit. Material intervals use accepted
cells and are unioned without counting overlap twice. Each ray batch retains its
immutable snapshot while fragments move.

These source changes have not been compiled, tested, played or packaged in this
completion pass. Existing release artifacts and upload state are unchanged.

## Connected vegetation and water source

The planet appearance now queries the indexed vegetation geometry and submits its
material and silhouette sunlight passes on OpenGL/GLES, Vulkan and WebGPU. The
older tree crowns are excluded while this selection is enabled. Continuous scene
animation has a navigation-owned event even when audio is disabled. Application
open/close restores and atomically saves schema-2 vegetation growth/damage state.

Water now advances from that scene event, consumes a cached accepted terrain and
soil collision snapshot and submits separate opaque color/depth, reflected scene,
water and composite passes. Reflection redraws clipped terrain/tools and indexed
vegetation. Collision terrain uses 3-metre samples over the local 192-metre patch;
accepted cut boundaries and material faces are retained separately. Missing
coverage remains unknown. The current consumes the existing water strength; moving
feet disturb nearby water. Desktop Y places a trough point (Shift starts a chain),
[ and ] adjust its radius, U changes its material, and Backspace clears the current
chain (Shift clears all). Existing mobile movement/tool gestures are unchanged.
Natural river/lake generation follows the regional drainage sampler described below.
Explicit authored rivers and volumetric sources remain optional procedure data. Physical tree fracture/root coupling and ecological calibration are separate
from the existing plant age/damage query operations.

This source pass has not run gameplay, native render validation, performance
measurements or device tests, and has not packaged or uploaded a release.

## Continuous regional water generation

The terrain and water query paths now share a deterministic drainage network.
A priority flood over 64-metre elevation samples identifies basin spill levels
and acyclic downstream links across the 16.384-kilometre reference region.
Catchment area selects river branches; smooth courses carve continuous channels
and banks into the same height sampler used by appearance and physical contact.
The spawn reference remains exactly zero. Connected depressions produce flat
lake surfaces, and ocean membership requires a connection to the regional outlet
below the current tide level. Lakes have separate still-water current behavior.

The live water mesh clips its shore triangles against accepted bed depth and
channel boundaries at a 1.5-metre presentation spacing; this sampling does not
create block occupancy. GPU micro-displacement fades to zero at the wet boundary.
Planar reflection uses the actual lake/ocean level and is disabled for mixed or
sloping free surfaces that cannot share one plane. Cardinal's water optics and
continuous parcel/current adaptations remain in use; voxel slope IDs, columns
and World/Level storage are not imported.

The regional analysis is finite and approximate. Current excavation/material
snapshots change water occupancy, local depth, shoreline and parcel collision.
This does not implement a transient catchment rainfall, erosion, water-volume or
whole-basin drainage simulation after excavation. Those physical models require
additional development; they are not implied by a generation mesh or shader.

Two isolated production drainage checks pass (minimum spill/acyclic routing and
shared channel/ground/spawn reference). Native gameplay and device performance
have not been exercised in this source-only pass.

## Continuous caves and first Depths source pass

Procedures/caves.json defines the first continuous north-region entrance near
local [12, 2, -8], a descending passage, and three connected chambers. The main
chamber uses Cardinal's Depths spawn [-51, -44, 16] and floor around -66 as
content references. This is a new connected layout rather than a copy of the
old terrain generator. The old -36 water level is recorded as a reference;
this pass does not generate a lava or water lake there.

CaveSystem configures the profile and exposes bounded sampling, support,
local mesh and capsule-contact operations. Scene geometry substitutes a local
continuous volume surface for the old height-only patch. Navigation can select
underground support and resolve walls/ceilings; acoustic material queries and
water collision snapshots receive the same cave definition. Local display
extraction uses 2-metre samples, a 36-by-36-metre horizontal window, a fixed
96-metre vertical span and a 10,000-triangle limit. This is an initial bounded
prototype, with mesh seams, contact accuracy, performance and device behavior
still requiring separately authorized validation.

No erase-volume excavation hook was added. Existing accepted soil/material
experiments remain intact; underground rock cutting, fracture, detached mass
accounting and long-lived tunnel edits remain work for the excavation phase.
The original 24-by-24 texture names and Stone tile reference are recorded in the
profile; the native material/atlas binding has not yet been implemented here.
No build, tests, gameplay execution or uploads were performed in this pass.

October 26 source update: the planet appearance now samples Eastern civil wall
time (US DST, one real second per sky second) once per view. The original
Salamander authored Expanse RGB profiles and directional yellow sunset equations
are preserved in Procedures/Atmosphere/sky_palette.json and generated into GL,
GLES, Vulkan and WebGPU fragment shaders. The sky uses per-pixel local-horizon
rays, with adapted angular sun/moon disks. Terrain haze and lighting, native
vegetation light/shadows, carried objects, player luminance and water optics use
that same sample. Water reflection rays reflect through the actual water plane.
This adaptation does not yet reproduce the original star pass or occlusion-based
godrays. New unit cases are authored for Eastern DST, palette wrapping and solar
orientation. Formatting, JSON/source checks and shader regeneration consistency
are checked; compilation, shader validation, visual parity, phone performance
and gameplay are unverified. No build or upload was started for this update.

October 26 woodland, seasons and cloud source pass:

The Pennsylvania scene now uses Cardinal's actual terrain prototype mappings
from its original atlas: meadow grass top, four conifer grass tops, DirtExternal,
StoneExternal, the DepthStoneBlockTex cave-wall tile, beach sand and seabed
sand. The ground renderer uses Cardinal's terrain color tints and nearest texel
sampling, with distant averaging. Tree bark keeps its previous woodland-atlas
textures. Earlier versions selected the wrong atlas and then guessed stone
slots by name instead of following the terrain prototypes. Its water configuration selects generated inland streams
and lakes without ocean tide or ocean waves. Water and terrain meshes still use
different sampling resolutions, so matching natural stream banks and distant
terrain to the water surface remains unverified in phone gameplay.

The interactive frame loop now includes drawing and simulation time in its
60 Hz pacing budget instead of adding a full 16.7 ms wait after the work. This
removes an unconditional delay; actual iPhone frame times are not measured here.

The selected cave profile now generates its local visible surface with a
3-metre surface net. The previous marching-tetrahedra path remains selectable
and retries at 3 metres if its 2-metre mesh exceeds the triangle limit.
Surface-net samples are fixed in world space across moving cave windows. Solid
patch-boundary vertices near the surface are aligned to the outdoor height;
open entrances are not filled. Vertical movement no longer invalidates cave
or water collision caches solely because its altitude crossed a window.

Initial geology work assigns soil, brown rock and blue-gray rock according to
depth below the local surface. The 22 m transition to blue-gray rock is a first
artistic approximation, not a surveyed geological boundary. The selected cave
profile includes a bounded continuous two-noise organic-cave experiment in the
upper rock and existing authored tunnels and Depths chambers. The organic score
is an approximate field rather than a true signed distance; visual seams,
collision and phone performance need hands-on verification. It is inspired by
the older Salamander 84 noise-cave configuration, not established as an exact
reproduction of the cave version being sought.

The first Pennsylvania habitat/material pass imports thirteen unchanged native
24-pixel tiles from the existing woodland atlas. Continuous ground uses grass,
leaf litter, moss, loam, shale, stone and creek sand/cobbles. Sand deposits are
selected inside drainage beds, with no desert habitat. Meadow/ridge/ravine/stream
conditions vary discovery density while exact root resolution and persisted
individuals keep their identities. The two current broadleaf architectures now
sample oak/hickory bark on curved branches. Distinct evergreen architecture and
species calibration remain future work.

Eastern civil calendar sampling preserves the original northern month-based
season detector. Gradual artistic leaf-out, autumn color and leaf-drop schedules
vary slightly per tree. Winter removes actual leaf geometry from both native
color and shadow submissions; seasonal appearance does not advance plant age.

CloudSystem.Frame is registered and enabled. The original 64-cubed periodic
noise, seven guttered mip levels, independent low/middle/high cloud shape and
scattering equations are adapted to spherical intervals and the shared sky.
Wind uses sampled wall time; cloud color follows daylight, sunset and moonlight.
The first pass composes clouds into the sky and its water reflection. Initial
coverage/convection fields are authored, with a gentle seasonal bias. The
original regional condensate/convection weather solver, rain, half-resolution
HDR cloud filtering and terrain cloud shadows are not ported in this pass.
Cloud/terrain ordering during flight and phone frame cost require visual testing.

Validation uses a metadata-only macOS Rust source check with existing generated
native bindings, twelve isolated source tests, GL/GLES/Vulkan shader-stage
validation, WebGPU module validation through the local Metal adapter, stable
shader regeneration and the actual isolated intent inspector. These checks do
not establish final app linking, iOS native compilation, rendered appearance,
performance, or a full application test-suite result. No app build, archive,
TestFlight upload or gameplay execution was performed. Current evidence is in
output/checks/woodland-seasons-clouds-20261026.

October 26 mobile controls/testing update:

Each play session now starts walking with cheats off. The mobile top-right
pause menu exposes Cheats, Flight and Noclip switches; requests apply on Resume
and remain queued until the engine acknowledges them. Cheats gate the old
flight shortcuts. Normal jumps and double taps cannot enable flight while cheats
are off. Flight has independent rise/descend gestures (bottom-right/bottom-left)
and fast movement; noclip follows the look direction and bypasses both terrain
projection and cave capsule contact. Disabling noclip validates the current
capsule and returns to the last collision-checked position if embedded in rock.
Desktop F8/F9/F10 toggle cheats/flight/noclip respectively. Cheat flags do not save.
With cheats enabled, the mobile pause menu also offers a time-of-day slider and
Use real time. The chosen hour holds the lighting and sky at that time until
restored, cheats are disabled, or the session ends; the actual calendar and
season continue to follow Eastern time. The authored lighting currently reaches
its night level by 7 p.m.; the slider does not alter the normal palette.

A seven-step mobile tutorial starts each play session and teaches move, look, jump, pickup, tool
manipulation, drop and pause/cheats. It advances on relevant gestures, also offers
Next and Skip, and does not save completion across sessions. The overlay disappears
after Done/Skip and can be replayed from pause. Pause controls disappear on Resume.
The spade lesson includes the experimental guided-dig tap and manual cancellation;
the cheats lesson includes the time-of-day slider and return to real time.
This temporary tutorial and pause menu are the user's approved exceptions to the
previous blanket HUD restriction; ordinary gameplay keeps no permanent HUD.

Cave snapshots now survive planet Entity appearance validation and updates so
noclip inspection can actually show underground geometry. iOS retention/export
validation now includes child water/vegetation loader tables, fixing thirteen
previously missing native renderer exports before release packaging.

Preflight: five isolated actual-source controller/touch tests pass, iOS Rust
metadata checking passes with cached native bindings, UIKit syntax checking
passes, and intent inspection reports zero structural mismatches. These do not
establish hands-on controls, tutorial layout or cave gameplay on the phone.
BuildSupport/iOS/release_playtest.py runs library tests before creating the signed
archive and uploading through the existing TestFlight workflow. Release outcome
is recorded separately under output/checks/cheats-mobile-tutorial-20261026 and,
on successful upload, output/releases/<build-number>. Upload does not prove Apple
processing, tester availability or phone gameplay.

October 29 free-time audio workstation foundation:

`WorkstationDataSystem` now exposes source-only document operations for audio
tracks/clips, independent MIDI piano-roll clocks and note-to-note clip snapping.
Canvas placement uses elapsed seconds with no project BPM or musical grid.
Rhythm observations consume the master mix only; the experimental analyzer emits
local steady-pulse regions and leaves meter unknown. It does not warp recordings
or change MIDI clocks. Generation/revision guards reject stale analysis.

All workstation presentation is intended to use 3D models. The source provides
ray/plane control-surface mapping for the existing gem panel assets, with no
placeholder UI. A model loader, workstation appearance, live capture/playback,
master mix/tap and worker scheduling remain to be connected. These are explicit
control-thread hooks with no automatic startup. Details and limits are recorded
in `Workstation/README.txt`. No builds, tests or runtime verification ran for
this source pass.

October 29 blank 3D workstation panel scene:

The four authored gem GLBs now have a dedicated source path through
`WorkstationPanelSystem` and `AudioWorkstationSystem`. The Strip/top, Broad/bottom,
Tall/left and Compact/right panels use Cardinal's edge-slide and toggle-retraction
animation. Their faces remain blank. Common uniform scaling fits the viewport
while preserving the actual cut geometry. Ruby appearance uses authored normals,
IOR and convex-mesh thickness absorption; full per-pixel/multibounce optics remain
future material work. No 2D placeholder UI is added.

Desktop entry is `--workstation BACKEND`, with an optional `--frames N` limit.
The macOS application menu also exposes Audio workstation. Arrow keys toggle
individual edges; Space toggles all; Escape closes. Both packagers include only
the four runtime GLBs and manifest. An iOS launch/gesture adapter remains unwired.
Details are in `Workstation/PanelScene.txt`. This pass included source/contract
inspection and formatting only: no builds, tests, native rendering or uploads.

October 29 iOS workstation workshop source update:

The iOS workshop menu now offers Audio workstation · 3D panels. Its native
launcher starts the same blank 3D scene on the selected graphics backend. Swipe
down/up/right/left to toggle top/bottom/left/right; a two-finger double-tap exits.
The workshop controls overlay is hidden in this scene. The four panel GLBs are
packaged with the app. Native touch behavior and actual appearance remain subject
to phone verification. The dedicated release_workstation_panels.py checks sources
and builds/uploads through the existing TestFlight signing workflow.

October 29 workstation panel convexity fix: phone build 2493.22.9 rejected the
Strip GLB at preparation because its imported face normals deviate from exact
planes by up to 0.018 mm, exceeding the 0.001 mm check. All four authored GLBs
exhibit small discrepancies. Validation and optical intersection now use a
scale-relative tolerance of 0.005% of the longest dimension. A focused test loads
the real four GLBs, validates them and generates the portrait scene; this test
needs execution before the replacement build is claimed successful.

October 29 workstation iOS pause fix: the previous panel demo called the
gameplay InputMode hook with pointer=true, which on iOS invokes the game pause
menu. Workstation launch no longer calls that switch on iOS, and the native
controller ignores delayed game pause/control callbacks while the workstation
is active. Desktop retains its visible pointer. Phone behavior remains to be
verified in a new build.

October 29 Origami and Blender-material revision (supersedes the initial panel demo):

The fixed four-model composition and CPU vertex-color material have been replaced
in source. `Workstation/PanelLayout.rs` ports Cardinal Origami opening-order corner
ownership, adaptive rectangles, fixed-top default, expansion and overview. A
matching parametric Blender asset and runtime cut regenerate the 26 facets at new
dimensions. Dedicated native shader variants trace the dielectric/volume material
with the exported studio and Blender's actual AgX57 display transform. Desktop
and phone controls now reach layout changes; mobile edge/rim gesture bounds use
the same rectangles as rendering. Details and explicit integrator limitations
are in `Workstation/PanelScene.txt` and `Procedures/Shaders/workstation_abi.txt`.

Isolated source geometry/motion/gesture cases, Blender deformation checks, actual
OCIO comparisons and shader frontend validation passed during implementation.
Integrated validation now passes 20 focused tests, the native build, iOS Rust type
checking, structural intent checks and 120-frame launches on all three desktop
backends. Matched native/Cycles captures were reviewed with Cycles filtering and
clamping explicitly disabled. Origami silhouettes, area-light placement and ruby
transport agree; finite-sample facet and approximate floor-light differences
remain. This does not claim pixel equivalence with Cycles.

Shadow refraction, strongest-ray scheduling, a shared floor-visibility field and
compact ray state reduce rendering cost. Apple workstation surfaces use one pixel
per logical point while preserving panel/gesture dimensions. At 640-by-480,
Vulkan/OpenGL median frame submission/wait times are approximately 43/58 ms.
WebGPU now waits for each workstation frame to prevent seconds of queued stale
input. Its separate 120-frame latency check passed: median and final frame times
were both approximately 60 ms, without the previous multi-second queue drain. Phone
performance and touch interactions remain unverified. This revision has not been
uploaded to TestFlight.

## Floating bubble piano roll — October 30

The central open-air field now has a clip-local bubble-note editor connected to
WorkstationDataSystem through revision-checked edits. It ports single-note drawing,
painting, movement, resizing, erasing, keyboard edits, pan/zoom and bounded local
undo. The pitch/time grid stays invisible and local snap defaults off. Existing
gem panels surround the field. The authored bubble mesh has duration-aware rounded
ends and placement deformation; texture-free uniform thin-film optics replaces
the Blender procedural film material. Details and controls: Workstation/BUBBLE_PIANO_ROLL.txt.

The authored mesh exported with the Blender source unchanged. Validation passed
174 workstation tests, the desktop build, shader checks and four-note native
captures on OpenGL, Vulkan and WebGPU, with zero structural intent mismatches.
Visual review found the default pitch scale too small. Improved opening pitch
framing passed185 workstation tests (zero failures, one ignored), the desktop
build and intent inspection. Updated four-note captures passed on all three
backends and were visually reviewed at640x480; the film remains subtle against
the studio background. Evidence: output/checks/bubble-piano-roll-20261030-102901.
Subsequent native input review found brief N/Enter taps could disappear between
frames. Ordered press events now retain their modifiers and clear on focus loss;
the shared follow-up passed194 workstation tests, the desktop build and packaged
startup preflight. The DAW-agent native walkthrough confirmed quick N opens the
piano roll and quick Return places a visible note.
Instrument output, project persistence and touch-note editing
are not connected by this change. No Mac package, phone build or upload is claimed.


## Spawn theatre and shared workstation — October 30

The active Cardinal casino/theatre layout is now authored as continuous geometry
at spawn: the circular shell, proscenium stage, acoustic walls, audience terraces,
FOH desk and walking ramps. The original rig keeps its sixteen connected flown
cabinets, four subwoofers, four front fills, four bus assignments and splay angles.
New editable Blender enclosures and the computer are in `Art/Theatre`; runtime
triangles come from those same evaluated models. The game currently uses their
authored diffuse colors and geometric lighting, not the full Blender PBR materials.

Walk forward from spawn and click the computer screen to enter the existing
workstation. Escape returns from the piano roll first, then returns to the game;
on mobile the existing two-finger double-tap returns. The game camera, document
and panel layout survive this focus change. On desktop, returning to the theatre
leaves the final rendered workstation frame on the computer glass until the next visit. The launcher retains a separate
“DAW only mode” button (`--daw-only BACKEND`, with `--workstation` still accepted).

The shared native audio output consumes bounded four-channel PCM outside the
editor. World mode applies the authored bus EQ, cabinet directionality, distance,
visibility and propagation delays. Actual post-EQ levels drive colored traveling
wavefronts clipped against current terrain and venue surfaces. Thin opaque strokes
replace the old translucent edges in the shared depth pass. DAW-only mode uses
stereo monitoring. Pause, seek and speaker mute retain separate meanings. MIDI
preview uses an explicit adaptation of Cardinal's sine/ADSR ChucK voice; external
VST3/CLAP/AU instrument hosting is not supplied by this adapter.

`BuildSupport/Theatre/validate_theatre.py` performs the local build, structural
intent check, focused collision/DSP/focus tests, workstation regressions, a bounded
OpenGL spawn capture and a Blender hardware render. Those checks and visual review
remain pending for this source revision. No phone build or upload is claimed.

## Physical phone — October 30

The phone sits face-up beside the theatre computer. Its new continuous Blender
model is in `Art/Phone/Phone.blend`, with a glTF export and matching runtime
triangles. Rounded metal, glass, lenses, side keys, USB contacts and speaker slots
replace the old cell model. Blender preserves full materials; current native
rendering uses the same mesh with diffuse colors and facet lighting.

Aim at it within3.25m and press F to pick it up with an empty tool. R focuses the
screen, clicking selects apps, and Escape releases focus before opening pause.
F lowers a focused phone, then places it on the aimed surface. Loaded tools must
be emptied first. While held, the empty tool's contact shell and controls are
suspended; material simulation continues and the tool restores at the hands.
Mobile taps pick up/focus/select; the existing tool action becomes Lower phone
or Put down phone while held. The pose and selected app persist on normal exit.

The home screen keeps Cardinal's Light, Camera, Meter and Weather hit regions.
Light uses the original cone/range with bounded coarse shadows on ordinary scene
triangles. Separate vegetation, water and player materials are not lit by this
CPU pass. Camera projects the current full player skin as the original live
selfie; there is no shutter. Meter observes actual final mixed stereo RMS/peak.
Weather samples the game's current climate and6/12/18/24-hour forecast; missing
humidity, pressure and rain data remain unavailable.

`--check-phone OpenGL` captures all five screens in the native planet scene and
restores its temporary state without saving it. `BuildSupport/Phone/validate_phone.py`
collects Blender, build, contract, focused regression and native capture evidence.
The October 31 combined macOS build passed the phone regressions and all five
packaged OpenGL screen captures, followed by visual review. Native input feel,
other phone graphics backends and iOS device runs remain separate.

## Combined agent build — October 31

The agents share this checkout: arrangement and gem track handles, tape clips,
bubble piano roll, lane monitoring indicators, engraved controls, spatial audio,
custom style packages, export, desktop plugin hosting, physical tools, the spawn
theatre and the phone are included by the current build. The separate compressed
render-state experiment remains experimental and is not enabled in the renderer.
The standalone SoundDesign ChucK drafts remain separate from the game runtime.

The integration pass fixes a theatre ramp/landing collision gap, failed computer
presentation cleanup, and relative CARGO_TARGET_DIR handling in the Mac packager. Launcher instructions now include
the theatre computer and phone. BuildSupport/macOS/build_combined.py saves source
hashes, regression results, the signed staged app and packaged native captures.
All 55 recorded checks passed in `output/checks/combined-build-20261031-044315-579250/report.json`.
The reviewed, signed bundle is now `output/macOS/MirrorPrismals.app`, with the prior
app preserved alongside it. Checks cover cleanup/retry, packaged startup, DAW
controls on OpenGL/Vulkan/WebGPU, arrangement/bubbles/spatial spheres, all five
phone screens and the actual interactive theatre spawn. Bubble highlights retain
their authored low contrast; physical input feel was not playtested.
The theatre validator now enters the game instead of the graphics demo. Native third-party
plugin compatibility and optional VST2 worker setup remain separate; the bundled
VST3/CLAP/AU host does not imply that every plugin has been tested.

## Workstation performance investigation — October 31

The installed workstation's sampled idle loop sleeps between input polls. Earlier
corrected render-pass measurements identify gem rendering as the main active-frame
cost. The current candidate caches unchanged floor-light visibility and adds a
conservative ordered hierarchy over control instances. Actual triangle tests,
materials, optical sample/bounce budgets and drawable resolution are unchanged.
The hierarchy preserves the original instance order and retains the old traversal
for packets without its optional header; the lighting cache invalidates on actual
panel, light, floor and domain changes.

`BuildSupport/Workstation/check_workstation_performance.py` freezes the installed
app, stages a candidate and compares matching windowed/fullscreen runs, actual GPU
timestamps and native controls captures. The first comparison passed 200
workstation tests and eight performance tests, with byte-identical controls
captures on all three backends. Source inspection passed after correcting the
runner to inspect the checkout rather than runtime-only packaged assets.
At 640×480, mean frame time was 339.7 ms before and 341.0 ms after. At 1920×1200,
it was 938.9 ms before and 907.4 ms after; GPU time accounts for nearly all of it.
These short sequential runs do not establish a meaningful active-frame speedup,
and the candidate was not promoted. Evidence is in
`output/checks/workstation-performance-20261031-051718-694160/report.json`.
The benchmark excludes playback; the user's existing audio session is preserved,
so timings do not claim exclusive access to the machine.

A second, private compute comparison retained the full optical packet, geometry,
samples and path budgets. Compiling out absent bubbles, audio spheres, custom
panels and timeline meshes reduced the controls frame from 357.6–360.8 ms to
318.6 ms at 640×480. Its RGB8 capture matched exactly; maximum float RGB error
was 0.00000159. The repeated baseline drifted by 0.9%. These six-pass static
compute measurements do not establish native application FPS or motion fidelity.
Evidence: `output/checks/optics-rethink-20261031-061827-752714/report.json`.
Both corresponding controls previews were reviewed and match. The canonical
compute and native fragment captures differ at 60 pixels, so the compute result
is compared with its own unchanged compute reference, not claimed as a perfect
replacement for the native pipeline.

The shared-workgroup ray-queue alternatives are rejected for production: despite
negligible float differences, they took 609.3 ms and 925.2 ms, versus 367.8 ms for
the matching private-queue 4×4 workgroup. Their generators remain as diagnostic
evidence only. The source now precompiles one controls-only WebGPU
program and retains the full program whenever an excluded family is present.
It does not change resolution, geometry, lighting, optical budgets or materials;
native speed, feature fallback and image comparisons remain pending. The first
native specialization check built successfully but aborted at startup because
the installed WebGPU library rejected the Internal error-scope filter. The source
now uses only its supported Validation and OutOfMemory filters. That correction
has not yet been rebuilt and verified; the failed candidate was not installed.

The next private experiment changes the optical representation itself.
`BuildSupport/Workstation/compiled_gem_paths.py` compiles fixed-view ruby facet
sequences into affine hit positions and distances, constant ray directions and
optical factors. `evaluate_compiled_gem.py` evaluates these continuous formulas
at actual sample positions while retaining live engraving, absorption, floor and
area lights. Cells without a certified geometric path use the canonical tracer.
This is a table of path formulas, not stored pixel colors. The first fixture is
one isolated authored ruby; it does not yet represent the full workstation.
`check_compiled_gem.py` compares the same compiled program against an independent
canonical shader under fractional-pixel translations, a press and changed
lighting, saving GPU timings, float errors and images. Speed and visual fidelity
are unverified until that comparison has completed and been reviewed.
The first launch stopped before GPU measurement because the authored ruby
exceeded an unnecessary global convexity assumption. The compiler now records
that diagnostic while retaining exhaustive per-cell nearest-hit and exterior
self-intersection checks against the original triangles. It does not enlarge
the geometric acceptance tolerances or alter the mesh.
The subsequent 128-cell-per-axis preprocessing attempt hit its 900-second
deadline before producing a program; it measured no GPU improvement. The private
compiler now memoizes repeated affine intersections, visits cells breadth-first
across both samples, and saves partial certified coverage after a 40-second work
budget. Unvisited cells use the original tracer. The next diagnostic defaults to
32 cells per axis; pixel resolution and optical sample/bounce budgets stay fixed.
Its report must show actual compiled-sample coverage alongside any timings.

The bounded comparison completed in
`output/checks/compiled-gem-20261031-081633-655859/report.json`. Preprocessing
finished in 29.1 seconds, but only two of 2,048 cells qualified. The training
frame used compiled paths for 171 of 614,400 primary samples. Its GPU median was
69.4 ms versus 65.1 ms for the canonical shader; every held-out candidate median
was also slower. Baseline drift was -6.27%, so the precise slowdown is uncertain,
but this experiment establishes no speedup. The training image pair was reviewed
without an obvious visible difference; almost universal canonical fallback makes
that weak evidence of compiled-path fidelity. This full-path cell-cache design
is rejected for production. Its scripts remain diagnostic evidence only; no app
was installed and the requested major workstation speedup remains unfinished.

The next private ruby prototype uses a sampled optical response instead of
certifying complete ray paths across cells. `BuildSupport/Workstation/gem_response.py`
generates a GPU bake that records eight exterior light paths and four linear
engraving-light response bases per sample. Runtime lookup reevaluates terminal
lighting from local exit positions and directions, blends neighboring responses,
and applies the existing AgX display transform. It performs no glass intersection
searches. The isolated floor retains its twelve emitter samples and original
BRDF through `response_floor.py`; this specialization relies on zero panel/custom
occluders and cannot be used unconditionally in the full workstation.

The 256-square, two-sample response occupies 60 MiB before compression. Shape,
material, scale, view and engraved glyph are fixed in this first experiment.
Light colors combine linearly; moved-light engraving uses a geometric-gain
approximation with its internal escape response held fixed. Filtering and keeping
the strongest eight terminal paths are also approximations. The GPU baker lives
in `response_probe.cpp`; `check_gem_response.py` compares one shared training
atlas against the independent canonical shader under translation, press, moved
lighting and recoloring, and records discarded throughput and actual hit coverage.
This prototype is unverified, is not installed in the app, and does not establish
a whole-workstation speedup until measurements and images have been reviewed.
The first response job baked successfully (42.4 ms measured GPU pass, excluding
pipeline compilation and readback), with no terminal-lobe weight discarded.
Its lookup shader then failed parsing because an interpolated negative bound
formed a `--` token. The generator now parenthesizes signed bounds; runtime
speed and image quality remain unverified pending the corrected comparison.

The corrected comparison completed in
`output/checks/gem-response-20261031-085359-797556/report.json`. The reference
case dropped from 67.1 ms to 10.2 ms GPU time at 640x480, with two primary samples
in both versions. The eight paired cases measured speed ratios of 6.5–9.5;
baseline repeat drifted +27.43%, so these are short-run estimates rather than
stable application FPS. The baked table discarded no terminal-lobe throughput.
Reviewed training images retain the broad ruby/engraving appearance but expose
softened detail and thin diagonal gray artifacts. The moved-light response was
also viewed; the remaining image pairs require review. This is a promising
performance result, not a production installation. The user subsequently accepted
the appearance and requested integration; artifact removal is not a prerequisite.


Production integration is now being validated for the four ruby transport caps.
`GemResponse.rs` loads an immutable, integrity-checked response asset once;
`GemResponseKernel.glsl` feeds the baked exterior paths into the existing scene
ray queue. Other objects, occlusion, lighting, AgX and audio behavior remain live.
The four 128-square tables occupy approximately 60 MiB together. Compatible
neighboring paths interpolate; silhouette/path discontinuities, unsupported
view/quality/material and substantial glyph-footprint changes use full tracing.
Moved-light engraving retains the prototype's geometric-gain approximation.
The shared kernel generates every backend; optical texture width is now 512 to
keep the larger packet within a 16K texture height, with partial frame rows
uploaded separately on GL. iOS device validation is deferred.
`build_gem_response.py` bakes each actual glyph and runs Rust and full-scene GPU
checks, staging assets separately for review. The installed app has not yet been
replaced, and the isolated ruby speedup is not a whole-DAW performance claim.

The integration check completed in
`output/checks/gem-response-integration-20261031-055437-867216/report.json`:
204 workstation tests passed (one ignored), and Vulkan/GL shader checks passed.
However, the full-scene GPU median increased from 378.9 ms to 395.1 ms; repeated
baseline was 375.0 ms. Only 8,107 of 614,400 primary samples used the response
(1.32%). This integration is rejected for performance and its staged response
assets have not been published or installed. The isolated prototype speedup did
not transfer to this conservative four-cap integration. Broader scene coverage
and less lookup/queue overhead are required before enabling a production asset.


The rejected four-cap runtime is now retired: its buffer tail, loader registration
and shared shader hook were removed. Its files and measurements remain as
experimental evidence. `RubyResponsePlan.rs` and `RubyResponseShader.rs` replace
it with a native WebGPU compute cache selected by ruby optical properties.
Four adaptive panel slots use128-square maps and up to16 visible ruby control
components use64-square maps, within a separate120MiB buffer. Missing slots,
unsupported views, other materials and timeline components retain canonical
tracing. The other native backends retain their canonical shaders.

Shapes, material parameters, poses, glyph content, footprint and lighting are
part of the cache key. Changed maps bake before the render pass; submission
acknowledgement prevents an update from losing pending work. This initial runtime
implementation rebakes after translation and lighting changes; it does not yet
normalize those changes away. Exterior rays use the live scene and immediately
resolve terminal leaves before entering the ray queue. GPU timestamps include
baking on frames that need it. The finite `check_ruby_cache.py` stages an app and
compares both warm reuse and cold rebuild cost.

The material-wide check completed in
`output/checks/ruby-cache-20261031-141437-244706/report.json`: the shader and native
pipeline validated, with four panels and eight control components admitted.
430,096 of 614,400 primary samples used the cache. Static GPU median was
367.36 ms without it, 286.20 ms with warm reuse, and 359.86 ms including a full
rebuild; repeated baseline was 371.84 ms. The short native panel-motion benchmark
rose from 2.84 to 3.04 FPS. This is insufficient for the requested drastic gain,
and visual inspection found blocky panel lettering. The candidate is rejected
for promotion and the cache now requires `WORMWOOD_RUBY_RESPONSE=1`; ordinary
launches retain the prior renderer. The installed application remains unchanged.
The isolated 6.6-times result skipped scene traversal entirely, whereas this
integration still performs a primary scene query and live scene queries for
cached exterior paths. Material coverage alone does not remove that work.
Floor height is excluded from intrinsic invalidation because exterior floor
evaluation remains live; ambient RGB still invalidates baked engraving light.

The subsequent diagnostic-only cost isolation completed in
`output/checks/ruby-cost-20261031-142537-904513/report.json`. Warm rendering was
301.69 ms (302.85 ms repeated). Omitting floor lighting reduced it to277.17 ms;
omitting exterior scene occlusion/continuations reduced it to37.02 ms. Omitting
only cached continuations took156.00 ms. A separate small kernel retaining
cached terminal paths but omitting all fallback work took21.79 ms. These variants
deliberately remove visible effects, so none is a production optimization or a
preserved-image speedup. Their timings overlap and must not be added together.
`check_ruby_split.py` tests separating complete cached pixels from a compacted
canonical fallback pass, comparing the same scene and reference pixels. It is
a headless experiment; native integration and sharp panel text remain open.

The user subsequently reviewed the actual `omit-exterior-occlusion.png` preview
and approved its appearance for implementation. This supersedes the earlier
visual rejection for that specific variant. The default native WebGPU ruby
pipeline now resolves exterior terminal rays directly against the live studio
lights and floor, matching the approved preview. Primary scene visibility,
intrinsic optical baking, materials and AgX remain. Inter-object exterior
occlusion is intentionally omitted, and the approved coarse cache lettering is
retained. This is the approved direct-studio variant, not the separate-pass
experiment. `WORMWOOD_RUBY_RESPONSE=0` retains the full-scene comparison path.
Other native backends keep their existing shaders. The finite
`release_approved_ruby.py` verifies preview pixels, focused tests, native default
selection, motion performance and bundle signing before replacing the local
macOS app while preserving a backup. Release validation completed in
`output/checks/ruby-approved-20261031-144827-424473/report.json`: 206 workstation
tests passed (one ignored), the production preview matched the accepted pixels
exactly, native default selection and controls passed, and bundle signing passed.
At640x480, static GPU time fell from361.03 ms to37.51 ms (9.63 times faster);
including a full cache rebuild took116.73 ms. The32-frame native motion benchmark
fell from352.22 ms to79.56 ms per frame, or2.84 to12.57 FPS (4.43 times faster).
The validated app replaced `output/macOS/MirrorPrismals.app`; the previous app is
preserved at `output/macOS/MirrorPrismals-before-ruby-20261031-144827-424473.app`.

TestFlight build2495.19.10 uploaded successfully, but the reported iPhone DAW
startup failure exposed missing `EdisonHardware/connected.json` and
`connected.bin` in its package. The iOS packager now includes both. Before
distribution, two focused CPU checks load panel data and optical geometry from
the finished app's EngineAssets, rather than the source checkout. A regression
release script checks that the affected archive fails before rebuilding and
uploading the correction. This validation does not establish iPhone rendering
or performance; those still require a device run.

The requested next WebGPU optimization separates glass transport from labels
and studio lighting. Intrinsic caches survive translation and lighting/label
edits; shapes, material, scale and path budgets still invalidate them. A compute
pass resolves current lighting once per cache sample before each draw. A small
primary shader path bypasses the full ray queue for supported visible surfaces.
Lettering evaluates the original three-layer density, absorption and lighting
at drawable resolution on the first refracted segment. Later internal text
reflections are omitted in this explicit appearance approximation. The previous
approved exterior studio policy and actual primary mesh visibility remain.

Headless evidence is in
`output/checks/ruby-transport-20261031-154919-389887/report.json`. Production output
matched the diagnostic candidate, and the fast primary path was pixel-identical.
All12 entries survived a translated scene and changed lights without rebaking.
The held-cache versus fresh-bake comparison has RGB RMS error0.0000434 and17
pixels with any reported difference above0.00003 or cache-count differences;
world-coordinate float rounding produces sparse silhouette/facet differences,
so translation reuse is not claimed bit-identical. The grid and interpolated
floor experiments are retained as diagnostics, not runtime paths. Native validation completed in
`output/checks/ruby-transport-release-20261031-160532-850296/report.json`:
211 workstation tests passed (three ignored), the production preview matched
the validated sharp-text candidate exactly, native controls and signing passed,
and the app was installed with the prior app preserved. The640x480 motion
benchmark improved from78.45ms/12.75FPS to49.85ms/20.06FPS (1.57times faster).
GPU time averaged41.44ms. The stationary phase remained about27ms per frame,
while the expansion phase averaged75.76ms; shape-changing motion still triggered
26 bake passes across36 total draws. Pass count does not count entries refreshed.
These measurements establish a motion improvement, not a completed60FPS target
or phone performance. The installed app is `output/macOS/MirrorPrismals.app`;
its predecessor is `output/macOS/MirrorPrismals-before-transport-20261031-160532-850296.app`.

The exact panel optimization is installed and validated on macOS. `RubyConvexShader.rs` copies the canonical optical equations into a
separate single-continuation kernel for native convex panels. An actual authored
halfspace can certify that an outgoing ray cannot reenter; uncertain cases keep
the original intersection test. A second compute pass replaces rejected samples
with the original integrator and bakes dirty controls. Both passes remain inside
the measured frame. Cache size, sampling, artwork, and sharp text are unchanged.

Evidence is in `output/checks/ruby-convex-20261031-205611-021702/report.json`.
Seven shader tests passed. The original, small, large, and uniform resize
fixtures and deliberately forced fallback produced identical RGB/cache-count
pixels. These fixtures use the actual optical constructors but hold other
furniture fixed; native motion results are reported below. Full dirty-cache
rebuild time fell from about40ms to28.5ms, with the isolated panel pass about5.4ms.
Native validation completed in
`output/checks/ruby-convex-release-20261031-211440-032081/report.json`.
The paired 640x480 motion run improved from28.24FPS to34.11FPS (1.208times),
with29.32ms mean frame time and41.73ms p95. Actual GPU timestamps averaged20.61ms.
The preview comparison was identical; native controls and signing passed.
The app is `output/macOS/MirrorPrismals.app`; its predecessor is preserved as
`output/macOS/MirrorPrismals-before-convex-20261031-211440-032081.app`.
This short32-frame run used four warmup frames and had no observed audio
callbacks; it establishes neither sustained60FPS nor audio responsiveness.
Idle averaged about67FPS, but panel expansion remained about25FPS.

The same investigation found that ivory fader/meter glass and clear windows
dominate uncached primary shading. Broadening the existing coarse cache reached
about14.6ms warm frame time, but changed visible reflections and is rejected.
It is not a runtime path. Removing the provably unused studio fallback queue
showed no speed gain and is also diagnostic only. Whole-path reuse during
arbitrary resizing is not valid because pavilion normals change with aspect.

The next workstation experiment is opt-in only: `WORMWOOD_WORKSTATION_PIXEL_TRANSPORT=1`
adds exact screen-pixel transport for eligible ivory/clear controls, including all
canonical interior engraving segments. Original/small/large/uniform fixtures
match within3.58e-7 maximum RGB error; zero pixels exceed3e-5. The static640x480
headless GPU median is14.14ms versus24.65ms over12passes. This is not native FPS.
Storage stays120MiB; components whose complete pixel records do not fit use
the original tracer. Higher resolutions therefore require separate evidence.
`WORMWOOD_WORKSTATION_FLOOR_SAMPLE_CACHE=1` separately reuses independently keyed
panel shadow contributions. CPU fields remain bit-identical in the checked
motion/light/floor cases. Single-panel changes take4.36ms versus7.83ms in the
standalone check, two-panel changes7.23ms versus7.80ms. Shared floor/light changes
use the original computation (7.79ms versus7.78ms); initializing factors on a
later panel-only change costs about14ms once. Neither switch is enabled by default.

`BuildSupport/Workstation/validate_pixel_transport.py` stages a separate Mac app
and performs matched native A/B measurements, with all optimization switches
explicitly recorded. Each new duration run requires at least30seconds after
warmup, bounded by20120totalframes and240seconds. It exercises repeated panel
motion and settled idle, actual owned native audio where available, diagnostic
640x480 and the observed normal fullscreen drawable. Every repeat must match
workload/device/dimensions and verify requested audio before aggregate gains are
reported. GPU timestamps now distinguish panel bake, general bake, live resolve
and optical shading; CPU stage timing is optional. Meter meshes remain static;
control gestures and input-to-visible latency are not established by these runs.
The first staged run built and signed successfully and matched the640x480
controls capture within one8-bit channel code, but the third fullscreen motion
repeat stopped at a nondrawable window. Completed normal1920x1200 audio trials
were about7.4FPS; no completed fullscreen aggregate or60FPS result exists.

The follow-up source extends opt-in exact pixel transport to the ruby timeline
track handles, which previously bypassed the cache. Variable-sized pixel spans
share the existing120MiB allocation with fixed legacy control spans; unsupported
or clipped objects retain canonical tracing. Text keeps all canonical
interior segments. Native reports distinguish CPU cache admission from actual
GPU path reuse. `WORMWOOD_WORKSTATION_RESOLVE_REUSE=1` independently skips a
lighting resolve only when its complete supported inputs and submitted cache
are unchanged; it is also off by default. These additions have not replaced the
playable app. The finite
`BuildSupport/Workstation/validate_timeline_transport.py` first compares an
authentic four-handle/one-tape fixture against the original shader, including
one-time resolve reuse, then builds and measures a separate fullscreen app.

The completed staged timeline validation is recorded in
`output/checks/ruby-timeline-native-20261031-224550-202605/report.json`.
All identity checks passed and the installed app was unchanged. At640x480,
the static GPU median fell from32.38ms to16.22ms; immutable one-time resolve
measured12.13ms. The candidate actually reused7903 timeline primary samples,
and maximum RGB error was3.84e-7 with no channels over3e-5. This is diagnostic
GPU work, not native FPS. Matched1920x1200 native runs on AppleM1Pro/Metal with
48kHz/256-frame native audio produced median measured-loop motion throughput
7.493FPS versus7.540FPS, and settled idle8.260FPS versus9.303FPS. All three
repeats completed with GPU query coverage. Motion baked on248of256 measured
frames in each variant; admission counts are not effective GPU hit rates.
Measured transport queue underruns remained nonzero. Measured VM underrun
deltas ranged from0to2; callback/backend timing counters are separate evidence.

The next source change removes only floor-height and active-instance-count
dependencies from eligible pixel engraving keys. Ambient RGB bits, own pose,
camera, glyphs, clipping and lighting remain exact dependencies; floor height
still invalidates terminal lighting resolve. Seventeen Plan and five resolve
tests pass, including new admission/removal/reactivation and ambient-bit cases.
Native gain from these key changes is unmeasured. The diagnostic-only
`BuildSupport/Workstation/probe_engraving_factors.py` isolates invariant glyph
metadata, unused lighting queries and exact-zero navigation engraving; it
validates each generated shader and image before any runtime integration.
That experiment completed in
`output/checks/engraving-factors-20261031-230555-267487/report.json`: all ten
generated shaders validated, all24 GPU cases were finite, and the maximum RGB
difference was1.35e-6 with unchanged cache-use counts. Its individual arithmetic
changes gave no substantial repeatable improvement, so they remain diagnostic
variants and are not enabled in production.

The single next discriminating experiment is
`BuildSupport/Workstation/probe_primary_visibility.py`. It preserves the completed
fixture's two primary rays and canonical intersections while measuring a
query-only kernel separately from full shading. Separate instrumented kernels
count actual control/timeline candidates, BVH nodes, triangles and panel planes.
These isolated costs are not additive, their diagnostic outputs are not artwork,
and640x480 fixture timings are not fullscreen FPS. This tests whether conservative
primary candidate lists merit implementation before allocating new GPU buffers.
The completed result is
`output/checks/primary-visibility-20261031-231916-537536/report.json`.
Full shading measured12.31ms and12.02ms; isolated primary queries measured1.893ms
and1.896ms. All outputs were finite and input identities matched. Actual control
mesh candidates averaged0.0736 per primary sample and timeline candidates0.601;
the existing broadphase already rejects most objects. These isolated timings
do not justify expecting a large gain from primary candidate lists alone.
The next diagnostic, `probe_split_primary.py`, instead tests the current renderer
in separate queue-free fast and canonical fallback dispatches. The older
full-occlusion split was slower; it is not reused as evidence for this different
direct-studio/pixel-cache workload. Every pass and its gap remains timed, rejected
pixels use the unchanged complete shader, and image/cache-count equivalence is
required. No native split pipeline has been integrated.

`WORMWOOD_WORKSTATION_CACHE_CAUSES=1` independently reports bounded CPU placement
causes for at most256 Plan updates,20 slots per update and16 changed key words
per slot. Raw bits and variable glyph/mip offsets are recorded through existing
benchmark metadata. Twenty-four focused Plan/helper tests pass, including
off/on output equivalence, pending versus newly dirty entries, disabled frames
and budget exhaustion. This observer defaults off and is not GPU-hit evidence.

`WORMWOOD_WORKSTATION_FRAME_ATTRIBUTION=1` adds bounded adjacent host-wall
intervals to finite benchmark samples, covering audio/update/Pump and WebGPU
acquire/encode/submit/wait/readback/present. The clocks are independent of GPU
queries; nested intervals are not additive GPU costs. The diagnostic override
`WORMWOOD_WORKSTATION_BENCHMARK_PROFILE=0` suppresses automatic benchmark
profiling, while explicit `--performance` or `WORMWOOD_PERFORMANCE=1` still wins.
These flags default to existing behavior. Their compilation and native
profile-off/on comparison are pending; no speedup is inferred from this source.
`BuildSupport/Workstation/validate_frame_attribution.py` now defines that bounded
check: one separate staged build, matched fullscreen idle profile0/1/0 sessions,
then a64-frame motion replay with cache-cause observation. The profile-off arm
uses native callback counters for audio evidence independently of disabled
performance telemetry. It verifies actual profile selection, adjacent timing
sums, complete draw indices, dimensions/device/audio matching and build identity.
The driver has passed Python syntax checks but has not run; it neither installs
the staged app nor substitutes these short diagnostics for sustained validation.

Floor-plane numerator factoring preserves all29 checked fields bit-for-bit.
The current standalone one-panel case takes4.25ms versus7.76ms; this is CPU
field work, not application FPS. GPU reports now use the envelope of active
pass intervals because Metal render and compute intervals may overlap; pass
times are not additive shader costs. The harness preserves interrupted attempts
and permits at most three fresh sessions for explicitly recoverable pre-draw
window gates. Backend errors remain failures. Audio reports retain lifetime
underrun totals and separately measure counter deltas around the timed window.


Mixer meter glass correction (2026-11-02): the macOS retained compositor now
refreshes visible authored meter apertures through the original optical trace.
The previous opaque-face update stopped at the protective glass and left cached
readings unchanged. Meter rays now check the mixer assembly before resolving
exterior studio lighting, so the LEDs behind the window remain visible. The
ivory housing, glass geometry, lightbox, 60 dB scale and existing audio observation
path are preserved. This deliberately adds optical work within visible meter
apertures; it does not invalidate the entire panel or add audio callback work.
The opaque-only live-meter alternative is retired for glass-covered meters.
`WORMWOOD_WORKSTATION_REFRESH_VALIDATION=meters` with the explicit finite audio
benchmark exercises silent and independent left/right levels for visual checks.
Native verification: the 36-frame WebGPU check changed left and right spectrum
pixels independently through the window, with one retained bake throughout.
The six mixer tests and eleven retained-render tests passed; intent inspection
reported zero structural mismatches. Evidence: `output/checks/mixer-meters-20261102`.
The native comparison tool also now preserves its initial immutable asset upload;
previously its frame interception discarded the LUT and produced black captures.

## Opt-in combined game/workstation benchmark

`--benchmark-game-workstation WebGPU --frames 32` follows Cardinal's
`PerfSystem::prepareHybridDawCapture` pattern: locate the actual theatre computer,
use the normal game Suspend → RunEmbedded → Resume transition, and close through
normal application cleanup. It requires the existing security-camera pass-through
setting and validates camera/venue geometry on every frame. It does not substitute
standalone DAW mode or enable a synthetic audio stress project. Existing benchmark
warmup, cycle, minimum-duration and pacing environment variables apply. Set
`WORMWOOD_WORKSTATION_BENCHMARK_PACED=1` to retain production embedded pacing;
reported complete-loop FPS includes pacing and final GPU completion. Whole-loop
percentiles include those costs, while existing work-only frame timings do not.
The nested report preserves the DAW GPU queries before restored game cleanup can
overwrite the shared query file. Application presents are not physical scanout.
Normal interactive entry and rendering are unchanged.


Historical combined game/workstation performance update (2026-11-02; the
projected camera cache/color prefix is superseded by the live-world scene above):
macOS at that milestone cached
unchanged projected camera geometry, draws the camera prefix through an equivalent
color-only pipeline, and runs the original live-meter optics only within
conservative meter scissors. Unsupported geometry/layouts retain the original
full shader. Meter sampling, depth, glass, reflections and display transforms are
unchanged. Embedded active-frame pacing now uses the window display metadata;
quiet polling remains bounded and FIFO presentation is unchanged.

At1920x1200 on M1Pro/Metal, the finite theatre-camera pass-through benchmark
measured28.8 to58.8FPS settled and7.36 to8.64FPS during the same120-frame panel
motion cycle. These are complete application-loop/present-call rates, not measured
physical scanout. The initial5-frame comparison and subsequent18-frame comparisons
including changing meters, camera motion and authored track-I/O controls were
byte-identical. Timed windows reported zero added audio underruns, but the startup
document is empty: this is not a loaded plug-in/audio stress test or the exterior
world scene. Motion still performs expensive appearance refreshes; hundreds of
FPS have not been achieved. Explicit CAMERA_CACHE, CAMERA_SPLIT, METER_SPLIT and
COMBINED_DISPLAY_CADENCE environment overrides retain reference comparisons.
Primary reuse, extended opaque controls and the bar primary gate remain opt-in
experiments. Local evidence is in the combined-optimization task artifacts.


Exact live-meter contribution cache (2026-11-02): the approved macOS retained
path now records the original ordered FP32 radiance contributions after two
stable draws. It replays the same arithmetic with the current meter levels.
A complete packet/resource key, bounded64MiB generation and GPU-completion
publication prevent stale reuse; overflow, unsupported or changed inputs retain
the original optical trace. No sample count, material, lighting or geometry changed.
`WORMWOOD_WORKSTATION_METER_LEDGER=0` selects the original reference path.

At1920x1200 on M1Pro/Metal, matched settled runs improved58.94 to79.26FPS and
58.96 to80.91FPS in the reversed-order repeat. Eighteen native comparison images
were byte-identical. The cache requested56,218,388bytes in the empty fixture,
with a64MiB hard cap. Real transport playback on an empty silent timeline with
one unarmed audio track remained49.25 versus49.53FPS: its moving playhead
invalidates this strict key. The final packaged empty panel-motion pair remained
8.65 versus8.68FPS, with91appearance bakes. These are complete application-loop/
present-call rates, not physical scanout; startup/bake cost is separate. Measured
audio underrun deltas were zero, not a claim that startup had no underruns.
Playback, panel-motion and populated-project performance remain unfinished.
Ray-queue and capture-workgroup experiments remain disabled after no useful gain.
Local evidence: combined-optimization/meter-ledger-native, meter-ledger-playback,
meter-ledger-packaged-motion and meter-ledger-app in the local task workspace.


Certified playhead-independent lighting reuse (2026-11-02): the normal macOS
ledger now reuses only samples whose actual original ray queries prove both
moving playhead solids cannot affect them. Other samples run the original trace.
All other packet/resource dependencies remain checked. Certificate instrumentation
has its own compute module; ordinary optical-bake and display WGSL stays unchanged.
`WORMWOOD_WORKSTATION_METER_LEDGER_PLAYHEAD=0` retains the strict previous key.

Two matched10-second real-transport playback pairs on M1Pro/Metal at1920x1200
improved49.51→79.10FPS and49.48→81.53FPS (about62% on the pair means).
All18 captured native image comparisons, including both moving solids and
changing meter levels, were byte-identical. Playback used an empty silent timeline
with one unarmed audio track; no populated plug-in/source stress is claimed.
Measured audio-underrun deltas were zero. GPU envelope fell18.23→9.12–9.31ms;
these are application-completion/present-call rates, not physical scanout. Cache
build is outside the warm measured interval and has no isolated GPU timing scope.
One generation requested64,051,988bytes under the64MiB cap; unsupported states
fall back. Panel-motion appearance rebuilding remains expensive and unresolved.
The attempted studio ray-queue simplification failed its native image gate and
remains disabled. Evidence: combined-optimization/ledger-playback-isolated and
linear-studio-native in the local task workspace.


Live daylight correction and independent camera colors (2026-11-02): the earlier
combined benchmark figures above are historical. Their camera geometry cache
omitted implicit wall-clock daylight, holding exterior colors until camera motion.
The earlier18 identical images remain actual observations, but they did not record
camera lighting inputs. A fixed-clock rerun now reproduces18/18 identical images
for the playhead certificate and18/18 for the combined camera/meter optimizations,
including identical prepared camera hashes and DAW packets. Normal implicit-clock
views now rebuild the original camera packet so daylight remains live.

That correction alone measured59.02FPS settled,8.56FPS for the empty panel-motion
cycle, and49.25/49.70FPS for real transport playback with one empty unarmed track.
A separate exact dependency correction now permits optical-cache reuse when ONLY
finite RGB values of the independently drawn camera prefix change. Every geometry,
depth, tag, count and fullscreen-tail bit remains checked. Camera uploads and draws
continue every frame; no lighting value is frozen, rounded or approximated.
`WORMWOOD_WORKSTATION_LEDGER_CAMERA_RGB=0` selects the conservative reference.

The changing-daylight validation produced18/18 byte-identical images, five distinct
camera packet hashes and visibly changing background pixels. Matched10-second
ABBA real-clock playback runs improved49.25/49.70 to79.35/80.60FPS, with GPU envelopes
18.18–18.28 to9.19–9.27ms and zero measured audio-underrun deltas. These remain
completed application-loop/present-call rates on1920x1200 M1Pro/Metal, not physical
scanout or a populated-project stress test. Startup and the one-time bounded64MiB
cache build remain outside the warm timing window. Panel-motion rebakes remain
unsolved. Bounded capture dispatch matched18images but regressed8.68 to8.37FPS
in its first timing pair and stays disabled. Evidence: combined-optimization/
clock-scope-assessment.json, clock-controlled-native and live-camera-ledger in the
local task workspace. The previous frozen-daylight app is preserved as an explicitly
labeled faulty rollback; the normal app includes the daylight correction.


iOS archive compatibility refresh (2026-11-02): current device compilation now
accepts the shared title signature, queries optional UIKit drawable dimensions,
and explicitly rejects the desktop-only scripted GLFW resize diagnostic on iOS.
The iOS package includes the required CrosswalkArming assets and retains five new
WebGPU runtime exports. Build 0.1.0 (2497.22.21) compiled, archived, signed, passed
both packaged workstation loader checks and all 161 required graphics export checks,
and was accepted by App Store Connect for internal TestFlight processing. Final
TestFlight availability remains unverified: the separate status tool requires
authentication beyond the configured Xcode upload session. No phone launch or
device gameplay/performance validation was performed. Existing iOS touch coverage,
output-only physical audio and exclusion of desktop plugin hosting remain; macOS
retained/stored-pose defaults were not silently enabled on mobile. This build is
not evidence of desktop DAW feature or FPS parity. The desktop app is unchanged.


## iOS full-screen DAW candidate (2026-11-03)

Mobile now selects one full-screen original gem panel, or a full-screen timeline.
From Timeline, swipe down/up/right/left anywhere to open top/bottom/left/right.
Only the opposite swipe closes the active panel; reach Timeline before opening
another. Panels visibly slide from their physical edge. The nominal transition
is 280ms, with at most 50ms advancement per rendered frame; slow rendering can
extend its duration. Editing is suspended during travel. Tap existing controls; hold briefly (180ms)
before dragging. Two fingers scroll and pinch zooms the editor; two-finger
double-tap exits as before. A held browser component dragged beyond screen
center reveals the timeline, where release inserts its track through the same
ordered-row workflow. Desktop layout and input are unchanged.

The iOS touch bridge now supplies the shared workstation controller input instead
of only special-case transport taps. It buffers down/up across slow frames,
coalesces motion, and cancels on overload, interruption and size changes. The
UIKit pass-through text/selection adapter displays existing bottom feedback,
range selection, drag labels, CC picker and Theme/Lightbox settings. It adds no
independent buttons or optical shader changes. The modal keeps ownership until
Apply or Cancel. Current iOS orientation remains landscape.

This is not complete mobile/desktop backend parity: native desktop plugin formats
and their Edison editors, physical iOS microphone capture/monitoring, native
desktop context menus and keyboard-only commands still need platform-specific
work. Phone gesture ergonomics, safe areas, rendering speed and audible playback
require device testing. Local candidate builds use build_ios.py --output-dir to
preserve the prior archive; publication is separate. No new TestFlight upload
is implied by preparing this candidate.

The workstation plays Cardinal’s ChucK interface cues through the listener output while transport is stopped or playing. Set per-cue gains from 0 (muted) to 1 in Procedures/chuck/daw/gains.json before launch. Panel motion starts muted, matching Cardinal.

## Rainbow audio clip preview

Audio tape clips now carry a source-derived stereo waveform layer inside the
existing 3D tape. Six windowed frequency probes determine spectral balance and
concentration. Absolute amplitude shifts a repeating rainbow and its brightness,
with bands across each waveform channel. The bounded 128-column preview follows
source offset and clip duration and remains anchored while panning. This first
version previews raw source PCM, including for clips with playback treatments.
The waveform is drawn as source-derived color on the opaque tape's front face,
with no additional waveform mesh. This replaces the invisible interior layer:
all three authored tape materials have zero transmission, so an interior-only
waveform was never reached by the renderer. Side walls, perforations and the
underside retain their authored material. Stereo amplitude and color sampling
still follow the visible source window. OpenGL/iOS/Vulkan shader validation,
the three timeline scene tests and the retained WebGPU shader validation pass.
A separate production group-gate test fails its host-source signature check
(`source0 fn update(`), before shader validation. A focused Metal/WebGPU compute probe of the generated production shader
confirms nonzero color and coverage on both stereo lanes and no coverage
outside the sampled amplitude, on the underside, on walls or on track handles.
The signed local macOS app is rebuilt. This probe verifies shader samples, not
a captured arrangement or live clip appearance. Treatment mapping and performance
with large arrangements remain unverified. Evidence: output/waveform-fix-20261108/.


## Modeled theatre and FOH computer, November 7

The visible theatre now loads the Blender-authored architecture packet from
Art/Theatre/TheatreInterior.blend, exported by make_theatre.py/make_interior.py.
Physical acoustic wedges, walnut wall framing, stage boards, ceiling beams,
proscenium trim, rails, lights and FOH furniture replace the image-finished room.
The computer now has an inset display, vents, ports, hinged stand, individual keys,
mouse and cables. The phone and outdoor assets were not changed by this work.
See Art/Theatre/README.md for authoring and the intentionally coarse physical
query envelopes. Room plus instantiated hardware: 40,408 triangles; shared room
and appearance budget: 48,000. The initial modeled room had no image textures; the desktop now has the
authored UV walnut material described below.
The legacy image-finished room is retired as visible geometry; its existing
construction is retained for contact/acoustic envelopes. Runtime shading still
uses diffuse colors rather than Blender metallic/roughness materials.

Five focused theatre checks passed, including preserved walking routes, pivots,
landing support and actual runtime mesh integration without image-finish tags.
The final release build passed; source and packaged WebGPU six-frame captures
completed with zero remaining Entities. Native visual review confirms removal
of coplanar ceiling artifacts and face-centroid lighting diagonals. Structural
intent inspection reports zero active mismatches; behavioral obligations outside
these checks remain pending. These short captures are not a frame-rate benchmark.
The local output/macOS/MirrorPrismals.app was replaced with the signed, verified
package after the packaged capture; the previous app was retained beside it.
Evidence and backup path: output/theatre-models-validation/report.json.
Actual native preview: Art/Theatre/TheatreNative_preview.png.

The final build also required correcting three invalid 1.e-6 literals to 1.0e-6
in existing PanelMotion.rs/PanelLayout.rs expressions; their intended values
were preserved. No panel behavior change was introduced by those syntax fixes.


## UV walnut for the FOH table, November 7

The table now carries Blender-authored metric UVs and coordinated 1K color,
normal, roughness, height and AO maps. The native material uses filtered 512-to-1
map pyramids, tangent normal lighting, a nonmetallic GGX satin response and four
bounded UV-relief iterations. Height changes surface sampling, not silhouette,
collision, acoustic geometry or actual depth. The previous plain desktop color
is retired; the modeled beveled shape is retained. Other theatre surfaces have
not been assigned this material in this pass. No Substance Painter is required.

Game and workstation theatre-camera paths preserve the UV payload and use their
actual venue-local camera positions. Normal material buffers are separate from
the existing monitor region, legacy finish tiles and gem storage. The desktop
material receives the existing theatre lamps; environment reflections, shadow
maps and phone-flashlight integration are not added. iOS device checks remain
outside the macOS validation scope. Authoring/channel details and reproducible
shader generation are in Art/Theatre/Materials/Walnut/README.txt and the theatre
README. Validation and the promoted app backup are recorded in
output/table-material-validation/report.json after native checks finish.

Final table material checks: 13 focused checks passed (material channels/layout,
scene shader, venue geometry/routes, camera shader partition and clipping).
GLSL scene/camera variants validated; actual six-frame OpenGL, Vulkan and WebGPU
game captures completed. The OpenGL embedded workstation camera benchmark
completed eight measured frames with zero remaining Entities. The signed packaged
WebGPU six-frame capture passed, and output/macOS/MirrorPrismals.app was updated
with a retained backup. Deep/strict signature verification passed after promotion.
Intent inspection has zero active structural mismatches; this does not discharge
unrelated pending behavioral checks. These finite native checks are not a
performance or listening-quality claim. Evidence is in the table-material report.


## Expansive brown caves and blue Depths, November 7

Upper caves now select the pre-binary Cardinal Perlin proportions over a
1024-metre local radius, with a three-metre roof and a four-metre overlap into
the existing Depths band. Authored descent passages remain. The prior value-noise
prototype remains selectable; no square cave cells or voxel storage are added.

The original atlas selected by Cardinal's circuit breaker supplies the color
reference: warm brown StoneExternal (34) above and dark blue Depths wall (461)
below -20 m. The grey replacement atlas is not selected. Original editable
Blender substances and their albedo, normal, roughness, height and cavity maps
are in Art/Terrain/Rock/CaveRock.blend; RockPreview.png shows both materials.
Native GL, Vulkan, WebGPU and GLES sources bind the rock-map mip pyramids through
the shared material tail after walnut. Nearby terrain gets oriented diffuse
normal relief; height is modeled on Blender wall swatches and retained for later
runtime displacement. No runtime parallax, GGX rock specular or collision relief
is claimed. Stone mechanics remain unchanged; grey is reserved for Nightworld.

The deep surface-net window widens to 72 metres to include the main chamber's
walls. Shoreline-margin conditioning improves mesh agreement with the same
accepted river/cave occupancy. Deep views omit outdoor terrain, theatre and
surface vegetation and fade to darkness at 24..34 metres. This bounds visibility
without presenting the outdoor sky through missing remote cave geometry; a full
underground LOD/visibility system remains future work. Surface entrances retain
the previous 36-metre patch and daylight. Validation evidence is under
output/checks/cave-rock/; gameplay traversal and iOS device performance are not
established by source checks or finite desktop captures.

Cave validation: 28 terrain, seven shared-material/shader and three presentation
checks pass. Both upper and Depths views complete three-frame captures on native
OpenGL, Vulkan and WebGPU. WGSL validation and GL/GLES/Vulkan source shader
compilation pass. Intent inspection has zero active structural mismatches.
These checks do not establish full traversal, phone visuals or device performance.
The packaged application was not replaced by this source pass.


Computer peripherals, November 7

Removed the disconnected rear cable rods and desk stubs. The mouse now has a
rounded palm shell, separate front buttons and a wheel between the fingertips
at the monitor-facing end. Updated the generator, both editable Blender masters,
FohComputer.glb and the runtime computer mesh while preserving other mesh entries.
The computer uses 5,984 triangles within its existing 6,000-triangle runtime
limit; the full source venue uses 43,688 within 48,000. Final Cycles preview
reviewed. The local output/macOS/MirrorPrismals.app computer mesh is updated and
its ad-hoc signature verifies. Its previous packet is preserved under
output/checks/computer-peripherals/packaged-venue-before.json. Native appearance
has not been checked. Focused theatre tests were cancelled while waiting for
another build's Cargo lock; no test pass is claimed.

The bubble piano room retains native mouse callback edges in Play Mirror as
well as DAW-only mode, so a complete click between rendered frames still places
a MIDI note. Callback positions and edit acknowledgments are retained in order;
focus loss and other UI ownership cancel drafts. Regression/build evidence:
`output/checks/embedded-piano-input/`.

## CPU scene preparation optimization, November 8

Body and scene-instance rendering now uses the existing numeric Entity/Get
boundary instead of serializing and reparsing meshes. Instance camera values
are read outside vertex loops. A single bounded venue cache retains normals,
UVs and both orientations of fixed interior lamp illumination, invalidated by
mesh-content changes. Camera projection and exterior/day/night light remain live.
The real-venue regression compares original and cached projection across camera
sides, four day/night fractions and in-place mesh edits. It passed, along with
projection, numeric Entity compatibility and material projection regressions.
Intent inspection has zero structural mismatches; this is not behavioral proof.

Four alternating release WebGPU runs of 180 stationary updates at the theatre
spawn each measured
mean per-run update medians of 57.95 -> 49.97 ms (13.8% less time). Appearance.View
medians fell 32.08 -> 23.97 ms (25.3%). GPU intervals remain about 22 ms, terrain
audio tracing about 8.3 ms and initial scene construction about 2.8 seconds.
The 120-frame embedded workstation comparison remains about 12.1 ms median,
with 23.5 ms p95. No graphics quality or acoustic ray budgets were reduced.
These are short stationary theatre-view observations, not outdoor traversal, project stress or
physical scanout FPS. Evidence: output/benchmarks/current-20261108/optimization/.

## Acoustic traversal, cave bounds and opaque GPU optimization, November 8

Closest-hit terrain audio queries now stop behind a nearer accepted surface or
venue hit while preserving the original sample grid and crossing bracket. Cave
noise evaluations use conservative bounds on their existing capped terms to
skip calculations that cannot change the maximum. Bitwise field and full-grid
hit regressions passed, along with cave/support, material continuity, acoustic
and GPU timestamp checks. WebGPU disables blending only for the alpha-one
scene prefix, retaining the original player/effect pipeline and draw order.
Five fixed native captures with that pipeline off/on are byte-identical.

Four alternating 180-update runs against the previous CPU optimization measured
game update medians 49.61 -> 47.56 ms, terrain audio 8.34 -> 5.52 ms, GPU primary
pass means 23.35 -> 20.35 ms and first Appearance.View 2.84 -> 2.31 seconds.
These are stationary theatre-view observations on M1 Pro/WebGPU, not outdoor
traversal, display scanout or a 60 FPS claim. The embedded workstation stayed
about 11.9 ms median and 22.5 ms p95. No graphics quality or ray budgets changed.

The pixel-equivalent opaque-before-sky experiment regressed GPU time and remains
a diagnostic helper only; production retains sky-first. The explicit
WORMWOOD_SCENE_OPAQUE_PIPELINE=0 alternative restores the prior blended draw for
comparison. Existing DAW baked-art and paused-world policies remain unchanged.
Evidence: output/benchmarks/extra-20261108/final/RESULTS.txt.

## Fused CPU scene preparation, November 8

Rendering reads immutable shared Entity snapshots through the existing Get
route. Terrain transformation, projection, clipping, lighting and material
packing now run per triangle; the same clipping predicate rejects offscreen
faces before fog work. Theatre shading projects directly. A bounded scene
buffer is reused, with the original sky/flashlight order preserved. Reference
checks match projection/terrain bits across motion, light, cuts and caves;
shared snapshot lifetimes, eight-instance scene cleanup, flashlight and native
material compatibility passed. Five fixed native captures are byte-identical.

The controlled M1 Pro/WebGPU crossover used identical runtime assets and only
six CPU implementation files changed: scene View medians 23.31 -> 17.95 ms
(23.0% lower), exclusive scene work 15.38 -> 10.55 ms (31.4% lower), game
updates 45.94 -> 40.47 ms (11.9% lower). These are short stationary theatre
observations, not outdoor traversal or scanout FPS. The repeated embedded
workstation replay measured 1,200 frames per variant at 15.12 -> 15.11 ms mean.
GPU time and cold geometry construction are effectively unchanged. No quality
or ray budgets were reduced. Concurrent edits were isolated in frozen source
and asset snapshots; an identical-executable cached baseline was discarded.
Evidence: output/benchmarks/scene-cpu-20261108/controlled/RESULTS.txt.

Audio clip fades — November 9, 2026

Audio clips now have upper-edge drag handles for nondestructive linear fade-in
and fade-out. The gold native overlay previews the envelope without optical
recapture; release commits one revision-checked edit. Playback (including hosted
track effects) and frozen WAV export apply the same clip-local envelope. The
fields round-trip through Snapshot/PutClip and default to zero for existing
callers. Fade-only Undo/Redo retains up to 32 edits until an unrelated document
edit; general project-file persistence is still not implemented.

Validation: 609 workstation module tests passed (2 ignored), including endpoint,
stereo, source-offset, block-boundary, move/restoration, overlap, atomic rejection,
undo/redo, drag cancellation and actual exported WAV checks. The registered Host
workstation checks passed 149 tests (3 ignored) with RUST_MIN_STACK=16777216.
The default debug test-thread stack overflowed; rerunning with the larger stack
reached the checks. Native overlay drawing was separately rendered and visually
reviewed. These checks do not establish device listening or manual embedded fade
interaction. See Workstation/README.txt and Workstation/ClipFades.rs.

The release was rebuilt and ad-hoc signed at
output/macOS/fades-20261109/MirrorPrismals.app, with the ZIP beside it. Deep
strict signature verification passed. The packaged WebGPU two-frame DAW startup
and eight-frame real embedded game/workstation benchmark passed outside the
sandbox, ending with zero remaining Entities. The sandboxed graphics launches
aborted. Intent inspection reported zero active structural mismatches. Evidence
is under output/checks/fades-20261109. Native device listening and manual embedded
fade gestures remain unverified; the embedded benchmark uses an empty timeline.

## Game frame scheduling overhaul, November 9

The macOS WebGPU game now separates accepted scene preparation from native
presentation. Registered Host routes collect immutable Entity, vegetation,
venue, phone and acoustic inputs. One scene runs on a CPU worker and at most
one more is queued. Pump publishes only the latest completed coherent scene;
View no longer causes a second upload in the same iteration. Native graphics
stays on the owning thread. A bounded 0.75 ms readiness grace avoids needless
repeated frames at the publication boundary. Initial scene construction remains
loading work before the interactive loop.

Terrain projection overlaps the other scene work and reuses its output buffer.
Near/distant geometry replacements build separately and publish together. The
current camera and lighting continue projecting the previous complete geometry
while a replacement builds; view-conditioned detail can therefore catch up after
motion. Accepted world edits invalidate immediately and old-world results are
discarded. There is no mesh/ray-budget reduction. The three-way projection-pool
experiment regressed the measured workload and remains explicitly opt-in through
WORMWOOD_GAME_PROJECTION_PARALLEL=1.

Terrain acoustic batches, immutable acoustic geometry assembly and cabinet
visibility queries run on bounded non-callback workers. Responses use the latest
completed observation; current listener gain/pan still updates on the control
thread. Geometry packets share ownership instead of copying/releasing thousands
of nested JSON values on that thread. Position-driven water collision refreshes
also run off-thread while their accepted coverage remains valid. World edits or
uncovered positions require the matching collision snapshot. Water contacts copy
the existing numeric spatial index instead of serializing and rebuilding it.
Static river/lake extraction is cached by its actual position dependency; camera
turns do not invalidate it. Ocean waves, optics, ripples and parcels remain live.

Digging/resetting no longer request screenshots. Explicit bounded diagnostic
captures use WORMWOOD_GAME_CAPTURE_FRAMES=1,60 with --verify-session. The normal
game pacer sleeps coarsely then spends at most its final millisecond on a monotonic
60 Hz deadline. This does not change callback or workstation pacing.

With --performance, game-frame-timing.json records up to 36,000 complete loop
intervals, render/event pump time, update time, fresh scene sequences, percentiles
and worst frames. Repeated draws do not count as fresh scenes. Paused intervals
remain separately identifiable. Reports include initial-draw stalls rather than
silently removing them.

Player skin updates now carry numeric vertices and share validated immutable
UV/head metadata rather than rebuilding a JSON mesh every frame. Diagnostic trace
writes and rotation run on a bounded worker; slow-call telemetry distinguishes
thread CPU time from elapsed time. The game pacer uses an anchored 60 Hz deadline.

The signed review app is output/macOS/game-overhaul-20261109/MirrorPrismals.app.
Four 600-frame Apple M1 Pro/WebGPU captures measured, after the first two startup
frames, 60.00 loop / 59.80 fresh-scene FPS stationary, and 60.00 / 60.00 during
continuous turning, platform walking, and isolated outdoor turning. The outdoor
check uses a copied asset configuration without the theatre; production assets
keep it enabled. Platform walking reaches a railing, so this is not broad terrain
traversal. Turning kept producing fresh scenes while background mesh builds took
up to 1.80 seconds, replacing the earlier ~1.7-second frozen view.

Post-startup p99 frame times were 16.83–16.95 ms; the worst loop frame was
20.30 ms. The stationary run contained a 34.20 ms gap between fresh scenes.
Cold loading remains, and startup-inclusive first-draw intervals reached 102 ms.
These application timings do not establish physical scanout or an unconditional
locked 60 FPS on every workload or machine.

Five fixed native venue images are byte-identical to the prior app. Conservative
culling omits fully clipped faces only when planar reflections do not need them.
The final focused suite had 57 passes and one legacy direct-spade stress timeout;
that test uses former ShovelSystem routes under test-only permissions, rather than
normal navigation. It also timed out in optimized release and remains unresolved.
Earlier focused suites and full terrain motion/light/edit/cave equivalence passed.
The 120-frame real embedded DAW transition passed and ended with zero Entities,
retaining baked artwork and paused-world caching. Its 960x600 empty-project timing
is not a populated-project or audio-playback stress test. Deep strict signature
verification passed. Intent inspection is structural evidence, not behavioral
proof. Raw reports, limitations and source hashes are under
output/checks/game-overhaul-20261109/.

## Exterior turn-back shutdown, November 9

A captured real session rejected a finite 131099-triangle composed scene while
looking back toward the theatre from its entrance steps. The old 131072-face
limit was insufficient once terrain, venue and rigs were combined. Render/Limits.rs
now owns a shared 262144-face composed-scene bound, used by JSON/numeric validation,
native allocations and descriptor ranges. Water reflection storage allows 524288
faces because one plane can split each input triangle into two. This explicitly
increases the former bound without dropping geometry or reducing detail; main
GPU storage grows from 12 to 24 MiB and optional reflection storage from 12 to
48 MiB. OpenGL uploads remain sized to actual data.

Rejected scenes log their view separately and return a short error, preserving
the bounded Abort/cleanup input. The diagnostic build's initial verbose error
exceeded that input bound; it is not used by the corrected failure path.
Scene timing reports now include maximum completed scene triangle count.
Interactive diagnostic runs can request up to 3600 frames; live cue diagnostics
retain their 600-frame cap. The verify-only exit-back replay covers sprinting to
the entrance steps and turning toward the theatre; exit covers a longer outdoor
traversal and yaw/pitch sweep. Validation evidence is recorded separately in
output/checks/outdoor-crash-20261109/.

Validation of the exterior capacity correction: two bounds tests, six exterior
checks and three scene-pipeline checks passed (overlapping selections). The signed
app accepted 135088-face batches in a 2400-frame entrance turn-back replay, and
a separate 2400-frame real exterior traversal/camera sweep ended cleanly with
zero Entities. Five fixed native images remained byte-identical and the real
120-frame embedded DAW transition passed. The broader exterior traversal measured
59.35 loop / 58.60 fresh-scene FPS, p99 20.20 ms; it is not locked 60. The capacity
fix resolves the captured fatal error, not every remaining performance outlier.
Other backend devices and enabled planar-reflection native stress remain untested.

## Translucent animated MIDI artwork, November 9

The production macOS WebGPU piano now uses offline Cycles frames from the original
BubbleNotes.blend material. The old native uniform-film single-image note remains
a retained compatibility/research artifact; it is no longer the production note
image. Four authored duration families provide18 placement and32 settled film
frames each. Frame interpolation plays the source plop/squash/settling wave; film
continues after geometry settles, independently of selection and editor redraw
changes. Rounded duration shoulders retain their thickness during layout changes.

Transmitted background is separate from reflected light. The offline authoring
command reads premultiplied HDR EXR, grades the reflected radiance before alpha
unpremultiplication, and stores display reflection RGB plus physical occlusion
alpha. Premultiplied blending composes E+(1-alpha)*background. This avoids both
the former gray matte/fixed72% opacity and the dark glints caused by multiplying
reflected light by transmission alpha twice. The original.blend remains intact.

The immutable81,100,800-byte movie bank is loaded once. Runtime only samples and
interpolates frames and uploads sprite metadata; no live optical functions or
background recaptures are added. Stable document note IDs occupy a32KiB dynamic
packet extension after browser labels, with all old field addresses unchanged.
Selected settled ages decode the packed5.35f32 sentinel before subtracting4,
avoiding a round-off reset of the film clock.

The finite film sequence uses a four-frame soft wrap (28/12seconds per loop),
not an indefinitely evolving Cycles simulation. Fixed studio reflections and
illumination remain deliberate. Extra-short/long durations interpolate image
layout between the four source shapes; arbitrary-duration Cycles parity is not
claimed. Other native backends retain their prior optical research paths.

The separately staged review app is
output/macOS/bubble-film-fixed-20261109/MirrorPrismals.app. Native before/after
images, settled-film comparisons, populated embedded DAW/cache checks, timing,
source hashes and test results are recorded in
output/checks/bubble-composition-20261109/.

## Theatre interior materials, November 9, 2026

Inner perimeter and flat proscenium walls now share the exterior dressed
limestone maps, with room lighting and inward/planar normals. Physical acoustic
wedges use a new open-cell graphite texture; native tag 114 samples its albedo
with room lighting, while Blender also evaluates pore bump and roughness.
Both theatre Blender masters, architecture GLB and runtime packet are updated.
Regeneration and Cycles close reviews are in Art/Theatre/Materials/AcousticFoam/.

Validation: 17 theatre tests and the embedded camera shader validation test
passed. Main and workstation WGSL parsed/validated with Naga; OpenGL fragments
validated and Vulkan fragments compiled to SPIR-V. Shader regeneration is
idempotent. Cycles room and close-up images were visually reviewed. Native
appearance and installed application packaging have not been checked.

## CC curve editing, November 9, 2026

CC clips now expose a timeline point editor: click to add, drag a square point
to change time/value, Option-click a point to remove it. The top ten logical
points of the tape remain a clip-move strip. Dragging previews only; release
commits one revision-checked PutClip, and Escape/pointer loss/resize/document
changes cancel. Curves use the existing unlit overlay without artwork rebakes.
The legacy CC clear-clips button and document action are explicitly retired
by author request. Arm and lane/device/parameter selection remain. Automation
recording is not added. Native interaction and installed-app packaging remain
unverified for this source change.

Validation: the compiled current library test binary passed all 615 selected
workstation tests (0 failed, 2 ignored), including CC editor gestures, retired
clear rejection and the shifted parameter-picker button. All 41 arrangement
tests also passed through Cargo. macOS WorkstationControls.m passed clang syntax
checking with existing deprecation warnings. A post-edit intent inspection was
queued behind another build and cancelled; no post-edit structural result is
claimed. No native interaction, audible output or app repackaging was performed.

## Shared workstation content undo — November 9, 2026

The timeline now uses a shared Cardinal-derived content journal with 256 entries,
nested transaction grouping, explicit gesture merging and revision-checked
undo/redo. Clip edits, track mix/routing/order, timing, playlists, committed
capture/imports and drum edits share this journal; immutable PCM remains shared
and history runs outside audio callbacks. Piano-roll and Loopback shortcuts
retain their local histories. Plugin state, frozen-track workflows and mutable
style history are outside checkpoint coverage; unsupported mutations form a
history barrier. See Workstation/README.txt for API and cancellation semantics.

Validation: 642 workstation module tests and the registered Host timeline
undo/redo/cleanup test passed; two asset-dependent checks were ignored. The
complete Host suite and native UI/device audio have not been verified.

Piano roll: ARP in the second toolbar row selects MIDI pattern stamping. Cycle direction, note count (1–32), extension and Arpeggio/Chord with clicks; Shift-click cycles backward. Scale thirds supply Triad/9th/11th/13th; Maj7/Dom7/Min7 explicitly set quality. Chromatic uses major for scale-derived chords. Grid controls spacing; current note length controls duration. Each click is one undoable edit and rejects patterns that cannot fit.

## Ground blueprint crafting — November 9, 2026

Cave jars at three authored candidate sites yield single-use plank blueprints;
only sites with actual cave-floor support and clearance spawn. F opens a jar
and carries its plan; F places it on nearby supported ground. Aim at the sheet
for its requirement prompt, then Shift-left-click beside a physically felled
tree (crown centre within 4 metres). The recipe reserves that exact tree, needs
60 seconds of local work, then consumes tree and sheet into six separate planks.
F picks up and places one plank at a time, sharing the phone/glasses/tool hand
guards. A crafting sheet cannot be picked up while working.

Crafting runs at full speed within 10 metres. Between 10 and 20 metres its rate
is 100^(-(distance-10)/10): 10% at 15 metres and 1% at 20 or beyond. The clock
pauses if its tree is no longer eligible or close enough. Pause/menu/embedded
workstation time does not advance game crafting. Reset/close cancels recipe
Audicles. This first version is session-only: no saved crafting objects, offline
progress, general building editor, or additional recipes. The whole physical
tree, including its stump, is consumed by the initial recipe.

New Blender masters and a reviewed render are in Art/Crafting; metric runtime
meshes ship under Procedures/Assets/Crafting. Production rendering uses diffuse
facet shading, not Blender material parity. Validation and remaining native
interaction obligations are recorded in output/checks/crafting-20261109/.

## Theatre waterpark — November 9, 2026

The theatre entrance now connects by a graded six-metre asphalt road to a
waterpark. Two adjacent Blender-authored smooth fiberglass flumes share a
24.5-metre launch deck and six-flight switchback stair tower. Magenta ends in
an outdoor basin. Lime descends through a sunken portal into a landscaped
underground grotto pool. Pool steps lead to its dry eastern deck; a railed
return staircase and upper landing reconnect to the park promenade. Walk into
either launch mouth to ride; reset, flight and noclip release the ride.

The editable master is Art/Waterpark/Waterpark.blend; the interchange export is
Waterpark.glb in that directory. BuildSupport/Waterpark/build_waterpark.py
reproduces both and Procedures/Assets/Waterpark/waterpark.json. Approximately
30,000 new visible triangles and 21,600 contact triangles use the configured
shared venue/scene budgets. Rendering, stair contact and acoustic traces share
the Blender export. Runtime water uses exact measured basin boundaries and
shallow flume films in the existing native water pass. The authored pools do
not introduce a new swimming solver or simulated pump/parcel source.

A deterministic site survey keeps the grotto floor above untouched natural
terrain; planted earth banks surround the raised park. Its pool and exit are
below the landscaped crown. Cardinal's cyberworld broad S-shaped descent and
launch deck informed the new courses; no voxel world or old terrain code was
imported. Martin and preserved Cardinal sources remain independent.

Validation: all five waterpark checks pass, including real Host configuration,
world-space basin placement, registered body contact, both ride completions,
cancellation, tower/cave/pool stair traversal and road support. Nineteen theatre
and fifteen water regressions pass (four waterpark checks overlap the theatre
filter; 35 unique checks). The executable builds offline. Intent inspection
reports zero active structural mismatches; it does not prove behavior. Five
finite WebGPU frames rendered on Apple M1 Pro; overview, approach, launch,
grotto pool and daylight-stair images were inspected. Evidence is in
output/checks/waterpark/native/ and output/waterpark-validation.json.

The built executable is output/waterpark-target/debug/wormwood. The installed
application has not been repackaged. Manual full-game traversal, gameplay
performance, third-party audio, and phone/device validation remain unverified.

## Experimental physical computer — November 10, 2026

Left-clicking the theatre computer now holds the player at the current pose,
shows the desktop cursor, and projects the live baked DAW onto the physical
monitor glass. Mouse input uses the inverse perspective map. E or Escape
returns to gameplay; AR/VR keep their separate immersive DAW entry. The
physical display requires macOS baked WebGPU. Other renderer configurations
report an unavailable display rather than entering the DAW through the computer.
The real paused game image is retained between UI edits; no lighting is rebaked.

This is an experimental presentation: native window overlays (including some
labels, selection feedback, and parameter pickers) are withheld because their
window coordinates have not been projected. The baked controls and document
input remain active. Native click/drag, visual alignment, resize and release
still require gameplay review; project save/load remains outside this change.

Validation: the normal debug executable builds. Perspective input/image
round-trip, baked shader/no-live-optics, retained game/document/layout, and
failed-entry recovery checks each pass. Intent inspection reports zero active
structural mismatches; this does not establish native behavior. The native
combined smoke check stalled during macOS desktop service initialization after
JACK connection errors and was stopped. Visual/input/device review remains
unverified. The installed app has not been repackaged.


## Sustained traversal benchmark, November 10

The opt-in `WORMWOOD_GAME_REPLAY=area-soak` diagnostic repeats a 1,200-frame
pose route around the church, slide tower, underground pools and theatre. Use
`--performance --interactive planet_surface WebGPU --verify-session --close-after-frames 3600`
with a separate `WORMWOOD_OUTPUT_ROOT`. It requires bounded verification mode,
uses noclip survey poses and retains the existing fixed 1/60-second diagnostic
time step; ordinary player input does not select the route.

Three laps completed in 21 minutes 14 seconds on M1 Pro at 1920x1200. Overall
loop throughput was 2.94 FPS; church circuits were about 4.1 FPS and deep-pool
circuits about 1.5 FPS. Water presentation dominated main-thread preparation,
including repeated mesh/JSON conversion and Entity validation. Lap throughput
fell 2.7%; sampled RSS peaked at 1,568 MiB, with rises and falls across regions.
The longest background terrain build was 36.27 seconds. These measurements do
not establish a leak or the cause of the mild late-session regression.

Native completion, identical repeated camera positions, audio output and zero
remaining Entities were verified. The replay covers about 60 simulated seconds
across its long wall-time run, so longer normal-play accumulation remains open.
No graphics quality or audio budgets were reduced in this baseline run.
Evidence and limitations: `output/benchmarks/traversal-20261110/RESULTS.txt`.

## Traversal CPU optimizations, November 10

Water presentation now carries immutable numeric meshes through the registered
Host routes instead of converting every vertex to JSON and back. Explicit JSON
Entity/Get and graphics adapters remain available. Native water uploads borrow
the validated 96-byte vertex layout; mesh topology, optics and shaders are
unchanged. Appearance existence checks share Entity snapshots instead of
reconstructing and validating discarded JSON. Unchanged material/cave envelopes
retain their previous validation; numeric skin updates still validate every row.
Exterior and deep-cave terrain retain separate accepted cache slots in each
realm (four fixed slots per scene thread), so a transition does not discard the
other domain's reusable geometry. The existing world/profile/deep-view acceptance
checks still apply before reuse; background updates retain their original budgets.

Cave distance queries use a conservative spatial index with the original exact
leaf formulas. Collision requests compare exact numeric keys, retain immutable
static architecture separately from moving material, and reuse snapshots only
inside their covered guard. Snapshot guards use projected surface displacement,
so flight altitude alone no longer forces a rebuild. World edits and architecture
changes invalidate the accepted geometry. Foot-support queries are batched over
the same registered routes. Water-grid and cave-support CPU sampling use three
scoped workers on machines with at least four logical CPUs; set
`WORMWOOD_WATER_SAMPLE_PARALLEL=0` or `WORMWOOD_CAVE_SUPPORT_PARALLEL=0` to compare
the serial reference paths. Graphics APIs and Host dispatch remain on their
original threads. Terrain projection parallelism retains its existing opt-in.

An experimental bounded cross-thread surface-cover cache is available with
`WORMWOOD_SHARED_SURFACE_COVER=1`; it is not a production default because its
throughput advantage has not been isolated. Tests compare colors across threads
and world-profile changes. No model, tessellation, reflection, audio-ray or
collision budgets were reduced by these optimizations.

Final production-default WebGPU replay (M1 Pro, 1920x1200, 3,600 requested
frames/three laps) completed in 130.94 seconds including startup/shutdown:
31.95 loop FPS and 30.95 fresh scenes/s, versus the original 2.94 and 2.66.
Median loop time was 30.01 ms, p95 45.52 ms, worst 187.38 ms. Fresh delivery by
lap was 31.05, 30.91 and 30.89/s. Keeping separate terrain domains improved
fresh delivery from 23.17/s in the preceding optimized three-lap run and reduced
busy scene requests skipped from 1,050 to 75. Peak sampled RSS was 1,776 MiB
with the additional bounded cache slots; this does not establish a memory leak.

The 60 FPS minimum remains unmet. Water Step still takes about 11.6 ms median
and the full navigation update 22.1 ms on lap three; these are inclusive scopes.
The asynchronous terrain builder still has a roughly 10.9-second worst build.
The replay advances about 60 simulated seconds at fixed diagnostic dt, so these
results do not establish long normal-play stability or physical scanout FPS.

Targeted collision, cave-index, batching, water-byte-layout, clipping, material
invalidation, reflection and projection regressions passed. Fourteen fixed
church/slide capture files match the preserved baseline byte for byte; the
embedded WebGPU workstation smoke check passed. Intent inspection reports zero
active structural mismatches; its behavior checks remain pending, not globally
certified. Only macOS was run. Evidence, binary/source hashes and staged native
checks: `output/benchmarks/performance-20261110/`. The local macOS app was rebuilt
and ad-hoc signed; the previous bundle is retained as
`output/macOS/MirrorPrismals.before-performance-20261110.app`.

## Early water view culling, November 10

The water extractor previously ignored camera direction. Gameplay now passes
the actual eye, basis and aspect to a conservative 70-degree view-frustum test.
Radial cell capsules bound the complete possible water-height interval, including
ocean crests and underground drop sheets. Rejected cells are skipped before
water queries; shared corners needed by surviving cells remain sampled. Authored
pools are rejected before tessellation and flume triangles before mesh emission.
Remaining sampling spacing and vertex attributes are unchanged. This changes
presentation only; water simulation, collision, occupancy and audio continue.
View-dependent meshes bypass the old world-centred mesh cache. Incomplete camera
inputs fall back to the full reference path; `WORMWOOD_WATER_VIEW_CULLING=0`
explicitly selects it. Entity slots handle changing water/trough chunk boundaries.

The tighter production-default three-lap replay measured 37.36 loop FPS and
34.90 fresh scenes/s, compared with 31.95 and 30.95 before view culling. Median
frame time fell from 30.01 to 24.03 ms; p95 was 41.67 ms, worst 234.09 ms.
About 71.5% of grid cells were rejected, with 4,842 water sample queries/frame
instead of 16,641. Lap-three median Water.Step was 4.98 ms. Fresh delivery by
lap was 34.77, 35.62 and 34.34/s. Peak sampled RSS was 1,731 MiB. The 60 FPS
minimum remains unmet, and the worst frame did not improve in this run.

This is frustum culling, NOT rock-wall occlusion or connected-air visibility.
Water inside the view but hidden by opaque rock may still be meshed. The existing
GPU opaque-depth discard prevents those pixels appearing, but comes after CPU
mesh construction/upload. Physics collision triangles are not substituted as
visual occluders: their coarser apertures could incorrectly hide visible water.

Eleven targeted visibility tests passed, including bounds at both poles and
every potentially visible reference triangle across 27 water camera/position/
aspect combinations. The native three-lap replay and embedded workstation smoke
check passed; intent inspection reports zero structural mismatches. Dynamic flight
captures are not claimed pixel-identical because accepted background terrain
builds depend on scheduling. Evidence: `output/benchmarks/performance-20261110/`
`WATER_VISIBILITY.txt`. The prior optimized app is preserved as
`output/macOS/MirrorPrismals.before-water-view-culling-20261110.app`.

For fixed screenshot review, bounded `--verify-session` runs with
`WORMWOOD_GAME_REPLAY=area-soak` and `WORMWOOD_GAME_REVIEW_VIEWS=1` hold six
inspection cameras for 240 frames each and use the existing noon override (720
minutes). Views cover the church, slides, upper grotto, two blue-cave viewpoints
and the slide grotto pool. Capture frames 220,460,700,940,1180,1420 and close after
1440 frames. Requested captures are retained as `game-frame-NNNNN.ppm` after the
following native Pump, rather than overwriting every requested image. These
injected cameras/lighting do not run in ordinary gameplay. Actual noon captures,
including matching water-culling-off views, are under
`output/benchmarks/performance-20261110/daylight-review*/`. Cave screenshots show
visible gaps and are review evidence, not a visual-correctness certification.

`WORMWOOD_GAME_REVIEW_FULLBRIGHT=1` additionally selects unlit rock albedo and
disables terrain fog for the below-datum review cameras. It requires the bounded
verified `area-soak` review, cheats enabled and `WORMWOOD_GAME_REVIEW_VIEWS=1`.
The deep-view background becomes neutral gray to distinguish missing coverage
from fog. Geometry, textures, sampling and water culling are retained. This is a
screenshot diagnostic, not a new normal-gameplay cheats lighting rule; regular
cheats still do not disable the 24–34 m cave fade. Native captures are under
`output/benchmarks/performance-20261110/fullbright-review/`.
## Original panel artwork restored — November 11, 2026

The author declined the trial of stretched GemButton_Mini artwork on panels.
That trial is retired; the four main panels again use their original individually
baked panel images.


## Phone spawn, cave visibility and cheat inspection — November 11, 2026

Each new planet game places the phone on a clear spot on the current theatre
desk, opposite the VR/AR glasses. The saved
app selection is restored, while saved held/stowed/absolute-world placement no
longer overrides the new spawn. This intentionally replaces pose persistence
across new games; placement remains continuous within a running game.

R focuses a held phone as before. Session reset now requires enabled cheats,
with a separate explicit initialization marker for application startup. The
pause menu has a Night vision switch under Cheats. It presents full-bright
terrain and slides and lit water without the normal cave distance fade. Turning
cheats off, resetting, or starting a game clears this session-local option.

Deep cave views retain the authored venue geometry, including waterpark slides,
grotto walls and ceilings, and use its existing frustum/depth handling. Rock and
outdoor material lighting suppress daylight beneath the analytical terrain roof.
Water carries per-vertex roof cover and no longer adds unlit scattering and
specular highlights. Its normal deep-view fade matches the 24–34 metre rock
fade; night vision bypasses that fade. Underground rivers supply signed dry-bank
samples to the mesh clipper instead of dropping whole triangles at their banks.
The occupancy and cave-carving queries continue to exclude those dry samples.

The roof-cover approximation does not introduce bounced light or a new sun-ray
visibility solver. The finite cave mesh window remains; night vision is an
inspection aid, not a replacement for missing distant geometry.

Review build and verification evidence: output/cave-fixes-20261111.
Deep views additionally exclude wholly exterior venue faces and surface-water
triangles, while retaining underground faces and water sheets that cross into
the caves. Underground venue shading follows the same distance fade as rock
and water. This prevents the omitted exterior height field from exposing roads
and surface pools through the edge of the finite cave mesh.

Verification: 37 cave-related tests passed with a 32 MiB test-thread stack.
Focused phone-spawn, river-bank, cave-lighting, water-WGSL, deep-water visibility
and reflection-culling checks also passed. Native packaged WebGPU normal and
full-bright 1,440-frame routes completed with zero remaining Entities. The final
three-frame spawn capture confirms the phone on the clear side of the desk.
The final app passes strict signature verification; packaged changed contracts
and water shaders match source hashes. Evidence and limits are recorded in
output/cave-fixes-20261111/validation; captures are alongside it. Full-bright
images still show the pre-existing finite cave-mesh boundary as gray gaps.

Northern forest, sea and ice city — November 12, 2026

Distances use metres (one requested block = one metre), relative to town; north
is -Z. The forest edge slopes northwest from (east 1 km, north 1 km) to
(east -4 km, north 6 km), joining the existing eastern forest. A rectangular
sea occupies north 6–11 km and east -9–21 km. Its 30 km-wide far shore leads
into 30 km of forest, ending at north 41 km. Beyond lies permanent treeless
snow, with a cyan-white faceted ice city centered at north 43 km, east 0.
Those lateral bounds and city offset are implementation choices for the
requested layout. Existing western mountains and southern regions are retained.

Regional water uses the same basin height, signed shore distance and shore
directions for physics and rendering. The northern sea participates in the
bounded horizon mesh and disables static water caching when nearby. Snow is
a ground substance rather than white turf; excavated snow selects the existing
snow material. The city uses the existing bounded venue and collision path,
with stylized opaque icy walls and luminous glass facets. This does not add
city melting, new interior gameplay or optical ice transmission.

Validation: four northern checks and nine regional/city checks passed, including
physical northern water, bounded horizon geometry, continuous banks, polar snow,
ice-city location, existing regional cities and their building contact. Intent
inspection reports zero structural mismatches. The native WebGPU review was
started outside the sandbox but stopped after more than two minutes without
producing its first forest image. Native appearance, walking/listening and
installed-app packaging remain unverified; no frame-rate claim is made.

## Right-panel bus labels — November 12, 2026

Removed the duplicate grey bus-name overlay and its unused control-packet name
field. Initial and newly created matrix/mix buses retain the same white routing
and mute engravings. Matrix L/S/FF/R remain routing choices, not display names.
The separate grey name presentation is explicitly retired by author request.

Validation: the track_io selection passes 20 checks; one broader mobile workflow
check fails at visible transport capture/press. The default test stack overflowed
in that host check; rerunning with RUST_MIN_STACK=16777216 exposed the assertion.
Intent inspection reports zero structural mismatches; this does not prove
behavior. Native appearance and installed-app packaging remain unverified.

## Native User Audio import — November 12, 2026

The macOS right I/O panel now has an IMPORT AUDIO button fixed at the bottom.
It opens the OS file chooser with multiple selection enabled. Selected files
are copied into the existing editable USER library (or WORMWOOD_USER_ROOT),
alongside the test tone; the Audio browser refreshes after import. Existing
filenames are retained and new duplicates receive numbered suffixes. This
library operation does not create tracks or clips.

WAV files are copied. AIFF, MP3, M4A, AAC, CAF, FLAC and WAVE selections use the
Mac's afconvert decoder to produce WAV, where that decoder supports the input.
A background worker validates the result against the current engine's sample
loader: mono/stereo, supported sample rates and PCM formats, up to 8 MiB per WAV.
Unsupported files report an error without publishing a partial library entry.
The existing requirement that sample rate match playback when dropping a file
onto the timeline remains. Chooser cancellation changes no library files;
closing the workstation cancels remaining worker operations. Original files
are not modified. The button reuses persistent baked artwork.

## Continuous free clips and browser audition — November 12, 2026

Unassigned audio clips now use continuous vertical positions. The legacy `lane`
field accepts a fractional coordinate in 96-point units; existing integral
positions remain readable. The arrangement no longer constructs synthetic tracks
for them, so free clips have no track handle or input-monitor traffic light.
Dragging a free clip onto a writable audio track assigns it to that track; free-space drags retain its unassigned identity.
Dropping a new sample onto a real audio lane still assigns it normally; explicit
MoveClip requests can still attach/detach clips. Source offsets, routing, gain,
fades and undo/redo remain document operations.

A vertical ruby zoom/pan bar sits at the left of the arrangement, beside the
track handles. Drag horizontally on it to zoom vertically; drag vertically to
pan. The original horizontal zoom bar still controls time. Vertical navigation
is view-only, uses pointer anchoring, and transforms the existing baked artwork.
The prior synthetic free-row presentation is retired; no live optics were added.

Click a WAV item in the browser to audition it; click it again to pause, and again
to resume. Selecting another WAV replaces the audition, and the end of a file
stops it. Leaving the workstation clears it. One bounded background decoder
feeds an independent stereo queue; the device callback only consumes prepared
frames. Preview resamples to the output rate and does not change the arrangement
transport, recording input or physical cabinet/microphone routing. It uses the
same supported WAV formats and 8 MiB file limit as the existing importer.

Validation: 89 focused arrangement, channel, timeline, browser, registered import and preview tests
pass, including fractional-position undo/redo and independent preview cursors.
Source intent inspection reports zero structural mismatches; behavioral
obligations outside these checks remain unverified. A packaged four-frame native
WebGPU free-clip diagnostic completed with zero remaining Entities, and its
capture was reviewed. The final package also passed the real theatre-computer
embedded transition and its finite benchmark. This is not a sustained audio
listening test. Test bundle:
`output/macOS-free-canvas-20261112/MirrorPrismals.app`; evidence is alongside it
in `verification` and `output/free-canvas-final-tests.log`.


ADD PLUGINS, directly below IMPORT AUDIO, manually adds selected macOS VST3,
VST2, CLAP and Audio Unit bundles into USER_VST3, USER_VST2, USER_CLAP and
USER_AUDIOUNIT under the same USER root. It preserves bundle resources and
links, rejects existing filenames, and refreshes the user catalog. Discovery
continues to exclude system plugin folders. Compatibility and vendor licensing
remain the responsibility of the existing format host; copying a bundle does
not guarantee that its processor can load. Active insert instances survive a
catalog refresh.

## Spectral timeline clips — November 12, 2026

DAW Settings → Workspace now offers Clips: Spectral / Waveform. Spectral is
the default for new and existing preferences without this field; Apply saves
the choice, Cancel discards edits, and Reset workspace restores Spectral.
Decoded audio clips use the existing processed-source spectrogram analyzer,
cached by document revision and visible clip set. A bounded 32 × 16
log-frequency thumbnail uses the existing tape display payload and unlit
shader; no artwork baking, lighting or audio callback work is added. Low
frequencies are at the bottom, and brighter warm colors mark stronger energy.
The waveform presentation remains available. Read-only master-mix previews
retain their existing waveform. Native appearance remains to be reviewed.

## Properties panel lettering — November 12, 2026

The left-panel Properties page now uses the same persistent baked engraved
lettering as Browser, including uppercase bright, heavier glyphs. Native
selection faces remain, with the duplicate native text layer suppressed.
Validation attempts were blocked by an existing Cargo build lock; native
appearance remains unverified.

Validation: 21 focused settings, timeline, spectral-analysis and baked-shader
checks pass. Intent inspection reports zero structural mismatches. Native
appearance and installed-app packaging have not been verified for this change.

Validation for manual imports: 11 import-related tests pass, including both
button actions, WAV/AIFF handling, duplicate protection and plugin bundle link
preservation. The native custom-USER catalog probe passes repeated refresh and
active-insert retention. The signed package passes 362 source-asset comparisons,
zero intent structural mismatches, and a 90-frame WebGPU DAW check. Settled
button placement was visually reviewed. Physical file-chooser interaction and
arbitrary third-party plugin execution remain unverified. Evidence is under
output/manual-import-native; test bundle: output/macOS-manual-import/MirrorPrismals.app.

## Clip interaction and display quality — November 12, 2026

Free audio clips now attach when dropped on a writable audio track, including
an assigned preview while dragging. Free-space movement remains continuous.
Double-clicking free audio opens the waveform editor, and clicking empty free
canvas seeks the transport. The previous always-unassigned drag policy is
retired by the author's request.

Option-arrow panel cycling skips timeline-bar geometry while expansion hides
or collapses the timeline. The previously failing registered shortcut test
now passes for all four panel directions.

Clip spectra retain all 128 by 32 analyzed intensities, with smooth sampling,
rather than reducing to 32 by 16 cells. Display data has its own bounded GPU
tail, preserving immutable model addresses. Tape artwork is rebuilt offline
at 1024 square instead of 192 square; interaction still only composites images.
The larger tape image adds about 31 MiB of resident artwork data.

Desktop pointer drags switch to GLFW virtual coordinates at the window edge.
The macOS edge bubble reports the offscreen direction and distance. Release
restores the visible cursor at the boundary. Captured theatre-computer gestures
extrapolate their projection beyond the display rather than losing the pointer.
Native mouse feel and the installed app still require review.

Validation: 27 focused free-canvas, projected-pointer, all-direction panel,
timeline spectrum, baked-art and shader/GPU-layout checks pass. Native tape
asset baking completed and its 1024-pixel result was inspected. Intent inspection
reports zero structural mismatches. Six broader browser/range integration checks
fail; the browser-creation failure also reproduces with the new clip interaction
changes removed. Evidence: output/clip-quality-20261112/verification. Native
mouse interaction and installed-app packaging were not validated.

### Mirror / Prismals package split (November 12, 2026)

The base application is now named Mirror. The optional Prismals project lives in
`../Prismals` and builds its own native package. Mirror retains the DAW, theatre,
equipment, and continuous outdoor landscape; the town, waterpark, regional cities,
Nightworld assets, and character controllers belong to Prismals. Mods are disabled
by default. See `Modding/README.md` and the sibling project's README for build,
selection, ABI, and current adapter boundaries. Existing user data keeps the
`MirrorPrismals` directory for compatibility; the visible app name is Mirror.

The preceding DAW fixes and GPU upload-drain memory fix remain in this build.
The new package split needs its own native validation; earlier bundle validation
applies to the earlier bundle, not automatically to this refactor.

Validated Mirror/Prismals test bundle: `output/macOS-mirror-mods-20261112/Mirror.app`.
Prismals is installed as an independent package and starts unchecked in Mods… .
The exact bundled engine passed base/modded world construction, native System
registration, source-intent inspection (zero structural mismatches in both modes),
4 DAW WebGPU frames and 8 frames of each world with zero remaining Entities.
103 DAW regression tests, 12 Prismals unit tests, and 2 loader tests passed.
45-second DAW memory probes exited cleanly: final-20-second RSS was 687–697 MiB
without Prismals and 932–942 MiB with it (peaks 902 and 1170 MiB). This is a short
observation, not a long-duration soak. See `output/mirror-mods-verification.json`.

Mirror iPhone TestFlight upload (November 12, 2026): build 0.1.0 (2508.3.0)
was compiled, archived, signed, and accepted by App Store Connect. Apple reported
that processing had started; tester availability was not independently verified.
The update preserves org.salamander.mirror-prismals.ios so it updates the existing
TestFlight app, while its visible name is Mirror. This is base Mirror; the
macOS Prismals dynamic library is not included. The iOS renderer was brought up
to date with shared WebGPU construction, theatre monitor textures, and authored
table camera/light uniforms. Desktop-only tree diagnostics remain desktop-only.
Both archived DAW loader checks and all 161 required graphics export checks
passed; 263 production artwork files matched their source copies. No phone
gameplay/device rendering was tested. Apple accepted the upload with a nonfatal
missing-dSYM upload warning, so complete crash symbolication is not established.
Evidence: output/ios-mirror-mods-20261112 and output/releases/2508.3.0.

Phone startup follow-up (November 12, 2026): the user reported build 2508.3.0
crashing in Play and DAW-only on all graphics backends. Its mobile launcher used
a default GCD worker although compiled entry plus Host construction reserves
2,615,632 bytes of nested stack. A macOS native probe measured the old GCD stack
at 536,576 bytes; the paired phone was unavailable, so its crash log and actual
thread stack could not be retrieved. This is a concrete startup stack mismatch
consistent with the report, not a device-confirmed crash diagnosis.

The launcher now uses a named 16 MiB NSThread for every session, checks its actual
stack before entering Rust, and writes mobile-session-startup.json. Packaging
checks the compiled arm64 startup frames against the budget. Rust line-table
information and matching dSYMs are now generated and included explicitly in the
archive; CMake had left them in its configuration build folder.
Build 0.1.0 (2508.3.39) passed stack, symbol UUID, code-sign, 161 graphics export,
and both packaged DAW loader checks. Apple accepted the upload and reported it
processing, with no missing-symbol upload warning. Phone runtime and tester
availability remain unverified. Evidence: output/ios-startup-stack-fix-20261112
and output/releases/2508.3.39. Only regenerable target/debug/incremental cache
was removed after a disk-full archive failure; source and app bundles were kept.

## Overview side-panel alignment — November 12, 2026

The initial outer-margin widening was rejected by the author. Shift zoom-out
must retain normal scaled side-panel widths and translate the left/right panels
to their respective screen edges. It must not widen them or activate the
three-quarter expanded view. The rejected widening is replaced by translation;
normal-scale layout remains unchanged.

Validation: 16 panel-layout checks pass, including normal scaled widths,
open-panel screen containment and swapped sides. The replacement signed app
completed 604 native WebGPU frames and exited with zero remaining Entities.
The final overview capture shows the normal-width left panel aligned to the
screen; the replay retracts the right panel in this phase. The previous widening
bundle and ZIP were replaced. Corrected app: `output/macOS-overview-fix-20261112/Mirror.app`.
Evidence: `output/macOS-overview-fix-20261112/alignment-verification`.


### Retracted overview follow-up

The prior translation fix still used the normal offscreen position for a
retracted side panel. Overview now exposes both complete narrow side panels
at the physical screen edges regardless of their ordinary slide state. Only
explicit overview-hide parks a panel offscreen. Widths and normal-scale slide
behavior are retained. This corrects the earlier open-panel-only verification.
Validation: 16 panel-layout and 6 overview/navigation checks pass, including
retracted/open width, onscreen placement, explicit hide and restored placement.
The signed replacement app completed 604 windowed WebGPU frames with zero
remaining Entities. Its final capture shows the retracted right panel fully
visible at the right edge and the normal-width left panel at the left edge.
All 337 comparable packaged source assets match. A fullscreen attempt lost its
drawable after four frames and is not counted as a pass. The corrected app is
`output/macOS-overview-visible-20261112/Mirror.app`; evidence is in its sibling
`verification` directory.
