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

Panel size cycle — November 12, 2026

At native desktop zoom, Option/Alt plus any arrow cycles that physical panel
through normal, three-quarter extended and fullscreen, then back to normal.
This includes the top transport. The intermediate view uses 75 percent of
viewport width for left/right and 75 percent of height for top/bottom, anchored
to the panel's current physical edge. Escape restores normal directly. Saved
open/size preferences and mobile navigation remain as before. Artwork stays
baked; the new sizes use the existing composition and motion path.

Right-panel controls and browser lettering — November 12, 2026

Numbered right-page selectors use 43.2 px square targets, matching the mini
switch height. Their labels and the page-two switches use the existing bright,
bold engraved overlay above the baked gems. Settings-page labels now sit over
independently scaled persistent mini-ruby backgrounds; existing actions and
label placement are retained. The left browser uppercases display labels before
font measurement and uses the same bright engraved overlay. Source names and
search strings are unchanged. No runtime optical work or audio changes.

Validation: five selector/retained-packet checks and the live browser engraving
check pass. The development build succeeds; intent inspection reports zero
structural mismatches. Native visual review remains unverified: the default
stack overflowed at startup, and a larger-stack retry stalled after macOS
window-service connection errors. The installed app has not been repackaged.

Overview side-panel travel — November 12, 2026

Left and right panels now anchor their slide endpoints to the centered design
canvas during Shift-scroll overview. As they open, their widths also grow
outward across the overview margins to meet the full-width bottom panel.
Retracted panels remain beside that canvas;
explicit overview hiding moves them to the screen edges. Native-scale geometry,
corner ownership and upright swaps retain their existing behavior. This corrects
the inherited outer-window anchors, which put overview side panels offscreen.
Only shared layout transforms change; persistent baked artwork is reused.

Validation: all 14 panel-layout tests pass in the full library test build,
including native bounds, overview swaps, intermediate side travel, explicit
hiding, gestures, mobile and full views. An isolated layout/PanelMotion build
also passes 18 tests. Native visual review and installed-app packaging remain
unverified.

Clip editor face layout — November 12, 2026

Waveform, Warp and Spectral use the bottom gem's planar face width instead of
retaining the mixer navigation gutter and two control rows. Page selectors,
editor tabs and editing buttons share a single row on the upper bevel. The
waveform zoom/loop strips remain at the top of the display face. Drawing and
pointer handling consume the same layout rectangles. Baked artwork and audio
processing are unchanged.

Validation: 22 bottom-panel tests pass in an isolated compilation of the source
modules, including face bounds, button separation, waveform bars and spectral
selection. The unrelated mixer-bus integration test was excluded from that
harness. The existing executable's intent inspection reports zero structural
mismatches. Full app rebuild and native visual review remain unverified.

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

Plugin target gem row — November 11, 2026

Bottom page two (Plugins) now has a second row of numbered 22-point ruby gems
beneath its page selectors. Click a gem to select an audio track, MIDI track or
Mix Bus directly in mixer order. The selected target uses the same pressed white
ruby artwork as the page row. Previous/next plugin-track arrows are retired.
Wheel over the second row reveals further groups of three; external target
selection reveals its group. Empty documents show no target gems. Existing
press-origin/release-inside and cancellation ownership remain in effect.
This reuses persistent baked artwork and introduces no lighting refresh.
Validation: all 49 bottom-panel checks and the baked shader/no-live-optics
check pass. Normal executable builds; intent inspection has zero structural
mismatches. Native appearance and installed-app packaging remain unverified.

Timeline mixer mirrors — November 11, 2026

Right panel page two now has LANE METERS, LANE FADERS and LANE ARMING
below KEYBOARD. Each switch independently shows a column beside timeline lanes;
all default off. These are local session view preferences. Audio and MIDI parent
tracks use the existing bottom-panel gain law and the same smoothed, routed
stereo meter readings. Crosswalk arming cycles the existing track arm modes and
uses the existing authored palettes, including recording blink. CC tracks and
expanded comp rows keep their spacing without duplicate controls.

Lane faders edit live through SetTrackMix and retain capture through their own
accepted gain edits. Escape, pointer loss, resize, visibility changes and external
document edits cancel capture. Track identity survives reorder. Enabled columns
reserve space outside elapsed time for clipping, seeking, editing and ruler math.
The macOS baked compositor reuses shipped artwork; no live lighting or asset
regeneration is involved. Timeline hardware has its own bounded packet, so bottom
panel scrolling/visibility and capacity do not consume its records. Reference
optics and phone appearance are not extended by this production compositor work.
Validation: 52 arrangement tests, 4 keyboard/page tests, 3 lane layout/packet
tests and the baked shader validation/no-live-lighting test pass (60 total).
Native visual review and app-bundle packaging remain pending.

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

