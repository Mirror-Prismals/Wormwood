use super::document::{State, dispatch};
use serde_json::{Value, json};

fn edit(state: &mut State, hook: &str, mut input: Value) {
    input["document_revision"] =
        dispatch(state, "Snapshot", &Value::Null).unwrap()["revision"].clone();
    dispatch(state, hook, &input).unwrap();
}
fn render(state: &mut State, start: u64, frames: u64, gain: f64) -> Value {
    let revision = dispatch(state, "Snapshot", &Value::Null).unwrap()["revision"].clone();
    dispatch(
        state,
        "RenderRange",
        &json!({"document_revision":revision,"sample_rate":8000,
        "start_frame":start,"frames":frames,"master_gain":gain}),
    )
    .unwrap()
}
fn audio() -> State {
    let mut s = State::default();
    edit(
        &mut s,
        "PutTrack",
        json!({"id":"audio","name":"Audio","kind":"audio"}),
    );
    edit(
        &mut s,
        "SetTrackMix",
        json!({"id":"audio","output_bus_l":"L","output_bus_r":"R"}),
    );
    edit(
        &mut s,
        "PutSource",
        json!({"id":"source","sample_rate":8000,"channels":2,
        "samples":[0.1,0.2,0.3,0.4,0.5,0.6,0.7,0.8]}),
    );
    edit(
        &mut s,
        "PutClip",
        json!({"id":"clip","track":"audio","kind":"audio",
        "source":"source","start_seconds":0.0005,"source_offset_seconds":0.000125,"duration_seconds":0.00025}),
    );
    s
}
fn close(a: &Value, b: f64) {
    assert!((a.as_f64().unwrap() - b).abs() < 1e-6, "{a} != {b}");
}

#[test]
fn offsets_block_boundaries_and_pre_master_export_share_one_sample_clock() {
    let mut s = audio();
    let before = dispatch(&mut s, "Snapshot", &Value::Null).unwrap();
    let whole = render(&mut s, 0, 8, 1.);
    assert_eq!(whole["export_tap"], "pre_stereo_master");
    assert_eq!(whole["bus_order"], json!(["L", "S", "FF", "R"]));
    assert_eq!(whole["lsffr"][3], json!([0., 0., 0., 0.]));
    close(&whole["lsffr"][4][0], 0.3);
    close(&whole["lsffr"][4][3], 0.4);
    close(&whole["lsffr"][5][0], 0.5);
    close(&whole["lsffr"][5][3], 0.6);
    assert_eq!(whole["lsffr"][6], json!([0., 0., 0., 0.]));
    let a = render(&mut s, 0, 5, 1.);
    let b = render(&mut s, 5, 3, 1.);
    let joined: Vec<_> = a["lsffr"]
        .as_array()
        .unwrap()
        .iter()
        .chain(b["lsffr"].as_array().unwrap())
        .cloned()
        .collect();
    assert_eq!(whole["lsffr"], json!(joined));
    let muted = render(&mut s, 0, 8, 0.);
    let loud = render(&mut s, 0, 8, 2.);
    assert_eq!(whole["lsffr"], muted["lsffr"]);
    assert_eq!(whole["lsffr"], loud["lsffr"]);
    assert_eq!(muted["master_stereo"][4], json!([0., 0.]));
    close(&loud["master_stereo"][4][0], 0.6);
    assert_eq!(dispatch(&mut s, "Snapshot", &Value::Null).unwrap(), before);
}

#[test]
fn mono_duplicates_and_ff_routing_sums_before_listening() {
    let mut s = audio();
    edit(
        &mut s,
        "SetTrackMix",
        json!({"id":"audio","output_bus_l":"FF","output_bus_r":"FF"}),
    );
    edit(
        &mut s,
        "PutSource",
        json!({"id":"source","sample_rate":8000,"channels":1,"samples":[0.,2.,-3.,0.]}),
    );
    let out = render(&mut s, 4, 2, 1.);
    assert_eq!(out["lsffr"], json!([[0., 0., 4., 0.], [0., 0., -6., 0.]]));
    assert_eq!(out["master_stereo"], json!([[2., 2.], [-3., -3.]]));
}

