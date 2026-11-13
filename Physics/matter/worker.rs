//! Third-person, supported worker. The same finite-mass held Tool and material
//! solvers as gameplay receive two anatomical grips and muscle-limited targets.
//! Feet/torso are a prescribed motor plan, not a dynamic ragdoll.
use super::game::hand::Pose;
use super::*;
use crate::base_system::layer_two::player_model_system::{Model, math as rig};
use crate::physics::rigid_contact::{Grasp, Grip};
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct Worker {
    model: Option<Arc<Model>>,
    case: String,
    clock: f32,
    cycle: usize,
    floor: f32,
    target: V,
    pelvis: V,
    shoulders: [V; 2],
    feet: [V; 2],
    arm: [[f32; 2]; 2],
    phase: &'static str,
    max_grip_error: f32,
    peak_payload: usize,
    surface_found: bool,
    balance: V,
}
fn smooth(t: f32, a: f32, b: f32) -> f32 {
    let x = ((t - a) / (b - a)).clamp(0., 1.);
    x * x * (3. - 2. * x)
}
fn basis(angle: f32) -> [[f64; 3]; 3] {
    let (s, c) = (angle as f64).sin_cos();
    [[-1., 0., 0.], [0., c, -s], [0., -s, -c]]
}
fn at(p: V, edge: V, a: [[f64; 3]; 3]) -> Pose {
    Pose::at(p.map(f64::from), edge, a)
}
fn chain(root: V, target: V, a: f32, b: f32, pole: V) -> [V; 3] {
    let delta = sub(target, root);
    let axis = unit(delta);
    let d = length(delta).clamp((a - b).abs() + 0.0001, a + b - 0.0001);
    let along = (a * a - b * b + d * d) / (2. * d);
    let bend = unit(sub(pole, scale(axis, dot(pole, axis))));
    [
        root,
        add(
            root,
            add(
                scale(axis, along),
                scale(bend, (a * a - along * along).max(0.).sqrt()),
            ),
        ),
        add(root, scale(axis, d)),
    ]
}
impl Worker {
    pub fn new(case: &str) -> Self {
        let floor = if case == "matter_worker_dig" {
            0.49
        } else {
            0.
        };
        Self {
            model: None,
            case: case.into(),
            clock: 0.,
            cycle: 0,
            floor,
            target: [0., floor, 0.],
            pelvis: [0., floor + 0.88, 0.60],
            shoulders: [[0.; 3]; 2],
            feet: [[-0.17, floor + 0.08, 0.40], [0.17, floor + 0.08, 0.72]],
            arm: [[0.31, 0.29]; 2],
            phase: "Plant feet · prepare grip",
            max_grip_error: 0.,
            peak_payload: 0,
            surface_found: true,
            balance: [0.; 3],
        }
    }
    pub fn load(&mut self, host: &mut Host) -> Result<(), String> {
        if self.model.is_none() {
            let model = Model::from_data(&host.invoke("PlayerModelSystem.Load", Value::Null)?)?;
            let length = |a: &str, b: &str| -> Result<f32, String> {
                let a = model
                    .joints
                    .iter()
                    .find(|j| j.name == a)
                    .ok_or("worker shoulder joint missing")?
                    .rest;
                let b = model
                    .joints
                    .iter()
                    .find(|j| j.name == b)
                    .ok_or("worker arm joint missing")?
                    .rest;
                Ok(rig::len(rig::sub(a, b)) as f32)
            };
            let height = model
                .vertices
                .iter()
                .map(|v| v.p[1])
                .fold(f64::NEG_INFINITY, f64::max)
                - model
                    .vertices
                    .iter()
                    .map(|v| v.p[1])
                    .fold(f64::INFINITY, f64::min);
            let scale = 1.78 / height as f32;
            for (i, s) in ["L", "R"].iter().enumerate() {
                self.arm[i] = [
                    length(&format!("UpperArm.{s}"), &format!("ForeArm.{s}"))? * scale,
                    length(&format!("ForeArm.{s}"), &format!("Wrist.{s}"))? * scale,
                ];
            }
            self.model = Some(Arc::new(model));
        }
        Ok(())
    }
    pub fn phase(&self) -> &str {
        self.phase
    }
    fn is_axe(&self) -> bool {
        self.case == "matter_worker_axe"
    }
    fn period(&self) -> f32 {
        if self.is_axe() { 3.8 } else { 6.8 }
    }
    fn target_pose(&self, t: f32) -> Pose {
        if self.is_axe() {
            // Head at the end of a transverse haft. Rotate the whole axe in
            // a diagonal chopping plane about the upper grip, edge inward.
            let theta = if t < 1.1 {
                smooth(t, 0.25, 1.1)
            } else {
                1. - smooth(t, 1.1, 1.45)
            };
            let (sn, cs) = (theta as f64).sin_cos();
            let haft = rig::unit([0.55, 0.48, 0.72]);
            let edge_back = rig::unit(rig::sub(
                [0., 0.9, 1.],
                rig::mul(haft, rig::dot([0., 0.9, 1.], haft)),
            ));
            let x = rig::add(rig::mul(haft, cs), rig::mul(edge_back, sn));
            let y = rig::add(rig::mul(haft, -sn), rig::mul(edge_back, cs));
            let z = rig::cross(x, y);
            let contact = at(
                add(self.target, [0., -0.035, -0.065]),
                [0., -0.19, 0.],
                [haft, edge_back, rig::cross(haft, edge_back)],
            );
            // Swing sweeps the cutting edge to the near trunk surface.
            let pivot = [0.43, self.target[1] + 0.50, 0.65];
            let wind = Pose::at(pivot.map(f64::from), [0.64, 0.045, 0.], [x, y, z]);
            if t < 1.1 {
                wind
            } else if t < 1.45 {
                wind.blend(&contact, smooth(t, 1.1, 1.45))
            } else if t < 2.0 {
                contact
            } else {
                contact.blend(&wind, smooth(t, 2.0, 3.3))
            }
        } else {
            let edge = [0., -0.29, 0.];
            let p = self.target;
            let carry = at(add(p, [-0.43, 0.46, -0.25]), edge, basis(-1.32));
            let pour = Pose::at(carry.point([0., 0.58, 0.]), [0., 0.58, 0.], basis(0.35));
            let poses = [
                (0., at(add(p, [0., 0.18, 0.17]), edge, basis(-0.12))),
                (0.8, at(add(p, [0., 0.012, 0.025]), edge, basis(-0.12))),
                (1.55, at(add(p, [0., -0.16, 0.015]), edge, basis(-0.12))),
                (2.9, at(add(p, [0., -0.08, -0.09]), edge, basis(-1.0))),
                (4.0, at(add(p, [0., 0.42, -0.23]), edge, basis(-1.32))),
                (4.9, carry),
                (5.65, pour),
                (6.8, at(add(p, [0., 0.18, 0.17]), edge, basis(-0.12))),
            ];
            for pair in poses.windows(2) {
                if t <= pair[1].0 {
                    return pair[0].1.blend(&pair[1].1, smooth(t, pair[0].0, pair[1].0));
                }
            }
            poses.last().unwrap().1.clone()
        }
    }
    pub fn drive(&mut self, tool: &mut Tool, time: f32, dt: f32) {
        self.clock = time;
        let t = time % self.period();
        let digging = !self.is_axe();
        let bend = if digging {
            smooth(t, 0.25, 1.3) * (1. - 0.45 * smooth(t, 2.9, 4.0)) * (1. - smooth(t, 5.65, 6.8))
        } else {
            0.12
        };
        let side = if digging {
            -0.23 * smooth(t, 4.0, 4.9) * (1. - smooth(t, 5.65, 6.8))
        } else {
            0.
        };
        self.pelvis = [
            side,
            self.floor + 0.89 - (0.34 + (self.floor - self.target[1]).clamp(0., 0.25)) * bend,
            0.58 + 0.03 * bend,
        ];
        // Quasistatic weight shift from the accepted tool reaction. Bound the
        // shift inside the planted stance; no infinite-strength torso spring.
        let desired = [
            (-tool.contact_reaction[0] / (75. * 9.81) * 0.20).clamp(-0.09, 0.09),
            0.,
            (-tool.contact_reaction[2] / (75. * 9.81) * 0.16).clamp(-0.07, 0.07),
        ];
        self.balance = add(
            self.balance,
            scale(sub(desired, self.balance), 1. - (-dt / 0.12).exp()),
        );
        self.pelvis = add(self.pelvis, self.balance);
        let chest = add(self.pelvis, [0., 0.46 - 0.28 * bend, -0.36 * bend]);
        self.shoulders = [add(chest, [-0.19, 0., 0.]), add(chest, [0.19, 0., 0.])];
        tool.grasp = Some(Grasp {
            grips: std::array::from_fn(|i| Grip {
                point: tool.wrists()[i].map(f64::from),
                shoulder: self.shoulders[i].map(f64::from),
                minimum: (self.arm[i][0] - self.arm[i][1]).abs() as f64 + 0.005,
                maximum: (self.arm[i][0] + self.arm[i][1]) as f64 - 0.012,
                slide: None,
            }),
        });
        let target = self.target_pose(t);
        let target = super::game::hand::reachable(target, tool, tool.grasp.as_ref());
        tool.manual = Some(target.vertices(tool));
        let effort = if digging {
            self.phase = if t < 0.8 {
                "Set edge · bend knees"
            } else if t < 1.55 {
                "Drive blade · body weight"
            } else if t < 2.9 {
                "Lever · break roots and soil"
            } else if t < 4.9 {
                "Lift with legs · carry"
            } else if t < 5.65 {
                "Tip load beside hole"
            } else {
                "Recover grip"
            };
            if (0.65..3.9).contains(&t) { 0.95 } else { 0.34 }
        } else {
            self.phase = if t < 1.1 {
                "Wind up · brace stance"
            } else if t < 1.45 {
                "Accelerate · chop across grain"
            } else if t < 2.0 {
                "Contact · wedging"
            } else {
                "Recover · inspect cut"
            };
            if (0.9..1.7).contains(&t) { 1. } else { 0.36 }
        };
        tool.muscles.effort = effort;
        // A finite 75 kg supported worker; strength is not infinite even when
        // the motor plan requests a pose on the far side of an obstruction.
        tool.force_limit = Some(if digging { 620. } else { 700. });
        if time == 0. {
            tool.initialize_pose(tool.manual.clone().unwrap());
        }
        let pose = Pose::fit(tool);
        for i in 0..2 {
            let hand = pose.point(tool.wrists()[i]).map(|v| v as f32);
            let solved = chain(
                self.shoulders[i],
                hand,
                self.arm[i][0],
                self.arm[i][1],
                [if i == 0 { -1. } else { 1. }, -0.3, 0.35],
            );
            self.max_grip_error = self.max_grip_error.max(length(sub(solved[2], hand)));
        }
        let _ = dt;
    }
    pub fn report(&self, tool: &Tool) -> Value {
        let pose = Pose::fit(tool);
        let edge = pose
            .point(if self.is_axe() {
                [0., -0.19, 0.]
            } else {
                [0., -0.29, 0.]
            })
            .map(|v| v as f32);
        json!({"stroke":self.cycle.saturating_add(1),"phase":self.phase,"target":self.target,"edge":edge,
            "edge_to_target_m":length(sub(edge,self.target)),"peak_grip_error_m":self.max_grip_error,"peak_carried_samples":self.peak_payload,
            "surface_found":self.surface_found,"balance_shift_m":self.balance,"body_model":"Prismals mesh; planted-leg and two-arm IK; supported prescribed torso","dynamic_ragdoll":false})
    }
    pub fn render(&self, tool: &Tool, out: &mut Vec<[f32; 6]>) {
        let Some(model) = &self.model else { return };
        let height = model
            .vertices
            .iter()
            .map(|v| v.p[1])
            .fold(f64::NEG_INFINITY, f64::max)
            - model
                .vertices
                .iter()
                .map(|v| v.p[1])
                .fold(f64::INFINITY, f64::min);
        let scale = 1.78 / height;
        let index = |n: &str| model.joints.iter().position(|j| j.name == n).unwrap();
        let root = index("Root");
        let rest_root = model.joints[root].rest;
        let bind = |p: rig::V| [-p[0], p[1], -p[2]];
        let mut targets: Vec<rig::V> = model
            .joints
            .iter()
            .map(|j| {
                rig::add(
                    self.pelvis.map(f64::from),
                    rig::mul(bind(rig::sub(j.rest, rest_root)), scale),
                )
            })
            .collect();
        let pose = Pose::fit(tool);
        let chest = scale_v(add(self.shoulders[0], self.shoulders[1]), 0.5);
        targets[index("Ribs.001")] = chest.map(f64::from);
        let torso = unit(sub(chest, self.pelvis));
        for n in ["Neck", "Head"] {
            let y =
                (model.joints[index(n)].rest[1] - model.joints[index("Ribs.001")].rest[1]) * scale;
            targets[index(n)] = rig::add(chest.map(f64::from), rig::mul(torso.map(f64::from), y));
        }
        for (i, s) in ["L", "R"].iter().enumerate() {
            let names = [
                format!("UpperArm.{s}"),
                format!("ForeArm.{s}"),
                format!("Wrist.{s}"),
            ];
            let hand = pose.point(tool.wrists()[i]).map(|v| v as f32);
            let arm = chain(
                self.shoulders[i],
                hand,
                self.arm[i][0],
                self.arm[i][1],
                [if i == 0 { -1. } else { 1. }, -0.25, 0.4],
            );
            for k in 0..3 {
                targets[index(&names[k])] = arm[k].map(f64::from);
            }
            let names = [
                format!("Thigh.{s}"),
                format!("Shin.{s}"),
                format!("Foot.{s}"),
            ];
            let ids = names.each_ref().map(|n| index(n));
            let lengths = [
                rig::len(rig::sub(
                    model.joints[ids[1]].rest,
                    model.joints[ids[0]].rest,
                )) * scale,
                rig::len(rig::sub(
                    model.joints[ids[2]].rest,
                    model.joints[ids[1]].rest,
                )) * scale,
            ];
            let leg = chain(
                targets[ids[0]].map(|v| v as f32),
                self.feet[i],
                lengths[0] as f32,
                lengths[1] as f32,
                [0., 0., -1.],
            );
            for k in 0..3 {
                targets[ids[k]] = leg[k].map(f64::from);
            }
        }
        const CHAINS: [(&str, &str); 11] = [
            ("Root", "Ribs.001"),
            ("Ribs.001", "Neck"),
            ("Neck", "Head"),
            ("UpperArm.L", "ForeArm.L"),
            ("ForeArm.L", "Wrist.L"),
            ("UpperArm.R", "ForeArm.R"),
            ("ForeArm.R", "Wrist.R"),
            ("Thigh.L", "Shin.L"),
            ("Shin.L", "Foot.L"),
            ("Thigh.R", "Shin.R"),
            ("Shin.R", "Foot.R"),
        ];
        let driven = |n: &str| CHAINS.iter().any(|(a, b)| n == *a || n == *b);
        // Resolve transforms once per joint, not once per influence/vertex.
        let transforms: Vec<_> = (0..model.joints.len())
            .map(|mut id| {
                while !driven(&model.joints[id].name) {
                    match model.joints[id].parent {
                        Some(parent) => id = parent,
                        None => break,
                    }
                }
                let joint = &model.joints[id];
                let (from, to) =
                    if let Some((_, child)) = CHAINS.iter().find(|(a, _)| *a == joint.name) {
                        let c = index(child);
                        (
                            bind(rig::sub(model.joints[c].rest, joint.rest)),
                            rig::sub(targets[c], targets[id]),
                        )
                    } else if joint.name == "Head" {
                        (
                            [0., 0., -1.],
                            rig::unit(rig::sub(self.target.map(f64::from), targets[id])),
                        )
                    } else if joint.name.starts_with("Wrist.") {
                        let side = usize::from(joint.name.ends_with('R'));
                        let fingers = tool.finger_directions()[side];
                        let direction = std::array::from_fn(|k| {
                            (0..3).map(|j| pose.axes[j][k] * fingers[j] as f64).sum()
                        });
                        ([0., -1., 0.], direction)
                    } else {
                        ([0., 1., 0.], [0., 1., 0.])
                    };
                let axes = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]
                    .map(|v| rig::between(bind(v), from, to));
                (joint.rest, targets[id], axes)
            })
            .collect();
        for tri in model.vertices.chunks_exact(3) {
            let p = std::array::from_fn(|k| {
                let v = &tri[k];
                let mut p = [0.; 3];
                for j in 0..4 {
                    if v.weights[j] > 0. {
                        let (rest, target, axes) = transforms[v.joints[j]];
                        let delta = rig::sub(v.p, rest);
                        let q = rig::add(
                            target,
                            std::array::from_fn(|k| {
                                (0..3).map(|a| axes[a][k] * delta[a] * scale).sum()
                            }),
                        );
                        p = rig::add(p, rig::mul(q, v.weights[j]));
                    }
                }
                p.map(|v| v as f32)
            });
            let dominant = (0..4)
                .max_by(|&a, &b| tri[0].weights[a].total_cmp(&tri[0].weights[b]))
                .unwrap();
            let name = &model.joints[tri[0].joints[dominant]].name;
            let color = if name.contains("Foot") || name.contains("Toe") {
                [0.16, 0.12, 0.085]
            } else if name.contains("Thigh") || name.contains("Shin") || name == "Root" {
                [0.23, 0.28, 0.32]
            } else if name.contains("Arm") || name.contains("Rib") || name.contains("Spine") {
                [0.25, 0.39, 0.50]
            } else {
                [0.72, 0.47, 0.32]
            };
            skin::triangle(out, p, color);
        }
        if self.floor > 0. {
            box_mesh(
                out,
                [-0.8, 0., 0.26],
                [0.8, self.floor, 1.10],
                [0.29, 0.24, 0.16],
            );
        }
    }
}
fn scale_v(v: V, s: f32) -> V {
    scale(v, s)
}
pub(super) fn box_mesh(out: &mut Vec<[f32; 6]>, lo: V, hi: V, c: V) {
    let p = crate::physics::cut_geometry::Poly::box_from(lo.map(f64::from), hi.map(f64::from));
    for f in &p.faces {
        for k in 1..f.nodes.len() - 1 {
            skin::triangle(
                out,
                [f.nodes[0], f.nodes[k], f.nodes[k + 1]]
                    .map(|i| p.vertices[i].position.map(|x| x as f32)),
                c,
            );
        }
    }
}
impl World {
    pub(super) fn initialize_showcase(&mut self, case: &str) -> Result<(), String> {
        if case.starts_with("matter_ice") {
            return self.initialize_ice(case);
        }
        let mut worker = Worker::new(case);
        if case == "matter_worker_dig" {
            let mut l = layered::Layered::new()?;
            l.refresh_skin(&self.tool);
            self.layers = Some(Box::new(l));
        } else if case == "matter_worker_axe" {
            self.solid = Some(showcase_tree()?);
            worker.target = [0., 0.82, 0.115];
        } else {
            *self = World::filled_box_spaced(
                [-0.37, 0.015, -0.28],
                [0.37, 0.24, 0.28],
                Kind::Snow,
                false,
                0.065,
            );
            self.case = case.into();
            self.thermal = Some(thermal::Thermal::snow(self));
            worker.target = [0., 0.24, 0.];
        }
        self.tool = Tool::held(if case == "matter_worker_axe" {
            "matter_wood_chop"
        } else {
            "matter_grass"
        });
        self.tool.ground = vec![[-3., 0., -3.], [3., 0., -3.], [3., 0., 3.], [-3., 0., 3.]];
        if worker.floor > 0. {
            // The worker stands on unexcavated ground behind the exposed section.
            self.tool.ground.extend([
                [-0.8, worker.floor, 0.26],
                [0.8, worker.floor, 0.26],
                [0.8, worker.floor, 1.10],
                [-0.8, worker.floor, 1.10],
            ]);
            self.tool.ground_faces.extend([[4, 6, 5], [4, 7, 6]]);
        }
        worker.drive(&mut self.tool, 0., 1. / 240.);
        self.worker = Some(worker);
        Ok(())
    }
    pub(crate) fn load_worker(&mut self, host: &mut Host) -> Result<(), String> {
        if let Some(w) = &mut self.worker {
            w.load(host)?;
            w.drive(&mut self.tool, 0., 1. / 240.);
        }
        Ok(())
    }
    pub(super) fn prepare_worker(&mut self, dt: f32) {
        let Some(mut worker) = self.worker.take() else {
            return;
        };
        let cycle = (self.time / worker.period()) as usize;
        if cycle != worker.cycle {
            worker.cycle = cycle;
            if !worker.is_axe() {
                let x = [0., -0.065, 0.065, 0.][cycle % 4];
                let z = 0.03;
                let faces = self.collision();
                let top = faces
                    .iter()
                    .filter_map(|p| {
                        let a = [p[1][0] - p[0][0], p[1][2] - p[0][2]];
                        let b = [p[2][0] - p[0][0], p[2][2] - p[0][2]];
                        let d = a[0] * b[1] - a[1] * b[0];
                        if d.abs() < 1e-8 {
                            return None;
                        }
                        let q = [x - p[0][0], z - p[0][2]];
                        let u = (q[0] * b[1] - q[1] * b[0]) / d;
                        let v = (a[0] * q[1] - a[1] * q[0]) / d;
                        if u < 0. || v < 0. || u + v > 1. {
                            None
                        } else {
                            Some(p[0][1] + u * (p[1][1] - p[0][1]) + v * (p[2][1] - p[0][1]))
                        }
                    })
                    .filter(|h| *h < worker.floor + 0.35)
                    .max_by(f32::total_cmp);
                worker.surface_found = top.is_some();
                if let Some(h) = top {
                    worker.target = [x, h, z];
                }
            }
        }
        worker.peak_payload = worker.peak_payload.max(self.payload_count());
        worker.drive(&mut self.tool, self.time, dt);
        self.worker = Some(worker);
    }
}
fn showcase_tree() -> Result<solid::Solid, String> {
    use crate::vegetation::{self, geometry, profiles};
    let site = vegetation::Site {
        id: 9172,
        normal: [0., 1., 0.],
        origin: [0.; 3],
        profile: 0,
        moisture: 0.6,
        soil_depth: 0.8,
    };
    let mut state = vegetation::initial_state(site);
    state.age_years = 14.;
    let profile = profiles::PROFILES[0];
    let mut tree = vegetation::Tree::generate(site, &state, profile);
    tree.frame = Some([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]);
    // A small standing tree gives the full crown room in a third-person view.
    let size = 3.2 / tree.height;
    let radial = 0.12 / tree.branches[0].radius;
    for b in &mut tree.branches {
        b.start = b.start.map(|x| x * size);
        b.end = b.end.map(|x| x * size);
        b.radius *= radial;
    }
    let mesh = geometry::build_seasonal(&tree, profile, geometry::Detail::Middle, 0., 0.);
    let vertices:Vec<_>=mesh.indices.iter().map(|&i|{let v=mesh.vertices[i as usize];json!({"position":v.position,"color":v.color,"branch":v.branch_id,"material":v.material})}).collect();
    let branches:Vec<_>=tree.branches.iter().map(|b|json!({"start":b.start,"end":b.end,"radius":b.radius,"end_radius":b.radius*if b.order==0{0.77}else{0.36},"id":b.id})).collect();
    game::workshop_tree(
        &json!({"branches":branches,"vertices":vertices,"bark":profile.bark_color}),
        0.82,
    )
}
