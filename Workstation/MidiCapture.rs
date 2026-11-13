//! Timestamped 12-EDO input adapter, live voices and original MIDI take capture.
//! Source: MidiTransportSystem recording/loop segmentation and AudioSystem held
//! keyboard-note merging. A real keyboard/CoreMIDI sender must supply events.
use super::document::{Clip, Content, Note, State as Document, Track};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone)]
pub(super) struct Voice {
    pub track: String,
    pub pitch: u8,
    pub tuning_cents:f64,
    pub velocity: f64,
    pub on: u64,
    pub off: Option<u64>,
    pub preview: bool,
}
#[derive(Clone)]
struct Captured {
    on: u64,
    off: u64,
    pitch: u8,
    tuning_cents:f64,
    velocity: f64,
}
#[derive(Clone)]
struct Session {
    start: u64,
    frames: u64,
    range: Option<[u64; 2]>,
    held: BTreeMap<u8, (u64, f64, f64)>,
    notes: Vec<Captured>,
    enabled: bool,
}
#[derive(Clone, Default)]
struct Takes {
    clips: Vec<Clip>,
    active: usize,
}
#[derive(Clone, Default)]
pub(super) struct State {
    voices: Vec<Voice>,
    sessions: BTreeMap<String, Session>,
    takes: BTreeMap<String, Takes>,
    recording: bool,
    block: Option<(u64, usize, u32)>,
}
impl State {
    pub fn enabled(&self) -> Vec<String> {
        self.sessions
            .iter()
            .filter(|(_, s)| s.enabled)
            .map(|(id, _)| id.clone())
            .collect()
    }
    pub fn pending(&self) -> bool {
        !self.sessions.is_empty()
    }
    pub fn status(&self) -> Value {
        json!({"record_enabled_tracks":self.enabled(),"takes":self.takes.iter().map(|(track,t)|json!({"track":track,"active":t.active,"clips":t.clips.iter().map(Clip::snapshot).collect::<Vec<_>>()})).collect::<Vec<_>>(),"live_voices":self.voices.len(),"tuning_supported":true,"event_sender_connected":false})
    }
    pub fn disarm(&mut self, id: &str) {
        if let Some(s) = self.sessions.get_mut(id) {
            finish(s, s.start + s.frames);
            s.enabled = false;
        }
    }
    pub fn clear(&mut self, id: &str) {
        self.sessions.remove(id);
        self.takes.remove(id);
        self.voices.retain(|v| v.track != id);
    }
    /// Only an explicitly matching capture block may supply live notes to a render.
    /// Offline rendering/export never sees the user's held keyboard state.
    pub fn voices(&self, capture_start: u64, frames: usize, rate: u32) -> Result<&[Voice], String> {
        if self.block != Some((capture_start, frames, rate)) {
            return Err("live MIDI render must match the accepted CaptureInput block".into());
        }
        Ok(&self.voices)
    }
    pub fn advance(
        &mut self,
        tracks: &BTreeMap<String, Track>,
        input: &Value,
        rate: u32,
    ) -> Result<(), String> {
        let recording = input["recording"] == true;
        let playing = input["playing"] == true;
        let frames = input["frames"].as_u64().unwrap();
        let at = input["capture_start_frame"].as_u64().unwrap();
        let timeline = input["timeline_start_frame"].as_u64().unwrap();
        if recording && !self.recording {
            if !self.sessions.is_empty() {
                return Err("stop and commit pending MIDI recording before another take".into());
            }
            let range = input["loop_start_frame"]
                .as_u64()
                .zip(input["loop_end_frame"].as_u64())
                .map(|(a, b)| [a, b]);
            for track in tracks
                .values()
                .filter(|t| t.kind == "midi" && t.io.records())
            {
                self.sessions.insert(
                    track.id.clone(),
                    Session {
                        start: timeline,
                        frames: 0,
                        range,
                        held: BTreeMap::new(),
                        notes: vec![],
                        enabled: true,
                    },
                );
            }
        }
        for (id, s) in &mut self.sessions {
            if !recording || !tracks.get(id).is_some_and(|t| t.io.records()) {
                finish(s, s.start + s.frames);
                s.enabled = false;
            }
        }
        self.voices.retain(|v| {
            v.off
                .is_none_or(|off| off.saturating_add((rate as f64 * 0.12).ceil() as u64) > at)
        });
        for e in input["midi_events"].as_array().into_iter().flatten() {
            let source = e["source"].as_str().unwrap_or("keyboard");
            if !matches!(source, "keyboard" | "preview") {
                return Err("MIDI source must be keyboard or preview".into());
            }
            let id = e["track"].as_str().unwrap();
            let pitch = e["pitch"].as_u64().unwrap() as u8;
            let tuning_cents=super::piano_tuning::cents(&e["tuning_cents"],pitch)?;
            let velocity = e["velocity"].as_f64().unwrap();
            let when = e["capture_frame"].as_u64().unwrap();
            let preview = source == "preview";
            for v in self.voices.iter_mut().filter(|v| {
                v.track == id && v.pitch == pitch && v.preview == preview && v.off.is_none()
            }) {
                v.off = Some(when);
            }
            if velocity > 0. {
                self.voices.push(Voice {
                    track: id.into(),
                    pitch, tuning_cents,
                    velocity: velocity.max(1. / 127.),
                    on: when,
                    off: None,
                    preview,
                });
            }
            if let Some(s) = self.sessions.get_mut(id).filter(|s| s.enabled) {
                let linear = s.start + s.frames + (when - at);
                if let Some((on, vel, cents)) = s.held.remove(&pitch) {
                    if linear > on {
                        s.notes.push(Captured {
                            on,
                            off: linear,
                            pitch,
                            velocity: vel, tuning_cents:cents,
                        });
                    }
                }
                if velocity > 0. {
                    s.held.insert(pitch, (linear, velocity.max(1. / 127.),tuning_cents));
                }
            }
        }
        if self.voices.len() > 8192 {
            return Err("live MIDI voice history capacity reached".into());
        }
        for s in self.sessions.values_mut() {
            if s.enabled {
                s.frames = s
                    .frames
                    .checked_add(frames)
                    .ok_or("MIDI capture clock overflow")?;
            }
            if s.notes.len() + s.held.len() > 8192 {
                return Err("MIDI pending-note capacity reached".into());
            }
        }
        if frames==0 && !playing {
            for voice in self.voices.iter_mut().filter(|v|!v.preview && v.off.is_none()){voice.off=Some(at);}
        }
        if !playing {
            for s in self.sessions.values_mut() {
                finish(s, s.start + s.frames);
                s.enabled = false;
            }
        }
        self.recording = recording;
        self.block = (frames > 0).then_some((at, frames as usize, rate));
        Ok(())
    }
    pub fn commit(
        &mut self,
        clips: &mut BTreeMap<String, Clip>,
        order: &mut Vec<String>,
        rate: u32,
    ) -> Result<Vec<String>, String> {
        let mut created = vec![];
        let sessions = std::mem::take(&mut self.sessions);
        let mut used = clips.keys().cloned().collect::<BTreeSet<_>>();
        for (track, mut s) in sessions {
            let end = s.start + s.frames;
            finish(&mut s, end);
            if s.frames == 0 {
                continue;
            }
            let mut cursor = 0;
            let mut loop_takes = vec![];
            let mut regular = vec![];
            while cursor < s.frames {
                let raw = s.start + cursor;
                let (logical, count, is_loop) = match s.range {
                    Some([a, b]) if raw < a => (raw, (a - raw).min(s.frames - cursor), false),
                    Some([a, b]) => {
                        let p = (raw - a) % (b - a);
                        (a + p, (b - a - p).min(s.frames - cursor), true)
                    }
                    None => (raw, s.frames - cursor, false),
                };
                let id = identity("midi-record", &used);
                used.insert(id.clone());
                // An elapsed sample span is represented by this new clip's own120BPM
                // clock, matching the existing free-time empty-clip adapter, never project BPM.
                let notes = s
                    .notes
                    .iter()
                    .filter_map(|n| {
                        let on = n.on.max(raw);
                        let off = n.off.min(raw + count);
                        (off > on).then(|| Note { bend:0.,
                            id: format!("record-note-{}", n.on),
                            quarter: (on - raw) as f64 / rate as f64 * 2.,
                            length: (off - on) as f64 / rate as f64 * 2.,
                            pitch: n.pitch, tuning_cents:n.tuning_cents,
                            velocity: n.velocity,
                        })
                    })
                    .enumerate()
                    .map(|(i, mut n)| {
                        n.id = format!("record-note-{i}");
                        n
                    })
                    .collect::<Vec<_>>();
                if notes.len() > 2048 {
                    return Err("recorded MIDI segment exceeds piano-roll note capacity".into());
                }
                let clip = Clip { output:String::new(),lane:0.,gain:1., fade_in:0., fade_out:0.,
                    id,
                    track: track.clone(),
                    start: logical as f64 / rate as f64,
                    content: Content::Midi { edo:12,scale:0,root:0,
                        bpm: 120.,
                        quarters: count as f64 / rate as f64 * 2.,
                        numerator: 4,
                        denominator: 4,
                        notes,
                    },
                };
                if is_loop {
                    loop_takes.push(clip);
                } else {
                    regular.push(clip);
                }
                cursor += count;
            }
            for clip in regular {
                let takes = self.takes.entry(track.clone()).or_default();
                push_take(takes, clip.clone())?;
                insert(clips, order, clip.clone(), Some(takes))?;
                created.push(clip.id);
            }
            if !loop_takes.is_empty() {
                let takes = self.takes.entry(track.clone()).or_default();
                for clip in &loop_takes {
                    push_take(takes, clip.clone())?;
                }
                let active = loop_takes.last().unwrap().clone();
                insert(clips, order, active.clone(), Some(takes))?;
                created.push(active.id);
            }
        }
        validate_document(clips)?;
        let archive_clips = self.takes.values().map(|t| t.clips.len()).sum::<usize>();
        let archive_notes = self
            .takes
            .values()
            .flat_map(|t| &t.clips)
            .map(|c| match &c.content {
                Content::Midi { notes, .. } => notes.len(),
                _ => 0,
            })
            .sum::<usize>();
        if archive_clips > 512 || archive_notes > 8192 {
            return Err(
                "MIDI take archive exceeds document capacity; pending notes retained".into(),
            );
        }
        created.retain(|id| clips.contains_key(id));
        Ok(created)
    }
}
fn finish(s: &mut Session, end: u64) {
    for (pitch, (on, velocity, tuning_cents)) in std::mem::take(&mut s.held) {
        if end > on {
            s.notes.push(Captured {
                on,
                off: end,
                pitch, tuning_cents,
                velocity,
            });
        }
    }
}
fn identity(prefix: &str, used: &BTreeSet<String>) -> String {
    (1..=used.len() + 1)
        .map(|n| format!("{prefix}-{n}"))
        .find(|s| !used.contains(s))
        .unwrap()
}
fn push_take(t: &mut Takes, clip: Clip) -> Result<(), String> {
    if t.clips.len() >= 512 {
        return Err("MIDI take capacity reached; pending capture retained".into());
    }
    t.clips.push(clip);
    t.active = t.clips.len() - 1;
    Ok(())
}
fn portion(clip: &Clip, start: f64, end: f64, id: String) -> Clip {
    let Content::Midi {
        edo,scale,root,
        bpm,
        numerator,
        denominator,
        notes,
        ..
    } = &clip.content
    else {
        unreachable!()
    };
    let notes = notes
        .iter()
        .filter_map(|n| {
            let a = (clip.start + n.quarter * 60. / bpm).max(start);
            let b = (clip.start + (n.quarter + n.length) * 60. / bpm).min(end);
            (b > a).then(|| Note { bend:0.,
                id: n.id.clone(),
                quarter: (a - start) * bpm / 60.,
                length: (b - a) * bpm / 60.,
                pitch: n.pitch, tuning_cents:n.tuning_cents,
                velocity: n.velocity,
            })
        })
        .collect();
    Clip { output:String::new(),lane:0.,gain:1., fade_in:0., fade_out:0.,
        id,
        track: clip.track.clone(),
        start,
        content: Content::Midi { edo:*edo,scale:*scale,root:*root,
            bpm: *bpm,
            quarters: (end - start) * bpm / 60.,
            numerator: *numerator,
            denominator: *denominator,
            notes,
        },
    }
}
fn insert(
    clips: &mut BTreeMap<String, Clip>,
    order: &mut Vec<String>,
    clip: Clip,
    mut takes: Option<&mut Takes>,
) -> Result<(), String> {
    let a = clip.start;
    let b = a + clip.duration();
    let affected = order
        .iter()
        .filter(|id| {
            clips[*id].track == clip.track
                && clips[*id].start < b
                && clips[*id].start + clips[*id].duration() > a
        })
        .cloned()
        .collect::<Vec<_>>();
    for id in affected {
        let old = clips.remove(&id).unwrap();
        order.retain(|v| v != &id);
        let end = old.start + old.duration();
        if !matches!(old.content, Content::Midi { .. }) {
            return Err("MIDI recording found non-MIDI lane content".into());
        }
        if let Some(t) = takes.as_deref_mut() {
            push_take(
                t,
                portion(&old, a.max(old.start), b.min(end), old.id.clone()),
            )?;
        }
        if old.start < a {
            let left = portion(&old, old.start, a, old.id.clone());
            order.push(left.id.clone());
            clips.insert(left.id.clone(), left);
        }
        if end > b {
            let id = if old.start < a {
                identity("midi-split", &clips.keys().cloned().collect())
            } else {
                old.id.clone()
            };
            let right = portion(&old, b, end, id);
            order.push(right.id.clone());
            clips.insert(right.id.clone(), right);
        }
    }
    order.push(clip.id.clone());
    clips.insert(clip.id.clone(), clip);
    validate_document(clips)
}
fn validate_document(clips: &BTreeMap<String, Clip>) -> Result<(), String> {
    if clips.len() > 512
        || clips
            .values()
            .map(|c| match &c.content {
                Content::Midi { notes, .. } => notes.len(),
                _ => 0,
            })
            .sum::<usize>()
            > 8192
    {
        return Err("MIDI commit exceeds document clip/note capacity".into());
    }
    Ok(())
}
pub(super) fn cycle(document: &mut Document, input: &Value) -> Result<Value, String> {
    let id = input["id"].as_str().ok_or("MIDI take track id required")?;
    let direction = input["direction"]
        .as_i64()
        .filter(|d| *d == 1 || *d == -1)
        .ok_or("take direction must be -1 or1")?;
    if document.recording.midi.pending() {
        return Err("stop MIDI recording before cycling takes".into());
    }
    let mut state = document.recording.midi.clone();
    let t = state
        .takes
        .get_mut(id)
        .filter(|t| t.clips.len() >= 2)
        .ok_or("MIDI lane requires at least two takes")?;
    t.active = (t.active as i64 + direction).rem_euclid(t.clips.len() as i64) as usize;
    let mut selected = t.clips[t.active].clone();
    // Archive identity can also name an active unaffected slice; issue a new
    // document identity when needed, preserving the archived take itself.
    if document.clips.contains_key(&selected.id) {
        selected.id = identity("midi-take", &document.clips.keys().cloned().collect());
    }
    let mut clips = document.clips.clone();
    let mut order = document.clip_order.clone();
    insert(&mut clips, &mut order, selected.clone(), None)?;
    document.clips = clips;
    document.clip_order = order;
    document.recording.midi = state;
    document.recording_changed();
    Ok(json!({"revision":document.revision,"selected_clip":selected.id}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn document(arm: u8) -> Document {
        let mut d = Document::default();
        d.tracks.insert(
            "m".into(),
            Track { output:String::new(),
                id: "m".into(),
                name: "Midi".into(),
                kind: "midi".into(),
                gain: 1.,
                mute: false,
                solo: false,
                default_sound_muted: false,
                route: super::super::routing::Route::default(),
                io: super::super::track_io::TrackIo {
                    arm_mode: arm,
                    ..super::super::track_io::TrackIo::new("midi")
                },
                gain_target: None,
                cc_lane:super::super::track_io::CcLane::None,
                cc_device_slot:0,cc_parameter_slot:0,
            },
        );
        d
    }
    fn block(at: u64, timeline: u64, frames: usize, events: Value) -> Value {
        json!({"sample_rate":8000,"capture_start_frame":at,"timeline_start_frame":timeline,"frames":frames,"playing":true,"recording":true,"live_inputs":{"physical":[]},"midi_events":events})
    }
    fn event(at: u64, pitch: u8, velocity: f64) -> Value {
        json!({"capture_frame":at,"track":"m","pitch":pitch,"velocity":velocity})
    }
    fn send(d: &mut Document, v: Value) -> Value {
        super::super::recording::capture(d, &v).unwrap()
    }
    fn stop(d: &mut Document, at: u64) -> Value {
        let mut v = block(at, at, 0, json!([]));
        v["playing"] = json!(false);
        v["recording"] = json!(false);
        send(d, v)
    }
    #[test]
    fn midi_records_timestamped_gate_and_closes_held_note_at_stop() {
        let mut d = document(1);
        let r = send(&mut d, block(0, 80, 8, json!([event(2, 60, 0.5)])));
        assert_eq!(r["record_enabled_tracks"], json!(["m"]));
        send(&mut d, block(8, 88, 8, json!([])));
        assert!(d.clips.is_empty());
        stop(&mut d, 16);
        let c = d.clips.values().next().unwrap();
        assert_eq!(c.start, 80. / 8000.);
        let Content::Midi {
            notes, quarters, ..
        } = &c.content
        else {
            panic!()
        };
        assert_eq!(*quarters, 16. / 8000. * 2.);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].quarter, 2. / 8000. * 2.);
        assert_eq!(notes[0].length, 14. / 8000. * 2.);
        assert_eq!(d.recording.midi.takes["m"].clips.len(), 1);
    }
    #[test]
    fn midi_tracking_keeps_live_voice_without_recorded_clip() {
        let mut d = document(3);
        send(&mut d, block(0, 0, 8, json!([event(0, 69, 1.)])));
        assert_eq!(d.recording.midi.voices(0, 8, 8000).unwrap().len(), 1);
        assert!(d.recording.midi.voices(1, 8, 8000).is_err());
        stop(&mut d, 8);
        assert!(d.clips.is_empty());
    }
    #[test]
    fn midi_loop_preserves_each_take_and_cycles_last_to_previous() {
        let mut d = document(1);
        let mut b = block(0, 6, 8, json!([event(0, 60, 1.), event(7, 60, 0.)]));
        b["loop_start_frame"] = json!(4);
        b["loop_end_frame"] = json!(8);
        send(&mut d, b);
        stop(&mut d, 8);
        assert_eq!(d.recording.midi.takes["m"].clips.len(), 3);
        assert_eq!(d.clips.len(), 1);
        let c = d.clips.values().next().unwrap();
        assert_eq!(c.start, 4. / 8000.);
        assert_eq!(c.duration(), 2. / 8000.);
        cycle(&mut d, &json!({"id":"m","direction":-1})).unwrap();
        assert_eq!(d.clips.len(), 1);
        assert_eq!(d.clips.values().next().unwrap().duration(), 4. / 8000.);
    }
    #[test]
    fn midi_overwrite_archives_overlap_and_keeps_clipped_outside_notes() {
        let mut d = document(1);
        let old = Clip { output:String::new(),lane:0.,gain:1., fade_in:0., fade_out:0.,
            id: "old".into(),
            track: "m".into(),
            start: 0.,
            content: Content::Midi { edo:12,scale:0,root:0,
                bpm: 120.,
                quarters: 8. / 8000. * 2.,
                numerator: 4,
                denominator: 4,
                notes: vec![Note { bend:0.,tuning_cents:0.,
                    id: "oldnote".into(),
                    quarter: 0.,
                    length: 8. / 8000. * 2.,
                    pitch: 60,
                    velocity: 0.5,
                }],
            },
        };
        d.clip_order.push(old.id.clone());
        d.clips.insert(old.id.clone(), old);
        send(&mut d, block(0, 2, 3, json!([event(0, 64, 1.)])));
        stop(&mut d, 3);
        assert_eq!(d.clips.len(), 3);
        assert_eq!(d.recording.midi.takes["m"].clips.len(), 2);
        let total = d.clips.values().map(|c| c.duration()).sum::<f64>();
        assert!((total - 8. / 8000.).abs() < 1e-10);
        for c in d.clips.values() {
            let Content::Midi {
                notes, quarters, ..
            } = &c.content
            else {
                panic!()
            };
            assert!(
                notes
                    .iter()
                    .all(|n| n.quarter + n.length <= quarters + 1e-10)
            );
        }
    }
    #[test]
    fn midi_invalid_tuning_is_atomic_and_disarm_closes_gate() {
        let mut d = document(1);
        send(&mut d, block(0, 0, 8, json!([event(0, 60, 1.)])));
        let before = d.recording.status();
        let mut e = event(8, 64, 1.);
        e["tuning_cents"] = json!(100000.);
        assert!(super::super::recording::capture(&mut d, &block(8, 8, 8, json!([e]))).is_err());
        assert_eq!(before, d.recording.status());
        d.tracks.get_mut("m").unwrap().io.arm_mode = 0;
        d.recording.disarm("m");
        send(&mut d, block(8, 8, 8, json!([])));
        stop(&mut d, 16);
        let Content::Midi { notes, .. } = &d.clips.values().next().unwrap().content else {
            panic!()
        };
        assert_eq!(notes[0].length, 8. / 8000. * 2.);
    }
}