Fixed theatre lighting automation — November 9, 2026

Add a CC track. Its target lane selector cycles NONE -> WORKSPACE -> audio/MIDI
lanes. Select WORKSPACE; click the device selector to choose LIGHT 1 KEY LEFT,
LIGHT 2 KEY RIGHT, LIGHT 3 BACK LEFT, LIGHT 4 BACK RIGHT, LIGHT 5 WASH LEFT or
LIGHT 6 WASH RIGHT. Use the existing parameter picker for INTENSITY, RED, GREEN
or BLUE. Each CC lane controls one parameter; use several lanes for a color fade
plus a dimmer envelope. Draw and edit CC clips with the normal point editor.
All four parameters use normalized 0..1 values. Fixture intensity scales the
fixed Cardinal attenuation up to 6. A zero dimmer removes fixture illumination;
the room and low stage ambient remain. The lens display retains a small colored
idle indication. RGB zero is black. No hosted plugin is needed for these targets.

Turn on THEATRE output to select the theatre camera and PA. In the embedded DAW,
the camera previews lighting against the actual room as the transport plays.
Pause holds the current song position; rewind, seek and loops evaluate curves at
the audible transport cursor, independently of the audio preparation queue.
Curves are linear, first active clip per lane, last active lane per parameter.
Clip intervals are half-open. Gaps and removed/disconnected curves use each
fixture's authored defaults; they do not retain stale values from a previous seek.
Lighting does not process audio, generate sound or change exported WAV buses.

The theatre camera uses the actual venue-only material/geometry pass for lighting
rehearsal. It does not rerun outdoor water, vegetation, reflection or shadow passes.
Identical lighting and observer packets retain the completed theatre image.
Other cameras retain the normal paused game image. All gem artwork stays baked.

Audio waveform page and clip gain — November 9, 2026

Select an audio clip and use the bottom panel's fourth page, or double-click
an audio tape to open it. WAVEFORM shows the source envelope. GAIN -1/+1 edits
clip gain in 1 dB steps; the dB readout resets to unity. Gain is independent of
the track fader and applies before track processing and routing.
Drag the waveform to select a range. TRIM keeps that range; SPLIT uses its
anchor. These edits require warp to be disabled; splitting at an endpoint is
disabled. Existing fade-ramp split restrictions still apply.

WARP exposes the existing general warp engine: toggle it, select a position
and ADD MARK, drag an interior marker, or select it and REMOVE. Endpoints stay
fixed. PITCH -1/+1 changes transposition within -24..24 semitones. Add/remove
are disabled without a valid position or interior marker. Warp uses the shared
musical clock; it does not create a default project tempo. This is the existing
ported warp engine, not a complete implementation of Ableton's warp modes.
Edits stay on the control side and use the persistent baked panel artwork.

CC editor update — November 9, 2026

CC point editing and hosted-plugin target selection now exist; see ARRANGEMENT.txt
for gestures. The legacy CC C/clear action is retired. Older milestone statements
below about missing CC targeting/editor connections are historical.

Audio-file browser, November 8

The current macOS desktop left panel imports matching-rate WAVs through the
Audio category or USER_SAMPLES under All. Drag onto an existing audio lane.
See the current root README for directory resolution, limits and validation.
Older statements below about file import describe their historical milestones.

Free-time audio workstation backend

Focused backend port, October 30

The document now has audio, MIDI/piano-roll and CC tracks in explicit insertion
order. Clips also retain insertion order; a later audio clip replaces earlier
material where they overlap on the same track, matching Cardinal's comp playback.
ReorderTracks changes mixed track order without changing track identities.
PutTrack renames an existing track without resetting its mix or CC binding.

Audio and MIDI tracks route their two channels independently to OFF/L/S/FF/R.
Defaults are FF/FF. Same-bus channels sum without averaging. Four mono export
buses remain ordered Left, Sub, Front Fill, Right. The stereo master is only the
listening fold: [L + 0.5*(S+FF), R + 0.5*(S+FF)]. Export always precedes this fold
and any listening master gain. Neither implicit clipping nor normalization is
applied. Mute-on clears solo, solo-on clears mute; solo selection spans audio
and MIDI. CC tracks have control targets rather than audio routes.

The document operations and bounded PCM renderer run on the control side. The
shared native transport selects the cardinal_sine instrument explicitly, and
file export now uses that same renderer. RenderRange alone does not start a
device. File decoding/resampling and nested comp editing remain separate work.
The current flat composition preserves source references and offsets; it does
not claim nested comps exist.

The AI tempo model remains deferred. Its input will be the stereo listening
master. The older spectral-flux experiment below remains retained separately;
rendering and snapping never invoke it or depend on its findings.

