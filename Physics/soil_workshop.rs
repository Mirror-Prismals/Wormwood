//! Automatic cohesive-soil excavation showcase. OGC supplies contact; XPBD
//! supplies elasticity and breakable interfaces. Strengths are artistic values.
//! Cells exist before the stroke, retain mass, and never attach to the tool.
use super::{Scene, math::*};
use serde_json::{Value, json};

const FACES: [[u32; 3]; 12] = [
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
];
const TETS: [[usize; 4]; 6] = [
    [0, 1, 3, 7],
    [0, 3, 2, 7],
    [0, 2, 6, 7],
    [0, 6, 4, 7],
    [0, 4, 5, 7],
    [0, 5, 1, 7],
];
#[derive(Clone)]
struct Cell {
    start: usize,
    anchored: bool,
    rest_center: V,
    mass: f32,
}
#[derive(Clone)]
struct Interface {
    cells: [usize; 2],
    pairs: [(usize, usize); 4],
    rest_offset: [V; 4],
    normal_sign: f32,
    lambda: [V; 4],
    strength: f32,
    damage: f32,
    demand: f32,
    broken: bool,
}
#[derive(Clone)]
pub(super) struct Probe {
    blade: std::ops::Range<usize>,
    soil: std::ops::Range<usize>,
    cells: Vec<Cell>,
    interfaces: Vec<Interface>,
    peak_contacts: u64,
    peak_blade_force: f64,
    target_drop: f32,
}
fn smooth(t: f32, start: f32, end: f32) -> f32 {
    let x = ((t - start) / (end - start)).clamp(0., 1.);
    x * x * (3. - 2. * x)
}
fn mean(s: &Scene, start: usize) -> V {
    scale(
        s.p[start..start + 8].iter().copied().fold([0.; 3], add),
        0.125,
    )
}
impl Probe {
    pub(super) fn phase(&self, t: f32) -> &'static str {
        if t < 0.8 {
            "Approach"
        } else if t < 3.8 {
            "Press into soil"
        } else if t < 6.8 {
            "Lever"
        } else if t < 9.8 {
            "Lift"
        } else if t < 11. {
            "Hold"
        } else if t < 13. {
            "Tip and release"
        } else {
            "Settling — reset to replay"
        }
    }
    pub(super) fn drive(&mut self, s: &mut Scene, dt: f32) {
        for bond in &mut self.interfaces {
            bond.lambda = [[0.; 3]; 4];
            bond.demand = 0.;
        }
        let t = s.time;
        self.target_drop = 0.31 * smooth(t, 0.8, 3.8);
        // The shoulder stays near the surface while the lower edge sweeps
        // underneath the earth. Rotation is requested through finite forces;
        // neither the soil nor the blade is teleported to this trajectory.
        let angle = -1.12 * smooth(t, 3.8, 6.8) - 1.7 * smooth(t, 11., 13.);
        let pivot = [
            0.,
            0.72 - self.target_drop + 0.36 * smooth(t, 6.8, 9.8),
            -0.16 * smooth(t, 6.8, 9.8),
        ];
        for i in self.blade.clone() {
            let local = sub(s.rest[i], [0., 0.72, 0.]);
            let target = add(pivot, rotate_x(local, [0.; 3], angle));
            let acceleration = sub(
                add(scale(sub(target, s.p[i]), 380.), scale(s.velocity[i], -34.)),
                s.gravity,
            );
            // Eight nodes total 2.4 kg; capped actuator force is 288 N.
            let a = scale(
                acceleration,
                (120. / length(acceleration).max(120.)).min(1.),
            );
            s.velocity[i] = add(s.velocity[i], scale(a, dt));
        }
    }
    pub(super) fn solve(&mut self, s: &mut Scene, dt: f32) {
        let h2 = dt * dt;
        for bond in &mut self.interfaces {
            if bond.broken {
                continue;
            }
            // Co-rotating rest gap: rotating a clump does not itself tear it.
            let a = bond.pairs.map(|(a, _)| s.p[a]);
            let normal = scale(
                unit(cross(sub(a[1], a[0]), sub(a[2], a[0]))),
                bond.normal_sign,
            );
            let tangent = unit(sub(a[1], a[0]));
            let across = unit(cross(normal, tangent));
            let alpha = 2e-7 / h2;
            for k in 0..4 {
                let (a, b) = bond.pairs[k];
                let offset = bond.rest_offset[k];
                let rest = add(
                    add(scale(tangent, offset[0]), scale(across, offset[1])),
                    scale(normal, offset[2]),
                );
                let error = sub(sub(s.p[b], s.p[a]), rest);
                let denom = s.weights[a] + s.weights[b] + alpha;
                if denom <= 0. {
                    continue;
                }
                let dl = scale(
                    add(scale(error, -1.), scale(bond.lambda[k], -alpha)),
                    1. / denom,
                );
                let next = add(bond.lambda[k], dl);
                let traction = scale(next, -1. / h2);
                let normal_load = dot(traction, normal);
                let shear = length(sub(traction, scale(normal, normal_load)));
                let capacity = bond.strength * (1. - 0.8 * bond.damage);
                // Compression supports weight without tensile damage. Friction
                // raises shear resistance when an interface is compressed.
                let demand = ((normal_load.max(0.) / capacity).powi(2)
                    + (shear / (capacity + 0.55 * (-normal_load).max(0.))).powi(2))
                .sqrt();
                bond.demand = bond.demand.max(demand);
                bond.lambda[k] = next;
                s.p[a] = sub(s.p[a], scale(dl, s.weights[a]));
                s.p[b] = add(s.p[b], scale(dl, s.weights[b]));
            }
        }
    }
    pub(super) fn end_step(&mut self, dt: f32) {
        for bond in &mut self.interfaces {
            if !bond.broken {
                // Accumulate once per substep, never once per solver iteration.
                bond.damage = (bond.damage + dt * 7. * (bond.demand - 1.).max(0.)).min(1.);
                bond.broken = bond.damage >= 1.;
            }
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
    fn supported(&self) -> Vec<bool> {
        let mut supported: Vec<_> = self.cells.iter().map(|c| c.anchored).collect();
        loop {
            let mut changed = false;
            for b in self.interfaces.iter().filter(|b| !b.broken) {
                let [a, c] = b.cells;
                if supported[a] != supported[c] {
                    supported[a] = true;
                    supported[c] = true;
                    changed = true;
                }
            }
            if !changed {
                return supported;
            }
        }
    }
    pub(super) fn report(&self, s: &Scene) -> Value {
        let supported = self.supported();
        let mut detached = 0usize;
        let mut lifted = 0usize;
        let mut detached_mass = 0.;
        let mut max_lift = 0f32;
        for (i, cell) in self.cells.iter().enumerate() {
            if !supported[i] {
                detached += 1;
                detached_mass += cell.mass;
                let lift = mean(s, cell.start)[1] - cell.rest_center[1];
                max_lift = max_lift.max(lift);
                if lift > 0.04 {
                    lifted += 1;
                }
            }
        }
        json!({"phase":self.phase(s.time),"target_drop_m":self.target_drop,
            "accepted_blade_drop_m":0.58-mean(s,self.blade.start)[1],
            "blade_center":mean(s,self.blade.start),"peak_contact_pairs":self.peak_contacts,
            "peak_blade_force_newtons":self.peak_blade_force,
            "maximum_soil_displacement_m":self.soil.clone().map(|i|length(sub(s.p[i],s.rest[i]))).fold(0f32,f32::max),
            "fracture_implemented":true,"interfaces":self.interfaces.len(),
            "broken_interfaces":self.interfaces.iter().filter(|b|b.broken).count(),
            "detached_soil":detached>0,"detached_cells":detached,"lifted_cells":lifted,
            "detached_mass_kg":detached_mass,"detached_lift_m":max_lift,
            "limitations":"Prepartitioned cohesive cells with numerical contact gaps; uncalibrated strength, no arbitrary remeshing, roots or compaction. Automatic finite-force tool trajectory; outcome requires playtesting."})
    }
    pub(super) fn decorate(&self, s: &Scene, vertices: &mut Vec<[f32; 6]>) {
        // Shovel shaft and D grip follow the accepted blade frame, not its target.
        let top = scale(
            add(s.p[self.blade.start + 4], s.p[self.blade.start + 5]),
            0.5,
        );
        let bottom = scale(add(s.p[self.blade.start], s.p[self.blade.start + 1]), 0.5);
        let up = unit(sub(top, bottom));
        let right = unit(sub(s.p[self.blade.start + 5], s.p[self.blade.start + 4]));
        let grip = add(top, scale(up, 0.43));
        rod(vertices, top, grip, 0.014, [0.52, 0.32, 0.13]);
        let left = sub(grip, scale(right, 0.065));
        let right_grip = add(grip, scale(right, 0.065));
        rod(
            vertices,
            sub(grip, scale(up, 0.10)),
            left,
            0.011,
            [0.23, 0.27, 0.30],
        );
        rod(
            vertices,
            sub(grip, scale(up, 0.10)),
            right_grip,
            0.011,
            [0.23, 0.27, 0.30],
        );
        rod(vertices, left, right_grip, 0.015, [0.23, 0.27, 0.30]);
        // Short visual turf is anchored to the real top faces and follows clods.
        // This is presentation only; no independent grass/root physics claimed.
        for c in &self.cells {
            if s.rest[c.start + 4][1] < 0.32 {
                continue;
            }
            let p = [
                s.p[c.start + 4],
                s.p[c.start + 5],
                s.p[c.start + 6],
                s.p[c.start + 7],
            ];
            let n = unit(cross(sub(p[2], p[0]), sub(p[1], p[0])));
            for k in 0..5 {
                let u = 0.15 + (k as f32 * 0.37).fract() * 0.7;
                let v = 0.15 + (k as f32 * 0.61).fract() * 0.7;
                let root = add(
                    scale(add(scale(p[0], 1. - u), scale(p[1], u)), 1. - v),
                    scale(add(scale(p[2], 1. - u), scale(p[3], u)), v),
                );
                let side = scale(unit(sub(p[1], p[0])), 0.003);
                let tip = add(
                    root,
                    add(scale(n, 0.027 + 0.004 * k as f32), scale(side, 2.)),
                );
                triangle(
                    vertices,
                    [sub(root, side), add(root, side), tip],
                    [0.24, 0.42, 0.10],
                );
            }
        }
    }
}
fn triangle(out: &mut Vec<[f32; 6]>, p: [V; 3], color: V) {
    let n = unit(cross(sub(p[1], p[0]), sub(p[2], p[0])));
    let light = 0.35 + 0.65 * dot(n, unit([0.4, 0.8, 0.5])).abs();
    out.extend(p.map(|p| {
        [
            p[0],
            p[1],
            p[2],
            color[0] * light,
            color[1] * light,
            color[2] * light,
        ]
    }));
}
fn rod(out: &mut Vec<[f32; 6]>, a: V, b: V, radius: f32, color: V) {
    let axis = unit(sub(b, a));
    let reference = if axis[0].abs() < 0.8 {
        [1., 0., 0.]
    } else {
        [0., 0., 1.]
    };
    let u = scale(unit(cross(axis, reference)), radius);
    let v = scale(unit(cross(axis, u)), radius);
    for k in 0..8 {
        let p = |i: usize| {
            let t = i as f32 * std::f32::consts::TAU / 8.;
            add(scale(u, t.cos()), scale(v, t.sin()))
        };
        let x = p(k);
        let y = p(k + 1);
        triangle(out, [add(a, x), add(b, x), add(b, y)], color);
        triangle(out, [add(a, x), add(b, y), add(a, y)], color);
        triangle(out, [a, add(a, y), add(a, x)], color);
        triangle(out, [b, add(b, x), add(b, y)], color);
    }
}
fn solid(s: &mut Scene, points: [V; 8], mass: f32, top: bool) -> usize {
    let start = s.p.len();
    for (i, p) in points.into_iter().enumerate() {
        s.node(
            p,
            mass / 8.,
            if top && i >= 4 {
                [0.24, 0.36, 0.10]
            } else {
                [0.34, 0.22, 0.115]
            },
        );
    }
    for tet in TETS {
        s.tetra(tet.map(|i| start + i));
    }
    s.surface(start, FACES.to_vec(), mass == 0., false);
    start
}
pub(super) fn fixture(s: &mut Scene) {
    s.radius = 0.0006;
    s.substeps = 6;
    s.iterations = 8;
    const N: usize = 6;
    const Y: usize = 3;
    let soil_start = s.p.len();
    let mut cells = Vec::new();
    // Shared uneven reference lattice, then tiny separation for OGC offsets.
    // The central seam admits the tapered edge; fracture is resolved at this
    // cell scale, not claimed to be arbitrary cutting through cell interiors.
    let corner = |x: usize, y: usize, z: usize| -> V {
        let jitter = if y == Y {
            0.004 * ((x * 17 + z * 31) as f32).sin()
        } else {
            0.
        };
        [
            -0.30 + x as f32 * 0.10,
            0.065 + y as f32 * 0.09 + jitter,
            -0.30 + z as f32 * 0.10,
        ]
    };
    for y in 0..Y {
        for z in 0..N {
            for x in 0..N {
                let center = scale(add(corner(x, y, z), corner(x + 1, y + 1, z + 1)), 0.5);
                let points = std::array::from_fn(|i| {
                    let p = corner(x + (i & 1), y + (i >> 2), z + ((i >> 1) & 1));
                    add(center, scale(sub(p, center), 0.96))
                });
                let anchored = y == 0;
                let mass = 0.10 * 0.10 * 0.09 * 0.96f32.powi(3) * 1400.;
                let start = solid(s, points, if anchored { 0. } else { mass }, y == Y - 1);
                cells.push(Cell {
                    start,
                    anchored,
                    rest_center: mean(s, start),
                    mass,
                });
            }
        }
    }
    let soil_end = s.p.len();
    let mut interfaces = Vec::new();
    for y in 0..Y {
        for z in 0..N {
            for x in 0..N {
                let id = (y * N + z) * N + x;
                for (valid, other, fa, fb) in [
                    (x + 1 < N, id + 1, [1, 3, 5, 7], [0, 2, 4, 6]),
                    (z + 1 < N, id + N, [2, 3, 6, 7], [0, 1, 4, 5]),
                    (y + 1 < Y, id + N * N, [4, 5, 6, 7], [0, 1, 2, 3]),
                ] {
                    if !valid {
                        continue;
                    }
                    let pairs = std::array::from_fn(|k| {
                        (cells[id].start + fa[k], cells[other].start + fb[k])
                    });
                    let points = fa.map(|k| s.p[cells[id].start + k]);
                    let normal = unit(cross(sub(points[1], points[0]), sub(points[2], points[0])));
                    let gap = sub(s.p[pairs[0].1], s.p[pairs[0].0]);
                    let sign = if dot(normal, gap) >= 0. { 1. } else { -1. };
                    let normal = scale(normal, sign);
                    let tangent = unit(sub(points[1], points[0]));
                    let across = unit(cross(normal, tangent));
                    let rest_offset = pairs.map(|(a, b)| {
                        let d = sub(s.p[b], s.p[a]);
                        [dot(d, tangent), dot(d, across), dot(d, normal)]
                    });
                    let area = length(cross(sub(points[1], points[0]), sub(points[2], points[0])));
                    // Mild deterministic heterogeneity gives cracks alternative paths.
                    let strength =
                        area * 1500. / 4. * (0.8 + 0.4 * ((id * 13 + other * 7) % 19) as f32 / 18.);
                    interfaces.push(Interface {
                        cells: [id, other],
                        pairs,
                        rest_offset,
                        normal_sign: sign,
                        lambda: [[0.; 3]; 4],
                        strength,
                        damage: 0.,
                        demand: 0.,
                        broken: false,
                    });
                }
            }
        }
    }
    let blade = s.p.len();
    // Finite wedge: sharp lower edge, thicker shoulder. Eight nodes, one body.
    let points = std::array::from_fn(|i| {
        let upper = i >= 4;
        [
            if i & 1 == 0 { -0.115 } else { 0.115 },
            if upper { 0.72 } else { 0.44 },
            (if i & 2 == 0 { -1. } else { 1. }) * if upper { 0.009 } else { 0.0005 },
        ]
    });
    solid(s, points, 2.4, false);
    for c in &mut s.colors[blade..blade + 8] {
        *c = [0.65, 0.71, 0.75];
    }
    // A fixed receiving tray keeps released soil in view. It is a real OGC
    // surface and does not constrain any detached cell to its old position.
    let tray = std::array::from_fn(|i| {
        [
            if i & 1 == 0 { -0.8 } else { 0.8 },
            if i >= 4 { 0.025 } else { 0. },
            if i & 2 == 0 { -0.8 } else { 0.8 },
        ]
    });
    let floor = solid(s, tray, 0., false);
    for c in &mut s.colors[floor..floor + 8] {
        *c = [0.19, 0.23, 0.25];
    }
    s.soil_probe = Some(Probe {
        blade: blade..blade + 8,
        soil: soil_start..soil_end,
        cells,
        interfaces,
        peak_contacts: 0,
        peak_blade_force: 0.,
        target_drop: 0.,
    });
    s.proxy="Automatic cohesive excavation: prepartitioned elastic soil cells, load-damaged interfaces, tapered force-driven spade and retained contact-driven debris. Finite cell resolution and numerical gaps; uncalibrated material. No runtime outcome verified for this revision.".into();
}
