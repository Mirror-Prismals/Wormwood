//! Per-animal hoof anchors, swing/replant targets and registered IK requests.
use super::behavior::Animal;
use crate::{
    audio::{V, add, cross, dot, length, mul, sub, unit},
    host::Host,
};
use serde_json::{Value, json};
#[derive(Clone, Copy)]
struct Replant {
    start: V,
    target: V,
    progress: f64,
}
#[derive(Clone, Copy, Default)]
struct Foot {
    anchor: Option<V>,
    step: Option<Replant>,
    goal: Option<V>,
    seed: Option<[V; 3]>,
    planted: bool,
}
#[derive(Default)]
pub(crate) struct State {
    feet: [Foot; 4],
    next_leg: usize,
    drop: f64,
}
fn mix(a: V, b: V, t: f64) -> V {
    add(mul(a, 1. - t), mul(b, t))
}
fn floor(host: &mut Host, p: V, eye: V) -> Result<V, String> {
    let height = host.invoke(
        "PlanetSurfaceSystem.Height",
        json!({"position":p,"camera_eye":eye,"eye_height":1.35}),
    )?["height"]
        .as_f64()
        .filter(|x| x.is_finite())
        .ok_or("deer hoof support required")?;
    Ok(mul(unit(p), crate::terrain::RADIUS + height + 0.025))
}
// A suspended bound: hind push, fore support, then a ballistic flight.
// Distance-based phase gives 7.6 m strides at full speed (about two per second).
fn bound(phase: f64, speed: f64, blend: f64) -> (f64, bool) {
    let airborne = phase >= 0.32 && speed > 2. && blend > 0.5;
    let t = ((phase - 0.32) / 0.68).clamp(0., 1.);
    let duration = (7.6 / speed.max(4.)) * 0.68;
    // h = v*t - g*t²/2, with a landing at the end of flight.
    let height = 0.5 * 9.81 * duration.powi(2) * t * (1. - t);
    (if airborne { height.min(0.28) * blend } else { 0. }, airborne)
}
impl State {
    pub(crate) fn solve(
        &mut self,
        host: &mut Host,
        animal: &Animal,
        eye: V,
        dt: f64,
    ) -> Result<Value, String> {
        let frame = crate::base_system::layer_four::deer_rig::authored_pose(
            animal.time,
            animal.stride,
            animal.run_blend,
            animal.alert_blend,
        );
        let lengths = crate::base_system::layer_four::deer_rig::leg_lengths();
        let up = animal.up;
        let forward = unit(sub(animal.forward, mul(up, dot(animal.forward, up))));
        let right = unit(cross(up, forward));
        let local_vector = |v: V| [dot(v, right), dot(v, up), dot(v, forward)];
        let local = |p: V| local_vector(sub(p, animal.position));
        let world_vector = |p: V| add(add(mul(right, p[0]), mul(up, p[1])), mul(forward, p[2]));
        let world = |p: V| add(animal.position, world_vector(p));
        let standing = animal.speed < 0.05 && animal.run_blend < 0.15;
        let desired: [V; 4] = std::array::from_fn(|i| world(frame.legs[i][2]));
        // A stationary body can turn under its gaze. Lift and replace one hoof at a
        // time instead of dragging all four contacts around with the body frame.
        if standing && !self.feet.iter().any(|f| f.step.is_some()) {
            for offset in 0..4 {
                let i = (self.next_leg + offset) % 4;
                if let Some(anchor) = self.feet[i].anchor {
                    let delta = sub(desired[i], anchor);
                    let horizontal = sub(delta, mul(up, dot(delta, up)));
                    if length(horizontal) > 0.38 {
                        self.feet[i].step = Some(Replant {
                            start: self.feet[i].goal.unwrap_or(anchor),
                            target: floor(host, desired[i], eye)?,
                            progress: 0.,
                        });
                        self.feet[i].anchor = None;
                        self.next_leg = (i + 1) % 4;
                        break;
                    }
                }
            }
        }
        let phase = animal.stride.rem_euclid(1.);
        let (rise, airborne) = bound(phase, animal.speed, animal.run_blend);
        let mut need_drop: f64 = 0.;
        let mut chains = Vec::with_capacity(4);
        let mut soles = Vec::with_capacity(4);
        let min_height = frame
            .legs
            .iter()
            .map(|c| c[2][1])
            .fold(f64::INFINITY, f64::min);
        for i in 0..4 {
            let f = &mut self.feet[i];
            let contact = if i < 2 { (0.18..0.32).contains(&phase) }
                else { (0.0..0.14).contains(&phase) };
            let stance = standing || (!airborne && contact);
            let mut sole = frame.foot_up[i];
            let goal = if let Some(mut step) = f.step {
                if !standing {
                    f.step = None;
                    f.anchor = None;
                    floor(host, desired[i], eye)?
                } else {
                    step.progress = (step.progress + dt / 0.28).min(1.);
                    let t = step.progress;
                    let ease = t * t * (3. - 2. * t);
                    let goal = add(
                        mix(step.start, step.target, ease),
                        mul(up, 0.10 * (std::f64::consts::PI * t).sin()),
                    );
                    if t >= 1. {
                        f.anchor = Some(step.target);
                        f.step = None;
                    } else {
                        f.step = Some(step);
                    }
                    sole = [0., 1., 0.];
                    goal
                }
            } else if stance {
                let anchor = f.anchor.unwrap_or(desired[i]);
                let grounded = floor(host, anchor, eye)?;
                f.anchor = Some(grounded);
                sole = [0., 1., 0.];
                grounded
            } else {
                f.anchor = None;
                let tuck = if airborne { 0.20 * ((phase - 0.32) / 0.68 * std::f64::consts::PI).sin() } else { 0. };
                let lift = (frame.legs[i][2][1] - min_height).clamp(0.05, 0.60) + rise + tuck;
                add(floor(host, desired[i], eye)?, mul(up, lift))
            };
            f.planted = stance && f.step.is_none();
            let mut goal = if f.planted {
                goal
            } else if let Some(old) = f.goal {
                mix(old, goal, 1. - (-22. * dt).exp())
            } else {
                goal
            };
            // A rising support surface takes precedence over smoothing to avoid driving
            // the hoof through newly raised ground.
            let support = floor(host, goal, eye)?;
            let normal = unit(goal);
            let penetration = dot(sub(support, goal), normal);
            if penetration > 0. {
                goal = add(goal, mul(normal, penetration));
            }
            f.goal = Some(goal);
            let seed = f.seed.map(|s| s.map(local)).unwrap_or(frame.legs[i]);
            let root = frame.legs[i][0];
            let pole = add(frame.legs[i][1], [0., 0., if i < 2 { 0.8 } else { -0.8 }]);
            if f.planted {
                need_drop = need_drop
                    .max(length(sub(root, local(goal))) - (lengths[i][0] + lengths[i][1] - 0.035));
            }
            chains.push(json!({"root":root,"mid":seed[1],"end":seed[2],"target":local(goal),"pole":pole,"lengths":lengths[i]}));
            soles.push(sole);
        }
        let desired_drop = if airborne { 0. } else { need_drop.clamp(0., 0.20) };
        if self.feet.iter().all(|f| f.seed.is_none()) {
            self.drop = desired_drop;
        } else {
            self.drop += (desired_drop - self.drop) * (1. - (-14. * dt).exp());
        }
        let body_offset = [0., rise - self.drop * if airborne { 0. } else { 1. }, 0.];
        for chain in &mut chains {
            for key in ["root", "pole"] {
                let p: V = serde_json::from_value(chain[key].clone())
                    .map_err(|_| "deer chain vector missing")?;
                chain[key] = json!(add(p, body_offset));
            }
        }
        let mut result = host.invoke(
            "InverseKinematicsSystem.SolveChains",
            json!({"chains":chains}),
        )?;
        for i in 0..4 {
            let solved: [V; 3] = serde_json::from_value(result["chains"][i]["joints"].clone())
                .map_err(|_| "solved deer chain missing")?;
            self.feet[i].seed = Some(solved.map(world));
        }
        result["airborne"] = json!(airborne);
        result["ballistic_rise"] = json!(rise);
        result["foot_up"] = json!(soles);
        result["body_offset"] = json!(body_offset);
        result["planted"] = json!(self.feet.iter().map(|f| f.planted).collect::<Vec<_>>());
        result["anchors"] = json!(self.feet.iter().map(|f| f.anchor).collect::<Vec<_>>());
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deer_bound_has_gravity_arc_and_grounded_contact_windows() {
        let speed = 15.;
        let duration = 7.6 / speed * 0.68;
        let (start, _) = bound(0.32, speed, 1.);
        let (apex, flight) = bound(0.66, speed, 1.);
        let (end, _) = bound(1., speed, 1.);
        assert!(start.abs() < 1e-10); assert!(end.abs() < 1e-10);
        assert!(flight && apex > 0.14 && apex < 0.16);
        assert!((apex - 9.81 * duration * duration / 8.).abs() < 1e-10);
        assert!(!bound(0.2, speed, 1.).1);
        assert!(!bound(0.66, 0., 1.).1);
    }
}
