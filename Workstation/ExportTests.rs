use super::document::{State, dispatch};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "mirror-export-test-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn output(&self) -> PathBuf {
        self.0.join("export")
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn edit(s: &mut State, hook: &str, mut v: Value) {
    v["document_revision"] = dispatch(s, "Snapshot", &Value::Null).unwrap()["revision"].clone();
    dispatch(s, hook, &v).unwrap();
}
fn document() -> State {
    let mut s = State::default();
    edit(
        &mut s,
        "PutTrack",
        json!({"id":"audio","name":"Audio","kind":"audio"}),
    );
    edit(
        &mut s,
        "SetTrackMix",
        json!({"id":"audio","output_bus_l":"L","output_bus_r":"FF"}),
    );
    let samples: Vec<f32> = (0..8200)
        .flat_map(|i| [if i % 2 == 0 { 2. } else { -3. }, 0.25])
        .collect();
    edit(
        &mut s,
        "PutSource",
        json!({"id":"pcm","sample_rate":8000,"channels":2,"samples":samples}),
    );
    edit(
        &mut s,
        "PutClip",
        json!({"id":"clip","track":"audio","kind":"audio","source":"pcm",
        "source_offset_seconds":0.,"start_seconds":0.,"duration_seconds":1.025}),
    );
    s
}
fn begin(s: &mut State, t: &Temp) -> Result<Value, String> {
    let revision = dispatch(s, "Snapshot", &Value::Null).unwrap()["revision"].clone();
    dispatch(
        s,
        "ExportBegin",
        &json!({"document_revision":revision,"sample_rate":8000,
        "start_seconds":0.000125,"end_seconds":1.025,"destination":t.output()}),
    )
}
fn finish(s: &mut State, report: &Value) -> Value {
    let token = json!({"job_id":report["job_id"]});
    for _ in 0..8 {
        let status = dispatch(s, "ExportStep", &token).unwrap();
        if status["phase"] != "rendering" {
            return status;
        }
    }
    panic!("small export failed to complete in bounded steps")
}
fn pcm(path: &Value) -> Vec<f32> {
    let b = fs::read(path.as_str().unwrap()).unwrap();
    assert_eq!(&b[..4], b"RIFF");
    assert_eq!(&b[8..12], b"WAVE");
    assert_eq!(
        u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize,
        b.len() - 8
    );
    let mut p = 12;
    let mut format = false;
    while p + 8 <= b.len() {
        let n = u32::from_le_bytes(b[p + 4..p + 8].try_into().unwrap()) as usize;
        if &b[p..p + 4] == b"fmt " {
            assert_eq!(u16::from_le_bytes(b[p + 8..p + 10].try_into().unwrap()), 3);
            assert_eq!(u16::from_le_bytes(b[p + 10..p + 12].try_into().unwrap()), 1);
            assert_eq!(
                u32::from_le_bytes(b[p + 12..p + 16].try_into().unwrap()),
                8000
            );
            format = true;
        }
        if &b[p..p + 4] == b"data" {
            assert!(format);
            assert_eq!(p + 8 + n, b.len());
            return b[p + 8..]
                .chunks_exact(4)
                .map(|s| f32::from_le_bytes(s.try_into().unwrap()))
                .collect();
        }
        p += 8 + n + (n % 2);
    }
    panic!("no WAV data")
}
#[test]
fn export_streams_four_pre_master_float_wavs_with_exact_exclusive_range() {
    let t = Temp::new();
    let mut s = document();
    let before = dispatch(&mut s, "Snapshot", &Value::Null).unwrap();
    let start = begin(&mut s, &t).unwrap();
    let step = dispatch(&mut s, "ExportStep", &json!({"job_id":start["job_id"]})).unwrap();
    assert_eq!(step["phase"], "rendering");
    assert_eq!(step["frames_written"], 4096);
    let done = finish(&mut s, &start);
    assert_eq!(done["phase"], "completed");
    assert_eq!(done["progress"], 1.);
    assert_eq!(done["bus_order"], json!(["L", "S", "FF", "R"]));
    assert_eq!(done["export_tap"], "pre_stereo_master");
    let buses: Vec<_> = done["files"].as_array().unwrap().iter().map(pcm).collect();
    assert!(buses.iter().all(|v| v.len() == 8199));
    assert_eq!(&buses[0][..3], &[-3., 2., -3.]);
    assert!(buses[1].iter().all(|v| *v == 0.));
    assert!(buses[2].iter().all(|v| *v == 0.25));
    assert!(buses[3].iter().all(|v| *v == 0.));
    assert_eq!(dispatch(&mut s, "Snapshot", &Value::Null).unwrap(), before);
    assert_eq!(fs::read_dir(t.output()).unwrap().count(), 4);
}
#[test]
fn export_freezes_sources_and_mix_across_live_edits() {
    let t = Temp::new();
    let mut s = document();
    let start = begin(&mut s, &t).unwrap();
    edit(&mut s, "SetTrackMix", json!({"id":"audio","gain":0.}));
    edit(
        &mut s,
        "PutSource",
        json!({"id":"pcm","sample_rate":8000,"channels":1,"samples":[0.]}),
    );
    edit(&mut s, "RemoveClip", json!({"id":"clip"}));
    let done = finish(&mut s, &start);
    assert_eq!(done["phase"], "completed");
    assert_eq!(pcm(&done["files"][0])[0], -3.);
    assert_ne!(
        done["document_revision"],
        dispatch(&mut s, "Snapshot", &Value::Null).unwrap()["revision"]
    );
}
#[test]
fn cancel_removes_owned_partial_output_and_rejects_stale_tokens() {
    let t = Temp::new();
    let mut s = document();
    let start = begin(&mut s, &t).unwrap();
    assert!(dispatch(&mut s, "ExportStep", &json!({"job_id":999})).is_err());
    assert!(begin(&mut s, &t).is_err());
    dispatch(&mut s, "ExportStep", &json!({"job_id":start["job_id"]})).unwrap();
    let cancelled = dispatch(&mut s, "ExportCancel", &json!({"job_id":start["job_id"]})).unwrap();
    assert_eq!(cancelled["phase"], "cancelled");
    assert!(!t.output().exists());
    let next = begin(&mut s, &t).unwrap();
    assert_ne!(next["job_id"], start["job_id"]);
    assert!(dispatch(&mut s, "ExportCancel", &json!({"job_id":start["job_id"]})).is_err());
    dispatch(&mut s, "ExportCancel", &json!({"job_id":next["job_id"]})).unwrap();
}
#[test]
fn midi_uses_the_live_builtin_instrument_and_renders_release_across_blocks() {
    let t = Temp::new();
    let mut s = document();
    edit(
        &mut s,
        "PutTrack",
        json!({"id":"midi","name":"Piano","kind":"midi"}),
    );
    edit(
        &mut s,
        "PutClip",
        json!({"id":"roll","track":"midi","kind":"midi","start_seconds":0.,"bpm":120.,"length_quarters":4.,
        "notes":[{"id":"n","quarter":0.,"length_quarters":1.,"pitch":69,"velocity":1.}]}),
    );
    let job = begin(&mut s, &t).unwrap();
    let done = finish(&mut s, &job);
    assert_eq!(done["phase"], "completed");
    assert_eq!(done["instrument"], "cardinal_sine");
    let ff = pcm(&done["files"][2]);
    // The default FF/FF route sums both instrument channels. An independent
    // sine/ADSR calculation checks samples on both sides of a render boundary.
    for frame in [1usize, 80, 1000, 4096, 4097, 4959, 4961] {
        let time = frame as f64 / 8000.;
        let envelope = if time < 0.005 {
            time / 0.005
        } else if time < 0.045 {
            1. - 0.65 * (time - 0.005) / 0.04
        } else if time < 0.5 {
            0.35
        } else {
            0.35 * (1. - (time - 0.5) / 0.12).clamp(0., 1.)
        };
        let expected = 0.25 + 2. * 0.16 * envelope * (std::f64::consts::TAU * 440. * time).sin();
        assert!((ff[frame - 1] as f64 - expected).abs() < 1e-6);
    }
    let t = Temp::new();
    edit(&mut s, "SetTrackMix", json!({"id":"midi","mute":true}));
    let job = begin(&mut s, &t).unwrap();
    let muted = finish(&mut s, &job);
    assert!(pcm(&muted["files"][2]).iter().all(|v| *v == 0.25));
    let t = Temp::new();
    edit(&mut s, "SetTrackMix", json!({"id":"midi","mute":false}));
    edit(&mut s, "MoveClip", json!({"id":"roll","start_seconds":10.}));
    let job = begin(&mut s, &t).unwrap();
    let outside = finish(&mut s, &job);
    assert!(pcm(&outside["files"][2]).iter().all(|v| *v == 0.25));
}
#[test]
fn missing_later_sources_fail_before_creating_any_files() {
    let t = Temp::new();
    let mut s = document();
    edit(
        &mut s,
        "PutClip",
        json!({"id":"late","track":"audio","kind":"audio","source":"absent",
        "source_offset_seconds":0.,"start_seconds":0.8,"duration_seconds":0.1}),
    );
    assert!(begin(&mut s, &t).unwrap_err().contains("missing decoded"));
    assert!(!t.output().exists());
}
#[test]
fn export_applies_frozen_cc_gain_before_the_four_bus_tap() {
    let t = Temp::new();
    let mut s = document();
    edit(
        &mut s,
        "PutTrack",
        json!({"id":"cc","name":"Gain","kind":"cc"}),
    );
    edit(
        &mut s,
        "SetCcTarget",
        json!({"id":"cc","target":{"track":"audio","parameter":"gain"}}),
    );
    edit(
        &mut s,
        "PutClip",
        json!({"id":"curve","track":"cc","kind":"cc",
        "start_seconds":0.,"duration_seconds":1.025,"points":[{"offset_seconds":0.,"value":1.}]}),
    );
    edit(&mut s, "SetTrackMix", json!({"id":"audio","gain":0.}));
    let job = begin(&mut s, &t).unwrap();
    edit(&mut s, "SetCcTarget", json!({"id":"cc","target":null}));
    let done = finish(&mut s, &job);
    assert_eq!(done["phase"], "completed");
    let gain = 10f32.powf(5. / 20.);
    assert!((pcm(&done["files"][0])[0] + 3. * gain).abs() < 1e-6);
    assert!((pcm(&done["files"][2])[4096] - 0.25 * gain).abs() < 1e-6);
}
#[test]
fn rejected_range_names_rate_and_revision_preserve_document_and_destination() {
    let t = Temp::new();
    let mut s = document();
    let before = dispatch(&mut s, "Snapshot", &Value::Null).unwrap();
    let base = json!({"document_revision":before["revision"],"sample_rate":8000,"start_seconds":0.,"end_seconds":1.,"destination":t.output()});
    for (key, value) in [
        ("end_seconds", json!(0.)),
        ("sample_rate", json!(16000)),
        ("document_revision", json!(0)),
        ("stem_names", json!(["same", "SAME", "FF", "R"])),
    ] {
        let mut request = base.clone();
        request[key] = value;
        assert!(dispatch(&mut s, "ExportBegin", &request).is_err());
        assert!(!t.output().exists());
    }
    assert_eq!(dispatch(&mut s, "Snapshot", &Value::Null).unwrap(), before);
}

