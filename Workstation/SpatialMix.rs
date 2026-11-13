//! Control-thread projection and gain gestures for measured source spheres.
//!
//! The two depth inputs stay separate: pre-fader PCM level follows the source,
//! while the document's gain is the user's persistent control. Stereo balance,
//! width and spectral height are measurements, never invented pan/EQ controls.
//! Layer five dispatches returned proposals through SetTrackMix and immediately
//! calls acknowledge with an authoritative Snapshot, including after rejection.
use super::{
    panel_layout::Viewport,
    piano_roll::{Input, Rect},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const MAX_SOURCES: usize = 64;
const FRESH_SECONDS: f64 = 0.5;
const MIN_DB: f64 = -72.;
const MAX_DB: f64 = 24.082_399_653_118_497; // 20 log10(16)
const SILENCE: f64 = 0.000_01;

#[derive(Clone)]
struct Metadata {
    id: String,
    name: String,
    kind: String,
    gain: f64,
    audible: bool,
    cc_connected: bool,
}

struct Measurement {
    id: String,
    available: bool,
    audible: bool,
    pre: f64,
    post: f64,
    balance: f64,
    width: f64,
    centroid: Option<f64>,
    gain: f64,
    automated: bool,
}

struct Observation {
    sequence: u64,
    tracks: Vec<Measurement>,
}

struct Source {
    meta: Metadata,
    color: [f64; 3],
    observed: bool,
    available: bool,
    age: f64,
    pre: f64,
    pre_target: f64,
    post_target: f64,
    activity: f64,
    balance: f64,
    balance_target: f64,
    width: f64,
    width_target: f64,
    frequency: f64,
    frequency_target: f64,
    effective_gain: f64,
    automated: bool,
}

impl Source {
    fn new(meta: Metadata) -> Self {
        Self {
            color: color(&meta.id),
            effective_gain: meta.gain,
            meta,
            observed: false,
            available: false,
            age: FRESH_SECONDS,
            pre: 0.,
            pre_target: 0.,
            post_target: 0.,
            activity: 0.,
            balance: 0.,
            balance_target: 0.,
            width: 0.,
            width_target: 0.,
            frequency: 0.5,
            frequency_target: 0.5,
            automated: false,
        }
    }

    fn fresh(&self) -> bool {
        self.available && self.age < FRESH_SECONDS
    }

    fn gain_locked(&self) -> bool {
        // After a revision invalidates the observation, a connected CC lane
        // must not masquerade as an independently effective manual fader.
        self.automated || (!self.fresh() && self.meta.cc_connected)
    }

    fn advance(&mut self, age_dt: f64, smoothing_dt: f64) {
        self.age = (self.age + age_dt).min(60.);
        let pre = if self.fresh() && self.meta.audible {
            self.pre_target
        } else {
            0.
        };
        let activity = if self.fresh() && self.meta.audible {
            (self.post_target * 4.).clamp(0., 1.)
        } else {
            0.
        };
        self.pre = smooth(self.pre, pre, smoothing_dt, 0.045, 0.22);
        self.activity = smooth(self.activity, activity, smoothing_dt, 0.04, 0.18);
        // Silence has no spectral/image evidence. Hold the last measured
        // position; only loudness recedes. Unobserved handles stay neutral.
        if self.fresh() && self.pre_target > SILENCE {
            self.balance = smooth(self.balance, self.balance_target, smoothing_dt, 0.09, 0.09);
            self.width = smooth(self.width, self.width_target, smoothing_dt, 0.09, 0.09);
            self.frequency = smooth(
                self.frequency,
                self.frequency_target,
                smoothing_dt,
                0.12,
                0.12,
            );
        }
    }

    fn animating(&self) -> bool {
        self.activity > SILENCE
            || (self.meta.audible
                && (if self.automated {
                    self.effective_gain
                } else {
                    self.meta.gain
                }) > 0.
                && (self.pre > SILENCE || (self.fresh() && self.pre_target > SILENCE)))
    }
}

#[derive(Clone)]
struct Drag {
    id: String,
    revision: u64,
    pointer_y: f64,
    gain: f64,
    pre: f64,
    db_per_pixel: f64,
}

struct Pending {
    id: String,
    revision: u64,
    gain: f64,
}

#[derive(Default)]
pub(crate) struct State {
    revision: Option<u64>,
    sequence: Option<u64>,
    sources: BTreeMap<String, Source>,
    selected: Option<String>,
    drag: Option<Drag>,
    pending: Option<Pending>,
    previous_left: bool,
    owns_left: bool,
    consumed: bool,
    time_seconds: f64,
    message: Option<String>,
}

impl State {
    pub(crate) fn sync(&mut self, snapshot: &Value, dt_seconds: f64) -> Result<(), String> {
        self.synchronize(snapshot, dt_seconds, false, None)
    }

    pub(crate) fn sync_with_visualization(&mut self,snapshot:&Value,observation:Option<&Value>,dt_seconds:f64)->Result<(),String> {
        self.synchronize(snapshot,dt_seconds,false,observation)
    }
    fn synchronize(
        &mut self,
        snapshot: &Value,
        dt_seconds: f64,
        own_edit: bool,
        observation:Option<&Value>,
    ) -> Result<(), String> {
        let parsed = read_snapshot_observation(snapshot,observation);
        let (revision, metadata, observation) = match parsed {
            Ok(parsed) => parsed,
            Err(error) => {
                self.drag = None;
                self.pending = None;
                self.message = Some(error.clone());
                // Invalid input cannot keep an old source perpetually alive.
                for source in self.sources.values_mut() {
                    source.age = FRESH_SECONDS;
                    source.pre = 0.;
                    source.activity = 0.;
                }
                return Err(error);
            }
        };
        let changed = self.revision.is_some_and(|old| old != revision);
        if changed && !own_edit {
            self.drag = None;
            self.pending = None;
            self.message = Some("Source gesture cancelled: document changed".into());
        }
        if changed {
            self.sequence = None;
            if !own_edit {
                for source in self.sources.values_mut() {
                    source.age = FRESH_SECONDS;
                    source.available = false;
                    source.automated = false;
                }
            }
        }
        self.revision = Some(revision);
        let present: BTreeSet<_> = metadata.iter().map(|meta| meta.id.clone()).collect();
        self.sources.retain(|id, _| present.contains(id));
        for meta in metadata {
            if let Some(source) = self.sources.get_mut(&meta.id) {
                source.meta = meta;
                if !source.automated {
                    source.effective_gain = source.meta.gain;
                }
            } else {
                self.sources.insert(meta.id.clone(), Source::new(meta));
            }
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|id| !self.sources.contains_key(id))
        {
            self.selected = None;
        }
        if self
            .drag
            .as_ref()
            .is_some_and(|drag| !self.sources.contains_key(&drag.id))
        {
            self.drag = None;
        }
        // Only a new published window refreshes age. Repeated snapshots of the
        // same PCM window cannot make a stopped source animate forever.
        let mut refreshed = BTreeSet::new();
        if let Some(observation) =
            observation.filter(|o| self.sequence.is_none_or(|sequence| o.sequence > sequence))
        {
            self.sequence = Some(observation.sequence);
            for measurement in observation.tracks {
                let Some(source) = self.sources.get_mut(&measurement.id) else {
                    continue;
                };
                source.observed = true;
                source.available = measurement.available;
                source.age = 0.;
                source.pre_target = measurement.pre;
                source.post_target = measurement.post;
                source.effective_gain = measurement.gain;
                source.automated = measurement.automated;
                source.meta.audible = measurement.audible;
                if measurement.available && measurement.pre > SILENCE {
                    source.balance_target = measurement.balance;
                    source.width_target = measurement.width;
                    if let Some(hz) = measurement.centroid {
                        source.frequency_target = frequency_height(hz);
                    }
                }
                refreshed.insert(measurement.id);
            }
        }
        if self
            .drag
            .as_ref()
            .is_some_and(|drag| self.sources[&drag.id].gain_locked())
        {
            self.drag = None;
            self.pending = None;
            self.message = Some("Gain automation controls this source".into());
        }
        let age_dt = if dt_seconds.is_finite() {
            dt_seconds.clamp(0., 60.)
        } else {
            0.
        };
        let smoothing_dt = age_dt.min(0.1);
        for (id, source) in &mut self.sources {
            source.advance(
                if refreshed.contains(id) { 0. } else { age_dt },
                smoothing_dt,
            );
        }
        if self.sources.values().any(Source::animating) {
            self.time_seconds = (self.time_seconds + smoothing_dt) % 3600.;
        }
        Ok(())
    }

    /// Rebase only a verified accepted proposal. An unexpected revision or gain
    /// is a rejection/external edit and ends capture instead of committing over it.
    pub(crate) fn acknowledge(&mut self, snapshot: &Value) -> Result<(), String> {
        let Some(pending) = self.pending.take() else {
            return self.sync(snapshot, 0.);
        };
        let accepted = snapshot["revision"].as_u64() == pending.revision.checked_add(1)
            && snapshot["tracks"].as_array().is_some_and(|tracks| {
                tracks.iter().any(|track| {
                    track["id"] == pending.id
                        && track["gain"].as_f64().is_some_and(|gain| {
                            (gain as f32).to_bits() == (pending.gain as f32).to_bits()
                        })
                })
            });
        if !accepted {
            self.drag = None;
            self.message = Some("Gain change was not accepted; gesture cancelled".into());
        }
        self.synchronize(snapshot, 0., accepted, None)?;
        if accepted && let Some(drag) = &mut self.drag {
            drag.revision = self.revision.unwrap_or(drag.revision);
        }
        Ok(())
    }

    /// Call even outside the canvas while captured. A null pointer (including
    /// focus loss), Escape or invalid geometry cancels; an ordinary release ends
    /// the already-applied gain gesture without emitting another edit.
    pub(crate) fn input(&mut self, input: &Input, rect: Rect, view: Viewport) -> Option<Value> {
        self.consumed = false;
        let pressed = input.left && !self.previous_left;
        self.previous_left = input.left;
        let pointer = input.pointer.filter(|p| p.iter().all(|v| v.is_finite()));
        if input.escape || pointer.is_none() || !valid_geometry(rect, view) {
            self.consumed = self.drag.is_some() || self.owns_left;
            self.drag = None;
            self.owns_left &= input.left;
            return None;
        }
        let pointer = pointer?;
        // A blocked automated hit or cancelled captured gesture owns its held
        // button through release, so another editor cannot begin mid-click.
        if self.owns_left && self.drag.is_none() {
            self.consumed = true;
            self.owns_left &= input.left;
            return None;
        }
        if let Some(drag) = self.drag.clone() {
            self.consumed = true;
            if !input.left {
                self.drag = None;
                self.owns_left = false;
                return None;
            }
            if self.revision != Some(drag.revision)
                || !self
                    .sources
                    .get(&drag.id)
                    .is_some_and(|source| !source.gain_locked())
            {
                self.drag = None;
                return None;
            }
            // The captured pointer and gain, not a moving audio envelope, are
            // the baseline. Huge deltas are bounded before exponentiation.
            let delta = (pointer[1] - drag.pointer_y).clamp(-100_000., 100_000.);
            let gain = adjusted_gain(drag.gain, delta * drag.db_per_pixel);
            return self.propose(&drag.id, gain);
        }
        if !contains(rect, pointer) {
            return None;
        }
        let wheel = input.wheel[1];
        let wheel_active = wheel.is_finite() && wheel != 0.;
        if !pressed && !wheel_active {
            return None;
        }
        let id = self.hit(pointer, rect, view)?;
        self.consumed = true;
        self.owns_left = pressed;
        self.selected = Some(id.clone());
        let source = &self.sources[&id];
        if source.gain_locked() {
            self.message =
                Some("Gain automation controls this source; edit its automation lane".into());
            return None;
        }
        self.message =
            Some("Drag up to send farther / quieter; down to bring closer / louder".into());
        let gain = source.meta.gain;
        if pressed {
            self.drag = Some(Drag {
                id,
                revision: self.revision?,
                pointer_y: pointer[1],
                gain,
                pre: source.pre,
                db_per_pixel: (96. / rect.height.max(240.)).clamp(0.02, 0.4),
            });
            None
        } else {
            // Positive wheel goes up/back, consistently with the drag gesture.
            self.propose(&id, adjusted_gain(gain, -wheel.clamp(-24., 24.) * 1.5))
        }
    }

    fn propose(&mut self, id: &str, gain: f64) -> Option<Value> {
        if self.pending.is_some() {
            return None;
        }
        let source = self.sources.get(id)?;
        if (source.meta.gain as f32).to_bits() == (gain as f32).to_bits() {
            return None;
        }
        let revision = self.revision?;
        self.pending = Some(Pending {
            id: id.into(),
            revision,
            gain,
        });
        Some(json!({"document_revision":revision,"id":id,"gain":gain}))
    }

    fn displayed_gain(&self, id: &str, source: &Source) -> f64 {
        self.pending
            .as_ref()
            .filter(|pending| pending.id == id)
            .map_or_else(
                || {
                    if source.automated {
                        source.effective_gain
                    } else {
                        source.meta.gain
                    }
                },
                |pending| pending.gain,
            )
    }

    fn placement(&self, id: &str, source: &Source, rect: Rect) -> Placement {
        let pre = self
            .drag
            .as_ref()
            .filter(|drag| drag.id == id)
            .map_or(source.pre, |drag| drag.pre);
        let gain = self.displayed_gain(id, source);
        // Distinct additive decibel layers: gain never overwrites measured
        // source level, and a changing source never writes back to the fader.
        let level = if source.meta.audible && pre > SILENCE && gain > 0. {
            (level_db(pre) + gain_db(gain)).clamp(MIN_DB, 6.)
        } else {
            MIN_DB
        };
        let near = ((level - MIN_DB) / (6. - MIN_DB)).clamp(0., 1.);
        let perspective = 0.34 + 0.66 * near;
        let base_radius = (rect.width.min(rect.height) * 0.09).clamp(15., 62.);
        // A quiet source always retains a small, stationary, selectable handle.
        let radius_y = (base_radius * perspective * (1. + 0.6 * source.activity))
            .max(7.)
            .min(rect.height * 0.16);
        let radius_x = (radius_y * (1. + source.width * 1.35)).min(rect.width * 0.24);
        let desired_x = rect.left
            + rect.width * 0.5
            + source.balance * (rect.width * 0.5 - radius_x - 3.).max(0.) * perspective;
        let desired_y =
            rect.top + rect.height * (0.50 + (0.5 - source.frequency) * 0.42 + near * 0.13);
        let x = desired_x.clamp(rect.left + radius_x, rect.left + rect.width - radius_x);
        let y = desired_y.clamp(rect.top + radius_y, rect.top + rect.height - radius_y);
        Placement {
            center: [x, y],
            radii: [radius_x, radius_y],
            near,
        }
    }

    fn hit(&self, pointer: [f64; 2], rect: Rect, _view: Viewport) -> Option<String> {
        // Positive camera Z looks down on the scene. The nearest sphere's
        // visible ellipsoid surface wins, not its insertion or track order.
        let mpp = 1.3 / _view.height;
        self.sources
            .iter()
            .filter(|(_, source)| source.observed && source.available)
            .filter_map(|(id, source)| {
                let place = self.placement(id, source, rect);
                let dx = (pointer[0] - place.center[0]) / place.radii[0];
                let dy = (pointer[1] - place.center[1]) / place.radii[1];
                let distance = dx * dx + dy * dy;
                (distance <= 1.).then(|| {
                    (
                        id,
                        0.02 + 0.01 * place.near + place.radii[1] * mpp * (1. - distance).sqrt(),
                    )
                })
            })
            .max_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.0.cmp(a.0)))
            .map(|(id, _)| id.clone())
    }

    pub(crate) fn presentation(&self, rect: Rect, view: Viewport) -> Value {
        if !valid_geometry(rect, view) || self.sources.is_empty() {
            return Value::Null;
        }
        let mpp = 1.3 / view.height;
        let world = |p: [f64; 2]| {
            [
                (p[0] - view.width * 0.5) * mpp,
                (view.height * 0.5 - p[1]) * mpp,
            ]
        };
        let lo = world([rect.left, rect.top + rect.height]);
        let hi = world([rect.left + rect.width, rect.top]);
        let spheres: Vec<_> = self
            .sources
            .iter()
            .filter(|(_, source)| source.observed && source.available)
            .map(|(id, source)| {
                let place = self.placement(id, source, rect);
                let center = world(place.center);
                json!({"id":id,"center":[center[0],center[1],0.02+0.01*place.near],
                "radii":[place.radii[0]*mpp,place.radii[1]*mpp,place.radii[1]*mpp],
                "color_linear_rgb":source.color,"selected":self.selected.as_deref()==Some(id),
                "activity":source.activity})
            })
            .collect();
        if spheres.is_empty() {
            return Value::Null;
        }
        json!({"time_seconds":self.time_seconds,"clip_rect":[lo[0],lo[1],hi[0],hi[1]],"spheres":spheres})
    }

    pub(crate) fn status(&self) -> Value {
        json!({"document_revision":self.revision,"sequence":self.sequence,
            "selected_track":self.selected,"interacting":self.is_interacting(),
            "animating":self.is_animating(),"message":self.message,
            "sources":self.sources.iter().map(|(id,s)| json!({"track":id,"name":s.meta.name,
                "kind":s.meta.kind,"observed":s.observed,"available":s.available,
                "fresh":s.fresh(),"audible":s.meta.audible,"gain":s.meta.gain,
                "automated":s.automated,"gain_locked":s.gain_locked()})).collect::<Vec<_>>()})
    }

    pub(crate) fn is_interacting(&self) -> bool {
        self.drag.is_some()
    }
    pub(crate) fn consumes_pointer(&self) -> bool {
        self.consumed
    }
    pub(crate) fn is_animating(&self) -> bool {
        self.sources.values().any(Source::animating)
    }

    /// Closing/focus loss releases capture and its held-button history; source
    /// measurements remain available for the next authoritative sync.
    pub(crate) fn close(&mut self) {
        self.drag = None;
        self.pending = None;
        self.previous_left = false;
        self.owns_left = false;
        self.consumed = false;
    }
}

