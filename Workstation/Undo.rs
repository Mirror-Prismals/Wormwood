//! Cardinal DawUndoSystem's bounded journal, adapted to owned Rust document
//! checkpoints. Runs only under the serialized control-side document owner.
//! PCM and spectral/warp buffers remain shared immutable allocations.
use super::{
    document::{self, Clip, State, Track},
    playback::Source,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};

const MAX_ENTRIES: usize = 256;
#[derive(Clone)]
struct Checkpoint {
    tracks: BTreeMap<String, Track>,
    buses: super::buses::Buses,
    track_order: Vec<String>,
    clips: BTreeMap<String, Clip>,
    clip_order: Vec<String>,
    sources: BTreeMap<String, Source>,
    treatments: BTreeMap<String, super::clip_treatment::Treatment>,
    playlists: super::audio_playlists::State,
    timing: super::musical_time::Timing,
    zero: f64,
    drums: super::drum_rack::Rack,
    warp: std::sync::Arc<super::warp_treatment::Engine>,
}
impl Checkpoint {
    fn take(s: &State) -> Self {
        Self {
            tracks: s.tracks.clone(),
            buses:s.buses.clone(),
            track_order: s.track_order.clone(),
            clips: s.clips.clone(),
            clip_order: s.clip_order.clone(),
            sources: s.sources.clone(),
            treatments: s.clip_treatments.clone(),
            playlists: s.audio_playlists.clone(),
            timing: s.musical_time.clone(),
            zero: s.timeline_zero_seconds,
            drums: s.drum_rack.frozen(),
            warp: std::sync::Arc::new(s.warp.frozen()),
        }
    }
    fn apply(&self, s: &mut State) {
        s.tracks = self.tracks.clone();
        s.buses=self.buses.clone();
        s.track_order = self.track_order.clone();
        s.clips = self.clips.clone();
        s.clip_order = self.clip_order.clone();
        s.sources = self.sources.clone();
        s.clip_treatments = self.treatments.clone();
        s.audio_playlists = self.playlists.clone();
        s.musical_time = self.timing.clone();
        s.timeline_zero_seconds = self.zero;
        s.warp = self.warp.frozen();
        s.drum_rack = self.drums.frozen();
        // Local histories cannot replay across a global document restoration.
        s.fade_history = Default::default();
    }
}
struct Entry {
    before: Checkpoint,
    after: Checkpoint,
    label: String,
    merge_key: String,
}
#[derive(Default)]
pub(super) struct Journal {
    undo: VecDeque<Entry>,
    redo: VecDeque<Entry>,
    transaction: Option<Entry>,
    depth: usize,
    label: String,
    merge_key: String,
    revision: Option<u64>,
    status: String,
}
impl Journal {
    pub(super) fn status(&self) -> Value {
        json!({"undo_available":self.depth==0&&!self.undo.is_empty(),
            "redo_available":self.depth==0&&!self.redo.is_empty(),
            "undo_label":self.undo.back().map(|e|&e.label),
            "redo_label":self.redo.back().map(|e|&e.label),
            "undo_count":self.undo.len(),"redo_count":self.redo.len(),
            "transaction_depth":self.depth,"status":self.status,"capacity":MAX_ENTRIES,
            "domain":"content"})
    }
    fn push(&mut self, entry: Entry) {
        // Cardinal's explicit merge key denotes ONE gesture, never a timeout.
        if !entry.merge_key.is_empty()
            && self.redo.is_empty()
            && self
                .undo
                .back()
                .is_some_and(|e| e.merge_key == entry.merge_key)
        {
            let old = self.undo.back_mut().unwrap();
            old.after = entry.after;
            old.label = entry.label;
        } else {
            self.undo.push_back(entry);
        }
        self.redo.clear();
        while self.undo.len() > MAX_ENTRIES {
            self.undo.pop_front();
        }
    }
    fn record(&mut self, mut entry: Entry) {
        if self.depth > 0 {
            if let Some(old) = &mut self.transaction {
                old.after = entry.after;
            } else {
                entry.label = self.label.clone();
                entry.merge_key = self.merge_key.clone();
                self.transaction = Some(entry);
            }
        } else {
            self.push(entry);
        }
    }
}
fn editable(s: &State) -> Result<(), String> {
    if s.freeze.busy() || s.freeze.any() {
        return Err("Finish or flatten track freeze before undo/redo".into());
    }
    if s.recording.status()["recording"] == true
        || s.recording.status()["pending_frames"].as_u64().unwrap_or(0) > 0
        || s.recording.midi.pending()
    {
        return Err("Stop recording before undo/redo".into());
    }
    Ok(())
}
fn text(v: &Value, key: &str, default: &str) -> Result<String, String> {
    match v.get(key) {
        None => Ok(default.into()),
        Some(Value::String(s)) if s.len() <= 128 && !s.chars().any(char::is_control) => {
            Ok(s.clone())
        }
        _ => Err(format!("{key} must be printable text of at most 128 bytes")),
    }
}
fn command(s: &mut State, hook: &str, v: &Value) -> Result<Value, String> {
    if v["domain"].as_str().is_some_and(|d| d != "content") {
        return Err(
            "This workstation has no editable style document; undo domain must be content".into(),
        );
    }
    if hook == "UndoStatus" {
        return Ok(s.undo.status());
    }
    s.require_revision(v)?;
    if hook == "ClearUndo" {
        s.undo = Box::default();
        s.undo.revision = Some(s.revision);
        return Ok(s.undo.status());
    }
    editable(s)?;
    match hook {
        "BeginUndoTransaction" => {
            let label = text(v, "label", "Edit")?;
            let key = text(v, "merge_key", "")?;
            if s.undo.depth == 0 {
                s.undo.label = label;
                s.undo.merge_key = key;
            }
            s.undo.depth = s
                .undo
                .depth
                .checked_add(1)
                .ok_or("undo transaction depth exhausted")?;
        }
        "CommitUndoTransaction" => {
            if s.undo.depth == 0 {
                return Err("no undo transaction is open".into());
            }
            s.undo.depth -= 1;
            if s.undo.depth == 0 {
                if let Some(entry) = s.undo.transaction.take() {
                    s.undo.push(entry);
                }
            }
        }
        // Like Cardinal, cancellation discards history bookkeeping, NOT edits.
        // Invalidate prior snapshots so they cannot overwrite cancelled edits.
        "CancelUndoTransaction" => {
            if s.undo.depth == 0 {
                return Err("no undo transaction is open".into());
            }
            if s.undo.transaction.is_some() {
                s.undo = Box::default();
            } else {
                s.undo.depth = 0;
            }
        }
        "Undo" | "Redo" => {
            if s.undo.depth != 0 {
                return Err("finish the undo transaction first".into());
            }
            s.revision
                .checked_add(1)
                .ok_or("document revision exhausted")?;
            let undo = hook == "Undo";
            let entry = if undo {
                s.undo.undo.pop_back()
            } else {
                s.undo.redo.pop_back()
            }
            .ok_or("nothing to undo or redo")?;
            if undo {
                entry.before.apply(s);
            } else {
                entry.after.apply(s);
            }
            s.changed();
            s.undo.status = format!("{} {}", if undo { "Undid" } else { "Redid" }, entry.label);
            if undo {
                s.undo.redo.push_back(entry);
            } else {
                s.undo.undo.push_back(entry);
            }
        }
        _ => unreachable!(),
    }
    s.undo.revision = Some(s.revision);
    Ok(json!({"revision":s.revision,"undo":s.undo.status()}))
}
pub(super) fn dispatch(s: &mut State, hook: &str, v: &Value) -> Result<Value, String> {
    if s.undo.revision.is_some_and(|r| r != s.revision) {
        s.undo = Box::default();
    }
    if matches!(
        hook,
        "Undo"
            | "Redo"
            | "UndoStatus"
            | "BeginUndoTransaction"
            | "CommitUndoTransaction"
            | "CancelUndoTransaction"
            | "ClearUndo"
    ) {
        return command(s, hook, v);
    }
    let record = (hook == "DrumRack"
        || matches!(
            hook,
            "PutBus" | "SetBus" | "RemoveBus" | "PutTrack" | "RemoveTrack"
                | "PutClip"
                | "CreateRangeClips"
                | "RemoveClip"
                | "TrimAudioClip"
                | "SetClipGain" | "SetClipRouting"
                | "SplitClip"
                | "SetClipFades"
                | "MoveClip"
                | "RebaseTimeline"
                | "SetMusicalTime"
                | "SetMidiTempo"
                | "SnapMidi"
                | "SnapClip"
                | "SetTrackMix"
                | "SetCcTarget"
                | "ReorderTracks"
                | "PutSource"
                | "ImportAudio"
                | "SetClipWarp"
                | "SetWarpMap"
                | "AddSpectralMask"
                | "UndoSpectralMask"
                | "ClearSpectralMasks"
                | "AudioPlaylist"
                | "CycleMidiTake"
                | "CaptureInput"
                | "CommitRetrospectiveAudio"
                | "InsertRetrospectiveMidi"
        ))
        && (hook != "CaptureInput" || v["playing"] == false);
    // Parse journal metadata before any mutation; rejected edits never enter it.
    let label = if record {
        text(v, "undo_label", hook)?
    } else {
        String::new()
    };
    let key = if record {
        text(v, "merge_key", "")?
    } else {
        String::new()
    };
    let before = record.then(|| Checkpoint::take(s));
    let revision = s.revision;
    let result = document::dispatch_inner(s, hook, v);
    if s.revision != revision {
        if result.is_ok() && record {
            let after = Checkpoint::take(s);
            s.undo.record(Entry {
                before: before.unwrap(),
                after,
                label,
                merge_key: key,
            });
        } else if hook != "PumpWarp" {
            // Unsupported document owners form a history barrier, never silently
            // restore an incomplete plugin/drum/freeze/legacy editor snapshot.
            s.undo = Box::default();
        }
        s.undo.revision = Some(s.revision);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn call(s: &mut State, h: &str, mut v: Value) -> Result<Value, String> {
        v["document_revision"] = json!(s.revision);
        document::dispatch(s, h, &v)
    }
    fn track(s: &mut State, id: &str) {
        call(s, "PutTrack", json!({"id":id,"name":id,"kind":"audio"})).unwrap();
    }
    #[test]
    fn ordered_edits_redo_branch_and_stale_rejection() {
        let mut s = State::default();
        track(&mut s, "a");
        track(&mut s, "b");
        call(&mut s, "SetTrackMix", json!({"id":"a","gain":0.5})).unwrap();
        call(&mut s, "Undo", json!({})).unwrap();
        assert_eq!(s.tracks["a"].gain, 1.);
        call(&mut s, "Undo", json!({})).unwrap();
        assert!(!s.tracks.contains_key("b"));
        call(&mut s, "Redo", json!({})).unwrap();
        assert!(s.tracks.contains_key("b"));
        let count = s.undo.undo.len();
        assert!(document::dispatch(&mut s, "Undo", &json!({"document_revision":0})).is_err());
        assert!(call(&mut s, "SetTrackMix", json!({"id":"a","gain":-1})).is_err());
        assert_eq!(count, s.undo.undo.len());
        track(&mut s, "c");
        assert!(s.undo.redo.is_empty());
    }
    #[test]
    fn nested_transaction_and_explicit_gesture_merge() {
        let mut s = State::default();
        track(&mut s, "a");
        for _ in 0..2 {
            call(
                &mut s,
                "BeginUndoTransaction",
                json!({"label":"Mix gesture"}),
            )
            .unwrap();
        }
        for gain in [0.25, 0.5] {
            call(&mut s, "SetTrackMix", json!({"id":"a","gain":gain})).unwrap();
        }
        assert!(call(&mut s, "Undo", json!({})).is_err());
        for _ in 0..2 {
            call(&mut s, "CommitUndoTransaction", json!({})).unwrap();
        }
        call(&mut s, "Undo", json!({})).unwrap();
        assert_eq!(s.tracks["a"].gain, 1.);
        call(&mut s, "Redo", json!({})).unwrap();
        assert_eq!(s.tracks["a"].gain, 0.5);
        for gain in [0.3, 0.7] {
            call(
                &mut s,
                "SetTrackMix",
                json!({"id":"a","gain":gain,"merge_key":"drag-17"}),
            )
            .unwrap();
        }
        call(&mut s, "Undo", json!({})).unwrap();
        assert_eq!(s.tracks["a"].gain, 0.5);
        call(&mut s, "Redo", json!({})).unwrap();
        assert_eq!(s.tracks["a"].gain, 0.7);
    }
    #[test]
    fn cancellation_and_capacity() {
        let mut s = State::default();
        track(&mut s, "a");
        call(&mut s, "BeginUndoTransaction", json!({})).unwrap();
        call(&mut s, "SetTrackMix", json!({"id":"a","gain":0.5})).unwrap();
        call(&mut s, "CancelUndoTransaction", json!({})).unwrap();
        assert_eq!(s.tracks["a"].gain, 0.5);
        assert!(s.undo.undo.is_empty());
        for i in 0..270 {
            call(
                &mut s,
                "SetTrackMix",
                json!({"id":"a","gain":i as f64/270.}),
            )
            .unwrap();
        }
        assert_eq!(s.undo.undo.len(), MAX_ENTRIES);
    }
    #[test]
    fn pcm_is_shared_and_split_gain_restore_in_order() {
        let mut s = State::default();
        track(&mut s, "a");
        call(
            &mut s,
            "PutSource",
            json!({"id":"src","sample_rate":8000,"channels":1,"samples":vec![0.5;64]}),
        )
        .unwrap();
        call(&mut s,"PutClip",json!({"id":"c","track":"a","kind":"audio","source":"src","start_seconds":0.,"source_offset_seconds":0.,"duration_seconds":0.008})).unwrap();
        let pcm = s.sources["src"].samples.clone();
        call(&mut s, "SetClipGain", json!({"id":"c","gain":0.5})).unwrap();
        call(&mut s, "SplitClip", json!({"id":"c","time_seconds":0.004})).unwrap();
        assert_eq!(s.clips.len(), 2);
        call(&mut s, "Undo", json!({})).unwrap();
        assert_eq!(s.clips.len(), 1);
        assert_eq!(s.clips["c"].gain, 0.5);
        call(&mut s, "Undo", json!({})).unwrap();
        assert_eq!(s.clips["c"].gain, 1.);
        call(&mut s, "Redo", json!({})).unwrap();
        call(&mut s, "Redo", json!({})).unwrap();
        assert_eq!(s.clips.len(), 2);
        assert!(std::sync::Arc::ptr_eq(&pcm, &s.sources["src"].samples));
    }
}
