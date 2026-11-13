//! Bounded implicit affine material pieces. Twelve freedoms per piece retain
//! rotation, shear and compression after geometric subdivision. Native OGC
//! responses are pulled back through the same affine vertex map.
use super::cut_geometry::*;
pub(super) type Matrix = [[f64; 12]; 12];
pub(super) type Vector = [f64; 12];
#[derive(Clone)]
pub(super) struct Contact {
    pub a: usize,
    pub b: usize,
    pub normal: D,
    /// Shared material coordinates for a touching interface. Empty for pieces
    /// separated by intervening material, which use convex support points.
    pub points: Vec<D>,
}
pub(super) fn linear(mut a: Matrix, mut b: Vector) -> Option<Vector> {
    for col in 0..12 {
        let pivot = (col..12).max_by(|i, j| a[*i][col].abs().total_cmp(&a[*j][col].abs()))?;
        if !a[pivot][col].is_finite() || a[pivot][col].abs() < 1e-18 {
            return None;
        }
        a.swap(pivot, col);
        b.swap(pivot, col);
        let d = a[col][col];
        for j in col..12 {
            a[col][j] /= d;
        }
        b[col] /= d;
        for row in 0..12 {
            if row == col {
                continue;
            }
            let s = a[row][col];
            for j in col..12 {
                a[row][j] -= s * a[col][j];
            }
            b[row] -= s * b[col];
        }
    }
    b.iter().all(|x| x.is_finite()).then_some(b)
}
pub(super) fn identity() -> [D; 3] {
    [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]
}
pub(super) fn apply(f: [D; 3], v: D) -> D {
    (0..3).fold([0.; 3], |a, i| add(a, mul(f[i], v[i])))
}
pub(super) fn determinant(f: [D; 3]) -> f64 {
    dot(f[0], cross(f[1], f[2]))
}
pub(super) fn inverse(f: [D; 3]) -> Option<[D; 3]> {
    let d = determinant(f);
    if !d.is_finite() || d.abs() < 1e-12 {
        return None;
    }
    let rows = [
        mul(cross(f[1], f[2]), 1. / d),
        mul(cross(f[2], f[0]), 1. / d),
        mul(cross(f[0], f[1]), 1. / d),
    ];
    Some(std::array::from_fn(|j| std::array::from_fn(|i| rows[i][j])))
}
fn rotation(mut f: [D; 3]) -> [D; 3] {
    for _ in 0..16 {
        let Some(inv) = inverse(f) else {
            return identity();
        };
        let mut change = 0f64;
        for j in 0..3 {
            for i in 0..3 {
                let next = (f[j][i] + inv[i][j]) * 0.5;
                change = change.max((next - f[j][i]).abs());
                f[j][i] = next;
            }
        }
        if change < 1e-10 {
            break;
        }
    }
    f
}
pub(super) fn jacobian(r: D) -> [[f64; 12]; 3] {
    std::array::from_fn(|i| {
        let mut row = [0.; 12];
        row[i] = 1.;
        for j in 0..3 {
            row[3 + j * 3 + i] = r[j];
        }
        row
    })
}
#[derive(Clone)]
pub(super) struct Body {
    pub id: u64,
    pub ancestors: Vec<u64>,
    pub poly: Poly,
    pub reference_center: D,
    pub center: D,
    pub deformation: [D; 3],
    pub velocity: D,
    pub deformation_velocity: [D; 3],
    pub mass: f64,
    pub rest_volume: f64,
    pub anchors: Vec<D>,
    pub initial_center: D,
}
impl Body {
    pub fn fully_fixed(&self) -> bool {
        if self.anchors.len() < 4 {
            return false;
        }
        let a = self.anchors[0];
        for i in 1..self.anchors.len() {
            for j in i + 1..self.anchors.len() {
                let normal = cross(sub(self.anchors[i], a), sub(self.anchors[j], a));
                if length(normal) > 1e-10
                    && self
                        .anchors
                        .iter()
                        .any(|p| dot(normal, sub(*p, a)).abs() > 1e-10)
                {
                    return true;
                }
            }
        }
        false
    }
    pub fn new(id: u64, poly: Poly, density: f64, bottom: f64) -> Self {
        let rest = poly.measure(true);
        let now = poly.measure(false);
        let anchors = poly
            .vertices
            .iter()
            .filter(|v| v.material[1] <= bottom + 1e-7)
            .map(|v| v.material)
            .collect();
        Self {
            id,
            ancestors: Vec::new(),
            poly,
            reference_center: rest.center,
            center: now.center,
            deformation: identity(),
            velocity: [0.; 3],
            deformation_velocity: [[0.; 3]; 3],
            mass: rest.volume * density,
            rest_volume: rest.volume,
            anchors,
            initial_center: now.center,
        }
    }
    pub fn point(&self, material: D) -> D {
        add(
            self.center,
            apply(self.deformation, sub(material, self.reference_center)),
        )
    }
    pub fn material_point(&self, current: D) -> Option<D> {
        Some(add(
            self.reference_center,
            apply(inverse(self.deformation)?, sub(current, self.center)),
        ))
    }
    pub fn point_velocity(&self, material: D) -> D {
        add(
            self.velocity,
            apply(
                self.deformation_velocity,
                sub(material, self.reference_center),
            ),
        )
    }
    pub fn update_vertices(&mut self) {
        let center = self.center;
        let reference = self.reference_center;
        let f = self.deformation;
        let velocity = self.velocity;
        let df = self.deformation_velocity;
        for v in &mut self.poly.vertices {
            v.position = add(center, apply(f, sub(v.material, reference)));
            v.velocity = add(velocity, apply(df, sub(v.material, reference)));
        }
    }
    pub fn split(&self, plane: Plane, tag: u64, ids: [u64; 2]) -> Option<[Self; 2]> {
        let parts = self.poly.split(plane, tag)?;
        let density = self.mass / self.rest_volume;
        Some(std::array::from_fn(|i| {
            let poly = parts[i].clone();
            let rest = poly.measure(true);
            let mut ancestors = self.ancestors.clone();
            ancestors.push(self.id);
            let anchors = poly
                .vertices
                .iter()
                .filter(|v| {
                    self.anchors
                        .iter()
                        .any(|p| length(sub(*p, v.material)) < 1e-7)
                        || (self.anchors.len() >= 3
                            && self
                                .anchors
                                .iter()
                                .all(|p| (p[1] - v.material[1]).abs() < 1e-7))
                })
                .map(|v| v.material)
                .collect();
            let mut out = Self {
                id: ids[i],
                ancestors,
                poly,
                reference_center: rest.center,
                center: self.point(rest.center),
                deformation: self.deformation,
                velocity: self.point_velocity(rest.center),
                deformation_velocity: self.deformation_velocity,
                mass: rest.volume * density,
                rest_volume: rest.volume,
                anchors,
                initial_center: self.initial_center,
            };
            out.update_vertices();
            out
        }))
    }
    fn inertia(&self, dt: f64) -> Matrix {
        let m = self.poly.measure(true);
        let trace = (0..3).map(|i| m.inertia[i][i]).sum::<f64>() * 0.5;
        let covariance: [D; 3] = std::array::from_fn(|i| {
            std::array::from_fn(|j| (if i == j { trace } else { 0. }) - m.inertia[i][j])
        });
        let density = self.mass / self.rest_volume;
        let mut a = [[0.; 12]; 12];
        for i in 0..3 {
            a[i][i] = self.mass / (dt * dt);
            for j in 0..3 {
                for k in 0..3 {
                    a[3 + j * 3 + i][3 + k * 3 + i] = density * covariance[j][k] / (dt * dt);
                }
            }
        }
        a
    }
    /// Exact affine pullback of a point force and its PSD Hessian.
    pub fn add_force(a: &mut Matrix, rhs: &mut Vector, reference: D, force: D, hessian: [D; 3]) {
        Self::add_weighted_force(
            a,
            rhs,
            [1., reference[0], reference[1], reference[2]],
            force,
            hessian,
        )
    }
    pub fn add_internal_force(
        a: &mut Matrix,
        rhs: &mut Vector,
        reference_delta: D,
        force: D,
        hessian: [D; 3],
    ) {
        Self::add_weighted_force(
            a,
            rhs,
            [
                0.,
                reference_delta[0],
                reference_delta[1],
                reference_delta[2],
            ],
            force,
            hessian,
        )
    }
    fn add_weighted_force(
        a: &mut Matrix,
        rhs: &mut Vector,
        weights: [f64; 4],
        force: D,
        hessian: [D; 3],
    ) {
        let indices = |axis: usize| [axis, 3 + axis, 6 + axis, 9 + axis];
        for axis in 0..3 {
            let rows = indices(axis);
            for k in 0..4 {
                rhs[rows[k]] += weights[k] * force[axis];
            }
            for other in 0..3 {
                let h = hessian[other][axis];
                if h == 0. {
                    continue;
                }
                let cols = indices(other);
                for k in 0..4 {
                    for l in 0..4 {
                        a[rows[k]][cols[l]] += weights[k] * h * weights[l];
                    }
                }
            }
        }
    }
    pub fn system(&self, prior: &Self, dt: f64) -> (Matrix, Vector) {
        let mut a = self.inertia(dt);
        let mut rhs = [0.; 12];
        let desired_center = add(prior.center, mul(prior.velocity, dt));
        let dc = sub(desired_center, self.center);
        let mut desired = [0.; 12];
        desired[..3].copy_from_slice(&dc);
        for j in 0..3 {
            for i in 0..3 {
                desired[3 + j * 3 + i] = prior.deformation[j][i]
                    + prior.deformation_velocity[j][i] * dt
                    - self.deformation[j][i];
            }
        }
        for i in 0..12 {
            rhs[i] = (0..12).map(|j| a[i][j] * desired[j]).sum();
        }
        rhs[1] -= self.mass * 9.81;
        // Corotated elastic energy plus a volumetric penalty. The changing
        // polar factor is updated each coupled iteration; no world-axis springs.
        let shear = 18000.;
        let bulk = 45000.;
        let r = rotation(self.deformation);
        let cof = [
            cross(self.deformation[1], self.deformation[2]),
            cross(self.deformation[2], self.deformation[0]),
            cross(self.deformation[0], self.deformation[1]),
        ];
        let jdet = determinant(self.deformation);
        for j in 0..3 {
            for i in 0..3 {
                let row = 3 + j * 3 + i;
                rhs[row] -= self.rest_volume
                    * (2. * shear * (self.deformation[j][i] - r[j][i])
                        + bulk * (jdet - 1.) * cof[j][i]);
                a[row][row] += 2. * shear * self.rest_volume;
                for k in 0..3 {
                    for l in 0..3 {
                        a[row][3 + k * 3 + l] += bulk * self.rest_volume * cof[j][i] * cof[k][l];
                    }
                }
            }
        }
        (a, rhs)
    }
    #[cfg(test)]
    pub fn step(
        &mut self,
        a: Matrix,
        rhs: Vector,
        contact: &[[f32; 13]],
        native_positions: &[D],
        _separating: &[(D, f64)],
    ) -> bool {
        coupled_step(
            std::slice::from_mut(self),
            &[(a, rhs)],
            contact,
            native_positions,
            &[],
        )
    }
    pub fn finish(&mut self, prior: &Self, dt: f64) {
        let damping = (-2. * dt).exp();
        self.velocity = mul(sub(self.center, prior.center), damping / dt);
        self.deformation_velocity = std::array::from_fn(|j| {
            mul(sub(self.deformation[j], prior.deformation[j]), damping / dt)
        });
        self.update_vertices();
    }
}

