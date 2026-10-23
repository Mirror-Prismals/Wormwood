//! Application flight behavior. Input activation owns an Audicle, not a frame scheduler.
use crate::host::Host;
use serde_json::{Value, json};
pub(crate) struct State {
    position: [f64; 3],
    yaw: f64,
    pitch: f64,
    event: Option<Value>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            position: [3., 2., 5.],
            yaw: -0.5404195,
            pitch: -0.33042265,
            event: None,
        }
    }
}
impl State {
    fn advance(&mut self, input: &Value) {
        if input["reset"] == true {
            let event = self.event.clone();
            *self = Self::default();
            self.event = event;
            return;
        }
        self.yaw = (self.yaw + input["look"][0].as_f64().unwrap() * 0.003)
            .rem_euclid(std::f64::consts::TAU);
        self.pitch = (self.pitch - input["look"][1].as_f64().unwrap() * 0.003).clamp(-1.553, 1.553);
        let k = &input["move"];
        let axis = |a: usize, b: usize| f64::from(k[a] == true) - f64::from(k[b] == true);
        let (forward, side, up) = (axis(0, 1), axis(3, 2), axis(4, 5));
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let direction = [
            forward * sy * cp + side * cy,
            forward * sp + up,
            -forward * cy * cp + side * sy,
        ];
        let length = direction.iter().map(|v| v * v).sum::<f64>().sqrt().max(1.);
        let scale = 3. * input["dt"].as_f64().unwrap().min(0.05) / length;
        for (i, v) in direction.iter().enumerate() {
            self.position[i] += v * scale;
        }
    }
}
pub(crate) fn valid(input: &Value) -> bool {
    input["entity"].as_u64().is_some_and(|n| n > 0)
        && input["dt"]
            .as_f64()
            .is_some_and(|v| v.is_finite() && (0.0..=1.0).contains(&v))
        && input["move"]
            .as_array()
            .is_some_and(|v| v.len() == 6 && v.iter().all(Value::is_boolean))
        && input["look"].as_array().is_some_and(|v| {
            v.len() == 2
                && v.iter().all(|x| {
                    x.as_f64()
                        .is_some_and(|x| x.is_finite() && x.abs() <= 1000.)
                })
        })
        && input["reset"].is_boolean()
}
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let mut state = std::mem::take(&mut host.flight);
    let result = (|| {
        if hook == "Stop" {
            if let Some(event) = &state.event {
                host.invoke("AudicleSystem.Finish", event.clone())?;
                state.event = None;
            }
            return Ok(Value::Null);
        }
        let moving = input["reset"] == true
            || input["move"].as_array().unwrap().iter().any(|v| v == true)
            || input["look"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_f64() != Some(0.));
        if moving {
            if state.event.is_none() {
                let event = host.invoke("AudicleSystem.Create", Value::Null)?;
                state.event = Some(event.clone());
                host.invoke("AudicleSystem.Ready", event.clone())?;
                host.invoke("AudicleSystem.Go", event)?;
            }
            state.advance(input);
            host.invoke("AppearanceSystem.View",json!({"entity":input["entity"],"position":state.position,"yaw":state.yaw,"pitch":state.pitch}))?;
            host.emit(json!({"event":"flight_step","position":state.position,"yaw":state.yaw,"pitch":state.pitch,"reset":input["reset"]}))?;
        } else if let Some(event) = &state.event {
            host.invoke("AudicleSystem.Finish", event.clone())?;
            state.event = None;
        }
        Ok(
            json!({"position":state.position,"yaw":state.yaw,"pitch":state.pitch,"moving":state.event.is_some()}),
        )
    })();
    host.flight = state;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> Value {
        json!({"entity":1,"dt":0.05,"move":[true,false,false,false,false,false],"look":[0,0],"reset":false})
    }
    #[test]
    fn movement_uses_elapsed_time_and_bounds_stalls() {
        let mut one = State::default();
        let mut two = State::default();
        one.advance(&input());
        let mut half = input();
        half["dt"] = json!(0.025);
        two.advance(&half);
        two.advance(&half);
        for i in 0..3 {
            assert!((one.position[i] - two.position[i]).abs() < 1e-10);
        }
        let mut stalled = State::default();
        let mut long = input();
        long["dt"] = json!(0.9);
        stalled.advance(&long);
        assert_eq!(one.position, stalled.position);
    }
    #[test]
    fn diagonal_speed_pitch_limits_and_reset_are_defined() {
        let mut straight = State::default();
        let mut diagonal = State::default();
        let origin = straight.position;
        straight.advance(&input());
        let mut data = input();
        data["move"][3] = json!(true);
        diagonal.advance(&data);
        let distance = |p: [f64; 3]| {
            p.iter()
                .zip(origin)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                .sqrt()
        };
        assert!((distance(straight.position) - distance(diagonal.position)).abs() < 1e-10);
        data["look"] = json!([0, -1000]);
        diagonal.advance(&data);
        assert!(diagonal.pitch <= 1.553);
        data["reset"] = json!(true);
        diagonal.advance(&data);
        assert_eq!(diagonal.position, origin);
        data["dt"] = json!(-1);
        assert!(!valid(&data));
    }
}
