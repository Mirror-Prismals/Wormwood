//! Deterministic regional drainage over continuous terrain samples.
//! The analysis lattice stores elevations and drainage links, not occupied blocks.
//! Water and carved banks are queried in continuous metre coordinates.
use std::{cmp::Ordering, collections::BinaryHeap, sync::OnceLock};
const SIDE: usize = 257;
const STEP: f64 = 64.;
const HALF: f64 = (SIDE as f64 - 1.) * STEP * 0.5;
const RIVER_AREA: f64 = 120_000.;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Lake,
    River,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Water {
    pub id: u64,
    pub level: f64,
    pub direction: [f64; 2],
    pub shore_distance: f64,
    pub kind: Kind,
}
#[derive(Clone, Copy)]
struct Queue {
    height: f64,
    index: usize,
}
impl PartialEq for Queue {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.height.to_bits() == other.height.to_bits()
    }
}
impl Eq for Queue {}
impl PartialOrd for Queue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Queue {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .height
            .total_cmp(&self.height)
            .then_with(|| other.index.cmp(&self.index))
    }
}
#[derive(Clone, Copy)]
struct Segment {
    a: [f64; 2],
    b: [f64; 2],
    level: [f64; 2],
    width: f64,
    depth: f64,
    id: u64,
}
pub(crate) struct Network {
    spill: Vec<f64>,
    basin: Vec<usize>,
    segments: Vec<Segment>,
    bins: Vec<Vec<usize>>,
    zero: f64,
    parent: Vec<usize>,
    area: Vec<f64>,
}
static NETWORK: OnceLock<Network> = OnceLock::new();
pub(crate) fn water_height_bounds()->[f64;2]{
    static BOUNDS:OnceLock<[f64;2]>=OnceLock::new();
    *BOUNDS.get_or_init(||{let n=network();n.spill.iter().map(|h|h-n.zero).fold([f64::INFINITY,f64::NEG_INFINITY],|b,h|[b[0].min(h),b[1].max(h)])})
}
fn network() -> &'static Network {
    NETWORK.get_or_init(Network::generate)
}
fn point(index: usize) -> [f64; 2] {
    [
        (index % SIDE) as f64 * STEP - HALF,
        (index / SIDE) as f64 * STEP - HALF,
    ]
}
fn normal(x: f64, z: f64) -> super::Point {
    let x = x / super::RADIUS;
    let z = z / super::RADIUS;
    [x, (1. - x * x - z * z).max(0.).sqrt(), z]
}
fn neighbours(index: usize) -> impl Iterator<Item = usize> {
    let x = (index % SIDE) as isize;
    let z = (index / SIDE) as isize;
    (-1..=1).flat_map(move |dz| {
        (-1..=1).filter_map(move |dx| {
            let nx = x + dx;
            let nz = z + dz;
            if (dx != 0 || dz != 0)
                && nx >= 0
                && nz >= 0
                && nx < SIDE as isize
                && nz < SIDE as isize
            {
                Some(nz as usize * SIDE + nx as usize)
            } else {
                None
            }
        })
    })
}
fn direction(a: [f64; 2]) -> [f64; 2] {
    let length = (a[0] * a[0] + a[1] * a[1]).sqrt().max(1e-12);
    a.map(|v| v / length)
}
fn smooth(t: f64) -> f64 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
impl Network {
    fn generate() -> Self {
        let base: Vec<_> = (0..SIDE * SIDE)
            .map(|i| {
                let p = point(i);
                super::base_height(normal(p[0], p[1]))
            })
            .collect();
        let (spill, parent, order) = route(&base, SIDE);
        let mut area = vec![STEP * STEP; SIDE * SIDE];
        let mut main_child = vec![usize::MAX; SIDE * SIDE];
        for &i in order.iter().rev() {
            let p = parent[i];
            if p != i {
                area[p] += area[i];
                if main_child[p] == usize::MAX || area[i] > area[main_child[p]] {
                    main_child[p] = i;
                }
            }
        }
        let mut basin = vec![0; SIDE * SIDE];
        for i in 0..basin.len() {
            if basin[i] != 0 || spill[i] - base[i] < 0.35 {
                continue;
            }
            let id = i + 1;
            let mut todo = vec![i];
            basin[i] = id;
            while let Some(j) = todo.pop() {
                for k in neighbours(j) {
                    if basin[k] == 0
                        && spill[k] - base[k] >= 0.35
                        && (spill[k] - spill[j]).abs() < 1e-7
                    {
                        basin[k] = id;
                        todo.push(k);
                    }
                }
            }
        }
        let mut result = Self {
            spill,
            basin,
            segments: Vec::new(),
            bins: vec![Vec::new(); SIDE * SIDE],
            zero: 0.,
            parent,
            area,
        };
        for i in 0..SIDE * SIDE {
            let p = result.parent[i];
            if p == i || result.area[i] < RIVER_AREA || result.basin[i] != 0 {
                continue;
            }
            let a = point(i);
            let b = point(p);
            let before = if main_child[i] != usize::MAX {
                point(main_child[i])
            } else {
                a
            };
            let after = point(result.parent[p]);
            let tangent_a = direction([b[0] - before[0], b[1] - before[1]]);
            let tangent_b = direction([after[0] - a[0], after[1] - a[1]]);
            let length = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
            let controls = [
                a,
                [
                    a[0] + tangent_a[0] * length / 3.,
                    a[1] + tangent_a[1] * length / 3.,
                ],
                [
                    b[0] - tangent_b[0] * length / 3.,
                    b[1] - tangent_b[1] * length / 3.,
                ],
                b,
            ];
            let curve = |t: f64| {
                std::array::from_fn(|axis| {
                    let u = 1. - t;
                    u * u * u * controls[0][axis]
                        + 3. * u * u * t * controls[1][axis]
                        + 3. * u * t * t * controls[2][axis]
                        + t * t * t * controls[3][axis]
                })
            };
            let width = (2.5 + (result.area[i] / RIVER_AREA).sqrt() * 1.5).clamp(4., 28.);
            for part in 0..8 {
                let t0 = part as f64 / 8.;
                let t1 = (part + 1) as f64 / 8.;
                let level = |t: f64| result.spill[i] * (1. - t) + result.spill[p] * t;
                result.segments.push(Segment {
                    a: curve(t0),
                    b: curve(t1),
                    level: [level(t0), level(t1)],
                    width,
                    depth: (0.6 + width * 0.09).min(3.5),
                    id: i as u64 + 1,
                });
            }
        }
        for (index, s) in result.segments.iter().enumerate() {
            let radius = s.width * 1.5;
            let min = [s.a[0].min(s.b[0]) - radius, s.a[1].min(s.b[1]) - radius];
            let max = [s.a[0].max(s.b[0]) + radius, s.a[1].max(s.b[1]) + radius];
            let low = Self::bin(min);
            let high = Self::bin(max);
            for z in low[1]..=high[1] {
                for x in low[0]..=high[0] {
                    result.bins[z * SIDE + x].push(index);
                }
            }
        }
        result.zero = result.carve(0., 0., 0.);
        result
    }
    fn bin(p: [f64; 2]) -> [usize; 2] {
        p.map(|v| ((v + HALF) / STEP).floor().clamp(0., (SIDE - 1) as f64) as usize)
    }
    fn covered(x: f64, z: f64) -> bool {
        x.abs() < HALF - STEP && z.abs() < HALF - STEP
    }
    fn nearby(&self, x: f64, z: f64) -> impl Iterator<Item = &Segment> {
        let p = Self::bin([x, z]);
        self.bins[p[1] * SIDE + p[0]]
            .iter()
            .map(|i| &self.segments[*i])
    }
    fn closest(s: &Segment, x: f64, z: f64) -> (f64, f64) {
        let d = [s.b[0] - s.a[0], s.b[1] - s.a[1]];
        let t = (((x - s.a[0]) * d[0] + (z - s.a[1]) * d[1])
            / (d[0] * d[0] + d[1] * d[1]).max(1e-12))
        .clamp(0., 1.);
        let distance = ((x - s.a[0] - d[0] * t).powi(2) + (z - s.a[1] - d[1] * t).powi(2)).sqrt();
        (distance, t)
    }
    fn carve(&self, x: f64, z: f64, base: f64) -> f64 {
        if !Self::covered(x, z) {
            return base;
        }
        let mut bed = base;
        for s in self.nearby(x, z) {
            let (distance, t) = Self::closest(s, x, z);
            let radius = s.width * 0.5;
            let level = s.level[0] * (1. - t) + s.level[1] * t;
            let target = if distance <= radius {
                level - s.depth * (1. - (distance / radius).powi(2))
            } else {
                let blend = smooth((distance - radius) / s.width);
                level * (1. - blend) + base * blend
            };
            bed = bed.min(target);
        }
        bed
    }
    fn water(&self, x: f64, z: f64, include_dry: bool) -> Option<Water> {
        if !Self::covered(x, z) {
            return None;
        }
        let bin = Self::bin([x, z]);
        let mut lake = None;
        for dz in 0..=1 {
            for dx in 0..=1 {
                let i = (bin[1] + dz).min(SIDE - 1) * SIDE + (bin[0] + dx).min(SIDE - 1);
                if self.basin[i] != 0 {
                    let p = point(i);
                    let distance = (p[0] - x).powi(2) + (p[1] - z).powi(2);
                    if lake.is_none_or(|(_, best, _)| distance < best) {
                        lake = Some((self.basin[i], distance, self.spill[i]));
                    }
                }
            }
        }
        if let Some((id, _, level)) = lake {
            if include_dry || super::base_height(normal(x, z)) < level - 0.01 {
                return Some(Water {
                    id: id as u64,
                    level: level - self.zero,
                    direction: [0.; 2],
                    shore_distance: 0.,
                    kind: Kind::Lake,
                });
            }
        }
        self.nearby(x, z)
            .filter_map(|s| {
                let (distance, t) = Self::closest(s, x, z);
                if distance >= s.width * 0.5 + if include_dry { 2. } else { 0. } {
                    return None;
                }
                Some((
                    distance,
                    Water {
                        id: s.id,
                        level: s.level[0] * (1. - t) + s.level[1] * t - self.zero,
                        direction: direction([s.b[0] - s.a[0], s.b[1] - s.a[1]]),
                        shore_distance: s.width * 0.5 - distance,
                        kind: Kind::River,
                    },
                ))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, water)| water)
    }
}
/// Priority flood gives each sample a minimum spill elevation and an acyclic
/// downstream parent. Reverse settlement order accumulates the whole catchment.
fn route(base: &[f64], side: usize) -> (Vec<f64>, Vec<usize>, Vec<usize>) {
    let mut spill = base.to_vec();
    let mut parent = (0..base.len()).collect::<Vec<_>>();
    let mut seen = vec![false; base.len()];
    let mut heap = BinaryHeap::new();
    for z in 0..side {
        for x in 0..side {
            if x == 0 || z == 0 || x + 1 == side || z + 1 == side {
                let i = z * side + x;
                seen[i] = true;
                heap.push(Queue {
                    height: base[i],
                    index: i,
                });
            }
        }
    }
    let mut order = Vec::with_capacity(base.len());
    while let Some(Queue { height, index }) = heap.pop() {
        order.push(index);
        let x = index % side;
        let z = index / side;
        for dz in -1..=1 {
            for dx in -1..=1 {
                let nx = x as isize + dx;
                let nz = z as isize + dz;
                if nx < 0 || nz < 0 || nx >= side as isize || nz >= side as isize {
                    continue;
                }
                let j = nz as usize * side + nx as usize;
                if seen[j] {
                    continue;
                }
                seen[j] = true;
                spill[j] = base[j].max(height);
                parent[j] = index;
                heap.push(Queue {
                    height: spill[j],
                    index: j,
                });
            }
        }
    }
    (spill, parent, order)
}
/// A dry-bank viewpoint aimed across the nearest generated river segment.
pub(crate) fn camera_bank()->([f64;2],f64) {
    let s=network().segments.iter().min_by(|a,b| {
        let distance=|s:&Segment| ((s.a[0]+s.b[0])*0.5).powi(2)+((s.a[1]+s.b[1])*0.5).powi(2);
        distance(a).total_cmp(&distance(b))
    }).expect("generated reference drainage contains river segments");
    let d=direction([s.b[0]-s.a[0],s.b[1]-s.a[1]]);
    let across=[-d[1],d[0]];let offset=s.width*0.5+12.;
    ([(s.a[0]+s.b[0])*0.5+across[0]*offset,(s.a[1]+s.b[1])*0.5+across[1]*offset],(-across[1]).atan2(-across[0]).to_degrees())
}
pub(crate) fn height(normal: super::Point, base: f64) -> f64 {
    let n = super::unit(normal);
    if super::regions::southern(n).is_some() || super::regions::sandy(n) || super::regions::polar(n) { return base; }
    let n0 = network();
    if n[1] < 0.95 {
        return base - n0.zero;
    }
    n0.carve(n[0] * super::RADIUS, n[2] * super::RADIUS, base) - n0.zero
}
pub(crate) fn water(normal: super::Point) -> Option<Water> {
    let n = super::unit(normal);
    if n[1] < 0.95 {
        return None;
    }
    network().water(n[0] * super::RADIUS, n[2] * super::RADIUS, false)
}
pub(crate) fn water_candidate(normal: super::Point) -> Option<Water> {
    let n = super::unit(normal);
    if n[1] < 0.95 {
        return None;
    }
    network().water(n[0] * super::RADIUS, n[2] * super::RADIUS, true)
}
pub(crate) fn ocean_connected(normal: super::Point, level: f64) -> Option<bool> {
    let n = super::unit(normal);
    let x = n[0] * super::RADIUS;
    let z = n[2] * super::RADIUS;
    if super::regions::ocean(n) { return Some(true); }
    // The newly enabled sea belongs to the eastern coast, not low plains.
    if n[1] > 0.95 { return Some(false); }
    if n[1] < 0.95 || !Network::covered(x, z) {
        return None;
    }
    let b = Network::bin([x, z]);
    let i = b[1] * SIDE + b[0];
    Some(network().spill[i] - network().zero <= level)
}
pub(crate) fn report() -> serde_json::Value {
    let n = network();
    let basins = n
        .basin
        .iter()
        .filter(|id| **id != 0)
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    serde_json::json!({"algorithm":"priority_flood_catchments","analysis_spacing_metres":STEP,"region_half_extent_metres":HALF,"lake_basins":basins.len(),"river_segments":n.segments.len(),"river_catchment_threshold_square_metres":RIVER_AREA,"closed_basin_levels":"spill_elevation","dynamic_mass_simulation":false})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn basin_spills_through_lowest_outlet_and_parents_never_cycle() {
        let mut heights = vec![9.; 25];
        for z in 1..4 {
            for x in 1..4 {
                heights[z * 5 + x] = 1.;
            }
        }
        heights[2] = 4.;
        let (spill, parent, order) = route(&heights, 5);
        assert_eq!(spill[12], 4.);
        assert_eq!(order.len(), 25);
        for start in 0..25 {
            let mut i = start;
            for step in 0..=25 {
                if parent[i] == i {
                    break;
                }
                assert!(step < 25, "drainage cycle");
                assert!(spill[parent[i]] <= spill[i]);
                i = parent[i];
            }
        }
    }
    #[test]
    fn generated_channels_share_ground_and_keep_the_spawn_reference() {
        assert!(super::super::height([0., 1., 0.]).abs() < 1e-9);
        let n = network();
        assert!(!n.segments.is_empty());
        for segment in n.segments.iter().step_by(137) {
            assert!(segment.level[1] <= segment.level[0] + 1e-9);
            let x = (segment.a[0] + segment.b[0]) * 0.5;
            let z = (segment.a[1] + segment.b[1]) * 0.5;
            let normal = normal(x, z);
            if let Some(water) = water(normal) {
                assert!(super::super::height(normal) <= water.level + 0.01);
            }
        }
    }
}
