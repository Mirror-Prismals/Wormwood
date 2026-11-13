//! Document-boundary regressions for the focused workstation backend.
use super::document::{State, dispatch};
use serde_json::{Value, json};

fn snapshot(state: &mut State) -> Value {
    dispatch(state, "Snapshot", &Value::Null).unwrap()
}

fn edit(state: &mut State, hook: &str, mut input: Value) -> Result<Value, String> {
    input["document_revision"] = snapshot(state)["revision"].clone();
    dispatch(state, hook, &input)
}

fn track(state: &mut State, id: &str, kind: &str) {
    edit(state, "PutTrack", json!({"id":id,"name":id,"kind":kind})).unwrap();
}

fn find<'a>(document: &'a Value, collection: &str, id: &str) -> &'a Value {
    document[collection]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == id)
        .unwrap()
}

fn close(actual: &Value, expected: f64) {
    let actual = actual.as_f64().unwrap();
    assert!((actual - expected).abs() < 1e-10, "{actual} != {expected}");
}

fn rejected_without_change(state: &mut State, hook: &str, input: Value) {
    let before = snapshot(state);
    assert!(
        edit(state, hook, input).is_err(),
        "{hook} unexpectedly accepted invalid input"
    );
    assert_eq!(
        snapshot(state),
        before,
        "{hook} partially changed the document"
    );
}

fn midi(id: &str, track: &str, start: f64, bpm: f64, quarters: f64, notes: Value) -> Value {
    json!({"id":id,"track":track,"kind":"midi","start_seconds":start,
        "bpm":bpm,"length_quarters":quarters,"notes":notes})
}

fn note(id: &str, quarter: f64, length: f64) -> Value {
    json!({"id":id,"quarter":quarter,"length_quarters":length,"pitch":60,"velocity":0.75})
}

fn audio(id: &str, track: &str, start: f64, duration: f64) -> Value {
    json!({"id":id,"track":track,"kind":"audio","start_seconds":start,
        "source":"pcm","source_offset_seconds":0.0,"duration_seconds":duration})
}

fn render(state: &mut State, frames: u64) -> Value {
    let revision = snapshot(state)["revision"].clone();
    dispatch(
        state,
        "RenderRange",
        &json!({"document_revision":revision,
        "sample_rate":8000,"start_frame":0,"frames":frames}),
    )
    .unwrap()
}

#[test]
fn piano_roll_tempos_remain_independent_and_invalid_edits_are_atomic() {
    let mut state = State::default();
    track(&mut state, "fast", "midi");
    track(&mut state, "slow", "midi");
    edit(
        &mut state,
        "PutClip",
        midi(
            "fast-roll",
            "fast",
            0.125,
            120.0,
            4.0,
            json!([note("fast-note", 1.0, 0.5)]),
        ),
    )
    .unwrap();
    edit(
        &mut state,
        "PutClip",
        midi(
            "slow-roll",
            "slow",
            0.375,
            60.0,
            3.0,
            json!([note("slow-note", 2.0, 0.5)]),
        ),
    )
    .unwrap();
    let before = snapshot(&mut state);
    close(
        &find(&before, "clips", "fast-roll")["notes"][0]["time_seconds"],
        0.625,
    );
    close(
        &find(&before, "clips", "slow-roll")["notes"][0]["time_seconds"],
        2.375,
    );

    edit(
        &mut state,
        "SetMidiTempo",
        json!({"id":"fast-roll","bpm":90.0}),
    )
    .unwrap();
    let after = snapshot(&mut state);
    assert_eq!(
        after["revision"].as_u64(),
        before["revision"].as_u64().map(|n| n + 1)
    );
    assert_eq!(after["time_basis"], "seconds");
    assert!(after.get("bpm").is_none());
    assert_eq!(
        find(&after, "clips", "slow-roll"),
        find(&before, "clips", "slow-roll")
    );
    let changed = find(&after, "clips", "fast-roll");
    close(&changed["start_seconds"], 0.125);
    close(&changed["duration_seconds"], 8.0 / 3.0);
    close(&changed["notes"][0]["quarter"], 1.0);
    close(&changed["notes"][0]["length_quarters"], 0.5);
    close(&changed["notes"][0]["time_seconds"], 0.125 + 2.0 / 3.0);
    close(&changed["notes"][0]["duration_seconds"], 1.0 / 3.0);
    for bpm in [json!(0), json!(1001), Value::Null] {
        rejected_without_change(
            &mut state,
            "SetMidiTempo",
            json!({"id":"fast-roll","bpm":bpm}),
        );
    }
    edit(
        &mut state,
        "PutClip",
        midi(
            "late-roll",
            "slow",
            86399.0,
            120.0,
            1.0,
            json!([note("late-note", 0.0, 0.5)]),
        ),
    )
    .unwrap();
    rejected_without_change(
        &mut state,
        "SetMidiTempo",
        json!({"id":"late-roll","bpm":1.0}),
    );
}

