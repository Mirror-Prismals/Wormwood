//! Revision-owned master display analysis. Incremental preparation only: never
//! called by the audio callback, and never processes the live plugin instances.
use super::{
    document::State as Document,
    source_analysis::{Complex, fft},
};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::Arc;
pub(crate) const ID: &str = "::master-preview";
const FFT: usize = 8192;
const BINS: usize = 256;
#[derive(Default)]
pub(crate) struct State {
    key: Option<Value>,
    generation: u64,
    job: Option<Box<Job>>,
    result: Arc<Value>,
}
struct Job {
    document: Box<Document>,
    track: Option<String>,
    rate: u32,
    start: u64,
    end: u64,
    cursor: u64,
    hop: u64,
    next: u64,
    history: VecDeque<[f32; 2]>,
    peaks: [[f32; 2]; 128],
    spectral: Vec<Value>,
    times: Vec<f64>,
}
impl Job {
    fn analyze(&mut self) {
        let mut power = vec![0.; FFT / 2 + 1];
        let mut bins = vec![Complex::default(); FFT];
        for ch in 0..2 {
            bins.fill(Complex::default());
            let pad = FFT - self.history.len();
            for (i, s) in self.history.iter().enumerate() {
                let n = i + pad;
                bins[n].re = s[ch] as f64
                    * (0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / (FFT - 1) as f64).cos());
            }
            fft(&mut bins);
            for (p, b) in power.iter_mut().zip(&bins) {
                *p += b.re * b.re + b.im * b.im;
            }
        }
        let row = (0..BINS)
            .map(|i| {
                let hz = 440. * 2f64.powf((i as f64 * 0.5 - 69.) / 12.);
                let at = hz * FFT as f64 / self.rate as f64;
                let lo = at.floor() as usize;
                let energy = if lo + 1 < power.len() {
                    power[lo] * (1. - at.fract()) + power[lo + 1] * at.fract()
                } else {
                    0.
                };
                // Fixed dB reference preserves relative loudness between edits.
                let amplitude = (energy / 2.).sqrt() / (FFT as f64 * 0.25);
                ((20. * amplitude.max(1e-9).log10() + 80.) / 80.).clamp(0., 1.) as f32
            })
            .collect::<Vec<_>>();
        self.spectral.push(json!(row));
        self.times
            .push((self.cursor as f64 - FFT as f64 * 0.5) / self.rate as f64);
    }
}
/// The caller polls once per UI step. A completed immutable result is reused;
/// accepted edits (including Undo and plugin state) supersede the entire job.
pub(crate) fn step(s: &mut Document, input: &Value) -> Result<Value, String> {
    let status = s.plugins.status();
    let kind=input["kind"].as_str().unwrap_or("both");
    if !matches!(kind,"audio"|"midi"|"both"){return Err("Unknown spectrum track type".into());}
    let track=input["track"].as_str();
    if track.is_some_and(|id|!s.tracks.contains_key(id)){return Err("Spectrum track no longer exists".into());}
    let included=|id:&str|s.tracks.get(id).is_some_and(|t|t.kind!="cc"&&(kind=="both"||kind==t.kind)&&track.is_none_or(|selected|selected==id));
    let key = json!([s.revision, status["inserts"], status["sample_rate"],kind,track]);
    let enabled = input["enabled"] != false;
    if s.mix_preview.key.as_ref() != Some(&key)
        || (enabled && s.mix_preview.result["phase"] == "idle")
    {
        let generation = s.mix_preview.generation.wrapping_add(1);
        s.mix_preview = State {
            key: Some(key),
            generation,
            ..Default::default()
        };
        if !enabled {
            let result = json!({"revision":s.revision,"phase":"idle","generation":generation});
            s.mix_preview.result = Arc::new(result.clone());
            return Ok(result);
        }
        let begin = s
            .clips
            .values()
            .filter(|c| included(&c.track) && !matches!(c.content, super::document::Content::Cc { .. }))
            .map(|c| c.start)
            .fold(f64::INFINITY, f64::min);
        let end = s
            .clips
            .values()
            .filter(|c|included(&c.track))
            .map(|c| c.start + c.duration())
            .fold(0., f64::max);
        s.mix_preview.result = Arc::new(json!({"revision":s.revision,"phase":"empty"}));
        if begin.is_finite() && end > begin {
            let rate = s
                .clips
                .values()
                .find_map(|clip| match &clip.content {
                    super::document::Content::Audio { source, .. } => {
                        s.sources.get(source).map(|source| source.sample_rate)
                    }
                    _ => None,
                })
                .or_else(|| {
                    status["sample_rate"]
                        .as_u64()
                        .filter(|r| (8000..=192000).contains(r))
                        .map(|r| r as u32)
                })
                .unwrap_or(48000);
            let mut copy = s.freeze_for_export();
            // Filter only the isolated analysis document. Preserve live routing,
            // MIDI events, sources, gain automation and the user's mute/solo state.
            copy.clips.retain(|_,c|included(&c.track)||matches!(c.content,super::document::Content::Cc{..}));
            copy.clip_order.retain(|id|copy.clips.contains_key(id));
            for t in copy.tracks.values_mut(){if t.kind!="cc"&&!included(&t.id){t.mute=true;}}

            let mut tracks = s
                .tracks
                .values()
                .map(|t| (t.id.as_str(), t.kind.as_str()))
                .collect::<Vec<_>>();
            tracks.extend(
                s.buses
                    .rows
                    .iter()
                    .filter(|b| b.kind == "mix" || b.kind == "channel")
                    .map(|b| (b.id.as_str(), "audio")),
            );
            match s
                .plugins
                .clone_tracks(&tracks, rate, input["root"].as_str().unwrap_or("."))
            {
                Ok(plugins) => copy.plugins = plugins,
                Err(error) => {
                    s.mix_preview.result =
                        Arc::new(json!({"revision":s.revision,"phase":"failed","error":error}));
                    return Ok((*s.mix_preview.result).clone());
                }
            }
            // Preview observes playback; never capture live input or microphone.
            for t in copy.tracks.values_mut() {
                t.io.input_monitor_mode = 0;
            }
            let start = (begin * rate as f64).floor() as u64;
            let tail = if tracks.iter().any(|(id, _)| s.plugins.has_track(id, false)) {
                2.
            } else {
                0.12
            };
            let end = ((end + tail) * rate as f64).ceil() as u64;
            let hop = ((end - start).div_ceil(1024)).max(2048);
            s.mix_preview.job = Some(Box::new(Job {
                document: copy,
                track: track.map(str::to_owned),
                rate,
                start,
                end,
                cursor: start,
                hop,
                next: start + FFT as u64 / 2,
                history: VecDeque::with_capacity(FFT),
                peaks: [[0.; 2]; 128],
                spectral: Vec::new(),
                times: Vec::new(),
            }));
        }
    }
    // No rendering while both consumers are hidden. Retain a valid completed
    // cache or pause a pending job; an edit still invalidates it above.
    if !enabled {
        return Ok(json!({"unchanged":true}));
    }
    let Some(mut job) = s.mix_preview.job.take() else {
        if input["known_generation"].as_u64() == Some(s.mix_preview.generation) {
            return Ok(json!({"unchanged":true}));
        }
        let mut out = (*s.mix_preview.result).clone();
        out["generation"] = json!(s.mix_preview.generation);
        return Ok(out);
    };
    // A copied document owns its own pending warp preparation. Never publish
    // a master waveform with an enabled, not-yet-ready warped clip omitted.
    super::warp_timing::ensure(&mut job.document)?;
    super::warp_treatment::pump(&mut job.document);
    if let Err(error) = super::warp_treatment::ready_for_export(&job.document) {
        let failed = super::warp_treatment::snapshot(&job.document)
            .as_array()
            .is_some_and(|rows| rows.iter().any(|row| row["status"] == "error"));
        let mut result = json!({"revision":s.revision,"generation":s.mix_preview.generation,"phase":if failed{"failed"}else{"preparing"}});
        if failed {
            result["error"] = json!(error);
        } else {
            s.mix_preview.job = Some(job);
        }
        s.mix_preview.result = Arc::new(result.clone());
        return Ok(result);
    }
    let started = std::time::Instant::now();
    for _ in 0..32 {
        if job.cursor >= job.end {
            break;
        }
        let count = (job.end - job.cursor).min(4096);
        let block = match super::playback::render(
            &job.document,
            &json!({"sample_rate":job.rate,"start_frame":job.cursor,"frames":count,"instrument":"cardinal_sine","preview_track":job.track,"playback_clock":{"generation":s.revision,"sequence":job.cursor-job.start}}),
        ) {
            Ok(block) => block,
            Err(error) => {
                s.mix_preview.result =
                    Arc::new(json!({"revision":s.revision,"phase":"failed","error":error}));
                return Ok((*s.mix_preview.result).clone());
            }
        };
        let pcm_key=if job.track.is_some(){"preview_pcm"}else{"master_stereo"};
        for frame in block[pcm_key]
            .as_array()
            .ok_or("master preview missing PCM")?
        {
            let pcm = [
                frame[0].as_f64().unwrap_or(0.) as f32,
                frame[1].as_f64().unwrap_or(0.) as f32,
            ];
            let column =
                (((job.cursor - job.start) * 128 / (job.end - job.start)) as usize).min(127);
            for c in 0..2 {
                job.peaks[column][c] = job.peaks[column][c].max(pcm[c].abs());
            }
            if job.history.len() == FFT {
                job.history.pop_front();
            }
            job.history.push_back(pcm);
            job.cursor += 1;
            if job.cursor >= job.next {
                job.analyze();
                job.next += job.hop;
            }
        }
        if started.elapsed().as_millis() >= 4 {
            break;
        }
    }
    if job.cursor >= job.end {
        if job.times.is_empty()
            || *job.times.last().unwrap() < (job.end as f64 - FFT as f64 * 0.5) / job.rate as f64
        {
            job.analyze();
        }
        let wave = job
            .peaks
            .iter()
            .map(|p| json!([p[0], p[1], 0.45, 0.5]))
            .collect::<Vec<_>>();
        s.mix_preview.result = Arc::new(
            json!({"revision":s.revision,"phase":"ready","start_seconds":job.start as f64/job.rate as f64,"duration_seconds":(job.end-job.start)as f64/job.rate as f64,"rainbow_waveform":wave,"spectral":job.spectral,"times":job.times,"hop_seconds":job.hop as f64/job.rate as f64}),
        );
    } else {
        s.mix_preview.result = Arc::new(
            json!({"revision":s.revision,"phase":"rendering","progress":(job.cursor-job.start) as f64/(job.end-job.start) as f64}),
        );
        s.mix_preview.job = Some(job);
    }
    let mut out = (*s.mix_preview.result).clone();
    out["generation"] = json!(s.mix_preview.generation);
    Ok(out)
}

