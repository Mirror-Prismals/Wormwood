//! Original convex material clipping. A split partitions existing matter; it
//! does not subtract a shovel-shaped volume or create replacement material.
//! Material coordinates and velocity are interpolated at every new vertex.
use std::collections::{BTreeMap, BTreeSet};
pub(super) type D = [f64; 3];
pub(super) fn add(a: D, b: D) -> D {
    std::array::from_fn(|i| a[i] + b[i])
}
pub(super) fn sub(a: D, b: D) -> D {
    std::array::from_fn(|i| a[i] - b[i])
}
pub(super) fn mul(a: D, s: f64) -> D {
    a.map(|x| x * s)
}
pub(super) fn dot(a: D, b: D) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
pub(super) fn cross(a: D, b: D) -> D {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(super) fn length(a: D) -> f64 {
    dot(a, a).sqrt()
}
pub(super) fn unit(a: D) -> D {
    mul(a, 1. / length(a).max(1e-30))
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Vertex {
    pub position: D,
    pub material: D,
    pub velocity: D,
}
impl Vertex {
    fn between(a: Self, b: Self, t: f64) -> Self {
        let lerp = |a, b| add(a, mul(sub(b, a), t));
        Self {
            position: lerp(a.position, b.position),
            material: lerp(a.material, b.material),
            velocity: lerp(a.velocity, b.velocity),
        }
    }
}
#[derive(Clone, Debug)]
pub(super) struct Face {
    pub nodes: Vec<usize>,
    pub tag: u64,
}
#[derive(Clone, Debug)]
pub(super) struct Poly {
    pub vertices: Vec<Vertex>,
    pub faces: Vec<Face>,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Plane {
    pub normal: D,
    pub offset: f64,
}
impl Plane {
    pub fn through(point: D, normal: D) -> Self {
        let normal = unit(normal);
        Self {
            normal,
            offset: dot(point, normal),
        }
    }
    pub fn distance(self, p: D) -> f64 {
        dot(self.normal, p) - self.offset
    }
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Measure {
    pub volume: f64,
    pub center: D,
    pub inertia: [[f64; 3]; 3],
}
impl Poly {
    pub fn box_from(min: D, max: D) -> Self {
        let mut vertices = Vec::new();
        for y in [min[1], max[1]] {
            for z in [min[2], max[2]] {
                for x in [min[0], max[0]] {
                    let p = [x, y, z];
                    vertices.push(Vertex {
                        position: p,
                        material: p,
                        velocity: [0.; 3],
                    });
                }
            }
        }
        let faces = vec![
            vec![0, 1, 3, 2],
            vec![4, 6, 7, 5],
            vec![0, 4, 5, 1],
            vec![2, 3, 7, 6],
            vec![0, 2, 6, 4],
            vec![1, 5, 7, 3],
        ];
        Self {
            vertices,
            faces: faces
                .into_iter()
                .enumerate()
                .map(|(i, nodes)| Face {
                    nodes,
                    tag: i as u64 + 1,
                })
                .collect(),
        }
    }
    pub fn triangles(&self) -> impl Iterator<Item = [usize; 3]> + '_ {
        self.faces.iter().flat_map(|f| {
            (1..f.nodes.len() - 1).map(move |i| [f.nodes[0], f.nodes[i], f.nodes[i + 1]])
        })
    }
    pub fn center_vertices(&self, material: bool) -> D {
        mul(
            self.vertices.iter().fold([0.; 3], |a, v| {
                add(a, if material { v.material } else { v.position })
            }),
            1. / self.vertices.len() as f64,
        )
    }
    /// Exact signed tetrahedral integrals for a consistently oriented closed
    /// polygonal surface. Unit-density inertia is about the material centroid.
    pub fn measure(&self, material: bool) -> Measure {
        let origin = self.center_vertices(material);
        let mut volume = 0.;
        let mut first = [0.; 3];
        let mut second = [[0.; 3]; 3];
        for f in self.triangles() {
            let [a, b, c] = f.map(|i| {
                sub(
                    if material {
                        self.vertices[i].material
                    } else {
                        self.vertices[i].position
                    },
                    origin,
                )
            });
            let dv = dot(a, cross(b, c)) / 6.;
            volume += dv;
            let total = add(add(a, b), c);
            first = add(first, mul(total, dv / 4.));
            for i in 0..3 {
                for j in 0..3 {
                    second[i][j] +=
                        dv * (total[i] * total[j] + a[i] * a[j] + b[i] * b[j] + c[i] * c[j]) / 20.;
                }
            }
        }
        if volume <= 1e-20 {
            return Measure {
                volume,
                center: origin,
                inertia: [[0.; 3]; 3],
            };
        }
        let local = mul(first, 1. / volume);
        let trace = (0..3).map(|i| second[i][i]).sum::<f64>();
        let square = dot(local, local);
        let inertia = std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                (if i == j { trace } else { 0. })
                    - second[i][j]
                    - volume * ((if i == j { square } else { 0. }) - local[i] * local[j])
            })
        });
        Measure {
            volume,
            center: add(origin, local),
            inertia,
        }
    }
    pub fn valid(&self) -> bool {
        if self.vertices.len() < 4
            || self.faces.len() < 4
            || self.vertices.iter().any(|v| {
                v.position
                    .iter()
                    .chain(&v.material)
                    .chain(&v.velocity)
                    .any(|x| !x.is_finite())
            })
        {
            return false;
        }
        let mut edges: BTreeMap<(usize, usize), (usize, i32)> = BTreeMap::new();
        for f in &self.faces {
            if f.nodes.len() < 3 || f.nodes.iter().any(|i| *i >= self.vertices.len()) {
                return false;
            }
            for i in 0..f.nodes.len() {
                let (a, b) = (f.nodes[i], f.nodes[(i + 1) % f.nodes.len()]);
                if a == b {
                    return false;
                }
                let e = edges.entry((a.min(b), a.max(b))).or_default();
                e.0 += 1;
                e.1 += if a < b { 1 } else { -1 };
            }
        }
        edges.values().all(|e| *e == (2, 0))
            && self.measure(false).volume > 1e-14
            && self.measure(true).volume > 1e-14
    }
    pub fn convex(&self, tolerance: f64) -> bool {
        self.faces.iter().all(|f| {
            let a = self.vertices[f.nodes[0]].position;
            let n = self.face_normal(f, false);
            self.vertices
                .iter()
                .all(|v| dot(n, sub(v.position, a)) <= tolerance)
        })
    }
    pub fn face_normal(&self, face: &Face, material: bool) -> D {
        let point = |i: usize| {
            if material {
                self.vertices[i].material
            } else {
                self.vertices[i].position
            }
        };
        let a = point(face.nodes[0]);
        unit((1..face.nodes.len() - 1).fold([0.; 3], |sum, k| {
            add(
                sum,
                cross(
                    sub(point(face.nodes[k]), a),
                    sub(point(face.nodes[k + 1]), a),
                ),
            )
        }))
    }
    /// A vertex/edge/face-aligned plane is a no-op, not a perturbed cut.
    pub fn split(&self, plane: Plane, tag: u64) -> Option<[Self; 2]> {
        let distances: Vec<_> = self
            .vertices
            .iter()
            .map(|v| plane.distance(v.position))
            .collect();
        if !distances.iter().any(|d| *d < -1e-9) || !distances.iter().any(|d| *d > 1e-9) {
            return None;
        }
        let a = self.half(plane, tag, true, &distances)?;
        let b = self.half(plane, tag, false, &distances)?;
        let original = self.measure(true).volume;
        let sum = a.measure(true).volume + b.measure(true).volume;
        if !a.valid() || !b.valid() || (sum - original).abs() > original * 1e-8 {
            return None;
        }
        Some([a, b])
    }
    /// Retain one convex half-space during material tessellation. A discarded
    /// corner can have vanishing volume; it need not itself be a valid body.
    /// Callers tessellating matter must still check total partition volume.
    pub fn clip_negative(&self, plane: Plane, tag: u64) -> Option<Self> {
        let distances: Vec<_> = self
            .vertices
            .iter()
            .map(|v| plane.distance(v.position))
            .collect();
        let half = self.half(plane, tag, true, &distances)?;
        let before = self.measure(true).volume;
        (half.valid() && half.convex(1e-8) && half.measure(true).volume <= before * (1. + 1e-8))
            .then_some(half)
    }
    fn half(&self, plane: Plane, tag: u64, negative: bool, distances: &[f64]) -> Option<Self> {
        let mut out = Self {
            vertices: Vec::new(),
            faces: Vec::new(),
        };
        let mut originals = BTreeMap::new();
        let mut intersections = BTreeMap::new();
        let mut cap = BTreeSet::new();
        let inside = |d: f64| if negative { d <= 1e-10 } else { d >= -1e-10 };
        for face in &self.faces {
            let mut nodes = Vec::new();
            for j in 0..face.nodes.len() {
                let (a, b) = (face.nodes[j], face.nodes[(j + 1) % face.nodes.len()]);
                if inside(distances[a]) {
                    let id = *originals.entry(a).or_insert_with(|| {
                        let i = out.vertices.len();
                        out.vertices.push(self.vertices[a]);
                        i
                    });
                    nodes.push(id);
                    if distances[a].abs() <= 1e-10 {
                        cap.insert(id);
                    }
                }
                if (distances[a] < -1e-10 && distances[b] > 1e-10)
                    || (distances[a] > 1e-10 && distances[b] < -1e-10)
                {
                    let key = (a.min(b), a.max(b));
                    let id = *intersections.entry(key).or_insert_with(|| {
                        let t = distances[a] / (distances[a] - distances[b]);
                        let i = out.vertices.len();
                        out.vertices
                            .push(Vertex::between(self.vertices[a], self.vertices[b], t));
                        i
                    });
                    nodes.push(id);
                    cap.insert(id);
                }
            }
            nodes.dedup();
            if nodes.len() > 1 && nodes[0] == *nodes.last().unwrap() {
                nodes.pop();
            }
            if nodes.len() >= 3 {
                out.faces.push(Face {
                    nodes,
                    tag: face.tag,
                });
            }
        }
        if cap.len() < 3 {
            return None;
        }
        let center = mul(
            cap.iter()
                .fold([0.; 3], |a, i| add(a, out.vertices[*i].position)),
            1. / cap.len() as f64,
        );
        let normal = mul(plane.normal, if negative { 1. } else { -1. });
        let axis = unit(cross(
            normal,
            if normal[0].abs() < 0.8 {
                [1., 0., 0.]
            } else {
                [0., 1., 0.]
            },
        ));
        let other = cross(normal, axis);
        let mut nodes: Vec<_> = cap.into_iter().collect();
        nodes.sort_by(|a, b| {
            let angle = |i: &usize| {
                let p = sub(out.vertices[*i].position, center);
                dot(p, other).atan2(dot(p, axis))
            };
            angle(a).total_cmp(&angle(b))
        });
        out.faces.push(Face { nodes, tag });
        Some(out)
    }
    /// Nonnegative, affine-complete coordinates from a centroid-to-boundary
    /// tetrahedral fan. The centroid is virtual, so it creates no interior OGC vertex.
    pub fn coordinates(&self, point: D, material: bool) -> Option<Vec<f64>> {
        let center = self.center_vertices(material);
        for face in self.triangles() {
            let [a, b, c] = face.map(|i| {
                sub(
                    if material {
                        self.vertices[i].material
                    } else {
                        self.vertices[i].position
                    },
                    center,
                )
            });
            let p = sub(point, center);
            let det = dot(a, cross(b, c));
            if det.abs() < 1e-15 {
                continue;
            }
            let w = [
                dot(p, cross(b, c)) / det,
                dot(a, cross(p, c)) / det,
                dot(a, cross(b, p)) / det,
            ];
            let q = 1. - w.iter().sum::<f64>();
            if q >= -1e-7 && w.iter().all(|v| *v >= -1e-7) {
                let mut result = vec![q / self.vertices.len() as f64; self.vertices.len()];
                for i in 0..3 {
                    result[face[i]] += w[i];
                }
                return Some(result);
            }
        }
        None
    }
    pub fn sample(&self, weights: &[f64], material: bool) -> D {
        self.vertices
            .iter()
            .zip(weights)
            .fold([0.; 3], |a, (v, w)| {
                add(a, mul(if material { v.material } else { v.position }, *w))
            })
    }
    /// Positive lumped masses obtained by integrating the centroid fan, with
    /// its centroid share distributed equally to boundary vertices.
    pub fn mass_weights(&self) -> Vec<f64> {
        let c = self.center_vertices(true);
        let mut masses = vec![0.; self.vertices.len()];
        for face in self.triangles() {
            let [a, b, z] = face.map(|i| sub(self.vertices[i].material, c));
            let quarter = dot(a, cross(b, z)).abs() / 24.;
            for i in face {
                masses[i] += quarter;
            }
            let center_share = quarter / self.vertices.len() as f64;
            for m in &mut masses {
                *m += center_share;
            }
        }
        masses
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn box_poly() -> Poly {
        Poly::box_from([-0.26, -0.24, -0.36], [0.26, 0., 0.36])
    }
    #[test]
    fn oblique_cuts_partition_mass_first_moment_and_momentum() {
        let mut p = box_poly();
        for v in &mut p.vertices {
            v.velocity = add([1., 2., 3.], cross([0.1, 0.2, 0.3], v.position));
        }
        let before = p.measure(true);
        let children = p
            .split(Plane::through([0.023, -0.11, 0.03], [0.3, 0.8, -0.4]), 100)
            .unwrap();
        let m = children.each_ref().map(|p| p.measure(true));
        assert!((m[0].volume + m[1].volume - before.volume).abs() < 1e-12);
        let moment = add(mul(m[0].center, m[0].volume), mul(m[1].center, m[1].volume));
        assert!(length(sub(moment, mul(before.center, before.volume))) < 1e-12);
        for p in &children {
            assert!(p.valid() && p.convex(1e-9));
            for v in &p.vertices {
                assert!(
                    length(sub(
                        v.velocity,
                        add([1., 2., 3.], cross([0.1, 0.2, 0.3], v.position))
                    )) < 1e-12
                );
            }
            assert!((p.mass_weights().iter().sum::<f64>() - p.measure(true).volume).abs() < 1e-12);
        }
    }
    #[test]
    fn repeated_cuts_preserve_all_original_material() {
        let original = box_poly();
        let volume = original.measure(true).volume;
        let mut pieces = vec![original];
        for i in 0..18 {
            let n = unit([
                (i as f64 * 1.7).sin(),
                0.3 + (i as f64 * 2.1).cos(),
                (i as f64 * 0.9).cos(),
            ]);
            let plane = Plane::through([0., -0.12, 0.], n);
            let mut next = Vec::new();
            for p in pieces {
                if let Some([a, b]) = p.split(plane, 100 + i) {
                    next.extend([a, b]);
                } else {
                    next.push(p);
                }
            }
            pieces = next;
            let sum = pieces.iter().map(|p| p.measure(true).volume).sum::<f64>();
            assert!(
                (sum - volume).abs() < 1e-10,
                "cut {i} lost or duplicated matter"
            );
            assert!(pieces.iter().all(Poly::valid));
        }
        assert!(pieces.len() > 20);
    }
    #[test]
    fn aligned_and_near_vertex_cuts_are_finite_without_geometry_perturbation() {
        let p = box_poly();
        assert!(p
            .split(Plane::through([0., 0., 0.], [0., 1., 0.]), 100)
            .is_none());
        for offset in [0., 1e-12, 1e-8, 1e-5] {
            let plane = Plane::through([-0.26 + offset, -0.24, -0.36], [1., 1., 1.]);
            if let Some(parts) = p.split(plane, 100) {
                assert!(parts.iter().all(Poly::valid));
            }
        }
    }
    #[test]
    fn embedded_points_survive_cutting_and_mass_has_correct_box_inertia() {
        let p = box_poly();
        let m = p.measure(true);
        let dims = [0.52, 0.24, 0.72];
        for i in 0..3 {
            assert!(
                (m.inertia[i][i]
                    - m.volume
                        * (dims[(i + 1) % 3] * dims[(i + 1) % 3]
                            + dims[(i + 2) % 3] * dims[(i + 2) % 3])
                        / 12.)
                    .abs()
                    < 1e-12
            );
        }
        let parts = p
            .split(Plane::through([0., -0.1, 0.], [0., 1., 0.]), 100)
            .unwrap();
        for point in [[0., -0.03, 0.], [0.2, -0.2, 0.1]] {
            let weights = parts
                .iter()
                .find_map(|p| p.coordinates(point, true).map(|w| (p, w)))
                .unwrap();
            assert!(length(sub(weights.0.sample(&weights.1, false), point)) < 1e-12);
        }
    }
}
