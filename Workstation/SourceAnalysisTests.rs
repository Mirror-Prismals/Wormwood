use super::document::{State, dispatch};
use serde_json::{Value, json};

const RATE: u32 = 8192;

fn edit(state: &mut State, hook: &str, mut input: Value) {
    input["document_revision"] =
        dispatch(state, "Snapshot", &Value::Null).unwrap()["revision"].clone();
    dispatch(state, hook, &input).unwrap();
}

fn add_audio(state: &mut State, id: &str, channels: usize, samples: Vec<f32>) {
    let frames = samples.len() / channels;
    edit(state, "PutTrack", json!({"id":id,"name":id,"kind":"audio"}));
    edit(
        state,
        "PutSource",
        json!({"id":id,"sample_rate":RATE,"channels":channels,"samples":samples}),
    );
    edit(
        state,
        "PutClip",
        json!({"id":id,"track":id,"kind":"audio","start_seconds":0.,
        "source":id,"source_offset_seconds":0.,"duration_seconds":frames as f64/RATE as f64}),
    );
}

fn request(state: &mut State, frames: usize) -> Value {
    json!({"document_revision":dispatch(state,"Snapshot",&Value::Null).unwrap()["revision"],
        "sample_rate":RATE,"start_frame":0,"frames":frames,"analyze_visualization":true})
}

fn render(state: &mut State, frames: usize) -> Value {
    let input = request(state, frames);
    dispatch(state, "RenderRange", &input).unwrap()
}

fn number(value: &Value) -> f64 {
    value.as_f64().unwrap()
}
fn near(value: &Value, expected: f64, tolerance: f64) {
    assert!(
        (number(value) - expected).abs() <= tolerance,
        "{value} != {expected}"
    );
}

fn tone(frequency: f64, frames: usize) -> Vec<f32> {
    (0..frames)
        .map(|i| (std::f64::consts::TAU * frequency * i as f64 / RATE as f64).sin() as f32 * 0.25)
        .collect()
}

#[test]
fn source_rms_tracks_actual_pcm_gain_and_energy_balance_before_routing() {
    let mut state = State::default();
    add_audio(&mut state, "source", 2, [0.25f32, 0.5].repeat(64));
    edit(&mut state, "SetTrackMix", json!({"id":"source","gain":2.}));
    let result = render(&mut state, 64);
    let source = &result["visualization"]["tracks"][0];
    near(&source["rms_pre_gain"], (0.15625f64).sqrt(), 1e-9);
    near(&source["rms_post_gain"], (0.625f64).sqrt(), 1e-9);
    near(&source["rms_l"], 0.5, 1e-9);
    near(&source["rms_r"], 1., 1e-9);
    near(&source["stereo_balance"], 0.6, 1e-9);
    near(&source["stereo_width"], 0., 1e-9);
    near(&source["effective_gain"], 2., 1e-9);
    assert_eq!(source["manual_gain"], 2.);
    assert_eq!(source["automated"], false);
    // Default FF/FF folds to mono without destroying the measured source image.
    assert_eq!(result["master_stereo"][0], json!([0.75, 0.75]));
    edit(
        &mut state,
        "SetTrackMix",
        json!({"id":"source","output_bus_l":"OFF","output_bus_r":"OFF"}),
    );
    let rerouted = render(&mut state, 64);
    // Source analysis stays pre-routing; the new Cardinal mixer meter is
    // deliberately post-routing and becomes silent with both buses OFF.
    let mut original_tracks=result["visualization"]["tracks"].clone();
    let mut rerouted_tracks=rerouted["visualization"]["tracks"].clone();
    assert_eq!(original_tracks[0]["meter_peak"],1.5);
    assert_eq!(rerouted_tracks[0]["meter_peak"],0.);
    for tracks in [&mut original_tracks,&mut rerouted_tracks] {
        for track in tracks.as_array_mut().unwrap(){track.as_object_mut().unwrap().remove("meter_peak");}
    }
    assert_eq!(rerouted_tracks,original_tracks);
    assert_eq!(rerouted["master_stereo"][0], json!([0., 0.]));
}

#[test]
fn frequency_uses_both_channel_powers_and_preserves_antiphase_audio() {
    let mut state = State::default();
    let low = tone(256., 1024);
    let high = tone(1024., 1024);
    add_audio(&mut state, "mono", 1, low.clone());
    add_audio(
        &mut state,
        "opposed",
        2,
        low.iter().flat_map(|v| [*v, -*v]).collect(),
    );
    add_audio(
        &mut state,
        "wide",
        2,
        low.iter().zip(&high).flat_map(|(a, b)| [*a, *b]).collect(),
    );
    add_audio(&mut state, "high", 1, high);
    let result = render(&mut state, 1024);
    let tracks = &result["visualization"]["tracks"];
    near(&tracks[0]["stereo_width"], 0., 1e-6);
    near(&tracks[1]["stereo_width"], 1., 1e-6);
    near(&tracks[2]["stereo_width"], 0.5f64.sqrt(), 1e-5);
    near(&tracks[0]["spectral_centroid_hz"], 256., 1.);
    near(&tracks[1]["spectral_centroid_hz"], 256., 1.);
    near(&tracks[2]["spectral_centroid_hz"], 640., 1.);
    near(&tracks[3]["spectral_centroid_hz"], 1024., 1.);
    near(
        &tracks[1]["rms_post_gain"],
        number(&tracks[0]["rms_post_gain"]),
        1e-9,
    );
}