/// Resample on the *same* local quarter clock and physical keyboard as notes.
/// Frequency is continuous (MIDI semitones), independently of scale snapping.
pub(crate) fn project(
    preview: &Value,
    clip: Option<&Value>,
    view: super::piano_roll::Viewport,
) -> Value {
    let Some(clip) = clip else {
        return Value::Null;
    };
    if preview["phase"] != "ready" {
        return Value::Null;
    }
    let Some(times) = preview["times"].as_array() else {
        return Value::Null;
    };
    let Some(spectra) = preview["spectral"].as_array() else {
        return Value::Null;
    };
    if times.is_empty() {
        return Value::Null;
    }
    let start = clip["start_seconds"].as_f64().unwrap_or(0.);
    let spq = 60. / clip["bpm"].as_f64().unwrap_or(120.);
    let edo = clip["edo"].as_f64().unwrap_or(12.);
    let low = super::piano_key_layout::physical(view.lowest_pitch);
    let span = view.key_span();
    let pitches = (0..128)
        .map(|y| {
            let row =
                super::piano_key_layout::row(low + (1. - (y as f64 + 0.5) / 128.) * span) - 0.5;
            (60. + (row - 60.) * 12. / edo) * 2.
        })
        .collect::<Vec<_>>();
    let mut grid = vec![0f32; 128 * 128];
    for x in 0..128 {
        let t =
            start + (view.first_quarter + (x as f64 + 0.5) / 128. * view.quarters_per_width) * spq;
        let begin = preview["start_seconds"].as_f64().unwrap_or(0.);
        let end = begin + preview["duration_seconds"].as_f64().unwrap_or(0.);
        if t < begin || t >= end {
            continue;
        }
        let upper = times
            .partition_point(|v| v.as_f64().unwrap_or(0.) < t)
            .min(times.len() - 1);
        let lower = upper.saturating_sub(1);
        let a = times[lower].as_f64().unwrap();
        let b = times[upper].as_f64().unwrap();
        let f = if b > a {
            ((t - a) / (b - a)).clamp(0., 1.)
        } else {
            0.
        };
        for (y, p) in pitches.iter().enumerate() {
            if !(0. ..255.).contains(p) {
                continue;
            }
            let i = p.floor() as usize;
            let mix = p.fract();
            let at = |row: usize| {
                spectra[row][i].as_f64().unwrap_or(0.) * (1. - mix)
                    + spectra[row][i + 1].as_f64().unwrap_or(0.) * mix
            };
            grid[y * 128 + x] = (at(lower) * (1. - f) + at(upper) * f) as f32;
        }
    }
    json!(grid)
}