#[test]
fn cc_clamps_points_and_enforces_clip_track_and_target_kinds() {
    let mut state = State::default();
    track(&mut state, "audio", "audio");
    track(&mut state, "midi", "midi");
    track(&mut state, "control", "cc");
    let cc = json!({"id":"curve","track":"control","kind":"cc",
    "start_seconds":0.125,"duration_seconds":1.0,"points":[
        {"offset_seconds":2.0,"value":2.0},
        {"offset_seconds":0.125,"value":0.75},
        {"offset_seconds":-1.0,"value":-0.5}
    ]});
    edit(&mut state, "PutClip", cc.clone()).unwrap();
    let document = snapshot(&mut state);
    assert_eq!(
        find(&document, "clips", "curve")["points"],
        json!([
            {"offset_seconds":0.0,"value":0.0},
            {"offset_seconds":0.125,"value":0.75},
            {"offset_seconds":1.0,"value":1.0}
        ])
    );
    let control = find(&document, "tracks", "control");
    assert!(control["target"].is_null());
    assert!(control.get("output_bus_l").is_none());
    assert!(control.get("mute").is_none());

    let mut bad = cc.clone();
    bad["points"][1]["value"] = Value::Null;
    rejected_without_change(&mut state, "PutClip", bad);
    let mut wrong_track = cc.clone();
    wrong_track["track"] = json!("audio");
    rejected_without_change(&mut state, "PutClip", wrong_track);
    let mut zero_length = cc;
    zero_length["duration_seconds"] = json!(0.0);
    rejected_without_change(&mut state, "PutClip", zero_length);
    rejected_without_change(
        &mut state,
        "PutTrack",
        json!({"id":"control","name":"Changed","kind":"audio"}),
    );
    rejected_without_change(
        &mut state,
        "SetTrackMix",
        json!({"id":"control","mute":true}),
    );
    rejected_without_change(
        &mut state,
        "SetMidiTempo",
        json!({"id":"curve","bpm":120.0}),
    );

    for destination in ["audio", "midi"] {
        edit(
            &mut state,
            "SetCcTarget",
            json!({"id":"control",
            "target":{"track":destination,"parameter":"gain"}}),
        )
        .unwrap();
        let document = snapshot(&mut state);
        assert_eq!(
            find(&document, "tracks", "control")["target"],
            json!({"track":destination,"parameter":"gain"})
        );
    }
    for target in [
        json!({"track":"control","parameter":"gain"}),
        json!({"track":"missing","parameter":"gain"}),
        json!({"track":"audio","parameter":"tempo"}),
    ] {
        rejected_without_change(
            &mut state,
            "SetCcTarget",
            json!({"id":"control","target":target}),
        );
    }
    rejected_without_change(
        &mut state,
        "SetCcTarget",
        json!({"id":"audio","target":null}),
    );
    edit(
        &mut state,
        "SetCcTarget",
        json!({"id":"control","target":null}),
    )
    .unwrap();
    assert!(find(&snapshot(&mut state), "tracks", "control")["target"].is_null());
}

#[test]
fn renaming_preserves_audio_mix_and_cc_binding() {
    let mut state = State::default();
    track(&mut state, "audio", "audio");
    track(&mut state, "control", "cc");
    let initial = snapshot(&mut state);
    let initial_audio = find(&initial, "tracks", "audio");
    assert_eq!(initial_audio["output_bus_l"], "FF");
    assert_eq!(initial_audio["output_bus_r"], "FF");
    close(&initial_audio["gain"], 1.0);
    edit(
        &mut state,
        "SetTrackMix",
        json!({"id":"audio","gain":0.375,"mute":true,
        "output_bus_l":"L","output_bus_r":"OFF"}),
    )
    .unwrap();
    edit(
        &mut state,
        "SetCcTarget",
        json!({"id":"control",
        "target":{"track":"audio","parameter":"gain"}}),
    )
    .unwrap();
    let before = snapshot(&mut state);
    edit(
        &mut state,
        "PutTrack",
        json!({"id":"audio","name":"Renamed source","kind":"audio"}),
    )
    .unwrap();
    edit(
        &mut state,
        "PutTrack",
        json!({"id":"control","name":"Renamed automation","kind":"cc"}),
    )
    .unwrap();
    let after = snapshot(&mut state);
    for (id, name) in [
        ("audio", "Renamed source"),
        ("control", "Renamed automation"),
    ] {
        let mut expected = find(&before, "tracks", id).clone();
        expected["name"] = json!(name);
        assert_eq!(find(&after, "tracks", id), &expected);
    }
}