The canvas uses elapsed seconds for placement and playback addressing. It has
no project BPM, bar ruler or global musical grid. Each MIDI piano roll behaves
like an independent instrument with its own adjustable BPM and meter. Its notes
use local quarter-note positions. Moving a clip preserves that clock; changing
its BPM preserves note positions in quarters and changes their elapsed times.

Rhythm observations come exclusively from the post-mix MASTER BUS. Individual
recordings, audio clips and MIDI clips do not generate their own canvas guides.
Regular regions describe the audible result; they do not warp audio, move clips
or change MIDI clocks. A pulse estimate alone does not determine a time signature.

All visible workstation elements are intended to be 3D models, including panels,
clips, notes and rhythm guides. This source foundation adds no placeholder UI. Panel
UVs are coordinates on an actual 3D control surface, not a second UI renderer.

WorkstationDataSystem is a layer-two non-Audicle API, registered for explicit
control-thread calls. It has no startup steps. The initial blank panel scene now uses AudioWorkstationSystem in layer five
and WorkstationPanelSystem in layer four; see PanelScene.txt. Further editor
actions and 3D content still need their application/presentation connections. None of these allocating JSON operations
may run in the audio callback. The document API alone does not launch a workstation window.

Document API

Snapshot takes Unit and returns revision, tracks, clips, master rhythm regions
and visualization (null until an observation is explicitly published).
All edits require document_revision from that snapshot and return the new revision.
Every edit invalidates old master analysis because the audible mix may change.
Names/IDs are stable caller-supplied strings of at most 128 bytes. Capacity is
64 tracks, 512 clips, 2048 notes per clip and 8192 notes across the document;
clip placement and source extents are bounded to 24 hours. No persistence format
or old Cardinal session import is established here.

PutTrack: id, name, kind (audio, midi or cc).
PutClip audio: id, track, kind, start_seconds, source, source_offset_seconds,
duration_seconds. The source is an opaque identity; this API does not open files.
PutClip MIDI: id, track, kind, start_seconds, bpm, length_quarters, notes;
optional meter {numerator,denominator}, defaulting to local 4/4.
Each note: id, quarter, length_quarters, pitch (0..127), velocity (0..1).
PutClip CC: id, track, kind="cc", start_seconds, duration_seconds, points.
Each point has offset_seconds and normalized value. Finite points clamp to the
clip duration and 0..1, then sort by time/value. At most 2048 points per clip and
8192 across the document. Curves interpolate linearly, hold endpoint values,
and return 0.5 when empty, matching Cardinal. Clip activity is half-open [start,
end), choosing a consistent boundary from the old consumers' mixed conventions.
PutClip also replaces a clip with validated edited content. Overlaps are allowed.
RemoveClip: id. MoveClip: id, start_seconds. SetMidiTempo: id, bpm (1..1000).

SetTrackMix: id, optional gain (linear 0..16), mute, solo, output_bus_l,
output_bus_r. Route strings are exactly OFF/L/S/FF/R. Contradictory mute+solo
enable requests and invalid partial patches fail without changing any state.
SetCcTarget: id, target=null to disconnect, or {track,parameter:"gain"}.
This built-in target is the explicit adaptation for the focused port; Cardinal's
hosted-plugin parameter adapters are not imported. Normalized CC uses Cardinal's
fader law: <=0.02 is silence, then -60 to +5 dB logarithmic gain. Active automation
replaces the manual gain. Gaps return to manual gain. First active clip per CC
track wins; the last active CC track in track order wins a shared gain target.
ReorderTracks: ids, containing every current track exactly once.

SnapMidi: id, proposed_start_seconds, tolerance_seconds (0..1).
It is a query. It compares actual note onsets in other MIDI clips and returns a
proposed clip start, the two matched notes and revision. The controller converts
its screen-space snap radius to seconds using current zoom. Commit with MoveClip
and the returned revision. Neither clip BPM changes. There is no grid snapping
on the canvas, and rhythm guides are not automatically snap targets.

SnapClip: id, proposed_start_seconds, tolerance_seconds (0..1), optional anchor
"start" (default) or "notes" (MIDI only). The source start or its actual notes
align to actual MIDI note onsets on other tracks. Audio clips need no detected
tempo or transient analysis. Same-track clips, self, note ends, clip boundaries,
and inferred/grid positions are not targets. The old SnapMidi API is preserved.

Decoded sources and range rendering

PutSource: id, sample_rate (8000..192000), channels (1 or 2), interleaved samples,
document_revision. This is bounded decoded-PCM ingestion, not a file decoder.
One request accepts at most 393216 finite values within +/-64; storage is bounded
to 256 sources and 16777216 values total. A source replacement is atomic and
invalidates stale document revisions. Large-file streaming is not implemented.