#[cfg(test)]
mod tests {
    use super::super::document::dispatch;
    use super::*;
    fn edit(s: &mut Document, hook: &str, mut v: Value) {
        v["document_revision"] = json!(s.revision);
        dispatch(s, hook, &v).unwrap();
    }
    fn fixture() -> Document {
        let mut s = Document::default();
        edit(
            &mut s,
            "PutTrack",
            json!({"id":"a","name":"Audio","kind":"audio"}),
        );
        let samples = (0..24000)
            .map(|i| (std::f64::consts::TAU * 440. * i as f64 / 48000.).sin() as f32 * 0.5)
            .collect::<Vec<_>>();
        edit(
            &mut s,
            "PutSource",
            json!({"id":"src","sample_rate":48000,"channels":1,"samples":samples}),
        );
        edit(
            &mut s,
            "PutClip",
            json!({"id":"c","track":"a","kind":"audio","source":"src","start_seconds":2.,"source_offset_seconds":0.,"duration_seconds":0.5}),
        );
        s
    }
    fn complete(s: &mut Document) -> Value {
        for _ in 0..1000 {
            let v = step(s, &json!({})).unwrap();
            if v["phase"] != "rendering" {
                return v;
            }
        }
        panic!("bounded fixture did not finish")
    }
    #[test]
    fn mix_preview_renders_actual_master_and_keeps_document_and_live_dsp_untouched() {
        let mut s = fixture();
        let revision = s.revision;
        let before = dispatch(&mut s, "Snapshot", &Value::Null).unwrap();
        let result = complete(&mut s);
        assert_eq!(result["phase"], "ready");
        assert_eq!(s.revision, revision);
        assert_eq!(dispatch(&mut s, "Snapshot", &Value::Null).unwrap(), before);
        let peak = result["rainbow_waveform"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p[0].as_f64().unwrap())
            .fold(0., f64::max);
        assert!(peak > 0.1);
        let row = &result["spectral"][3];
        let strongest = row
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.as_f64().unwrap().total_cmp(&b.1.as_f64().unwrap()))
            .unwrap()
            .0;
        assert!(
            (strongest as i32 - 138).abs() <= 1,
            "440Hz must sit at A4: {strongest}"
        );
        let cached = step(&mut s, &json!({"known_generation":result["generation"]})).unwrap();
        assert_eq!(cached["unchanged"], true);
        edit(&mut s, "SetBus", json!({"id":"mix-1","mute":true}));
        let muted = complete(&mut s);
        assert_ne!(muted["generation"], result["generation"]);
        assert!(
            muted["rainbow_waveform"]
                .as_array()
                .unwrap()
                .iter()
                .all(|p| p[0] == 0. && p[1] == 0.)
        );
        edit(&mut s, "Undo", json!({}));
        assert!(
            complete(&mut s)["rainbow_waveform"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p[0].as_f64().unwrap() > 0.1)
        );
    }
    #[test]
    fn mix_preview_projection_aligns_offset_tempo_edo_and_physical_keys() {
        let times = (0..21).map(|i| i as f64 * 0.1).collect::<Vec<_>>();
        let spectra = times
            .iter()
            .map(|t| {
                (0..256)
                    .map(|p| {
                        if (1. ..1.5).contains(t) && p == 138 {
                            1.
                        } else {
                            0.
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let preview = json!({"phase":"ready","start_seconds":0.,"duration_seconds":2.,"times":times,"spectral":spectra});
        let view = super::super::piano_roll::Viewport {
            first_quarter: 0.,
            quarters_per_width: 2.,
            lowest_pitch: 60.,
            pitches_per_height: 24.,
        };
        let c = json!({"start_seconds":1.,"bpm":120.,"edo":24});
        let grid = project(&preview, Some(&c), view);
        let rect = super::super::piano_roll::Rect {
            left: 0.,
            top: 0.,
            width: 128.,
            height: 128.,
        };
        // A4 is physical key 78 in 24 EDO referenced to C4.
        let p = view.project(0.5, 78., rect);
        let y = p[1].floor() as usize;
        assert!(grid[y * 128 + 32].as_f64().unwrap() > 0.2);
        assert_eq!(grid[y * 128 + 112], 0.);
        let shifted = json!({"start_seconds":3.,"bpm":120.,"edo":24});
        assert!(
            project(&preview, Some(&shifted), view)
                .as_array()
                .unwrap()
                .iter()
                .all(|v| *v == 0.)
        );
    }
    #[test]
    fn mix_preview_replaces_inflight_edits_and_handles_empty_documents() {
        let mut s = Document::default();
        assert_eq!(complete(&mut s)["phase"], "empty");
        let mut s = fixture();
        let first = step(&mut s, &json!({})).unwrap();
        edit(&mut s, "MoveClip", json!({"id":"c","start_seconds":5.}));
        let next = complete(&mut s);
        assert_eq!(next["start_seconds"], 5.);
        assert_ne!(first["generation"], next["generation"]);
    }
    #[test]
    fn mix_preview_prepares_warped_audio_without_mutating_live_cache() {
        let mut s = fixture();
        edit(&mut s, "SetClipWarp", json!({"id":"c","enabled":true}));
        edit(&mut s, "SetMusicalTime", json!({"bpm":80.}));
        let revision = s.revision;
        assert!(super::super::warp_treatment::ready_for_export(&s).is_err());
        let began = std::time::Instant::now();
        let result = loop {
            let v = step(&mut s, &json!({})).unwrap();
            if v["phase"] == "ready" || v["phase"] == "failed" {
                break v;
            }
            assert!(began.elapsed().as_secs() < 10, "{v}");
            std::thread::sleep(std::time::Duration::from_millis(1));
        };
        assert_eq!(result["phase"], "ready", "{result}");
        assert!(
            result["rainbow_waveform"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p[0].as_f64().unwrap() > 0.1)
        );
        assert_eq!(s.revision, revision);
        assert!(super::super::warp_treatment::ready_for_export(&s).is_err());
    }
    #[test]
    fn mix_preview_hidden_consumers_do_no_render_and_reuse_completed_cache() {
        let mut s = fixture();
        let idle = step(&mut s, &json!({"enabled":false})).unwrap();
        assert_eq!(idle["phase"], "idle");
        assert!(s.mix_preview.job.is_none());
        let ready = complete(&mut s);
        let generation = ready["generation"].clone();
        assert_eq!(
            step(
                &mut s,
                &json!({"enabled":false,"known_generation":generation})
            )
            .unwrap()["unchanged"],
            true
        );
        assert_eq!(
            step(
                &mut s,
                &json!({"enabled":true,"known_generation":generation})
            )
            .unwrap()["unchanged"],
            true
        );
        edit(&mut s, "SetBus", json!({"id":"mix-1","gain":0.5}));
        assert_eq!(
            step(&mut s, &json!({"enabled":false})).unwrap()["phase"],
            "idle"
        );
        assert!(s.mix_preview.job.is_none());
    }
}

#[cfg(test)] mod selection_tests {
 use super::*;
 use super::super::document::dispatch;
 fn edit(s:&mut Document,h:&str,mut v:Value){v["document_revision"]=json!(s.revision);dispatch(s,h,&v).unwrap();}
 fn complete(s:&mut Document,request:Value)->Value{for _ in 0..1000{let v=step(s,&request).unwrap();if !matches!(v["phase"].as_str(),Some("rendering"|"preparing")){return v;}}panic!("preview timed out")}
 fn peak(v:&Value)->f64{v["rainbow_waveform"].as_array().into_iter().flatten().map(|p|p[0].as_f64().unwrap()).fold(0.,f64::max)}
 #[test]fn audio_midi_both_track_selection_and_default_mute_are_independent(){
  let mut s=Document::default();
  for(id,kind)in[("a","audio"),("m","midi")]{edit(&mut s,"PutTrack",json!({"id":id,"name":id,"kind":kind}));edit(&mut s,"SetTrackMix",json!({"id":id,"output_bus_l":"L","output_bus_r":"R"}));}
  edit(&mut s,"PutSource",json!({"id":"src","sample_rate":8000,"channels":1,"samples":vec![0.1;800]}));edit(&mut s,"PutClip",json!({"id":"a1","kind":"audio","track":"a","source":"src","source_offset_seconds":0.,"duration_seconds":0.1,"start_seconds":0.}));
  edit(&mut s,"PutClip",json!({"id":"m1","kind":"midi","track":"m","start_seconds":0.,"bpm":120.,"length_quarters":1.,"notes":[{"id":"n","quarter":0.,"length_quarters":0.2,"pitch":69,"velocity":0.8}]}));
  let before=dispatch(&mut s,"Snapshot",&Value::Null).unwrap();let a=complete(&mut s,json!({"kind":"audio"}));let m=complete(&mut s,json!({"kind":"midi"}));let both=complete(&mut s,json!({"kind":"both"}));
  assert!(peak(&a)>0.05);assert!(peak(&m)>0.01);assert!(peak(&both)>peak(&a));assert_eq!(dispatch(&mut s,"Snapshot",&Value::Null).unwrap(),before);
  assert_eq!(complete(&mut s,json!({"kind":"audio","track":"m"}))["phase"],"empty");
  let direct=complete(&mut s,json!({"kind":"midi","track":"m"}));assert!(peak(&direct)>0.01);
  edit(&mut s,"SetTrackMix",json!({"id":"m","default_sound_muted":true}));
  let muted=complete(&mut s,json!({"kind":"midi"}));assert_eq!(peak(&muted),0.);assert!(peak(&complete(&mut s,json!({"kind":"audio"})))>0.05);
  let pcm=super::super::playback::render(&s,&json!({"sample_rate":8000,"start_frame":0,"frames":800,"instrument":"cardinal_sine"})).unwrap();assert!(!pcm["midi_events"].as_array().unwrap().is_empty());
  edit(&mut s,"Undo",json!({}));assert!(peak(&complete(&mut s,json!({"kind":"midi"})))>0.01);
 }
}
