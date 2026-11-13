//! Established sod, owned by PhysicsSystem. A deformable, volume-bearing root
//! mat with separate soil cohesion and tensile root bundles. No dirt particles,
//! metaball skin, timed fractures, or tool attachments are used by this case.
//! The reduced tissue parameters are workshop presets, not measured botany.
use super::*;

const NX: usize = 8;
const NZ: usize = 6;
const STEP: f32 = 0.08;
const THICKNESS: f32 = 0.085;
const DENSITY: f32 = 1050.;
const TETS: [[usize; 4]; 3] = [[0, 1, 2, 3], [1, 2, 3, 4], [2, 3, 4, 5]];
const CROWNS: [V; 4] = [
    [0.60, 0.20, 0.20],
    [0.20, 0.60, 0.20],
    [0.20, 0.20, 0.60],
    [0.34, 0.33, 0.33],
];

#[derive(Clone)]
struct Element {
    nodes: [usize; 6], // top triangle, corresponding bottom triangle
    seams: [Option<usize>; 3],
    edges: Vec<(usize, usize, f32, f32)>, // nodes, rest length, XPBD multiplier
    volumes: [f32; 3],
    volume_lambda: [f32; 3],
    anchored: bool,
}

#[derive(Clone, Copy)]
struct Anchor {
    element: usize,
    bary: V,
    depth: f32,
}
impl Anchor {
    fn weights(self, elements: &[Element]) -> [(usize, f32); 6] {
        std::array::from_fn(|k| {
            (
                elements[self.element].nodes[k],
                self.bary[k % 3] * if k < 3 { 1. - self.depth } else { self.depth },
            )
        })
    }
    fn position(self, p: &[V], elements: &[Element]) -> V {
        self.weights(elements)
            .iter()
            .fold([0.; 3], |sum, &(i, w)| add(sum, scale(p[i], w)))
    }
}

#[derive(Clone)]
struct Root {
    ends: [Anchor; 2],
    rest: f32,
    stiffness: f32,
    strength: f32,
    work: f32,
    lambda: f32,
    peak: f32,
    broken: bool,
    // Each torn end belongs to its own tissue, never follows the other piece.
    tips: [Anchor; 2],
}

#[derive(Clone)]
struct Seam {
    elements: [usize; 2],
    pairs: [[usize; 2]; 4],
    roots: [usize; 4],
    normal: V,
    area: f32,
    lambda: [V; 4],
    work: f32,
    peak: f32,
    broken: bool, // soil matrix; roots can still bridge the open crack
    contact_ready: bool,
}

#[derive(Clone)]
pub(super) struct Turf {
    elements: Vec<Element>,
    seams: Vec<Seam>,
    roots: Vec<Root>,
    pub(super) p: Vec<V>,
    rest: Vec<V>,
    velocity: Vec<V>,
    mass: Vec<f32>,
    inv_mass: Vec<f32>,
    pub(super) dirty: bool,
    tool_work: f32,
    cut_roots: usize,
    torn_roots: usize,
    touching: Vec<[usize; 2]>,
    self_contact: bool,
}

fn volume(p: [V; 4]) -> f32 {
    dot(sub(p[1], p[0]), cross(sub(p[2], p[0]), sub(p[3], p[0]))) / 6.
}
fn face_normal(p: [V; 3]) -> V {
    unit(cross(sub(p[1], p[0]), sub(p[2], p[0])))
}

