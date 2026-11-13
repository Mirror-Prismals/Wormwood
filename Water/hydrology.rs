//! Real continuous terrain/soil/trough triangle queries. This module neither
//! samples old terrain code nor acquires the soil-owned native OGC context.
use super::{
    math::*,
    types::{Snapshot, Triangle},
};
#[derive(Default, Clone)]
pub(crate) struct State {
    pub snapshot: Option<Snapshot>,
    pub bodies: super::bodies::State,
    pub material_domains: Vec<crate::terrain::material_domain::Domain>,
    pub triangles: Vec<Triangle>,
    pub revision: u64,
    pub slide_revision: u64,
    pub(crate) surface_cache: Option<([u64;3], super::math::Frame, super::surface_mesh::Mesh)>,
    bins: std::collections::HashMap<[i32; 3], Vec<usize>>,
    large: Vec<usize>,
    index_origin: V,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hit {
    pub fraction: f64,
    pub point: V,
    pub normal: V,
}
pub(crate) fn closest(p: V, t: [V; 3]) -> V {
    let [a, b, c] = t;
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0. && d2 <= 0. {
        return a;
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0. && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0. && d1 >= 0. && d3 <= 0. {
        return add(a, mul(ab, d1 / (d1 - d3)));
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0. && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0. && d2 >= 0. && d6 <= 0. {
        return add(a, mul(ac, d2 / (d2 - d6)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0. && (d4 - d3) >= 0. && (d5 - d6) >= 0. {
        return add(b, mul(sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6))));
    }
    let inv = 1. / (va + vb + vc);
    add(a, add(mul(ab, vb * inv), mul(ac, vc * inv)))
}
impl State {
    pub(crate) fn free_surface_visible(&mut self,focus:V,forward:V,mean:f64,wave:&super::ocean::State,view:Option<&super::visibility::View>)->(Frame,super::surface_mesh::Mesh){
        if view.is_none(){return self.free_surface(focus,forward,mean,wave);}
        // A view-dependent mesh must never populate the world-centred cache.
        super::surface_mesh::free_surfaces_visible(focus,forward,&self.bodies,mean,wave,self,view)
    }
    pub(crate) fn free_surface(&mut self,focus:V,forward:V,mean:f64,wave:&super::ocean::State)->(Frame,super::surface_mesh::Mesh){
        // The extractor's forward parameter is deliberately unused; a camera
        // turn cannot change a world-centred river/lake mesh.
        let key=focus.map(f64::to_bits);
        // The generated eastern ocean is outside the 16 km horizon mesh
        // in inland regions. Preserve the static lake/river cache there even
        // though ocean rendering is enabled for the coast.
        
        let inland=self.bodies.generated && crate::terrain::regions::ocean_distance(unit(focus)) < -16_500.;
        let static_surface=!self.bodies.ocean || inland;
        if static_surface {
            if let Some((old,frame,mesh))=&self.surface_cache {
                if *old==key{return (*frame,mesh.clone());}
            }
        }
        let (frame,mesh)=super::surface_mesh::free_surfaces(focus,forward,&self.bodies,mean,wave,self);
        if static_surface {self.surface_cache=Some((key,frame,mesh.clone()));}
        (frame,mesh)
    }
    pub fn update(&mut self, snapshot: Snapshot, slide: &super::slides::Mesh, slide_revision: u64) {
        self.surface_cache=None;
        let mut triangles:Vec<_> = snapshot
            .triangles
            .iter()
            .map(|t| {
                Triangle::new(t.map(|i| {
                    add(
                        snapshot.origin,
                        snapshot.vertices[i as usize].map(f64::from),
                    )
                }))
            })
            .collect();
        triangles.extend(
            slide
                .triangles
                .iter()
                .map(|t| Triangle::new(t.map(|i| slide.vertices[i as usize].position))),
        );
        // Terrain is the stable prefix; moving material and slides follow it.
        // Keep its spatial memberships until an actual triangle/origin changes.
        let unchanged=if self.index_origin==snapshot.origin {
            self.triangles.iter().zip(&triangles).take_while(|(a,b)|a.p==b.p).count()
        } else {0};
        self.triangles=triangles;
        self.index_origin = snapshot.origin;
        if unchanged==0 {
            // Preserve bucket allocations across moving collision patches.
            // The local metre-space bin keys mostly repeat after origin shifts.
            for ids in self.bins.values_mut(){ids.clear();}self.large.clear();
        }
        else {
            for ids in self.bins.values_mut(){let keep=ids.partition_point(|&i|i<unchanged);ids.truncate(keep);}
            self.large.retain(|&i|i<unchanged);
        }
        for (i, t) in self.triangles.iter().enumerate().skip(unchanged) {
            let low = self.bin(t.min);
            let high = self.bin(t.max);
            let volume: i64 = (0..3)
                .map(|j| high[j] as i64 - low[j] as i64 + 1)
                .fold(1i64, |a, b| a.saturating_mul(b));
            if volume > 4096 {
                self.large.push(i);
                continue;
            }
            for z in low[2]..=high[2] {
                for y in low[1]..=high[1] {
                    for x in low[0]..=high[0] {
                        self.bins.entry([x, y, z]).or_default().push(i);
                    }
                }
            }
        }
        // Remove unused keys only after refill, so overlapping empty buckets
        // can reuse their existing storage without retaining obsolete regions.
        self.bins.retain(|_,ids|!ids.is_empty());
        self.revision = snapshot.revision;
        self.slide_revision = slide_revision;
        self.snapshot = Some(snapshot);
    }
    fn bin(&self, p: V) -> [i32; 3] {
        std::array::from_fn(|i| ((p[i] - self.index_origin[i]) / 2.).floor() as i32)
    }
    fn candidates(&self, low: V, high: V) -> Vec<usize> {
        let low = self.bin(low);
        let high = self.bin(high);
        let volume: i64 = (0..3)
            .map(|i| high[i] as i64 - low[i] as i64 + 1)
            .fold(1i64, |a, b| a.saturating_mul(b));
        if volume > 16384 {
            return (0..self.triangles.len()).collect();
        }
        let mut indices = self.large.clone();
        for z in low[2]..=high[2] {
            for y in low[1]..=high[1] {
                for x in low[0]..=high[0] {
                    if let Some(rows) = self.bins.get(&[x, y, z]) {
                        indices.extend(rows);
                    }
                }
            }
        }
        indices.sort_unstable();
        indices.dedup();
        indices
    }
    pub fn covered(&self, p: V, r: f64) -> bool {
        self.snapshot.as_ref().is_some_and(|s| s.contains(p, r))
    }
    pub fn sweep(&self, start: V, end: V, radius: f64) -> Option<Hit> {
        let motion = sub(end, start);
        let speed = len(motion);
        let mut result: Option<Hit> = None;
        let low: V = std::array::from_fn(|i| start[i].min(end[i]) - radius);
        let high: V = std::array::from_fn(|i| start[i].max(end[i]) + radius);
        for i in self.candidates(low, high) {
            let tri = &self.triangles[i];
            if (0..3).any(|i| tri.max[i] < low[i] || tri.min[i] > high[i]) {
                continue;
            }
            let mut t = 0.;
            for _ in 0..48 {
                if t > 1. || result.is_some_and(|h| t > h.fraction) {
                    break;
                }
                let p = add(start, mul(motion, t));
                let q = closest(p, tri.p);
                let delta = sub(p, q);
                let distance = len(delta);
                let n = safe_unit(
                    delta,
                    safe_unit(
                        cross(sub(tri.p[1], tri.p[0]), sub(tri.p[2], tri.p[0])),
                        unit(p),
                    ),
                );
                if distance <= radius + 1e-7 {
                    // Existing support permits tangential/retreat motion. Other
                    // triangles still participate, including the trough walls.
                    if distance >= radius - 1e-6 && dot(motion, n) >= -1e-9 {
                        break;
                    }
                    result = Some(Hit {
                        fraction: t,
                        point: add(q, mul(n, radius + 1e-6)),
                        normal: n,
                    });
                    break;
                }
                if speed < 1e-12 {
                    break;
                }
                t += ((distance - radius) / speed).max(1e-8);
            }
        }
        result
    }
    /// First actual triangle hit along a finite ray; usable under overhangs.
    pub fn ray(&self, start: V, direction: V, distance: f64) -> Option<Hit> {
        let direction = unit(direction);
        let mut best = distance;
        let mut hit = None;
        let end = add(start, mul(direction, distance));
        let low = std::array::from_fn(|i| start[i].min(end[i]));
        let high = std::array::from_fn(|i| start[i].max(end[i]));
        for i in self.candidates(low, high) {
            let t = &self.triangles[i];
            let [a, b, c] = t.p;
            let e1 = sub(b, a);
            let e2 = sub(c, a);
            let h = cross(direction, e2);
            let det = dot(e1, h);
            if det.abs() < 1e-12 {
                continue;
            }
            let s = sub(start, a);
            let u = dot(s, h) / det;
            if !(0.0..=1.).contains(&u) {
                continue;
            }
            let q = cross(s, e1);
            let v = dot(direction, q) / det;
            if v < 0. || u + v > 1. {
                continue;
            }
            let d = dot(e2, q) / det;
            if d < 0. || d > best {
                continue;
            }
            best = d;
            let mut normal = unit(cross(e1, e2));
            if dot(normal, direction) > 0. {
                normal = mul(normal, -1.);
            }
            hit = Some(Hit {
                fraction: d / distance,
                point: add(start, mul(direction, d)),
                normal,
            });
        }
        hit
    }
}

#[cfg(test)]mod free_surface_cache_tests {
 use super::*;
 #[test]fn static_water_cache_matches_extractor_and_never_freezes_ocean_time(){
  let mut state=State::default();
  state.bodies=super::super::bodies::State::parse(&serde_json::json!({"ocean":false,"generated":true,"rivers":[],"sources":[]})).unwrap();
  let focus=[0.,crate::terrain::RADIUS+8.,-52.];let forward=[0.,0.,1.];let mut wave=super::super::ocean::State::default();
  let (_,expected)=super::super::surface_mesh::free_surfaces(focus,forward,&state.bodies,0.,&wave,&state);
  assert_eq!(state.free_surface(focus,forward,0.,&wave).1,expected);
  assert!(state.surface_cache.is_some());wave.time=10.;
  assert_eq!(state.free_surface(focus,forward,100.,&wave).1,expected);
  state.bodies.ocean=true;
  let (_,expected)=super::super::surface_mesh::free_surfaces(focus,forward,&state.bodies,2.,&wave,&state);
  assert_eq!(state.free_surface(focus,forward,2.,&wave).1,expected);
 }
}
