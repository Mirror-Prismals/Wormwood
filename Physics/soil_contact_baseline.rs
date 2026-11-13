//! Isolated spade/soil contact fixture. Fracture is deliberately not claimed.
use super::{Scene, math::*};
use serde_json::{Value, json};

#[derive(Clone)]
pub(super) struct Probe {
    blade: std::ops::Range<usize>,
    soil: std::ops::Range<usize>,
    start_center: V,
    target_drop: f32,
    peak_contacts: u64,
    peak_blade_force: f64,
}

impl Probe {
    fn center(&self, s: &Scene) -> V {
        let count = self.blade.len() as f32;
        self.blade
            .clone()
            .map(|i| s.p[i])
            .fold([0.; 3], add)
            .map(|v| v / count)
    }
    pub(super) fn drive(&mut self, s: &mut Scene, dt: f32) {
        // A force-limited servo, not a kinematic pin: OGC gets the final say
        // about each accepted position. Hold the target after insertion.
        self.target_drop = (s.time * 0.14).min(0.27);
        let target = add(self.start_center, [0., -self.target_drop, 0.]);
        let center = self.center(s);
        let mean_velocity = self
            .blade
            .clone()
            .map(|i| s.velocity[i])
            .fold([0.; 3], add)
            .map(|v| v / self.blade.len() as f32);
        let acceleration = add(scale(sub(target, center), 70.), scale(mean_velocity, -14.));
        let acceleration = scale(acceleration, (5. / length(acceleration).max(5.)).min(1.));
        for i in self.blade.clone() {
            s.velocity[i] = add(s.velocity[i], scale(acceleration, dt));
        }
    }
    pub(super) fn observe(&mut self, contact: &Value) {
        self.peak_contacts = self.peak_contacts.max(
            contact["vertex_face_contacts"].as_u64().unwrap_or(0)
                + contact["edge_edge_contacts"].as_u64().unwrap_or(0),
        );
        if let Some(forces) = contact["forces"].as_array() {
            let sum = self
                .blade
                .clone()
                .filter_map(|i| forces.get(i).and_then(Value::as_array))
                .map(|v| {
                    v.iter()
                        .map(|x| x.as_f64().unwrap_or(0.).powi(2))
                        .sum::<f64>()
                        .sqrt()
                })
                .sum::<f64>();
            self.peak_blade_force = self.peak_blade_force.max(sum);
        }
    }
    pub(super) fn report(&self, s: &Scene) -> Value {
        let center = self.center(s);
        let max_soil_displacement = self
            .soil
            .clone()
            .map(|i| length(sub(s.p[i], s.rest[i])))
            .fold(0f32, f32::max);
        json!({"target_drop_m":self.target_drop,"accepted_blade_drop_m":self.start_center[1]-center[1],
            "blade_center":center,"peak_contact_pairs":self.peak_contacts,"peak_blade_force_newtons":self.peak_blade_force,
            "maximum_soil_displacement_m":max_soil_displacement,
            "fracture_implemented":false,"detached_soil":false,
            "limitations":"Deformable tetrahedral block and compliant spade; no fracture, roots, compaction or detached fragments."})
    }
}

pub(super) fn fixture(s: &mut Scene) {
    s.radius = 0.004;
    s.substeps = 4;
    s.iterations = 8;
    let n = 4usize;
    let start = s.p.len();
    let layer = (n + 1) * (n + 1);
    for y in 0..2 {
        for z in 0..=n {
            for x in 0..=n {
                s.node(
                    [
                        -0.6 + 1.2 * x as f32 / n as f32,
                        0.07 + y as f32 * 0.22,
                        -0.6 + 1.2 * z as f32 / n as f32,
                    ],
                    if y == 0 { 0. } else { 0.4 },
                    [0.37, 0.25, 0.12],
                );
            }
        }
    }
    let mut faces = Vec::new();
    for z in 0..n {
        for x in 0..n {
            let a = start + z * (n + 1) + x;
            let b = a + 1;
            let c = a + n + 1;
            let d = c + 1;
            let u = layer;
            for tet in [
                [a, b, d, d + u],
                [a, d, c, d + u],
                [a, c, c + u, d + u],
                [a, c + u, a + u, d + u],
                [a, a + u, b + u, d + u],
                [a, b + u, b, d + u],
            ] {
                s.tetra(tet);
            }
            let (a, b, c, d) = (
                (a - start) as u32,
                (b - start) as u32,
                (c - start) as u32,
                (d - start) as u32,
            );
            let u = u as u32;
            faces.extend([
                [a, b, c],
                [b, d, c],
                [a + u, c + u, b + u],
                [b + u, c + u, d + u],
            ]);
        }
    }
    let mut boundary = Vec::new();
    boundary.extend(0..n);
    boundary.extend((0..n).map(|z| z * (n + 1) + n));
    boundary.extend((1..=n).rev().map(|x| n * (n + 1) + x));
    boundary.extend((1..=n).rev().map(|z| z * (n + 1)));
    for k in 0..boundary.len() {
        let a = boundary[k] as u32;
        let b = boundary[(k + 1) % boundary.len()] as u32;
        let u = layer as u32;
        faces.extend([[a, a + u, b], [b, a + u, b + u]]);
    }
    s.surface(start, faces, false, true);
    let end = s.p.len();
    let blade = s.p.len();
    for y in [0.46, 0.82] {
        for z in [-0.025, 0.025] {
            for x in [-0.10, 0.10] {
                s.node([x, y, z], 0.15, [0.66, 0.68, 0.72]);
            }
        }
    }
    for tet in [
        [0, 1, 3, 7],
        [0, 3, 2, 7],
        [0, 2, 6, 7],
        [0, 6, 4, 7],
        [0, 4, 5, 7],
        [0, 5, 1, 7],
    ] {
        s.tetra(tet.map(|i| blade + i));
    }
    s.surface(
        blade,
        vec![
            [0, 2, 1],
            [1, 2, 3],
            [4, 5, 6],
            [5, 7, 6],
            [0, 1, 4],
            [1, 5, 4],
            [2, 6, 3],
            [3, 6, 7],
            [0, 4, 2],
            [2, 4, 6],
            [1, 3, 5],
            [3, 7, 5],
        ],
        false,
        false,
    );
    s.contact_probe = Some(Probe {
        blade: blade..blade + 8,
        soil: start..end,
        start_center: [0., 0.64, 0.],
        target_drop: 0.,
        peak_contacts: 0,
        peak_blade_force: 0.,
    });
    s.proxy="Mirror soil contact experiment: an intact deformable tetrahedral patch and force-driven compliant spade. No fracture or soil separation is implemented.".into();
}
