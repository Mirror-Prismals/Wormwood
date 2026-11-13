use super::{
    document::{State, dispatch},
    warp_treatment,
};
use serde_json::{Value, json};
fn edit(s: &mut State, h: &str, mut input: Value) -> Result<Value, String> {
    input["document_revision"] = json!(s.revision);
    dispatch(s, h, &input)
}
fn fixture(rate: u32) -> State {
    let mut s = State::default();
    edit(
        &mut s,
        "PutTrack",
        json!({"id":"t","name":"Audio","kind":"audio"}),
    )
    .unwrap();
    let samples: Vec<f32> = (0..rate)
        .map(|i| 0.5 * (std::f64::consts::TAU * 440. * i as f64 / rate as f64).sin() as f32)
        .collect();
    edit(
        &mut s,
        "PutSource",
        json!({"id":"src","sample_rate":rate,"channels":1,"samples":samples}),
    )
    .unwrap();
    for (id, start) in [("warped", 0.), ("free", 2.)] {
        edit(&mut s,"PutClip",json!({"id":id,"track":"t","kind":"audio","source":"src","source_offset_seconds":0.,"duration_seconds":1.,"start_seconds":start})).unwrap();
    }
    s
}
fn warp(s: &mut State) {
    edit(s, "SetClipWarp", json!({"id":"warped","enabled":true})).unwrap();
}
fn pump(s: &mut State) {
    let start = std::time::Instant::now();
    loop {
        dispatch(s, "PumpWarp", &Value::Null).unwrap();
        let v = dispatch(s, "Snapshot", &Value::Null).unwrap();
        if v["warp"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["status"] == "ready" || e["status"] == "disabled")
        {
            break;
        }
        assert!(start.elapsed().as_secs() < 10, "{v}");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
fn pcm(s: &mut State, rate: u32, first: u64) -> Value {
    edit(
        s,
        "RenderRange",
        json!({"sample_rate":rate,"start_frame":first,"frames":4096}),
    )
    .unwrap()["lsffr"]
        .clone()
}
#[test]
fn real_default_map_creates_identity_without_retiming_free_time() {
    let mut s = fixture(44100);
    let before = pcm(&mut s, 44100, 3000);
    warp(&mut s);
    assert_eq!(before, pcm(&mut s, 44100, 3000));
    assert_eq!(s.clips["free"].duration(), 1.);
    assert!(warp_treatment::ready_for_export(&s).is_ok());
}
#[test]
fn publishing_shared_tempo_stretches_only_enabled_warp_clips_and_keeps_source_end() {
    let mut s = fixture(48000);
    warp(&mut s);
    let before = warp_treatment::source_span(&s, "warped").unwrap();
    edit(&mut s, "SetMusicalTime", json!({"bpm":80.})).unwrap();
    assert_eq!(s.clips["warped"].duration(), 1.5);
    assert_eq!(s.clips["free"].duration(), 1.);
    assert_eq!(s.clips["free"].start, 2.);
    assert_eq!(warp_treatment::source_span(&s, "warped").unwrap(), before);
    pump(&mut s);
}
#[test]
fn rebase_captures_endpoint_before_start_and_origin_move() {
    let mut s = fixture(44100);
    edit(&mut s,"SetMusicalTime",json!({"map":{"bpm":120.,"rubato_regions":[{"start_quarter":0.,"seconds":[0.,0.25,1.,1.2],"length_quarters":2.5}]}})).unwrap();
    warp(&mut s);
    let span = warp_treatment::source_span(&s, "warped").unwrap();
    let duration = s.clips["warped"].duration();
    let before = pcm(&mut s, 44100, 3000);
    edit(&mut s, "RebaseTimeline", json!({"shift_seconds":4.})).unwrap();
    assert_eq!(s.timeline_zero_seconds, 4.);
    assert_eq!(s.clips["warped"].start, 4.);
    assert_eq!(s.clips["free"].start, 6.);
    assert!((s.clips["warped"].duration() - duration).abs() < 1e-12);
    assert_eq!(warp_treatment::source_span(&s, "warped").unwrap(), span);
    pump(&mut s);
    assert_eq!(pcm(&mut s, 44100, 4 * 44100 + 3000), before);
}
#[test]
fn rejected_map_publication_leaves_clock_pcm_and_document_atomic() {
    let mut s = fixture(48000);
    warp(&mut s);
    edit(
        &mut s,
        "MoveClip",
        json!({"id":"warped","start_seconds":86398.8}),
    )
    .unwrap();
    let before = dispatch(&mut s, "Snapshot", &Value::Null).unwrap();
    let old_clock = s.warp.clock.as_ref().unwrap().cache_identity();
    assert!(edit(&mut s, "SetMusicalTime", json!({"bpm":60.})).is_err());
    assert_eq!(dispatch(&mut s, "Snapshot", &Value::Null).unwrap(), before);
    assert_eq!(s.warp.clock.as_ref().unwrap().cache_identity(), old_clock);
}
#[test]
fn revision_cache_key_tracks_timing_but_not_unrelated_clip_edits() {
    let mut s = fixture(44100);
    warp(&mut s);
    let a = s.warp.clock.as_ref().unwrap().cache_identity();
    edit(&mut s, "SetTrackMix", json!({"id":"t","gain":0.5})).unwrap();
    assert_eq!(s.warp.clock.as_ref().unwrap().cache_identity(), a);
    edit(&mut s, "SetMusicalTime", json!({"bpm":80.})).unwrap();
    let c = s
        .warp
        .clock
        .as_ref()
        .unwrap()
        .for_sample_rate(44100)
        .unwrap()
        .unwrap();
    assert_ne!(c.cache_identity(), a);
    assert_eq!(c.cache_identity()[2], 44100f64.to_bits());
    assert_eq!(c.cache_identity()[1], s.musical_time.revision);
}
#[test]
fn failed_rebase_keeps_every_start_origin_clock_and_revision() {
    let mut s = fixture(48000);
    warp(&mut s);
    let before = dispatch(&mut s, "Snapshot", &Value::Null).unwrap();
    assert!(edit(&mut s, "RebaseTimeline", json!({"shift_seconds":86400.})).is_err());
    assert_eq!(dispatch(&mut s, "Snapshot", &Value::Null).unwrap(), before);
}
#[test]
fn pure_rebase_rekeys_ready_warp_audio_without_silent_gap() {
    let mut s = fixture(48000);
    warp(&mut s);
    edit(&mut s, "SetMusicalTime", json!({"bpm":80.})).unwrap();
    pump(&mut s);
    let before = pcm(&mut s, 48000, 12000);
    edit(&mut s, "RebaseTimeline", json!({"shift_seconds":4.})).unwrap();
    assert!(warp_treatment::ready_for_export(&s).is_ok());
    assert_eq!(pcm(&mut s, 48000, 4 * 48000 + 12000), before);
}
#[test]
fn grid_only_publication_preserves_ready_warp_cache() {
    let mut s = fixture(48000);
    warp(&mut s);
    edit(&mut s, "SetMusicalTime", json!({"bpm":80.})).unwrap();
    pump(&mut s);
    let before = pcm(&mut s, 48000, 12000);
    edit(&mut s, "SetMusicalTime", json!({"grid_choice":"fixed_1_8"})).unwrap();
    assert!(warp_treatment::ready_for_export(&s).is_ok());
    assert_eq!(pcm(&mut s, 48000, 12000), before);
}
#[test]
fn decimal_rubato_rebase_retains_prepared_pcm() {
    let mut s = fixture(48000);
    edit(&mut s,"SetMusicalTime",json!({"map":{"bpm":120.,"rubato_regions":[{"start_quarter":0.,"seconds":[0.,0.3,0.9,1.2],"length_quarters":2.5}]}})).unwrap();
    warp(&mut s);
    edit(&mut s, "SetMusicalTime", json!({"bpm":80.})).unwrap();
    pump(&mut s);
    let before = pcm(&mut s, 48000, 12000);
    edit(&mut s, "RebaseTimeline", json!({"shift_seconds":4.})).unwrap();
    assert!(warp_treatment::ready_for_export(&s).is_ok());
    assert_eq!(pcm(&mut s, 48000, 4 * 48000 + 12000), before);
}