#[test]
fn drum_rack_export_writes_frozen_sample_pcm_and_excludes_preview(){
 let temp=Temp::new();let mut s=State::default();
 edit(&mut s,"PutSource",json!({"id":"hit","sample_rate":8000,"channels":1,"samples":[1.,0.5,0.25,0.]}));
 edit(&mut s,"DrumRack",json!({"action":"add","track":"rack","name":"Hit","source":"hit"}));
 edit(&mut s,"DrumRack",json!({"action":"steps","channel":0,"steps":1}));
 edit(&mut s,"DrumRack",json!({"action":"place","channel":0,"id":"pattern","start_seconds":0.}));
 edit(&mut s,"DrumRack",json!({"action":"preview","enabled":true}));
 let revision=s.revision;
 let start=dispatch(&mut s,"ExportBegin",&json!({"document_revision":revision,"sample_rate":8000,"start_seconds":0.,"end_seconds":0.0005,"destination":temp.output()})).unwrap();
 edit(&mut s,"PutSource",json!({"id":"hit","sample_rate":8000,"channels":1,"samples":[0.]}));
 edit(&mut s,"DrumRack",json!({"action":"mix","channel":0,"volume":0.}));
 let done=finish(&mut s,&start);assert_eq!(done["phase"],"completed");let samples=pcm(&done["files"][2]);assert_eq!(samples.len(),4);assert!((samples[0]-1.64).abs()<1e-6);assert!((samples[1]-0.82).abs()<1e-6);
 let temp2=Temp::new();edit(&mut s,"DrumRack",json!({"action":"mix","channel":0,"volume":1.}));edit(&mut s,"PutSource",json!({"id":"hit","sample_rate":8000,"channels":1,"samples":[1.]}));
 let revision=s.revision;let start=dispatch(&mut s,"ExportBegin",&json!({"document_revision":revision,"sample_rate":8000,"start_seconds":0.125,"end_seconds":0.1255,"destination":temp2.output()})).unwrap();let done=finish(&mut s,&start);assert_eq!(pcm(&done["files"][2]),vec![0.;4]);
}

