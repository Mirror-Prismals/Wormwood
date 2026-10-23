//! Game movement in a homogeneous medium or around an analytic planet.
use crate::host::Host;
use serde_json::{Value, json};
const PLANET: bool = true;
const RADIUS: f64 = 6_371_000.;
type V = [f64; 3];
fn dot(a: V, b: V) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn normalized(a: V) -> V {
    let n = dot(a, a).sqrt().max(1e-12);
    a.map(|x| x / n)
}
fn rotate(v: V, axis: V, angle: f64) -> V {
    let (s, c) = angle.sin_cos();
    let p = cross(axis, v);
    let d = dot(axis, v);
    std::array::from_fn(|i| v[i] * c + p[i] * s + axis[i] * d * (1. - c))
}
pub(crate) struct State {
    position: V,
    velocity: V,
    right: V,
    up: V,
    forward: V,
    event: Option<Value>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            position: if PLANET {
                [0., RADIUS + 2., 0.]
            } else {
                [0.; 3]
            },
            velocity: [0.; 3],
            right: [1., 0., 0.],
            up: if PLANET {
                [0., 0.9928086359, -0.1197122073]
            } else {
                [0., 1., 0.]
            },
            forward: if PLANET {
                [0., -0.1197122073, -0.9928086359]
            } else {
                [0., 0., -1.]
            },
            event: None,
        }
    }
}
pub(crate) fn valid(v: &Value) -> bool {
    crate::base_system::layer_five::flight_system::valid(v)
        && v["roll"]
            .as_f64()
            .is_some_and(|x| x.is_finite() && x.abs() <= 1.)
        && v["boost"].is_boolean()
}
impl State {
    fn advance(&mut self, input: &Value) {
        if input["reset"] == true {
            let event = self.event.clone();
            *self = Self::default();
            self.event = event;
            return;
        }
        let dt = input["dt"].as_f64().unwrap().min(0.05);
        let yaw = -input["look"][0].as_f64().unwrap() * 0.003;
        self.forward = rotate(self.forward, self.up, yaw);
        self.right = rotate(self.right, self.up, yaw);
        let pitch = -input["look"][1].as_f64().unwrap() * 0.003;
        self.forward = rotate(self.forward, self.right, pitch);
        self.up = rotate(self.up, self.right, pitch);
        let roll = input["roll"].as_f64().unwrap() * dt * 1.5;
        self.up = rotate(self.up, self.forward, roll);
        self.right = normalized(cross(self.forward, self.up));
        self.forward = normalized(self.forward);
        self.up = normalized(cross(self.right, self.forward));
        let k = &input["move"];
        let axis = |a: usize, b: usize| f64::from(k[a] == true) - f64::from(k[b] == true);
        let (f, s, u) = (axis(0, 1), axis(3, 2), axis(4, 5));
        let vertical = if PLANET {
            normalized(self.position)
        } else {
            self.up
        };
        let direction: V =
            std::array::from_fn(|i| f * self.forward[i] + s * self.right[i] + u * vertical[i]);
        let length = dot(direction, direction).sqrt().max(1.);
        if PLANET {
            let altitude = dot(self.position, self.position).sqrt() - RADIUS;
            let speed = (10. + altitude.max(0.) * 0.6).min(3_000_000.)
                * if input["boost"] == true { 100. } else { 1. };
            self.velocity = direction.map(|v| v / length * speed);
        } else {
            // Neutral buoyancy, thrust, and water drag; no global gravity or floor.
            let decay = (-3. * dt).exp();
            for (i, d) in direction.iter().enumerate() {
                self.velocity[i] =
                    self.velocity[i] * decay + (d / length) * (6. / 3.) * (1. - decay);
            }
            if dot(self.velocity, self.velocity) < 1e-6 {
                self.velocity = [0.; 3];
            }
        }
        for i in 0..3 {
            self.position[i] += self.velocity[i] * dt;
        }
        if PLANET && dot(self.position, self.position).sqrt() < RADIUS + 2. {
            let n = normalized(self.position);
            self.position = n.map(|v| v * (RADIUS + 2.));
        }
    }
    fn snapshot(&self) -> Value {
        json!({"position":self.position,"velocity":self.velocity,"right":self.right,"up":self.up,"forward":self.forward,"moving":self.event.is_some(),"medium":if PLANET{"planet"}else{"water"}})
    }
}
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let mut state = std::mem::take(&mut host.environment);
    let result = (|| {
        if hook == "Stop" {
            if let Some(event) = &state.event {
                host.invoke("AudicleSystem.Finish", event.clone())?;
                state.event = None;
            }
            return Ok(Value::Null);
        }
        let active = input["reset"] == true
            || input["move"].as_array().unwrap().iter().any(|v| v == true)
            || input["look"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_f64() != Some(0.))
            || input["roll"].as_f64() != Some(0.)
            || (!PLANET && dot(state.velocity, state.velocity) > 0.);
        if active {
            if state.event.is_none() {
                let event = host.invoke("AudicleSystem.Create", Value::Null)?;
                state.event = Some(event.clone());
                host.invoke("AudicleSystem.Ready", event.clone())?;
                host.invoke("AudicleSystem.Go", event)?;
            }
            state.advance(input);
            host.invoke("AppearanceSystem.View",json!({"entity":input["entity"],"position":state.position,"right":state.right,"up":state.up,"forward":state.forward,"yaw":0.,"pitch":0.}))?;
            host.emit(
                json!({"event":"environment_step","state":state.snapshot(),"reset":input["reset"]}),
            )?;
        } else if let Some(event) = &state.event {
            host.invoke("AudicleSystem.Finish", event.clone())?;
            state.event = None;
        }
        Ok(state.snapshot())
    })();
    host.environment = state;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> Value {
        json!({"entity":1,"dt":0.05,"move":[false,false,false,false,false,false],"look":[0,0],"reset":false,"roll":0.,"boost":false})
    }
    #[test]
    fn rotation_preserves_an_orthonormal_personal_frame() {
        let mut s = State::default();
        let mut i = input();
        i["roll"] = json!(1.);
        for _ in 0..400 {
            s.advance(&i);
        }
        assert!(dot(s.right, s.up).abs() < 1e-10);
        assert!((dot(s.up, s.up) - 1.).abs() < 1e-10);
        assert!(s.up[0].abs() > 0.1);
    }
    #[test]
    fn movement_obeys_the_environment_boundary() {
        let mut s = State::default();
        let mut i = input();
        i["move"][5] = json!(true);
        for _ in 0..100 {
            s.advance(&i);
        }
        if PLANET {
            assert!(dot(s.position, s.position).sqrt() >= RADIUS + 1.999);
        } else {
            assert!(s.position[1] < -1.);
            i["move"][5] = json!(false);
            let speed = dot(s.velocity, s.velocity);
            s.advance(&i);
            assert!(dot(s.velocity, s.velocity) < speed);
        }
    }
}
