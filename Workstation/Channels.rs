//! Control-side stereo routing graph. Channel buses own effects, never clips.
use super::{
    document::{Clip, Content, State, Track},
    render_clock::Clock,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
pub(super) fn default_output(s: &State) -> String {
    s.buses
        .rows
        .iter()
        .find(|b| b.kind == "matrix" && b.input == 2)
        .or_else(|| s.buses.rows.iter().find(|b| b.kind == "matrix"))
        .map_or("OFF".into(), |b| b.id.clone())
}
pub(super) fn validate_clip(s: &State, c: &Clip) -> Result<(), String> {
    if c.track.is_empty() {
        if !matches!(c.content, Content::Audio { .. }) {
            return Err("Only audio clips can be trackless".into());
        }
        validate_output(s, "", &c.output, false)
    } else if s.tracks.get(&c.track).is_none_or(|t| match &c.content {
        Content::Audio { .. } => t.kind != "audio",
        Content::Midi { .. } => t.kind != "midi",
        Content::Cc { .. } => t.kind != "cc",
    }) {
        Err("clip kind must match its track".into())
    } else {
        Ok(())
    }
}
pub(super) fn validate_output(
    s: &State,
    owner: &str,
    out: &str,
    allow_track: bool,
) -> Result<(), String> {
    if out == "OFF" || out.is_empty() {
        return Ok(());
    }
    let valid = s
        .buses
        .rows
        .iter()
        .any(|b| b.id == out && matches!(b.kind.as_str(), "matrix" | "channel"))
        || (allow_track && s.tracks.get(out).is_some_and(|t| t.kind != "cc"));
    if !valid {
        return Err("Choose an available matrix, channel bus or audio/MIDI track".into());
    }
    let mut seen = BTreeSet::new();
    seen.insert(owner);
    let mut at = out;
    loop {
        if !seen.insert(at) {
            return Err("Audio routing would create feedback".into());
        }
        if let Some(t) = s.tracks.get(at) {
            at = &t.output;
        } else if let Some(b) = s
            .buses
            .rows
            .iter()
            .find(|b| b.id == at && b.kind == "channel")
        {
            at = &b.output;
        } else {
            break;
        }
        if at.is_empty() || at == "OFF" {
            break;
        }
    }
    Ok(())
}
/// Returns post-FX, pre-fader track buffers plus the final stem sums. Every
/// source feeds its selected destination once; incoming channels enter before FX.
pub(super) fn render<F>(
    s: &State,
    tracks: &[&Track],
    mut pcm: Vec<Vec<[f32; 2]>>,
    gains: &[Vec<f32>],
    rate: u32,
    clock: &Clock,
    monitor_only: bool,
    mut fx: F,
) -> Result<
    (
        Vec<Vec<[f32; 2]>>,
        Vec<[f32; 4]>,
        BTreeMap<String, Vec<f32>>,
        Value,
    ),
    String,
>
where
    F: FnMut(usize, &mut [[f32; 2]]) -> Result<(), String>,
{
    let frames = pcm
        .first()
        .map_or_else(|| clock.spans().iter().map(|x| x.frames).sum(), Vec::len);
    let channels: Vec<_> = s
        .buses
        .rows
        .iter()
        .filter(|b| b.kind == "channel")
        .collect();
    let mut ids: BTreeMap<String, usize> = tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| t.kind != "cc")
        .map(|(i, t)| (t.id.clone(), i))
        .collect();
    for (i, b) in channels.iter().enumerate() {
        ids.insert(b.id.clone(), tracks.len() + i);
        pcm.push(vec![[0.; 2]; frames]);
    }
    let outputs: Vec<_> = tracks
        .iter()
        .map(|t| t.output.as_str())
        .chain(channels.iter().map(|b| b.output.as_str()))
        .collect();
    let mut indegree = vec![0; pcm.len()];
    for out in &outputs {
        if let Some(&j) = ids.get(*out) {
            indegree[j] += 1;
        }
    }
    let mut stems = vec![[0.; 4]; frames];
    let mut direct: BTreeMap<String, Vec<f32>> = s
        .buses
        .rows
        .iter()
        .filter(|b| b.kind == "matrix")
        .map(|b| (b.id.clone(), vec![0.; frames]))
        .collect();
    let route = |out: &str,
                 sample: [f32; 2],
                 frame: usize,
                 pcm: &mut Vec<Vec<[f32; 2]>>,
                 direct: &mut BTreeMap<String, Vec<f32>>| {
        if let Some(&j) = ids.get(out) {
            for ch in 0..2 {
                pcm[j][frame][ch] += sample[ch];
            }
        } else if let Some(feed) = direct.get_mut(out) {
            feed[frame] += sample[0] + sample[1];
        }
    };
    if !monitor_only {
        for id in &s.clip_order {
            let c = &s.clips[id];
            if !c.track.is_empty() {
                continue;
            }
            let Content::Audio { source, offset, .. } = &c.content else {
                continue;
            };
            let (range_start, range_end) = clock.bounds();
            let first = (c.start * rate as f64).round() as u64;
            let last = first.saturating_add((c.duration() * rate as f64).round().max(1.) as u64);
            if last <= range_start || first >= range_end {
                continue;
            }
            let Some((source, offset)) =
                super::warp_treatment::render_source(s, &c.id, source, *offset)?
            else {
                continue;
            };
            if source.sample_rate != rate {
                return Err("source sample rate must match render rate".into());
            }
            let begin = (c.start * rate as f64).round() as u64;
            let length = (c.duration() * rate as f64).round().max(1.) as u64;
            let offset = (offset * rate as f64).round() as usize;
            let available = (source.samples.len() / source.channels).saturating_sub(offset) as u64;
            for i in 0..frames {
                let at = clock.at(i);
                if at >= begin && at - begin < length && at - begin < available {
                    let sample = source.stereo(offset + (at - begin) as usize).map(|v| {
                        v * c.gain as f32
                            * super::clip_fades::gain(
                                at - begin,
                                length,
                                c.fade_in,
                                c.fade_out,
                                rate,
                            )
                    });
                    route(&c.output, sample, i, &mut pcm, &mut direct);
                }
            }
        }
    }
    let mut done = vec![false; pcm.len()];
    let mut meters = Vec::new();
    for _ in 0..pcm.len() {
        let Some(j) = (0..pcm.len()).find(|&j| !done[j] && indegree[j] == 0) else {
            return Err("Audio routing contains feedback".into());
        };
        done[j] = true;
        if j < tracks.len() {
            fx(j, &mut pcm[j])?;
        } else {
            let b = channels[j - tracks.len()];
            if s.plugins.has_track(&b.id, false) {
                super::plugin_render::process_clocked(
                    s,
                    &b.id,
                    &mut pcm[j],
                    rate,
                    clock,
                    b.mute,
                    None,
                    !monitor_only,
                )?;
            }
            if b.mute {
                pcm[j].fill([0.; 2]);
            }
        }
        let mut peaks = [0f32; 2];
        for i in 0..frames {
            let gain = if j < tracks.len() {
                gains[j][i]
            } else {
                channels[j - tracks.len()].gain
            };
            let sample = pcm[j][i].map(|v| v * gain);
            for ch in 0..2 {
                peaks[ch] = peaks[ch].max(sample[ch].abs());
            }
            if outputs[j].is_empty() && j < tracks.len() {
                tracks[j].route.add(sample, &mut stems[i]);
            } else {
                route(outputs[j], sample, i, &mut pcm, &mut direct);
            }
        }
        if j >= tracks.len() {
            meters.push(json!({"id":channels[j-tracks.len()].id,"peaks":peaks}));
        }
        if let Some(&next) = ids.get(outputs[j]) {
            indegree[next] -= 1;
        }
    }
    pcm.truncate(tracks.len());
    Ok((pcm, stems, direct, json!(meters)))
}