impl Turf {
    pub fn new() -> Self {
        let mut t = Self {
            elements: vec![],
            seams: vec![],
            roots: vec![],
            p: vec![],
            rest: vec![],
            velocity: vec![],
            mass: vec![],
            inv_mass: vec![],
            dirty: true,
            tool_work: 0.,
            cut_roots: 0,
            torn_roots: 0,
            touching: vec![],
            self_contact: false,
        };
        let mut grid = Vec::new();
        for z in 0..=NZ {
            for x in 0..=NX {
                let salt = (x + z * 137) as i32;
                let edge = x == 0 || x == NX || z == 0 || z == NZ;
                grid.push([
                    (x as f32 - NX as f32 / 2.) * STEP
                        + if edge { 0. } else { 0.012 * noise(salt) },
                    0.004 + THICKNESS + 0.003 * noise(salt + 101),
                    (z as f32 - NZ as f32 / 2.) * STEP
                        + if edge { 0. } else { 0.012 * noise(salt + 37) },
                ]);
            }
        }
        let mut edges = BTreeMap::<[usize; 2], (usize, usize, [usize; 2])>::new();
        for z in 0..NZ {
            for x in 0..NX {
                let a = z * (NX + 1) + x;
                let b = a + 1;
                let c = a + NX + 1;
                let d = c + 1;
                let triangles = if (x + z) % 2 == 0 {
                    [[a, c, d], [a, d, b]]
                } else {
                    [[a, c, b], [b, c, d]]
                };
                for tri in triangles {
                    let id = t.elements.len();
                    let first = t.p.len();
                    let nodes = std::array::from_fn(|k| first + k);
                    let top = tri.map(|i| grid[i]);
                    t.p.extend(top);
                    t.p.extend(top.map(|p| [p[0], 0.004, p[2]]));
                    let volumes = TETS.map(|tet| volume(tet.map(|k| t.p[nodes[k]])));
                    let total = volumes.iter().map(|v| v.abs()).sum::<f32>();
                    // Only boundary tissue is held by the surrounding lawn.
                    // Interior lower vertices can leave the table with the sod.
                    let anchored = tri.iter().any(|&i| {
                        let (gx, gz) = (i % (NX + 1), i / (NX + 1));
                        gx == 0 || gx == NX || gz == 0 || gz == NZ
                    });
                    t.mass.extend([total * DENSITY / 6.; 6]);
                    t.inv_mass
                        .extend([if anchored { 0. } else { 6. / (total * DENSITY) }; 6]);
                    let mut internal = Vec::new();
                    for a in 0..6 {
                        for b in a + 1..6 {
                            internal.push((
                                nodes[a],
                                nodes[b],
                                length(sub(t.p[nodes[a]], t.p[nodes[b]])),
                                0.,
                            ));
                        }
                    }
                    t.elements.push(Element {
                        nodes,
                        seams: [None; 3],
                        edges: internal,
                        volumes,
                        volume_lambda: [0.; 3],
                        anchored,
                    });
                    for side in 0..3 {
                        let next = (side + 1) % 3;
                        let mut key = [tri[side], tri[next]];
                        key.sort_unstable();
                        if let Some((other, other_side, ends)) = edges.remove(&key) {
                            let matches = if ends[0] == tri[side] {
                                [other_side, (other_side + 1) % 3]
                            } else {
                                [(other_side + 1) % 3, other_side]
                            };
                            let on = t.elements[other].nodes;
                            let pairs = [
                                [on[matches[0]], nodes[side]],
                                [on[matches[1]], nodes[next]],
                                [on[matches[1] + 3], nodes[next + 3]],
                                [on[matches[0] + 3], nodes[side + 3]],
                            ];
                            let center = |element: usize| {
                                scale(
                                    t.elements[element].nodes[..3]
                                        .iter()
                                        .fold([0.; 3], |s, &i| add(s, t.p[i])),
                                    1. / 3.,
                                )
                            };
                            let normal = unit(sub(center(id), center(other)));
                            let area = length(sub(top[side], top[next])) * THICKNESS;
                            let mut roots = [0; 4];
                            for (r, slot) in roots.iter_mut().enumerate() {
                                let u = 0.15 + r as f32 * 0.23;
                                let mut left = [0.10; 3];
                                let mut right = [0.10; 3];
                                left[matches[0]] = 0.9 * (1. - u);
                                left[matches[1]] = 0.9 * u;
                                right[side] = 0.9 * (1. - u);
                                right[next] = 0.9 * u;
                                let depth = [0.12, 0.32, 0.53, 0.76][r];
                                let ends = [
                                    Anchor {
                                        element: other,
                                        bary: left,
                                        depth,
                                    },
                                    Anchor {
                                        element: id,
                                        bary: right,
                                        depth,
                                    },
                                ];
                                let rest = length(sub(
                                    ends[0].position(&t.p, &t.elements),
                                    ends[1].position(&t.p, &t.elements),
                                ));
                                *slot = t.roots.len();
                                t.roots.push(Root {
                                    ends,
                                    rest,
                                    stiffness: 1800.,
                                    strength: 2.4 + 0.7 * noise(*slot as i32),
                                    work: 0.,
                                    lambda: 0.,
                                    peak: 0.,
                                    broken: false,
                                    tips: ends,
                                });
                            }
                            let seam = t.seams.len();
                            t.seams.push(Seam {
                                elements: [other, id],
                                pairs,
                                roots,
                                normal,
                                area,
                                lambda: [[0.; 3]; 4],
                                work: 0.,
                                peak: 0.,
                                broken: false,
                                contact_ready: false,
                            });
                            t.elements[other].seams[other_side] = Some(seam);
                            t.elements[id].seams[side] = Some(seam);
                        } else {
                            edges.insert(key, (id, side, [tri[side], tri[next]]));
                        }
                    }
                }
            }
        }
        t.rest = t.p.clone();
        t.velocity = vec![[0.; 3]; t.p.len()];
        // Elements meeting at only a vertex also start at zero clearance.
        // They need the same opening exclusion as a shared side face.
        for a in 0..t.elements.len() {
            for b in a + 1..t.elements.len() {
                if t.elements[a].nodes.iter().any(|&i| {
                    t.elements[b]
                        .nodes
                        .iter()
                        .any(|&j| length(sub(t.p[i], t.p[j])) < 0.003)
                }) {
                    t.touching.push([a, b]);
                }
            }
        }
        t
    }