RenderRange: document_revision, sample_rate, start_frame, frames (1..4096),
optional master_gain (0..16, default 1), analyze_visualization and
publish_visualization (booleans, default false). It does not change the document
revision or clock. Default renders return visualization null and do no source
analysis allocation or FFT work. Analyze-only requests are pure; explicit
publication retains only the small source observation returned by a successful
render. Invalid flags and failed renders leave prior observations unchanged.
Decode/resample before ingestion; mismatched rates, absent sources, and source
extents beyond available PCM fail explicitly. Frame positions round to the
nearest audio sample. There is no musical grid. Split ranges preserve PCM and
absolute MIDI event timing. Note-off precedes note-on at a shared frame.
Audio/CC region duration rounds independently of placement, so sub-sample moves
do not change the source extent. Ranges may extend past the 24-hour document
limit to flush final MIDI note-offs; overflowing the u64 sample clock is rejected.

Returns lsffr (one four-channel frame per output sample), master_stereo,
midi_events, active_notes_at_start, and track_mix with per-sample gain envelopes,
audibility and route destinations. Export data is lsffr exclusively. The master
gain changes only master_stereo. With no instrument selected, MIDI returns its
schedule only and audio_complete is false when an audible piano-roll track
contains notes, even outside the range because external tails are unknown.
With instrument:"cardinal_sine", the bounded built-in sine/ADSR renderer supplies
MIDI audio, held notes and 120 ms release tails at the requested absolute sample
clock. This is the selection used by native transport and export. On macOS a loaded native instrument replaces that sine on its track; hosted
effects follow it before gain/routing. See Audio/Plugins/README.txt. Native
plugin ranges retain DSP history and therefore are stateful. RenderRange itself always reports no device connection.
All JSON allocations and source ownership changes happen outside the callback.


Four-stem file export

In the macOS workstation, Cmd/Ctrl+Shift+E opens export settings. The shortcut
retains a native key-press edge so a quick press/release between frames is not lost. Choose an
elapsed start and end in seconds, the audio sample rate, four names and a parent
folder plus a new export-folder name. Export creates four mono 32-bit IEEE float
WAVs in L/S/FF/R order; default names are master_L, master_S, master_F, master_R.
F is Cardinal's filename for the Front Fill bus. The selected interval is
[start,end), rounded to samples, with no implicit tail appended. Extend the end
when a MIDI release should be included. The listening master and environment
sounds never enter these files. No clipping, normalization or stereo bounce is
applied. Existing folders/files are never overwritten.

A job freezes document structure, source PCM, routing and automation at Begin.
Edits and live playback can continue independently. Export advances at most
4096 frames per control-loop iteration and keeps only one output block in
memory. Cancel, closing the export window, and leaving the workstation cancel
unfinished work. Completed files remain. Progress reaches completion only after
all four files have been written, flushed and published successfully. Failure
and cancellation remove only files owned by that job; cleanup failures are
reported. The four final paths are published individually, not as a single
atomic filesystem transaction.

ExportBegin takes document_revision, sample_rate, start_seconds, end_seconds,
destination (absolute NEW folder), and optional stem_names (four strings, at
most 48 characters). Reserved filename punctuation becomes underscores, and
names must remain distinct after sanitization. It returns a job_id and report.
ExportStep and ExportCancel require that job_id; stale tokens cannot affect a
replacement job. ExportStatus takes Unit. WorkstationExportSystem exposes
Begin/Step/Status/Cancel/Close as the layer-five lifecycle and Open/Pump for UI.

Source rates must match the selected rate; no resampler or file importer is
added. MIDI uses the same cardinal_sine built-in as transport, with its existing
128-voice-per-block bound. Documents with loaded native plugins currently reject
export explicitly: a separate offline plugin instance is required, so plugin
processing is never silently omitted. Four WAVs are produced even for silent buses. RIFF32 size limits are checked before files
are created. The dialog is macOS only; the typed workflow remains independent
of a graphics/audio device. Native UI and file work run outside audio callbacks.


Export validation, October 30: the shared build passed 194 workstation tests,
0 failed, 1 ignored; desktop and packaged startup checks passed, with zero
active structural intent mismatches. All 21 export data/writer/Host tests pass,
including real four-file contents, frozen edits/CC, MIDI block-boundary release,
failed writes, non-overwrite, cancellation, stale tokens and parent ownership.
The native Cocoa dialog separately passed 151 assertions and visual inspection.

A matching frozen native app was then exercised through real keyboard and Cocoa
controls: N opened the roll, Return placed a note, Cmd+Shift+E opened Export,
the parent folder was chosen, and an eight-second export reached Completed.
Independent RIFF parsing found four mono float32 WAVs at 48000 Hz, exactly
384000 frames each. The default-routed MIDI note produced 11759 nonzero samples
only on Front Fill (peak 0.3112241); L/S/R were silent. A second 600-second export
was cancelled; its unfinished directory was removed and completed files retained.
This proves the keyboard-to-file workflow, not device audio or plugin export.
Evidence: /private/tmp/mida-export-keys-16ijh5gt/report.json and
/private/tmp/mida-export-keys-16ijh5gt/interactive-export-report.json.
The folder picker sometimes returned focus to the main window during this run;
reopening Export retained the chosen destination. Follow-up focus correction
and its validation are recorded separately below when complete.