#[test]
fn midi_local_tempos_schedule_half_open_ranges_with_off_before_on() {
    let mut s = State::default();
    edit(
        &mut s,
        "PutTrack",
        json!({"id":"midi","name":"Piano","kind":"midi"}),
    );
    edit(
        &mut s,
        "PutClip",
        json!({"id":"roll","kind":"midi","track":"midi","start_seconds":0.000125,
        "bpm":120.,"length_quarters":1.,"notes":[
            {"id":"first","quarter":0.25,"length_quarters":0.001,"pitch":60,"velocity":0.8},
            {"id":"second","quarter":0.251,"length_quarters":0.001,"pitch":60,"velocity":0.9}]}),
    );
    let all = render(&mut s, 1000, 10, 1.);
    let events = all["midi_events"].as_array().unwrap();
    assert_eq!(events.len(), 4);
    assert_eq!(events[0]["frame"], 1001);
    assert_eq!(events[0]["kind"], "note_on");
    assert_eq!(events[1]["frame"], 1005);
    assert_eq!(events[1]["kind"], "note_off");
    assert_eq!(events[2]["frame"], 1005);
    assert_eq!(events[2]["kind"], "note_on");
    assert_eq!(events[3]["frame"], 1009);
    assert_eq!(events[3]["kind"], "note_off");
    let a = render(&mut s, 1000, 5, 1.);
    let b = render(&mut s, 1005, 5, 1.);
    assert_eq!(a["midi_events"].as_array().unwrap().len(), 1);
    assert_eq!(b["midi_events"].as_array().unwrap().len(), 3);
    let held = render(&mut s, 1002, 1, 1.);
    assert_eq!(held["active_notes_at_start"][0]["note"], "first");
    assert_eq!(held["active_notes_at_start"][0]["off_frame"], 1005);
    assert_eq!(all["midi_instrument_connected"], false);
    assert_eq!(all["audio_device_connected"], false);
    edit(&mut s, "SetMidiTempo", json!({"id":"roll","bpm":240.}));
    let faster = render(&mut s, 500, 8, 1.);
    assert_eq!(faster["midi_events"][0]["frame"], 501);
}

#[test]
fn cc_first_clip_last_track_and_gap_fallback_are_explicit() {
    let mut s = audio();
    edit(
        &mut s,
        "PutTrack",
        json!({"id":"cc","name":"CC","kind":"cc"}),
    );
    edit(
        &mut s,
        "SetCcTarget",
        json!({"id":"cc","target":{"track":"audio","parameter":"gain"}}),
    );
    edit(
        &mut s,
        "PutClip",
        json!({"id":"first","track":"cc","kind":"cc","start_seconds":0.0005,
        "duration_seconds":0.000125,"points":[{"offset_seconds":0.,"value":0.}]}),
    );
    edit(
        &mut s,
        "PutClip",
        json!({"id":"later_overlap","track":"cc","kind":"cc","start_seconds":0.0005,
        "duration_seconds":0.000125,"points":[{"offset_seconds":0.,"value":1.}]}),
    );
    let out = render(&mut s, 4, 2, 1.);
    assert_eq!(out["lsffr"][0], json!([0., 0., 0., 0.]));
    close(&out["lsffr"][1][0], 0.5); // clip endpoint is exclusive; manual gain resumes.
    edit(
        &mut s,
        "PutTrack",
        json!({"id":"cc2","name":"CC 2","kind":"cc"}),
    );
    edit(
        &mut s,
        "SetCcTarget",
        json!({"id":"cc2","target":{"track":"audio","parameter":"gain"}}),
    );
    edit(
        &mut s,
        "PutClip",
        json!({"id":"override","track":"cc2","kind":"cc","start_seconds":0.0005,
        "duration_seconds":0.000125,"points":[{"offset_seconds":0.,"value":1.}]}),
    );
    let overridden = render(&mut s, 4, 1, 1.);
    close(&overridden["lsffr"][0][0], 0.3 * 10f64.powf(5. / 20.));
    edit(&mut s, "ReorderTracks", json!({"ids":["audio","cc2","cc"]}));
    assert_eq!(
        render(&mut s, 4, 1, 1.)["lsffr"][0],
        json!([0., 0., 0., 0.])
    );
}

#[test]
fn solo_is_shared_by_audio_and_midi_but_cc_never_solos_audio_away() {
    let mut s = audio();
    edit(
        &mut s,
        "PutTrack",
        json!({"id":"midi","name":"Piano","kind":"midi"}),
    );
    edit(&mut s, "SetTrackMix", json!({"id":"midi","solo":true}));
    assert_eq!(
        render(&mut s, 4, 1, 1.)["lsffr"][0],
        json!([0., 0., 0., 0.])
    );
    edit(&mut s, "SetTrackMix", json!({"id":"midi","mute":true}));
    close(&render(&mut s, 4, 1, 1.)["lsffr"][0][0], 0.3);
}

