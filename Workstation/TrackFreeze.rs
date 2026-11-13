//! Incremental control-side track bounce. Completion is provisional until OK;
//! cancelling simply drops isolated DSP/PCM and leaves the document untouched.
use super::{
    document::{Clip, Content, State as Document},
    playback::Source,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
const MAX_VALUES: usize = 16_777_216; // existing document capture/source memory budget
struct Job {
    snapshot: Box<Document>,
    track: String,
    rate: u32,
    start: u64,
    end: u64,
    cursor: u64,
    pcm: Vec<f32>,
}
struct Frozen {
    originals: Vec<Clip>,
    source: String,
    clip: String,
}
#[derive(Default)]
pub(crate) struct State {
    job: Option<Job>,
    frozen: BTreeMap<String, Frozen>,
    report: Value,
    next: u64,
}
impl State {
    pub(crate) fn busy(&self) -> bool {
        self.job.is_some()
    }
    pub(crate) fn frozen(&self, id: &str) -> bool {
        self.frozen.contains_key(id)
    }
    pub(crate) fn any(&self) -> bool {
        !self.frozen.is_empty()
    }
}
fn token(s: &Document, q: &Value) -> Result<(), String> {
    if q["job_id"] != s.freeze.report["job_id"] || q["job_id"].as_u64().is_none() {
        Err("Stale freeze job".into())
    } else {
        Ok(())
    }
}
pub(crate) fn dispatch(s: &mut Document, hook: &str, q: &Value) -> Result<Value, String> {
    match hook {
        "TrackFreezeStatus" => {}
        "TrackFreezeBegin" => {
            if s.recording.status()["recording"] == true
                || s.recording.status()["pending_frames"].as_u64().unwrap_or(0) > 0
            {
                return Err("Stop recording and finish pending captures before freezing".into());
            }
            if s.freeze.busy() || s.export_active() {
                return Err("Another render is active".into());
            }
            if q["document_revision"].as_u64() != Some(s.revision) {
                return Err("Stale document revision".into());
            }
            let id = q["id"].as_str().ok_or("Select a track handle first")?;
            let track = s.tracks.get(id).ok_or("Unknown track")?;
            if track.kind == "cc" || s.freeze.frozen(id) {
                return Err("Select an unfrozen audio or MIDI track".into());
            }
            if s.recording.busy(id) {
                return Err("Stop recording and finish pending captures before freezing".into());
            }
            let clips = s
                .clips
                .values()
                .filter(|c| c.track == id)
                .collect::<Vec<_>>();
            let start = clips.iter().map(|c| c.start).fold(f64::INFINITY, f64::min);
            let mut end = clips
                .iter()
                .map(|c| c.start + c.duration())
                .fold(0., f64::max);
            if !start.is_finite() || end <= start {
                return Err("Track has no clips to freeze".into());
            }
            if s.plugins.has_track(id, false) {
                end += 2.;
            } else if track.kind == "midi" {
                end += 0.12;
            }
            let rate = q["sample_rate"].as_u64().unwrap_or_else(|| {
                clips
                    .iter()
                    .find_map(|c| {
                        if let Content::Audio { source, .. } = &c.content {
                            s.sources.get(source).map(|s| s.sample_rate as u64)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(48000)
            });
            if !(8000..=192000).contains(&rate) {
                return Err("Invalid freeze sample rate".into());
            }
            let rate = rate as u32;
            let start = (start * rate as f64).round() as u64;
            let end = (end * rate as f64).round() as u64;
            let stored = s.sources.values().map(|s| s.samples.len()).sum::<usize>();
            if end <= start
                || end - start > ((MAX_VALUES.saturating_sub(stored)) / 2) as u64
                || s.sources.len() >= 256
            {
                return Err("Freeze exceeds the available source memory budget".into());
            }
            let mut snapshot = s.freeze_for_export();
            snapshot.plugins =
                s.plugins
                    .clone_track(id, &track.kind, rate, q["root"].as_str().unwrap_or("."))?;
            // Only this track sounds. Preserve CC lanes for hosted parameter automation.
            for t in snapshot.tracks.values_mut() {
                t.mute = t.kind != "cc" && t.id != id;
                t.solo = false;
                if t.id == id {
                    t.io.input_monitor_mode = 0;
                }
            }
            s.freeze.next += 1;
            s.freeze.report =
                json!({"job_id":s.freeze.next,"phase":"rendering","progress":0.,"track":id});
            s.freeze.job = Some(Job {
                snapshot,
                track: id.into(),
                rate,
                start,
                end,
                cursor: start,
                pcm: Vec::new(),
            });
        }
        "TrackFreezeStep" => {
            token(s, q)?;
            if let Some(mut job) = s.freeze.job.take() {
                if job.snapshot.revision != s.revision {
                    s.freeze.report["phase"] = json!("failed");
                    s.freeze.report["error"] = json!("Document changed while freezing");
                    return Ok(s.freeze.report.clone());
                }
                if job.cursor < job.end {
                    let count = (job.end - job.cursor).min(4096);
                    let result = super::playback::render(
                        &job.snapshot,
                        &json!({"sample_rate":job.rate,"start_frame":job.cursor,"frames":count,"instrument":"cardinal_sine","freeze_track":job.track,"playback_clock":{"generation":s.freeze.next,"sequence":job.cursor-job.start}}),
                    );
                    match result {
                        Ok(block) => {
                            let pcm: Vec<[f32; 2]> =
                                match serde_json::from_value(block["freeze_pcm"].clone()) {
                                    Ok(pcm) => pcm,
                                    Err(e) => {
                                        s.freeze.report["phase"] = json!("failed");
                                        s.freeze.report["error"] = json!(e.to_string());
                                        return Ok(s.freeze.report.clone());
                                    }
                                };
                            if pcm.len() != count as usize
                                || pcm.iter().flatten().any(|v| !v.is_finite())
                            {
                                s.freeze.report["phase"] = json!("failed");
                                s.freeze.report["error"] = json!("Freeze produced invalid PCM");
                                return Ok(s.freeze.report.clone());
                            }
                            job.pcm.extend(pcm.into_iter().flatten());
                            job.cursor += count;
                        }
                        Err(e) => {
                            s.freeze.report["phase"] = json!("failed");
                            s.freeze.report["error"] = json!(e);
                            return Ok(s.freeze.report.clone());
                        }
                    }
                }
                s.freeze.report["progress"] =
                    json!((job.cursor - job.start) as f64 / (job.end - job.start) as f64);
                if job.cursor == job.end {
                    s.freeze.report["phase"] = json!("finished");
                }
                s.freeze.job = Some(job);
            }
        }
        "TrackFreezeCancel" => {
            token(s, q)?;
            s.freeze.job = None;
            s.freeze.report["phase"] = json!("cancelled");
        }
        "TrackFreezeAccept" => {
            token(s, q)?;
            if s.freeze.report["phase"] != "finished" {
                return Err("Freeze has not finished".into());
            }
            let job = s.freeze.job.as_ref().ok_or("No pending freeze")?;
            if job.snapshot.revision != s.revision {
                return Err("Document changed while freezing".into());
            }
            let job = s.freeze.job.take().unwrap();
            let mut source = format!("freeze-source-{}", s.freeze.next);
            while s.sources.contains_key(&source) {
                source.push('_');
            }
            let mut clip = format!("freeze-clip-{}", s.freeze.next);
            while s.clips.contains_key(&clip) {
                clip.push('_');
            }
            let originals = s
                .clip_order
                .iter()
                .filter_map(|id| s.clips.get(id).filter(|c| c.track == job.track).cloned())
                .collect::<Vec<_>>();
            for c in &originals {
                s.clips.remove(&c.id);
            }
            s.clip_order.retain(|id| s.clips.contains_key(id));
            s.sources.insert(
                source.clone(),
                Source {
                    sample_rate: job.rate,
                    channels: 2,
                    samples: job.pcm.into(),
                },
            );
            s.clips.insert(
                clip.clone(),
                Clip { output:String::new(),lane:0.,
                    id: clip.clone(),
                    track: job.track.clone(),
                    start: job.start as f64 / job.rate as f64,
                    gain: 1.,
                    fade_in: 0.,
                    fade_out: 0.,
                    content: Content::Audio {
                        source: source.clone(),
                        offset: 0.,
                        duration: (job.end - job.start) as f64 / job.rate as f64,
                    },
                },
            );
            s.clip_order.push(clip.clone());
            s.tracks.get_mut(&job.track).unwrap().kind = "audio".into();
            s.tracks.get_mut(&job.track).unwrap().io.arm_mode = 0;
            s.tracks.get_mut(&job.track).unwrap().io.input_monitor_mode = 0;
            s.freeze.frozen.insert(
                job.track,
                Frozen {
                    originals,
                    source,
                    clip,
                },
            );
            s.changed();
            s.freeze.report["phase"] = json!("accepted");
        }
        "TrackFreezeFlatten" => {
            if s.freeze.busy() {
                return Err("Finish the current freeze first".into());
            }
            if q["document_revision"].as_u64() != Some(s.revision) {
                return Err("Stale document revision".into());
            }
            let id = q["id"].as_str().ok_or("Track required")?;
            if !s.freeze.frozen(id) {
                return Err("Track is not frozen".into());
            }
            let plugins = s.plugins.status();
            for row in plugins["inserts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|p| {
                    p["track"] == id && p["chain"].as_str().unwrap_or("playback") == "playback"
                })
            {
                s.plugins.command(&json!({"op":"remove","id":row["id"]}))?;
            }
            let frozen = s.freeze.frozen.remove(id).unwrap();
            for c in frozen.originals {
                s.warp.entries.remove(&c.id);
                s.clip_treatments.remove(&c.id);
            }
            let _ = (frozen.source, frozen.clip); // baked source/clip become ordinary editable audio
            s.changed();
            s.freeze.report = json!({"phase":"flattened","track":id});
        }
        _ => return Err("Unknown track freeze operation".into()),
    }
    Ok(s.freeze.report.clone())
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn ww_track_freeze_dialog(
        op: i32,
        progress: f64,
        message: *const std::ffi::c_char,
        finished: i32,
    ) -> i32;
}
fn dialog(op: i32, report: &Value) -> Result<i32, String> {
    #[cfg(target_os = "macos")]
    {
        let message = if report["phase"] == "finished" {
            "Finished. Click OK to keep the freeze, or Cancel to undo it."
        } else if let Some(e) = report["error"].as_str() {
            e
        } else {
            "Freezing track…"
        };
        let message = std::ffi::CString::new(message).map_err(|e| e.to_string())?;
        let result = unsafe {
            ww_track_freeze_dialog(
                op,
                report["progress"].as_f64().unwrap_or(0.),
                message.as_ptr(),
                i32::from(report["phase"] == "finished" || report["phase"] == "failed"),
            )
        };
        if result < 0 {
            return Err("Freeze popup requires the main thread".into());
        }
        Ok(result)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (op, report);
        Err("Track freeze popup is available on macOS".into())
    }
}
#[derive(Default)]
pub(crate) struct Workflow {
    report: Option<Value>,
    event: Option<Value>,
}
impl Workflow {
    pub(crate) fn active(&self) -> bool {
        self.report.is_some()
    }
    pub(crate) fn start(&mut self, host: &mut crate::host::Host, id: &str) -> Result<(), String> {
        if self.active() {
            return Ok(());
        }
        let snapshot = host.invoke("WorkstationDataSystem.Snapshot", Value::Null)?;
        let track = snapshot["tracks"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|t| t["id"] == id)
            .ok_or("Select a track handle first")?;
        let audio = host.invoke("EnvironmentAudioSystem.Status", Value::Null)?;
        if audio["transport"]["recording"] == true {
            return Err("Stop recording before freezing a track".into());
        }
        let request = json!({"id":id,"document_revision":snapshot["revision"],"root":host.root().to_string_lossy(),"sample_rate":audio["transport"]["sample_rate"].as_u64().unwrap_or(48000)});
        let event = host.invoke("AudicleSystem.Create", Value::Null)?;
        if track["frozen"] == true {
            host.invoke("AudicleSystem.Ready", event.clone())?;
            host.invoke("AudicleSystem.Go", event.clone())?;
            let result = host.invoke("WorkstationDataSystem.TrackFreezeFlatten", request);
            host.invoke(
                if result.is_ok() {
                    "AudicleSystem.Finish"
                } else {
                    "AudicleSystem.Fail"
                },
                event,
            )?;
            result?;
            return Ok(());
        }
        let report = match host.invoke("WorkstationDataSystem.TrackFreezeBegin", request) {
            Ok(r) => r,
            Err(e) => {
                host.invoke("AudicleSystem.Fail", event)?;
                return Err(e);
            }
        };
        self.report = Some(report);
        self.event = Some(event.clone());
        host.invoke("AudicleSystem.Ready", event.clone())?;
        host.invoke("AudicleSystem.Go", event)?;
        if let Err(e) = dialog(0, &Value::Null) {
            self.close(host)?;
            return Err(e);
        }
        Ok(())
    }
    pub(crate) fn pump(&mut self, host: &mut crate::host::Host) -> Result<(), String> {
        let Some(report) = self.report.clone() else {
            return Ok(());
        };
        match dialog(1, &report)? {
            1 if report["phase"] == "finished" => {
                let result = host.invoke(
                    "WorkstationDataSystem.TrackFreezeAccept",
                    json!({"job_id":report["job_id"]}),
                );
                if let Err(e) = result {
                    self.close(host)?;
                    return Err(e);
                }
                if let Some(event) = self.event.take() {
                    host.invoke("AudicleSystem.Finish", event)?;
                }
                self.report = None;
                dialog(3, &Value::Null)?;
                return Ok(());
            }
            1 | 2 => {
                return self.close(host);
            }
            _ => {}
        }
        if report["phase"] == "rendering" {
            let next = host.invoke(
                "WorkstationDataSystem.TrackFreezeStep",
                json!({"job_id":report["job_id"]}),
            )?;
            if next["phase"] == "failed" {
                if let Some(mut event) = self.event.take() {
                    event["reason"] = next["error"].clone();
                    host.invoke("AudicleSystem.Fail", event)?;
                }
            }
            dialog(2, &next)?;
            self.report = Some(next);
        }
        Ok(())
    }
    pub(crate) fn close(&mut self, host: &mut crate::host::Host) -> Result<(), String> {
        let was_open = self.report.is_some();
        if let Some(report) = self.report.take() {
            host.invoke(
                "WorkstationDataSystem.TrackFreezeCancel",
                json!({"job_id":report["job_id"]}),
            )?;
        }
        if let Some(event) = self.event.take() {
            host.invoke("AudicleSystem.Cancel", event)?;
        }
        #[cfg(target_os = "macos")]
        if was_open {
            dialog(3, &Value::Null)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn edit(s: &mut Document, hook: &str, mut q: Value) -> Value {
        q["document_revision"] = json!(s.revision);
        super::super::document::dispatch(s, hook, &q).unwrap()
    }
    fn fixture() -> Document {
        let mut s = Document::default();
        edit(
            &mut s,
            "PutTrack",
            json!({"id":"a","name":"Audio","kind":"audio"}),
        );
        edit(
            &mut s,
            "PutSource",
            json!({"id":"s","sample_rate":8000,"channels":2,"samples":[0.5,-0.25,1.,-0.5]}),
        );
        edit(
            &mut s,
            "PutClip",
            json!({"id":"c","track":"a","kind":"audio","start_seconds":1.,"source":"s","source_offset_seconds":0.,"duration_seconds":2./8000.,"gain":0.5}),
        );
        s
    }
    fn begin(s: &mut Document) -> Value {
        edit(s, "TrackFreezeBegin", json!({"id":"a","sample_rate":8000}))
    }
    fn finish(s: &mut Document, r: &Value) -> Value {
        let q = json!({"job_id":r["job_id"]});
        for _ in 0..100 {
            let r = dispatch(s, "TrackFreezeStep", &q).unwrap();
            if r["phase"] != "rendering" {
                return r;
            }
        }
        panic!("render did not finish")
    }
    #[test]
    fn cancel_after_finished_preserves_original_document_and_pcm() {
        let mut s = fixture();
        let before = edit(&mut s, "Snapshot", json!({}));
        let r = begin(&mut s);
        let done = finish(&mut s, &r);
        assert_eq!(done["phase"], "finished");
        assert_eq!(edit(&mut s, "Snapshot", json!({})), before);
        dispatch(&mut s, "TrackFreezeCancel", &json!({"job_id":r["job_id"]})).unwrap();
        assert_eq!(edit(&mut s, "Snapshot", json!({})), before);
        assert!(!s.freeze.frozen("a"));
    }
    #[test]
    fn accept_bakes_clip_gain_once_keeps_mix_and_flatten_unlocks_audio() {
        let mut s = fixture();
        let route = s.tracks["a"].route.snapshot();
        let gain = s.tracks["a"].gain;
        let r = begin(&mut s);
        finish(&mut s, &r);
        dispatch(&mut s, "TrackFreezeAccept", &json!({"job_id":r["job_id"]})).unwrap();
        assert!(s.freeze.frozen("a"));
        let c = &s.clips[&s.clip_order[0]];
        assert_eq!(c.start, 1.);
        assert_eq!(c.gain, 1.);
        let Content::Audio { source, .. } = &c.content else {
            panic!()
        };
        assert_eq!(&*s.sources[source].samples, &[0.25, -0.125, 0.5, -0.25]);
        assert_eq!(s.tracks["a"].gain, gain);
        assert_eq!(s.tracks["a"].route.snapshot(), route);
        let id = c.id.clone();
        let revision = s.revision;
        assert!(
            super::super::document::dispatch(
                &mut s,
                "RemoveClip",
                &json!({"document_revision":revision,"id":id})
            )
            .is_err()
        );
        edit(&mut s, "TrackFreezeFlatten", json!({"id":"a"}));
        assert!(!s.freeze.frozen("a"));
        edit(&mut s, "SetClipGain", json!({"id":id,"gain":0.75}));
    }
    #[test]
    fn stale_tokens_and_edits_cannot_commit_or_corrupt_pending_freeze() {
        let mut s = fixture();
        let r = begin(&mut s);
        assert!(dispatch(&mut s, "TrackFreezeCancel", &json!({"job_id":99})).is_err());
        assert!(dispatch(&mut s, "TrackFreezeAccept", &json!({"job_id":r["job_id"]})).is_err());
        let revision = s.revision;
        assert!(
            super::super::document::dispatch(
                &mut s,
                "MoveClip",
                &json!({"document_revision":revision,"id":"c","start_seconds":9.})
            )
            .is_err()
        );
        dispatch(&mut s, "TrackFreezeCancel", &json!({"job_id":r["job_id"]})).unwrap();
        assert_eq!(s.clips["c"].start, 1.);
    }
    #[test]
    fn missing_source_fails_without_replacing_originals() {
        let mut s = fixture();
        s.sources.clear();
        let r = begin(&mut s);
        assert_eq!(finish(&mut s, &r)["phase"], "failed");
        assert!(s.clips.contains_key("c"));
        assert!(!s.freeze.busy());
    }
}
#[cfg(all(test, target_os = "macos"))]
mod native_tests {
    use super::*;
    #[test]
    fn isolated_stock_effect_bakes_and_frozen_playback_skips_original_chain() {
        let mut s = Document::default();
        let edit = |s: &mut Document, hook: &str, mut q: Value| {
            q["document_revision"] = json!(s.revision);
            super::super::document::dispatch(s, hook, &q).unwrap()
        };
        edit(
            &mut s,
            "PutTrack",
            json!({"id":"a","name":"Audio","kind":"audio"}),
        );
        edit(
            &mut s,
            "PutSource",
            json!({"id":"s","sample_rate":8000,"channels":1,"samples":vec![0.2;512]}),
        );
        edit(
            &mut s,
            "PutClip",
            json!({"id":"c","track":"a","kind":"audio","start_seconds":0.,"source":"s","source_offset_seconds":0.,"duration_seconds":0.064}),
        );
        let added=s.plugins.command(&json!({"op":"add","stock":"LShaping","track":"a","track_kind":"audio","sample_rate":8000})).unwrap();
        let id = added["inserts"][0]["id"].clone();
        s.plugins
            .command(&json!({"op":"stock_parameter","id":id,"parameter":"gain_db","value":6.}))
            .unwrap();
        let original = s.plugins.command(&json!({"op":"save","id":id})).unwrap();
        let reference = super::super::playback::render(
            &s,
            &json!({"sample_rate":8000,"start_frame":0,"frames":512,"freeze_track":"a"}),
        )
        .unwrap();
        let r = edit(
            &mut s,
            "TrackFreezeBegin",
            json!({"id":"a","sample_rate":8000}),
        );
        for _ in 0..20 {
            let r = dispatch(&mut s, "TrackFreezeStep", &json!({"job_id":r["job_id"]})).unwrap();
            if r["phase"] == "finished" {
                break;
            }
        }
        assert_eq!(s.freeze.report["phase"], "finished");
        dispatch(&mut s, "TrackFreezeAccept", &json!({"job_id":r["job_id"]})).unwrap();
        assert_eq!(
            s.plugins.command(&json!({"op":"save","id":id})).unwrap(),
            original
        );
        let frozen = super::super::playback::render(
            &s,
            &json!({"sample_rate":8000,"start_frame":0,"frames":512,"freeze_track":"a"}),
        )
        .unwrap();
        let a: Vec<[f32; 2]> = serde_json::from_value(reference["freeze_pcm"].clone()).unwrap();
        let b: Vec<[f32; 2]> = serde_json::from_value(frozen["freeze_pcm"].clone()).unwrap();
        assert!(
            a.iter()
                .flatten()
                .zip(b.iter().flatten())
                .all(|(a, b)| (a - b).abs() < 1e-6)
        );
        edit(&mut s, "TrackFreezeFlatten", json!({"id":"a"}));
        assert!(!s.plugins.has_track("a", false));
    }
}
#[cfg(test)]
mod midi_tests {
    use super::*;
    #[test]
    fn midi_freeze_produces_stereo_audio_and_retains_original_notes_until_flatten() {
        let mut s = Document::default();
        let edit = |s: &mut Document, hook: &str, mut q: Value| {
            q["document_revision"] = json!(s.revision);
            super::super::document::dispatch(s, hook, &q).unwrap()
        };
        edit(
            &mut s,
            "PutTrack",
            json!({"id":"m","name":"MIDI","kind":"midi"}),
        );
        edit(
            &mut s,
            "PutClip",
            json!({"id":"notes","track":"m","kind":"midi","start_seconds":0.,"bpm":120.,"length_quarters":0.5,"meter":{"numerator":4,"denominator":4},"notes":[{"id":"n","quarter":0.,"length_quarters":0.25,"pitch":60,"velocity":1.}]}),
        );
        edit(&mut s, "SetTrackMix", json!({"id":"m","mute":true}));
        let r = edit(
            &mut s,
            "TrackFreezeBegin",
            json!({"id":"m","sample_rate":8000}),
        );
        let token = json!({"job_id":r["job_id"]});
        for _ in 0..10 {
            if dispatch(&mut s, "TrackFreezeStep", &token).unwrap()["phase"] == "finished" {
                break;
            }
        }
        dispatch(&mut s, "TrackFreezeAccept", &token).unwrap();
        assert_eq!(s.tracks["m"].kind, "audio");
        assert!(s.tracks["m"].mute);
        let f = &s.freeze.frozen["m"];
        assert!(matches!(&f.originals[0].content,Content::Midi{notes,..}if notes.len()==1));
        assert!(s.sources[&f.source].samples.iter().any(|v| v.abs() > 0.001));
        edit(&mut s, "TrackFreezeFlatten", json!({"id":"m"}));
        assert!(!s.freeze.any());
    }
}