#[test]
fn clip_fades_are_frozen_and_written_to_export_wavs(){
 let t=Temp::new();let mut s=document();
 edit(&mut s,"SetClipFades",json!({"id":"clip","fade_in_seconds":0.002,"fade_out_seconds":0.002}));
 let start=begin(&mut s,&t).unwrap();
 edit(&mut s,"SetClipFades",json!({"id":"clip","fade_in_seconds":0.,"fade_out_seconds":0.}));
 let done=finish(&mut s,&start);assert_eq!(done["phase"],"completed");
 let left=pcm(&done["files"][0]);let right=pcm(&done["files"][2]);
 assert_eq!(left[0],-3./16.);assert_eq!(right[0],0.25/16.);
 assert_eq!(*left.last().unwrap(),0.);assert_eq!(*right.last().unwrap(),0.);
 assert_eq!(left[4095],2.);assert_eq!(right[4095],0.25);
}

#[test]fn clip_gain_is_frozen_in_export_pcm(){let t=Temp::new();let mut s=document();edit(&mut s,"SetClipGain",json!({"id":"clip","gain":0.25}));let job=begin(&mut s,&t).unwrap();edit(&mut s,"SetClipGain",json!({"id":"clip","gain":1.}));let done=finish(&mut s,&job);assert_eq!(done["phase"],"completed");let left=pcm(&done["files"][0]);let fill=pcm(&done["files"][2]);assert_eq!(left[0],-0.75);assert_eq!(left[1],0.5);assert_eq!(fill[0],0.0625);}