struct Placement {
    center: [f64; 2],
    radii: [f64; 2],
    near: f64,
}

fn smooth(current: f64, target: f64, dt: f64, attack: f64, release: f64) -> f64 {
    let tau = if target > current { attack } else { release };
    let next = current + (target - current) * (1. - (-dt / tau).exp());
    if (next - target).abs() < SILENCE {
        target
    } else {
        next
    }
}

fn gain_db(gain: f64) -> f64 {
    if gain <= 0. {
        MIN_DB
    } else {
        (20. * gain.log10()).clamp(MIN_DB, MAX_DB)
    }
}
fn level_db(level: f64) -> f64 {
    (20. * level.max(0.000_000_01).log10()).clamp(MIN_DB, 48.)
}
fn db_gain(db: f64) -> f64 {
    if db <= MIN_DB {
        0.
    } else {
        10f64.powf(db.min(MAX_DB) / 20.).clamp(0., 16.)
    }
}
fn adjusted_gain(gain: f64, delta_db: f64) -> f64 {
    if delta_db == 0. {
        return gain;
    }
    let baseline_db = if gain > 0. {
        20. * gain.log10()
    } else {
        MIN_DB
    };
    let db = baseline_db + delta_db;
    // Imported documents may contain positive values below the visible fader
    // floor. Keep their exact relative gain instead of jumping them up to that
    // floor. Ordinary gestures retain the existing explicit zero endpoint.
    if gain > 0. && baseline_db < MIN_DB {
        10f64.powf(db.min(MAX_DB) / 20.).clamp(0., 16.)
    } else {
        db_gain(db)
    }
}
fn frequency_height(hz: f64) -> f64 {
    (hz.clamp(20., 20_000.) / 20.).ln() / 1000f64.ln()
}

