//! Output components are document-owned and never occupy timeline lanes.
//! The four stem sums precede both hardware matrices and parallel monitor mixes.
use super::{document::State, render_clock::Clock};
use serde_json::{Value, json};
#[derive(Clone)]
pub(super) struct Bus {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub input: usize,
    pub destination: usize,
    pub output: String,
    pub gain: f32,
    pub mute: bool,
}
#[derive(Clone)]
pub(super) struct Buses {
    pub rows: Vec<Bus>,
}
const STEMS: [&str; 4] = ["L", "S", "FF", "R"];
pub(crate) fn physical_outputs() -> Value {
    json!([{"id":"flown_array","name":"Flown Array","channels":["L","S","FF","R"]}])
}
impl Default for Buses {
    fn default() -> Self {
        let mut rows: Vec<_> = STEMS
            .iter()
            .enumerate()
            .map(|(i, s)| Bus {
                id: format!("matrix-{s}"),
                name: format!("Matrix {s}"),
                kind: "matrix".into(),
                input: i,
                destination: i,
                output: "flown_array".into(),
                gain: 1.,
                mute: false,
            })
            .collect();
        rows.push(Bus {
            id: "mix-1".into(),
            name: "Mix Bus 1".into(),
            kind: "mix".into(),
            input: 0,
            destination: 0,
            output: "monitor".into(),
            gain: 1.,
            mute: false,
        });
        Self { rows }
    }
}
impl Buses {
    pub fn contains(&self, id: &str) -> bool {
        self.rows.iter().any(|b| b.id == id)
    }
    pub fn snapshot(&self) -> Value {
        json!(self.rows.iter().map(|b|json!({"id":b.id,"name":b.name,"kind":b.kind,"input":if b.kind=="mix"{"LSFFR"}else if b.kind=="channel"{"STEREO"}else{STEMS[b.input]},"output":b.output,"output_channel":STEMS[b.destination],"gain":b.gain,"mute":b.mute,"accepts_fx":b.kind!="matrix","timeline":false})).collect::<Vec<_>>())
    }
}
fn text(v: &Value, key: &str) -> Result<String, String> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(|| format!("{key} requires printable text"))
}
fn stem(v: &Value) -> Result<usize, String> {
    STEMS
        .iter()
        .position(|s| v == *s)
        .ok_or("matrix input/channel must be L, S, FF or R".into())
}
pub(super) fn dispatch(s: &mut State, hook: &str, v: &Value) -> Result<Value, String> {
    s.require_revision(v)?;
    if s.freeze.busy() {
        return Err("Finish or cancel track freeze first".into());
    }
    let id = text(v, "id")?;
    if hook == "RemoveBus" {
        let i = s
            .buses
            .rows
            .iter()
            .position(|b| b.id == id)
            .ok_or("unknown bus")?;
        // Keep dormant insert ownership for content Undo. Never process a deleted bus
        // or silently give its chain to a new component reusing the same identity.
        s.buses.rows.remove(i);
        return Ok(s.changed());
    }
    let old = s.buses.rows.iter().position(|b| b.id == id);
    let mut b = if hook == "PutBus" {
        if old.is_some() || s.tracks.contains_key(&id) || s.plugins.has_track(&id, false) {
            return Err("component identity already exists or owns retained inserts".into());
        }
        if s.buses.rows.len() >= 32 {
            return Err("bus capacity reached".into());
        }
        let kind = text(v, "kind")?;
        if !matches!(kind.as_str(), "matrix" | "mix" | "channel") {
            return Err("bus kind must be matrix, mix or channel".into());
        }
        Bus {
            id: id.clone(),
            name: text(v, "name")?,
            output: if kind == "channel" { "OFF" } else if kind == "matrix" {
                "flown_array"
            } else {
                "monitor"
            }
            .into(),
            kind,
            input: 2,
            destination: 2,
            gain: 1.,
            mute: false,
        }
    } else {
        s.buses.rows[old.ok_or("unknown bus")?].clone()
    };
    if hook=="PutBus" && b.kind=="channel" && v.get("output").is_none(){b.output=super::channels::default_output(s);}
    for key in v.as_object().ok_or("bus patch must be an object")?.keys() {
        if !matches!(
            key.as_str(),
            "document_revision"
                | "id"
                | "kind"
                | "name"
                | "input"
                | "output"
                | "output_channel"
                | "gain"
                | "mute"
                | "undo_label"
                | "merge_key"
        ) {
            return Err(format!("unsupported bus field {key}"));
        }
    }
    if let Some(kind) = v.get("kind") {
        if kind.as_str() != Some(b.kind.as_str()) {
            return Err("bus kind cannot change".into());
        }
    }
    if v.get("name").is_some() {
        b.name = text(v, "name")?;
    }
    if let Some(n) = v.get("gain") {
        b.gain = n
            .as_f64()
            .filter(|n| n.is_finite() && (0. ..=16.).contains(n))
            .ok_or("bus gain must be within 0..16")? as f32;
    }
    if let Some(n) = v.get("mute") {
        b.mute = n.as_bool().ok_or("bus mute must be boolean")?;
    }
    if let Some(n) = v.get("input") {
        if b.kind == "matrix" {
            b.input = stem(n)?;
        } else if n != if b.kind=="channel"{"STEREO"}else{"LSFFR"} {
            return Err("mix bus input is the LSFFR sum".into());
        }
    }
    if let Some(n) = v.get("output_channel") {
        if b.kind != "matrix" {
            return Err("only matrix buses select a hardware channel".into());
        }
        b.destination = stem(n)?;
    }
    if v.get("output").is_some() {
        let out = text(v, "output")?;
        if b.kind=="channel" {super::channels::validate_output(s,&id,&out,true)?;}
        else if out != "OFF"
            && out
                != if b.kind == "matrix" {
                    "flown_array"
                } else {
                    "monitor"
                }
        {
            return Err("unknown output destination".into());
        }
        b.output = out;
    }
    if let Some(i) = old {
        s.buses.rows[i] = b;
    } else {
        s.buses.rows.push(b);
    }
    Ok(s.changed())
}
/// Preparation worker only: native plugin calls never enter the audio callback.
/// Every mix reads the same original fold, then their outputs add without
/// normalization. Matrix feeds and exported stems never pass through this FX.
pub(super) fn render(
    s: &State,
    stems: &[[f32; 4]],
    direct: &std::collections::BTreeMap<String,Vec<f32>>,
    rate: u32,
    clock: &Clock,
    master_gain: f32,
    process_fx: bool,
) -> Result<(Vec<[f32; 4]>, Vec<[f32; 2]>, Value), String> {
    let mut hardware = vec![[0.; 4]; stems.len()];
    let mut monitor = vec![[0.; 2]; stems.len()];
    let mut monitor_stems=stems.to_vec();
    for b in s.buses.rows.iter().filter(|b|b.kind=="matrix"){if let Some(feed)=direct.get(&b.id){for (i,sample) in feed.iter().enumerate(){monitor_stems[i][b.input]+=sample;}}}
    let mut meters = Vec::new();
    for b in &s.buses.rows {
        if b.kind=="channel"{continue;}
        let mut peaks = [0f32; 2];
        if b.kind == "matrix" {
            for (i, frame) in stems.iter().enumerate() {
                let sample = if b.mute { 0. } else { (frame[b.input]+direct.get(&b.id).map_or(0.,|f|f[i])) * b.gain };
                peaks[0] = peaks[0].max(sample.abs());
                peaks[1] = peaks[0];
                if b.output == "flown_array" {
                    hardware[i][b.destination] += sample;
                }
            }
        } else {
            let mut pcm: Vec<_> = monitor_stems
                .iter()
                .map(|f| {
                    if b.mute {
                        [0.; 2]
                    } else {
                        super::routing::listen(*f)
                    }
                })
                .collect();
            if process_fx && s.plugins.has_track(&b.id, false) {
                for span in clock.spans() {
                    s.plugins.process_clocked(
                        &b.id,
                        &mut pcm[span.offset..span.offset + span.frames],
                        rate,
                        span.start,
                        [120., span.start as f64 / rate as f64 * 2., 4., 4.],
                        &[0.; 128],
                        span.sequence,
                    )?;
                }
            }
            for (i, frame) in pcm.iter().enumerate() {
                for ch in 0..2 {
                    let sample = if b.mute {
                        0.
                    } else {
                        frame[ch] * b.gain * master_gain
                    };
                    peaks[ch] = peaks[ch].max(sample.abs());
                    if b.output == "monitor" {
                        monitor[i][ch] += sample;
                    }
                }
            }
        }
        meters.push(json!({"id":b.id,"peaks":peaks}));
    }
    Ok((hardware, monitor, json!(meters)))
}

