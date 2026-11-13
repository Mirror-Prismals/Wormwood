//! Controls-only rigid spade/terrain contact. The experimental soil model is selected separately.
use crate::{
    host::Host,
    physics::{Scene, math::*},
    terrain,
};
use serde_json::{Value, json};
type D = [f64; 3];
fn d(v: &Value) -> D {
    std::array::from_fn(|i| v[i].as_f64().unwrap())
}
fn dotd(a: D, b: D) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn addd(a: D, b: D) -> D {
    std::array::from_fn(|i| a[i] + b[i])
}
fn subd(a: D, b: D) -> D {
    std::array::from_fn(|i| a[i] - b[i])
}
fn dm(a: D, s: f64) -> D {
    a.map(|x| x * s)
}
fn unitd(a: D) -> D {
    dm(a, 1. / dotd(a, a).sqrt().max(1e-12))
}
fn crossd(a: D, b: D) -> D {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn point(p: &Value, v: V) -> D {
    let axes = [d(&p["right"]), d(&p["up"]), d(&p["forward"])];
    addd(
        d(&p["position"]),
        std::array::from_fn(|i| (0..3).map(|j| axes[j][i] * v[j] as f64).sum()),
    )
}
fn blend(a: &Value, b: &Value, t: f64) -> Value {
    let mut p = b.clone();
    for k in ["position", "right", "up", "forward"] {
        p[k] = json!(addd(d(&a[k]), dm(subd(d(&b[k]), d(&a[k])), t)));
    }
    let r = unitd(d(&p["right"]));
    let u = d(&p["up"]);
    let u = unitd(subd(u, dm(r, dotd(r, u))));
    p["right"] = json!(r);
    p["up"] = json!(u);
    p["forward"] = json!(unitd(crossd(r, u)));
    p
}
#[derive(Default)]
pub(crate) struct State {
    scene: Option<Scene>,
    anchor: D,
    basis: [D; 3],
    tool: Vec<V>,
    start: usize,
    pose: Value,
    velocity: D,
    active: bool,
    force: D,
    torque: D,
    contact: Value,
    steps: u64,
    blocked: bool,
    accumulator: f64,
}
impl State {
    fn global(&self, p: V) -> D {
        addd(
            self.anchor,
            std::array::from_fn(|i| (0..3).map(|j| self.basis[j][i] * p[j] as f64).sum()),
        )
    }
    fn local(&self, p: D) -> V {
        let q = subd(p, self.anchor);
        std::array::from_fn(|i| dotd(q, self.basis[i]) as f32)
    }
    fn spec(&self, scene: &Scene) -> Value {
        let mut v = scene.spec();
        v["contact_stiffness"] = json!(80000.);
        v["surfaces"][1]["motion"] = json!(1);
        v
    }
    fn initialize(&mut self, host: &mut Host, input: &Value) -> Result<(), String> {
        let tool: Vec<V> = serde_json::from_value(input["blade"]["vertices"].clone())
            .map_err(|e| e.to_string())?;
        if tool.is_empty() {
            return Err("spade contact requires blade geometry".into());
        }
        let n = unitd(d(&input["pose"]["position"]));
        let r = d(&input["pose"]["right"]);
        let r = unitd(subd(r, dm(n, dotd(r, n))));
        let mut candidate = Self {
            anchor: dm(n, terrain::RADIUS),
            basis: [r, n, unitd(crossd(r, n))],
            tool,
            pose: input["pose"].clone(),
            velocity: self.velocity,
            ..Default::default()
        };
        let mut s = Scene::new("rigid_spade_controls");
        s.radius = 0.003;
        let size = 12usize;
        for z in 0..=size {
            for x in 0..=size {
                let normal =
                    unitd(candidate.global([(x as f32 - 6.) * 0.5, 0., (z as f32 - 6.) * 0.5]));
                let h=host.invoke("PlanetSurfaceSystem.Height",json!({"position":normal}))?["height"].as_f64().unwrap();
                s.node(
                    candidate.local(dm(normal, terrain::RADIUS + h)),
                    0.,
                    [0.3, 0.3, 0.2],
                );
            }
        }
        let mut faces = Vec::new();
        for z in 0..size {
            for x in 0..size {
                let a = (z * (size + 1) + x) as u32;
                faces.extend([
                    [a, a + (size + 1) as u32, a + 1],
                    [a + 1, a + (size + 1) as u32, a + (size + 2) as u32],
                ]);
            }
        }
        s.surface(0, faces, true, false);
        candidate.start = s.p.len();
        for v in &candidate.tool {
            s.node(
                candidate.local(point(&candidate.pose, *v)),
                0.,
                [0.4, 0.4, 0.4],
            );
        }
        s.surface(
            candidate.start,
            serde_json::from_value(input["blade"]["triangles"].clone())
                .map_err(|e| e.to_string())?,
            true,
            false,
        );
        s.finish()?;
        if s.intersection_count() > 0 {
            return Err("spade contact preparation intersects the ground".into());
        }
        host.invoke("OGCConnectionSystem.Configure", candidate.spec(&s))?;
        candidate.scene = Some(s);
        *self = candidate;
        Ok(())
    }
    pub(crate) fn advance(&mut self, host: &mut Host, input: &Value) -> Result<Value, String> {
        let equipped = input["equipped"] == true;
        if self.scene.is_none() {
            if !equipped && input["release"] != true {
                return Ok(self.response(&input["pose"]));
            }
            self.initialize(host, input)?;
        }
        let local = self.local(d(&input["pose"]["position"]));
        if equipped && (local[0].abs() > 2. || local[2].abs() > 2.) {
            let mut reframe = input.clone();
            reframe["pose"] = self.pose.clone();
            self.initialize(host, &reframe)?;
        }
        let dt = input["dt"].as_f64().unwrap();
        self.accumulator = (self.accumulator + dt).min(8. / 240.);
        let steps = (self.accumulator * 240.).floor() as usize;
        self.accumulator -= steps as f64 / 240.;
        self.blocked = false;
        for _ in 0..steps {
            let old = self.pose.clone();
            let oldpos = d(&old["position"]);
            let mut target = if equipped {
                input["pose"].clone()
            } else {
                old.clone()
            };
            if !equipped {
                self.velocity = addd(self.velocity, dm(self.basis[1], -9.81 / 240.));
                target["position"] = json!(addd(oldpos, dm(self.velocity, 1. / 240.)));
            }
            let delta = subd(d(&target["position"]), oldpos);
            let ratio = (1.2 / 240. / dotd(delta, delta).sqrt().max(1e-12)).min(1.);
            target["position"] = json!(addd(oldpos, dm(delta, ratio)));
            let mut t = 1f64;
            for k in ["right", "up", "forward"] {
                let q = subd(d(&target[k]), d(&old[k]));
                t = t.min(3. / 240. / dotd(q, q).sqrt().max(1e-12));
            }
            let finalpos = target["position"].clone();
            target = blend(&old, &target, t.min(1.));
            target["position"] = finalpos;
            let previous = self.scene.as_ref().unwrap().p.clone();
            let mut scene = self.scene.as_ref().unwrap().clone();
            for (i, v) in self.tool.iter().enumerate() {
                scene.p[self.start + i] = self.local(point(&target, *v));
            }
            let mut projected=host.invoke("OGCConnectionSystem.Project",json!({"positions":scene.p,"previous":previous,"inverse_mass":scene.weights,"dt":1./240.}))?;
            let positions: Vec<V> =
                serde_json::from_value(projected["positions"].take()).map_err(|e| e.to_string())?;
            let mut fraction = 1f32;
            for i in self.start..scene.p.len() {
                let displacement = sub(scene.p[i], previous[i]);
                let l = dot(displacement, displacement);
                if l > 1e-14 && length(sub(positions[i], scene.p[i])) > 1e-6 {
                    fraction = fraction
                        .min((dot(sub(positions[i], previous[i]), displacement) / l).clamp(0., 1.));
                }
            }
            if fraction < 0.999 {
                self.blocked = true;
                target = blend(&old, &target, (fraction * 0.8) as f64);
            }
            let mut accepted = false;
            for retry in 0..5 {
                for (i, v) in self.tool.iter().enumerate() {
                    scene.p[self.start + i] = self.local(point(&target, *v));
                }
                if scene.intersection_count() == 0 {
                    accepted = true;
                    break;
                }
                self.blocked = true;
                target = blend(&old, &target, if retry < 3 { 0.5 } else { 0. });
            }
            if !accepted {
                target = old;
                scene = self.scene.as_ref().unwrap().clone();
                self.blocked = true;
            }
            // Keep the native accepted positions identical to the one rigid rendered pose.
            host.invoke("OGCConnectionSystem.Configure", self.spec(&scene))?;
            let forces: Vec<V> =
                serde_json::from_value(projected["forces"].take()).unwrap_or_default();
            let mut force = [0.; 3];
            let mut torque = [0.; 3];
            let center = self.local(d(&target["position"]));
            for (i, f) in forces.iter().enumerate().skip(self.start) {
                force = add(force, *f);
                torque = add(torque, cross(sub(scene.p[i], center), *f));
            }
            self.force =
                std::array::from_fn(|i| (0..3).map(|j| self.basis[j][i] * force[j] as f64).sum());
            self.torque =
                std::array::from_fn(|i| (0..3).map(|j| self.basis[j][i] * torque[j] as f64).sum());
            self.contact = projected;
            self.velocity = dm(subd(d(&target["position"]), oldpos), 240.);
            if !equipped && self.blocked {
                self.velocity = dm(self.velocity, 0.7);
            }
            self.pose = target;
            scene.time += 1. / 240.;
            self.scene = Some(scene);
            self.steps += 1;
        }
        let pending = equipped
            && dotd(
                subd(d(&input["pose"]["position"]), d(&self.pose["position"])),
                subd(d(&input["pose"]["position"]), d(&self.pose["position"])),
            ) > 1e-8;
        self.active = pending || (!equipped && dotd(self.velocity, self.velocity) > 1e-5);
        Ok(self.response(&input["pose"]))
    }
    pub(super) fn response(&self, fallback: &Value) -> Value {
        json!({"pose":if self.scene.is_some(){&self.pose}else{fallback},"active":self.active,"blocked":self.blocked,"failure":null,"force":self.force,"torque":self.torque,"contact":self.contact,"material":{"initialized":false,"changed":false,"rooted_soil_enabled":false,"controls_test":true},"steps":self.steps})
    }
    pub(crate) fn frame(&self) -> Value {
        json!({"debris":[],"active":self.active,"failure":null,"material":{"initialized":false,"changed":false,"rooted_soil_enabled":false,"controls_test":true}})
    }
    pub(crate) fn release(&mut self, host: &mut Host) -> Result<(), String> {
        if self.scene.is_some() {
            host.invoke("OGCConnectionSystem.Release", Value::Null)?;
        }
        *self = Self::default();
        Ok(())
    }
}