fn valid_geometry(rect: Rect, view: Viewport) -> bool {
    [
        rect.left,
        rect.top,
        rect.width,
        rect.height,
        view.width,
        view.height,
        view.canvas_scale,
    ]
    .iter()
    .all(|v| v.is_finite())
        && rect.width > 2.
        && rect.height > 2.
        && rect.left.abs() <= 1_000_000.
        && rect.top.abs() <= 1_000_000.
        && rect.width <= 1_000_000.
        && rect.height <= 1_000_000.
        && (1. ..=1_000_000.).contains(&view.width)
        && (1. ..=1_000_000.).contains(&view.height)
        && (0.01..=1.).contains(&view.canvas_scale)
}

fn contains(rect: Rect, p: [f64; 2]) -> bool {
    p[0] >= rect.left
        && p[0] < rect.left + rect.width
        && p[1] >= rect.top
        && p[1] < rect.top + rect.height
}

fn number(value: &Value, key: &str, lo: f64, hi: f64) -> Result<f64, String> {
    value[key]
        .as_f64()
        .filter(|v| v.is_finite() && (lo..=hi).contains(v))
        .ok_or_else(|| format!("source spheres: invalid {key}"))
}

fn identity(value: &Value, key: &str) -> Result<String, String> {
    value[key]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(|| format!("source spheres: invalid {key}"))
}