#[cfg(test)]
mod tests {
    use super::super::document::dispatch as call;
    use super::*;
    fn edit(s: &mut State, h: &str, mut v: Value) -> Value {
        v["document_revision"] = json!(s.revision);
        call(s, h, &v).unwrap()
    }
    fn pcm(s: &mut State) -> Value {
        edit(
            s,
            "RenderRange",
            json!({"sample_rate":8000,"start_frame":0,"frames":4,"publish_visualization":true}),
        )
    }
    fn fixture() -> State {
        let mut s = State::default();
        edit(
            &mut s,
            "PutTrack",
            json!({"id":"a","name":"Audio","kind":"audio"}),
        );
        edit(
            &mut s,
            "SetTrackMix",
            json!({"id":"a","output_bus_l":"L","output_bus_r":"R"}),
        );
        edit(
            &mut s,
            "PutSource",
            json!({"id":"src","sample_rate":8000,"channels":2,"samples":[1.,2.,0.,0.,0.,0.,0.,0.]}),
        );
        edit(
            &mut s,
            "PutClip",
            json!({"id":"clip","track":"a","kind":"audio","source":"src","start_seconds":0.,"source_offset_seconds":0.,"duration_seconds":0.0005}),
        );
        s
    }
    #[test]
    fn default_components_do_not_create_timeline_lanes() {
        let mut s = State::default();
        let v = call(&mut s, "Snapshot", &Value::Null).unwrap();
        assert_eq!(v["tracks"], json!([]));
        assert_eq!(v["clips"], json!([]));
        assert_eq!(v["buses"].as_array().unwrap().len(), 5);
        assert_eq!(v["buses"][0]["input"], "L");
        assert_eq!(v["buses"][3]["input"], "R");
        assert_eq!(v["physical_outputs"][0]["id"], "flown_array");
    }
    #[test]
    fn parallel_unity_mixes_double_monitor_and_never_hardware_or_stems() {
        let mut s = fixture();
        let before = pcm(&mut s);
        assert_eq!(before["master_stereo"][0], json!([1., 2.]));
        edit(
            &mut s,
            "PutBus",
            json!({"id":"parallel","name":"Parallel","kind":"mix"}),
        );
        let after = pcm(&mut s);
        assert_eq!(after["lsffr"], before["lsffr"]);
        assert_eq!(after["matrix_pcm"], before["matrix_pcm"]);
        assert_eq!(after["master_stereo"][0], json!([2., 4.]));
        edit(&mut s, "SetBus", json!({"id":"parallel","mute":true}));
        assert_eq!(pcm(&mut s)["master_stereo"], before["master_stereo"]);
        edit(&mut s, "RemoveBus", json!({"id":"mix-1"}));
        assert_eq!(pcm(&mut s)["master_stereo"], json!(vec![[0., 0.]; 4]));
        assert_eq!(pcm(&mut s)["matrix_pcm"], before["matrix_pcm"]);
    }
    #[test]
    fn matrix_repatch_and_delete_change_only_physical_feeds_and_undo_restores() {
        let mut s = fixture();
        let before = pcm(&mut s);
        edit(&mut s, "SetBus", json!({"id":"matrix-L","input":"R"}));
        let patched = pcm(&mut s);
        assert_eq!(patched["matrix_pcm"][0], json!([2., 0., 0., 2.]));
        assert_eq!(patched["lsffr"], before["lsffr"]);
        assert_eq!(patched["master_stereo"], before["master_stereo"]);
        edit(&mut s, "RemoveBus", json!({"id":"matrix-R"}));
        assert_eq!(pcm(&mut s)["matrix_pcm"][0], json!([2., 0., 0., 0.]));
        edit(&mut s, "Undo", json!({}));
        assert_eq!(pcm(&mut s)["matrix_pcm"], patched["matrix_pcm"]);
    }
    #[test]
    fn output_patch_and_disconnect_preserve_stems_and_monitor_and_support_undo() {
        let mut s=fixture();
        let before=pcm(&mut s);
        edit(&mut s,"SetBus",json!({"id":"matrix-L","output":"flown_array","output_channel":"FF"}));
        let connected=pcm(&mut s);
        assert_eq!(connected["matrix_pcm"][0],json!([0.,0.,1.,2.]));
        assert_eq!(connected["lsffr"],before["lsffr"]);
        assert_eq!(connected["master_stereo"],before["master_stereo"]);
        edit(&mut s,"SetBus",json!({"id":"matrix-L","output":"OFF"}));
        assert_eq!(pcm(&mut s)["matrix_pcm"][0],json!([0.,0.,0.,2.]));
        edit(&mut s,"Undo",json!({}));
        assert_eq!(pcm(&mut s)["matrix_pcm"],connected["matrix_pcm"]);
    }
    #[test]
    fn rejected_routes_and_stale_requests_do_not_mutate() {
        let mut s = fixture();
        let original = s.buses.snapshot();
        for patch in [
            json!({"id":"matrix-L","input":"NO","gain":2.}),
            json!({"id":"matrix-L","output":"imaginary"}),
            json!({"id":"mix-1","input":"L"}),
            json!({"id":"matrix-L","gain":-1.}),
            json!({"id":"matrix-L","mute":5}),
        ] {
            let mut v = patch;
            v["document_revision"] = json!(s.revision);
            assert!(call(&mut s, "SetBus", &v).is_err());
            assert_eq!(s.buses.snapshot(), original);
        }
        assert!(
            call(
                &mut s,
                "RemoveBus",
                &json!({"document_revision":0,"id":"matrix-L"})
            )
            .is_err()
        );
    }
    #[test]
    fn mix_effect_tail_does_not_enter_stems_or_array_and_survives_natural_wrap() {
        let mut s = fixture();
        s.plugins.test_install_tail("mix-1", false);
        let out = edit(
            &mut s,
            "RenderRange",
            json!({"sample_rate":8000,"start_frame":0,"frames":4,"playback_clock":{"generation":1,"sequence":0}}),
        );
        assert_eq!(
            out["master_stereo"],
            json!([[1., 2.], [0.5, 1.], [0.25, 0.5], [0.125, 0.25]])
        );
        assert_eq!(out["lsffr"][1], json!([0., 0., 0., 0.]));
        assert_eq!(out["matrix_pcm"], out["lsffr"]);
        let out = edit(
            &mut s,
            "RenderRange",
            json!({"sample_rate":8000,"start_frame":0,"frames":2,"playback_clock":{"generation":1,"sequence":4},"loop_range":{"start_frame":0,"end_frame":1}}),
        );
        assert_eq!(
            out["master_stereo"],
            json!([[1.0625, 2.125], [1.53125, 3.0625]])
        );
        assert_eq!(s.plugins.test_tail_counters().0, 1);
    }
    #[test]
    fn stem_export_ignores_live_mix_plugins_and_bus_deletion() {
        let mut s = fixture();
        let before = pcm(&mut s)["lsffr"].clone();
        s.plugins.test_install_tail("mix-1", false);
        edit(&mut s, "RemoveBus", json!({"id":"matrix-L"}));
        let frozen = s.freeze_for_export();
        let out = super::super::playback::render(
            &frozen,
            &json!({"sample_rate":8000,"start_frame":0,"frames":4,"stem_export":true}),
        )
        .unwrap();
        assert_eq!(out["lsffr"], before);
        assert_eq!(frozen.buses.snapshot(), s.buses.snapshot());
        assert_eq!(s.plugins.test_tail_counters(), (0, 0));
    }
}
