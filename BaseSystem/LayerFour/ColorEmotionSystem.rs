//! The game's two color emotions, driven by a layer-five interaction Audicle.
use crate::host::Host;
use serde_json::{Value, json};
#[derive(Default)]
pub(crate) struct State {
    entity: Option<u64>,
    time: f64,
    intensity: f64,
    fire: f64,
    mode: String,
    charge: f64,
}
pub(crate) fn valid(v: &Value) -> bool {
    v["dt"]
        .as_f64()
        .is_some_and(|x| x.is_finite() && (0.0..=0.05).contains(&x))
        && v["charge"]
            .as_f64()
            .is_some_and(|x| x.is_finite() && (0.0..=1.).contains(&x))
        && v["trigger"].is_boolean()
        && matches!(v["mode"].as_str(), Some("dig" | "pickup"))
}
pub(crate) fn valid_view(v: &Value) -> bool {
    v["active"].is_boolean()
        && (v["entity"].is_null() || v["entity"].as_u64().is_some_and(|x| x > 0))
}
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let mut s = std::mem::take(&mut host.color_emotion);
    let result = (|| {
        if hook == "Release" {
            if let Some(id) = s.entity {
                host.invoke("EntitySystem.Remove", json!({"entity":id}))?;
            }
            s = State::default();
            return Ok(Value::Null);
        }
        if hook == "Active" {
            return Ok(json!(s.intensity >= 0.001 || s.fire > 0.));
        }
        if hook != "Advance" {
            return Err("unknown color emotion hook".into());
        }
        let dt = input["dt"].as_f64().unwrap();
        s.time = (s.time + dt).rem_euclid(std::f64::consts::TAU * 5.);
        s.fire = (s.fire - dt).max(0.);
        if input["trigger"] == true {
            s.fire = 1.;
        }
        let charge = input["charge"].as_f64().unwrap();
        if charge > 0. || input["trigger"] == true || s.mode.is_empty() {
            s.mode = input["mode"].as_str().unwrap().to_owned();
        }
        if charge > 0. {
            s.charge = charge;
        } else if s.fire > 0. {
            s.charge = 1.;
        }
        let target = if charge > 0. {
            0.08 + charge * 0.86
        } else {
            s.fire * 0.34
        };
        s.intensity += (target - s.intensity) * (dt * 8.5).clamp(0., 1.);
        if s.intensity < 0.001 && s.fire == 0. && charge == 0. {
            if let Some(id) = s.entity.take() {
                host.invoke("EntitySystem.Remove", json!({"entity":id}))?;
            }
            return Ok(json!({"entity":null,"active":false}));
        }
        let mut appearance = json!({"kind":"visual","shader":"color_emotion","vertices":[[-1.,-1.,0.,1.,1.,1.],[1.,-1.,0.,1.,1.,1.],[1.,1.,0.,1.,1.,1.],[-1.,-1.,0.,1.,1.,1.],[1.,1.,0.,1.,1.,1.],[-1.,1.,0.,1.,1.,1.]],"procedural":{"intensity":s.intensity,"time":s.time,"charge":s.charge,"mode":s.mode}});
        if let Some(id) = s.entity {
            appearance["entity"] = json!(id);
            host.invoke("EntitySystem.Update", appearance)?;
        } else {
            s.entity = host.invoke("EntitySystem.Create", appearance)?["entity"].as_u64();
        }
        Ok(json!({"entity":s.entity,"active":true}))
    })();
    host.color_emotion = s;
    result
}