fn read_snapshot_observation(snapshot: &Value, observation:Option<&Value>) -> Result<(u64, Vec<Metadata>, Option<Observation>), String> {
    let revision = snapshot["revision"]
        .as_u64()
        .ok_or("source spheres: missing document revision")?;
    let tracks = snapshot["tracks"]
        .as_array()
        .filter(|tracks| tracks.len() <= MAX_SOURCES)
        .ok_or("source spheres: missing or unbounded tracks")?;
    let mut ids = BTreeSet::new();
    let cc_targets: BTreeSet<_> = tracks
        .iter()
        .filter(|t| t["kind"] == "cc")
        .filter_map(|t| t["target"]["track"].as_str())
        .collect();
    let any_solo = tracks
        .iter()
        .any(|t| t["kind"] != "cc" && t["solo"] == true);
    let mut metadata = Vec::new();
    for track in tracks {
        let id = identity(track, "id")?;
        if !ids.insert(id.clone()) {
            return Err("source spheres: duplicate track identity".into());
        }
        let kind = track["kind"]
            .as_str()
            .ok_or("source spheres: missing track kind")?;
        if kind == "cc" {
            continue;
        }
        if kind != "audio" && kind != "midi" {
            return Err("source spheres: unknown track kind".into());
        }
        metadata.push(Metadata {
            name: track["name"]
                .as_str()
                .unwrap_or(&id)
                .chars()
                .take(256)
                .collect(),
            kind: kind.into(),
            gain: number(track, "gain", 0., 16.)?,
            audible: track["mute"] != true && (!any_solo || track["solo"] == true),
            cc_connected: cc_targets.contains(id.as_str()),
            id,
        });
    }
    let value = observation.unwrap_or(&snapshot["visualization"]);
    if value.is_null() {
        return Ok((revision, metadata, None));
    }
    if value["revision"].as_u64() != Some(revision) {
        return Err("source spheres: stale visualization revision".into());
    }
    let sequence = value["sequence"]
        .as_u64()
        .filter(|s| *s > 0)
        .ok_or("source spheres: visualization was not published")?;
    let sample_rate = number(value, "sample_rate", 1., 768_000.)?;
    let start = value["start_frame"]
        .as_u64()
        .ok_or("source spheres: invalid start frame")?;
    let end = value["end_frame"]
        .as_u64()
        .filter(|end| *end >= start)
        .ok_or("source spheres: invalid end frame")?;
    if end == start {
        return Err("source spheres: empty visualization window".into());
    }
    let measured = value["tracks"]
        .as_array()
        .filter(|a| a.len() <= MAX_SOURCES)
        .ok_or("source spheres: invalid measured sources")?;
    let mut seen = BTreeSet::new();
    let mut measurements = Vec::new();
    for track in measured {
        let id = identity(track, "track")?;
        if !seen.insert(id.clone()) || !metadata.iter().any(|m| m.id == id) {
            return Err("source spheres: unknown or duplicate measured source".into());
        }
        let available = track["available"]
            .as_bool()
            .ok_or("source spheres: missing availability")?;
        let audible = track["audible"]
            .as_bool()
            .ok_or("source spheres: missing audibility")?;
        let automated = track["automated"]
            .as_bool()
            .ok_or("source spheres: missing automation state")?;
        let gain = if available {
            number(track, "effective_gain", 0., 16.)?
        } else {
            metadata.iter().find(|meta| meta.id == id).unwrap().gain
        };
        let mut measurement = Measurement {
            id,
            available,
            audible,
            pre: 0.,
            post: 0.,
            balance: 0.,
            width: 0.,
            centroid: None,
            gain,
            automated,
        };
        if available {
            measurement.pre = number(track, "rms_pre_gain", 0., 1.0e12)?;
            measurement.post = number(track, "rms_post_gain", 0., 1.0e12)?;
            measurement.balance = number(track, "stereo_balance", -1., 1.)?;
            measurement.width = number(track, "stereo_width", 0., 1.)?;
            if !track["spectral_centroid_hz"].is_null() {
                measurement.centroid = Some(number(
                    track,
                    "spectral_centroid_hz",
                    0.,
                    sample_rate * 0.5,
                )?);
            }
        }
        measurements.push(measurement);
    }
    Ok((
        revision,
        metadata,
        Some(Observation {
            sequence,
            tracks: measurements,
        }),
    ))
}