/// A Newton metric restricted to the exact planted-point nullspace. Building
/// the reduced system directly avoids subtracting nearly equal inverses in
/// thin pieces at the cutting edge.
struct Block {
    q: Vector,
    inverse: Matrix,
}
fn vdot(a: Vector, b: Vector) -> f64 {
    (0..12).map(|i| a[i] * b[i]).sum()
}
fn matvec(a: Matrix, x: Vector) -> Vector {
    std::array::from_fn(|i| vdot(a[i], x))
}
fn point_move(q: Vector, r: D) -> D {
    std::array::from_fn(|i| q[i] + (0..3).map(|j| q[3 + j * 3 + i] * r[j]).sum::<f64>())
}
fn point_gradient(r: D, n: D) -> Vector {
    std::array::from_fn(|i| {
        if i < 3 {
            n[i]
        } else {
            r[(i - 3) / 3] * n[(i - 3) % 3]
        }
    })
}
impl Block {
    fn new(body: &Body, a: Matrix, rhs: Vector, stationary: &[D]) -> Option<Self> {
        if body.fully_fixed() {
            return Some(Self {
                q: [0.; 12],
                inverse: [[0.; 12]; 12],
            });
        }
        let mut rows = Vec::<Vector>::new();
        let mut target = Vec::<f64>::new();
        // A zero native displacement ball is an equality at the CURRENT
        // material point. Eliminate it exactly with the planted-point rows;
        // repeated finite projections cannot certify a zero-radius ball near
        // a float rounding boundary. These rows exist only for this solve.
        let constraints = body.anchors.iter()
            .map(|p| (*p, sub(*p, body.point(*p))))
            .chain(stationary.iter().map(|p| (*p, [0.; 3])));
        for (point, delta) in constraints {
            let jac = jacobian(sub(point, body.reference_center));
            for axis in 0..3 {
                let mut row = jac[axis];
                let mut t = delta[axis];
                for _ in 0..2 {
                    for (g, d) in rows.iter().zip(&target) {
                        let k = vdot(row, *g);
                        for i in 0..12 {
                            row[i] -= k * g[i];
                        }
                        t -= k * d;
                    }
                }
                let norm = vdot(row, row).sqrt();
                if norm > 1e-10 {
                    rows.push(row.map(|x| x / norm));
                    target.push(t / norm);
                }
            }
        }
        let particular =
            std::array::from_fn(|i| rows.iter().zip(&target).map(|(g, t)| g[i] * t).sum::<f64>());
        let mut basis = Vec::<Vector>::new();
        for axis in 0..12 {
            let mut n = [0.; 12];
            n[axis] = 1.;
            // Reorthogonalization keeps exact pins accurate at narrow kerfs.
            for _ in 0..2 {
                for g in rows.iter().chain(&basis) {
                    let k = vdot(n, *g);
                    for i in 0..12 {
                        n[i] -= k * g[i];
                    }
                }
            }
            let norm = vdot(n, n).sqrt();
            if norm > 1e-9 {
                basis.push(n.map(|x| x / norm));
            }
        }
        let dim = basis.len();
        if dim == 0 {
            return Some(Self {
                q: particular,
                inverse: [[0.; 12]; 12],
            });
        }
        let ab: Vec<_> = basis.iter().map(|n| matvec(a, *n)).collect();
        let fixed_force = matvec(a, particular);
        let residual = std::array::from_fn(|i| rhs[i] - fixed_force[i]);
        let mut reduced = [[0.; 12]; 12];
        let mut r = [0.; 12];
        for i in 0..12 {
            reduced[i][i] = 1.;
        }
        for i in 0..dim {
            r[i] = vdot(basis[i], residual);
            for j in 0..dim {
                reduced[i][j] = vdot(basis[i], ab[j]);
            }
        }
        let solution = linear(reduced, r)?;
        let q = std::array::from_fn(|i| {
            particular[i] + (0..dim).map(|j| basis[j][i] * solution[j]).sum::<f64>()
        });
        let mut inv = [[0.; 12]; 12];
        for col in 0..dim {
            let mut e = [0.; 12];
            e[col] = 1.;
            inv[col] = linear(reduced, e)?;
        }
        let inverse = std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                (0..dim)
                    .map(|k| {
                        (0..dim)
                            .map(|l| basis[k][i] * inv[l][k] * basis[l][j])
                            .sum::<f64>()
                    })
                    .sum()
            })
        });
        Some(Self { q, inverse })
    }
    fn direction(&self, g: Vector) -> Vector {
        matvec(self.inverse, g)
    }
}