#[cfg(test)]
mod tests {
    use super::super::document::dispatch;
    use super::*;
    fn edit(s: &mut State, hook: &str, mut v: Value) -> Value {
        v["document_revision"] = json!(s.revision);
        dispatch(s, hook, &v).unwrap()
    }
    fn source(s: &mut State) {
        edit(
            s,
            "PutSource",
            json!({"id":"src","sample_rate":8000,"channels":2,"samples":[1.,2.,0.,0.,0.,0.,0.,0.]}),
        );
    }
    fn clip(s: &mut State, id: &str, track: &str, output: &str) {
        edit(
            s,
            "PutClip",
            json!({"id":id,"track":track,"output":output,"kind":"audio","source":"src","start_seconds":0.,"source_offset_seconds":0.,"duration_seconds":4./8000.}),
        );
    }
    fn track(s: &mut State, id: &str) {
        edit(s, "PutTrack", json!({"id":id,"kind":"audio","name":id}));
    }
    fn channel(s: &mut State, id: &str, out: &str) {
        edit(
            s,
            "PutBus",
            json!({"id":id,"kind":"channel","name":id,"output":out}),
        );
    }
    fn render(s: &mut State) -> Value {
        edit(
            s,
            "RenderRange",
            json!({"sample_rate":8000,"start_frame":0,"frames":4,"publish_visualization":true}),
        )
    }
    #[test]
    fn trackless_clips_sum_without_creating_tracks_and_keep_edits_undoable() {
        let mut s = State::default();
        source(&mut s);
        clip(&mut s, "one", "", "matrix-FF");
        clip(&mut s, "two", "", "matrix-FF");
        assert!(s.tracks.is_empty());
        assert_eq!(render(&mut s)["lsffr"][0], json!([0., 0., 6., 0.]));
        edit(
            &mut s,
            "SetClipRouting",
            json!({"id":"two","output":"matrix-L"}),
        );
        assert_eq!(render(&mut s)["lsffr"][0], json!([3., 0., 3., 0.]));
        edit(&mut s, "Undo", json!({}));
        assert_eq!(render(&mut s)["lsffr"][0], json!([0., 0., 6., 0.]));
        edit(
            &mut s,
            "MoveClip",
            json!({"id":"one","start_seconds":1.,"lane":3}),
        );
        assert_eq!(s.clips["one"].lane, 3.);
        assert_eq!(s.clips["one"].output, "matrix-FF");
    }
    #[test]
    fn fractional_free_position_undo_redo_preserves_audio_and_routing() {
        let mut s=State::default();source(&mut s);clip(&mut s,"one","","matrix-FF");
        let before=render(&mut s)["lsffr"].clone();
        edit(&mut s,"MoveClip",json!({"id":"one","start_seconds":0.,"lane":2.375}));
        assert_eq!(s.clips["one"].lane,2.375);assert!(s.tracks.is_empty());assert_eq!(render(&mut s)["lsffr"],before);
        edit(&mut s,"Undo",json!({}));assert_eq!(s.clips["one"].lane,0.);
        edit(&mut s,"Redo",json!({}));assert_eq!(s.clips["one"].snapshot()["lane"],2.375);assert_eq!(s.clips["one"].output,"matrix-FF");
        let revision=s.revision;let result=super::super::document::dispatch(&mut s,"MoveClip",&json!({"document_revision":revision,"id":"one","start_seconds":0.,"lane":-1.}));assert!(result.is_err());assert_eq!(s.clips["one"].lane,2.375);
    }
    #[test]
    fn channels_chain_stereo_through_tracks_before_output() {
        let mut s = State::default();
        source(&mut s);
        track(&mut s, "source");
        track(&mut s, "return");
        channel(&mut s, "second", "return");
        channel(&mut s, "first", "second");
        edit(&mut s, "SetBus", json!({"id":"first","gain":2.}));
        edit(&mut s, "SetBus", json!({"id":"second","gain":0.5}));
        edit(
            &mut s,
            "SetTrackMix",
            json!({"id":"source","output":"first"}),
        );
        edit(
            &mut s,
            "SetTrackMix",
            json!({"id":"return","output_bus_l":"L","output_bus_r":"R"}),
        );
        clip(&mut s, "one", "source", "OFF");
        clip(&mut s, "two", "", "first");
        assert_eq!(render(&mut s)["lsffr"][0], json!([2., 0., 0., 4.]));
        assert_eq!(s.track_order, vec!["source", "return"]);
        let before = s.revision;
        assert!(
            dispatch(
                &mut s,
                "SetBus",
                &json!({"document_revision":before,"id":"second","output":"source"})
            )
            .is_err()
        );
        assert_eq!(s.revision, before);
    }
    #[test]
    fn channel_fx_and_mute_apply_to_trackless_clips() {
        let mut s = State::default();
        source(&mut s);
        channel(&mut s, "fx", "matrix-FF");
        clip(&mut s, "one", "", "fx");
        s.plugins.test_install_tail("fx", false);
        let out = render(&mut s);
        assert_eq!(
            out["lsffr"],
            json!([
                [0., 0., 3., 0.],
                [0., 0., 1.5, 0.],
                [0., 0., 0.75, 0.],
                [0., 0., 0.375, 0.]
            ])
        );
        edit(&mut s, "SetBus", json!({"id":"fx","mute":true}));
        assert_eq!(render(&mut s)["lsffr"], json!(vec![[0.; 4]; 4]));
    }
    #[test]
    fn assigned_clip_routing_rejected_and_detach_preserves_source() {
        let mut s = State::default();
        source(&mut s);
        track(&mut s, "a");
        clip(&mut s, "one", "a", "OFF");
        let revision = s.revision;
        assert!(
            dispatch(
                &mut s,
                "SetClipRouting",
                &json!({"document_revision":revision,"id":"one","output":"matrix-L"})
            )
            .is_err()
        );
        edit(
            &mut s,
            "MoveClip",
            json!({"id":"one","track":"","start_seconds":0.,"output":"matrix-R","lane":2}),
        );
        assert_eq!(render(&mut s)["lsffr"][0], json!([0., 0., 0., 3.]));
        edit(
            &mut s,
            "MoveClip",
            json!({"id":"one","track":"a","start_seconds":0.}),
        );
        assert_eq!(render(&mut s)["lsffr"][0], json!([0., 0., 3., 0.]));
    }
    #[test]
    fn deleted_destination_goes_silent_until_undo_and_stale_routes_do_not_commit() {
        let mut s = State::default();
        source(&mut s);
        channel(&mut s, "fx", "matrix-L");
        clip(&mut s, "one", "", "fx");
        edit(&mut s, "RemoveBus", json!({"id":"fx"}));
        assert_eq!(render(&mut s)["lsffr"], json!(vec![[0.; 4]; 4]));
        edit(&mut s, "Undo", json!({}));
        assert_eq!(render(&mut s)["lsffr"][0], json!([3., 0., 0., 0.]));
        assert!(
            dispatch(
                &mut s,
                "SetClipRouting",
                &json!({"document_revision":0,"id":"one","output":"OFF"})
            )
            .is_err()
        );
    }
    #[test]
    fn direct_matrix_identity_does_not_fan_out_to_another_matrix_input() {
        let mut s = State::default();
        source(&mut s);
        edit(
            &mut s,
            "PutBus",
            json!({"id":"duplicate","name":"Duplicate input","kind":"matrix","input":"L","output_channel":"R"}),
        );
        clip(&mut s, "one", "", "matrix-L");
        let out = render(&mut s);
        assert_eq!(out["matrix_pcm"][0], json!([3., 0., 0., 0.]));
        assert_eq!(out["master_stereo"][0], json!([3., 0.]));
    }
    #[test]
    fn splitting_trackless_audio_keeps_lane_destination_and_gain() {
        let mut s = State::default();
        source(&mut s);
        clip(&mut s, "one", "", "matrix-L");
        edit(
            &mut s,
            "MoveClip",
            json!({"id":"one","start_seconds":0.,"lane":3}),
        );
        edit(&mut s, "SetClipGain", json!({"id":"one","gain":0.5}));
        let result = edit(
            &mut s,
            "SplitClip",
            json!({"id":"one","time_seconds":2./8000.}),
        );
        let right = &s.clips[result["right_clip"].as_str().unwrap()];
        assert!(right.track.is_empty());
        assert_eq!(right.lane, 3.);
        assert_eq!(right.output, "matrix-L");
        assert_eq!(right.gain, 0.5);
        assert_eq!(render(&mut s)["lsffr"][0], json!([1.5, 0., 0., 0.]));
    }

    #[test]
    fn short_sources_leave_silent_tail_in_trackless_clips() {
        let mut s = State::default();
        source(&mut s);
        edit(
            &mut s,
            "PutClip",
            json!({"id":"long","kind":"audio","output":"matrix-L","source":"src","start_seconds":0.,"source_offset_seconds":2./8000.,"duration_seconds":8./8000.}),
        );
        assert_eq!(render(&mut s)["lsffr"], json!(vec![[0.; 4]; 4]));
    }
}