fn color(id: &str) -> [f64; 3] {
    // Stable cross-process FNV identity hash; no dependence on list position,
    // names, wall time, or Rust's per-process randomized HashMap seed.
    let hash = id.bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3)
    });
    let hue = (hash % 3600) as f64 / 600.;
    let channel = |offset: f64| {
        let triangle = (((hue + offset) % 6.) - 3.).abs() - 1.;
        let srgb = 0.22 + 0.66 * triangle.clamp(0., 1.);
        if srgb <= 0.04045 {
            srgb / 12.92
        } else {
            ((srgb + 0.055) / 1.055).powf(2.4)
        }
    };
    [channel(0.), channel(4.), channel(2.)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Rect {
        Rect {
            left: 100.,
            top: 80.,
            width: 800.,
            height: 500.,
        }
    }
    fn view() -> Viewport {
        Viewport {
            width: 1000.,
            height: 700.,
            canvas_scale: 1.,
        }
    }
    fn fixture(pre: f64, gain: f64, balance: f64, width: f64, hz: f64) -> Value {
        json!({"revision":1,"tracks":[{"id":"source-a","name":"Source A","kind":"audio",
            "gain":gain,"mute":false,"solo":false}],"visualization":{"revision":1,
            "sample_rate":48000,"start_frame":0,"end_frame":1024,"sequence":1,
            "tracks":[{"track":"source-a","available":true,"audible":true,
                "rms_pre_gain":pre,"rms_post_gain":pre*gain,"stereo_balance":balance,
                "stereo_width":width,"spectral_centroid_hz":hz,"effective_gain":gain,
                "manual_gain":gain,"automated":false}]}})
    }
    #[test]
    fn borrowed_live_visualization_matches_materialized_snapshot_and_null_clear() {
        let original=fixture(0.5,1.,0.,1.,500.);let mut updated=original.clone();
        updated["visualization"]["sequence"]=json!(2);
        updated["visualization"]["tracks"][0]["rms_pre_gain"]=json!(0.75);
        let mut owned=State::default();let mut borrowed=State::default();
        for value in [&updated["visualization"],&Value::Null] {
            let mut materialized=original.clone();materialized["visualization"]=value.clone();
            owned.sync(&materialized,0.05).unwrap();
            borrowed.sync_with_visualization(&original,Some(value),0.05).unwrap();
            assert_eq!(owned.status(),borrowed.status());
            assert_eq!(owned.presentation(rect(),view()),borrowed.presentation(rect(),view()));
        }
        let mut stale=updated["visualization"].clone();stale["revision"]=json!(0);
        assert!(borrowed.sync_with_visualization(&original,Some(&stale),0.05).is_err());
        assert_eq!(original["visualization"]["sequence"],1);
    }
    fn settled(snapshot: &Value) -> State {
        let mut state = State::default();
        for sequence in 1..=100 {
            let mut snapshot = snapshot.clone();
            snapshot["visualization"]["sequence"] = json!(sequence);
            state.sync(&snapshot, 0.05).unwrap();
        }
        state
    }
    fn place(state: &State) -> Placement {
        state.placement("source-a", &state.sources["source-a"], rect())
    }
    fn press(state: &mut State) -> [f64; 2] {
        let p = place(state).center;
        assert!(
            state
                .input(
                    &Input {
                        pointer: Some(p),
                        left: true,
                        ..Input::default()
                    },
                    rect(),
                    view()
                )
                .is_none()
        );
        p
    }
    fn accept(state: &mut State, snapshot: &mut Value, proposal: &Value) {
        snapshot["revision"] = json!(snapshot["revision"].as_u64().unwrap() + 1);
        snapshot["tracks"][0]["gain"] = proposal["gain"].clone();
        snapshot["visualization"] = Value::Null;
        state.acknowledge(snapshot).unwrap();
    }

    #[test]
    fn source_level_and_manual_gain_are_independent_depth_layers() {
        let loud = settled(&fixture(0.5, 1., 0., 0., 1000.));
        let quiet_source = settled(&fixture(0.05, 1., 0., 0., 1000.));
        let quiet_fader = settled(&fixture(0.5, 0.1, 0., 0., 1000.));
        assert!(place(&loud).near > place(&quiet_source).near);
        assert!((place(&quiet_source).near - place(&quiet_fader).near).abs() < 0.000_01);
        assert_eq!(quiet_source.sources["source-a"].meta.gain, 1.);
    }

    #[test]
    fn silence_stays_far_at_max_gain_and_zero_fader_stays_far_for_hot_pcm() {
        let silence = settled(&fixture(0., 16., 0., 0., 1000.));
        let zero_gain = settled(&fixture(8., 0., 0., 0., 1000.));
        assert_eq!(place(&silence).near, 0.);
        assert_eq!(place(&zero_gain).near, 0.);
        assert_eq!(silence.sources["source-a"].activity, 0.);
        assert_eq!(zero_gain.sources["source-a"].activity, 0.);
        assert!(!zero_gain.is_animating());
    }

    #[test]
    fn reference_amplitude_pulse_scales_visible_and_pickable_geometry() {
        let mut state = settled(&fixture(0.25, 1., 0., 0., 1000.));
        assert_eq!(state.sources["source-a"].activity, 1.);
        let active = place(&state);
        let p = [active.center[0] + active.radii[0] * 0.9, active.center[1]];
        assert!(state.hit(p, rect(), view()).is_some());
        state.sources.get_mut("source-a").unwrap().activity = 0.;
        let resting = place(&state);
        assert!((active.radii[1] / resting.radii[1] - 1.6).abs() < 1e-8);
        assert!(state.hit(p, rect(), view()).is_none());
    }

    #[test]
    fn stereo_width_balance_and_log_frequency_control_independent_axes() {
        let low_left = settled(&fixture(0.2, 1., -0.8, 0., 100.));
        let high_right = settled(&fixture(0.2, 1., 0.8, 1., 10_000.));
        let a = place(&low_left);
        let b = place(&high_right);
        assert!(a.center[0] < b.center[0]);
        assert!(a.center[1] > b.center[1]);
        assert!((a.radii[0] - a.radii[1]).abs() < 1e-8);
        assert!(b.radii[0] > b.radii[1] * 2.);
        assert!(
            (frequency_height(200.)
                - frequency_height(20.)
                - (frequency_height(2000.) - frequency_height(200.)))
            .abs()
                < 1e-9
        );
    }

    #[test]
    fn drag_uses_capture_gain_and_survives_own_acknowledgement_and_canvas_exit() {
        let mut snapshot = fixture(0.2, 1., 0., 0., 1000.);
        let mut state = settled(&snapshot);
        let p = press(&mut state);
        let input = Input {
            pointer: Some([p[0], p[1] - 100.]),
            left: true,
            ..Input::default()
        };
        let proposal = state.input(&input, rect(), view()).unwrap();
        assert!(proposal["gain"].as_f64().unwrap() < 1.);
        let captured_pre = state.drag.as_ref().unwrap().pre;
        accept(&mut state, &mut snapshot, &proposal);
        assert!(state.is_interacting());
        assert_eq!(state.drag.as_ref().unwrap().pre, captured_pre);
        assert!(
            state.input(&input, rect(), view()).is_none(),
            "same pointer must not ratchet gain"
        );
        let farther = state
            .input(
                &Input {
                    pointer: Some([p[0], -100.]),
                    left: true,
                    ..Input::default()
                },
                rect(),
                view(),
            )
            .unwrap();
        assert!(farther["gain"].as_f64().unwrap() < proposal["gain"].as_f64().unwrap());
        assert!(state.consumes_pointer());
    }

    #[test]
    fn rejected_edit_external_revision_and_removed_track_cancel_capture() {
        let snapshot = fixture(0.2, 1., 0., 0., 1000.);
        let mut state = settled(&snapshot);
        let p = press(&mut state);
        state
            .input(
                &Input {
                    pointer: Some([p[0], p[1] + 20.]),
                    left: true,
                    ..Input::default()
                },
                rect(),
                view(),
            )
            .unwrap();
        state.acknowledge(&snapshot).unwrap();
        assert!(!state.is_interacting());
        assert_eq!(state.sources["source-a"].meta.gain, 1.);
        state.close();
        press(&mut state);
        let mut changed = snapshot.clone();
        changed["revision"] = json!(2);
        changed["visualization"] = Value::Null;
        state.sync(&changed, 0.).unwrap();
        assert!(!state.is_interacting());
        state.close();
        changed["visualization"] = snapshot["visualization"].clone();
        changed["visualization"]["revision"] = json!(2);
        changed["visualization"]["sequence"] = json!(101);
        state.sync(&changed, 0.1).unwrap();
        press(&mut state);
        assert!(state.is_interacting());
        changed["tracks"] = json!([]);
        changed["visualization"] = Value::Null;
        state.sync(&changed, 0.).unwrap();
        assert!(!state.is_interacting());
        assert!(state.presentation(rect(), view()).is_null());
    }

    #[test]
    fn silence_and_unavailable_handles_do_not_invent_motion_or_spectrum() {
        let mut snapshot = fixture(0., 1., 0., 0., 1000.);
        snapshot["visualization"]["tracks"][0]["spectral_centroid_hz"] = Value::Null;
        let state = settled(&snapshot);
        assert!(!state.is_animating());
        assert_eq!(state.sources["source-a"].frequency, 0.5);
        assert!(place(&state).radii[1] >= 7.);
        let mut unseen = State::default();
        snapshot["visualization"] = Value::Null;
        unseen.sync(&snapshot, 100.).unwrap();
        assert!(!unseen.sources["source-a"].observed);
        assert!(!unseen.is_animating());
        assert_eq!(unseen.time_seconds, 0.);
        assert!(unseen.presentation(rect(), view()).is_null());
        assert!(unseen.hit(place(&unseen).center, rect(), view()).is_none());
    }

    #[test]
    fn unavailable_midi_with_null_metrics_does_not_reject_measured_audio() {
        let mut snapshot = fixture(0.2, 1., 0., 0., 1000.);
        snapshot["tracks"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"midi","name":"MIDI",
            "kind":"midi","gain":0.75,"mute":false,"solo":false}));
        snapshot["visualization"]["tracks"]
            .as_array_mut()
            .unwrap()
            .push(json!({"track":"midi",
            "available":false,"audible":true,"automated":false,"effective_gain":null,
            "rms_pre_gain":null,"rms_post_gain":null,"stereo_balance":null,"stereo_width":null,
            "spectral_centroid_hz":null}));
        let state = settled(&snapshot);
        assert!(state.sources["source-a"].fresh());
        assert!(!state.sources["midi"].available);
        assert_eq!(state.sources["midi"].activity, 0.);
        assert_eq!(state.sources["midi"].effective_gain, 0.75);
        assert_eq!(
            state.presentation(rect(), view())["spheres"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn repeated_pcm_window_expires_and_animation_eventually_stops() {
        let snapshot = fixture(0.5, 1., 0.7, 0.8, 8000.);
        let mut state = settled(&snapshot);
        let mut same = snapshot.clone();
        same["visualization"]["sequence"] = json!(100);
        let original_frequency = state.sources["source-a"].frequency;
        for _ in 0..100 {
            state.sync(&same, 0.1).unwrap();
        }
        assert!(!state.is_animating());
        assert!(!state.sources["source-a"].fresh());
        assert_eq!(state.sources["source-a"].frequency, original_frequency);
        let time = state.time_seconds;
        state.sync(&same, 0.1).unwrap();
        assert_eq!(state.time_seconds, time);
    }

    #[test]
    fn automated_source_consumes_hit_without_claiming_to_control_gain() {
        let mut snapshot = fixture(0.2, 1., 0., 0., 1000.);
        snapshot["visualization"]["tracks"][0]["automated"] = json!(true);
        let mut state = settled(&snapshot);
        press(&mut state);
        assert!(state.consumes_pointer());
        assert!(!state.is_interacting());
        assert!(state.pending.is_none());
        assert!(state.message.as_ref().unwrap().contains("automation"));
        let held = Input {
            pointer: Some([105., 85.]),
            left: true,
            ..Input::default()
        };
        assert!(state.input(&held, rect(), view()).is_none());
        assert!(
            state.consumes_pointer(),
            "locked sphere keeps its held click outside the sphere"
        );
        state.input(
            &Input {
                pointer: held.pointer,
                ..Input::default()
            },
            rect(),
            view(),
        );
        assert!(
            state.consumes_pointer(),
            "release cannot leak to the piano roll"
        );
        state.input(
            &Input {
                pointer: held.pointer,
                ..Input::default()
            },
            rect(),
            view(),
        );
        assert!(!state.consumes_pointer());
    }

    #[test]
    fn wheel_only_edits_a_hit_and_clamps_gain_to_document_limits() {
        for gain in [1e-6, 1e-20] {
            assert!((adjusted_gain(gain, 1.5) / gain - 10f64.powf(1.5 / 20.)).abs() < 1e-12);
            assert!((adjusted_gain(gain, -1.5) / gain - 10f64.powf(-1.5 / 20.)).abs() < 1e-12);
            let mut tiny = settled(&fixture(0.2, gain, 0., 0., 1000.));
            assert!(tiny.propose("source-a", adjusted_gain(gain, 1.5)).is_some());
        }
        assert_eq!(adjusted_gain(0., -1.5), 0.);
        assert!(adjusted_gain(0., 1.5) > 0.);
        let mut snapshot = fixture(0.2, 16., 0., 0., 1000.);
        let mut state = settled(&snapshot);
        let point = place(&state).center;
        let down = Input {
            pointer: Some(point),
            wheel: [0., -100_000.],
            ..Input::default()
        };
        assert!(state.input(&down, rect(), view()).is_none());
        assert!(state.consumes_pointer());
        let up = Input {
            wheel: [0., 1.],
            ..down
        };
        let proposal = state.input(&up, rect(), view()).unwrap();
        assert!(proposal["gain"].as_f64().unwrap() < 16.);
        accept(&mut state, &mut snapshot, &proposal);
        assert!(
            state
                .input(
                    &Input {
                        pointer: Some([105., 85.]),
                        wheel: [0., 1.],
                        ..Input::default()
                    },
                    rect(),
                    view()
                )
                .is_none()
        );
        assert!(!state.consumes_pointer());
    }

    #[test]
    fn empty_canvas_and_invalid_pointer_never_take_capture() {
        let mut state = settled(&fixture(0.2, 1., 0., 0., 1000.));
        state.input(
            &Input {
                pointer: Some([105., 85.]),
                left: true,
                ..Input::default()
            },
            rect(),
            view(),
        );
        assert!(!state.consumes_pointer());
        assert!(!state.is_interacting());
        state.close();
        press(&mut state);
        state.input(
            &Input {
                pointer: Some([f64::NAN, 2.]),
                left: true,
                ..Input::default()
            },
            rect(),
            view(),
        );
        assert!(!state.is_interacting());
        assert!(state.consumes_pointer());
        state.close();
        press(&mut state);
        state.input(
            &Input {
                pointer: Some([500., 300.]),
                escape: true,
                ..Input::default()
            },
            rect(),
            view(),
        );
        assert!(!state.is_interacting());
    }

    #[test]
    fn hit_chooses_front_surface_and_colors_survive_reorder_rename() {
        let mut snapshot = fixture(0.2, 1., 0., 0., 1000.);
        let mut track = snapshot["tracks"][0].clone();
        track["id"] = json!("source-b");
        snapshot["tracks"].as_array_mut().unwrap().push(track);
        let mut measurement = snapshot["visualization"]["tracks"][0].clone();
        measurement["track"] = json!("source-b");
        measurement["rms_pre_gain"] = json!(0.8);
        measurement["rms_post_gain"] = json!(0.8);
        snapshot["visualization"]["tracks"]
            .as_array_mut()
            .unwrap()
            .push(measurement);
        let mut state = settled(&snapshot);
        let front = state.placement("source-b", &state.sources["source-b"], rect());
        assert_eq!(
            state.hit(front.center, rect(), view()).as_deref(),
            Some("source-b")
        );
        let color = state.sources["source-a"].color;
        snapshot["tracks"].as_array_mut().unwrap().reverse();
        snapshot["tracks"][1]["name"] = json!("Renamed");
        state.sync(&snapshot, f64::INFINITY).unwrap();
        assert_eq!(state.sources["source-a"].color, color);
        assert!(
            state
                .presentation(rect(), view())
                .to_string()
                .find("null")
                .is_none()
        );
        let mut tied = fixture(0., 1., 0., 0., 1000.);
        let mut track = tied["tracks"][0].clone();
        track["id"] = json!("source-b");
        tied["tracks"].as_array_mut().unwrap().push(track);
        let mut measurement = tied["visualization"]["tracks"][0].clone();
        measurement["track"] = json!("source-b");
        tied["visualization"]["tracks"]
            .as_array_mut()
            .unwrap()
            .push(measurement);
        let tied_state = settled(&tied);
        assert_eq!(
            tied_state
                .hit(place(&tied_state).center, rect(), view())
                .as_deref(),
            Some("source-a"),
            "exact depth ties must match the renderer's first-emitted source"
        );
    }

    #[test]
    fn malformed_measurement_stops_motion_and_invalid_view_returns_null() {
        let mut snapshot = fixture(0.2, 1., 0., 0., 1000.);
        let mut state = settled(&snapshot);
        snapshot["visualization"]["tracks"][0]["stereo_width"] = json!(3.);
        assert!(state.sync(&snapshot, 0.1).is_err());
        assert!(!state.is_animating());
        let invalid = Viewport {
            height: 0.,
            ..view()
        };
        assert!(state.presentation(rect(), invalid).is_null());
        assert_eq!(db_gain(1.0e10), 16.);
        assert_eq!(db_gain(-1.0e10), 0.);
    }
}