/// Find a separating axis using the proposed positions. The interface can
/// rotate with the soil; a plane frozen at the start of a step incorrectly
/// prevents even two pieces moving together in the same shear.
fn candidate_axis(a: &Body, b: &Body, qa: Vector, qb: Vector, hint: D) -> (f64, D, usize, usize) {
    let support = |body: &Body, q: Vector, sign: f64| {
        body.poly
            .vertices
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let point = add(
                    v.position,
                    point_move(q, sub(v.material, body.reference_center)),
                );
                (i, sign * dot(hint, point))
            })
            .max_by(|x, y| x.1.total_cmp(&y.1))
            .unwrap()
    };
    let (ia, high) = support(a, qa, 1.);
    let (ib, negative_low) = support(b, qb, -1.);
    let mut best = (-negative_low - high, hint, ia, ib);
    if best.0 >= -1e-10 {
        return best;
    }
    let points = |body: &Body, q: Vector| {
        body.poly
            .vertices
            .iter()
            .map(|v| {
                add(
                    v.position,
                    point_move(q, sub(v.material, body.reference_center)),
                )
            })
            .collect::<Vec<_>>()
    };
    let pa = points(a, qa);
    let pb = points(b, qb);
    let gap = |n: D| {
        let (ia, high) = pa
            .iter()
            .enumerate()
            .map(|(i, p)| (i, dot(n, *p)))
            .max_by(|x, y| x.1.total_cmp(&y.1))
            .unwrap();
        let (ib, low) = pb
            .iter()
            .enumerate()
            .map(|(i, p)| (i, dot(n, *p)))
            .min_by(|x, y| x.1.total_cmp(&y.1))
            .unwrap();
        (low - high, n, ia, ib)
    };
    let consider = |best: &mut (f64, D, usize, usize), axis: D| {
        if length(axis) < 1e-12 {
            return;
        }
        let axis = unit(axis);
        for n in [axis, mul(axis, -1.)] {
            let candidate = gap(n);
            if candidate.0 > best.0 {
                *best = candidate;
            }
        }
    };
    for (body, p) in [(a, &pa), (b, &pb)] {
        for face in &body.poly.faces {
            consider(
                &mut best,
                cross(
                    sub(p[face.nodes[1]], p[face.nodes[0]]),
                    sub(p[face.nodes[2]], p[face.nodes[0]]),
                ),
            );
        }
    }
    // Face axes are conservative for skewed edge-edge configurations. The
    // independent triangle monitor still checks the accepted geometry.
    best
}

/// All initially touching material pieces share a single feasibility solve.
/// Every correction acts equally and oppositely in the two body metrics. This
/// allows an internal face to move without temporarily penetrating a neighbor.
/// OGC retains responsibility for contacts with the spade, foundation and
/// already separated material. Its original point bounds are certified below.
#[derive(Clone, Copy)]
pub(super) struct ContactSample {
    pub body: usize,
    pub material: D,
    pub native: D,
    pub bound: f64,
}

pub(super) fn coupled_step(
    bodies: &mut [Body],
    systems: &[(Matrix, Vector)],
    contact: &[[f32; 13]],
    native: &[D],
    planes: &[Contact],
) -> bool {
    coupled_step_with_samples(bodies, systems, contact, native, planes, &[])
}