Earlier 125/13 and 174-test attempts encountered transient missing assets/module
inputs and a copied-binary/runtime mismatch. Those startup/build blockers are
superseded by the matched 194-test build and actual walkthrough above.

Source stereo observations

RenderRange with analyze_visualization:true or publish_visualization:true returns
visualization with revision, sample_rate, start_frame, end_frame, sequence and
tracks. An analyze-only pure result has sequence zero. Publishing
assigns a monotonically increasing sequence and exposes the same observation in
Snapshot; no PCM is retained there. Any document edit clears this observation.
This is range analysis of actual rendered PCM, not native device playback or a
running transport. It does not alter the LSFFR mix, master gain or automation.

Each non-CC track reports track (stable ID), name, kind, available, audible,
manual_gain, effective_gain, automated, rms_pre_gain, rms_post_gain, rms_l,
rms_r, stereo_balance, stereo_width and spectral_centroid_hz. Audio observations
use the selected clips after mute/solo and overlap selection. Stereo image and
frequency describe post-gain source stereo BEFORE LSFFR routing, so FF/FF or OFF
destinations do not erase or falsely relocate the source image. RMS is over the
whole requested range, including silence. Pre/post RMS combines channel energy;
rms_l/rms_r are post-gain. Effective gain is post/pre RMS, or the RMS gain
envelope when the source is silent. Automated is true if any sample used active
CC; manual_gain remains the document fader even while automation controls audio.

Balance is (right energy - left energy) / total energy, from -1 to +1.
Width is sqrt((1-correlation)/2) times 2*sqrt(left energy*right energy)/total
energy. It is zero for mono, correlated level imbalance or a single channel;
balanced uncorrelated stereo is about 0.707 and opposing phase is one. Silence
has zero RMS, balance and width. These are measured display descriptors, not pan
or width controls. Frequency is the one-sided power centroid of separately
Hann-windowed channels, including DC, with their powers summed to avoid phase
cancellation. The FFT zero-pads to the next power of two, at most 4096 frames;
silent ranges and blocks shorter than 16 frames have a null centroid. Resolution
depends on the supplied range; there is no growing sample history. Unconnected
MIDI instruments report available false and null signal metrics, never invented
audio. When RenderRange explicitly selects the cardinal_sine instrument, its
synthesized track PCM uses the same observation path and is available. An
observer of native transport must select the same instrument as its producer.
JSON/FFT work runs only on the control/worker side, outside callbacks.

PianoRoll: id, start_quarter, end_quarter, grid_quarters (1/64..4).
It returns clip/note data and at most 4096 grid positions for that piano roll's
viewport. This data is for model placement; it does not draw a 2D piano roll.

Master observation API

BeginMasterAnalysis: document_revision, capture_epoch, sample_rate, channels.
It selects a fresh analysis stream and returns analysis_generation. This is an
analysis reset, not microphone/device startup. A seek, capture discontinuity,
sample-rate change or changed document requires a new begin call.

AnalyzeMaster: document_revision, capture_epoch, analysis_generation, sample_rate,
channels, start_frame, samples, observed_at_seconds. Supply only the final mixed
master PCM in contiguous chunks. The same clock used for observed_at_seconds
drives the future marker reveal animation. start_frame is elapsed master time
at the native sample rate; samples are interleaved mono or stereo, at most
393216 finite values per call. Float headroom up to +/-64 is accepted.

Only the active generation may publish; stale revision/generation results fail.
The worker-side analyzer integrates each channel into 8 kHz analysis samples,
accumulates at least 10 seconds and retains at most 20 seconds. Until sufficient
evidence is buffered, it returns gathering and leaves accepted guides unchanged.
Partial analysis replacement retains old guide portions outside the new window.
State and analysis advancement commit together after publication validation.
Each region exposes markers with stable IDs and reveal times. Overlapping
observations reuse accepted marker identities; only new markers begin their
reveal. Compatible historical pieces consolidate, capped at 1024 markers per
region. Up to 512 regions are retained; capacity failure is explicit.

The onset calculation adapts Cardinal's spectral flux. Local consecutive attack
intervals must agree within 12%, with at least five observed pulses. Pulse rates
are bounded to 30..300 per minute. beat_seconds are approximate observed attacks,
not exact downbeats. regularity_score is not a calibrated musical confidence.
meter remains null. The weighted box resampling is an analysis approximation;
extra/missing attacks, subdivisions, rubato, transitions, weak attacks and
analysis-boundary behavior still require evaluation. This is not yet a reliable
musical tempo/meter estimator. No worker scheduler or live master tap is connected.