#[test]
fn mix_patches_enforce_mute_solo_exclusion_and_commit_routes_atomically() {
    let mut state = State::default();
    track(&mut state, "audio", "audio");
    for (patch, mute, solo) in [
        (json!({"id":"audio","mute":true}), true, false),
        (json!({"id":"audio","solo":true}), false, true),
        (json!({"id":"audio","solo":false}), false, false),
        (json!({"id":"audio","solo":true}), false, true),
        (json!({"id":"audio","mute":true}), true, false),
    ] {
        edit(&mut state, "SetTrackMix", patch).unwrap();
        let document = snapshot(&mut state);
        let track = find(&document, "tracks", "audio");
        assert_eq!(track["mute"], mute);
        assert_eq!(track["solo"], solo);
    }
    for patch in [
        json!({"id":"audio","gain":0.75,"output_bus_l":"L","output_bus_r":"invalid"}),
        json!({"id":"audio","mute":true,"solo":true}),
        json!({"id":"audio","mute":1}),
        json!({"id":"audio","gain":-0.5}),
        json!({"id":"audio","gain":0.5,"output_bus_l":null}),
    ] {
        rejected_without_change(&mut state, "SetTrackMix", patch);
    }
    edit(
        &mut state,
        "SetTrackMix",
        json!({"id":"audio","output_bus_l":"S"}),
    )
    .unwrap();
    let document = snapshot(&mut state);
    assert_eq!(find(&document, "tracks", "audio")["output_bus_l"], "S");
    assert_eq!(find(&document, "tracks", "audio")["output_bus_r"], "FF");
}

#[test]
fn render_observes_gain_routes_and_solo_across_audio_and_midi() {
    let mut state = State::default();
    track(&mut state, "audio", "audio");
    track(&mut state, "midi", "midi");
    edit(
        &mut state,
        "PutSource",
        json!({"id":"pcm","sample_rate":8000,
        "channels":2,"samples":[2.0,4.0,6.0,8.0]}),
    )
    .unwrap();
    edit(
        &mut state,
        "PutClip",
        audio("sound", "audio", 0.0, 2.0 / 8000.0),
    )
    .unwrap();
    edit(
        &mut state,
        "PutClip",
        midi(
            "roll",
            "midi",
            0.0,
            120.0,
            1.0,
            json!([note("note", 0.0, 0.5)]),
        ),
    )
    .unwrap();
    edit(
        &mut state,
        "SetTrackMix",
        json!({"id":"audio","gain":0.5,
        "output_bus_l":"L","output_bus_r":"R"}),
    )
    .unwrap();
    let before_render = snapshot(&mut state);
    let initial = render(&mut state, 2);
    assert_eq!(
        initial["lsffr"],
        json!([[1.0, 0.0, 0.0, 2.0], [3.0, 0.0, 0.0, 4.0]])
    );
    assert_eq!(initial["master_stereo"], json!([[1.0, 2.0], [3.0, 4.0]]));
    assert_eq!(initial["midi_events"][0]["kind"], "note_on");
    assert_eq!(initial["midi_events"][0]["frame"], 0);
    assert_eq!(snapshot(&mut state), before_render);

    edit(&mut state, "SetTrackMix", json!({"id":"midi","solo":true})).unwrap();
    let midi_solo = render(&mut state, 2);
    assert_eq!(
        midi_solo["lsffr"],
        json!([[0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0]])
    );
    assert_eq!(midi_solo["midi_events"].as_array().unwrap().len(), 1);
    edit(&mut state, "SetTrackMix", json!({"id":"audio","solo":true})).unwrap();
    assert_eq!(render(&mut state, 2)["lsffr"], initial["lsffr"]);
    edit(&mut state, "SetTrackMix", json!({"id":"midi","mute":true})).unwrap();
    let audio_solo = render(&mut state, 2);
    assert_eq!(audio_solo["lsffr"], initial["lsffr"]);
    assert!(audio_solo["midi_events"].as_array().unwrap().is_empty());
    edit(&mut state, "SetTrackMix", json!({"id":"audio","mute":true})).unwrap();
    assert_eq!(
        render(&mut state, 2)["lsffr"],
        json!([[0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0]])
    );
}