pub(super) fn coupled_step_with_samples(
    bodies: &mut [Body],
    systems: &[(Matrix, Vector)],
    contact: &[[f32; 13]],
    native: &[D],
    planes: &[Contact],
    samples: &[ContactSample],
) -> bool {
    if samples
        .iter()
        .any(|s| s.body >= bodies.len() || !s.bound.is_finite() || s.bound < 0.)
    {
        return false;
    }
    let mut starts = Vec::new();
    let mut count = 0;
    for b in bodies.iter() {
        starts.push(count);
        count += b.poly.vertices.len();
    }
    if contact.len() != count || native.len() != count {
        return false;
    }
    let mut stationary = vec![Vec::new(); bodies.len()];
    for (i, body) in bodies.iter().enumerate() {
        for (j, vertex) in body.poly.vertices.iter().enumerate() {
            if contact[starts[i] + j][12] == 0. {
                stationary[i].push(vertex.material);
            }
        }
    }
    for sample in samples {
        if sample.bound == 0. {
            stationary[sample.body].push(sample.material);
        }
    }
    let mut blocks = Vec::with_capacity(bodies.len());
    for (i, (body, (a, r))) in bodies.iter().zip(systems).enumerate() {
        let Some(block) = Block::new(body, *a, *r, &stationary[i]) else {
            #[cfg(test)]
            if std::env::var_os("MIRROR_CUT_SOLVE_LOG").is_some() {
                static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 10 {
                    eprintln!(
                        "coupled singular body={} mass={} volume={} anchors={}",
                        body.id,
                        body.mass,
                        body.rest_volume,
                        body.anchors.len()
                    );
                }
            }
            return false;
        };
        blocks.push(block);
    }
    let immobile = blocks
        .iter()
        .map(|b| b.inverse.iter().flatten().all(|x| *x == 0.))
        .collect::<Vec<_>>();
    // Temporary contact equalities do not exempt newly constrained bodies
    // from the final geometric certificate.
    let fixed = bodies.iter().map(Body::fully_fixed).collect::<Vec<_>>();
    // Preserve feasibility when a previously accepted contact lies exactly on
    // the numerical tolerance. Permit only floating-point roundoff beyond the
    // previously accepted gap, bounded by the monitor's 20 nm limit.
    // This prevents endlessly rejecting zero-motion candidates due to rounding.
    let gap_floors = planes
        .iter()
        .map(|plane| {
            let gap = candidate_axis(
                &bodies[plane.a],
                &bodies[plane.b],
                [0.; 12],
                [0.; 12],
                plane.normal,
            )
            .0;
            (-1e-8f64).min(gap - 1e-14).max(-2e-8 - 1e-14)
        })
        .collect::<Vec<_>>();
    let point_ceilings = planes
        .iter()
        .map(|plane| {
            plane
                .points
                .iter()
                .map(|p| {
                    let overlap = dot(
                        plane.normal,
                        sub(bodies[plane.a].point(*p), bodies[plane.b].point(*p)),
                    );
                    (1e-8f64).max(overlap + 1e-14).min(2e-8 + 1e-14)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    #[cfg(test)]
    let log = std::env::var_os("MIRROR_CUT_SOLVE_LOG").is_some() && {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 75 && bodies.len() > 1
    };
    #[cfg(test)]
    if log {
        eprintln!(
            "coupled pre bodies={} qmax={} contacts_min={} zero_bounds={}",
            bodies.len(),
            blocks
                .iter()
                .flat_map(|b| b.q)
                .map(f64::abs)
                .fold(0f64, f64::max),
            contact.iter().map(|r| r[12]).fold(f32::INFINITY, f32::min),
            contact.iter().filter(|r| r[12] < 1e-9).count()
        );
    }
    let mut residual_error = 0.;
    for sweep in 0..96 {
        let mut error = 0f64;
        for (i, b) in bodies.iter().enumerate() {
            for (j, v) in b.poly.vertices.iter().enumerate() {
                // Planted vertices are already constrained exactly by the
                // reduced basis. Re-projecting their near-zero numerical
                // freedom can amplify rounding noise into a huge correction.
                if b.anchors.iter().any(|p| length(sub(*p, v.material)) < 1e-9) {
                    continue;
                }
                if contact[starts[i] + j][12] == 0. {
                    continue;
                }
                let r = sub(v.material, b.reference_center);
                // `native` is the f32 rounding of the current vertex. A zero
                // proposed motion is feasible even when that round trip is
                // not exactly equal to the original f64 position.
                let delta = point_move(blocks[i].q, r);
                let distance = length(delta);
                let radius = (contact[starts[i] + j][12] as f64 * 0.85).max(0.);
                if distance > radius {
                    let g = point_gradient(r, unit(delta));
                    let d = blocks[i].direction(g);
                    let denominator = vdot(g, d);
                    if denominator > 1e-24 {
                        let lambda = (distance - radius) / denominator;
                        for k in 0..12 {
                            blocks[i].q[k] -= lambda * d[k];
                        }
                    }
                    error = error.max(distance - radius);
                }
            }
        }
        for sample in samples {
            if sample.bound == 0. {
                continue;
            }
            let b = &bodies[sample.body];
            let r = sub(sample.material, b.reference_center);
            let delta = point_move(blocks[sample.body].q, r);
            let distance = length(delta);
            let radius = sample.bound * 0.85;
            if distance > radius {
                let g = point_gradient(r, unit(delta));
                let d = blocks[sample.body].direction(g);
                let denominator = vdot(g, d);
                if denominator > 1e-24 {
                    let lambda = (distance - radius) / denominator;
                    for k in 0..12 {
                        blocks[sample.body].q[k] -= lambda * d[k];
                    }
                }
                error = error.max(distance - radius);
            }
        }
        for plane in planes {
            let (a, b, n) = (plane.a, plane.b, plane.normal);
            if immobile[a] && immobile[b] {
                continue;
            }
            if !plane.points.is_empty() {
                for &p in &plane.points {
                    let ra = sub(p, bodies[a].reference_center);
                    let rb = sub(p, bodies[b].reference_center);
                    let pa = add(bodies[a].point(p), point_move(blocks[a].q, ra));
                    let pb = add(bodies[b].point(p), point_move(blocks[b].q, rb));
                    let violation = dot(n, sub(pa, pb));
                    if violation > 1e-10 {
                        let ga = point_gradient(ra, n);
                        let gb = point_gradient(rb, mul(n, -1.));
                        let wa = blocks[a].direction(ga);
                        let wb = blocks[b].direction(gb);
                        let denominator = vdot(ga, wa) + vdot(gb, wb);
                        if denominator > 1e-24 {
                            let lambda = violation / denominator;
                            for k in 0..12 {
                                blocks[a].q[k] -= lambda * wa[k];
                                blocks[b].q[k] -= lambda * wb[k];
                            }
                        }
                        error = error.max(violation);
                    }
                }
            }
            // Interface-point constraints are cheap; recompute convex support
            // periodically rather than rebuilding it for every correction.
            if sweep % 8 != 0 {
                continue;
            }
            let (gap, n, va, vb) =
                candidate_axis(&bodies[a], &bodies[b], blocks[a].q, blocks[b].q, n);
            let violation = -gap;
            if violation > 1e-10 {
                let ga = point_gradient(
                    sub(
                        bodies[a].poly.vertices[va].material,
                        bodies[a].reference_center,
                    ),
                    n,
                );
                let gb = point_gradient(
                    sub(
                        bodies[b].poly.vertices[vb].material,
                        bodies[b].reference_center,
                    ),
                    mul(n, -1.),
                );
                let wa = blocks[a].direction(ga);
                let wb = blocks[b].direction(gb);
                let denominator = vdot(ga, wa) + vdot(gb, wb);
                if denominator > 1e-24 {
                    let lambda = violation / denominator;
                    for k in 0..12 {
                        blocks[a].q[k] -= lambda * wa[k];
                        blocks[b].q[k] -= lambda * wb[k];
                    }
                }
                error = error.max(violation);
            }
        }
        residual_error = error;
        if error < 1e-9 && sweep % 8 == 0 {
            break;
        }
    }
    let original: Vec<_> = bodies.iter().map(|b| (b.center, b.deformation)).collect();
    #[cfg(test)]
    let capture_failure = std::env::var("MIRROR_CUT_SOLVE_FAILURE_CAPTURE").ok();
    #[cfg(test)]
    let initial_q = capture_failure.as_ref().map(|_| blocks.iter().map(|b| b.q).collect::<Vec<_>>());
    // The candidate stays within a convex feasible region around the native
    // snapshot. Backtracking also checks exact float conversion and det(F).
    for backtrack in 0..32 {
        for (i, b) in bodies.iter_mut().enumerate() {
            let q = blocks[i].q;
            b.center = add(original[i].0, [q[0], q[1], q[2]]);
            b.deformation = std::array::from_fn(|j| {
                std::array::from_fn(|k| original[i].1[j][k] + q[3 + j * 3 + k])
            });
            b.update_vertices();
        }
        let valid = bodies.iter().enumerate().all(|(i, b)| {
            determinant(b.deformation) > 0.15
                && determinant(b.deformation) < 3.
                && b.anchors
                    .iter()
                    .all(|p| length(sub(b.point(*p), *p)) < 1e-7)
                && b.poly.vertices.iter().enumerate().all(|(j, v)| {
                    length(sub(
                        v.position.map(|x| x as f32 as f64),
                        native[starts[i] + j],
                    )) <= contact[starts[i] + j][12] as f64 + 1e-10
                })
        }) && samples.iter().all(|sample| {
            let current = bodies[sample.body]
                .point(sample.material)
                .map(|x| x as f32 as f64);
            length(sub(current, sample.native)) <= sample.bound + 1e-10
        }) && planes.iter().zip(&gap_floors).zip(&point_ceilings).all(
            |((plane, floor), ceilings)| {
                let (a, b, n) = (plane.a, plane.b, plane.normal);
                if fixed[a] && fixed[b] {
                    return true;
                }
                plane.points.iter().zip(ceilings).all(|(p, ceiling)| {
                    dot(n, sub(bodies[a].point(*p), bodies[b].point(*p))) <= *ceiling
                }) && candidate_axis(&bodies[a], &bodies[b], [0.; 12], [0.; 12], n).0 >= *floor
            },
        );
        if valid {
            #[cfg(test)]
            if log {
                eprintln!(
                    "coupled accepted backtrack={backtrack} residual={residual_error} qmax={}",
                    blocks
                        .iter()
                        .flat_map(|b| b.q)
                        .map(f64::abs)
                        .fold(0f64, f64::max)
                );
            }
            return true;
        }
        for block in &mut blocks {
            block.q = block.q.map(|x| x * 0.5);
        }
    }
    #[cfg(test)]
    if let Some(path) = capture_failure {
        let mut original_bodies = bodies.to_vec();
        for (body, (center, deformation)) in original_bodies.iter_mut().zip(&original) {
            body.center = *center;
            body.deformation = *deformation;
            body.update_vertices();
        }
        let original_certificate = solve_certificate(&original_bodies, contact, native, planes, samples, &starts, &fixed, &gap_floors, &point_ceilings);
        let final_certificate = solve_certificate(bodies, contact, native, planes, samples, &starts, &fixed, &gap_floors, &point_ceilings);
        eprintln!("coupled certification original={} final={}", original_certificate, final_certificate);
        let path = format!("{path}.certification.json");
        if !std::path::Path::new(&path).exists() {
            let capture = serde_json::json!({
                "original":original_certificate,"final":final_certificate,"initial_q":initial_q,
                "final_applied_q":blocks.iter().map(|b| b.q.map(|x| x * 2.)).collect::<Vec<_>>(),
                "gap_floors":gap_floors,"point_ceilings":point_ceilings,"residual":residual_error
            });
            if let Ok(bytes) = serde_json::to_vec(&capture) { let _ = std::fs::write(path, bytes); }
        }
    }
    #[cfg(test)]
    if std::env::var_os("MIRROR_CUT_SOLVE_LOG").is_some() {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 10 {
            let anchors = bodies
                .iter()
                .flat_map(|b| b.anchors.iter().map(|p| length(sub(b.point(*p), *p))))
                .fold(0f64, f64::max);
            let mut bound = 0f64;
            for (i, b) in bodies.iter().enumerate() {
                for (j, v) in b.poly.vertices.iter().enumerate() {
                    bound = bound.max(
                        length(sub(
                            v.position.map(|x| x as f32 as f64),
                            native[starts[i] + j],
                        )) - contact[starts[i] + j][12] as f64,
                    );
                }
            }
            let gap = planes
                .iter()
                .map(|p| candidate_axis(&bodies[p.a], &bodies[p.b], [0.; 12], [0.; 12], p.normal).0)
                .fold(f64::INFINITY, f64::min);
            eprintln!(
                "coupled FAILED residual={residual_error} anchor={anchors} bound={bound} mingap={gap}"
            );
        }
    }
    for (b, (center, f)) in bodies.iter_mut().zip(original) {
        b.center = center;
        b.deformation = f;
        b.update_vertices();
    }
    false
}

/// Diagnose precisely the existing certificate; this never changes acceptance.
#[cfg(test)]
fn solve_certificate(
    bodies: &[Body], contact: &[[f32; 13]], native: &[D], planes: &[Contact],
    samples: &[ContactSample], starts: &[usize], fixed: &[bool], floors: &[f64],
    ceilings: &[Vec<f64>],
) -> serde_json::Value {
    let mut counts = [0usize; 6];
    let mut failures = Vec::new();
    let mut record = |kind: usize, valid: bool, detail: serde_json::Value| {
        if !valid {
            counts[kind] += 1;
            if failures.len() < 16 { failures.push(detail); }
        }
    };
    for (i, b) in bodies.iter().enumerate() {
        let determinant = determinant(b.deformation);
        record(0, determinant > 0.15 && determinant < 3., serde_json::json!({"kind":"determinant","body":i,"id":b.id,"value":determinant}));
        for p in &b.anchors {
            let distance = length(sub(b.point(*p), *p));
            record(1, distance < 1e-7, serde_json::json!({"kind":"anchor","body":i,"distance":distance}));
        }
        for (j, v) in b.poly.vertices.iter().enumerate() {
            let current = v.position.map(|x| x as f32 as f64);
            let distance = length(sub(current, native[starts[i] + j]));
            let bound = contact[starts[i] + j][12] as f64;
            record(2, distance <= bound + 1e-10, serde_json::json!({"kind":"vertex_bound","body":i,"vertex":j,"distance":distance,"bound":bound,"current":current,"native":native[starts[i] + j]}));
        }
    }
    for (i, sample) in samples.iter().enumerate() {
        let precise = bodies[sample.body].point(sample.material);
        let current = precise.map(|x| x as f32 as f64);
        let distance = length(sub(current, sample.native));
        record(3, distance <= sample.bound + 1e-10, serde_json::json!({"kind":"sample_bound","sample":i,"body":sample.body,"material":sample.material,"precise":precise,"distance":distance,"bound":sample.bound,"current":current,"native":sample.native}));
    }
    for (i, plane) in planes.iter().enumerate() {
        if fixed[plane.a] && fixed[plane.b] { continue; }
        for (j, p) in plane.points.iter().enumerate() {
            let overlap = dot(plane.normal, sub(bodies[plane.a].point(*p), bodies[plane.b].point(*p)));
            record(4, overlap <= ceilings[i][j], serde_json::json!({"kind":"point_ceiling","plane":i,"point":j,"a":plane.a,"b":plane.b,"overlap":overlap,"ceiling":ceilings[i][j]}));
        }
        let gap = candidate_axis(&bodies[plane.a], &bodies[plane.b], [0.; 12], [0.; 12], plane.normal).0;
        record(5, gap >= floors[i], serde_json::json!({"kind":"gap_floor","plane":i,"a":plane.a,"b":plane.b,"gap":gap,"floor":floors[i]}));
    }
    serde_json::json!({"valid":counts.iter().all(|n| *n == 0),"failure_counts":{"determinant":counts[0],"anchor":counts[1],"vertex_bound":counts[2],"sample_bound":counts[3],"point_ceiling":counts[4],"gap_floor":counts[5]},"failures":failures,"samples":samples.len(),"zero_bound_samples":samples.iter().filter(|s|s.bound==0.).count()})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn captured_kerf_zero_bounds_preserve_current_points_without_persistent_pins() {
        // Body 42 from the 2026-10-30 failed rooted-soil solve. The third
        // sample lies next to a float rounding boundary: the old approximate
        // projection still moved it by one float ULP after 32 backtracks.
        let reference = [
            [0.14922479487748486, 0., 0.000008158682737321836],
            [0.14922519956595376, 0., -0.0005918803150438619],
            [-0.15077480566653467, 0., -0.0007919096493881339],
            [-0.15077521035500357, 0., -0.00019187065160697347],
            [-0.15078839970998362, -0.019999999999999993, -0.0005635709570510627],
            [-0.15078880439845252, -0.019999999999999993, 0.00003646804073012544],
            [0.1492112008340359, -0.019999999999999997, 0.00023649737507439592],
            [0.14921160552250481, -0.019999999999999997, -0.000363541622706785],
        ];
        let poly = Poly {
            vertices: reference.into_iter().map(|p| Vertex { material:p, position:p, velocity:[0.;3] }).collect(),
            faces: [[0,1,2,3],[3,2,4,5],[1,0,6,7],[7,6,5,4],[6,0,3,5],[1,7,4,2]]
                .into_iter().map(|nodes| Face { nodes:nodes.to_vec(), tag:0 }).collect(),
        };
        let mut body = Body::new(42, poly, 1600., -10.);
        body.reference_center = [-0.0007818024162490389, -0.010000000000000031, -0.0002777061371568724];
        body.center = [-0.0007162972792161352, -0.012749604726000143, 0.0002829178777499008];
        body.deformation = [
            [1.0000720374244976, 0.00018856101147664316, -0.00010247439958475193],
            [0.0017035190647703828, 0.9755287757523037, 0.00436759088332234],
            [0.0012358077268533004, 0.08287357308290606, 0.9913774119082003],
        ];
        body.update_vertices();
        let native = body.poly.vertices.iter().map(|v|v.position.map(|x|x as f32 as f64)).collect::<Vec<_>>();
        let mut contact = vec![[0f32;13];native.len()];
        for r in &mut contact { r[12] = 0.1; }
        let samples = [
            [0.11436028134029666, -0.0016408861697727475, 0.0000036468987914769017],
            [-0.11555323360553699, -0.0015966247697269745, -0.00015015660594233646],
            [-0.11538956182811361, -0.001599850128502555, -0.00015001065024995009],
            [0.02729358773238416, -0.001086897568812478, -0.0006607704463995052],
        ].map(|material|ContactSample { body:0, material, native:body.point(material).map(|x|x as f32 as f64), bound:0. });
        assert_eq!(samples[2].native[1], -0.004566044546663761);
        let system = body.system(&body, 1./120.);
        let mut constrained = vec![body.clone()];
        assert!(coupled_step_with_samples(&mut constrained, &[system], &contact, &native, &[], &samples));
        assert_eq!(constrained[0].center, body.center);
        assert_eq!(constrained[0].deformation, body.deformation);
        for sample in samples {
            assert_eq!(constrained[0].point(sample.material).map(|x|x as f32 as f64), sample.native);
        }
        // Releasing the contact on the next query restores the same body's
        // freedoms; a zero bound must not create a lasting material anchor.
        assert!(coupled_step(&mut constrained, &[system], &contact, &native, &[]));
        assert!(length(sub(constrained[0].center, body.center)) > 1e-6);
        let mut physical_contact = contact;
        for i in [0,1,2,4] { physical_contact[i][12] = 0.; }
        let mut physical = vec![body.clone()];
        assert!(coupled_step(&mut physical, &[system], &physical_contact, &native, &[]));
        assert_eq!(physical[0].center, body.center);
        assert_eq!(physical[0].deformation, body.deformation);
    }

    #[test]
    #[ignore = "requires MIRROR_AFFINE_SOLVE_REPLAY pointing to a captured failed-solve.json"]
    fn replay_captured_affine_solve() {
        let path = std::env::var("MIRROR_AFFINE_SOLVE_REPLAY").unwrap();
        let data: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let mut bodies = data["material"]["bodies"].as_array().unwrap().iter().map(|v| {
            let reference: Vec<D> = serde_json::from_value(v["reference"].clone()).unwrap();
            let positions: Vec<D> = serde_json::from_value(v["positions"].clone()).unwrap();
            let velocity: Vec<D> = serde_json::from_value(v["vertex_velocities"].clone()).unwrap();
            let poly = Poly {
                vertices: reference.into_iter().zip(positions).zip(velocity).map(|((material,position),velocity)|Vertex{material,position,velocity}).collect(),
                faces: v["faces"].as_array().unwrap().iter().map(|f|Face{nodes:serde_json::from_value(f["nodes"].clone()).unwrap(),tag:f["tag"].as_u64().unwrap()}).collect(),
            };
            Body {
                id:v["id"].as_u64().unwrap(), ancestors:serde_json::from_value(v["ancestors"].clone()).unwrap(), poly,
                reference_center:serde_json::from_value(v["reference_center"].clone()).unwrap(), center:serde_json::from_value(v["center"].clone()).unwrap(),
                deformation:serde_json::from_value(v["deformation"].clone()).unwrap(), velocity:serde_json::from_value(v["velocity"].clone()).unwrap(),
                deformation_velocity:serde_json::from_value(v["deformation_velocity"].clone()).unwrap(), mass:v["mass"].as_f64().unwrap(), rest_volume:v["rest_volume"].as_f64().unwrap(),
                anchors:serde_json::from_value(v["anchors"].clone()).unwrap(), initial_center:serde_json::from_value(v["initial_center"].clone()).unwrap(),
            }
        }).collect::<Vec<_>>();
        let systems = data["systems"].as_array().unwrap().iter().map(|s|(serde_json::from_value(s["matrix"].clone()).unwrap(),serde_json::from_value(s["rhs"].clone()).unwrap())).collect::<Vec<(Matrix,Vector)>>();
        let responses: Vec<[f32;13]> = serde_json::from_value(data["responses"].clone()).unwrap();
        let native: Vec<D> = serde_json::from_value(data["native"].clone()).unwrap();
        let planes = data["planes"].as_array().unwrap().iter().map(|p|Contact{a:p["a"].as_u64().unwrap()as usize,b:p["b"].as_u64().unwrap()as usize,normal:serde_json::from_value(p["normal"].clone()).unwrap(),points:serde_json::from_value(p["points"].clone()).unwrap()}).collect::<Vec<_>>();
        let samples = data["samples"].as_array().unwrap().iter().map(|s|ContactSample{body:s["body"].as_u64().unwrap()as usize,material:serde_json::from_value(s["material"].clone()).unwrap(),native:serde_json::from_value(s["native"].clone()).unwrap(),bound:s["bound"].as_f64().unwrap()}).collect::<Vec<_>>();
        let count = bodies.iter().map(|b|b.poly.vertices.len()).sum::<usize>();
        assert!(coupled_step_with_samples(&mut bodies, &systems, &responses[..count], &native[..count], &planes, &samples));
    }

    #[test]
    fn refinement_preserves_mass_momentum_and_deformation() {
        let mut b = Body::new(
            1,
            Poly::box_from([-0.26, -0.24, -0.36], [0.26, 0., 0.36]),
            1600.,
            -0.24,
        );
        b.deformation = [[1., 0.1, 0.], [0., 0.95, 0.], [0., 0., 1.02]];
        b.velocity = [1., 2., 3.];
        b.deformation_velocity = [[0., 0.2, 0.], [-0.2, 0., 0.], [0.; 3]];
        b.update_vertices();
        let children = b
            .split(
                Plane::through([0.03, -0.12, 0.], [0.3, 0.9, 0.1]),
                100,
                [2, 3],
            )
            .unwrap();
        assert!((children.iter().map(|b| b.mass).sum::<f64>() - b.mass).abs() < 1e-10);
        let momentum = children
            .iter()
            .fold([0.; 3], |a, b| add(a, mul(b.velocity, b.mass)));
        assert!(length(sub(momentum, mul(b.velocity, b.mass))) < 1e-10);
        for child in children {
            for v in &child.poly.vertices {
                assert!(length(sub(v.position, b.point(v.material))) < 1e-10);
            }
        }
    }
    #[test]
    fn additional_contact_sample_constrains_its_material_point() {
        let body = Body::new(
            1,
            Poly::box_from([-0.2, -0.2, -0.2], [0.2, 0., 0.2]),
            1600.,
            -10.,
        );
        let point = body.reference_center;
        let (a, mut rhs) = body.system(&body, 1. / 120.);
        rhs[1] = 1000.;
        let native = body
            .poly
            .vertices
            .iter()
            .map(|v| v.position.map(|x| x as f32 as f64))
            .collect::<Vec<_>>();
        let mut contact = vec![[0f32; 13]; native.len()];
        for r in &mut contact {
            r[12] = 0.1;
        }
        let mut free = vec![body.clone()];
        assert!(coupled_step(&mut free, &[(a, rhs)], &contact, &native, &[]));
        assert!(length(sub(free[0].point(point), body.point(point))) > 1e-6);
        let mut constrained = vec![body.clone()];
        let sample = ContactSample {
            body: 0,
            material: point,
            native: body.point(point).map(|x| x as f32 as f64),
            bound: 0.,
        };
        assert!(coupled_step_with_samples(
            &mut constrained,
            &[(a, rhs)],
            &contact,
            &native,
            &[],
            &[sample]
        ));
        assert!(length(sub(constrained[0].point(point), body.point(point))) < 1e-10);
    }
    #[test]
    fn a_planted_piece_accepts_contact_bounded_deformation_without_moving_its_base() {
        let mut b = Body::new(
            1,
            Poly::box_from([-0.2, -0.2, -0.2], [0.2, 0., 0.2]),
            1600.,
            -0.2,
        );
        let prior = b.clone();
        let (mut a, mut rhs) = b.system(&prior, 1. / 120.);
        Body::add_force(
            &mut a,
            &mut rhs,
            sub([0., 0., 0.], b.reference_center),
            [100., -100., 0.],
            [[0.; 3]; 3],
        );
        let points: Vec<_> = b.poly.vertices.iter().map(|v| v.position).collect();
        let mut contacts = vec![[0f32; 13]; points.len()];
        for c in &mut contacts {
            c[12] = 0.01;
        }
        assert!(b.step(a, rhs, &contacts, &points, &[]));
        assert!(
            b.anchors
                .iter()
                .all(|p| length(sub(b.point(*p), *p)) < 1e-6)
        );
        assert!(
            b.poly
                .vertices
                .iter()
                .zip(points)
                .all(|(v, p)| length(sub(v.position, p)) <= 0.01000001)
        );
        assert!(
            b.poly
                .vertices
                .iter()
                .zip(&prior.poly.vertices)
                .any(|(a, b)| length(sub(a.position, b.position)) > 1e-6),
            "planted soil must still deform under load"
        );
    }
    #[test]
    fn zero_bound_on_rounded_planted_vertices_does_not_freeze_the_surface() {
        let mut b = Body::new(
            1,
            Poly::box_from([-0.2, -0.2, -0.2], [0.2, 0., 0.2]),
            1600.,
            -0.2,
        );
        let prior = b.clone();
        let (mut a, mut rhs) = b.system(&prior, 1. / 120.);
        Body::add_force(
            &mut a,
            &mut rhs,
            sub([0., 0., 0.], b.reference_center),
            [0., -100., 0.],
            [[0.; 3]; 3],
        );
        let native = b
            .poly
            .vertices
            .iter()
            .map(|v| v.position.map(|x| x as f32 as f64))
            .collect::<Vec<_>>();
        let mut contacts = vec![[0f32; 13]; native.len()];
        for (v, c) in b.poly.vertices.iter().zip(&mut contacts) {
            c[12] = if v.material[1] < -0.19 { 0. } else { 0.01 };
        }
        assert!(b.step(a, rhs, &contacts, &native, &[]));
        assert!(
            b.poly
                .vertices
                .iter()
                .zip(&prior.poly.vertices)
                .any(|(v, p)| v.material[1] > -0.01 && length(sub(v.position, p.position)) > 1e-6)
        );
        assert!(
            b.anchors
                .iter()
                .all(|p| length(sub(b.point(*p), *p)) < 1e-8)
        );
    }
    #[test]
    fn rounded_contact_at_tolerance_can_move_without_growing_overlap() {
        let mut bodies = vec![
            Body::new(
                1,
                Poly::box_from([-0.2, -0.2, -0.2], [0., 0., 0.2]),
                1600.,
                -10.,
            ),
            Body::new(
                2,
                Poly::box_from([0., -0.2, -0.2], [0.2, 0., 0.2]),
                1600.,
                -10.,
            ),
        ];
        bodies[1].center[0] -= 1.0000002e-8;
        bodies[1].update_vertices();
        let before = bodies.clone();
        let systems = bodies
            .iter()
            .map(|b| {
                let (a, _) = b.system(b, 1. / 120.);
                let mut rhs = [0.; 12];
                rhs[1] = 1000.;
                (a, rhs)
            })
            .collect::<Vec<_>>();
        let native = bodies
            .iter()
            .flat_map(|b| {
                b.poly
                    .vertices
                    .iter()
                    .map(|v| v.position.map(|x| x as f32 as f64))
            })
            .collect::<Vec<_>>();
        let mut bounds = vec![[0f32; 13]; native.len()];
        for bound in &mut bounds {
            bound[12] = 0.01;
        }
        let plane = Contact {
            a: 0,
            b: 1,
            normal: [1., 0., 0.],
            points: vec![
                [0., -0.2, -0.2],
                [0., -0.2, 0.2],
                [0., 0., -0.2],
                [0., 0., 0.2],
            ],
        };
        let old_gap = candidate_axis(&bodies[0], &bodies[1], [0.; 12], [0.; 12], plane.normal).0;
        assert!(coupled_step(
            &mut bodies,
            &systems,
            &bounds,
            &native,
            &[plane]
        ));
        let gap = candidate_axis(&bodies[0], &bodies[1], [0.; 12], [0.; 12], [1., 0., 0.]).0;
        assert!(
            gap >= old_gap - 1.1e-14,
            "contact overlap grew: {old_gap} -> {gap}"
        );
        assert!(bodies[0].center[1] - before[0].center[1] > 1e-6);
    }
    #[test]
    fn touching_pieces_share_contact_motion_without_crossing_or_freezing() {
        let dt = 1. / 120.;
        let mut bodies = vec![
            Body::new(
                1,
                Poly::box_from([-0.2, -0.2, -0.2], [0., 0., 0.2]),
                1600.,
                -10.,
            ),
            Body::new(
                2,
                Poly::box_from([0., -0.2, -0.2], [0.2, 0., 0.2]),
                1600.,
                -10.,
            ),
        ];
        let initial = bodies.clone();
        let mut systems = bodies
            .iter()
            .map(|b| {
                let (a, _) = b.system(b, dt);
                (a, [0.; 12])
            })
            .collect::<Vec<_>>();
        systems[0].1[0] = 1000.;
        let points = bodies
            .iter()
            .flat_map(|b| {
                b.poly
                    .vertices
                    .iter()
                    .map(|v| v.position.map(|x| x as f32 as f64))
            })
            .collect::<Vec<_>>();
        let mut contacts = vec![[0f32; 13]; points.len()];
        for r in &mut contacts {
            r[12] = 0.01;
        }
        let shared = vec![
            [0., -0.2, -0.2],
            [0., -0.2, 0.2],
            [0., 0., -0.2],
            [0., 0., 0.2],
        ];
        assert!(coupled_step(
            &mut bodies,
            &systems,
            &contacts,
            &points,
            &[Contact {
                a: 0,
                b: 1,
                normal: [1., 0., 0.],
                points: shared
            }]
        ));
        let gap = bodies[1]
            .poly
            .vertices
            .iter()
            .map(|v| v.position[0])
            .fold(f64::INFINITY, f64::min)
            - bodies[0]
                .poly
                .vertices
                .iter()
                .map(|v| v.position[0])
                .fold(f64::NEG_INFINITY, f64::max);
        assert!(gap >= -1e-8, "touching solids must not overlap: {gap}");
        assert!(
            bodies[1].center[0] - initial[1].center[0] > 1e-6,
            "the unforced neighbor must receive contact motion"
        );
        let mass = bodies.iter().map(|b| b.mass).sum::<f64>();
        let displacement = bodies
            .iter()
            .zip(&initial)
            .map(|(b, p)| b.mass * (b.center[0] - p.center[0]))
            .sum::<f64>()
            / mass;
        assert!(
            (displacement - 1000. * dt * dt / mass).abs() < 1e-9,
            "internal contact corrections must preserve net linear momentum"
        );
    }
    #[test]
    fn touching_material_can_shear_together_without_freezing_its_interface() {
        let mut bodies = vec![
            Body::new(
                1,
                Poly::box_from([-0.2, -0.2, -0.2], [0., 0., 0.2]),
                1600.,
                -1.,
            ),
            Body::new(
                2,
                Poly::box_from([0., -0.2, -0.2], [0.2, 0., 0.2]),
                1600.,
                -1.,
            ),
        ];
        let mut matrix = [[0.; 12]; 12];
        for i in 0..12 {
            matrix[i][i] = 1.;
        }
        let mut rhs = [0.; 12];
        rhs[6] = 0.05;
        let systems = vec![(matrix, rhs); 2];
        let points = bodies
            .iter()
            .flat_map(|b| {
                b.poly
                    .vertices
                    .iter()
                    .map(|v| v.position.map(|x| x as f32 as f64))
            })
            .collect::<Vec<_>>();
        let mut contacts = vec![[0f32; 13]; points.len()];
        for r in &mut contacts {
            r[12] = 1.;
        }
        let shared = vec![
            [0., -0.2, -0.2],
            [0., -0.2, 0.2],
            [0., 0., -0.2],
            [0., 0., 0.2],
        ];
        assert!(coupled_step(
            &mut bodies,
            &systems,
            &contacts,
            &points,
            &[Contact {
                a: 0,
                b: 1,
                normal: [1., 0., 0.],
                points: shared.clone()
            }]
        ));
        assert!(
            bodies.iter().all(|b| b.deformation[1][0] > 0.049),
            "a shared shear must not be pinned to the old interface plane"
        );
        assert!(
            shared
                .iter()
                .all(|p| length(sub(bodies[0].point(*p), bodies[1].point(*p))) < 1e-9)
        );
    }
    #[test]
    fn internal_root_forces_do_not_add_translation_drag() {
        let mut a = [[0.; 12]; 12];
        let mut rhs = [0.; 12];
        Body::add_internal_force(
            &mut a,
            &mut rhs,
            [0.02, 0.18, 0.01],
            [0., -40., 0.],
            [[0., 0., 0.], [0., 20000., 0.], [0., 0., 0.]],
        );
        for i in 0..3 {
            assert_eq!(rhs[i], 0.);
            assert!(a[i].iter().all(|x| *x == 0.));
            assert!(a.iter().all(|row| row[i] == 0.));
        }
        assert!(rhs[7].abs() > 0.);
    }
}
