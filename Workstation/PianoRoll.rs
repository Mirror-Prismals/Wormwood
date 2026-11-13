//! Control-thread piano-roll interaction; the time/pitch lattice is never drawn.
//!
//! Adapted from the explicitly authorized Salamander137 PianoRollInputSystem,
//! PianoRollLayoutSystem and PianoRollResourceSystem interaction paths. Notes use
//! this clip's quarter clock, not the original global BPM/sample grid. Layer five
//! owns Editor; presentation consumes RenderNote and Viewport without drawing a
//! second UI. audition_note exposes the final editing preview to the layer-five
//! sender; no audio device, instrument or native callback is driven here.
//!
//! Dispatch each returned proposal through WorkstationDataSystem.PutClip, then
//! call sync with a fresh authoritative Snapshot, including after rejection.
//! A held gesture previews locally and commits once when released. Sync cancels
//! a gesture if its clip changed elsewhere; unrelated document revisions rebase
//! the unchanged clip safely. Undo is bounded to edits accepted in this editor.
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const MAX_NOTES: usize = 2048;
const MAX_DOCUMENT_NOTES: usize = 8192;
const MAX_HISTORY: usize = 32;
const MAX_HISTORY_NOTES: usize = 16_384;
const MIN_LENGTH: f64 = 0.000_001;
const EPSILON: f64 = 1e-9;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Rect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}
impl Rect {
    fn valid(self) -> bool {
        [self.left, self.top, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
            && self.width >= 0.
            && self.height >= 0.
    }
    fn contains(self, p: [f64; 2]) -> bool {
        p[0] >= self.left
            && p[0] <= self.left + self.width
            && p[1] >= self.top
            && p[1] <= self.top + self.height
    }
}

/// Logical positions have +Y downward. Button/key fields are held states;
/// wheel and enter_pressed are per-update events. Native key_events mode
/// preserves ordered short Enter taps separately from the physical held state.
#[derive(Clone, Debug, Default)]
pub(crate) struct Input {
    pub pointer: Option<[f64; 2]>,
    pub left: bool,
    pub right: bool,
    pub shift: bool,
    pub alt: bool,
    pub command: bool,
    pub wheel: [f64; 2],
    pub delete: bool,
    pub escape: bool,
    pub draw: bool,
    pub paint: bool,
    pub snap_toggle: bool,
    pub tab: bool,
    pub enter: bool,
    pub key_events: bool,
    pub enter_pressed: bool,
    pub create_range_pressed: bool,
    pub split_clip_pressed: bool,
    pub loop_range_pressed: bool,
    pub enter_modifiers: u8,
    pub home: bool,
    pub undo: bool,
    pub redo: bool,
    /// Left, right, up, down.
    pub arrows: [bool; 4],
    pub time_ruler: bool,
    pub pitch_ruler: bool,
    pub velocity_lane: bool,
}
impl Input {
    pub(crate) fn enter_pressed_since(&self, previous: &Self) -> bool {
        if self.key_events {
            self.enter_pressed
        } else {
            self.enter && !previous.enter
        }
    }
    fn enter_note_allowed(&self) -> bool {
        // Modified Enter does not insert. Use modifiers from the key event,
        // even if the chord has already been released before this frame.
        if self.key_events {
            self.enter_modifiers & 0x0f == 0
        } else {
            !self.alt && !self.shift && !self.command
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Viewport {
    pub first_quarter: f64,
    pub quarters_per_width: f64,
    pub lowest_pitch: f64,
    pub pitches_per_height: f64,
}
impl Default for Viewport {
    fn default() -> Self {
        Self {
            first_quarter: 0.,
            quarters_per_width: 16.,
            lowest_pitch: 48.,
            pitches_per_height: 36.,
        }
    }
}
impl Viewport {
    /// Pitch denotes a MIDI row; returned position is its center.
    pub(crate) fn project(self, quarter: f64, pitch: f64, rect: Rect) -> [f64; 2] {
        [
            rect.left + (quarter - self.first_quarter) / self.quarters_per_width * rect.width,
            rect.top + rect.height
                - (super::piano_key_layout::physical(pitch+0.5)-super::piano_key_layout::physical(self.lowest_pitch)) / self.key_span() * rect.height,
        ]
    }
    /// Returns continuous local quarters and row coordinate. Floor the latter
    /// for a MIDI pitch; the same transform drives both picking and placement.
    pub(crate) fn locate(self, p: [f64; 2], rect: Rect) -> (f64, f64) {
        (
            self.first_quarter + (p[0] - rect.left) / rect.width * self.quarters_per_width,
            super::piano_key_layout::row(self.physical_at(p,rect)),
        )
    }
    pub(crate) fn key_span(self)->f64{super::piano_key_layout::physical(self.lowest_pitch+self.pitches_per_height)-super::piano_key_layout::physical(self.lowest_pitch)}
    pub(crate) fn physical_at(self,p:[f64;2],rect:Rect)->f64{super::piano_key_layout::physical(self.lowest_pitch)+(rect.top+rect.height-p[1])/rect.height*self.key_span()}
    pub(crate) fn note_height(self,pitch:u8,rect:Rect)->f64{rect.height/self.key_span()*super::piano_key_layout::height(pitch)}
    fn anchor_pitch(&mut self,pitch:f64,fraction:f64){let target=super::piano_key_layout::physical(pitch);let(mut low,mut high)=(0.,128.-self.pitches_per_height);for _ in 0..40{let mid=(low+high)*0.5;let value=super::piano_key_layout::physical(mid)+fraction*(super::piano_key_layout::physical(mid+self.pitches_per_height)-super::piano_key_layout::physical(mid));if value<target{low=mid;}else{high=mid;}}self.lowest_pitch=(low+high)*0.5;}
    fn constrain(&mut self, length: f64) {
        self.quarters_per_width = self
            .quarters_per_width
            .clamp((1. / 64f64).min(length), length);
        self.first_quarter = self
            .first_quarter
            .clamp(0., (length - self.quarters_per_width).max(0.));
        self.pitches_per_height = self.pitches_per_height.clamp(6., 84.);
        self.lowest_pitch = self.lowest_pitch.clamp(0., 128. - self.pitches_per_height);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RenderNote {
    pub id: String,
    pub quarter: f64,
    pub length_quarters: f64,
    pub pitch: u8,
    pub velocity: f64,
    pub placement_age: f64,
    pub selected: bool,
    pub bend: f64,
}

#[derive(Clone, Debug, PartialEq)]
struct Note {
    bend: f64,
    id: String,
    quarter: f64,
    length: f64,
    pitch: u8,
    velocity: f64,
}
#[derive(Clone, Debug, PartialEq)]
struct Clip {
    edo:u16, scale:u8, root:u8,
    id: String,
    track: String,
    start: f64,
    bpm: f64,
    length: f64,
    numerator: u64,
    denominator: u64,
    notes: Vec<Note>,
}
impl Clip {
    fn metadata_matches(&self, other: &Self) -> bool {
        self.edo == other.edo && self.scale == other.scale && self.root == other.root && self.id == other.id
            && self.track == other.track
            && self.start == other.start
            && self.bpm == other.bpm
            && self.length == other.length
            && self.numerator == other.numerator
            && self.denominator == other.denominator
    }
    fn proposal(&self, notes: &[Note], revision: u64) -> Value {
        json!({"document_revision":revision,"id":self.id,"track":self.track,"kind":"midi",
            "edo":self.edo,"scale":self.scale,"root":self.root,"start_seconds":self.start,"bpm":self.bpm,"length_quarters":self.length,
            "meter":{"numerator":self.numerator,"denominator":self.denominator},
            "notes":notes.iter().map(|n| super::note_bend::write(super::piano_tuning::note(&json!({"id":n.id,"quarter":n.quarter,
                "length_quarters":n.length,"pitch":n.pitch,"velocity":n.velocity,"bend_semitones":n.bend}),self.edo),n.bend)).collect::<Vec<_>>()})
    }
}

fn finite(v: &Value, key: &str, low: f64, high: f64) -> Result<f64, String> {
    v[key]
        .as_f64()
        .filter(|v| v.is_finite() && *v >= low && *v <= high)
        .ok_or_else(|| format!("piano roll: invalid {key}"))
}
fn identity(v: &Value, key: &str) -> Result<String, String> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(|| format!("piano roll: invalid {key}"))
}
fn read_snapshot(snapshot: &Value, clip_id: &str) -> Result<(u64, Clip, usize), String> {
    let revision = snapshot["revision"]
        .as_u64()
        .ok_or("piano roll: snapshot has no revision")?;
    let clips = snapshot["clips"]
        .as_array()
        .filter(|a| a.len() <= 512)
        .ok_or("piano roll: snapshot has no bounded clips")?;
    let value = clips
        .iter()
        .find(|v| v["id"] == clip_id)
        .ok_or("piano roll: clip was removed")?;
    if value["kind"] != "midi" {
        return Err("piano roll requires a MIDI clip".into());
    }
    let edo=super::piano_tuning::edo(&value["edo"])?;
    let scale=value["scale"].as_u64().unwrap_or(0).min(6)as u8;let root=value["root"].as_u64().unwrap_or(0).min(11)as u8;
    let length = finite(value, "length_quarters", MIN_LENGTH, 65536.)?;
    let bpm = finite(value, "bpm", 1., 1000.)?;
    let start = finite(value, "start_seconds", 0., 86400.)?;
    if start + length * 60. / bpm > 86400. + EPSILON {
        return Err("piano roll exceeds document duration".into());
    }
    let numerator = value["meter"]["numerator"]
        .as_u64()
        .filter(|n| (1..=32).contains(n))
        .ok_or("invalid MIDI meter")?;
    let denominator = value["meter"]["denominator"]
        .as_u64()
        .filter(|d| [1, 2, 4, 8, 16, 32].contains(d))
        .ok_or("invalid MIDI meter")?;
    let values = value["notes"]
        .as_array()
        .filter(|a| a.len() <= MAX_NOTES)
        .ok_or("invalid piano-roll notes")?;
    let mut notes = Vec::with_capacity(values.len());
    let mut ids = BTreeSet::new();
    for n in values {
        let id = identity(n, "id")?;
        if !ids.insert(id.clone()) {
            return Err("duplicate piano-roll note identity".into());
        }
        let quarter = finite(n, "quarter", 0., length)?;
        let note_length = finite(n, "length_quarters", MIN_LENGTH, length)?;
        if quarter + note_length > length + EPSILON {
            return Err("note exceeds piano-roll clip".into());
        }
        notes.push(Note {
            bend: super::note_bend::read(&n["bend_semitones"])?,
            id,
            quarter,
            length: note_length,
            pitch: {let anchor=n["pitch"].as_u64().filter(|p|*p<=127).ok_or("invalid MIDI pitch")?as u8;
                let cents=super::piano_tuning::cents(&n["tuning_cents"],anchor)?;super::piano_tuning::key(f64::from(anchor),cents,edo)},
            velocity: finite(n, "velocity", 0., 1.)?,
        });
    }
    canonicalize(&mut notes);
    let other_notes: usize = clips
        .iter()
        .filter(|c| c["id"] != clip_id)
        .filter_map(|c| c["notes"].as_array())
        .map(Vec::len)
        .sum();
    if other_notes + notes.len() > MAX_DOCUMENT_NOTES {
        return Err("document note capacity exceeded".into());
    }
    Ok((
        revision,
        Clip { edo,scale,root,
            id: identity(value, "id")?,
            track: identity(value, "track")?,
            start,
            bpm,
            length,
            numerator,
            denominator,
            notes,
        },
        other_notes,
    ))
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Draw,
    Paint,
    Arp,
}
#[derive(Clone)]
enum GestureKind {
    Bend { id: String, start_y: f64, initial: f64 },
    Move {
        id: String,
        offset: f64,
    },
    Resize {
        id: String,
    },
    Paint {
        last_by_pitch: [Option<f64>; 128],
        last_global: Option<f64>,
        last_pitch: Option<u8>,
    },
    Stamp,
    Erase,
    Velocity {
        id: String,
    },
    TimeRuler {
        last: [f64; 2],
    },
    PitchRuler {
        last: [f64; 2],
    },
}
struct Gesture {
    before: Vec<Note>,
    base: Vec<Note>,
    kind: GestureKind,
}
#[derive(Clone)]
struct History {
    before: Vec<Note>,
    after: Vec<Note>,
}
#[derive(Clone, Copy)]
enum HistoryAction {
    New,
    Undo,
    Redo,
}
struct Pending {
    before: Vec<Note>,
    after: Vec<Note>,
    revision: u64,
    action: HistoryAction,
}

pub(crate) struct Editor {
    clip: Option<Clip>,
    revision: u64,
    other_notes: usize,
    draft: Vec<Note>,
    view: Viewport,
    selected: Option<String>,
    cursor_quarter: f64,
    cursor_pitch: u8,
    mode: Mode,
    arp_count: usize,
    arp_direction: usize,
    arp_extension: usize,
    arp_chord: bool,
    snap: Option<f64>,
    remembered_snap: f64,
    last_length: f64,
    last_velocity: f64,
    serial: u64,
    gesture: Option<Gesture>,
    pending: Option<Pending>,
    previous: Input,
    ages: BTreeMap<String, f64>,
    undo: VecDeque<History>,
    redo: VecDeque<History>,
    notice: Option<String>,
}
impl Default for Editor {
    fn default() -> Self {
        Self {
            clip: None,
            revision: 0,
            other_notes: 0,
            draft: vec![],
            view: Viewport::default(),
            selected: None,
            cursor_quarter: 0.,
            cursor_pitch: 60,
            mode: Mode::Draw,
            arp_count: 3, arp_direction: 0, arp_extension: 0, arp_chord: false,
            snap: None,
            remembered_snap: 0.25,
            last_length: 0.25,
            last_velocity: 1.,
            serial: 0,
            gesture: None,
            pending: None,
            previous: Input::default(),
            ages: BTreeMap::new(),
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            notice: None,
        }
    }
}

impl Editor {
    pub(crate) fn open(&mut self, snapshot: &Value, clip_id: &str) -> Result<(), String> {
        let (revision, clip, other_notes) = read_snapshot(snapshot, clip_id)?;
        *self = Self::default();
        self.revision = revision;
        self.other_notes = other_notes;
        self.view.quarters_per_width = clip.length;
        // Fit once when opening. A small empty field needs a usable octave
        // around middle C; existing notes get two rows of breathing room on
        // each side where the MIDI bounds and 84-row overview limit allow it.
        // sync deliberately keeps the user's later pitch pan/zoom unchanged.
        if let (Some(low), Some(high)) = (
            clip.notes.iter().map(|note| note.pitch).min(),
            clip.notes.iter().map(|note| note.pitch).max(),
        ) {
            let pitch_span = f64::from(high - low) + 1.;
            self.view.pitches_per_height = (pitch_span + 4.).clamp(12., 84.);
            self.view.lowest_pitch = if pitch_span > 84. {
                // Centering sparse extremes such as 0/127 inside an 84-row
                // window would hide every note. Anchor the capped overview at
                // its lowest actual note, with two rows below when possible;
                // the user can pan upward to the rest of the range.
                f64::from(low) - 2.
            } else {
                ((f64::from(low) + f64::from(high) + 1. - self.view.pitches_per_height) * 0.5)
                    .floor()
            };
        } else {
            self.view.lowest_pitch = 54.;
            self.view.pitches_per_height = 12.;
        }
        self.view.constrain(clip.length);
        // Pitch navigation starts in the first visible note's register,
        // without selecting it or moving the insertion time from zero.
        // Keeping the default middle-C cursor after a
        // high/low fit would make Home, Left or Delete reveal an unrelated row.
        if let Some(note) = clip.notes.iter().find(|note| {
            f64::from(note.pitch) >= self.view.lowest_pitch
                && f64::from(note.pitch) + 1.
                    <= self.view.lowest_pitch + self.view.pitches_per_height
        }) {
            self.cursor_pitch = note.pitch;
        }
        self.draft = clip.notes.clone();
        self.clip = Some(clip);
        self.refresh_ages();
        Ok(())
    }
    pub(crate) fn is_open(&self) -> bool {
        self.clip.is_some()
    }
    pub(crate) fn close(&mut self) {
        *self = Self::default();
    }
    pub(crate) fn set_workspace_time(&mut self,first:f64,span:f64){
        if first.is_finite()&&span.is_finite()&&span>0.{self.view.first_quarter=first;self.view.quarters_per_width=span;}
    }
    pub(crate) fn viewport(&self) -> Viewport {
        self.view
    }
    /// Consume the held state that opened this view without interpreting it as
    /// an edit. The controller calls this after its suppressed opening update;
    /// a held activation key/button must be released before a new press edits.
    pub(crate) fn workspace_time(&mut self) {
        if let Some(clip)=&self.clip {self.view.first_quarter=0.;self.view.quarters_per_width=clip.length;}
    }
    pub(crate) fn settings_request(&self,bpm:f64,edo:u16,numerator:u64,denominator:u64,scale:u8,root:u8)->Result<Value,String>{
        if self.gesture.is_some()||self.pending.is_some(){return Err("Finish the note gesture before changing clip settings".into());}
        let mut clip=self.clip.clone().ok_or("Open a MIDI clip first")?;
        clip.bpm=bpm;clip.edo=edo;clip.numerator=numerator;clip.denominator=denominator;clip.scale=scale;clip.root=root;
        Ok(clip.proposal(&self.draft,self.revision))
    }
    /// Replay only a native pointer edge. Keep prior keyboard-held state so
    /// replay cannot synthesize fresh snap/undo/key presses; Step applies the
    /// current keyboard and wheel sample exactly once after queued edges.
    pub(crate) fn pointer_edge(&self,current:&Input,point:[f64;2],left:bool)->Input {
        let mut edge=self.previous.clone();edge.pointer=Some(point);edge.left=left;
        edge.right=current.right;edge.shift=current.shift;edge.alt=current.alt;edge.command=current.command;
        // These queued edges belong to the note field/captured note, not
        // to the ruler beneath the cursor at the end of a slow frame.
        edge.time_ruler=false;edge.pitch_ruler=false;edge.velocity_lane=false;
        edge.wheel=[0.;2];edge.key_events=true;edge.enter_pressed=false;
        edge.split_clip_pressed=false;edge.create_range_pressed=false;edge.loop_range_pressed=false;edge
    }
    pub(crate) fn prime_input(&mut self, input: &Input) {
        self.previous = input.clone();
        self.previous.enter_pressed = false;
    }
    pub(crate) fn set_snap_quarters(&mut self, snap: Option<f64>) -> Result<(), String> {
        if self.gesture.is_some() || self.pending.is_some() {
            return Err("finish the note gesture before changing snap".into());
        }
        if let Some(step) = snap {
            if !step.is_finite() || !(1. / 64. ..=128.).contains(&step) {
                return Err("snap must be 1/64..128 local quarters".into());
            }
            self.remembered_snap = step;
        }
        self.snap = snap;
        Ok(())
    }
    /// Same audition precedence as Cardinal's final live-note publication:
    /// active move/resize/velocity note, then painting pitch. No document edit.
    pub(crate) fn audition_note(&self,input:&Input,rect:Rect)->Option<(String,u8,f64)> {
        if !input.left {return None;}
        let clip=self.clip.as_ref()?;
        let gesture=self.gesture.as_ref()?;
        let note=match &gesture.kind {
            GestureKind::Move{id,..}|GestureKind::Resize{id}|GestureKind::Velocity{id}=>{
                let note=self.draft.iter().find(|n|n.id==*id)?;(note.pitch,note.velocity)
            },
            GestureKind::Stamp => return None,
            GestureKind::Paint{..}=>{
                let point=input.pointer?;
                if rect.width<=1.||rect.height<=1.||point[0]<rect.left||point[0]>=rect.left+rect.width||point[1]<rect.top||point[1]>=rect.top+rect.height{return None;}
                let (_,row)=self.view.locate(point,rect);
                if !(0. ..128.).contains(&row){return None;}(row.floor() as u8,self.last_velocity)
            },
            _=>return None,
        };
        Some((clip.track.clone(),note.0,note.1.clamp(1./127.,1.)))
    }
    pub(crate) fn notes(&self) -> Vec<RenderNote> {
        self.draft
            .iter()
            .map(|n| RenderNote {
                id: n.id.clone(),
                quarter: n.quarter,
                length_quarters: n.length,
                pitch: n.pitch,
                velocity: n.velocity,
                placement_age: self.ages.get(&n.id).copied().unwrap_or(0.),
                bend: n.bend,
                selected: self.selected.as_ref() == Some(&n.id),
            })
            .collect()
    }
    pub(crate) fn status(&self) -> Value {
        json!({"open":self.is_open(),"clip_id":self.clip.as_ref().map(|c| &c.id),"revision":self.revision,
            "edo":self.clip.as_ref().map_or(12,|c|c.edo),"scale":self.clip.as_ref().map_or(0,|c|c.scale),"root":self.clip.as_ref().map_or(0,|c|c.root),"arp_count":self.arp_count,"arp_direction":self.arp_direction,"arp_extension":self.arp_extension,"arp_chord":self.arp_chord,"selected_note":self.selected,"mode":if self.mode==Mode::Draw {"draw"}else if self.mode==Mode::Paint {"paint"}else{"arp"},
            "snap_quarters":self.snap,"grid_visible":false,"audition_connected":false,
            "undo_available":!self.undo.is_empty(),"redo_available":!self.redo.is_empty(),
            "undo_scope":"accepted_edits_in_this_editor_session","pending_commit":self.pending.is_some(),
            "gesture_active":self.gesture.is_some(),"notice":self.notice,
            "viewport":{"first_quarter":self.view.first_quarter,"quarters_per_width":self.view.quarters_per_width,
                "lowest_pitch":self.view.lowest_pitch,"pitches_per_height":self.view.pitches_per_height}})
    }

    /// A fresh snapshot is the only acknowledgement. Equal revision with a
    /// pending proposal means it was rejected; previews roll back in that case.
    pub(crate) fn sync(&mut self, snapshot: &Value) -> Result<(), String> {
        let Some(current) = self.clip.as_ref() else {
            return Ok(());
        };
        let (revision, incoming, other_notes) = match read_snapshot(snapshot, &current.id) {
            Ok(value) => value,
            Err(error) => {
                self.close();
                return Err(error);
            }
        };
        if revision < self.revision {
            return Err("stale piano-roll snapshot".into());
        }
        if let Some(pending) = self.pending.take() {
            let accepted = revision > pending.revision
                && incoming.metadata_matches(current)
                && incoming.notes == pending.after;
            if accepted {
                match pending.action {
                    HistoryAction::New => {
                        self.undo.push_back(History {
                            before: pending.before,
                            after: pending.after,
                        });
                        self.redo.clear();
                        self.trim_history();
                    }
                    HistoryAction::Undo => {
                        if let Some(entry) = self.undo.pop_back() {
                            self.redo.push_back(entry);
                        }
                    }
                    HistoryAction::Redo => {
                        if let Some(entry) = self.redo.pop_back() {
                            self.undo.push_back(entry);
                        }
                    }
                }
                self.notice = None;
            } else {
                if &incoming != current {
                    self.undo.clear();
                    self.redo.clear();
                }
                self.notice = Some("The document rejected or superseded this edit; its current notes were restored.".into());
            }
            self.gesture = None;
            self.draft = incoming.notes.clone();
        } else if &incoming != current {
            self.gesture = None;
            self.undo.clear();
            self.redo.clear();
            self.draft = incoming.notes.clone();
            self.notice =
                Some("The clip changed elsewhere; the editor refreshed from the document.".into());
        } else if self.gesture.is_none() {
            self.draft = incoming.notes.clone();
        }
        self.revision = revision;
        self.other_notes = other_notes;
        self.view.constrain(incoming.length);
        self.clip = Some(incoming);
        if !self
            .draft
            .iter()
            .any(|n| Some(&n.id) == self.selected.as_ref())
        {
            self.selected = None;
        }
        self.refresh_ages();
        Ok(())
    }
    fn trim_history(&mut self) {
        while self.undo.len() + self.redo.len() > MAX_HISTORY
            || self
                .undo
                .iter()
                .chain(&self.redo)
                .map(|h| h.before.len() + h.after.len())
                .sum::<usize>()
                > MAX_HISTORY_NOTES
        {
            if self.undo.pop_front().is_none() {
                self.redo.pop_front();
            }
        }
    }
    fn refresh_ages(&mut self) {
        let ids: BTreeSet<_> = self.draft.iter().map(|n| n.id.as_str()).collect();
        self.ages.retain(|id, _| ids.contains(id.as_str()));
        for note in &self.draft {
            self.ages.entry(note.id.clone()).or_insert(0.);
        }
    }
    fn minimum(&self) -> f64 {
        self.snap.map_or(0.25, |s| s.min(0.25)).max(MIN_LENGTH)
    }
    fn step(&self) -> f64 {
        self.snap.unwrap_or(0.25)
    }
    fn next_id(&mut self) -> Result<String, String> {
        // Existing caller IDs are never rewritten; retries and undo retain IDs.
        for _ in 0..=MAX_NOTES {
            self.serial = self
                .serial
                .checked_add(1)
                .ok_or("piano-roll note identity exhausted")?;
            let id = format!("bubble-note-{}", self.serial);
            if !self.draft.iter().any(|n| n.id == id) {
                return Ok(id);
            }
        }
        Err("piano-roll note identity capacity reached".into())
    }
    fn can_add(&self) -> bool {
        self.draft.len() < MAX_NOTES && self.other_notes + self.draft.len() < MAX_DOCUMENT_NOTES
    }
    fn proposal(
        &mut self,
        mut before: Vec<Note>,
        action: HistoryAction,
    ) -> Result<Option<Value>, String> {
        // Document::parse_clip canonicalizes every accepted MIDI edit by
        // (quarter,id). Pending acknowledgements and history must use that same
        // order, including a move across another note or an earlier insertion.
        canonicalize(&mut before);
        canonicalize(&mut self.draft);
        if before == self.draft {
            return Ok(None);
        }
        let clip = self.clip.as_ref().ok_or("piano roll is closed")?;
        if self.draft.len() > MAX_NOTES || self.other_notes + self.draft.len() > MAX_DOCUMENT_NOTES
        {
            self.draft = clip.notes.clone();
            self.refresh_ages();
            return Err("piano-roll edit exceeds document note capacity".into());
        }
        let payload = clip.proposal(&self.draft, self.revision);
        self.pending = Some(Pending {
            before,
            after: self.draft.clone(),
            revision: self.revision,
            action,
        });
        self.refresh_ages();
        Ok(Some(payload))
    }

    pub(crate) fn update(
        &mut self,
        input: &Input,
        rect: Rect,
        dt: f64,
    ) -> Result<Option<Value>, String> {
        if !rect.valid()
            || !dt.is_finite()
            || !(0. ..=1.).contains(&dt)
            || input
                .pointer
                .is_some_and(|p| p.iter().any(|v| !v.is_finite()))
            || input
                .wheel
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 1000.)
        {
            return Err("invalid piano-roll interaction coordinates or elapsed time".into());
        }
        for age in self.ages.values_mut() {
            *age = (*age + dt).min(1.35);
        }
        let previous = std::mem::replace(&mut self.previous, input.clone());
        if self.clip.is_none() || self.pending.is_some() {
            return Ok(None);
        }
        if input.escape && !previous.escape {
            self.close();
            return Ok(None);
        }
        // A retracted panel may have no usable rectangle. It suspends the
        // editor and cancels an uncommitted gesture instead of dividing by zero
        // or mistaking disappearance for an intentional pointer release.
        if rect.width == 0. || rect.height == 0. {
            self.gesture = None;
            self.draft = self.clip.as_ref().unwrap().notes.clone();
            self.refresh_ages();
            return Ok(None);
        }
        if self.gesture.is_none() {
            if input.draw && !previous.draw {
                self.mode = Mode::Draw;
            }
            if input.paint && !previous.paint {
                self.mode = Mode::Paint;
            }
            if input.snap_toggle && !previous.snap_toggle {
                self.snap = if self.snap.is_some() {
                    None
                } else {
                    Some(self.remembered_snap)
                };
            }
            if input.undo && !previous.undo {
                if let Some(history) = self.undo.back() {
                    let before = self.draft.clone();
                    self.draft = history.before.clone();
                    return self.proposal(before, HistoryAction::Undo);
                }
            }
            if input.redo && !previous.redo {
                if let Some(history) = self.redo.back() {
                    let before = self.draft.clone();
                    self.draft = history.after.clone();
                    return self.proposal(before, HistoryAction::Redo);
                }
            }
        }
        self.wheel(input, rect);
        let inside = input.pointer.is_some_and(|p| rect.contains(p));
        if self.gesture.is_none() && inside && input.right && !previous.right {
            self.gesture = Some(Gesture {
                before: self.draft.clone(),
                base: self.draft.clone(),
                kind: GestureKind::Erase,
            });
        } else if self.gesture.is_none() && inside && input.left && !previous.left && !input.right {
            let p = input.pointer.unwrap();
            let before = self.draft.clone();
            let (quarter, pitch) = self.view.locate(p, rect);
            let kind = if input.time_ruler {
                Some(GestureKind::TimeRuler { last: p })
            } else if input.pitch_ruler {
                Some(GestureKind::PitchRuler { last: p })
            } else if input.velocity_lane {
                self.velocity_hit(p, rect)
                    .map(|id| GestureKind::Velocity { id })
            } else if self.mode == Mode::Arp && (0. ..128.).contains(&pitch) {
                self.create_arp(quarter, pitch.floor() as u8)?;
                Some(GestureKind::Stamp)
            } else if let Some(index) = self.hit(p, rect, false) {
                let note = &self.draft[index];
                self.selected = Some(note.id.clone());
                self.cursor_quarter = note.quarter;
                self.cursor_pitch = note.pitch;
                let start = self.view.project(note.quarter, note.pitch as f64, rect)[0];
                let pick_width = (note.length / self.view.quarters_per_width * rect.width).max(2.);
                // Retain the original eight-pixel handle on wide notes, but
                // preserve a movable body on short notes. Use hit()'s exact
                // minimum width so even subpixel durations have both zones.
                let handle_width = (pick_width * 0.35).min(8.);
                Some(if input.alt {
                    GestureKind::Bend { id: note.id.clone(), start_y: p[1], initial: if input.shift { 0. } else { note.bend } }
                } else if p[0] >= start + pick_width - handle_width {
                    GestureKind::Resize {
                        id: note.id.clone(),
                    }
                } else {
                    GestureKind::Move {
                        id: note.id.clone(),
                        offset: quarter - note.quarter,
                    }
                })
            } else if (0. ..128.).contains(&pitch) {
                self.selected = None;
                self.cursor_quarter = quarter.clamp(0., self.clip.as_ref().unwrap().length);
                self.cursor_pitch = pitch.floor() as u8;
                if self.mode == Mode::Paint {
                    Some(GestureKind::Paint {
                        last_by_pitch: [None; 128],
                        last_global: None,
                        last_pitch: None,
                    })
                } else {
                    self.create(quarter, pitch.floor() as u8, true)?.map(|id| {
                        let note = self.draft.iter().find(|n| n.id == id).unwrap();
                        GestureKind::Move {
                            id,
                            offset: quarter - note.quarter,
                        }
                    })
                }
            } else {
                None
            };
            if let Some(kind) = kind {
                self.gesture = Some(Gesture {
                    before,
                    base: self.draft.clone(),
                    kind,
                });
            }
        }
        if let Some(mut gesture) = self.gesture.take() {
            let held = if matches!(gesture.kind, GestureKind::Erase) {
                input.right
            } else {
                input.left
            };
            if let Some(p) = input.pointer {
                self.advance_gesture(&mut gesture, p, rect, held)?;
            }
            if held {
                self.gesture = Some(gesture);
                self.refresh_ages();
                return Ok(None);
            }
            self.refresh_ages();
            return self.proposal(gesture.before, HistoryAction::New);
        }
        if !input.left && !input.right {
            return self.keyboard(input, &previous, rect);
        }
        Ok(None)
    }

    fn wheel(&mut self, input: &Input, rect: Rect) {
        if input.wheel == [0., 0.] || self.gesture.is_some() {
            return;
        }
        let p = input
            .pointer
            .unwrap_or([rect.left + rect.width * 0.5, rect.top + rect.height * 0.5]);
        if !rect.contains(p) {
            return;
        }
        let (quarter, pitch) = self.view.locate(p, rect);
        self.view.first_quarter += input.wheel[0] * 40. / rect.width * self.view.quarters_per_width;
        if input.shift {
            self.view.first_quarter +=
                input.wheel[1] * 40. / rect.width * self.view.quarters_per_width;
        } else if input.time_ruler || input.command {
            self.view.quarters_per_width *= 1.18f64.powf(input.wheel[1].signum());
            self.view.constrain(self.clip.as_ref().unwrap().length);
            self.view.first_quarter =
                quarter - (p[0] - rect.left) / rect.width * self.view.quarters_per_width;
        } else if input.alt || input.pitch_ruler {
            self.view.pitches_per_height /= 1.1f64.powf(input.wheel[1].signum());
            self.view.pitches_per_height = self.view.pitches_per_height.clamp(6., 84.);
            self.view.anchor_pitch(pitch,(rect.top+rect.height-p[1])/rect.height);
        } else {
            self.view.lowest_pitch +=
                input.wheel[1] * 30. / rect.height * self.view.pitches_per_height;
        }
        self.view.constrain(self.clip.as_ref().unwrap().length);
    }
    fn hit(&self, p: [f64; 2], rect: Rect, erase: bool) -> Option<usize> {
        if !rect.contains(p) {
            return None;
        }
        let (pad_x, pad_y) = if erase { (6., 4.) } else { (0., 0.) };
        self.draft.iter().enumerate().filter_map(|(i, n)| {
            let height=self.view.note_height(n.pitch,rect);
            let fraction = ((self.view.locate(p, rect).0 - n.quarter) / n.length).clamp(0., 1.);
            let end_pitch = n.pitch as f64 + n.bend * self.clip.as_ref().unwrap().edo as f64 / 12.;
            let mut center = self.view.project(n.quarter, n.pitch as f64, rect);
            let end_y = self.view.project(n.quarter, end_pitch, rect)[1];
            center[1] += (end_y - center[1]) * super::note_bend::value(1., fraction);
            let width = (n.length / self.view.quarters_per_width * rect.width).max(2.);
            (p[0] >= center[0] - pad_x
                && p[0] <= center[0] + width + pad_x
                && p[1] >= center[1] - height * 0.5 - pad_y
                && p[1] <= center[1] + height * 0.5 + pad_y)
                .then_some((i,(p[1]-center[1]).abs()))
        }).min_by(|a,b|a.1.total_cmp(&b.1)).map(|v|v.0)
    }
    fn velocity_hit(&self, p: [f64; 2], rect: Rect) -> Option<String> {
        self.draft
            .iter()
            .filter_map(|n| {
                let x = self.view.project(n.quarter, n.pitch as f64, rect)[0];
                let width = (n.length / self.view.quarters_per_width * rect.width).clamp(7., 16.);
                let distance = (x - p[0]).max(p[0] - x - width).max(0.);
                (distance <= 7.).then_some((distance, &n.id))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, id)| id.clone())
    }
    pub(crate) fn arp_control(&mut self, control: usize, direction: i32) {
        if self.gesture.is_some() || self.pending.is_some() { return; }
        match control {
            0 => self.mode = Mode::Arp,
            1 => self.arp_direction = (self.arp_direction as i32 + direction).rem_euclid(3) as usize,
            2 => self.arp_count = (self.arp_count as i32 + direction).clamp(1, 32) as usize,
            3 => self.arp_extension = (self.arp_extension as i32 + direction).rem_euclid(7) as usize,
            _ => self.arp_chord = !self.arp_chord,
        }
    }
    fn create_arp(&mut self, quarter: f64, pitch: u8) -> Result<(), String> {
        let clip = self.clip.as_ref().unwrap();
        let pitches = arp_pitches(pitch, clip.scale, clip.root, clip.edo, self.arp_extension, self.arp_count, self.arp_direction);
        if pitches.len() != self.arp_count {
            self.notice = Some("Arp exceeds the MIDI pitch range.".into());
            return Ok(());
        }
        let q = snap_floor(quarter.max(0.), self.snap);
        let length = self.last_length.max(self.minimum());
        let step = self.snap.unwrap_or(length);
        let before = self.draft.clone();
        for (i, pitch) in pitches.into_iter().enumerate() {
            let onset = q + if self.arp_chord { 0. } else { i as f64 * step };
            if onset + length > self.clip.as_ref().unwrap().length + EPSILON || !self.can_add() {
                self.draft = before;
                self.notice = Some("Arp needs more clip space or note capacity.".into());
                return Ok(());
            }
            let note = Note { id: self.next_id()?, quarter: onset, length, pitch, bend: 0., velocity: self.last_velocity.clamp(1./127.,1.) };
            if let Some(notes) = place(&self.draft, note, self.clip.as_ref().unwrap().length, self.snap, self.minimum(), false) {
                self.draft = notes;
            } else {
                self.draft = before;
                self.notice = Some("Arp overlaps existing notes.".into());
                return Ok(());
            }
        }
        self.refresh_ages();
        Ok(())
    }

    fn create(
        &mut self,
        quarter: f64,
        pitch: u8,
        allow_shift: bool,
    ) -> Result<Option<String>, String> {
        if !self.can_add() {
            self.notice = Some("Piano-roll note capacity reached.".into());
            return Ok(None);
        }
        let id = self.next_id()?;
        let clip_length = self.clip.as_ref().unwrap().length;
        let q = snap_floor(quarter.max(0.), self.snap);
        let length = self
            .last_length
            .max(self.minimum())
            .min((clip_length - q).max(0.));
        let pitch=self.clip.as_ref().map_or(pitch,|c|super::piano_tuning::snap_key(pitch,c.scale,c.root,c.edo));
        let note = Note {
            bend: 0.,
            id: id.clone(),
            quarter: q,
            length,
            pitch,
            velocity: self.last_velocity.clamp(1. / 127., 1.),
        };
        if let Some(notes) = place(
            &self.draft,
            note,
            clip_length,
            self.snap,
            self.minimum(),
            allow_shift,
        ) {
            self.last_length = notes.last().unwrap().length;
            self.draft = notes;
            self.selected = Some(id.clone());
            self.refresh_ages();
            Ok(Some(id))
        } else {
            Ok(None)
        }
    }

    fn advance_gesture(
        &mut self,
        gesture: &mut Gesture,
        p: [f64; 2],
        rect: Rect,
        held: bool,
    ) -> Result<(), String> {
        let clip_length = self.clip.as_ref().unwrap().length;
        let (quarter, pitch) = self.view.locate(p, rect);
        match &mut gesture.kind {
            GestureKind::Stamp => {},
            GestureKind::Bend { id, start_y, initial } => {
                if let Some(note) = self.draft.iter_mut().find(|n| &n.id == id) {
                    let origin = self.view.project(note.quarter, note.pitch as f64, rect);
                    let (_, target) = self.view.locate([origin[0], origin[1] + p[1] - *start_y], rect);
                    let edo = self.clip.as_ref().unwrap().edo as f64;
                    note.bend = (*initial + (target - note.pitch as f64 - 0.5) * 12. / edo).clamp(-2., 2.);
                }
            }
            GestureKind::Move { id, offset } => {
                if let Some(original) = gesture.base.iter().find(|n| &n.id == id) {
                    let mut note = original.clone();
                    note.quarter = snap_floor(quarter - *offset, self.snap)
                        .clamp(0., (clip_length - note.length).max(0.));
                    if p[1] >= rect.top
                        && p[1] <= rect.top + rect.height
                        && (0. ..128.).contains(&pitch)
                    {
                        note.pitch = pitch.floor() as u8;
                    }
                    let others: Vec<_> = gesture
                        .base
                        .iter()
                        .filter(|n| &n.id != id)
                        .cloned()
                        .collect();
                    let minimum = self.minimum().min(original.length);
                    if let Some(mut notes) =
                        place(&others, note, clip_length, self.snap, minimum, true)
                    {
                        // Picking a note must not reorder the document or
                        // create a false edit solely because place appends.
                        let original_index = gesture.base.iter().position(|n| &n.id == id).unwrap();
                        let moved = notes.pop().unwrap();
                        notes.insert(original_index, moved);
                        self.draft = notes;
                    }
                }
            }
            GestureKind::Resize { id } => {
                self.draft = gesture.base.clone();
                if let Some(index) = self.draft.iter().position(|n| &n.id == id) {
                    let note = &self.draft[index];
                    let end = next_start(&self.draft, note.pitch, note.quarter, Some(id))
                        .unwrap_or(clip_length)
                        .min(clip_length);
                    let available = (end - note.quarter).max(MIN_LENGTH);
                    let minimum = self.step().min(available).max(MIN_LENGTH);
                    let length =
                        snap_nearest(quarter - note.quarter, self.snap).clamp(minimum, available);
                    self.draft[index].length = length;
                    self.last_length = length;
                }
            }
            GestureKind::Erase => {
                if held && let Some(index) = self.hit(p, rect, true) {
                    let note = self.draft.remove(index);
                    if self.selected.as_ref() == Some(&note.id) {
                        self.selected = None;
                    }
                }
            }
            GestureKind::Velocity { id } => {
                if let Some(note) = self.draft.iter_mut().find(|n| &n.id == id) {
                    note.velocity =
                        ((rect.top + rect.height - p[1]) / rect.height).clamp(1. / 127., 1.);
                    self.last_velocity = note.velocity;
                    self.selected = Some(id.clone());
                }
            }
            GestureKind::Paint {
                last_by_pitch,
                last_global,
                last_pitch,
            } => {
                if held
                    && rect.contains(p)
                    && (0. ..128.).contains(&pitch)
                    && self.hit(p, rect, false).is_none()
                {
                    let target_pitch = pitch.floor() as u8;
                    let length = self.last_length.max(self.minimum());
                    let last = last_by_pitch[target_pitch as usize];
                    let placement = if self.snap.is_some() {
                        let q = snap_floor(quarter, self.snap);
                        last.is_none_or(|old| (q - old).abs() + EPSILON >= length)
                            .then_some(q)
                    } else if let Some(base) = last.or(*last_global) {
                        let delta = quarter - base;
                        ((delta.abs() + EPSILON) >= length).then(|| {
                            base + delta.signum() * (delta.abs() / length).floor() * length
                        })
                    } else {
                        Some(quarter)
                    };
                    if let Some(q) = placement.filter(|q| *q >= 0. && *q < clip_length) {
                        let begin = last_pitch.unwrap_or(target_pitch);
                        let step: i16 = if target_pitch >= begin { 1 } else { -1 };
                        let mut row = if begin == target_pitch {
                            begin as i16
                        } else {
                            begin as i16 + step
                        };
                        loop {
                            let pitch = row as u8;
                            if last_by_pitch[pitch as usize]
                                .is_none_or(|old| (q - old).abs() >= length * 0.5)
                            {
                                let allow = self.snap.is_some()
                                    || last.or(*last_global).is_none_or(|old| quarter >= old);
                                if let Some(id) = self.create(q, pitch, allow)? {
                                    let actual =
                                        self.draft.iter().find(|n| n.id == id).unwrap().quarter;
                                    last_by_pitch[pitch as usize] = Some(actual);
                                    *last_global = Some(actual);
                                }
                            }
                            if row == target_pitch as i16 {
                                break;
                            }
                            row += step;
                        }
                        *last_pitch = Some(target_pitch);
                    }
                }
            }
            GestureKind::TimeRuler { last } => {
                let delta = [p[0] - last[0], p[1] - last[1]];
                self.view.quarters_per_width *= (-delta[1] * 0.01).clamp(-20., 20.).exp();
                self.view.constrain(clip_length);
                self.view.first_quarter = quarter
                    - (p[0] - rect.left) / rect.width * self.view.quarters_per_width
                    - delta[0] / rect.width * self.view.quarters_per_width;
                self.view.constrain(clip_length);
                *last = p;
            }
            GestureKind::PitchRuler { last } => {
                let delta = [p[0] - last[0], p[1] - last[1]];
                let center = self.view.lowest_pitch + self.view.pitches_per_height * 0.5;
                self.view.pitches_per_height /= (delta[0] * 0.01).clamp(-20., 20.).exp();
                self.view.pitches_per_height = self.view.pitches_per_height.clamp(6., 84.);
                self.view.lowest_pitch = center - self.view.pitches_per_height * 0.5
                    + delta[1] / rect.height * self.view.pitches_per_height;
                self.view.constrain(clip_length);
                *last = p;
            }
        }
        Ok(())
    }

    fn keyboard(
        &mut self,
        input: &Input,
        previous: &Input,
        rect: Rect,
    ) -> Result<Option<Value>, String> {
        if input.tab && !previous.tab && !input.alt {
            let mut order: Vec<_> = self.draft.iter().collect();
            order.sort_by(|a, b| {
                a.quarter
                    .total_cmp(&b.quarter)
                    .then(a.pitch.cmp(&b.pitch))
                    .then(a.id.cmp(&b.id))
            });
            if !order.is_empty() {
                let current = order
                    .iter()
                    .position(|n| Some(&n.id) == self.selected.as_ref());
                let next = match current {
                    Some(i) if input.shift => (i + order.len() - 1) % order.len(),
                    Some(i) => (i + 1) % order.len(),
                    None if input.shift => order.len() - 1,
                    None => 0,
                };
                self.selected = Some(order[next].id.clone());
                self.cursor_quarter = order[next].quarter;
                self.cursor_pitch = order[next].pitch;
            }
        }
        // A later-held Cmd/Ctrl must not cancel an earlier plain Enter event.
        // Isolate that event so the current command chord still cannot move,
        // resize or delete notes through the ordinary held-key paths below.
        let enter_only;
        let input = if input.command {
            if !(input.key_events && input.enter_pressed && input.enter_note_allowed()) {
                return Ok(None);
            }
            enter_only = Input {
                key_events: true,
                enter_pressed: true,
                ..Input::default()
            };
            &enter_only
        } else {
            input
        };
        let left = input.arrows[0] && !previous.arrows[0];
        let right = input.arrows[1] && !previous.arrows[1];
        let up = input.arrows[2] && !previous.arrows[2];
        let down = input.arrows[3] && !previous.arrows[3];
        let horizontal = i32::from(right) - i32::from(left);
        let vertical = i32::from(up) - i32::from(down);
        let before = self.draft.clone();
        let clip_length = self.clip.as_ref().unwrap().length;
        if let Some(index) = self
            .draft
            .iter()
            .position(|n| Some(&n.id) == self.selected.as_ref())
        {
            let mut note = self.draft[index].clone();
            if input.alt && !input.shift && vertical != 0 {
                note.velocity = (note.velocity + vertical as f64 / 127.).clamp(1. / 127., 1.);
            } else if input.shift && !input.alt && horizontal != 0 {
                let maximum = (clip_length - note.quarter).max(MIN_LENGTH);
                note.length = (note.length + horizontal as f64 * self.step())
                    .clamp(self.minimum().min(maximum), maximum);
            } else if !input.shift && !input.alt {
                note.quarter = (note.quarter + horizontal as f64 * self.step())
                    .clamp(0., (clip_length - note.length).max(0.));
                note.pitch = (i32::from(note.pitch) + vertical).clamp(0, 127) as u8;
            }
            let geometry = note.quarter != self.draft[index].quarter
                || note.pitch != self.draft[index].pitch
                || note.length != self.draft[index].length;
            if !geometry || !overlaps(&self.draft, &note, Some(&note.id)) {
                self.draft[index] = note;
            }
            self.cursor_quarter = self.draft[index].quarter;
            self.cursor_pitch = self.draft[index].pitch;
            if input.delete && !previous.delete && !input.alt && !input.shift {
                self.draft.remove(index);
                self.selected = self
                    .draft
                    .get(index.min(self.draft.len().saturating_sub(1)))
                    .map(|n| n.id.clone());
            }
        } else if !input.alt && !input.shift {
            self.cursor_quarter =
                (self.cursor_quarter + horizontal as f64 * self.step()).clamp(0., clip_length);
            self.cursor_pitch = (i32::from(self.cursor_pitch) + vertical).clamp(0, 127) as u8;
            if input.home && !previous.home {
                self.cursor_quarter = 0.;
            }
        }
        if input.enter_pressed_since(&previous) && input.enter_note_allowed() && self.can_add() {
            let start = self
                .draft
                .iter()
                .find(|n| Some(&n.id) == self.selected.as_ref())
                .map_or(self.cursor_quarter, |n| n.quarter + n.length);
            let length = self
                .step()
                .max(self.minimum())
                .min((clip_length - start).max(0.));
            let id = self.next_id()?;
            let note = Note {
                bend: 0.,
                id: id.clone(),
                quarter: start,
                length,
                pitch: self.cursor_pitch,
                velocity: self.last_velocity.clamp(1. / 127., 1.),
            };
            if let Some(notes) = place(
                &self.draft,
                note,
                clip_length,
                self.snap,
                self.minimum(),
                false,
            ) {
                self.draft = notes;
                self.selected = Some(id);
                self.cursor_quarter = start;
            }
        }
        if (input.tab && !previous.tab)
            || (input.home && !previous.home)
            || input.enter_pressed_since(&previous)
            || (input.delete && !previous.delete)
            || horizontal != 0
            || vertical != 0
        {
            self.reveal_cursor(rect);
        }
        self.refresh_ages();
        self.proposal(before, HistoryAction::New)
    }
    fn reveal_cursor(&mut self, rect: Rect) {
        let margin = (16. / rect.width).min(0.2) * self.view.quarters_per_width;
        if self.cursor_quarter < self.view.first_quarter + margin {
            self.view.first_quarter = self.cursor_quarter - margin;
        }
        if self.cursor_quarter > self.view.first_quarter + self.view.quarters_per_width - margin {
            self.view.first_quarter = self.cursor_quarter - self.view.quarters_per_width + margin;
        }
        if (self.cursor_pitch as f64) < self.view.lowest_pitch + 1. {
            self.view.lowest_pitch = self.cursor_pitch as f64 - 1.;
        }
        if self.cursor_pitch as f64 > self.view.lowest_pitch + self.view.pitches_per_height - 2. {
            self.view.lowest_pitch = self.cursor_pitch as f64 - self.view.pitches_per_height + 2.;
        }
        self.view.constrain(self.clip.as_ref().unwrap().length);
    }
}

fn snap_floor(value: f64, step: Option<f64>) -> f64 {
    step.map_or(value, |s| ((value + EPSILON) / s).floor() * s)
}
fn canonicalize(notes: &mut [Note]) {
    notes.sort_by(|a, b| a.quarter.total_cmp(&b.quarter).then(a.id.cmp(&b.id)));
}
fn snap_nearest(value: f64, step: Option<f64>) -> f64 {
    step.map_or(value, |s| (value / s).round() * s)
}
fn next_start(notes: &[Note], pitch: u8, start: f64, exclude: Option<&str>) -> Option<f64> {
    notes
        .iter()
        .filter(|n| {
            Some(n.id.as_str()) != exclude && n.pitch == pitch && n.quarter > start + EPSILON
        })
        .map(|n| n.quarter)
        .min_by(f64::total_cmp)
}
fn overlaps(notes: &[Note], note: &Note, exclude: Option<&str>) -> bool {
    notes.iter().any(|other| {
        Some(other.id.as_str()) != exclude
            && other.pitch == note.pitch
            && note.quarter < other.quarter + other.length - EPSILON
            && other.quarter < note.quarter + note.length - EPSILON
    })
}
/// Collision changes are transactional. Failed creation cannot shorten a prior
/// note; negative or too-short lengths never reach the document. Unlike the old
/// floor/end loop, a same-onset collision must advance on every iteration.
fn place(
    original: &[Note],
    mut note: Note,
    clip_length: f64,
    snap: Option<f64>,
    minimum: f64,
    allow_shift: bool,
) -> Option<Vec<Note>> {
    if note.length < minimum || note.quarter < 0. || note.quarter >= clip_length {
        return None;
    }
    let mut notes = original.to_vec();
    if !allow_shift {
        if note.quarter + note.length > clip_length + EPSILON || overlaps(&notes, &note, None) {
            return None;
        }
        notes.push(note);
        return Some(notes);
    }
    for _ in 0..=original.len() {
        let Some(same) = notes
            .iter()
            .find(|n| n.pitch == note.pitch && (n.quarter - note.quarter).abs() < EPSILON)
        else {
            break;
        };
        let end = same.quarter + same.length;
        let snapped = snap_floor(end, snap);
        note.quarter = if snapped > note.quarter + EPSILON {
            snapped
        } else {
            snap.map_or(end, |s| (end / s).ceil() * s)
        };
    }
    // Re-evaluate predecessors when enforcing their minimum moves this onset.
    for _ in 0..=original.len() {
        let prior = notes.iter().position(|n| {
            n.pitch == note.pitch
                && n.quarter < note.quarter - EPSILON
                && n.quarter + n.length > note.quarter + EPSILON
        });
        let Some(index) = prior else {
            break;
        };
        let length = note.quarter - notes[index].quarter;
        if length < minimum {
            note.quarter = notes[index].quarter + minimum;
        }
        notes[index].length = (note.quarter - notes[index].quarter).max(minimum);
    }
    note.length = note.length.min((clip_length - note.quarter).max(0.));
    if let Some(next) = next_start(&notes, note.pitch, note.quarter, None) {
        note.length = note.length.min(next - note.quarter);
    }
    if note.length < minimum
        || note.quarter < 0.
        || note.quarter + note.length > clip_length + EPSILON
        || overlaps(&notes, &note, None)
    {
        return None;
    }
    notes.push(note);
    Some(notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bend_gesture_commits_once_curved_picking_reset_and_undo() {
        let mut e=editor(json!([note("a",1.,2.,60)]));
        let p=point(&e,2.,60);
        let press=Input{pointer:Some(p),left:true,alt:true,..Default::default()};
        assert!(e.update(&press,rect(),0.01).unwrap().is_none());
        let target=point(&e,2.,61);
        assert!(e.update(&Input{pointer:Some(target),..press.clone()},rect(),0.01).unwrap().is_none());
        assert!(e.notes()[0].bend>0.);
        let proposal=e.update(&Input{pointer:Some(target),alt:true,..Default::default()},rect(),0.01).unwrap().unwrap();
        assert_eq!(proposal["notes"][0]["pitch"],60);
        assert_eq!(proposal["notes"][0]["quarter"],1.);
        assert_eq!(proposal["notes"][0]["length_quarters"],2.);
        ack(&mut e,&proposal);
        let n=&e.draft[0];let mut curved=point(&e,2.9,60);
        let y=point(&e,2.9,61)[1];curved[1]+=(y-curved[1])*super::super::note_bend::value(1.,0.95);
        assert_eq!(e.hit(curved,rect(),false),Some(0));assert!(n.bend>0.9);
        let undo=e.update(&Input{undo:true,..Default::default()},rect(),0.01).unwrap().unwrap();
        assert!(undo["notes"][0]["bend_semitones"].is_null());ack(&mut e,&undo);
        let redo=e.update(&Input{redo:true,..Default::default()},rect(),0.01).unwrap().unwrap();ack(&mut e,&redo);
        e.update(&Input{pointer:Some(curved),left:true,alt:true,shift:true,..Default::default()},rect(),0.01).unwrap();
        let reset=e.update(&Input{pointer:Some(curved),alt:true,shift:true,..Default::default()},rect(),0.01).unwrap().unwrap();
        assert!(reset["notes"][0]["bend_semitones"].is_null());
    }

    fn rect() -> Rect {
        Rect {
            left: 0.,
            top: 0.,
            width: 800.,
            height: 600.,
        }
    }
    fn snapshot(revision: u64, mut notes: Value) -> Value {
        // Mirror Document::parse_clip instead of echoing an arbitrary proposal
        // order; acknowledgements come from the document's canonical snapshot.
        notes.as_array_mut().unwrap().sort_by(|a, b| {
            a["quarter"]
                .as_f64()
                .unwrap()
                .total_cmp(&b["quarter"].as_f64().unwrap())
                .then(a["id"].as_str().unwrap().cmp(b["id"].as_str().unwrap()))
        });
        json!({"revision":revision,"clips":[{"id":"roll","track":"midi","kind":"midi","start_seconds":10.,"bpm":90.,"length_quarters":8.,"meter":{"numerator":3,"denominator":4},"notes":notes}]})
    }
    fn note(id: &str, q: f64, len: f64, pitch: u8) -> Value {
        json!({"id":id,"quarter":q,"length_quarters":len,"pitch":pitch,"velocity":0.75})
    }
    fn editor(notes: Value) -> Editor {
        let mut e = Editor::default();
        e.open(&snapshot(3, notes), "roll").unwrap();
        e
    }
    fn point(e: &Editor, q: f64, p: u8) -> [f64; 2] {
        e.viewport().project(q, p as f64, rect())
    }
    fn ack(e: &mut Editor, request: &Value) {
        e.sync(&snapshot(
            request["document_revision"].as_u64().unwrap() + 1,
            request["notes"].clone(),
        ))
        .unwrap();
    }

    #[test]
    fn draw_is_preview_until_release_and_preserves_local_clock() {
        let mut e = editor(json!([]));
        e.set_snap_quarters(Some(1.)).unwrap();
        let p = point(&e, 1.7, 60);
        assert!(
            e.update(
                &Input {
                    pointer: Some(p),
                    left: true,
                    ..Input::default()
                },
                rect(),
                0.01
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(e.notes()[0].quarter, 1.);
        assert_eq!(e.notes()[0].length_quarters, 0.25);
        let request = e
            .update(
                &Input {
                    pointer: Some(p),
                    ..Input::default()
                },
                rect(),
                0.01,
            )
            .unwrap()
            .unwrap();
        assert_eq!(request["document_revision"], 3);
        assert_eq!(request["bpm"], 90.);
        assert_eq!(request["start_seconds"], 10.);
        assert_eq!(request["meter"]["numerator"], 3);
        ack(&mut e, &request);
        assert_eq!(e.status()["undo_available"], true);
    }
    #[test]
    fn free_placement_pitch_rows_and_undo_use_stable_identity() {
        let mut e = editor(json!([]));
        e.set_snap_quarters(None).unwrap();
        let p = point(&e, 1.37, 63);
        e.update(
            &Input {
                pointer: Some(p),
                left: true,
                ..Input::default()
            },
            rect(),
            0.,
        )
        .unwrap();
        let request = e
            .update(
                &Input {
                    pointer: Some(p),
                    ..Input::default()
                },
                rect(),
                0.,
            )
            .unwrap()
            .unwrap();
        assert!((request["notes"][0]["quarter"].as_f64().unwrap() - 1.37).abs() < 1e-8);
        let id = request["notes"][0]["id"].clone();
        ack(&mut e, &request);
        let undo = e
            .update(
                &Input {
                    undo: true,
                    ..Input::default()
                },
                rect(),
                0.,
            )
            .unwrap()
            .unwrap();
        ack(&mut e, &undo);
        assert!(e.notes().is_empty());
        let redo = e
            .update(
                &Input {
                    redo: true,
                    ..Input::default()
                },
                rect(),
                0.,
            )
            .unwrap()
            .unwrap();
        assert_eq!(redo["notes"][0]["id"], id);
        ack(&mut e, &redo);
    }
    #[test]
    fn changed_clip_cancels_drag_without_overwriting_other_editor() {
        let mut e = editor(json!([note("a", 1., 2., 60)]));
        let p = point(&e, 1.3, 60);
        e.update(
            &Input {
                pointer: Some(p),
                left: true,
                ..Input::default()
            },
            rect(),
            0.,
        )
        .unwrap();
        e.sync(&snapshot(4, json!([note("a", 4., 1., 62)])))
            .unwrap();
        assert!(
            e.update(
                &Input {
                    pointer: Some(point(&e, 2., 60)),
                    ..Input::default()
                },
                rect(),
                0.
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(e.notes()[0].quarter, 4.);
        assert_eq!(e.notes()[0].pitch, 62);
    }
    #[test]
    fn rejected_proposal_restores_authoritative_notes() {
        let mut e = editor(json!([]));
        let p = point(&e, 1., 60);
        e.update(
            &Input {
                pointer: Some(p),
                left: true,
                ..Input::default()
            },
            rect(),
            0.,
        )
        .unwrap();
        assert!(
            e.update(
                &Input {
                    pointer: Some(p),
                    ..Input::default()
                },
                rect(),
                0.
            )
            .unwrap()
            .is_some()
        );
        e.sync(&snapshot(3, json!([]))).unwrap();
        assert!(e.notes().is_empty());
        assert_eq!(e.status()["undo_available"], false);
    }
    #[test]
    fn resize_clamps_before_next_onset_and_never_negative() {
        let mut e = editor(json!([note("a", 1., 1., 60), note("b", 3., 1., 60)]));
        let mut handle = point(&e, 2., 60);
        handle[0] -= 2.;
        e.update(
            &Input {
                pointer: Some(handle),
                left: true,
                ..Input::default()
            },
            rect(),
            0.,
        )
        .unwrap();
        e.update(
            &Input {
                pointer: Some(point(&e, -2., 60)),
                left: true,
                ..Input::default()
            },
            rect(),
            0.,
        )
        .unwrap();
        assert!(e.notes()[0].length_quarters > 0.);
        let request = e
            .update(
                &Input {
                    pointer: Some(point(&e, 6., 60)),
                    ..Input::default()
                },
                rect(),
                0.,
            )
            .unwrap()
            .unwrap();
        assert_eq!(request["notes"][0]["length_quarters"], 2.);
    }
    #[test]
    fn keyboard_rejects_same_pitch_overlap_but_moves_pitch() {
        let mut e = editor(json!([note("a", 1., 1., 60), note("b", 2., 1., 60)]));
        e.set_snap_quarters(Some(1.)).unwrap();
        e.update(
            &Input {
                tab: true,
                ..Input::default()
            },
            rect(),
            0.,
        )
        .unwrap();
        assert!(
            e.update(
                &Input {
                    arrows: [false, true, false, false],
                    ..Input::default()
                },
                rect(),
                0.
            )
            .unwrap()
            .is_none()
        );
        let request = e
            .update(
                &Input {
                    arrows: [false, false, true, false],
                    ..Input::default()
                },
                rect(),
                0.,
            )
            .unwrap()
            .unwrap();
        assert_eq!(request["notes"][0]["pitch"], 61);
    }
    #[test]
    fn failed_collision_placement_does_not_mutate_predecessor() {
        let source = vec![
            Note {
                bend: 0.,
                id: "a".into(),
                quarter: 0.,
                length: 1.,
                pitch: 60,
                velocity: 1.,
            },
            Note {
                bend: 0.,
                id: "b".into(),
                quarter: 0.6,
                length: 1.,
                pitch: 60,
                velocity: 1.,
            },
        ];
        let n = Note {
            bend: 0.,
            id: "new".into(),
            quarter: 0.5,
            length: 0.25,
            pitch: 60,
            velocity: 1.,
        };
        assert!(place(&source, n, 4., Some(1.), 0.25, true).is_none());
        assert_eq!(source[0].length, 1.);
    }
    #[test]
    fn equal_onset_with_short_existing_note_always_advances() {
        let source = vec![Note {
            bend: 0.,
            id: "a".into(),
            quarter: 0.,
            length: 0.25,
            pitch: 60,
            velocity: 1.,
        }];
        let n = Note {
            bend: 0.,
            id: "new".into(),
            quarter: 0.,
            length: 0.25,
            pitch: 60,
            velocity: 1.,
        };
        let result = place(&source, n, 4., Some(1.), 0.25, true).unwrap();
        assert_eq!(result[1].quarter, 1.);
    }
    #[test]
    fn viewport_mapping_round_trips_without_visible_grid() {
        let e = editor(json!([]));
        let view = e.viewport();
        let p = view.project(2.375, 67., rect());
        let (q, row) = view.locate(p, rect());
        assert!((q - 2.375).abs() < 1e-9);
        assert!((row - 67.5).abs() < 1e-9);
        assert_eq!(e.status()["grid_visible"], false);
    }
    #[test]
    fn canonical_move_crossing_an_onset_is_acknowledged_and_undoable() {
        let mut e = editor(json!([note("a", 2., 1., 60), note("b", 4., 1., 60)]));
        let from = point(&e, 4.3, 60);
        let to = point(&e, 1.3, 60);
        e.update(
            &Input {
                pointer: Some(from),
                left: true,
                ..Input::default()
            },
            rect(),
            0.,
        )
        .unwrap();
        let request = e
            .update(
                &Input {
                    pointer: Some(to),
                    ..Input::default()
                },
                rect(),
                0.,
            )
            .unwrap()
            .unwrap();
        assert_eq!(request["notes"][0]["id"], "b");
        ack(&mut e, &request);
        assert_eq!(e.status()["undo_available"], true);
        assert!(e.status()["notice"].is_null());
        let undo = e
            .update(
                &Input {
                    undo: true,
                    ..Input::default()
                },
                rect(),
                0.,
            )
            .unwrap()
            .unwrap();
        assert_eq!(undo["notes"][0]["id"], "a");
        assert_eq!(undo["notes"][1]["quarter"], 4.);
        ack(&mut e, &undo);
        assert_eq!(e.status()["redo_available"], true);
    }
    #[test]
    fn unchanged_sync_preserves_preview_and_settled_ages_stop_changing() {
        let mut e = editor(json!([]));
        assert!(e.status()["snap_quarters"].is_null());
        let p = point(&e, 1.37, 60);
        e.update(
            &Input {
                pointer: Some(p),
                left: true,
                ..Input::default()
            },
            rect(),
            0.,
        )
        .unwrap();
        e.sync(&snapshot(3, json!([]))).unwrap();
        assert_eq!(e.notes().len(), 1);
        let proposal = e
            .update(
                &Input {
                    pointer: Some(p),
                    ..Input::default()
                },
                rect(),
                0.,
            )
            .unwrap()
            .unwrap();
        ack(&mut e, &proposal);
        for _ in 0..4 {
            e.update(&Input::default(), rect(), 1.).unwrap();
        }
        assert_eq!(e.notes()[0].placement_age, 1.35);
        e.update(&Input::default(), rect(), 1.).unwrap();
        assert_eq!(e.notes()[0].placement_age, 1.35);
    }
    #[test]
    fn five_pixel_note_has_distinct_move_body_and_resize_handle() {
        let mut source = snapshot(3, json!([note("short", 4., 0.25, 60)]));
        source["clips"][0]["length_quarters"] = json!(16.);
        let field = Rect {
            width: 320.,
            ..rect()
        };
        let mut moving = Editor::default();
        moving.open(&source, "roll").unwrap();
        // 0.25 quarters / 16 quarters * 320 pixels = five pixels. Its
        // center belongs to the move body, even though it is <8px from the end.
        let body = moving.viewport().project(4.125, 60., field);
        moving
            .update(
                &Input {
                    pointer: Some(body),
                    left: true,
                    ..Input::default()
                },
                field,
                0.,
            )
            .unwrap();
        let destination = moving.viewport().project(6.125, 62., field);
        let moved = moving
            .update(
                &Input {
                    pointer: Some(destination),
                    ..Input::default()
                },
                field,
                0.,
            )
            .unwrap()
            .unwrap();
        assert_eq!(moved["notes"][0]["id"], "short");
        assert_eq!(moved["notes"][0]["quarter"], 6.);
        assert_eq!(moved["notes"][0]["pitch"], 62);
        assert_eq!(moved["notes"][0]["length_quarters"], 0.25);

        let mut resizing = Editor::default();
        resizing.open(&source, "roll").unwrap();
        // The rightmost 35% still resizes. The 90%-width point is well
        // inside that handle and inside the actual rendered duration.
        let handle = resizing.viewport().project(4.225, 60., field);
        resizing
            .update(
                &Input {
                    pointer: Some(handle),
                    left: true,
                    ..Input::default()
                },
                field,
                0.,
            )
            .unwrap();
        let new_end = resizing.viewport().project(5., 60., field);
        let resized = resizing
            .update(
                &Input {
                    pointer: Some(new_end),
                    ..Input::default()
                },
                field,
                0.,
            )
            .unwrap()
            .unwrap();
        assert_eq!(resized["notes"][0]["id"], "short");
        assert_eq!(resized["notes"][0]["quarter"], 4.);
        assert_eq!(resized["notes"][0]["pitch"], 60);
        assert_eq!(resized["notes"][0]["length_quarters"], 1.);
    }
    #[test]
    fn primed_activation_stays_consumed_until_release_and_new_press() {
        let mut e = editor(json!([]));
        let p = point(&e, 2.4, 64);
        let held = Input {
            pointer: Some(p),
            left: true,
            enter: true,
            ..Input::default()
        };
        e.prime_input(&held);
        assert!(e.notes().is_empty());
        assert!(e.update(&held, rect(), 0.).unwrap().is_none());
        assert!(e.notes().is_empty());
        assert_eq!(e.status()["gesture_active"], false);
        // Releasing only the mouse must not reinterpret the still-held Enter
        // as a fresh keyboard insert once pointer handling is no longer active.
        assert!(
            e.update(
                &Input {
                    pointer: Some(p),
                    enter: true,
                    ..Input::default()
                },
                rect(),
                0.
            )
            .unwrap()
            .is_none()
        );
        assert!(e.notes().is_empty());
        e.update(&Input::default(), rect(), 0.).unwrap();
        let inserted = e
            .update(
                &Input {
                    enter: true,
                    ..Input::default()
                },
                rect(),
                0.,
            )
            .unwrap()
            .unwrap();
        assert_eq!(inserted["notes"].as_array().unwrap().len(), 1);
        ack(&mut e, &inserted);
        e.update(&Input::default(), rect(), 0.).unwrap();
        assert!(
            e.update(
                &Input {
                    pointer: Some(p),
                    left: true,
                    ..Input::default()
                },
                rect(),
                0.
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(e.status()["gesture_active"], true);
        assert_eq!(e.notes().len(), 2);
        let released = e
            .update(
                &Input {
                    pointer: Some(p),
                    ..Input::default()
                },
                rect(),
                0.,
            )
            .unwrap()
            .unwrap();
        assert_eq!(released["notes"].as_array().unwrap().len(), 2);
    }
    #[test]
    fn empty_open_shows_a_useful_octave_around_middle_c() {
        let e = editor(json!([]));
        let view = e.viewport();
        assert_eq!(view.pitches_per_height, 12.);
        assert_eq!(view.lowest_pitch, 54.);
        let middle_c = view.project(0., 60., rect());
        assert!(middle_c[1] > rect().height * 0.4 && middle_c[1] < rect().height * 0.6);
        assert!(e.notes().is_empty());
        assert_eq!(e.status()["pending_commit"], false);
    }
    #[test]
    fn opening_fits_existing_pitch_extremes_within_midi_bounds_without_editing() {
        for (low, high) in [(0, 80), (10, 85), (47, 127), (60, 60)] {
            let source = snapshot(
                3,
                json!([note("low", 1., 0.25, low), note("high", 3., 1., high)]),
            );
            let mut e = Editor::default();
            e.open(&source, "roll").unwrap();
            let view = e.viewport();
            assert!((12. ..=84.).contains(&view.pitches_per_height));
            assert!(view.lowest_pitch >= 0.);
            assert!(view.lowest_pitch + view.pitches_per_height <= 128.);
            assert!(view.lowest_pitch <= f64::from(low));
            assert!(view.lowest_pitch + view.pitches_per_height >= f64::from(high) + 1.);
            if (low, high) == (10, 85) {
                assert_eq!(view.lowest_pitch, 8.);
                assert_eq!(view.pitches_per_height, 80.);
            }
            let unchanged = e.clip.as_ref().unwrap().proposal(&e.draft, e.revision);
            assert_eq!(unchanged["notes"], source["clips"][0]["notes"]);
            assert_eq!(unchanged["bpm"], source["clips"][0]["bpm"]);
            assert_eq!(
                unchanged["start_seconds"],
                source["clips"][0]["start_seconds"]
            );
            assert_eq!(e.status()["pending_commit"], false);
        }
    }
    #[test]
    fn sync_does_not_refit_a_manually_panned_and_zoomed_pitch_view() {
        let notes = json!([note("a", 1., 1., 60), note("b", 3., 1., 67)]);
        let mut e = editor(notes.clone());
        let pointer = Some([rect().width * 0.5, rect().height * 0.5]);
        e.update(
            &Input {
                pointer,
                alt: true,
                wheel: [0., 1.],
                ..Input::default()
            },
            rect(),
            0.,
        )
        .unwrap();
        e.update(
            &Input {
                pointer,
                wheel: [0., 3.],
                ..Input::default()
            },
            rect(),
            0.,
        )
        .unwrap();
        let manually_adjusted = e.viewport();
        e.sync(&snapshot(4, notes)).unwrap();
        assert_eq!(e.viewport().lowest_pitch, manually_adjusted.lowest_pitch);
        assert_eq!(
            e.viewport().pitches_per_height,
            manually_adjusted.pitches_per_height
        );
        // Even an external note edit refreshes data without resetting the view.
        e.sync(&snapshot(
            5,
            json!([note("a", 1., 1., 25), note("b", 3., 1., 100)]),
        ))
        .unwrap();
        assert_eq!(e.viewport().lowest_pitch, manually_adjusted.lowest_pitch);
        assert_eq!(
            e.viewport().pitches_per_height,
            manually_adjusted.pitches_per_height
        );
    }
    #[test]
    fn capped_sparse_extremes_open_on_an_actual_note_instead_of_empty_space() {
        let e = editor(json!([note("low", 1., 0.25, 0), note("high", 3., 1., 127)]));
        let view = e.viewport();
        assert_eq!(view.pitches_per_height, 84.);
        assert_eq!(view.lowest_pitch, 0.);
        let visible: Vec<_> = e
            .notes()
            .into_iter()
            .filter(|note| {
                rect().contains(view.project(note.quarter, f64::from(note.pitch), rect()))
            })
            .collect();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, "low");
        assert_eq!(e.notes().len(), 2);
        assert_eq!(e.cursor_pitch, 0);
        assert!(e.selected.is_none());
        assert_eq!(e.status()["pending_commit"], false);
    }
    #[test]
    fn initial_navigation_on_high_only_clip_keeps_its_fitted_pitch_view() {
        let mut e = editor(json!([note("high", 3., 1., 120)]));
        let fitted = e.viewport();
        assert_eq!(e.cursor_pitch, 120);
        assert_eq!(e.cursor_quarter, 0.);
        assert!(e.selected.is_none());
        for input in [
            Input {
                home: true,
                ..Input::default()
            },
            Input {
                arrows: [true, false, false, false],
                ..Input::default()
            },
            Input {
                delete: true,
                ..Input::default()
            },
        ] {
            assert!(e.update(&input, rect(), 0.).unwrap().is_none());
            assert_eq!(e.viewport().lowest_pitch, fitted.lowest_pitch);
            assert_eq!(e.viewport().pitches_per_height, fitted.pitches_per_height);
            assert!(rect().contains(e.viewport().project(3., 120., rect())));
            assert_eq!(e.notes()[0].quarter, 3.);
            assert_eq!(e.notes()[0].pitch, 120);
            assert_eq!(e.notes().len(), 1);
        }
        let proposal = e
            .update(
                &Input {
                    enter: true,
                    ..Input::default()
                },
                rect(),
                0.,
            )
            .unwrap()
            .unwrap();
        assert_eq!(proposal["notes"].as_array().unwrap().len(), 2);
        assert_eq!(proposal["notes"][0]["quarter"], 0.);
        assert_eq!(proposal["notes"][0]["pitch"], 120);
    }
    #[test]
    fn native_enter_uses_press_time_modifiers_and_never_replays_held_state() {
        let mut e = editor(json!([]));
        for modifiers in [1, 2, 4, 8] {
            let modified = Input {
                key_events: true,
                enter_pressed: true,
                enter_modifiers: modifiers,
                ..Input::default()
            };
            // Released Shift/Ctrl/Alt/Cmd stays attached to the Enter press.
            assert!(e.update(&modified, rect(), 0.).unwrap().is_none());
        }
        assert!(e.notes().is_empty());
        let plain = Input {
            key_events: true,
            enter_pressed: true,
            enter: true,
            alt: true,
            command: true,
            delete: true,
            ..Input::default()
        };
        // Alt/Cmd were pressed later; neither changes the earlier plain Enter.
        // Their current Delete chord must not pass through the ordinary edits.
        let proposal = e.update(&plain, rect(), 0.).unwrap().unwrap();
        ack(&mut e, &proposal);
        assert_eq!(e.notes().len(), 1);
        let held = Input {
            key_events: true,
            enter: true,
            ..Input::default()
        };
        assert!(e.update(&held, rect(), 0.).unwrap().is_none());
        assert_eq!(e.notes().len(), 1);
        let first_id = e.notes()[0].id.clone();
        let next = e.update(&plain, rect(), 0.).unwrap().unwrap();
        ack(&mut e, &next);
        assert_eq!(e.notes().len(), 2);
        assert!(e.notes().iter().any(|note| note.id == first_id));
    }
    #[test]
    fn audition_follows_active_note_gesture_and_releases_with_mouse() {
        let mut e=editor(json!([]));
        let point=point(&e,1.,60);
        let mut input=Input{pointer:Some(point),left:true,..Default::default()};
        e.update(&input,rect(),0.01).unwrap();
        let (track,pitch,velocity)=e.audition_note(&input,rect()).unwrap();
        assert_eq!((track.as_str(),pitch,velocity),("midi",60,1.));
        input.left=false;
        assert!(e.audition_note(&input,rect()).is_none());
    }

}

// Scale extensions stack thirds; explicit seventh qualities intentionally override scale.
fn arp_pitches(pitch:u8, scale:u8, root:u8, edo:u16, extension:usize, count:usize, direction:usize)->Vec<u8> {
    let scale = if scale == 0 { 1 } else { scale };
    let base = super::piano_tuning::snap_key(pitch,scale,root,edo);
    let tones = match extension {0=>3,1..=3=>4,4=>5,5=>6,_=>7};
    let mut chord = Vec::new();
    if (1..=3).contains(&extension) {
        let intervals = match extension {1=>[0,4,7,11],2=>[0,4,7,10],_=>[0,3,7,10]};
        for interval in intervals { chord.push(i32::from(base)+(interval as f64*f64::from(edo)/12.).round() as i32); }
    } else {
        let degrees:Vec<i32>=(i32::from(base)..=255).filter(|k| edo!=12 || super::piano_tuning::in_scale((k%12) as u8,scale,root)).collect();
        for i in 0..tones { chord.push(degrees[i*2]); }
    }
    let ascending:Vec<i32>=(0..count).map(|i|chord[i%tones]+(i/tones) as i32*i32::from(edo)).collect();
    (0..count).filter_map(|i| {
        let index=match direction {1=>count-1-i,2=>{let peak=count/2;if i<=peak {i} else {2*peak-i}},_=>i};
        u8::try_from(ascending[index]).ok().filter(|p|*p<=127)
    }).collect()
}

#[cfg(test)]
mod arp_tests {
    use super::*;
    #[test] fn diatonic_triads_repeat_in_octaves_and_extensions_stack_thirds() {
        assert_eq!(arp_pitches(62,1,0,12,0,6,0),vec![62,65,69,74,77,81]);
        assert_eq!(arp_pitches(60,1,0,12,6,7,0),vec![60,64,67,71,74,77,81]);
        assert_eq!(arp_pitches(60,1,0,12,2,4,0),vec![60,64,67,70]);
        assert_eq!(arp_pitches(60,1,0,12,3,4,0),vec![60,63,67,70]);
        assert_eq!(arp_pitches(60,1,0,12,0,3,1),vec![67,64,60]);
        assert_eq!(arp_pitches(60,1,0,12,0,5,2),vec![60,64,67,64,60]);
    }
    #[test] fn stamps_are_atomic_and_chords_share_the_onset() {
        let doc=json!({"revision":1,"tracks":[{"id":"t","kind":"midi"}],"clips":[{"id":"c","track":"t","kind":"midi","start_seconds":0.,"length_quarters":8.,"bpm":120.,"meter":{"numerator":4,"denominator":4},"scale":1,"notes":[]}]});
        let mut e=Editor::default();e.open(&doc,"c").unwrap();e.set_snap_quarters(Some(0.5)).unwrap();
        e.create_arp(0.2,60).unwrap();
        assert_eq!(e.draft.iter().map(|n|n.quarter).collect::<Vec<_>>(),vec![0.,0.5,1.]);
        let before=e.draft.clone();e.create_arp(7.8,60).unwrap();assert!(e.draft == before);
        e.draft.clear();e.arp_chord=true;e.create_arp(0.,60).unwrap();
        assert_eq!(e.draft.len(),3);assert!(e.draft.iter().all(|n|n.quarter==0.));
    }
}