#[test]
fn explicit_mixed_track_order_survives_rename_and_rejects_incomplete_orders() {
    let mut state = State::default();
    track(&mut state, "z-audio", "audio");
    track(&mut state, "a-control", "cc");
    track(&mut state, "m-midi", "midi");
    let initial = snapshot(&mut state);
    let ids = |document: &Value| {
        document["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["id"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&initial), vec!["z-audio", "a-control", "m-midi"]);
    edit(
        &mut state,
        "ReorderTracks",
        json!({"ids":["m-midi","z-audio","a-control"]}),
    )
    .unwrap();
    edit(
        &mut state,
        "PutTrack",
        json!({"id":"z-audio","name":"Renamed","kind":"audio"}),
    )
    .unwrap();
    let reordered = snapshot(&mut state);
    assert_eq!(ids(&reordered), vec!["m-midi", "z-audio", "a-control"]);
    assert_eq!(reordered["tracks"][0]["kind"], "midi");
    assert_eq!(reordered["tracks"][1]["kind"], "audio");
    assert_eq!(reordered["tracks"][2]["kind"], "cc");
    for ids in [
        json!(["m-midi", "z-audio"]),
        json!(["m-midi", "z-audio", "z-audio"]),
        json!(["m-midi", "z-audio", "missing"]),
    ] {
        rejected_without_change(&mut state, "ReorderTracks", json!({"ids":ids}));
    }
}

#[test]
fn audio_start_snaps_to_a_delayed_note_and_stale_commit_is_rejected() {
    let mut state = State::default();
    track(&mut state, "audio", "audio");
    track(&mut state, "midi", "midi");
    edit(&mut state, "PutClip", audio("sound", "audio", 0.125, 0.25)).unwrap();
    edit(
        &mut state,
        "PutClip",
        midi(
            "roll",
            "midi",
            2.125,
            80.0,
            4.0,
            json!([note("delayed", 2.0, 0.5)]),
        ),
    )
    .unwrap();
    let before = snapshot(&mut state);
    let snapped = dispatch(
        &mut state,
        "SnapClip",
        &json!({"id":"sound","anchor":"start",
        "proposed_start_seconds":3.6,"tolerance_seconds":0.05}),
    )
    .unwrap();
    assert_eq!(snapped["snapped"], true);
    assert_eq!(snapped["target_clip"], "roll");
    assert_eq!(snapped["target_note"], "delayed");
    assert!(snapped["source_note"].is_null());
    close(&snapped["target_time_seconds"], 3.625);
    close(&snapped["start_seconds"], 3.625);
    close(&snapped["delta_seconds"], 0.025);
    assert_eq!(snapshot(&mut state), before);

    edit(&mut state, "SetMidiTempo", json!({"id":"roll","bpm":160.0})).unwrap();
    let before_stale_commit = snapshot(&mut state);
    let result = dispatch(
        &mut state,
        "MoveClip",
        &json!({"id":"sound",
        "start_seconds":snapped["start_seconds"],"document_revision":snapped["revision"]}),
    );
    assert!(result.unwrap_err().contains("stale"));
    assert_eq!(snapshot(&mut state), before_stale_commit);

    let fresh = dispatch(
        &mut state,
        "SnapClip",
        &json!({"id":"sound","anchor":"start",
        "proposed_start_seconds":2.9,"tolerance_seconds":0.05}),
    )
    .unwrap();
    close(&fresh["start_seconds"], 2.875);
    dispatch(
        &mut state,
        "MoveClip",
        &json!({"id":"sound","start_seconds":fresh["start_seconds"],
        "document_revision":fresh["revision"]}),
    )
    .unwrap();
    let after = snapshot(&mut state);
    close(&find(&after, "clips", "sound")["start_seconds"], 2.875);
    close(&find(&after, "clips", "sound")["duration_seconds"], 0.25);
    assert_eq!(
        find(&after, "clips", "roll"),
        find(&before_stale_commit, "clips", "roll")
    );
    rejected_without_change(
        &mut state,
        "SnapClip",
        json!({"id":"sound","anchor":"notes",
        "proposed_start_seconds":2.9,"tolerance_seconds":0.05}),
    );
}

#[test]
fn snap_clip_excludes_same_track_while_legacy_snap_midi_preserves_it() {
    let mut state = State::default();
    track(&mut state, "same", "midi");
    track(&mut state, "other", "midi");
    edit(
        &mut state,
        "PutClip",
        midi(
            "moving",
            "same",
            0.0,
            120.0,
            1.0,
            json!([note("source", 0.0, 0.25)]),
        ),
    )
    .unwrap();
    edit(
        &mut state,
        "PutClip",
        midi(
            "same-target",
            "same",
            4.0,
            120.0,
            2.0,
            json!([note("target", 1.0, 0.25)]),
        ),
    )
    .unwrap();
    // An empty piano roll on another track is not a source of invented ticks.
    edit(
        &mut state,
        "PutClip",
        midi("empty", "other", 4.5, 60.0, 2.0, json!([])),
    )
    .unwrap();
    let before = snapshot(&mut state);
    let request = json!({"id":"moving","anchor":"notes",
        "proposed_start_seconds":4.45,"tolerance_seconds":0.1});
    let modern = dispatch(&mut state, "SnapClip", &request).unwrap();
    assert_eq!(modern["snapped"], false);
    close(&modern["start_seconds"], 4.45);
    let legacy = dispatch(&mut state, "SnapMidi", &request).unwrap();
    assert_eq!(legacy["snapped"], true);
    assert_eq!(legacy["target_clip"], "same-target");
    assert_eq!(legacy["source_note"], "source");
    assert_eq!(legacy["target_note"], "target");
    close(&legacy["start_seconds"], 4.5);
    close(&legacy["bpm"], 120.0);
    assert_eq!(snapshot(&mut state), before);
}

#[test]
fn snapping_uses_real_onsets_only_and_breaks_equal_distance_ties_by_time() {
    let mut state = State::default();
    track(&mut state, "audio", "audio");
    track(&mut state, "midi", "midi");
    edit(
        &mut state,
        "PutClip",
        audio("sound", "audio", 0.123456789, 0.25),
    )
    .unwrap();
    let alone = dispatch(
        &mut state,
        "SnapClip",
        &json!({"id":"sound",
        "proposed_start_seconds":0.123456789,"tolerance_seconds":1.0}),
    )
    .unwrap();
    assert_eq!(alone["snapped"], false);
    close(&alone["start_seconds"], 0.123456789);
    edit(
        &mut state,
        "PutClip",
        midi(
            "roll",
            "midi",
            8.0,
            60.0,
            3.0,
            json!([note("first", 1.0, 0.25), note("second", 1.5, 0.25)]),
        ),
    )
    .unwrap();
    let before = snapshot(&mut state);
    // Origin, clip start, an implied beat, and note end are not snap targets.
    for proposed in [0.03, 8.0, 8.5, 9.25] {
        let answer = dispatch(
            &mut state,
            "SnapClip",
            &json!({"id":"sound","anchor":"start",
            "proposed_start_seconds":proposed,"tolerance_seconds":0.01}),
        )
        .unwrap();
        assert_eq!(answer["snapped"], false, "invented target at {proposed}");
        close(&answer["start_seconds"], proposed);
    }
    let tied = dispatch(
        &mut state,
        "SnapClip",
        &json!({"id":"sound","anchor":"start",
        "proposed_start_seconds":9.25,"tolerance_seconds":0.25}),
    )
    .unwrap();
    close(&tied["start_seconds"], 9.0);
    close(&tied["delta_seconds"], -0.25);
    assert_eq!(tied["target_note"], "first");
    let exact = dispatch(
        &mut state,
        "SnapClip",
        &json!({"id":"sound","anchor":"start",
        "proposed_start_seconds":9.5,"tolerance_seconds":0.0}),
    )
    .unwrap();
    assert_eq!(exact["snapped"], true);
    close(&exact["delta_seconds"], 0.0);
    assert_eq!(snapshot(&mut state), before);
    rejected_without_change(
        &mut state,
        "SnapClip",
        json!({"id":"sound",
        "proposed_start_seconds":86400.0,"tolerance_seconds":0.1}),
    );
}

#[test]
fn browser_insertion_is_atomic_ordered_and_revision_checked(){
 let mut s=State::default();track(&mut s,"a","audio");track(&mut s,"c","cc");let old=snapshot(&mut s);
 edit(&mut s,"PutTrack",json!({"id":"m","name":"Midi","kind":"midi","insert_index":1})).unwrap();let now=snapshot(&mut s);
 assert_eq!(now["tracks"].as_array().unwrap().iter().map(|t|t["id"].as_str().unwrap()).collect::<Vec<_>>(),vec!["a","m","c"]);
 assert_eq!(now["revision"].as_u64().unwrap(),old["revision"].as_u64().unwrap()+1);
 for index in [json!(-1),json!(99),json!(1.5),Value::Null]{rejected_without_change(&mut s,"PutTrack",json!({"id":"bad","name":"Bad","kind":"audio","insert_index":index}));}
 rejected_without_change(&mut s,"PutTrack",json!({"id":"a","name":"No rename","kind":"audio","insert_index":0}));
 assert!(dispatch(&mut s,"PutTrack",&json!({"document_revision":old["revision"],"id":"stale","name":"Stale","kind":"midi","insert_index":0})).is_err());assert_eq!(snapshot(&mut s),now);
}


fn create_range(state: &mut State, first: &str, last: &str, start: f64, end: f64) -> Value {
    edit(state, "CreateRangeClips", json!({"first_track":first,"last_track":last,
        "start_seconds":start,"end_seconds":end})).unwrap()
}
fn empty_cc(id: &str, track: &str, start: f64, duration: f64) -> Value {
    json!({"id":id,"track":track,"kind":"cc","start_seconds":start,
        "duration_seconds":duration,"points":[]})
}

#[test]
fn range_clips_reverse_endpoints_follow_document_order_and_preserve_exact_duration() {
    let mut s=State::default();
    for (id,kind) in [("m1","midi"),("c","cc"),("a","audio"),("m2","midi")] {track(&mut s,id,kind);}
    edit(&mut s,"ReorderTracks",json!({"ids":["m2","a","c","m1"]})).unwrap();
    let before=snapshot(&mut s);
    let result=create_range(&mut s,"m1","m2",4.625,1.25);
    let after=snapshot(&mut s);
    assert_eq!(result["rejected"],json!([]));
    assert_eq!(after["tracks"],before["tracks"]);
    assert_eq!(after["revision"].as_u64().unwrap(),before["revision"].as_u64().unwrap()+2);
    let created=result["created"].as_array().unwrap();assert_eq!(created.len(),3);
    let ordered=created.iter().map(|id|find(&after,"clips",id.as_str().unwrap())["track"].as_str().unwrap()).collect::<Vec<_>>();
    assert_eq!(ordered,vec!["m2","m1","c"]);
    assert_eq!(result["selected_clip"],created[2]);
    for id in created {
        let c=find(&after,"clips",id.as_str().unwrap());
        close(&c["start_seconds"],1.25);close(&c["duration_seconds"],3.375);
        if c["kind"]=="midi" {close(&c["bpm"],120.);close(&c["length_quarters"],6.75);assert_eq!(c["notes"],json!([]));assert_eq!(c["meter"],json!({"numerator":4,"denominator":4}));}
        else {assert_eq!(c["points"],json!([]));}
    }
}

#[test]
fn range_clips_overlap_on_later_target_rejects_entire_kind_without_mutation() {
    let mut s=State::default();track(&mut s,"first","midi");track(&mut s,"later","midi");
    edit(&mut s,"PutClip",midi("occupied","later",1.5,120.,1.,json!([]))).unwrap();
    let before=snapshot(&mut s);let result=create_range(&mut s,"first","later",1.,2.);
    assert_eq!(result["created"],json!([]));assert!(result["selected_clip"].is_null());
    assert_eq!(result["rejected"].as_array().unwrap().len(),1);assert_eq!(result["rejected"][0]["kind"],"midi");
    assert!(result["rejected"][0]["reason"].as_str().unwrap().contains("occupied"));
    assert_eq!(snapshot(&mut s),before);
}

#[test]
fn range_clips_touching_endpoints_are_legal_and_existing_clips_unchanged() {
    for kind in ["midi","cc"] {
        let mut s=State::default();track(&mut s,"t",kind);
        for (id,start) in [("left",0.),("right",2.)] {
            let request=if kind=="midi" {midi(id,"t",start,120.,2.,json!([]))}else{empty_cc(id,"t",start,1.)};
            edit(&mut s,"PutClip",request).unwrap();
        }
        let before=snapshot(&mut s);let result=create_range(&mut s,"t","t",1.,2.);let after=snapshot(&mut s);
        assert_eq!(result["created"].as_array().unwrap().len(),1);assert_eq!(result["rejected"],json!([]));
        assert_eq!(after["revision"].as_u64().unwrap(),before["revision"].as_u64().unwrap()+1);
        for id in ["left","right"] {assert_eq!(find(&after,"clips",id),find(&before,"clips",id));}
    }
}

#[test]
fn range_clips_midi_and_cc_reject_independently() {
    for blocked in ["midi","cc"] {
        let mut s=State::default();track(&mut s,"m","midi");track(&mut s,"c","cc");
        let request=if blocked=="midi" {midi("existing","m",1.25,120.,1.,json!([]))}else{empty_cc("existing","c",1.25,0.5)};
        edit(&mut s,"PutClip",request).unwrap();let before=snapshot(&mut s);
        let result=create_range(&mut s,"m","c",1.,2.);let after=snapshot(&mut s);
        assert_eq!(result["created"].as_array().unwrap().len(),1);assert_eq!(result["rejected"][0]["kind"],blocked);
        let id=result["created"][0].as_str().unwrap();assert_eq!(result["selected_clip"],id);
        assert_ne!(find(&after,"clips",id)["kind"],blocked);
        assert_eq!(after["revision"].as_u64().unwrap(),before["revision"].as_u64().unwrap()+1);
        assert_eq!(find(&after,"clips","existing"),find(&before,"clips","existing"));
    }
}

#[test]
fn range_clips_invalid_or_stale_requests_and_audio_only_do_not_mutate() {
    let mut s=State::default();track(&mut s,"m","midi");track(&mut s,"a","audio");
    let valid=json!({"first_track":"m","last_track":"m","start_seconds":0.,"end_seconds":1.});
    for (key,value) in [("first_track",Value::Null),("first_track",json!("unknown")),("last_track",json!("")),
        ("last_track",json!(1)),("start_seconds",json!(-1.)),("start_seconds",json!("0")),
        ("end_seconds",Value::Null),("end_seconds",json!(86400.1)),("end_seconds",json!(0.)),("end_seconds",json!(0.0000005))] {
        let mut request=valid.clone();request[key]=value;rejected_without_change(&mut s,"CreateRangeClips",request);
    }
    let mut missing=valid.clone();missing.as_object_mut().unwrap().remove("end_seconds");rejected_without_change(&mut s,"CreateRangeClips",missing);
    rejected_without_change(&mut s,"CreateRangeClips",json!({"first_track":"a","last_track":"a","start_seconds":0.,"end_seconds":1.}));
    let before=snapshot(&mut s);let mut stale=valid;stale["document_revision"]=json!(before["revision"].as_u64().unwrap()-1);
    assert!(dispatch(&mut s,"CreateRangeClips",&stale).is_err());assert_eq!(snapshot(&mut s),before);
    stale.as_object_mut().unwrap().remove("document_revision");assert!(dispatch(&mut s,"CreateRangeClips",&stale).is_err());assert_eq!(snapshot(&mut s),before);
}

#[test]
fn range_clips_capacity_preflight_prevents_partial_same_kind_and_allows_other_kind() {
    let mut s=State::default();for id in ["m1","m2","m3"]{track(&mut s,id,"midi");}track(&mut s,"c","cc");
    for i in 0..510 {let request=json!({"document_revision":s.revision,"id":format!("old-{i}"),"track":"m1","kind":"midi","start_seconds":0.,"bpm":120.,"length_quarters":2.,"notes":[]});dispatch(&mut s,"PutClip",&request).unwrap();}
    let before=snapshot(&mut s);let result=create_range(&mut s,"m1","c",10.,11.);let after=snapshot(&mut s);
    assert_eq!(result["rejected"][0]["kind"],"midi");assert_eq!(result["created"].as_array().unwrap().len(),1);
    assert_eq!(after["clips"].as_array().unwrap().len(),511);assert_eq!(after["revision"].as_u64().unwrap(),before["revision"].as_u64().unwrap()+1);
    assert_eq!(find(&after,"clips",result["created"][0].as_str().unwrap())["track"],"c");
    assert_eq!(&after["clips"].as_array().unwrap()[..510],before["clips"].as_array().unwrap().as_slice());
    let full=create_range(&mut s,"c","c",12.,13.);assert_eq!(full["created"].as_array().unwrap().len(),1);
    let before=snapshot(&mut s);let rejected=create_range(&mut s,"m1","c",14.,15.);
    assert_eq!(rejected["created"],json!([]));assert_eq!(rejected["rejected"].as_array().unwrap().len(),2);assert_eq!(snapshot(&mut s),before);
}

#[test]
fn range_clips_identity_collisions_do_not_overwrite_previous_clips() {
    let mut s=State::default();track(&mut s,"m","midi");
    for id in ["range-clip-1","range-clip-3"]{edit(&mut s,"PutClip",midi(id,"m",0.,120.,2.,json!([]))).unwrap();}
    let before=snapshot(&mut s);let first=create_range(&mut s,"m","m",4.,5.);let second=create_range(&mut s,"m","m",6.,7.);let after=snapshot(&mut s);
    assert_eq!(first["created"],json!(["range-clip-2"]));assert_eq!(second["created"],json!(["range-clip-4"]));
    for id in ["range-clip-1","range-clip-3"]{assert_eq!(find(&after,"clips",id),find(&before,"clips",id));}
}

#[test]
fn range_clips_validate_local_midi_limit_without_clamping_cc_and_minimum_duration() {
    let mut s=State::default();track(&mut s,"m","midi");track(&mut s,"c","cc");
    let before=snapshot(&mut s);let result=create_range(&mut s,"m","c",0.,86400.);let after=snapshot(&mut s);
    assert_eq!(result["rejected"][0]["kind"],"midi");assert_eq!(result["created"].as_array().unwrap().len(),1);
    let clip=find(&after,"clips",result["created"][0].as_str().unwrap());close(&clip["duration_seconds"],86400.);assert_eq!(clip["points"],json!([]));
    assert_eq!(after["revision"].as_u64().unwrap(),before["revision"].as_u64().unwrap()+1);
    let mut s=State::default();track(&mut s,"m","midi");let result=create_range(&mut s,"m","m",0.,0.000001);let after=snapshot(&mut s);
    close(&find(&after,"clips",result["created"][0].as_str().unwrap())["duration_seconds"],0.000001);
    let mut s=State::default();track(&mut s,"m","midi");let result=create_range(&mut s,"m","m",0.,32768.);let after=snapshot(&mut s);
    close(&find(&after,"clips",result["created"][0].as_str().unwrap())["length_quarters"],65536.);
}


#[test]
fn range_clips_decimal_midi_touch_does_not_become_an_overlap_after_conversion() {
    let mut s=State::default();track(&mut s,"m","midi");
    let first=create_range(&mut s,"m","m",0.01,0.28);
    assert_eq!(first["created"].as_array().unwrap().len(),1);
    // 0.54 * 60 / 120 plus 0.01 reconstructs 0.2800000000000001.
    // The second request touches the exact first requested endpoint.
    let second=create_range(&mut s,"m","m",0.28,0.5);
    assert_eq!(second["rejected"],json!([]));
    assert_eq!(second["created"].as_array().unwrap().len(),1);
}

#[test]
fn range_clips_decimal_touch_tolerance_does_not_hide_more_than_two_ulps() {
    let mut s=State::default();track(&mut s,"m","midi");
    create_range(&mut s,"m","m",0.01,0.28);
    let before=snapshot(&mut s);
    // Reconstructed old end is one ULP above 0.28. Two ULP below
    // 0.28 is a three-ULP intersection and must still be rejected.
    let start=f64::from_bits(0.28_f64.to_bits()-2);
    let overlap=create_range(&mut s,"m","m",start,0.5);
    assert_eq!(overlap["created"],json!([]));
    assert_eq!(overlap["rejected"][0]["kind"],"midi");
    assert_eq!(snapshot(&mut s),before);
}


#[test]
fn track_io_document_states_clear_and_live_audio_follow_cardinal() {
    let mut state=State::default();track(&mut state,"audio","audio");track(&mut state,"midi","midi");
    let doc=snapshot(&mut state);
    assert_eq!(find(&doc,"tracks","audio")["input_monitor_mode"],0);
    assert_eq!(find(&doc,"tracks","midi")["input_monitor_mode"],2);
    rejected_without_change(&mut state,"SetTrackMix",json!({"id":"audio","gain":2.,"arm_mode":4}));
    rejected_without_change(&mut state,"SetTrackMix",json!({"id":"midi","physical_input_index":0}));
    edit(&mut state,"PutSource",json!({"id":"pcm","sample_rate":8000,"channels":1,"samples":[0.25,0.5]})).unwrap();
    edit(&mut state,"PutClip",audio("a","audio",0.,0.00025)).unwrap();
    edit(&mut state,"SetTrackMix",json!({"id":"audio","output_bus_l":"L","output_bus_r":"R","arm_mode":3,"input_monitor_mode":1})).unwrap();
    // An explicit connection-owned input block enables monitoring; exports do not carry one.
    let revision=snapshot(&mut state)["revision"].clone();
    let live=json!({"document_revision":revision,"sample_rate":8000,"start_frame":0,"frames":2,
        "live_inputs":{"physical":[[1.,2.],[3.,4.]],"virtual_mic":[5.,6.]}});
    let result=dispatch(&mut state,"RenderRange",&live).unwrap();
    assert_eq!(result["lsffr"],json!([[1.,0.,0.,1.],[2.,0.,0.,2.]]));
    let result=render(&mut state,2);assert_eq!(result["lsffr"],json!([[0.25,0.,0.,0.25],[0.5,0.,0.,0.5]]));
    edit(&mut state,"SetTrackMix",json!({"id":"audio","input_monitor_mode":0})).unwrap();
    let mut live=live;live["document_revision"]=snapshot(&mut state)["revision"].clone();
    assert_eq!(dispatch(&mut state,"RenderRange",&live).unwrap()["lsffr"],result["lsffr"]);
    edit(&mut state,"PutClip",midi("m","midi",0.,120.,1.,json!([]))).unwrap();
    edit(&mut state,"SetTrackMix",json!({"id":"midi","clear_midi_contents":true})).unwrap();
    let doc=snapshot(&mut state);assert_eq!(doc["clips"].as_array().unwrap().len(),1);assert_eq!(doc["clips"][0]["id"],"a");
}
