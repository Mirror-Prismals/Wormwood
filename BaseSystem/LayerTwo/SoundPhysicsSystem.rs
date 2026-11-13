//! Stateless distance/head response after ray publication, refreshed with current view.
use super::ray_traced_audio_system::{listener, response_value};
use crate::audio::{
    acoustic::{self, Response},
    vector,
};
use serde_json::{Value, json};
pub(crate) fn dispatch(hook: &str, input: &Value) -> Result<Value, String> {
    if hook != "Apply" && hook != "ApplyMicrophone" {
        return Err("unknown sound physics hook".into());
    }
    let l = listener(&input["listener"])?;
    let rate = input["sample_rate"]
        .as_f64()
        .filter(|n| n.is_finite() && (8000.0..=192000.0).contains(n))
        .ok_or("valid audio rate required")?;
    let rows = input["sources"]
        .as_array()
        .ok_or("acoustic sources required")?;
    if rows.len() > 17 {
        return Err("sound source budget exceeded".into());
    }
    let mut out = Vec::new();
    for x in rows {
        let number = |k: &str| {
            x[k].as_f64()
                .filter(|n| n.is_finite())
                .ok_or_else(|| format!("sound {k} required"))
        };
        let mut r = Response {
            id: x["id"].as_u64().ok_or("sound id required")?,
            channel: x["channel"]
                .as_u64()
                .filter(|n| *n < 17)
                .ok_or("sound channel required")? as usize,
            source: vector(&x["position"])?,
            gain: number("gain")?,
            pre_falloff: number("pre_falloff")?,
            occluded: x["occluded"].as_bool().ok_or("occluded flag required")?,
            direction: vector(&x["direction"])?,
            pan: 0.,
            hf_alpha: 0.,
            echo_delay: number("echo_delay")?,
            echo_gain: number("echo_gain")?,
            escape: number("escape")?,
        };
        acoustic::head_response(&mut r, l, rate);
        if hook == "ApplyMicrophone" {
            let direction = crate::audio::unit(if crate::audio::length(r.direction) > 1e-4 {
                r.direction
            } else {
                crate::audio::sub(r.source, l.position)
            });
            // Match applyMicCardioid: a coincident source with no measured
            // direction keeps its distance gain instead of losing half its level.
            if crate::audio::length(direction) > 1e-4 {
                r.gain *= (0.5
                    * (1. + crate::audio::dot(direction, crate::audio::unit(l.forward))))
                .clamp(0., 1.);
            }
            r.pan = 0.;
            let distance = crate::audio::length(crate::audio::sub(r.source, l.position));
            let cutoff = 18000. * (7500_f64 / 18000.).powf(((distance - 4.) / 56.).clamp(0., 1.));
            r.hf_alpha = 1. - (-2. * std::f64::consts::PI * cutoff / rate).exp();
        }
        out.push(response_value(&r));
    }
    Ok(json!({"sources":out}))
}

#[cfg(test)]
mod microphone_tests {
    use super::*;
    #[test]
    fn virtual_microphone_cardioid_rejects_rear_without_stereo_pan() {
        let source=|id,position,direction|json!({"id":id,"channel":id,"position":position,"gain":1.,"pre_falloff":1.,"occluded":false,"direction":direction,"echo_delay":0.,"echo_gain":0.,"escape":1.});
        let value=dispatch("ApplyMicrophone",&json!({"listener":{"position":[0.,0.,0.],"forward":[0.,0.,-1.],"up":[0.,1.,0.]},"sample_rate":48000,"sources":[source(0,[0.,0.,-2.],[0.,0.,-1.]),source(1,[0.,0.,2.],[0.,0.,1.])]})).unwrap();
        assert!(value["sources"][0]["gain"].as_f64().unwrap()>0.9);
        assert_eq!(value["sources"][1]["gain"],0.);
        for row in value["sources"].as_array().unwrap(){assert_eq!(row["pan"],0.);}
    }
}