#[test]
fn silence_single_channel_and_short_ranges_have_defined_metrics() {
    let mut state = State::default();
    add_audio(&mut state, "silent", 1, vec![0.; 64]);
    add_audio(&mut state, "left", 2, [0.25f32, 0.].repeat(64));
    let result = render(&mut state, 64);
    let tracks = &result["visualization"]["tracks"];
    for key in [
        "rms_pre_gain",
        "rms_post_gain",
        "rms_l",
        "rms_r",
        "stereo_balance",
        "stereo_width",
    ] {
        near(&tracks[0][key], 0., 0.);
    }
    assert!(tracks[0]["spectral_centroid_hz"].is_null());
    near(&tracks[1]["stereo_balance"], -1., 0.);
    near(&tracks[1]["stereo_width"], 0., 0.);
    assert!(render(&mut state, 4)["visualization"]["tracks"][1]["spectral_centroid_hz"].is_null());
}

#[test]
fn mute_and_global_solo_suppress_source_observations() {
    let mut state = State::default();
    add_audio(&mut state, "first", 1, vec![0.25; 64]);
    add_audio(&mut state, "second", 1, vec![0.5; 64]);
    edit(&mut state, "SetTrackMix", json!({"id":"first","mute":true}));
    let muted = render(&mut state, 64);
    assert_eq!(muted["visualization"]["tracks"][0]["audible"], false);
    near(&muted["visualization"]["tracks"][0]["rms_pre_gain"], 0., 0.);
    near(
        &muted["visualization"]["tracks"][0]["rms_post_gain"],
        0.,
        0.,
    );
    edit(
        &mut state,
        "SetTrackMix",
        json!({"id":"first","mute":false}),
    );
    edit(
        &mut state,
        "SetTrackMix",
        json!({"id":"second","solo":true}),
    );
    let solo = render(&mut state, 64);
    near(&solo["visualization"]["tracks"][0]["rms_post_gain"], 0., 0.);
    near(
        &solo["visualization"]["tracks"][1]["rms_post_gain"],
        0.5,
        0.,
    );
}

#[test]
fn automation_observation_reports_applied_gain_without_changing_manual_fader() {
    let mut state = State::default();
    add_audio(&mut state, "audio", 1, vec![0.25; 1024]);
    edit(&mut state, "SetTrackMix", json!({"id":"audio","gain":0.25}));
    edit(
        &mut state,
        "PutTrack",
        json!({"id":"cc","name":"CC","kind":"cc"}),
    );
    edit(
        &mut state,
        "SetCcTarget",
        json!({"id":"cc","target":{"track":"audio","parameter":"gain"}}),
    );
    edit(
        &mut state,
        "PutClip",
        json!({"id":"automation","track":"cc","kind":"cc","start_seconds":0.,
        "duration_seconds":1024./RATE as f64,"points":[{"offset_seconds":0.,"value":1.}]}),
    );
    let output = render(&mut state, 1024);
    let track = &output["visualization"]["tracks"][0];
    near(&track["effective_gain"], 10f64.powf(5. / 20.), 1e-6);
    near(&track["manual_gain"], 0.25, 0.);
    assert_eq!(track["automated"], true);
    assert_eq!(
        output["visualization"]["tracks"].as_array().unwrap().len(),
        1
    );
    let mut after = request(&mut state, 64);
    after["start_frame"] = json!(1024);
    let gap = dispatch(&mut state, "RenderRange", &after).unwrap();
    assert_eq!(gap["visualization"]["tracks"][0]["automated"], false);
    near(
        &gap["visualization"]["tracks"][0]["effective_gain"],
        0.25,
        0.,
    );
}

