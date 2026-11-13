//! Closed physical tool shells. Display and OGC use the same working surfaces.
use super::super::rigid_contact;
use super::*;
#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Spade,
    Bucket,
    Axe,
    Pick,
}
#[derive(Clone)]
pub struct Tool {
    pub model: Vec<V>,
    pub p: Vec<V>,
    pub faces: Vec<[u32; 3]>,
    v: Vec<V>,
    shape: Shape,
    blunt_strike: bool,
    turf_stroke: bool,
    cross_grain: bool,
    cut_height: f32,
    body: rigid_contact::Body,
    pub force_limit: Option<f32>,
    offset: V,
    yaw: f32,
    peak_energy: f32,
    contact_work: f32,
    peak_contact: f32,
    pub(super) contact_force: f32,
    pub(super) contact_reaction: V,
    pub dump_to_side: bool,
    pub(super) manual: Option<Vec<V>>,
    pub(super) ground: Vec<V>,
    pub(super) ground_faces: Vec<[u32; 3]>,
    pub(super) grasp: Option<rigid_contact::Grasp>,
    pub(super) muscles: super::muscle::Muscles,
    physical_handle: bool,
    head_faces: usize,
}
fn ease(t: f32, a: f32, b: f32) -> f32 {
    let x = ((t - a) / (b - a)).clamp(0., 1.);
    x * x * (3. - 2. * x)
}
impl Tool {
    pub fn new(case: &str) -> Self {
        let shape = match case {
            "matter_sand" | "matter_pebbles" | "matter_stone_blunt" => Shape::Bucket,
            "matter_wood" | "matter_wood_chop" => Shape::Axe,
            "matter_stone" => Shape::Pick,
            _ => Shape::Spade,
        };
        let mut t = Self {
            model: vec![],
            p: vec![],
            faces: vec![],
            v: vec![],
            shape,
            blunt_strike: case == "matter_stone_blunt",
            turf_stroke: case == "matter_grass",
            cross_grain: case == "matter_wood_chop",
            cut_height: 0.415,
            force_limit: None,
            offset: [0.; 3],
            yaw: 0.,
            peak_energy: 0.,
            contact_work: 0.,
            peak_contact: 0.,
            contact_force:0.,contact_reaction:[0.;3],
            dump_to_side: false,
            manual: None,
            ground: coupling::table_positions().to_vec(),
            ground_faces: vec![[0, 2, 1], [0, 3, 2]],
            grasp: None,
            muscles: super::muscle::Muscles::default(),
            physical_handle: false,
            head_faces: 0,
            body: rigid_contact::Body {
                mass: 2.2,
                center: [0., 0., 0.],
                inertia: [[0.06, 0., 0.], [0., 0.035, 0.], [0., 0., 0.055]],
                max_force: 450.,
                max_torque: 90.,
            },
        };
        match shape {
            Shape::Spade => t.spade(),
            Shape::Bucket => t.bucket(),
            Shape::Axe => t.axe(),
            Shape::Pick => t.pick(),
        }
        let center = scale(
            t.model.iter().copied().fold([0.; 3], add),
            1. / t.model.len() as f32,
        );
        t.body.center = center.map(|x| x as f64);
        t.p = t.pose(0.);
        t.v = vec![[0.; 3]; t.p.len()];
        t
    }
    /// The same material-space anchors drive the rigid grasp and visible IK.
    pub(super) fn grips(&self) -> [V; 2] {
        match self.shape {
            Shape::Axe | Shape::Pick => [[0.32, 0.016, 0.], [0.64, 0.045, 0.]],
            Shape::Bucket => [[-0.09, 0.16, 0.], [0.09, 0.16, 0.]],
            _ => [[0., 0.24, 0.], [0., 0.58, 0.]],
        }
    }
    pub(super) fn transverse_haft(&self) -> bool {
        matches!(self.shape, Shape::Axe | Shape::Pick)
    }
    pub(super) fn finger_directions(&self) -> [V; 2] {
        let axis = unit(sub(self.grips()[1], self.grips()[0]));
        let across = unit(cross(axis, [0., 0., 1.]));
        [across, scale(across, -1.)]
    }
    pub(super) fn wrists(&self) -> [V; 2] {
        let fingers = self.finger_directions();
        std::array::from_fn(|i| sub(self.grips()[i], scale(fingers[i], 0.035)))
    }
    pub(super) fn reframe(&mut self, point: impl Fn(V) -> V, vector: impl Fn(V) -> V) {
        self.p = self.p.iter().map(|&p| point(p)).collect();
        self.v = self.v.iter().map(|&v| vector(v)).collect();
        self.grasp = None;
    }
    pub(super) fn advance_held_air(&mut self, dt: f32) -> Result<(), String> {
        if dt <= 0. {
            return Ok(());
        }
        let count = (dt * 240.).ceil().max(1.) as usize;
        let h = dt / count as f32;
        let mut response = vec![[0.; 13]; self.p.len()];
        for r in &mut response {
            r[12] = 1000.;
        }
        for _ in 0..count {
            let old = self.p.clone();
            let goal = self.goal(0., h);
            self.project(&goal, &response, h)?;
            self.finish(&old, h);
        }
        Ok(())
    }
    pub(super) fn held(case: &str) -> Self {
        let mut t = Self::new(case);
        t.head_faces = t.faces.len();
        t.physical_handle = true;
        // Include the shaft in contact, so the hand cannot drag its visible
        // handle through a trunk while only the head encounters OGC.
        if t.shape == Shape::Bucket {
            for k in 0..10 {
                let arc = |k: usize| {
                    let a = k as f32 * std::f32::consts::PI / 10.;
                    [0.15 * a.cos(), 0.20 * a.sin(), 0.]
                };
                t.handle_segment(arc(k), arc(k + 1), 0.008);
            }
        } else if t.transverse_haft() {
            t.handle_segment([-0.045, 0.008, 0.], [0.32, 0.016, 0.], 0.018);
            t.handle_segment([0.32, 0.016, 0.], [0.70, 0.05, 0.], 0.021);
        } else {
            t.handle_segment([0., 0.01, 0.], [0., 0.58, 0.], 0.016);
            if t.shape == Shape::Spade {
                t.handle_segment([-0.07, 0.58, 0.], [0.07, 0.58, 0.], 0.019);
            }
        }
        t.p = t.model.clone();
        t.v = vec![[0.; 3]; t.model.len()];
        t
    }
    fn handle_segment(&mut self, a: V, b: V, radius: f32) {
        let axis = unit(sub(b, a));
        let x = unit(cross(
            axis,
            if axis[1].abs() < 0.9 {
                [0., 1., 0.]
            } else {
                [1., 0., 0.]
            },
        ));
        let z = cross(axis, x);
        let start = self.model.len() as u32;
        for o in [a, b] {
            for k in 0..6 {
                let angle = k as f32 * std::f32::consts::TAU / 6.;
                self.model.push(add(
                    o,
                    add(
                        scale(x, radius * angle.cos()),
                        scale(z, radius * angle.sin()),
                    ),
                ));
            }
        }
        for k in 0..6 {
            self.quad(
                start + k,
                start + (k + 1) % 6,
                start + 6 + (k + 1) % 6,
                start + 6 + k,
            );
        }
        for k in 1..5 {
            self.faces.extend([
                [start, start + k + 1, start + k],
                [start + 6, start + 6 + k, start + 6 + k + 1],
            ]);
        }
    }
    fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        self.faces.extend([[a, b, c], [a, c, d]]);
    }
    fn shell(&mut self, front: &[V], back: &[V], rows: usize, cols: usize) {
        let start = self.model.len() as u32;
        let n = front.len() as u32;
        self.model.extend(front);
        self.model.extend(back);
        for y in 0..rows - 1 {
            for x in 0..cols - 1 {
                let a = start + (y * cols + x) as u32;
                let b = a + 1;
                let d = a + cols as u32;
                let c = d + 1;
                self.quad(a, b, c, d);
                self.quad(a + n, d + n, c + n, b + n);
            }
        }
        let mut edge = Vec::new();
        edge.extend((0..cols).map(|x| start + x as u32));
        edge.extend((1..rows).map(|y| start + (y * cols + cols - 1) as u32));
        edge.extend(
            (0..cols - 1)
                .rev()
                .map(|x| start + ((rows - 1) * cols + x) as u32),
        );
        edge.extend((1..rows - 1).rev().map(|y| start + (y * cols) as u32));
        for k in 0..edge.len() {
            let a = edge[k];
            let b = edge[(k + 1) % edge.len()];
            self.quad(a, a + n, b + n, b);
        }
    }
    fn spade(&mut self) {
        let widths = [0.009, 0.067, 0.105, 0.126, 0.133, 0.13, 0.095];
        let heights = [-0.29, -0.255, -0.21, -0.155, -0.10, -0.045, 0.];
        let mut front = Vec::new();
        let mut back = Vec::new();
        for row in 0..7 {
            for col in 0..7 {
                let u = col as f32 / 3. - 1.;
                let z = -0.037 * (1. - u * u) * (1. - row as f32 / 9.);
                front.push([u * widths[row], heights[row], z]);
                back.push([
                    u * widths[row],
                    heights[row],
                    z - if row == 0 { 0.0015 } else { 0.006 },
                ]);
            }
        }
        self.shell(&front, &back, 7, 7);
    }
    fn bucket(&mut self) {
        // Open mouth, continuous U-shaped bottom and two end walls, with an
        // actual inner and outer skin. No invisible lid or carrying constraint.
        let mut front = Vec::new();
        let mut back = Vec::new();
        for x in [-0.15, 0.15] {
            for k in 0..13 {
                let a = std::f32::consts::PI * k as f32 / 12.;
                front.push([x, -0.16 * a.sin(), 0.16 * a.cos()]);
                back.push([x, -0.168 * a.sin(), 0.168 * a.cos()]);
            }
        }
        self.shell(&front, &back, 2, 13);
        for side in [-1., 1.] {
            let start = self.model.len() as u32;
            for x in [side * 0.15, side * 0.158] {
                self.model.push([x, 0., 0.]);
                for k in 0..13 {
                    let a = std::f32::consts::PI * k as f32 / 12.;
                    self.model.push([x, -0.16 * a.sin(), 0.16 * a.cos()]);
                }
            }
            for k in 1..13 {
                self.faces.push([start, start + k, start + k + 1]);
                self.faces
                    .push([start + 14, start + 14 + k + 1, start + 14 + k]);
                self.quad(start + k, start + 14 + k, start + 14 + k + 1, start + k + 1);
            }
            self.quad(start + 1, start + 15, start + 27, start + 13);
        }
    }
    fn axe(&mut self) {
        // Convex cheeks grow from a 1.2 mm honed edge to a 70 mm splitting
        // wedge. These are the contact faces, including the rounded toe/heel.
        // The haft runs along the edge direction through the eye, as on an
        // axe, rather than extending straight behind a spade-shaped plate.
        let mut front = Vec::new();
        let mut back = Vec::new();
        for row in 0..5 {
            let y = [-0.19, -0.155, -0.10, -0.025, 0.055][row];
            let width = [0.095, 0.092, 0.080, 0.052, 0.045][row];
            let thick = [0.0006, 0.009, 0.022, 0.035, 0.032][row];
            for col in 0..5 {
                let u = col as f32 / 2. - 1.;
                front.push([u * width, y + 0.012 * u * u, thick]);
                back.push([u * width, y + 0.012 * u * u, -thick]);
            }
        }
        self.shell(&front, &back, 5, 5);
    }
    fn pick(&mut self) {
        // Curved point tapering from a thick head to a small finite tip.
        let start = self.model.len() as u32;
        for ring in 0..7 {
            let u = ring as f32 / 6.;
            let radius = 0.035 * (1. - u) + 0.002 * u;
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::TAU / 8.;
                self.model
                    .push([radius * a.cos(), -0.28 * u, radius * a.sin() + 0.08 * u * u]);
            }
        }
        for r in 0..6 {
            for k in 0..8 {
                self.quad(
                    start + r * 8 + k,
                    start + r * 8 + (k + 1) % 8,
                    start + (r + 1) * 8 + (k + 1) % 8,
                    start + (r + 1) * 8 + k,
                );
            }
        }
        for k in 1..7 {
            self.faces.push([start, start + k + 1, start + k]);
            self.faces
                .push([start + 48, start + 48 + k, start + 48 + k + 1]);
        }
    }
    fn pose(&self, t: f32) -> Vec<V> {
        if self.shape == Shape::Axe {
            return self.axe_pose(t);
        }
        let (mut pivot, mut angle) = match self.shape {
            Shape::Spade if self.turf_stroke => {
                let lever = ease(t, 3., 6.);
                let angle = -1.45 * lever - 1.60 * ease(t, 10., 12.);
                // Follow the embedded tip while laying the blade under the
                // root zone. The force-limited body still accepts OGC motion.
                let y = 0.50
                    - 0.175 * ease(t, 0.5, 3.)
                    - (0.29 * (1. - (-1.45 * lever).cos()) - 0.037 * (1.45 * lever).sin())
                    + 0.34 * ease(t, 6., 9.);
                ([0., y, -0.11 - 0.09 * ease(t, 6., 9.)], angle)
            }
            Shape::Spade => (
                [
                    0.,
                    0.60 - 0.28 * ease(t, 0.5, 3.) + 0.38 * ease(t, 6., 9.),
                    -0.07 - 0.14 * ease(t, 6., 9.),
                ],
                -1.18 * ease(t, 3., 6.) - 1.6 * ease(t, 10., 12.),
            ),
            Shape::Bucket if !self.blunt_strike => (
                [
                    0.,
                    0.36 - 0.17 * ease(t, 0.5, 2.5)
                        + 0.04 * ease(t, 4.2, 5.5)
                        + 0.38 * ease(t, 5.5, 8.5),
                    -0.48 + 0.58 * ease(t, 2.5, 5.5),
                ],
                1.10 * ease(t, 0.5, 2.5) - 1.10 * ease(t, 4.2, 5.5) - 2.35 * ease(t, 10., 12.),
            ),
            Shape::Axe => unreachable!("axe has a separate swing and wedge trajectory"),
            Shape::Pick | Shape::Bucket => {
                let cycle = ((t - 0.8).max(0.) / 3.4).floor() as u32;
                let stroke = if t < 0.8 || cycle >= 3 {
                    0.
                } else {
                    (t - 0.8) % 3.4
                };
                let hit = ease(stroke, 0.3, 0.66);
                let release = ease(stroke, 1.4, 2.8);
                let deep = if self.shape == Shape::Bucket {
                    if cycle == 0 { 0.43 } else { 0.56 }
                } else {
                    if cycle == 0 { 0.32 } else { 0.51 }
                };
                (
                    [0.08, 0.93 - deep * (hit - release), 0.],
                    0.55 * (1. - hit + release),
                )
            }
        };
        if self.dump_to_side {
            pivot[0] += 0.65 * ease(t, 7., 9.5);
            // Withdraw to a visible clear position before returning for the
            // next scoop or exchanging tools. Material is never reset here.
            let park = ease(t, 12., 14.);
            pivot = add(scale(pivot, 1. - park), scale([0.72, 0.94, 0.02], park));
            angle *= 1. - park;
        }
        self.model
            .iter()
            .map(|p| add(add(pivot, rotate_x(*p, [0.; 3], angle)), self.offset))
            .collect()
    }
    fn axe_pose(&self, t: f32) -> Vec<V> {
        let cycle = ((t - 0.8).max(0.) / 4.2).floor() as usize;
        let stroke = if t < 0.8 || cycle >= 3 {
            0.
        } else {
            (t - 0.8) % 4.2
        };
        // Same requested arc for each force trial. Only the bounded actuator
        // changes; a weak swing can stall, and an embedded wedge can remain
        // behind its target. There is no animation that separates the wood.
        let hit = ease(stroke, 0.25, 0.68);
        let release = ease(stroke, 2.65, 3.85);
        let embedded = ease(stroke, 0.8, 1.4) * (1. - release);
        // Raise the haft, then bring the edge square to the intended cut at
        // contact. The old arc rotated the head past square during impact.
        let angle = 0.65 * (1. - (hit - release));
        let (s, c) = angle.sin_cos();
        let rotate = |p: V| [c * p[0] - s * p[1], s * p[0] + c * p[1], p[2]];
        // The edge is aimed through the log's end grain throughout the
        // downswing. A fixed shoulder pivot sent the edge sideways out of the
        // trunk before the thick cheeks could enter the opening.
        let progress = hit - release;
        let edge = [-0.12 * (1. - progress), 1.25 - 0.97 * progress, 0.];
        let head = sub(edge, rotate([0., -0.19, 0.]));
        let rock = 0.065 * embedded * ((stroke - 0.8) * 4.).sin();
        self.model
            .iter()
            .map(|&p| {
                let p = rotate_x(rotate(p), [0.; 3], rock);
                let p = add(head, p);
                let p = if self.cross_grain {
                    // Stand the edge vertically for a chop across the trunk.
                    // Local -Y is the cutting direction, toward the log.
                    [p[2], self.cut_height + p[0], p[1] - 0.36]
                } else {
                    p
                };
                let (s, c) = self.yaw.sin_cos();
                add(
                    [c * p[0] + s * p[2], p[1], -s * p[0] + c * p[2]],
                    self.offset,
                )
            })
            .collect()
    }
    pub fn phase(&self, t: f32) -> &str {
        match self.shape {
            _ if matches!(self.shape, Shape::Axe | Shape::Pick) || self.blunt_strike => {
                if t < 0.8 {
                    return "Prepare · finite force drive";
                }
                let period = if self.shape == Shape::Axe { 4.2 } else { 3.4 };
                let cycle = ((t - 0.8) / period).floor() as u32;
                if cycle >= 3 {
                    return "Settling · reset to replay";
                }
                let stroke = (t - 0.8).max(0.) % period;
                if self.shape == Shape::Axe && (0.8..2.65).contains(&stroke) {
                    return "Wedge / lever · contact-limited";
                }
                if stroke < 1.4 {
                    if self.force_limit.is_some() {
                        return "Strike · selected force limit";
                    }
                    match cycle {
                        0 => "Light tap · 45 N drive limit",
                        1 => "Strike · 240 N drive limit",
                        _ => "Strong strike · 800 N drive limit",
                    }
                } else {
                    "Withdraw"
                }
            }
            _ => {
                if t < 3. {
                    "Enter material"
                } else if t < 6. {
                    "Scoop / lever"
                } else if t < 9. {
                    "Lift"
                } else if t < 10. {
                    "Hold"
                } else if t < 12. {
                    "Pour / release"
                } else {
                    "Settling"
                }
            }
        }
    }
    pub fn goal(&mut self, t: f32, dt: f32) -> Vec<V> {
        let striking = matches!(self.shape, Shape::Axe | Shape::Pick) || self.blunt_strike;
        let period = if self.shape == Shape::Axe { 4.2 } else { 3.4 };
        let cycle = ((t - 0.8).max(0.) / period).floor() as u32;
        self.body.max_force = if let Some(limit) = self.force_limit {
            limit as f64
        } else if striking {
            match cycle {
                0 => 45.,
                1 => 240.,
                _ => 800.,
            }
        } else {
            450.
        };
        self.body.max_torque = self.body.max_force * 0.25;
        let target = self.manual.clone().unwrap_or_else(|| self.pose(t));
        if self.grasp.is_some() {
            return self.hand_prediction(&target, dt);
        }
        self.p
            .iter()
            .zip(&self.v)
            .zip(target)
            .map(|((p, v), wanted)| {
                let a = sub(scale(sub(wanted, *p), 420.), scale(*v, 34.));
                let cap = (self.body.max_force / self.body.mass) as f32;
                let a = scale(a, (cap / length(a).max(cap)).min(1.));
                add(*p, add(scale(*v, dt), scale(a, dt * dt)))
            })
            .collect()
    }
    fn hand_prediction(&mut self, target: &[V], dt: f32) -> Vec<V> {
        let Some((origin, axes)) = super::super::rigid_fit::fit(&self.model, &self.p) else {
            return self.p.clone();
        };
        let Some((wanted, wanted_axes)) = super::super::rigid_fit::fit(&self.model, target) else {
            return self.p.clone();
        };
        let point = |o: [f64; 3], a: [[f64; 3]; 3], p: [f64; 3]| -> V {
            std::array::from_fn(|k| (o[k] + (0..3).map(|j| a[j][k] * p[j]).sum::<f64>()) as f32)
        };
        let center = point(origin, axes, self.body.center);
        let predicted: Vec<_> = self
            .p
            .iter()
            .zip(&self.v)
            .map(|(&p, &v)| add(p, scale(v, dt)))
            .collect();
        let Some((next, next_axes)) = super::super::rigid_fit::fit(&self.model, &predicted) else {
            return self.p.clone();
        };
        let velocity = scale(
            sub(point(next, next_axes, self.body.center), center),
            1. / dt,
        );
        let mut angular = [0.; 3];
        for j in 0..3 {
            angular = add(
                angular,
                scale(
                    cross(axes[j].map(|x| x as f32), next_axes[j].map(|x| x as f32)),
                    0.5 / dt,
                ),
            );
        }
        let mut force = [0., -9.81 * self.body.mass as f32, 0.];
        let mut torque = [0.; 3];
        let goals = self.grips().map(|anchor| point(wanted,wanted_axes,anchor.map(f64::from)));
        let wanted_speeds = self.muscles.begin(goals,dt);
        for (side,anchor) in self.grips().into_iter().enumerate() {
            let hand=point(origin,axes,anchor.map(f64::from));
            let offset=sub(hand,center);
            let speed=add(velocity,cross(angular,offset));
            let direction=unit(sub(goals[side],hand));
            let moment=cross(offset,direction);
            let inverse_mass=1./self.body.mass as f32+(0..3).map(|j| {
                dot(moment,axes[j].map(|v|v as f32)).powi(2)/self.body.inertia[j][j] as f32
            }).sum::<f32>();
            let reach=self.grasp.as_ref().map_or(0.65,|g| {
                length(sub(hand,g.grips[side].shoulder.map(|v|v as f32)))/g.grips[side].maximum as f32
            });
            let f=self.muscles.hand_force(sub(goals[side],hand),speed,wanted_speeds[side],reach,
                self.body.max_force as f32*0.5,1./inverse_mass,dt);
            force=add(force,f);
            torque=add(torque,cross(offset,f));
        }
        // Hands also supply a bounded wrist couple about the handle. Forces
        // enter at the grips, so blocked blade contacts generate leverage.
        let mut rotation = [0.; 3];
        for j in 0..3 {
            rotation = add(
                rotation,
                scale(
                    cross(axes[j].map(|x| x as f32), wanted_axes[j].map(|x| x as f32)),
                    0.5,
                ),
            );
        }
        torque = add(torque, sub(scale(rotation, 35.), scale(angular, 4.)));
        let cap = self.body.max_torque as f32*(0.15+0.85*self.muscles.activation)*(1.-self.muscles.fatigue);
        torque = scale(torque, (cap / length(torque).max(cap)).min(1.));
        let local: V = std::array::from_fn(|j| dot(axes[j].map(|x| x as f32), torque));
        let i = self.body.inertia;
        let crossd = |a: [f64; 3], b: [f64; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let cof = [crossd(i[1], i[2]), crossd(i[2], i[0]), crossd(i[0], i[1])];
        let det = (0..3).map(|k| i[0][k] * cof[0][k]).sum::<f64>().max(1e-9);
        let accel: [f64; 3] =
            std::array::from_fn(|j| (0..3).map(|k| cof[j][k] * local[k] as f64).sum::<f64>() / det);
        let alpha: V =
            std::array::from_fn(|k| (0..3).map(|j| axes[j][k] * accel[j]).sum::<f64>() as f32);
        let w = scale(add(angular, scale(alpha, dt)), dt);
        let angle = length(w);
        let axis = unit(w);
        let displacement = add(
            scale(velocity, dt),
            scale(force, dt * dt / self.body.mass as f32),
        );
        self.p
            .iter()
            .map(|&p| {
                let r = sub(p, center);
                let rotated = if angle < 1e-8 {
                    r
                } else {
                    add(
                        add(scale(r, angle.cos()), scale(cross(axis, r), angle.sin())),
                        scale(axis, dot(axis, r) * (1. - angle.cos())),
                    )
                };
                add(add(center, displacement), rotated)
            })
            .collect()
    }
    pub fn project(&mut self, goal: &[V], response: &[[f32; 13]], dt: f32) -> Result<(), String> {
        // goal() already bounds actuator acceleration before adding momentum.
        // The generic hand-pose limiter would clamp that momentum a second
        // time, imposing a terminal speed F*dt/m and turning a strike into a
        // slow press. Contact solves the bounded drive's inertial prediction.
        let mut contact_body = self.body.clone();
        contact_body.max_force = 1e12;
        contact_body.max_torque = 1e12;
        // A moving shoulder can begin a substep just outside its reach sphere.
        // Permit recovery from that existing error, but never enlarge it. A
        // strict infeasible sphere would reject every Newton step and strand
        // the tool in the ground forever.
        let mut grasp = self.grasp.clone();
        if let (Some(g), Some((origin, axes))) = (
            &mut grasp,
            super::super::rigid_fit::fit(&self.model, &self.p),
        ) {
            for grip in &mut g.grips {
                let hand: [f64; 3] = std::array::from_fn(|k| {
                    origin[k] + (0..3).map(|j| axes[j][k] * grip.point[j]).sum::<f64>()
                });
                let distance = (0..3)
                    .map(|k| (hand[k] - grip.shoulder[k]).powi(2))
                    .sum::<f64>()
                    .sqrt();
                grip.maximum = grip.maximum.max(distance + 1e-6);
                grip.minimum = grip.minimum.min((distance - 1e-6).max(0.));
            }
        }
        let result = rigid_contact::project(
            &self.model,
            &self.p,
            goal,
            response,
            &contact_body,
            grasp.as_ref(),
            dt as f64,
        )?;
        self.p = result.positions;
        Ok(())
    }
    pub(super) fn initialize_pose(&mut self, pose: Vec<V>) {
        self.p=pose; self.v=vec![[0.;3];self.p.len()];
        self.muscles=super::muscle::Muscles::default();
    }
    pub fn finish(&mut self, old: &[V], dt: f32) {
        for ((v, p), old) in self.v.iter_mut().zip(&self.p).zip(old) {
            *v = scale(sub(*p, *old), 0.995 / dt);
        }
        self.peak_energy = self.peak_energy.max(self.kinetic_energy());
    }
    fn kinetic_energy(&self) -> f32 {
        0.5 * self.body.mass as f32 * self.v.iter().map(|v| dot(*v, *v)).sum::<f32>()
            / self.v.len() as f32
    }
    pub fn record_contact(&mut self, old: &[V], response: &[[f32; 13]], dt: f32) {
        let force = response
            .iter()
            .map(|r| length([r[0], r[1], r[2]]))
            .sum::<f32>();
        self.contact_force=force;
        self.contact_reaction=response.iter().map(|r|[r[0],r[1],r[2]]).fold([0.;3],add);
        let work = response
            .iter()
            .zip(self.p.iter().zip(old))
            .map(|(r, (p, o))| (-dot([r[0], r[1], r[2]], sub(*p, *o))).max(0.))
            .sum::<f32>();
        if force > 0.05 {
            self.contact_work += work.max(self.impact_energy(old, dt));
            self.peak_contact = self.peak_contact.max(force);
        }
    }
    pub fn set_placement(&mut self, offset: V, yaw: f32) {
        self.offset = offset;
        self.yaw = yaw;
        self.p = self.pose(0.);
        self.v.fill([0.; 3]);
    }
    pub fn retarget(&mut self, offset: V) {
        self.offset = offset;
    }
    pub fn use_soil_stroke(&mut self) {
        self.turf_stroke = false;
    }
    pub fn clear_of_sample(&self) -> bool {
        self.p.iter().all(|p| p[0] > 0.42 && p[1] > 0.55)
    }
    pub fn distance(&self, p: V) -> f32 {
        self.faces
            .iter()
            .map(|f| point_triangle(p, f.map(|i| self.p[i as usize])))
            .fold(f32::INFINITY, f32::min)
    }
    pub fn is_cutting_vertex(&self, i: usize) -> bool {
        match self.shape {
            Shape::Spade => i < 14 || (49..63).contains(&i),
            Shape::Axe => i < 5 || (25..30).contains(&i),
            Shape::Pick => (48..56).contains(&i),
            Shape::Bucket => false,
        }
    }
    pub fn set_crosscut_height(&mut self, height: f32) {
        if self.cross_grain {
            self.cut_height = height;
            self.p = self.pose(0.);
            self.v.fill([0.; 3]);
        }
    }
    pub fn cutting_normal(&self) -> V {
        super::super::rigid_fit::fit(&self.model, &self.p)
            .map(|(_, axes)| axes[2].map(|v| v as f32))
            .unwrap_or([0., 0., 1.])
    }
    pub fn edge_distance(&self, point: V) -> f32 {
        self.p
            .iter()
            .enumerate()
            .filter(|(i, _)| self.is_cutting_vertex(*i))
            .map(|(_, &p)| length(sub(point, p)))
            .fold(f32::INFINITY, f32::min)
    }
    pub fn projected_area(&self, vertex: usize, force: V, contact: [V; 3], band: f32) -> f32 {
        let n = unit(force);
        let normal = unit(cross(
            sub(contact[1], contact[0]),
            sub(contact[2], contact[0]),
        ));
        self.faces
            .iter()
            .filter(|f| f.contains(&(vertex as u32)))
            .map(|f| {
                // Only the strip of the tool surface actually near this
                // material plane bears the contact. Counting the entire axe
                // wedge face made a sharp edge behave like a broad stamp.
                let mut polygon = f.iter().map(|&i| self.p[i as usize]).collect::<Vec<_>>();
                for sign in [-1., 1.] {
                    let old = std::mem::take(&mut polygon);
                    for i in 0..old.len() {
                        let a = old[i];
                        let b = old[(i + 1) % old.len()];
                        let da = sign * dot(sub(a, contact[0]), normal) - band;
                        let db = sign * dot(sub(b, contact[0]), normal) - band;
                        if da <= 0. {
                            polygon.push(a);
                        }
                        if (da < 0.) != (db < 0.) {
                            polygon.push(add(a, scale(sub(b, a), da / (da - db))));
                        }
                    }
                }
                if polygon.len() < 3 {
                    return 0.;
                }
                (1..polygon.len() - 1)
                    .map(|i| {
                        dot(
                            cross(sub(polygon[i], polygon[0]), sub(polygon[i + 1], polygon[0])),
                            n,
                        )
                        .abs()
                            / 6.
                    })
                    .sum::<f32>()
            })
            .sum::<f32>()
    }
    pub fn report(&self, t: f32) -> Value {
        json!({"phase":self.phase(t),"drive_force_limit_n":self.force_limit.map(|f|f as f64).unwrap_or(self.body.max_force),"mass_kg":self.body.mass,
            "contact_force_n":self.contact_force,"peak_kinetic_energy_j":self.peak_energy,"contact_work_j":self.contact_work,"peak_contact_force_n":self.peak_contact,
            "crosscut_height_m":if self.cross_grain { Some(self.cut_height) } else { None },
            "kinetic_energy_j":0.5*self.body.mass*self.v.iter().map(|v|dot(*v,*v) as f64).sum::<f64>()/self.v.len() as f64})
    }
    pub(super) fn carried_samples(&self, points: impl Iterator<Item = (V, f32)>) -> usize {
        if self.shape == Shape::Bucket {
            return self.contained_grains(points);
        }
        if self.shape != Shape::Spade {
            return 0;
        }
        let Some((o, axes)) = crate::physics::rigid_fit::fit(&self.model, &self.p) else {
            return 0;
        };
        points
            .filter(|(p, r)| {
                let delta = std::array::from_fn::<_, 3, _>(|k| p[k] as f64 - o[k]);
                let q = axes.map(|a| (0..3).map(|k| a[k] * delta[k]).sum::<f64>() as f32);
                q[0].abs() < 0.15 + r
                    && q[1] > -0.30 - r
                    && q[1] < 0.015 + r
                    && q[2] > -0.05 - r
                    && q[2] < 0.08 + r
            })
            .count()
    }
    pub fn contained_grains(&self, grains: impl Iterator<Item = (V, f32)>) -> usize {
        if self.shape != Shape::Bucket {
            return 0;
        }
        let Some((origin, axes)) = super::super::rigid_fit::fit(&self.model, &self.p) else {
            return 0;
        };
        grains
            .filter(|(p, r)| {
                let delta = std::array::from_fn::<_, 3, _>(|k| p[k] as f64 - origin[k]);
                let local = axes.map(|a| (0..3).map(|k| a[k] * delta[k]).sum::<f64>() as f32);
                local[0].abs() + r < 0.15
                    && local[1] < -*r
                    && (local[1] * local[1] + local[2] * local[2]).sqrt() + r < 0.16
            })
            .count()
    }
    pub fn impact_energy(&self, old: &[V], dt: f32) -> f32 {
        // Contact can stop a strike within the clearance band with almost no
        // accepted travel. Its lost incoming kinetic energy still loads the
        // material. Use the same lumped head energy as the workshop report;
        // this is an estimate, not an additional scripted fracture impulse.
        let incoming = 0.5 * self.body.mass as f32 / self.v.len() as f32
            * self.v.iter().map(|v| dot(*v, *v)).sum::<f32>();
        let outgoing = 0.5 * self.body.mass as f32 / (self.p.len() as f32 * dt * dt)
            * self
                .p
                .iter()
                .zip(old)
                .map(|(p, o)| dot(sub(*p, *o), sub(*p, *o)))
                .sum::<f32>();
        // A stationary actuator pushing a blocked tool supplies no kinetic
        // work. Do not count the unrealized free prediction as impact energy.
        (incoming - outgoing).max(0.)
    }
    pub fn render(&self, out: &mut Vec<[f32; 6]>) {
        for (i, f) in self.faces.iter().enumerate() {
            let color =
                if self.physical_handle && i >= self.head_faces && self.shape != Shape::Bucket {
                    [0.43, 0.25, 0.10]
                } else {
                    [0.49, 0.57, 0.61]
                };
            skin::triangle(out, f.map(|i| self.p[i as usize]), color);
        }
        if self.physical_handle {
            return;
        }
        if let Some((origin, axes)) = super::super::rigid_fit::fit(&self.model, &self.p) {
            let point = |p: V| {
                std::array::from_fn(|i| {
                    (origin[i] + (0..3).map(|j| axes[j][i] * p[j] as f64).sum::<f64>()) as f32
                })
            };
            if self.shape == Shape::Bucket {
                for k in 0..10 {
                    let arc = |i: usize| {
                        let a = i as f32 * std::f32::consts::PI / 10.;
                        point([0.15 * a.cos(), 0.20 * a.sin(), 0.])
                    };
                    skin::rod(out, arc(k), arc(k + 1), 0.008, [0.34, 0.40, 0.43]);
                }
                return;
            }
            if self.transverse_haft() {
                let a = point([-0.045, 0.008, 0.]);
                let b = point([0.32, 0.016, 0.]);
                let c = point([0.70, 0.05, 0.]);
                skin::rod(out, a, b, 0.018, [0.52, 0.31, 0.13]);
                skin::rod(out, b, c, 0.021, [0.43, 0.25, 0.10]);
                skin::rod(out, point([0.58, 0.038, 0.]), c, 0.024, [0.26, 0.16, 0.08]);
                return;
            }
            let end = 0.58;
            skin::rod(
                out,
                point([0., 0.01, 0.]),
                point([0., end, 0.]),
                0.016,
                [0.43, 0.25, 0.10],
            );
            if self.shape == Shape::Spade {
                skin::rod(
                    out,
                    point([-0.07, end, 0.]),
                    point([0.07, end, 0.]),
                    0.019,
                    [0.24, 0.29, 0.31],
                );
            }
        }
    }
}
pub(super) fn point_triangle(p: V, t: [V; 3]) -> f32 {
    let [a, b, c] = t;
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0. && d2 <= 0. {
        return length(ap);
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0. && d4 <= d3 {
        return length(bp);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0. && d1 >= 0. && d3 <= 0. {
        return length(sub(p, add(a, scale(ab, d1 / (d1 - d3)))));
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0. && d5 <= d6 {
        return length(cp);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0. && d2 >= 0. && d6 <= 0. {
        return length(sub(p, add(a, scale(ac, d2 / (d2 - d6)))));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0. && (d4 - d3) >= 0. && (d5 - d6) >= 0. {
        return length(sub(
            p,
            add(b, scale(sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6)))),
        ));
    }
    dot(ap, unit(cross(ab, ac))).abs()
}