3D panel seam

LocatePanel and ProjectPanel accept origin/right/up/normal, width_m/height_m,
start_seconds and seconds_per_width. The basis is explicit and orthonormal;
UV (0,0) is the safe rectangle's lower-left corner.
LocatePanel also takes ray_origin and direction. ProjectPanel takes time_seconds
and v. Geometric misses return null. Hits include UV, local metres, world position
and elapsed time. They do not load models or create graphics resources.

The existing Art/AudioWorkstation/GemPanels kit provides safe rectangles and real
panel meshes. Blender uses +X right, +Y up, +Z outward; the GLBs use +X right,
-Z up, +Y outward. The consuming renderer must use the actual transformed basis.
Its four static models now load in the blank panel scene described in
PanelScene.txt. Full gem optics, notes, labels, guide animation and pointer
capture remain presentation/input work.

Cardinal source references

Salamander137/Host.h separates audio source offsets from clip placement and stores
MIDI note pitch/onset/length/velocity. The new local beat clock replaces the old
global-BPM assumption. Salamander137/BaseSystem/DawWarpSystem.cpp supplies the
onset-analysis reference, not its whole-source BPM or audio warping policy.
Salamander137/Host/LibraryRecording.h is a reference for later bounded capture;
its recorder is not ported or connected by this pass. Old timeline rendering,
global beat snapping, source-clip analysis and workstation features outside this
request were not imported.

Historical initial backend validation (superseded by the shared build and live
export validation above): baseline intent inspection passed with zero structural
mismatches. The first crate validation job exited 101 before running
tests: Render/webgpu.rs references WGPUNativeFeature_TimestampQueryInsideEncoders,
which is present in the installed native header but absent from generated bindings.
That renderer is under concurrent performance work. Focused validation then passed
all 28 automation, routing, document and range-render tests (job
job-mvuqzfkk-37ab7c01, exit 0). The harness compiled the actual production modules
against the existing cached serde_json dependency; source hashes were unchanged
throughout the run. It excludes Host/native/renderer integration. The authored
Host contract test and a fresh whole-crate intent inspection were then blocked by
the graphics compile error; subsequent shared validation above resolved that
compile blocker. This initial backend-only run did not verify native audio,
instruments, UI interaction or devices. Its then-outstanding scope was: device capture, streaming source
storage, native sample-clock transport, instrument output, the optional analysis
worker/tap, 3D editor content and pointer controller, persistence and undo.

Floating bubble piano-roll editor, October30

AudioWorkstationSystem now owns an editor over this document API. See
BUBBLE_PIANO_ROLL.txt for controls, asset adaptation and verification limits.
OpenPianoRoll {id} is the registered layer-five selection seam. No changes to
the local MIDI clock, LSFFR routing or rendering API are implied by presentation.

Audio clip fades — November 9, 2026

Audio clips expose square fade handles along their upper edge. Drag the left
handle right to fade in; drag the right handle left to fade out. Drag back to
the corresponding clip edge to remove a fade. The remaining tape body moves
the clip. Gold lines show the amplitude envelope; overlapping linear ramps
multiply. Source PCM and the raw source waveform remain unchanged.

Dragging previews only; release commits once. Escape, pointer loss, resize or
a changed document cancels. Timeline Undo/Redo keys operate on at most 32
accepted fade edits, until another document edit invalidates that history.
SetClipFades accepts id, fade_in_seconds and fade_out_seconds, or action undo/redo,
with document_revision. Durations must be finite, nonnegative and at most the
clip duration. Snapshot/PutClip round-trip both fields; omitted fields default
to zero. General project-file save/load remains unimplemented.

Playback, hosted track effects and frozen WAV export share the clip-local
envelope. A nonzero fade reaches zero on its first/last clip sample; envelopes
are evaluated before track gain, plugins and LSFFR routing. Fade presentation
is inexpensive unlit native overlay content and never rebakes optics.

Audio clip selection and splitting — November 9, 2026

Audio tape top/bottom lips (12 logical points, reduced for short rows) move
clips. The center selects a clip and places the edit cursor; dragging selects
time without moving the audio. Selected audio accepts the inexpensive range
overlay. Fade handles retain priority. Audio tracks default to 96 points; MIDI and CC rows retain 64; the existing
horizontal track-handle drag resizes each track independently.
Command-E splits the selected audio at the range anchor or current playhead.
Source PCM is shared; offsets advance on the right piece. Main-lane spectral
treatments are shared. Splits within fade ramps and enabled warped main clips
are rejected explicitly to preserve their audio. Alternate take lanes use their
existing revision-checked splitter and treatment restrictions. Native visual
and input review remains unverified.

Clip gain and bottom waveform editor — November 9, 2026