#[test]
fn visualization_publication_is_explicit_small_revision_checked_and_invalidated_on_edit() {
    let mut state = State::default();
    add_audio(&mut state, "audio", 1, vec![0.25; 64]);
    let before = dispatch(&mut state, "Snapshot", &Value::Null).unwrap();
    assert!(before["visualization"].is_null());
    let mut ordinary = request(&mut state, 64);
    ordinary
        .as_object_mut()
        .unwrap()
        .remove("analyze_visualization");
    let ordinary_result = dispatch(&mut state, "RenderRange", &ordinary).unwrap();
    assert!(ordinary_result["visualization"].is_null());
    assert_eq!(
        dispatch(&mut state, "Snapshot", &Value::Null).unwrap(),
        before
    );
    let pure = render(&mut state, 64);
    assert_eq!(pure["visualization"]["sequence"], 0);
    assert_eq!(
        dispatch(&mut state, "Snapshot", &Value::Null).unwrap(),
        before
    );
    let mut input = request(&mut state, 64);
    input
        .as_object_mut()
        .unwrap()
        .remove("analyze_visualization");
    input["publish_visualization"] = json!(true);
    let published = dispatch(&mut state, "RenderRange", &input).unwrap();
    let snapshot = dispatch(&mut state, "Snapshot", &Value::Null).unwrap();
    assert_eq!(snapshot["revision"], before["revision"]);
    assert_eq!(snapshot["visualization"], published["visualization"]);
    assert_eq!(snapshot["visualization"]["sequence"], 1);
    assert!(snapshot["visualization"].get("samples").is_none());
    assert!(snapshot["visualization"].get("master_stereo").is_none());
    let _ = render(&mut state, 64);
    assert_eq!(
        dispatch(&mut state, "Snapshot", &Value::Null).unwrap(),
        snapshot
    );
    for flag in [Value::Null, json!(1), json!("true")] {
        let mut invalid = input.clone();
        invalid["publish_visualization"] = flag;
        assert!(dispatch(&mut state, "RenderRange", &invalid).is_err());
        assert_eq!(
            dispatch(&mut state, "Snapshot", &Value::Null).unwrap(),
            snapshot
        );
        let mut invalid_analysis = input.clone();
        invalid_analysis["analyze_visualization"] = invalid["publish_visualization"].clone();
        assert!(dispatch(&mut state, "RenderRange", &invalid_analysis).is_err());
        assert_eq!(
            dispatch(&mut state, "Snapshot", &Value::Null).unwrap(),
            snapshot
        );
    }
    let mut bad_rate = input.clone();
    bad_rate["sample_rate"] = json!(16000);
    assert!(dispatch(&mut state, "RenderRange", &bad_rate).is_err());
    assert_eq!(
        dispatch(&mut state, "Snapshot", &Value::Null).unwrap(),
        snapshot
    );
    edit(&mut state, "SetTrackMix", json!({"id":"audio","gain":0.5}));
    let edited = dispatch(&mut state, "Snapshot", &Value::Null).unwrap();
    assert!(edited["visualization"].is_null());
    assert!(dispatch(&mut state, "RenderRange", &input).is_err());
    assert_eq!(
        dispatch(&mut state, "Snapshot", &Value::Null).unwrap(),
        edited
    );
    input["document_revision"] = edited["revision"].clone();
    let fresh = dispatch(&mut state, "RenderRange", &input).unwrap();
    assert_eq!(fresh["visualization"]["sequence"], 2);
}

#[test]
fn midi_without_an_instrument_is_explicitly_unavailable() {
    let mut state = State::default();
    edit(
        &mut state,
        "PutTrack",
        json!({"id":"piano","name":"Piano","kind":"midi"}),
    );
    let output = render(&mut state, 64);
    let track = &output["visualization"]["tracks"][0];
    assert_eq!(track["track"], "piano");
    assert_eq!(track["available"], false);
    assert!(track["rms_post_gain"].is_null());
    assert!(track["spectral_centroid_hz"].is_null());
}

#[test]
fn builtin_midi_observations_measure_synthesized_pcm_and_applied_gain() {
    let mut state = State::default();
    edit(
        &mut state,
        "PutTrack",
        json!({"id":"piano","name":"Piano","kind":"midi"}),
    );
    edit(
        &mut state,
        "PutClip",
        json!({"id":"note","track":"piano","kind":"midi",
        "start_seconds":0.,"bpm":120.,"length_quarters":4.,
        "notes":[{"id":"a","quarter":0.,"length_quarters":2.,"pitch":69,"velocity":1.}]}),
    );
    edit(&mut state, "SetTrackMix", json!({"id":"piano","gain":2.}));
    let mut input = request(&mut state, 1024);
    // Observe the steady sustain after its attack/decay, at the real sample clock.
    input["start_frame"] = json!(1024);
    input["instrument"] = json!("cardinal_sine");
    let output = dispatch(&mut state, "RenderRange", &input).unwrap();
    let track = &output["visualization"]["tracks"][0];
    assert_eq!(track["available"], true);
    assert_eq!(output["midi_instrument_connected"], true);
    assert!(number(&track["rms_pre_gain"]) > 0.03);
    near(&track["effective_gain"], 2., 1e-6);
    near(
        &track["rms_post_gain"],
        number(&track["rms_pre_gain"]) * 2.,
        1e-6,
    );
    near(&track["spectral_centroid_hz"], 440., 1.);
    near(&track["stereo_width"], 0., 1e-6);
    let master_rms = (output["master_stereo"]
        .as_array()
        .unwrap()
        .iter()
        .map(|frame| number(&frame[0]).powi(2))
        .sum::<f64>()
        / 1024.)
        .sqrt();
    near(&track["rms_post_gain"], master_rms, 1e-6);
}