#[test]
fn stale_render_missing_source_and_mismatched_rate_fail_without_document_mutation() {
    let mut s = audio();
    let before = dispatch(&mut s, "Snapshot", &Value::Null).unwrap();
    let mut request = json!({"document_revision":0,"sample_rate":8000,"start_frame":0,"frames":8});
    assert!(dispatch(&mut s, "RenderRange", &request).is_err());
    request["document_revision"] = before["revision"].clone();
    request["sample_rate"] = json!(16000);
    request["frames"] = json!(16);
    assert!(dispatch(&mut s, "RenderRange", &request).is_err());
    assert_eq!(dispatch(&mut s, "Snapshot", &Value::Null).unwrap(), before);
    edit(
        &mut s,
        "PutClip",
        json!({"id":"clip","kind":"audio","track":"audio","source":"absent",
        "start_seconds":0.,"source_offset_seconds":0.,"duration_seconds":0.01}),
    );
    request["sample_rate"] = json!(8000);
    request["document_revision"] =
        dispatch(&mut s, "Snapshot", &Value::Null).unwrap()["revision"].clone();
    assert!(
        dispatch(&mut s, "RenderRange", &request)
            .unwrap_err()
            .contains("missing decoded")
    );
}

#[test]
fn later_same_track_clip_replaces_earlier_material_in_a_comp() {
    let mut s = audio();
    edit(
        &mut s,
        "PutSource",
        json!({"id":"replacement","sample_rate":8000,"channels":1,"samples":[2.]}),
    );
    edit(
        &mut s,
        "PutClip",
        json!({"id":"replacement_clip","kind":"audio","track":"audio","source":"replacement",
        "start_seconds":0.0005,"source_offset_seconds":0.,"duration_seconds":0.000125}),
    );
    let out = render(&mut s, 4, 2, 1.);
    assert_eq!(out["lsffr"][0], json!([2., 0., 0., 2.]));
    close(&out["lsffr"][1][0], 0.5);
}

#[test]
fn sub_sample_cc_points_keep_cardinal_coincident_order_and_publish_midi_gain() {
    let mut s = audio();
    edit(
        &mut s,
        "PutTrack",
        json!({"id":"midi","name":"Piano","kind":"midi"}),
    );
    edit(
        &mut s,
        "PutTrack",
        json!({"id":"cc","name":"CC","kind":"cc"}),
    );
    edit(
        &mut s,
        "SetCcTarget",
        json!({"id":"cc","target":{"track":"midi","parameter":"gain"}}),
    );
    edit(
        &mut s,
        "PutClip",
        json!({"id":"control","track":"cc","kind":"cc","start_seconds":0.,
        "duration_seconds":0.01,"points":[{"offset_seconds":0.00001,"value":1.},{"offset_seconds":0.00002,"value":0.}]}),
    );
    let out = render(&mut s, 0, 1, 1.);
    let mix = out["track_mix"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["track"] == "midi")
        .unwrap();
    assert_eq!(mix["gain"], json!([0.]));
}

#[test]
fn fractional_moves_preserve_source_extent() {
    let mut s = audio();
    edit(
        &mut s,
        "PutSource",
        json!({"id":"one_frame","sample_rate":8000,
        "channels":1,"samples":[1.]}),
    );
    edit(
        &mut s,
        "PutClip",
        json!({"id":"clip","kind":"audio","track":"audio",
        "source":"one_frame","source_offset_seconds":0.,"start_seconds":0.,
        "duration_seconds":0.000175}),
    );
    let before = render(&mut s, 0, 3, 1.);
    edit(
        &mut s,
        "MoveClip",
        json!({"id":"clip","start_seconds":0.000025}),
    );
    let after = render(&mut s, 0, 3, 1.);
    assert_eq!(before["lsffr"], after["lsffr"]);
    assert_eq!(
        after["lsffr"],
        json!([[1., 0., 0., 1.], [0., 0., 0., 0.], [0., 0., 0., 0.]])
    );
}

#[test]
fn ranges_can_flush_midi_note_offs_at_the_document_limit() {
    let mut s = State::default();
    edit(
        &mut s,
        "PutTrack",
        json!({"id":"midi","name":"Piano","kind":"midi"}),
    );
    edit(
        &mut s,
        "PutClip",
        json!({"id":"last","kind":"midi","track":"midi",
        "start_seconds":86399.,"bpm":60.,"length_quarters":1.,"notes":[
        {"id":"note","quarter":0.,"length_quarters":1.,"pitch":60,"velocity":1.}]}),
    );
    let boundary = 86400 * 8000;
    let held = render(&mut s, boundary - 1, 1, 1.);
    assert_eq!(held["active_notes_at_start"][0]["off_frame"], boundary);
    assert_eq!(held["midi_events"], json!([]));
    let flush = render(&mut s, boundary, 1, 1.);
    assert_eq!(flush["midi_events"].as_array().unwrap().len(), 1);
    assert_eq!(flush["midi_events"][0]["kind"], "note_off");
    assert_eq!(flush["midi_events"][0]["frame"], boundary);
    assert_eq!(flush["midi_events"][0]["offset_frame"], 0);
    assert_eq!(flush["active_notes_at_start"], json!([]));
    let revision = dispatch(&mut s, "Snapshot", &Value::Null).unwrap()["revision"].clone();
    assert!(
        dispatch(
            &mut s,
            "RenderRange",
            &json!({"document_revision":revision,
        "sample_rate":8000,"start_frame":u64::MAX,"frames":1})
        )
        .is_err()
    );
}