The existing bottom Clip Editor is the audio editor page: right-click a clip
and choose OPEN AUDIO EDITOR, or use the bottom page arrows to reach page four.
It now opens Waveform. Drag across the source waveform to select a range; Trim
keeps that interval at its existing timeline position, and Split cuts at the
selection anchor. GAIN -1/+1 adjust clip gain by one dB; clicking the numeric
gain resets unity. Clip gain defaults to 1, accepts 0..16 and affects playback,
track-effect input and frozen export independently of track gain. It survives
source-preserving splits and trims; displayed waveform remains raw source PCM.

The Warp tab uses the existing Cardinal map, shared musical clock and
pitch-preserving worker. Toggle Warp, click source time then ADD MARK, drag an
interior marker to change its source anchor, or select and REMOVE it. Endpoints
stay fixed; PITCH -1/+1 adjust semitones within -24..24. Marker drag previews
locally and commits once on release; revision/focus/pointer/layout changes cancel.
This is source-anchor editing against fixed destination beats, not a new
independent tempo map or automatic beat inference. The Spectral tab remains.
Alternate takes support source preview and gain/trim, while their existing
main-lane promotion requirement for Warp remains. Project-file save/load and
general clip-edit undo are still outside this change.

Clip editing validation: 632 workstation tests passed, two ignored; 38 clip-focused
checks passed, including gain-scaled PCM, frozen WAV gain and registered Host
routes. The normal executable builds and intent inspection reports zero
structural mismatches. Native UI/device listening remains unverified.

Shared content undo — November 9, 2026

The timeline's Command-Z and redo shortcut now use Cardinal's shared content
journal rather than the fade-only journal. The journal retains up to 256 entries
across track creation/mix/routing/order, sources/imports, clip creation/deletion,
movement, gain, split/trim, fades, MIDI clip edits, tempo/shared timing, warp,
spectral masks, audio playlists, committed capture/recall and drum-rack edits.
PCM buffers remain shared immutable allocations. History runs on the serialized
control side; native audio callbacks never capture or restore checkpoints.

WorkstationDataSystem exposes Undo, Redo, UndoStatus, ClearUndo,
BeginUndoTransaction, CommitUndoTransaction and CancelUndoTransaction. Mutating
history requests require the current document_revision. Optional undo_label
names an edit; merge_key identifies one unique gesture and must not be reused
for separate gestures. Nested transactions publish one entry at the outer
commit. Cancellation follows Cardinal's bookkeeping semantics: it keeps applied
edits and clears history if edits occurred, preventing older snapshots from
silently overwriting them. New edits clear redo. Restores advance document
revision and invalidate analysis; they never restore an old revision number.
Snapshot.undo reports availability, labels, counts and transaction depth.

The piano-roll and Loopback keyboard controllers retain their existing local
journals; their accepted edits also enter the shared document journal. Legacy
local-history restore commands form a shared-history barrier. Freeze operations
also form a barrier; undo/redo is blocked while frozen, freezing, recording or
holding pending capture. Plugin instance state is not checkpointed. Current
baked production artwork has no mutable theme document, so Cardinal's style
checkpoint UI is not imported; non-content domain requests are rejected.
No project-file history persistence or native UI/device validation is implied.

Track freeze and flatten — November 9, 2026

Click a main track handle to select it independently of clips. Unmodified F
opens Freeze Track and begins a bounded render. Finished enables OK; Cancel
remains available and discards the result, including after completion. Closing
the popup cancels. OK replaces the main arrangement with frozen stereo audio.
F on its selected handle then flattens it into ordinary editable audio and
removes the playback inserts and retained original clip treatments.

The bounce is pre-fader/pre-routing, with clip gain/fades, warp, spectral repair,
instrument and playback effects included. Track gain, mute/solo, gain automation
and routing remain live. Native plugins render in a separately instantiated
host from saved state. Unsupported state save/load and VST2 fail explicitly.
Effects receive two seconds of tail. Recording must be stopped before freezing;
the frozen track is disarmed and cannot monitor/record until flattened. Other
tracks remain available for recording. Comp takes stay stored and silent.
The existing 16,777,216 source-value and 256-source budgets also bound freeze,
and sources must match the selected render rate. Global content undo has a
history barrier at freeze acceptance and flattening. Project save/load remains
outside this feature. Native F/pointer/device review and third-party listening
remain unverified.

Port validation: 642 workstation module tests passed, two ignored. The registered
Host test workstation_cardinal_undo_routes_and_timeline_shortcuts passed, covering
accepted track creation, timeline undo/redo and scene cleanup. Host tests used
RUST_MIN_STACK=16777216. An overbroad workstation-name-filter run encountered
existing integration failures and was stopped in the unrelated security-camera
render test; this is not a claim that the complete Host suite passed.