    fn faces(&self, i: usize) -> Vec<[usize; 3]> {
        let e = &self.elements[i];
        let n = e.nodes;
        let mut faces = vec![[n[0], n[1], n[2]], [n[3], n[5], n[4]]];
        for side in 0..3 {
            if e.seams[side].is_none_or(|s| self.seams[s].broken) {
                let next = (side + 1) % 3;
                faces.extend([
                    [n[side], n[side + 3], n[next + 3]],
                    [n[side], n[next + 3], n[next]],
                ]);
            }
        }
        faces
    }
    fn positions(&self, tool: &Tool) -> Vec<V> {
        let mut p = self.p.clone();
        p.extend(&tool.p);
        p.extend(&tool.ground);
        p
    }
    pub(super) fn contact_surface(&self, start: usize) -> Value {
        let n = self.p.len();
        let faces: Vec<_> = (0..self.elements.len())
            .flat_map(|i| self.faces(i))
            .map(|f| f.map(|k| k as u32))
            .collect();
        // Adjacent tissue starts with coincident faces. Its unilateral support
        // is retained until there is room for OGC to take over that pair.
        let excluded: Vec<_> = self.touching.iter().map(|p| p.map(|i| i as u32)).collect();
        let elements: Vec<_> = (0..self.elements.len())
            .flat_map(|i| [i as u32; 6])
            .collect();
        json!({"start":start,"count":n,"triangles":faces,"motion":0,"self_collision":self.self_contact,"friction":0.72,"element_ids":elements,"excluded_pairs":excluded,"movable_vertices":self.inv_mass.iter().map(|w|*w>0.).collect::<Vec<_>>()})
    }
    pub fn configure(&mut self, host: &mut Host, tool: &Tool) -> Result<(), String> {
        let n = self.p.len();
        host.invoke("OGCConnectionSystem.Configure", json!({"positions":self.positions(tool),"contact_radius":0.0012,"contact_stiffness":35000.,"surfaces":[
            self.contact_surface(0), coupling::tool_surface(tool,n), coupling::table_surface(tool,n+tool.p.len())]}))?;
        self.dirty = false;
        Ok(())
    }
    pub(super) fn shear(&mut self, slope: [f32; 2]) {
        let old = self.rest.clone();
        for p in self.p.iter_mut().chain(&mut self.rest) {
            p[1] += slope[0] * p[0] + slope[1] * p[2];
        }
        for e in &mut self.elements {
            for (a, b, rest, _) in &mut e.edges {
                *rest = length(sub(self.rest[*a], self.rest[*b]));
            }
            e.volumes = TETS.map(|t| volume(t.map(|k| self.rest[e.nodes[k]])));
        }
        for r in &mut self.roots {
            let a = length(sub(
                r.ends[0].position(&old, &self.elements),
                r.ends[1].position(&old, &self.elements),
            ));
            let b = length(sub(
                r.ends[0].position(&self.rest, &self.elements),
                r.ends[1].position(&self.rest, &self.elements),
            ));
            r.rest *= b / a.max(1e-8);
        }
        for seam in &mut self.seams {
            let n = seam.normal;
            let next = [n[0] - slope[0] * n[1], n[1], n[2] - slope[1] * n[1]];
            seam.area *= length(next);
            seam.normal = unit(next);
        }
        self.dirty = true;
    }
    pub(super) fn moving(&self) -> bool {
        self.velocity.iter().any(|v| length(*v) > 0.08)
    }
    pub(super) fn translate(&mut self, offset: V) {
        for p in self.p.iter_mut().chain(&mut self.rest) {
            *p = add(*p, offset);
        }
        self.dirty = true;
    }
    pub(super) fn collision(&self) -> Vec<[V; 3]> {
        (0..self.elements.len())
            .flat_map(|i| self.faces(i))
            .map(|f| f.map(|i| self.p[i]))
            .collect()
    }
    pub(super) fn lower_nodes(&self) -> Vec<usize> {
        self.elements
            .iter()
            .flat_map(|e| e.nodes[3..].iter().copied())
            .collect()
    }
    pub(super) fn inverse_mass(&self, i: usize) -> f32 {
        self.inv_mass[i]
    }
    pub(super) fn distance(&self, p: V) -> f32 {
        (0..self.elements.len())
            .flat_map(|i| self.faces(i))
            .map(|f| tools::point_triangle(p, f.map(|i| self.p[i])))
            .fold(f32::INFINITY, f32::min)
    }
    pub(super) fn covers(&self, point: V) -> bool {
        self.elements.iter().any(|e| {
            let [a, b, c] = [0, 1, 2].map(|k| self.p[e.nodes[k]]);
            let det = (b[2] - c[2]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[2] - c[2]);
            if det.abs() < 1e-8 {
                return false;
            }
            let u = ((b[2] - c[2]) * (point[0] - c[0]) + (c[0] - b[0]) * (point[2] - c[2])) / det;
            let v = ((c[2] - a[2]) * (point[0] - c[0]) + (a[0] - c[0]) * (point[2] - c[2])) / det;
            u >= 0.
                && v >= 0.
                && u + v <= 1.
                && u * a[1] + v * b[1] + (1. - u - v) * c[1] > point[1]
        })
    }
    pub(super) fn refresh_contact(&mut self) {
        let needed = self.needs_self_contact();
        if needed != self.self_contact {
            self.self_contact = needed;
            self.dirty = true;
        }
    }
    fn needs_self_contact(&self) -> bool {
        // Translation is capped at 8 mm per coupled iteration. Two convex
        // element envelopes farther apart than 16 mm plus the contact band cannot meet before the
        // next check. This is conservative culling, not disabled folded-sod
        // collision. It lets the usual flat mat use OGC's cached tool path.
        let margin = if self.self_contact { 0.024 } else { 0.019 };
        let bounds: Vec<[(f32, f32); 3]> = self
            .elements
            .iter()
            .map(|e| {
                std::array::from_fn(|k| {
                    e.nodes
                        .iter()
                        .map(|&i| self.p[i][k])
                        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), x| {
                            (lo.min(x), hi.max(x))
                        })
                })
            })
            .collect();
        let mut excluded = vec![false; self.elements.len() * self.elements.len()];
        for &[a, b] in &self.touching {
            excluded[a * self.elements.len() + b] = true;
        }
        for a in 0..self.elements.len() {
            for b in a + 1..self.elements.len() {
                if excluded[a * self.elements.len() + b]
                    || (0..3).any(|k| {
                        bounds[a][k].0 - bounds[b][k].1 > margin
                            || bounds[b][k].0 - bounds[a][k].1 > margin
                    })
                {
                    continue;
                }
                let separated = [a, b].iter().any(|&element| {
                    let n = self.elements[element].nodes;
                    [
                        [n[0], n[1], n[2]],
                        [n[3], n[5], n[4]],
                        [n[0], n[3], n[4]],
                        [n[1], n[4], n[5]],
                        [n[2], n[5], n[3]],
                    ]
                    .iter()
                    .any(|&face| {
                        let axis = face_normal(face.map(|i| self.p[i]));
                        let interval = |e: usize| {
                            self.elements[e]
                                .nodes
                                .iter()
                                .map(|&i| dot(self.p[i], axis))
                                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), x| {
                                    (lo.min(x), hi.max(x))
                                })
                        };
                        let (al, ah) = interval(a);
                        let (bl, bh) = interval(b);
                        bl - ah > margin || al - bh > margin
                    })
                });
                if !separated {
                    return true;
                }
            }
        }
        false
    }
    fn seam_normal(&self, seam: &Seam, p: &[V]) -> V {
        let [a, b, c, _] = seam.pairs.map(|pair| p[pair[0]]);
        let mut normal = face_normal([a, b, c]);
        // The topology's paired endpoints can reverse the winding.
        let rest_normal = face_normal([0, 1, 2].map(|k| self.rest[seam.pairs[k][0]]));
        if dot(rest_normal, seam.normal) < 0. {
            normal = scale(normal, -1.);
        }
        normal
    }
    fn solve_bulk(&mut self, q: &mut [V], dt: f32) {
        for e in &mut self.elements {
            if e.anchored {
                continue;
            }
            for (a, b, rest, lambda) in &mut e.edges {
                let delta = sub(q[*b], q[*a]);
                let distance = length(delta).max(1e-8);
                // Root-bearing tissue is compliant in bending at the seams,
                // but has a finite shear skeleton within each soil/root parcel.
                let alpha = 1. / (24_000. * dt * dt);
                let w = self.inv_mass[*a] + self.inv_mass[*b];
                let dl = (-(distance - *rest) - alpha * *lambda) / (w + alpha);
                *lambda += dl;
                let correction = scale(delta, dl / distance);
                q[*a] = sub(q[*a], scale(correction, self.inv_mass[*a]));
                q[*b] = add(q[*b], scale(correction, self.inv_mass[*b]));
            }
            for (k, tet) in TETS.iter().enumerate() {
                let ids = tet.map(|i| e.nodes[i]);
                let [a, b, c, d] = ids.map(|i| q[i]);
                let gb = scale(cross(sub(c, a), sub(d, a)), 1. / 6.);
                let gc = scale(cross(sub(d, a), sub(b, a)), 1. / 6.);
                let gd = scale(cross(sub(b, a), sub(c, a)), 1. / 6.);
                let gradients = [scale(add(add(gb, gc), gd), -1.), gb, gc, gd];
                let alpha = e.volumes[k].abs() / (450_000. * dt * dt);
                let w = ids
                    .iter()
                    .zip(gradients)
                    .map(|(&i, g)| self.inv_mass[i] * dot(g, g))
                    .sum::<f32>();
                let dl = (-(volume([a, b, c, d]) - e.volumes[k]) - alpha * e.volume_lambda[k])
                    / (w + alpha).max(1e-12);
                e.volume_lambda[k] += dl;
                for (&i, g) in ids.iter().zip(gradients) {
                    q[i] = add(q[i], scale(g, dl * self.inv_mass[i]));
                }
            }
        }
    }
    fn solve_seams(&mut self, q: &mut [V], dt: f32) {
        for id in 0..self.seams.len() {
            let normal = self.seam_normal(&self.seams[id], q);
            let s = &mut self.seams[id];
            for (k, [a, b]) in s.pairs.into_iter().enumerate() {
                let w = self.inv_mass[a] + self.inv_mass[b];
                if w == 0. {
                    continue;
                }
                let error = sub(q[b], q[a]);
                let opening = dot(error, normal);
                if s.broken {
                    if !s.contact_ready && opening < 0. {
                        let correction = scale(normal, -opening / w);
                        q[a] = sub(q[a], scale(correction, self.inv_mass[a]));
                        q[b] = add(q[b], scale(correction, self.inv_mass[b]));
                    }
                    continue;
                }
                let alpha = 1. / (18_000. * dt * dt);
                let dl = scale(add(error, scale(s.lambda[k], alpha)), -1. / (w + alpha));
                s.lambda[k] = add(s.lambda[k], dl);
                q[a] = sub(q[a], scale(dl, self.inv_mass[a]));
                q[b] = add(q[b], scale(dl, self.inv_mass[b]));
                let tension = opening.max(0.);
                let shear = length(sub(error, scale(normal, opening)));
                // Compression does not disintegrate soil at rest.
                let energy = 0.5 * 18_000. * (tension * tension + 0.45 * shear * shear);
                s.peak = s.peak.max(energy);
            }
        }
    }
    fn solve_roots(&mut self, q: &mut [V], dt: f32) {
        for r in &mut self.roots {
            if r.broken {
                continue;
            }
            let [a, b] = r.ends.map(|a| a.position(q, &self.elements));
            let delta = sub(b, a);
            let distance = length(delta).max(1e-9);
            let extension = distance - r.rest;
            let w = r
                .ends
                .iter()
                .flat_map(|a| a.weights(&self.elements))
                .map(|(i, b)| self.inv_mass[i] * b * b)
                .sum::<f32>();
            if w == 0. {
                continue;
            }
            // Fibres carry tension; buckled roots cannot push soil apart.
            let alpha = 1. / (r.stiffness * dt * dt);
            let next = (r.lambda + (-extension - alpha * r.lambda) / (w + alpha)).min(0.);
            let dl = next - r.lambda;
            r.lambda = next;
            r.peak = r.peak.max(-next / (dt * dt));
            for (side, anchor) in r.ends.iter().enumerate() {
                for (i, weight) in anchor.weights(&self.elements) {
                    q[i] = add(
                        q[i],
                        scale(
                            delta,
                            if side == 0 { -dl } else { dl } * self.inv_mass[i] * weight / distance,
                        ),
                    );
                }
            }
        }
    }
    fn anchor_at(&self, element: usize, point: V) -> Anchor {
        let n = self.elements[element].nodes;
        let [a, b, c] = [self.p[n[0]], self.p[n[1]], self.p[n[2]]];
        let u = sub(b, a);
        let v = sub(c, a);
        let down = scale(
            (0..3).fold([0.; 3], |s, k| add(s, sub(self.p[n[k + 3]], self.p[n[k]]))),
            1. / 3.,
        );
        let det = dot(u, cross(v, down));
        let d = sub(point, a);
        if det.abs() < 1e-12 {
            return Anchor {
                element,
                bary: [1. / 3.; 3],
                depth: 0.5,
            };
        }
        let x = dot(d, cross(v, down)) / det;
        let y = dot(u, cross(d, down)) / det;
        Anchor {
            element,
            bary: [1. - x - y, x, y],
            depth: dot(u, cross(v, d)) / det,
        }
    }
    fn sever(&mut self, index: usize, cut: bool) {
        if self.roots[index].broken {
            return;
        }
        let r = &self.roots[index];
        let [a, b] = r.ends.map(|e| e.position(&self.p, &self.elements));
        let middle = scale(add(a, b), 0.5);
        let tips = r.ends.map(|e| self.anchor_at(e.element, middle));
        self.roots[index].tips = tips;
        self.roots[index].broken = true;
        self.roots[index].lambda = 0.;
        if cut {
            self.cut_roots += 1;
        } else {
            self.torn_roots += 1;
        }
    }
    fn fracture(&mut self, tool: &Tool, old_tool: &[V], response: &[[f32; 13]], impact: f32) {
        let load = response
            .iter()
            .map(|r| length([r[0], r[1], r[2]]))
            .sum::<f32>();
        for (k, r) in response.iter().enumerate() {
            let force = [r[0], r[1], r[2]];
            let f = length(force);
            if f < 0.05 {
                continue;
            }
            let mut nearest = (f32::INFINITY, 0usize, [[0.; 3]; 3]);
            for id in 0..self.elements.len() {
                for face in self.faces(id) {
                    let tri = face.map(|i| self.p[i]);
                    let d = tools::point_triangle(tool.p[k], tri);
                    if d < nearest.0 {
                        nearest = (d, id, tri);
                    }
                }
            }
            if nearest.0 > 0.010 {
                continue;
            }
            let work = (-dot(force, sub(tool.p[k], old_tool[k])))
                .max(0.)
                .max(impact * f / load.max(1e-8));
            if work <= 0. {
                continue;
            }
            self.tool_work += work;
            let pressure = f / tool.projected_area(k, force, nearest.2, 0.0024).max(1e-7);
            // Local contact opens the soil matrix first. Root cutting needs an
            // actual sharp leading edge, pressure, and finite delivered work.
            let cutting = tool.is_cutting_vertex(k) && pressure > 160_000.;
            let seams: Vec<_> = self.elements[nearest.1]
                .seams
                .iter()
                .flatten()
                .copied()
                .collect();
            let sum = seams
                .iter()
                .map(|&s| {
                    let center = scale(
                        self.seams[s]
                            .pairs
                            .iter()
                            .fold([0.; 3], |p, v| add(p, self.p[v[0]])),
                        0.25,
                    );
                    1. / (0.015 + length(sub(center, tool.p[k]))).powi(2)
                })
                .sum::<f32>();
            for s in seams {
                let center = scale(
                    self.seams[s]
                        .pairs
                        .iter()
                        .fold([0.; 3], |p, v| add(p, self.p[v[0]])),
                    0.25,
                );
                let share =
                    work / ((0.015 + length(sub(center, tool.p[k]))).powi(2) * sum.max(1e-8));
                let matrix = !self.seams[s].broken;
                if pressure > 35_000. && matrix {
                    self.seams[s].work += share * if cutting { 0.65 } else { 1. };
                }
                if cutting {
                    for i in self.seams[s].roots {
                        if self.roots[i].broken {
                            continue;
                        }
                        let ends = self.roots[i]
                            .ends
                            .map(|a| a.position(&self.p, &self.elements));
                        let d = tool.edge_distance(scale(add(ends[0], ends[1]), 0.5));
                        // One element-wide fracture process zone. It cannot
                        // sever remote roots or supply work from a clock.
                        if d < STEP * 0.9 {
                            self.roots[i].work += share * if matrix { 0.35 } else { 1. } / 4.;
                        }
                    }
                }
            }
        }
        for s in &mut self.seams {
            if !s.broken && (s.work + s.peak > s.area * 20.) {
                s.broken = true;
                s.lambda = [[0.; 3]; 4];
                self.dirty = true;
            }
        }
        for i in 0..self.roots.len() {
            let r = &self.roots[i];
            if r.broken {
                continue;
            }
            let cut = r.work >= 0.010;
            if cut || r.peak > r.strength {
                self.sever(i, cut);
            }
        }
        for i in 0..self.seams.len() {
            if !self.seams[i].broken || self.seams[i].contact_ready {
                continue;
            }
            let normal = self.seam_normal(&self.seams[i], &self.p);
            if self.seams[i]
                .pairs
                .iter()
                .all(|&[a, b]| dot(sub(self.p[b], self.p[a]), normal) > 0.003)
            {
                self.seams[i].contact_ready = true;
                self.dirty = true;
            }
        }
        let old = std::mem::take(&mut self.touching);
        for pair in old {
            let bounds = |i: usize, axis: usize| {
                self.elements[i]
                    .nodes
                    .iter()
                    .map(|&v| self.p[v][axis])
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
                        (lo.min(p), hi.max(p))
                    })
            };
            let separated = (0..3).any(|axis| {
                let (al, ah) = bounds(pair[0], axis);
                let (bl, bh) = bounds(pair[1], axis);
                bl - ah > 0.003 || al - bh > 0.003
            });
            if separated {
                self.dirty = true;
            } else {
                self.touching.push(pair);
            }
        }
    }
    pub(super) fn begin_step(&mut self, dt: f32) -> coupling::Step {
        for e in &mut self.elements {
            e.volume_lambda = [0.; 3];
            for edge in &mut e.edges {
                edge.3 = 0.;
            }
        }
        for s in &mut self.seams {
            s.lambda = [[0.; 3]; 4];
            s.peak = 0.;
        }
        for r in &mut self.roots {
            r.lambda = 0.;
            r.peak = 0.;
        }
        coupling::Step::new(&self.p, &self.velocity, &self.inv_mass, dt)
    }
    pub(super) fn material_target(&mut self, step: &mut coupling::Step, dt: f32) -> Vec<V> {
        let mut q = self.p.clone();
        self.solve_bulk(&mut q, dt);
        self.solve_seams(&mut q, dt);
        self.solve_roots(&mut q, dt);
        step.target(&self.p, &mut q);
        q
    }
    pub(super) fn accept_contact(&mut self, q: &[V], response: &[[f32; 13]], dt: f32) {
        for i in 0..self.p.len() {
            if self.inv_mass[i] > 0. {
                self.p[i] = add(
                    self.p[i],
                    translation(self.p[i], q[i], self.mass[i], &response[i..i + 1], dt),
                );
            }
        }
    }
    pub(super) fn finish_step(
        &mut self,
        step: &coupling::Step,
        tool: &Tool,
        tool_old: &[V],
        response: &[[f32; 13]],
        dt: f32,
        impact: f32,
    ) -> Result<(), String> {
        self.fracture(tool, tool_old, response, impact);
        for i in 0..self.p.len() {
            self.velocity[i] = scale(sub(self.p[i], step.old[i]), (-4. * dt).exp() / dt);
        }
        if self
            .p
            .iter()
            .chain(&self.velocity)
            .flatten()
            .any(|v| !v.is_finite())
        {
            return Err("non-finite turf state".into());
        }
        for e in &self.elements {
            for (k, tet) in TETS.iter().enumerate() {
                if volume(tet.map(|v| self.p[e.nodes[v]])) / e.volumes[k] < 0.10 {
                    return Err("turf element inversion; retaining last valid state".into());
                }
            }
        }
        Ok(())
    }
    pub fn substep(
        &mut self,
        host: &mut Host,
        tool: &mut Tool,
        time: f32,
        dt: f32,
    ) -> Result<(u64, f32, [f64; 4]), String> {
        if self.dirty {
            self.configure(host, tool)?;
        }
        let previous = self.positions(tool);
        let tool_old = tool.p.clone();
        let mut step = self.begin_step(dt);
        let goal = tool.goal(time, dt);
        let mut timings = [0.; 4];
        let mut contacts = 0;
        let mut peak = 0f32;
        let mut final_response = vec![[0.; 13]; tool.p.len()];
        for _ in 0..5 {
            let started = std::time::Instant::now();
            self.refresh_contact();
            if self.dirty {
                self.configure(host, tool)?;
            }
            let q = self.material_target(&mut step, dt);
            timings[0] += started.elapsed().as_secs_f64() * 1000.;
            let started = std::time::Instant::now();
            let batch = host.contact_responses(&self.positions(tool), &previous)?;
            timings[1] += started.elapsed().as_secs_f64() * 1000.;
            timings[2] += batch.broad_ms;
            timings[3] += batch.narrow_ms;
            contacts = contacts.max(batch.contacts);
            self.accept_contact(&q, &batch.responses[..self.p.len()], dt);
            let n = self.p.len();
            final_response.copy_from_slice(&batch.responses[n..n + tool.p.len()]);
            peak = peak.max(
                final_response
                    .iter()
                    .map(|r| length([r[0], r[1], r[2]]))
                    .sum(),
            );
            tool.project(&goal, &final_response, dt)?;
        }
        self.finish_step(
            &step,
            tool,
            &tool_old,
            &final_response,
            dt,
            tool.impact_energy(&tool_old, dt),
        )?;
        tool.record_contact(&tool_old, &final_response, dt);
        tool.finish(&tool_old, dt);
        Ok((contacts, peak, timings))
    }

    pub fn report(&self) -> Value {
        let mut labels: Vec<_> = (0..self.elements.len()).collect();
        fn root(p: &[usize], mut i: usize) -> usize {
            while p[i] != i {
                i = p[i];
            }
            i
        }
        for s in &self.seams {
            if !s.broken || s.roots.iter().any(|&r| !self.roots[r].broken) {
                let a = root(&labels, s.elements[0]);
                let b = root(&labels, s.elements[1]);
                labels[b] = a;
            }
        }
        let anchored: std::collections::BTreeSet<_> = self
            .elements
            .iter()
            .enumerate()
            .filter(|(_, e)| e.anchored)
            .map(|(i, _)| root(&labels, i))
            .collect();
        let pieces: std::collections::BTreeSet<_> =
            (0..labels.len()).map(|i| root(&labels, i)).collect();
        let lifted = self
            .elements
            .iter()
            .enumerate()
            .filter(|(i, e)| {
                !anchored.contains(&root(&labels, *i))
                    && e.nodes
                        .iter()
                        .map(|&i| self.p[i][1] - self.rest[i][1])
                        .sum::<f32>()
                        / 6.
                        > 0.04
            })
            .count();
        json!({"model":"deformable volumetric turf, cohesive soil seams and irreversibly cut/ruptured tensile root bundles",
            "elements":self.elements.len(),"crowns":self.elements.len()*CROWNS.len(),"blades":self.elements.len()*CROWNS.len()*4,
            "root_bundles":self.roots.len(),"severed_roots":self.cut_roots+self.torn_roots,"cut_roots":self.cut_roots,"torn_roots":self.torn_roots,
            "open_soil_seams":self.seams.iter().filter(|s|s.broken).count(),"pieces":pieces.len(),"lifted_elements":lifted,
            "mass_kg":self.mass.iter().sum::<f32>(),"tool_contact_work_j":self.tool_work,
            "maximum_node_speed_m_s":self.velocity.iter().map(|v|length(*v)).fold(0f32,f32::max),
            "kinetic_energy_j":self.velocity.iter().zip(&self.mass).map(|(v,m)|0.5*m*dot(*v,*v)).sum::<f32>(),
            "initial_volume_m3":self.elements.iter().flat_map(|e|e.volumes).map(f32::abs).sum::<f32>(),
            "current_volume_m3":self.elements.iter().flat_map(|e|TETS.map(|tet|volume(tet.map(|k|self.p[e.nodes[k]])))).map(f32::abs).sum::<f32>(),
            "foliage":"attached crown geometry; individual leaf dynamics and biology remain in the separate seedling experiment"})
    }

    pub fn render(&self, out: &mut Vec<[f32; 6]>) {
        self.render_with_leaves(out, 4);
    }
    pub(super) fn render_with_leaves(&self, out: &mut Vec<[f32; 6]>, leaves: usize) {
        for (id, e) in self.elements.iter().enumerate() {
            let n = e.nodes;
            let normal = face_normal([self.p[n[0]], self.p[n[1]], self.p[n[2]]]);
            for f in self.faces(id) {
                let top = f.iter().all(|i| n[..3].contains(i));
                let bottom = f.iter().all(|i| n[3..].contains(i));
                let color = if top {
                    world_field::TURF_COLOR
                } else if bottom {
                    [0.20, 0.13, 0.065]
                } else {
                    world_field::ROOT_COLOR
                };
                skin::triangle(out, f.map(|i| self.p[i]), color);
            }
            let axis = unit(sub(self.p[n[1]], self.p[n[0]]));

            for (c, bary) in CROWNS.iter().enumerate() {
                let base = Anchor {
                    element: id,
                    bary: *bary,
                    depth: 0.,
                }
                .position(&self.p, &self.elements);
                world_field::grass_crown(
                    base,
                    normal,
                    axis,
                    (id * 503 + c * 73) as i32,
                    leaves,
                    |tri, color| skin::triangle(out, tri, color),
                );
            }
        }
        // Embedded bundles become visible as their matrix opens. Torn ends
        // move with their own sod fragment, never bridge or visually re-weld.
        for s in &self.seams {
            if !s.broken {
                continue;
            }
            for &i in &s.roots {
                let r = &self.roots[i];
                let ends = r.ends.map(|a| a.position(&self.p, &self.elements));
                if r.broken {
                    for k in 0..2 {
                        self.root_ribbon(out, ends[k], r.tips[k].position(&self.p, &self.elements));
                    }
                } else {
                    self.root_ribbon(out, ends[0], ends[1]);
                }
            }
        }
        // Sectioned perimeter exposes the established vertical root system.
        for (id, e) in self.elements.iter().enumerate() {
            for side in 0..3 {
                if e.seams[side].is_some() {
                    continue;
                }
                for k in 0..3 {
                    let u = (k as f32 + 0.5) / 3.;
                    let mut bary = [0.; 3];
                    bary[side] = 1. - u;
                    bary[(side + 1) % 3] = u;
                    let a = Anchor {
                        element: id,
                        bary,
                        depth: 0.06,
                    }
                    .position(&self.p, &self.elements);
                    let b = Anchor {
                        element: id,
                        bary,
                        depth: 0.88,
                    }
                    .position(&self.p, &self.elements);
                    self.root_ribbon(out, a, b);
                }
            }
        }
    }
    fn root_ribbon(&self, out: &mut Vec<[f32; 6]>, a: V, b: V) {
        let direction = unit(sub(b, a));
        let side = scale(
            unit(cross(
                direction,
                if direction[1].abs() < 0.9 {
                    [0., 1., 0.]
                } else {
                    [1., 0., 0.]
                },
            )),
            0.0009,
        );
        skin::triangle(out, [add(a, side), sub(a, side), b], [0.67, 0.57, 0.33]);
    }
}