#[test]
fn piano_microtone_pcm_and_local_tempo_do_not_change_other_clips() {
 let mut s=State::default();
 edit(&mut s,"PutTrack",json!({"id":"keys","name":"Keys","kind":"midi"}));
 let (anchor,cents)=super::piano_tuning::encode(61,19);
 edit(&mut s,"PutClip",json!({"id":"micro","track":"keys","kind":"midi","start_seconds":0.,"bpm":93.,"edo":19,"length_quarters":4.,"notes":[{"id":"black","quarter":0.,"length_quarters":2.,"pitch":anchor,"tuning_cents":cents,"velocity":1.}]}));
 edit(&mut s,"PutClip",json!({"id":"independent","track":"keys","kind":"midi","start_seconds":5.,"bpm":137.,"length_quarters":4.,"notes":[]}));
 let before=dispatch(&mut s,"Snapshot",&Value::Null).unwrap();
 let report=dispatch(&mut s,"RenderRange",&json!({"document_revision":before["revision"],"sample_rate":8000,"start_frame":800,"frames":2048,"instrument":"cardinal_sine"})).unwrap();
 let pcm=report["master_stereo"].as_array().unwrap();
 let crossings=pcm.windows(2).filter(|pair|pair[0][0].as_f64().unwrap()<=0.&&pair[1][0].as_f64().unwrap()>0.).count();
 let hz=crossings as f64*8000./2048.;let expected=super::piano_tuning::frequency(anchor,cents);
 assert!((hz-expected).abs()<5.,"measured {hz}, expected {expected}");
 let mut changed=before["clips"][0].clone();changed["bpm"]=json!(111.);edit(&mut s,"PutClip",changed);
 let after=dispatch(&mut s,"Snapshot",&Value::Null).unwrap();assert_eq!(before["clips"][1],after["clips"][1]);
 assert_eq!(before["clips"][0]["notes"][0]["quarter"],after["clips"][0]["notes"][0]["quarter"]);
 assert_eq!(before["clips"][0]["notes"][0]["tuning_cents"],after["clips"][0]["notes"][0]["tuning_cents"]);
}

#[test] fn note_bend_roundtrips_rejects_invalid_and_pcm_seeks_match(){
 let mut s=State::default();edit(&mut s,"PutTrack",json!({"id":"keys","name":"Keys","kind":"midi"}));
 let mut clip=json!({"id":"bend","track":"keys","kind":"midi","start_seconds":0.,"bpm":120.,"length_quarters":4.,"notes":[{"id":"a","quarter":0.,"length_quarters":2.,"pitch":60,"velocity":1.,"bend_semitones":2.}]});
 edit(&mut s,"PutClip",clip.clone());let before=dispatch(&mut s,"Snapshot",&Value::Null).unwrap();assert_eq!(before["clips"][0]["notes"][0]["bend_semitones"],2.);
 clip["document_revision"]=before["revision"].clone();clip["notes"][0]["bend_semitones"]=json!(2.01);
 assert!(dispatch(&mut s,"PutClip",&clip).is_err());assert_eq!(dispatch(&mut s,"Snapshot",&Value::Null).unwrap(),before);
 let pcm=|s:&mut State,start,frames| {let rev=dispatch(s,"Snapshot",&Value::Null).unwrap()["revision"].clone();dispatch(s,"RenderRange",&json!({"document_revision":rev,"sample_rate":8000,"start_frame":start,"frames":frames,"instrument":"cardinal_sine"})).unwrap()};
 let whole=pcm(&mut s,2000,4000);let part=pcm(&mut s,4000,2000);
 assert_eq!(whole["master_stereo"][2000],part["master_stereo"][0]);
 let mut flat=before["clips"][0].clone();flat["notes"][0]["bend_semitones"]=json!(0.);edit(&mut s,"PutClip",flat);
 let unbent=pcm(&mut s,4000,2000);assert_ne!(unbent["master_stereo"],part["master_stereo"]);
}
