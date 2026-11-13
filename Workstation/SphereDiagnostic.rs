//! Explicit finite diagnostic: real PCM goes through the normal source mixer.
//! Never called by ordinary workstation launch and never starts an audio device.
use crate::host::Host;
use serde_json::{Value, json};

pub(crate) fn prepare(host: &mut Host) -> Result<(), String> {
    let sample_rate = 48_000.;
    for (index, (frequency, amplitude, balance, wide)) in [
        (140., 0.42, -0.55, false),
        (1400., 0.18, 0., true),
        (6200., 0.045, 0.65, false),
    ].into_iter().enumerate() {
        let snapshot = host.invoke("WorkstationDataSystem.Snapshot", Value::Null)?;
        let track = (1..).map(|n| format!("sphere-check-{index}-{n}"))
            .find(|id| [("tracks",id.clone()),("sources",format!("{id}-pcm")),("clips",format!("{id}-clip"))]
                .iter().all(|(key,candidate)| !snapshot[*key].as_array().is_some_and(|a|
                    a.iter().any(|v| v["id"].as_str() == Some(candidate.as_str()))))).unwrap();
        let source = format!("{track}-pcm");
        let clip = format!("{track}-clip");
        edit(host, "PutTrack", json!({"id":track,"name":format!("Sphere PCM {}",index+1),"kind":"audio"}))?;
        let samples: Vec<f32> = (0..4096).flat_map(|frame| {
            let t = frame as f64 / sample_rate;
            let wave = |hz: f64| (t * hz * std::f64::consts::TAU).sin();
            let left = amplitude * wave(frequency) * (1.0_f64-balance).min(1.);
            let right = amplitude * wave(if wide {frequency*1.37} else {frequency})
                * (1.0_f64+balance).min(1.);
            [left as f32, right as f32]
        }).collect();
        edit(host, "PutSource", json!({"id":source,"sample_rate":48000,"channels":2,"samples":samples}))?;
        edit(host, "PutClip", json!({"id":clip,"track":track,"kind":"audio", "source":source,
            "start_seconds":0.,"source_offset_seconds":0.,"duration_seconds":4096./sample_rate}))?;
        edit(host, "SetTrackMix", json!({"id":track,"output_bus_l":"L","output_bus_r":"R"}))?;
    }
    publish(host)
}

fn edit(host: &mut Host, hook: &str, mut input: Value) -> Result<(), String> {
    input["document_revision"] = host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?["revision"].clone();
    host.invoke(&format!("WorkstationDataSystem.{hook}"), input)?;
    Ok(())
}

pub(crate) fn publish(host: &mut Host) -> Result<(), String> {
    let revision = host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?["revision"].clone();
    host.invoke("WorkstationDataSystem.RenderRange",json!({"document_revision":revision,
        "sample_rate":48000,"start_frame":0,"frames":4096,"publish_visualization":true}))?;
    Ok(())
}
