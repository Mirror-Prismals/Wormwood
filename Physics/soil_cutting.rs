//! Material-owned cutting state. Geometric refinement preserves matter and
//! strain; only finite blade coverage or material traction damages cohesion.
use super::{
    affine_soil::{self, Body, Matrix, Vector},
    cut_geometry::*,
    soil_material::Cohesion,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
type P = [f64; 2];
fn side(p: P, a: P, b: P) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}
fn area(p: &[P]) -> f64 {
    (0..p.len())
        .map(|i| p[i][0] * p[(i + 1) % p.len()][1] - p[(i + 1) % p.len()][0] * p[i][1])
        .sum::<f64>()
        .abs()
        * 0.5
}
fn ccw(mut p: Vec<P>) -> Vec<P> {
    if (0..p.len())
        .map(|i| p[i][0] * p[(i + 1) % p.len()][1] - p[(i + 1) % p.len()][0] * p[i][1])
        .sum::<f64>()
        < 0.
    {
        p.reverse();
    }
    p
}
fn clip(p: &[P], a: P, b: P, inside: bool) -> Vec<P> {
    let mut out = Vec::new();
    for i in 0..p.len() {
        let (x, y) = (p[i], p[(i + 1) % p.len()]);
        let (sx, sy) = (side(x, a, b), side(y, a, b));
        // Complementary halfspaces may share their zero-area boundary only.
        // An absolute signed-area tolerance makes BOTH halves retain positive
        // area when a short clipping edge scales every side value toward zero.
        let keep = |s: f64| if inside { s >= 0. } else { s <= 0. };
        if keep(sx) {
            out.push(x);
        }
        if (sx > 0. && sy < 0.) || (sx < 0. && sy > 0.) {
            let t = sx / (sx - sy);
            out.push(std::array::from_fn(|j| x[j] + t * (y[j] - x[j])));
        }
    }
    out.dedup_by(|a, b| (a[0] - b[0]).abs() + (a[1] - b[1]).abs() < 1e-11);
    out
}
fn intersect(mut p: Vec<P>, boundary: &[P]) -> Vec<P> {
    for i in 0..boundary.len() {
        p = clip(&p, boundary[i], boundary[(i + 1) % boundary.len()], true);
        if p.len() < 3 {
            return Vec::new();
        }
    }
    p
}
fn bounds_overlap(a: &[P], b: &[P], touching: bool) -> bool {
    let bounds = |p: &[P], axis: usize| {
        p.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(v[axis]), hi.max(v[axis]))
        })
    };
    (0..2).all(|axis| {
        let (amin, amax) = bounds(a, axis);
        let (bmin, bmax) = bounds(b, axis);
        if touching { amin <= bmax && bmin <= amax } else { amin < bmax && bmin < amax }
    })
}
fn subtract(p: Vec<P>, cut: &[P]) -> Vec<Vec<P>> {
    if p.len() < 3 {
        return Vec::new();
    }
    // Supporting lines of a distant polygon can cross p even though the cut
    // itself does not. Splitting on those lines creates gratuitous fragments,
    // particularly when subtracting an already-disjoint crack partition.
    if cut.len() < 3
        || !bounds_overlap(&p, cut, false)
        || area(&intersect(p.clone(), cut)) == 0.
    {
        return vec![p];
    }
    let mut inside = p;
    let mut out = Vec::new();
    for i in 0..cut.len() {
        let outside = clip(&inside, cut[i], cut[(i + 1) % cut.len()], false);
        if outside.len() >= 3 && area(&outside) > 1e-12 {
            out.push(outside);
        }
        inside = clip(&inside, cut[i], cut[(i + 1) % cut.len()], true);
        if inside.len() < 3 {
            break;
        }
    }
    out
}
fn convex_hull(mut points: Vec<P>) -> Vec<P> {
    points.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    points.dedup();
    let mut lower = Vec::new();
    for &p in &points {
        while lower.len() >= 2
            && side(p, lower[lower.len() - 2], lower[lower.len() - 1]) <= 0.
        {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper = Vec::new();
    for &p in points.iter().rev() {
        while upper.len() >= 2
            && side(p, upper[upper.len() - 2], upper[upper.len() - 1]) <= 0.
        {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}
/// Merge only when the convex hull adds no geometric area. A positive gap or
/// concavity is retained; this is not a convex approximation of a crack.
fn convex_union(a: &[P], b: &[P]) -> Option<Vec<P>> {
    if !bounds_overlap(a, b, true) {
        return None;
    }
    let overlap = intersect(a.to_vec(), b);
    let union_area = area(a) + area(b) - area(&overlap);
    let hull = convex_hull(a.iter().chain(b).copied().collect());
    let error = (area(&hull) - union_area).abs();
    let tolerance = 16. * f64::EPSILON * union_area;
    (error <= tolerance).then_some(hull)
}
/// Extend the existing disjoint union using only newly accepted masks. Old
/// regions are never subtracted from one another again. A single merge pass
/// removes eligible seams without a cubic all-pairs fixed-point search.
fn extend_damage(mut regions: Vec<Vec<P>>, mut masks: Vec<Vec<P>>) -> Vec<Vec<P>> {
    masks.sort_by(|a, b| area(b).total_cmp(&area(a)));
    for mask in masks {
        let mut pieces = vec![mask];
        for old in &regions {
            pieces = pieces.into_iter().flat_map(|p| subtract(p, old)).collect();
            if pieces.is_empty() {
                break;
            }
        }
        let before = regions.iter().chain(&pieces).map(|p| area(p)).sum::<f64>();
        let mut allowance = 32. * f64::EPSILON * before;
        for mut piece in pieces {
            let mut i = 0;
            while i < regions.len() {
                if let Some(union) = convex_union(&piece, &regions[i]) {
                    let error = (area(&union) - area(&piece) - area(&regions[i])).abs();
                    if error <= allowance {
                        allowance -= error;
                        piece = union;
                        regions.remove(i);
                        continue;
                    }
                }
                i += 1;
            }
            regions.push(piece);
        }
    }
    regions
}
#[cfg(test)]
fn canonical_damage(masks: Vec<Vec<P>>) -> Vec<Vec<P>> {
    extend_damage(Vec::new(), masks)
}
#[derive(Clone)]
struct Tag {
    id: u64,
    origin: D,
    axes: [D; 2],
    normal: D,
    damage: Vec<Vec<P>>,
}
impl Tag {
    fn extend_damage(&mut self, previous: usize) {
        let added = self.damage.split_off(previous);
        #[cfg(test)]
        if self.damage.len() + added.len() >= 64 {
            if let Ok(path) = std::env::var("MIRROR_DAMAGE_UNION_CAPTURE") {
                if !std::path::Path::new(&path).exists() {
                    let value = serde_json::json!({"tag":self.id,"canonical_regions":self.damage,"new_masks":added});
                    std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
                }
            }
        }
        self.damage = extend_damage(std::mem::take(&mut self.damage), added);
    }
    fn new(id: u64, points: &[D]) -> Self {
        let origin = points[0];
        let axis = unit(sub(points[1], origin));
        let normal = unit(cross(axis, sub(points[2], origin)));
        Self {
            id,
            origin,
            axes: [axis, cross(normal, axis)],
            normal,
            damage: Vec::new(),
        }
    }
    fn uv(&self, p: D) -> P {
        self.axes.map(|a| dot(sub(p, self.origin), a))
    }
    fn point(&self, p: P) -> D {
        add(
            self.origin,
            add(mul(self.axes[0], p[0]), mul(self.axes[1], p[1])),
        )
    }
}
#[derive(Clone)]
pub(super) struct Joint {
    pub a: u64,
    pub b: u64,
    point: D,
    area: f64,
    tag: u64,
    patch: Vec<P>,
}
#[derive(Clone)]
struct Pair {
    a: u64,
    b: u64,
    tag: u64,
    normal: D,
    points: Vec<D>,
}
#[derive(Clone)]
struct Root {
    a: u64,
    b: u64,
    pa: D,
    pb: D,
    rest: f64,
    limit: f64,
    broken: bool,
    peak: f64,
}
#[derive(Clone, Default)]
pub(super) struct Stats {
    pub cuts: u64,
    pub refinements: u64,
    pub cohesion_broken: u64,
    pub roots_broken: u64,
    pub peak_root_load: f64,
    pub cut_area: f64,
    pub fracture_work: f64,
    pub peak_lift: f64,
}
#[derive(Clone)]
pub(super) struct Material {
    pub bodies: Vec<Body>,
    tags: Vec<Tag>,
    pub joints: Vec<Joint>,
    pairs: Vec<Pair>,
    roots: Vec<Root>,
    pub stats: Stats,
    pub rest_volume: f64,
    pub mass: f64,
    pub detached: BTreeSet<u64>,
    pub revision: u64,
    next_id: u64,
    next_tag: u64,
    min: D,
    max: D,
    pub work_reservoir: f64,
    refined_directions: Vec<D>,
    depth_planes: BTreeSet<i32>,
}
impl Material {
    pub fn new(min: D, max: D) -> Self {
        let poly = Poly::box_from(min, max);
        let mut body = Body::new(1, poly, 1600., min[1]);
        let anchors = |p: D| {
            p[1] <= min[1] + 1e-8
                || (p[0] - min[0]).abs() < 1e-8
                || (p[0] - max[0]).abs() < 1e-8
                || (p[2] - min[2]).abs() < 1e-8
                || (p[2] - max[2]).abs() < 1e-8
        };
        body.anchors = body
            .poly
            .vertices
            .iter()
            .filter(|v| anchors(v.material))
            .map(|v| v.material)
            .collect();
        let rest_volume = body.rest_volume;
        let mass = body.mass;
        let mut roots = Vec::new();
        for z in 0..2 {
            for x in 0..2 {
                for k in 0..3 {
                    let px = min[0]
                        + (max[0] - min[0]) * (x as f64 + 0.5) / 2.
                        + (k as f64 - 1.) * 0.025;
                    let pz = min[2] + (max[2] - min[2]) * (z as f64 + 0.5) / 2.;
                    let pa = [px - 0.012, min[1] + 0.025, pz + 0.015];
                    let pb = [px, max[1] - 0.025, pz];
                    roots.push(Root {
                        a: 1,
                        b: 1,
                        pa,
                        pb,
                        rest: length(sub(pb, pa)),
                        limit: 75. + ((x * 7 + z * 3 + k) % 5) as f64 * 15.,
                        broken: false,
                        peak: 0.,
                    });
                }
            }
        }
        Self {
            bodies: vec![body],
            tags: Vec::new(),
            joints: Vec::new(),
            pairs: Vec::new(),
            roots,
            stats: Stats::default(),
            rest_volume,
            mass,
            detached: BTreeSet::new(),
            revision: 1,
            next_id: 2,
            next_tag: 100,
            min,
            max,
            work_reservoir: 0.,
            refined_directions: Vec::new(),
            depth_planes: BTreeSet::new(),
        }
    }
    fn index(&self, id: u64) -> usize {
        self.bodies.iter().position(|b| b.id == id).unwrap()
    }
    fn locate(&self, id: u64, point: D) -> Option<u64> {
        self.bodies
            .iter()
            .filter(|b| b.id == id || b.ancestors.contains(&id))
            .find(|b| b.poly.coordinates(point, true).is_some())
            .map(|b| b.id)
    }
    fn set_anchors(&self, b: &mut Body) {
        b.anchors = b
            .poly
            .vertices
            .iter()
            .filter(|v| {
                let p = v.material;
                p[1] <= self.min[1] + 1e-8
                    || (p[0] - self.min[0]).abs() < 1e-8
                    || (p[0] - self.max[0]).abs() < 1e-8
                    || (p[2] - self.min[2]).abs() < 1e-8
                    || (p[2] - self.max[2]).abs() < 1e-8
            })
            .map(|v| v.material)
            .collect();
    }
    /// Refinement is mechanically neutral: current deformation and velocity,
    /// reference volume, density and existing damage all survive the partition.
    fn partition(&mut self, plane: Plane, near: D) -> bool {
        self.partition_region(plane, near, None)
    }
    fn partition_region(
        &mut self,
        plane: Plane,
        near: D,
        region: Option<(D, D, f64, f64, Option<f64>)>,
    ) -> bool {
        let mut changed = false;
        let count = self.bodies.len();
        for i in (0..count).rev() {
            if self.bodies.len() >= 128 {
                break;
            }
            let body = &self.bodies[i];
            if length(sub(body.center, near)) > 0.9 {
                continue;
            }
            if region.is_some_and(|(x, z, w, d, h)| {
                let p = sub(body.center, near);
                dot(p, x).abs() > w + 1e-5
                    || dot(p, z).abs() > d + 1e-5
                    || h.is_some_and(|height| p[1].abs() > height)
            }) {
                continue;
            }
            let tag = self.next_tag;
            let Some(mut parts) = body.split(plane, tag, [self.next_id, self.next_id + 1]) else {
                continue;
            };
            if parts
                .iter()
                .any(|p| p.rest_volume < self.rest_volume * 1e-7)
            {
                continue;
            }
            let points: Vec<_> = parts[0]
                .poly
                .faces
                .iter()
                .find(|f| f.tag == tag)
                .unwrap()
                .nodes
                .iter()
                .map(|i| parts[0].poly.vertices[*i].material)
                .collect();
            self.tags.push(Tag::new(tag, &points));
            self.next_tag += 1;
            self.next_id += 2;
            for p in &mut parts {
                self.set_anchors(p);
            }
            self.bodies.remove(i);
            self.bodies.extend(parts);
            changed = true;
            self.stats.refinements += 1;
        }
        if changed {
            for i in 0..self.roots.len() {
                let (a, b, pa, pb) = {
                    let r = &self.roots[i];
                    (r.a, r.b, r.pa, r.pb)
                };
                if let (Some(na), Some(nb)) = (self.locate(a, pa), self.locate(b, pb)) {
                    self.roots[i].a = na;
                    self.roots[i].b = nb;
                }
            }
            self.rebuild_joints(true);
            self.revision += 1;
        }
        changed
    }
    fn rebuild_joints(&mut self, topology_changed: bool) {
        let previous_pairs = if topology_changed {
            Vec::new()
        } else {
            self.pairs.clone()
        };
        self.joints.clear();
        self.pairs.clear();
        let mut faces: BTreeMap<u64, Vec<(u64, Vec<P>)>> = BTreeMap::new();
        for b in &self.bodies {
            for f in &b.poly.faces {
                if let Some(tag) = self.tags.iter().find(|t| t.id == f.tag) {
                    let p = ccw(f
                        .nodes
                        .iter()
                        .map(|i| tag.uv(b.poly.vertices[*i].material))
                        .collect());
                    faces.entry(f.tag).or_default().push((b.id, p));
                }
            }
        }
        for (tag_id, faces) in faces {
            let tag = self.tags.iter().find(|t| t.id == tag_id).unwrap();
            for i in 0..faces.len() {
                for j in i + 1..faces.len() {
                    let (a, b) = (faces[i].0, faces[j].0);
                    let ca = self.bodies[self.index(a)].reference_center;
                    let cb = self.bodies[self.index(b)].reference_center;
                    if dot(sub(ca, tag.origin), tag.normal) * dot(sub(cb, tag.origin), tag.normal)
                        >= 0.
                    {
                        continue;
                    }
                    let overlap = intersect(faces[i].1.clone(), &faces[j].1);
                    if overlap.len() < 3 || area(&overlap) < 1e-11 {
                        continue;
                    }
                    let normal = mul(
                        tag.normal,
                        if dot(sub(ca, tag.origin), tag.normal) < 0. {
                            1.
                        } else {
                            -1.
                        },
                    );
                    self.pairs.push(Pair {
                        a,
                        b,
                        tag: tag_id,
                        normal,
                        points: overlap.iter().map(|p| tag.point(*p)).collect(),
                    });
                    let mut regions = vec![overlap];
                    for damage in &tag.damage {
                        regions = regions
                            .into_iter()
                            .flat_map(|p| subtract(p, damage))
                            .collect();
                    }
                    for p in regions {
                        for k in 1..p.len() - 1 {
                            let tri = [p[0], p[k], p[k + 1]];
                            let longest = (0..3)
                                .map(|i| {
                                    let a = tri[i];
                                    let b = tri[(i + 1) % 3];
                                    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
                                })
                                .fold(0f64, f64::max);
                            let n = (longest / 0.09).ceil().clamp(1., 8.) as usize;
                            let at = |i: usize, j: usize| {
                                std::array::from_fn::<_, 2, _>(|k| {
                                    tri[0][k]
                                        + (tri[1][k] - tri[0][k]) * i as f64 / n as f64
                                        + (tri[2][k] - tri[0][k]) * j as f64 / n as f64
                                })
                            };
                            for y in 0..n {
                                for x in 0..n - y {
                                    let mut small =
                                        vec![vec![at(x, y), at(x + 1, y), at(x, y + 1)]];
                                    if x + y + 1 < n {
                                        small.push(vec![
                                            at(x + 1, y),
                                            at(x + 1, y + 1),
                                            at(x, y + 1),
                                        ]);
                                    }
                                    for patch in small {
                                        let weight = area(&patch) / 3.;
                                        if weight < 1e-12 {
                                            continue;
                                        }
                                        for q in 0..3 {
                                            let uv = std::array::from_fn(|axis| {
                                                patch[q][axis] * 0.6
                                                    + patch[(q + 1) % 3][axis] * 0.2
                                                    + patch[(q + 2) % 3][axis] * 0.2
                                            });
                                            self.joints.push(Joint {
                                                a,
                                                b,
                                                point: tag.point(uv),
                                                area: weight,
                                                tag: tag_id,
                                                patch: patch.clone(),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if !topology_changed {
            self.pairs = previous_pairs;
            return;
        }
        // Nonconforming cuts also create edge and corner junctions. They have
        // zero initial clearance just like face pairs and need a supporting
        // contact plane until native OGC has a finite gap to work with.
        for i in 0..self.bodies.len() {
            for j in i + 1..self.bodies.len() {
                let a = &self.bodies[i];
                let b = &self.bodies[j];
                if self
                    .pairs
                    .iter()
                    .any(|p| (p.a == a.id && p.b == b.id) || (p.a == b.id && p.b == a.id))
                {
                    continue;
                }
                let boundary_contains = |body: &Body, p: D| {
                    body.poly.coordinates(p, true).is_some()
                        && body.poly.faces.iter().any(|f| {
                            let p0 = body.poly.vertices[f.nodes[0]].material;
                            let n = unit(cross(
                                sub(body.poly.vertices[f.nodes[1]].material, p0),
                                sub(body.poly.vertices[f.nodes[2]].material, p0),
                            ));
                            dot(n, sub(p, p0)).abs() < 1e-8
                        })
                };
                let mut points = Vec::new();
                for (source, target) in [(a, b), (b, a)] {
                    for v in &source.poly.vertices {
                        if boundary_contains(target, v.material)
                            && !points.iter().any(|p| length(sub(*p, v.material)) < 1e-8)
                        {
                            points.push(v.material);
                        }
                    }
                }
                if points.is_empty() {
                    // A thin kerf can leave two pieces close together with a third
                    // piece between them. Their internal faces are not exposed to
                    // one another, even though OGC's query radius spans the strip.
                    // Keep a separating constraint while they are close, then
                    // return them to native contact after they gain clearance.
                    let bounds = |body: &Body| {
                        std::array::from_fn::<_, 3, _>(|axis| {
                            body.poly
                                .vertices
                                .iter()
                                .map(|v| v.material[axis])
                                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
                                    (lo.min(x), hi.max(x))
                                })
                        })
                    };
                    let ba = bounds(a);
                    let bb = bounds(b);
                    if (0..3).any(|k| ba[k].0 > bb[k].1 + 0.0015 || bb[k].0 > ba[k].1 + 0.0015) {
                        continue;
                    }
                    let mut best = (f64::NEG_INFINITY, [0.; 3]);
                    for body in [a, b] {
                        for face in &body.poly.faces {
                            let p = face
                                .nodes
                                .iter()
                                .map(|i| body.poly.vertices[*i].material)
                                .collect::<Vec<_>>();
                            let axis = unit(cross(sub(p[1], p[0]), sub(p[2], p[0])));
                            for n in [axis, mul(axis, -1.)] {
                                let gap = b
                                    .poly
                                    .vertices
                                    .iter()
                                    .map(|v| dot(n, v.material))
                                    .fold(f64::INFINITY, f64::min)
                                    - a.poly
                                        .vertices
                                        .iter()
                                        .map(|v| dot(n, v.material))
                                        .fold(f64::NEG_INFINITY, f64::max);
                                if gap > best.0 {
                                    best = (gap, n);
                                }
                            }
                        }
                    }
                    if best.0 > 1e-8 && best.0 <= 0.0015 {
                        self.pairs.push(Pair {
                            a: a.id,
                            b: b.id,
                            tag: 0,
                            normal: best.1,
                            points: Vec::new(),
                        });
                    }
                    continue;
                }
                let direction = sub(b.reference_center, a.reference_center);
                let mut normals = Vec::<D>::new();
                for body in [a, b] {
                    for face in &body.poly.faces {
                        let p = face
                            .nodes
                            .iter()
                            .map(|i| body.poly.vertices[*i].material)
                            .collect::<Vec<_>>();
                        let mut n = unit(cross(sub(p[1], p[0]), sub(p[2], p[0])));
                        if dot(n, direction) < 0. {
                            n = mul(n, -1.);
                        }
                        let max_a = a
                            .poly
                            .vertices
                            .iter()
                            .map(|v| dot(n, v.material))
                            .fold(f64::NEG_INFINITY, f64::max);
                        let min_b = b
                            .poly
                            .vertices
                            .iter()
                            .map(|v| dot(n, v.material))
                            .fold(f64::INFINITY, f64::min);
                        if (min_b - max_a).abs() < 1e-8
                            && !normals.iter().any(|other| dot(*other, n) > 0.99999)
                        {
                            normals.push(n);
                        }
                    }
                }
                let normal = unit(normals.into_iter().fold([0.; 3], add));
                if length(normal) > 0.9 {
                    self.pairs.push(Pair {
                        a: a.id,
                        b: b.id,
                        tag: 0,
                        normal,
                        points,
                    });
                }
            }
        }
    }
    pub fn cohesive_area(&self) -> f64 {
        self.joints.iter().map(|j| j.area).sum()
    }
    /// Prepare material around the requested edge. This only changes topology:
    /// no material can fracture until a physical edge movement is accepted.
    pub fn cut(&mut self, edge: [D; 2], wanted: [D; 2], effort: f64, kerf: f64) -> bool {
        #[cfg(test)]
        let before_count = self.bodies.len();
        if effort < 2. {
            return false;
        }
        let tangent = unit(sub(edge[1], edge[0]));
        let travel = mul(add(sub(wanted[0], edge[0]), sub(wanted[1], edge[1])), 0.5);
        if length(travel) < 1e-6 {
            return false;
        }
        let normal = unit(cross(tangent, travel));
        if length(normal) < 0.9 {
            return false;
        }
        let center = mul(add(edge[0], edge[1]), 0.5);
        let mut changed = false;
        if !self
            .refined_directions
            .iter()
            .any(|d| dot(*d, tangent).abs() > 0.98)
        {
            for (i, p) in edge.into_iter().enumerate() {
                changed |= self.partition(
                    Plane::through(
                        add(p, mul(tangent, if i == 0 { -0.035 } else { 0.035 })),
                        tangent,
                    ),
                    center,
                );
            }
            let forward = unit(cross(tangent, [0., 1., 0.]));
            for distance in [-0.24, 0.24] {
                changed |= self.partition(
                    Plane::through(add(center, mul(forward, distance)), forward),
                    center,
                );
            }
            changed |= self.partition(
                Plane::through(add(center, [0., -0.20, 0.]), [0., 1., 0.]),
                center,
            );
            self.refined_directions.push(tangent);
        }
        let depth = (center[1] / 0.02).floor() as i32 - 1;
        let y = depth as f64 * 0.02;
        if y > self.min[1] + 0.005 && !self.depth_planes.contains(&depth) {
            let width = length(sub(edge[1], edge[0])) * 0.5 + 0.04;
            let forward = unit(cross(tangent, [0., 1., 0.]));
            changed |= self.partition_region(
                Plane::through([center[0], y, center[2]], [0., 1., 0.]),
                center,
                Some((tangent, forward, width, 0.245, None)),
            );
            self.depth_planes.insert(depth);
        }
        // Reuse a material plane for a progressive stroke. A changing blade
        // direction creates another local partition instead of voxel erosion.
        let matching = self.tags.iter().any(|tag| {
            self.bodies.iter().any(|b| {
                b.material_point(center).is_some_and(|p| {
                    if dot(sub(p, tag.origin), tag.normal).abs() >= 0.0025 {
                        return false;
                    }
                    let point = tag.uv(p);
                    b.poly
                        .faces
                        .iter()
                        .filter(|face| face.tag == tag.id)
                        .any(|face| {
                            let polygon = ccw(face
                                .nodes
                                .iter()
                                .map(|i| tag.uv(b.poly.vertices[*i].material))
                                .collect());
                            (0..polygon.len()).all(|i| {
                                let (a, z) = (polygon[i], polygon[(i + 1) % polygon.len()]);
                                side(point, a, z) >= -0.008 * ((z[0] - a[0]).hypot(z[1] - a[1]))
                            })
                        })
                }) && affine_soil::inverse(b.deformation).is_some_and(|inv| {
                    let n = unit(std::array::from_fn(|i| dot(inv[i], tag.normal)));
                    dot(n, normal).abs() > 0.985
                })
            })
        });
        if !matching {
            let forward = unit(cross(tangent, [0., 1., 0.]));
            let width = length(sub(edge[1], edge[0])) * 0.5 + 0.04;
            // Retain the matter under the finite-width cutting edge as its own
            // deformable material. It must be displaced, never boolean-deleted.
            for offset in [-kerf * 0.5, 0., kerf * 0.5] {
                changed |= self.partition_region(
                    Plane::through(add(center, mul(normal, offset)), normal),
                    center,
                    Some((tangent, forward, width, 0.245, Some(0.035))),
                );
            }
        }
        #[cfg(test)]
        if changed && std::env::var_os("MIRROR_CUT_REFINEMENT_LOG").is_some() {
            eprintln!(
                "cut refinement {before_count}->{} center={center:?} normal={normal:?} travel={} kerf={kerf}",
                self.bodies.len(),
                length(travel)
            );
        }
        changed
    }
    /// Damage the supplied edge strip, funded by accepted contact work. The
    /// runtime supplies actual movement plus its bounded fracture process zone;
    /// it must never supply a blocked requested movement as an accepted cut.
    pub fn fracture(&mut self, edge: [D; 2], wanted: [D; 2]) -> bool {
        if self.work_reservoir <= 1e-7 {
            return false;
        }
        let mut changed = false;
        let before = self.cohesive_area();
        let saved: Vec<_> = self.tags.iter().map(|t| t.damage.len()).collect();
        let mut factor = 1.;
        let mut spent = 0.;
        for _ in 0..4 {
            for (tag, n) in self.tags.iter_mut().zip(&saved) {
                tag.damage.truncate(*n);
            }
            let end: [D; 2] =
                std::array::from_fn(|i| add(edge[i], mul(sub(wanted[i], edge[i]), factor)));
            for ti in 0..self.tags.len() {
                let tag = &self.tags[ti];
                let mut cut = None;
                for b in &self.bodies {
                    if !b.poly.faces.iter().any(|f| f.tag == tag.id) {
                        continue;
                    }
                    let Some(points) = edge
                        .into_iter()
                        .chain(end.into_iter().rev())
                        .map(|p| b.material_point(p))
                        .collect::<Option<Vec<_>>>()
                    else {
                        continue;
                    };
                    if points
                        .iter()
                        .any(|p| dot(sub(*p, tag.origin), tag.normal).abs() > 0.003)
                    {
                        continue;
                    }
                    let uv = ccw(points.into_iter().map(|p| tag.uv(p)).collect());
                    if area(&uv) > 1e-12 {
                        cut = Some(uv);
                        break;
                    }
                }
                if let Some(cut) = cut {
                    let overlaps = self.bodies.iter().any(|b| {
                        b.poly.faces.iter().filter(|f| f.tag == tag.id).any(|f| {
                            let face = ccw(f
                                .nodes
                                .iter()
                                .map(|i| tag.uv(b.poly.vertices[*i].material))
                                .collect());
                            let hit = intersect(face, &cut);
                            hit.len() >= 3 && area(&hit) > 1e-12
                        })
                    });
                    if overlaps {
                        self.tags[ti].damage.push(cut);
                    }
                }
            }
            if self
                .tags
                .iter()
                .zip(&saved)
                .all(|(tag, n)| tag.damage.len() == *n)
            {
                #[cfg(test)]
                if (edge[0][1] + edge[1][1]) * 0.5 < -0.001 {
                    if let Ok(path) = std::env::var("MIRROR_DAMAGE_CAPTURE") {
                        if !std::path::Path::new(&path).exists() {
                            std::fs::write(path,serde_json::to_vec(&serde_json::json!({"material":self.diagnostic(),"edge":edge,"end":end})).unwrap()).unwrap();
                        }
                    }
                }
                #[cfg(test)]
                if std::env::var_os("MIRROR_DAMAGE_DIAGNOSTICS").is_some() {
                    static N: std::sync::atomic::AtomicUsize =
                        std::sync::atomic::AtomicUsize::new(0);
                    if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 24 {
                        eprintln!(
                            "damage has no overlapping mask: edge={edge:?} end={end:?} tags={} work={}",
                            self.tags.len(),
                            self.work_reservoir
                        );
                    }
                }
                return false;
            }
            self.rebuild_joints(false);
            let removed = (before - self.cohesive_area()).max(0.);
            spent = removed * 35.;
            #[cfg(test)]
            if std::env::var_os("MIRROR_DAMAGE_DIAGNOSTICS").is_some() {
                static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
                if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 24 {
                    eprintln!(
                        "damage edge={edge:?} end={end:?} masks={} removed={removed} work={}",
                        self.tags
                            .iter()
                            .zip(&saved)
                            .map(|(t, n)| t.damage.len() - n)
                            .sum::<usize>(),
                        self.work_reservoir
                    );
                }
            }
            if spent <= self.work_reservoir + 1e-9 {
                break;
            }
            factor *= self.work_reservoir / spent * 0.95;
        }
        if spent > self.work_reservoir + 1e-8 {
            for (tag, n) in self.tags.iter_mut().zip(saved) {
                tag.damage.truncate(n);
            }
            self.rebuild_joints(false);
            return changed;
        }
        if spent <= 1e-9 {
            // A swept strip outside every cohesive face is not a paid cut.
            // Do not accumulate uncharged masks that could affect later work.
            for (tag, n) in self.tags.iter_mut().zip(saved) {
                tag.damage.truncate(n);
            }
            self.rebuild_joints(false);
            return false;
        }
        if spent > 1e-9 {
            for (tag, previous) in self.tags.iter_mut().zip(&saved) {
                if tag.damage.len() != *previous {
                    tag.extend_damage(*previous);
                }
            }
            self.rebuild_joints(false);
            self.work_reservoir -= spent;
            self.stats.fracture_work += spent;
            self.stats.cut_area += spent / 35.;
            self.stats.cuts += 1;
            self.revision += 1;
            changed = true;
        }
        changed
    }
    fn current_normal(body: &Body, tag: &Tag) -> D {
        let inv = affine_soil::inverse(body.deformation).unwrap_or(affine_soil::identity());
        unit(std::array::from_fn(|i| dot(inv[i], tag.normal)))
    }
    /// Forces and PSD spring Hessians are assembled per affine block. Failed
    /// cohesion is recorded as material-space area, surviving further cuts.
    pub fn add_material_forces(&mut self, systems: &mut [(Matrix, Vector)]) {
        let mut failures: BTreeMap<u64, Vec<Vec<P>>> = BTreeMap::new();
        for joint in &self.joints {
            let ai = self.index(joint.a);
            let bi = self.index(joint.b);
            let (a, b) = (&self.bodies[ai], &self.bodies[bi]);
            let delta = sub(b.point(joint.point), a.point(joint.point));
            let stiffness = 600000. * joint.area;
            let force = mul(delta, -stiffness);
            let tag = self.tags.iter().find(|t| t.id == joint.tag).unwrap();
            let normal = mul(
                Self::current_normal(a, tag),
                if dot(sub(a.reference_center, tag.origin), tag.normal) < 0. {
                    1.
                } else {
                    -1.
                },
            );
            if Cohesion::LOAM.fails(
                delta.map(|x| x as f32),
                force.map(|x| x as f32),
                normal.map(|x| x as f32),
                joint.area as f32,
            ) {
                failures
                    .entry(joint.tag)
                    .or_default()
                    .push(joint.patch.clone());
                continue;
            }
            let h = std::array::from_fn(|i| {
                std::array::from_fn(|j| if i == j { stiffness } else { 0. })
            });
            Body::add_force(
                &mut systems[ai].0,
                &mut systems[ai].1,
                sub(joint.point, a.reference_center),
                mul(force, -1.),
                h,
            );
            Body::add_force(
                &mut systems[bi].0,
                &mut systems[bi].1,
                sub(joint.point, b.reference_center),
                force,
                h,
            );
        }
        for r in &mut self.roots {
            if r.broken {
                continue;
            }
            let ai = self.bodies.iter().position(|b| b.id == r.a).unwrap();
            let bi = self.bodies.iter().position(|b| b.id == r.b).unwrap();
            let (a, b) = (&self.bodies[ai], &self.bodies[bi]);
            let delta = sub(b.point(r.pb), a.point(r.pa));
            let l = length(delta);
            if l <= r.rest {
                continue;
            }
            let load = 20000. * (l - r.rest);
            r.peak = r.peak.max(load);
            self.stats.peak_root_load = self.stats.peak_root_load.max(load);
            if load > r.limit || l - r.rest > 0.045 {
                r.broken = true;
                self.stats.roots_broken += 1;
                continue;
            }
            let n = unit(delta);
            let force = mul(n, -load);
            let h = std::array::from_fn(|i| std::array::from_fn(|j| 20000. * n[i] * n[j]));
            if ai == bi {
                Body::add_internal_force(
                    &mut systems[ai].0,
                    &mut systems[ai].1,
                    sub(r.pb, r.pa),
                    force,
                    h,
                );
            } else {
                Body::add_force(
                    &mut systems[ai].0,
                    &mut systems[ai].1,
                    sub(r.pa, a.reference_center),
                    mul(force, -1.),
                    h,
                );
                Body::add_force(
                    &mut systems[bi].0,
                    &mut systems[bi].1,
                    sub(r.pb, b.reference_center),
                    force,
                    h,
                );
            }
        }
        if !failures.is_empty() {
            for (id, patches) in failures {
                let tag = self.tags.iter_mut().find(|t| t.id == id).unwrap();
                let previous = tag.damage.len();
                for p in patches {
                    if !tag.damage.contains(&p) {
                        tag.damage.push(p);
                        self.stats.cohesion_broken += 1;
                    }
                }
                tag.extend_damage(previous);
            }
            self.rebuild_joints(false);
            self.revision += 1;
        }
    }
    pub fn connectivity(&mut self) {
        let mut graph: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
        for j in &self.joints {
            graph.entry(j.a).or_default().push(j.b);
            graph.entry(j.b).or_default().push(j.a);
        }
        for r in &self.roots {
            if !r.broken {
                graph.entry(r.a).or_default().push(r.b);
                graph.entry(r.b).or_default().push(r.a);
            }
        }
        let mut reached: BTreeSet<_> = self
            .bodies
            .iter()
            .filter(|b| !b.anchors.is_empty())
            .map(|b| b.id)
            .collect();
        let mut todo: VecDeque<_> = reached.iter().copied().collect();
        while let Some(id) = todo.pop_front() {
            if let Some(neighbors) = graph.get(&id) {
                for n in neighbors {
                    if reached.insert(*n) {
                        todo.push_back(*n);
                    }
                }
            }
        }
        self.detached = self
            .bodies
            .iter()
            .filter(|b| !reached.contains(&b.id))
            .map(|b| b.id)
            .collect();
        for b in &self.bodies {
            if self.detached.contains(&b.id) {
                self.stats.peak_lift = self
                    .stats
                    .peak_lift
                    .max(b.center[1] - b.reference_center[1]);
            }
        }
    }
    fn pair_normal(&self, pair: &Pair) -> D {
        let a = &self.bodies[self.index(pair.a)];
        let b = &self.bodies[self.index(pair.b)];
        let transform = |body: &Body| {
            let inv = affine_soil::inverse(body.deformation).unwrap_or(affine_soil::identity());
            unit(std::array::from_fn(|i| dot(inv[i], pair.normal)))
        };
        unit(add(transform(a), transform(b)))
    }
    fn gap(&self, pair: &Pair) -> f64 {
        let n = self.pair_normal(pair);
        let a = &self.bodies[self.index(pair.a)];
        let b = &self.bodies[self.index(pair.b)];
        b.poly
            .vertices
            .iter()
            .map(|v| dot(n, v.position))
            .fold(f64::INFINITY, f64::min)
            - a.poly
                .vertices
                .iter()
                .map(|v| dot(n, v.position))
                .fold(f64::NEG_INFINITY, f64::max)
    }
    fn faces_overlap(&self, pair: &Pair) -> bool {
        let a = &self.bodies[self.index(pair.a)];
        let b = &self.bodies[self.index(pair.b)];
        if pair.tag == 0 {
            return pair
                .points
                .iter()
                .any(|p| length(sub(a.point(*p), b.point(*p))) < 0.004);
        }
        let n = self.pair_normal(pair);
        let u = unit(cross(
            n,
            if n[0].abs() < 0.8 {
                [1., 0., 0.]
            } else {
                [0., 1., 0.]
            },
        ));
        let v = cross(n, u);
        [u, v].into_iter().all(|axis| {
            let range = |body: &Body| {
                pair.points
                    .iter()
                    .map(|p| dot(body.point(*p), axis))
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
                        (lo.min(x), hi.max(x))
                    })
            };
            let (amin, amax) = range(a);
            let (bmin, bmax) = range(b);
            amin <= bmax + 1e-5 && bmin <= amax + 1e-5
        })
    }
    fn supporting_axis(&self, pair: &Pair) -> (f64, D) {
        let a = &self.bodies[self.index(pair.a)];
        let b = &self.bodies[self.index(pair.b)];
        let gap = |n: D| {
            b.poly
                .vertices
                .iter()
                .map(|v| dot(n, v.position))
                .fold(f64::INFINITY, f64::min)
                - a.poly
                    .vertices
                    .iter()
                    .map(|v| dot(n, v.position))
                    .fold(f64::NEG_INFINITY, f64::max)
        };
        let hint = self.pair_normal(pair);
        let mut best = (gap(hint), hint);
        if best.0 >= -1e-9 {
            return best;
        }
        let consider = |best: &mut (f64, D), axis: D| {
            if length(axis) < 1e-10 {
                return;
            }
            let axis = unit(axis);
            for n in [axis, mul(axis, -1.)] {
                let g = gap(n);
                if g > best.0 {
                    *best = (g, n);
                }
            }
        };
        for body in [a, b] {
            for face in &body.poly.faces {
                let p = face
                    .nodes
                    .iter()
                    .map(|i| body.poly.vertices[*i].position)
                    .collect::<Vec<_>>();
                consider(&mut best, cross(sub(p[1], p[0]), sub(p[2], p[0])));
            }
        }
        if best.0 >= -1e-9 {
            return best;
        }
        let edges = |body: &Body| {
            body.poly
                .faces
                .iter()
                .flat_map(|f| {
                    f.nodes.iter().enumerate().map(move |(i, j)| {
                        sub(
                            body.poly.vertices[f.nodes[(i + 1) % f.nodes.len()]].position,
                            body.poly.vertices[*j].position,
                        )
                    })
                })
                .collect::<Vec<_>>()
        };
        for ea in edges(a) {
            for eb in edges(b) {
                consider(&mut best, cross(ea, eb));
            }
        }
        best
    }
    pub fn contact_planes(&self) -> Vec<affine_soil::Contact> {
        self.pairs
            .iter()
            .filter_map(|p| {
                let (gap, n) = self.supporting_axis(p);
                (gap < 0.004).then(|| affine_soil::Contact {
                    a: self.index(p.a),
                    b: self.index(p.b),
                    normal: n,
                    points: p.points.clone(),
                })
            })
            .collect()
    }
    pub fn exclusions(&self) -> Vec<[usize; 2]> {
        self.contact_planes()
            .into_iter()
            .map(|p| [p.a.min(p.b), p.a.max(p.b)])
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    pub fn touch_matrix(&self) -> Vec<Vec<bool>> {
        let mut out = vec![vec![false; self.bodies.len()]; self.bodies.len()];
        for i in 0..out.len() {
            out[i][i] = true;
        }
        for p in &self.pairs {
            if self.supporting_axis(p).0 >= -2e-8 {
                let (a, b) = (self.index(p.a), self.index(p.b));
                out[a][b] = true;
                out[b][a] = true;
            }
        }
        out
    }
    pub fn root_lines(&self) -> Vec<(D, D, bool)> {
        self.roots
            .iter()
            .map(|r| {
                (
                    self.bodies[self.index(r.a)].point(r.pa),
                    self.bodies[self.index(r.b)].point(r.pb),
                    r.broken,
                )
            })
            .collect()
    }
    /// Triangulate the damaged portion of a face in reference material space.
    /// Both rendering and collision can attach these points to the same body.
    pub fn exposed_face_material(&self, body: &Body, face: &Face) -> Vec<[D; 3]> {
        let Some(tag) = self.tags.iter().find(|tag| tag.id == face.tag) else {
            return Vec::new();
        };
        let boundary = ccw(face
            .nodes
            .iter()
            .map(|i| tag.uv(body.poly.vertices[*i].material))
            .collect());
        let mut triangles = Vec::new();
        for damage in &tag.damage {
            let clipped = intersect(boundary.clone(), damage);
            if clipped.len() < 3 {
                continue;
            }
            // Accepted damage is already a disjoint union. Re-subtracting
            // historical overlapping stamps here used to create artificial
            // interior slivers and multiply native contact features.
            for i in 1..clipped.len().saturating_sub(1) {
                triangles.push([clipped[0], clipped[i], clipped[i + 1]].map(|uv| tag.point(uv)));
            }
        }
        triangles
    }
    /// Return the current positions of the damaged portion of a cut face.
    pub fn exposed_face(&self, body: &Body, face: &Face) -> Vec<[D; 3]> {
        self.exposed_face_material(body, face)
            .into_iter()
            .map(|triangle| triangle.map(|point| body.point(point)))
            .collect()
    }
    pub fn sample_material(&self, point: D) -> Option<(D, [D; 3])> {
        let b = self
            .bodies
            .iter()
            .find(|b| b.poly.coordinates(point, true).is_some())?;
        Some((b.point(point), b.deformation))
    }
    pub fn valid(&self) -> bool {
        let volume = self.bodies.iter().map(|b| b.rest_volume).sum::<f64>();
        let mass = self.bodies.iter().map(|b| b.mass).sum::<f64>();
        (volume - self.rest_volume).abs() < self.rest_volume * 1e-8
            && (mass - self.mass).abs() < self.mass * 1e-8
            && self
                .bodies
                .iter()
                .all(|b| b.poly.valid() && b.poly.convex(1e-6))
    }
    #[cfg(test)]
    pub fn diagnostic(&self) -> serde_json::Value {
        serde_json::json!({"tags":self.tags.iter().map(|t|serde_json::json!({"id":t.id,"origin":t.origin,"normal":t.normal,"axes":t.axes,"damage":t.damage,"damage_regions":t.damage.len(),"damage_vertices":t.damage.iter().map(Vec::len).sum::<usize>()})).collect::<Vec<_>>(),"bodies":self.bodies.iter().map(|b|serde_json::json!({"id":b.id,"ancestors":b.ancestors,"mass":b.mass,"rest_volume":b.rest_volume,"initial_center":b.initial_center,"velocity":b.velocity,"deformation_velocity":b.deformation_velocity,"vertex_velocities":b.poly.vertices.iter().map(|v|v.velocity).collect::<Vec<_>>(),"center":b.center,"reference_center":b.reference_center,"deformation":b.deformation,"fixed":b.fully_fixed(),"anchors":b.anchors,"positions":b.poly.vertices.iter().map(|v|v.position).collect::<Vec<_>>(),"reference":b.poly.vertices.iter().map(|v|v.material).collect::<Vec<_>>(),"faces":b.poly.faces.iter().map(|f|serde_json::json!({"nodes":f.nodes,"tag":f.tag})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "pairs":self.pairs.iter().map(|p|serde_json::json!({"a":self.index(p.a),"b":self.index(p.b),"tag":p.tag,"gap":self.gap(p),"overlap":self.faces_overlap(p),"normal":self.pair_normal(p)})).collect::<Vec<_>>()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rectangle(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<P> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }
    #[test]
    fn short_clipping_edges_partition_area_without_epsilon_duplication() {
        let p = rectangle(0., 0., 1., 0.00001);
        let a = [0., 0.000005];
        let b = [0.00000001, 0.000005];
        let inside = clip(&p, a, b, true);
        let outside = clip(&p, a, b, false);
        assert!((area(&inside) - 0.000005).abs() < 1e-18);
        assert!((area(&outside) - 0.000005).abs() < 1e-18);
        assert!((area(&inside) + area(&outside) - area(&p)).abs() < 1e-18);
        assert_eq!(area(&intersect(inside, &outside)), 0.);
    }
    #[test]
    fn subtraction_preserves_disjoint_polygons_without_support_line_splits() {
        let p = vec![[0., 0.], [1., 0.], [0., 1.]];
        let cut = vec![[1., 1.], [0.6, 1.], [1., 0.6]];
        assert!(bounds_overlap(&p, &cut, false));
        assert_eq!(subtract(p.clone(), &cut), vec![p]);
    }
    #[test]
    fn incremental_disjoint_damage_keeps_geometry_growth_linear() {
        let mut regions = Vec::new();
        for i in 0..500 {
            let x = (i % 25) as f64 * 0.003;
            let y = (i / 25) as f64 * 0.003;
            let mask = rectangle(x, y, x + 0.001, y + 0.001);
            regions = extend_damage(regions, vec![mask]);
            assert_eq!(regions.len(), i + 1);
            assert_eq!(regions.iter().map(Vec::len).sum::<usize>(), 4 * (i + 1));
        }
        assert!((regions.iter().map(|p| area(p)).sum::<f64>() - 0.0005).abs() < 1e-14);
        // Repeated already-covered damage must leave the entire canonical
        // partition unchanged, not re-cut its 500 unrelated old regions.
        let original = regions.clone();
        for _ in 0..100 {
            regions = extend_damage(regions, vec![rectangle(0.0002, 0.0002, 0.0008, 0.0008)]);
        }
        assert_eq!(regions, original);
    }
    #[test]
    fn damage_union_removes_stroke_seams_without_accumulating_area() {
        let mut masks = Vec::new();
        for i in 0..100 {
            let y = i as f64 * 0.001;
            masks.push(rectangle(-1., -1. + y, 1., y));
        }
        let mut union = canonical_damage(masks);
        assert_eq!(union.len(), 1, "overlapping straight strokes have one boundary");
        let expected = 2. * 1.099;
        assert!((area(&union[0]) - expected).abs() < 1e-13);
        for i in 0..1000 {
            let x = (i % 100) as f64 * 0.001;
            union.push(rectangle(-0.5 + x, -0.5, 0.5 + x, 0.05));
            union = canonical_damage(union);
        }
        assert_eq!(union.len(), 1);
        assert!((area(&union[0]) - expected).abs() < 1e-13);
    }
    #[test]
    fn damage_union_preserves_narrow_gaps_concavities_and_holes() {
        let gap = 1e-9;
        let separated = canonical_damage(vec![
            rectangle(-1., -1., 0., 1.),
            rectangle(gap, -1., 1., 1.),
        ]);
        assert_eq!(separated.len(), 2, "a real gap cannot be convexified away");
        let probe = rectangle(gap * 0.25, -0.5, gap * 0.75, 0.5);
        assert!(separated.iter().all(|p| area(&intersect(p.clone(), &probe)) < 1e-20));
        let corner = canonical_damage(vec![
            rectangle(0., 0., 2., 1.),
            rectangle(0., 0., 1., 2.),
        ]);
        assert!((corner.iter().map(|p| area(p)).sum::<f64>() - 3.).abs() < 1e-13);
        let absent_corner = rectangle(1.1, 1.1, 1.9, 1.9);
        assert!(corner.iter().all(|p| area(&intersect(p.clone(), &absent_corner)) == 0.));
        let hole = 0.00001;
        let ring = canonical_damage(vec![
            rectangle(-1., -1., 1., -hole),
            rectangle(-1., hole, 1., 1.),
            rectangle(-1., -hole, -hole, hole),
            rectangle(hole, -hole, 1., hole),
        ]);
        assert!((ring.iter().map(|p| area(p)).sum::<f64>() - (4. - 4. * hole * hole)).abs() < 1e-13);
        let hole_probe = rectangle(-hole * 0.5, -hole * 0.5, hole * 0.5, hole * 0.5);
        assert!(ring.iter().all(|p| area(&intersect(p.clone(), &hole_probe)) == 0.));
        for i in 0..ring.len() {
            for j in i + 1..ring.len() {
                assert!(area(&intersect(ring[i].clone(), &ring[j])) < 1e-13);
            }
        }
    }
    #[test]
    fn thin_intervening_soil_does_not_make_hidden_faces_repel() {
        let mut m = Material::new([-0.1, -0.1, -0.1], [0.1, 0.1, 0.1]);
        m.bodies = vec![
            Body::new(
                1,
                Poly::box_from([-0.1, -0.1, -0.1], [0.1, 0.1, -0.0006]),
                1600.,
                -1.,
            ),
            Body::new(
                2,
                Poly::box_from([-0.1, -0.1, -0.0006], [0.1, 0.1, 0.0006]),
                1600.,
                -1.,
            ),
            Body::new(
                3,
                Poly::box_from([-0.1, -0.1, 0.0006], [0.1, 0.1, 0.1]),
                1600.,
                -1.,
            ),
        ];
        m.rebuild_joints(true);
        assert!(
            m.exclusions().contains(&[0, 2]),
            "the buried outer faces must not repel across the middle piece"
        );
        assert!(
            m.contact_planes().iter().any(|p| p.a == 0 && p.b == 2),
            "filtered pairs must still have a non-penetration constraint"
        );
        m.bodies[2].center[2] += 0.01;
        m.bodies[2].update_vertices();
        assert!(
            !m.exclusions().contains(&[0, 2]),
            "separated pieces must return to ordinary contact"
        );
    }
    #[test]
    fn a_blocked_edge_reuses_its_existing_local_cut_planes() {
        let mut m = Material::new([-0.26, -0.24, -0.36], [0.26, 0., 0.36]);
        let edge = [[-0.115, 0.001, 0.], [0.115, 0.001, 0.]];
        let wanted = [[-0.115, -0.001, 0.], [0.115, -0.001, 0.]];
        assert!(m.cut(edge, wanted, 20., 0.0012));
        let count = m.bodies.len();
        for i in 0..30 {
            let offset = (i as f64 * 0.7).sin() * 0.0002;
            let jitter = edge.map(|p| [p[0], p[1] + offset, p[2] + offset]);
            let target = jitter.map(|p| [p[0], p[1] - 0.002, p[2]]);
            m.cut(jitter, target, 20., 0.0012);
        }
        assert_eq!(
            m.bodies.len(),
            count,
            "small motions at the same cut must not create more slivers"
        );
        assert!(m.valid());
    }
    #[test]
    fn small_accepted_strokes_accumulate_actual_cut_area() {
        let mut m = Material::new([-0.26, -0.24, -0.36], [0.26, 0., 0.36]);
        let first = [[-0.115, 0.001, 0.], [0.115, 0.001, 0.]];
        let next = [[-0.115, -0.001, 0.], [0.115, -0.001, 0.]];
        m.cut(first, next, 20., 0.0012);
        m.work_reservoir = 2.;
        for i in 0..100 {
            let y = 0.001 - i as f64 * 0.00005;
            let start = [[-0.115, y, 0.], [0.115, y, 0.]];
            let end = [[-0.115, y - 0.00005, 0.], [0.115, y - 0.00005, 0.]];
            m.fracture(start, end);
        }
        assert!(
            m.stats.cut_area > 0.0002,
            "small accepted motions through the surface must make a real cut; got {}",
            m.stats.cut_area
        );
    }
    #[test]
    fn finite_cut_removes_cohesion_area_without_removing_soil() {
        let mut m = Material::new([-0.26, -0.24, -0.36], [0.26, 0., 0.36]);
        let mass = m.mass;
        let edge = [[-0.115, 0.001, 0.], [0.115, 0.001, 0.]];
        let wanted = [[-0.115, -0.04, 0.], [0.115, -0.04, 0.]];
        assert!(m.cut(edge, wanted, 20., 0.0012));
        assert_eq!(m.stats.cuts, 0, "no energy means refinement only");
        m.connectivity();
        assert!(m.detached.is_empty());
        assert!(m.valid());
        assert!(
            m.bodies.iter().all(|b| b
                .poly
                .faces
                .iter()
                .filter(|f| f.tag >= 100)
                .all(|f| m.exposed_face(b, f).is_empty())),
            "refinement alone must not expose internal faces"
        );
        m.work_reservoir = 1.;
        m.cut(edge, wanted, 20., 0.0012);
        assert_eq!(
            m.stats.cut_area, 0.,
            "a requested stroke must not fracture soil on its own"
        );
        assert!(
            !m.fracture(edge, edge),
            "a blocked blade must not damage untouched soil"
        );
        assert!(m.fracture(edge, wanted));
        assert!(m.stats.cut_area > 0.001 && m.stats.cut_area < 0.04);
        let mut found_partial = false;
        for b in &m.bodies {
            for face in &b.poly.faces {
                let material = m.exposed_face_material(b, face);
                let rendered = m.exposed_face(b, face);
                assert_eq!(material.len(), rendered.len());
                for (reference, current) in material.iter().zip(&rendered) {
                    for (r, p) in reference.iter().zip(current) {
                        assert!(length(sub(b.point(*r), *p)) < 1e-12);
                    }
                }
                let triangle_area =
                    |tri: [D; 3]| length(cross(sub(tri[1], tri[0]), sub(tri[2], tri[0]))) * 0.5;
                let exposed_area: f64 = material.iter().map(|tri| triangle_area(*tri)).sum();
                let face_area: f64 = (1..face.nodes.len() - 1)
                    .map(|i| {
                        triangle_area(
                            [face.nodes[0], face.nodes[i], face.nodes[i + 1]]
                                .map(|n| b.poly.vertices[n].material),
                        )
                    })
                    .sum();
                found_partial |= exposed_area > 1e-9 && exposed_area < face_area * 0.9;
            }
        }
        assert!(
            found_partial,
            "the collision surface must preserve a partially damaged face"
        );
        assert!(
            m.bodies.iter().any(|b| b
                .poly
                .faces
                .iter()
                .any(|f| !m.exposed_face(b, f).is_empty())),
            "accepted fracture must expose its cut faces"
        );
        assert!(m.stats.fracture_work <= 1.);
        assert!((m.mass - mass).abs() < 1e-10);
        assert!(m.valid());
        let previous = m.stats.cut_area;
        m.work_reservoir = 1.;
        m.fracture(edge, wanted);
        assert!(
            (m.stats.cut_area - previous).abs() < 1e-10,
            "revisiting an existing cut must not damage it twice"
        );
    }
    #[test]
    fn refinement_preserves_roots_grass_and_material_after_repeated_planes() {
        let mut m = Material::new([-0.26, -0.24, -0.36], [0.26, 0., 0.36]);
        for i in 0..7 {
            m.partition(
                Plane::through(
                    [0., -0.12, 0.],
                    [0.1 + (i as f64).cos(), 0.4, (i as f64).sin()],
                ),
                [0., -0.1, 0.],
            );
            assert!(m.valid());
            assert_eq!(m.roots.len(), 12);
            assert_eq!(m.stats.roots_broken, 0);
            m.connectivity();
            assert!(
                m.detached.is_empty(),
                "pure refinement cannot excavate earth"
            );
            for point in [[-0.1, -0.01, 0.1], [0.1, -0.03, -0.1]] {
                assert!(length(sub(m.sample_material(point).unwrap().0, point)) < 1e-8);
            }
        }
    }
    #[test]
    fn cohesive_polygon_difference_accounts_exact_cut_coverage() {
        let original = vec![[-1., -1.], [1., -1.], [1., 1.], [-1., 1.]];
        let cut = vec![[-0.2, -2.], [0.2, -2.], [0.2, 0.3], [-0.2, 0.3]];
        let remaining = subtract(original.clone(), &cut);
        let removed = area(&intersect(original, &cut));
        assert!((remaining.iter().map(|p| area(p)).sum::<f64>() + removed - 4.).abs() < 1e-12);
    }
}