Track I/O mute indication: the right-panel M gem changes from red to the shared
ivory meter/fader/comp-handle palette while mute is enabled. Unmuting restores
red. This selects offline baked artwork; it does not recalculate lighting.

Track channel strip shortcut — November 9, 2026

Double-click a track lane, clip body or track handle to select that track and
reveal the bottom panel's Plugins page with its Edison inserts. Both clicks
must hit the same lane, clip or handle within 350 ms; dragging cancels the pair.
Option/Alt-double-click a MIDI clip retains the separate piano-roll shortcut.
This replaces plain MIDI-clip double-click opening the piano roll.

Wearable DAW and audio ray controls (November 9, 2026)
---------------------------------------------------
Right panel / track I/O: RAYS ON/OFF persists for the game session. Outside the
DAW, rays require worn glasses. AR: E freezes player/camera and opens the DAW on
the current real view; no plain backdrop or security cameras. VR: E opens/closes,
WASD walks and pointer movement looks in player mode. C or the right-panel view
button selects cameras; WASD then pans cameras, V cycles cameras, and player
movement pauses. VR Settings still offers plain/passthrough. Rays use real world
and cabinet signal observations and existing measured acoustic paths.

Waveform editor first pass — November 10, 2026

Audio-clip double-click now opens Waveform instead of the track Plugins page.
Lane, track handle and MIDI/CC double-click retain the channel-strip shortcut.
Waveform uses transparent native display content over the existing baked panel.
Drag a range on the upper zoom overview to zoom; click it to fit the full clip.
Drag the lower loop bar to set an enabled playback loop in the audio transport.
Waveform range selection, Trim and Split retain full-clip source coordinates.
No live lighting or asset rebuild is introduced. Native visual/listening review
and installed-app packaging remain unverified.

Output bus components — November 10, 2026

A new document has no timeline tracks, four Matrix Buses (L, S, FF, R) routed
one-to-one to the Flown Array hardware groups, and one clean Mix Bus. Matrix
and Mix Bus are DAW Components: drag either from the browser into the bottom
panel to create it. Neither creates a timeline lane. Click an output meter to
select its bus; Delete removes the selected bus. Selection is indicated by an
unlit outline and the component name. Content Undo/Redo includes bus edits.

The first right I/O page lists the buses after normal tracks. Matrix controls
select IN L/S/FF/R and mute. Physical output and channel selectors are retired
from this page. Settings / Audio Routing shows matrix buses and physical
channels as ports joined by cables: drag or click two ports to connect, and
use X to disconnect. Edits are immediate document changes with content Undo,
independent of Workspace preference Apply/Cancel. The current
physical-device catalog contains the engine's Flown Array. These destinations
are the engine's physical cabinet feeds, not an OS audio-device picker. Mix
controls select MON/OFF and mute. Bus patches also expose gain and names.

Each Mix Bus independently folds the original LSFFR sum to stereo, processes
its own playback FX chain and gain, and adds to monitor output without
normalization. Two clean unity Mix Buses double monitor samples. Select a mix
meter, choose bottom page 2 (Plugins), and drag an effect from the browser to
the bottom chain. Instruments and preamps are excluded from mix buses. Matrix
buses do not accept FX. The existing native plugin-host platform limits apply.

The Flown Array and its virtual microphone receive matrix PCM before Mix Bus
processing. Stereo/headphone monitoring receives the sum of Mix Bus outputs.
Four-stem export still reads original L/S/FF/R PCM, before either output stage;
it never runs monitor FX. Monitor-only FX do not block stem export. The existing
restriction on exporting unfrozen normal tracks with native plugins remains.
Deleting a bus makes its inserts dormant and keeps ownership for content Undo;
new components cannot reuse an identity with retained inserts.

Validation evidence is recorded in the main README. Native UI appearance and
listening/device checks are separate from control-driver and PCM tests.

Piano-roll toolbar artwork (November 10, 2026)
The production baked compositor uses the transport mini-button ruby artwork
for both piano toolbar rows. Draw, Paint and Arp latch to the existing ivory
button artwork with a recessed mounting position when selected. Labels remain
inexpensive unlit glyphs and existing toolbar hit areas/actions are preserved.
No lighting is evaluated or refreshed by tool selection. Native visual review
and installed-app packaging have not been performed for this change.

White ruby selected buttons — November 10, 2026

At the author's request, latched mute, selected bottom-panel pages and selected
piano-toolbar buttons now use desaturated versions of their own ruby artwork,
rather than the ivory fader casing material. Both Mini and Wide have matching
variants. The existing 12704 selected-state key is retained; MuteIvory is a
legacy asset name. Offline HSV saturation removal preserves the source value,
facets, reflections, alpha and auxiliary depth exactly. Faders, meters and
comp-lane handles retain their existing artwork. No runtime optics are added.
The bottom-page intent description now records this requested appearance.
