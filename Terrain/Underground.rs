//! Continuous cave voids beneath the shared radial surface. Numerical mesh
//! samples are not world cells, and never determine player-visible block shapes.
#[path="VoidIndex.rs"]
mod void_index;
use crate::audio::{V, add, cross, dot, length, mul, sub, unit, vector};
use serde_json::Value;
pub(crate) type Vertex = [f64; 6];
pub(crate) type Triangle = [Vertex; 3];
#[derive(Clone)]
struct Tunnel {
    start: V,
    end: V,
    radius: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depths_mesh_window_encloses_the_main_chamber_walls() {
        let data:Value=serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        let cave=Field::parse(&data).unwrap().unwrap();let center=world([-51.,-44.,16.]);
        assert!(cave.patch_half_extent(center)>28.);
        let mesh=cave.mesh(center,&[]).unwrap();
        for axis in [0,2] {
            assert!(mesh.iter().flatten().any(|v|v[axis]<center[axis]-24.));
            assert!(mesh.iter().flatten().any(|v|v[axis]>center[axis]+24.));
        }
    }

    #[test]
    fn expansive_upper_perlin_caves_extend_beyond_the_old_local_prototype() {
        let data:Value=serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        let cave=Field::parse(&data).unwrap().unwrap();
        assert!(cave.organic.unwrap().legacy_perlin);
        let mut air=0;let mut solid=0;let mut example=None;
        for x in (160..640).step_by(12) {for z in (160..640).step_by(12) {
            let normal=unit(world([x as f64,0.,z as f64]));let h=super::super::height(normal);
            let y=h-18.;if y<=-18. {continue;}
            let p=world([x as f64,y,z as f64]);let d=cave.distance(p,h);
            if d>0.5 {air+=1;example=Some(p);} else if d< -0.5 {solid+=1;}
            let roof=world([x as f64,h-1.,z as f64]);
            assert!(cave.distance(roof,h)<0.,"upper Perlin field must preserve the surface roof");
        }}
        assert!(air>10 && solid>10,"expansive field needs chambers and enclosing rock: {air}/{solid}");
        let point=example.unwrap();assert!(cave.affected(point,0.));
        let mesh=cave.mesh(point,&[]).unwrap();assert!(!mesh.is_empty() && mesh.len()<=10000);
        // The original author-provided route continues to join the blue chamber.
        for t in &cave.tunnels {for i in 0..=16 {
            let p=world(add(t.start,mul(sub(t.end,t.start),i as f64/16.)));
            assert!(cave.distance(p,super::super::height(unit(p)))>0.);
        }}
    }

    #[test]
    fn cardinal_depths_extend_beyond_the_authored_cave_and_carve_rivers() {
        let data: Value = serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        let cave = Field::parse(&data).unwrap().unwrap();
        let mut found = None;
        for x in (400..800).step_by(4) {
            for z in (400..800).step_by(4) {
                if let Some(r) = super::super::depths::river(x as f64, z as f64) {
                    let p = world([x as f64, (r.water+r.ceiling)*0.5, z as f64]);
                    let height = super::super::height(unit(p));
                    if length(p)-super::super::RADIUS < height-2. {
                        found=Some((p,r,height)); break;
                    }
                }
            }
            if found.is_some() { break; }
        }
        let (p,r,h)=found.expect("river below ground away from the authored cave");
        assert!(cave.affected(p,0.));
        assert!(cave.distance(p,h)>0., "river must carve open space");
        let local=local(p);
        let below=world([local[0],r.bottom-2.,local[2]]);
        assert!(cave.mesh(p,&[]).unwrap().len() <= 10_000);
        assert!(cave.distance(below,super::super::height(unit(below))).is_finite());
    }
    #[test]
    fn authored_cave_windows_fit_triangle_budget() {
        let data: Value = serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        let cave = Field::parse(&data).unwrap().unwrap();
        for point in [[12., 2., -8.], [20., -8., -13.], [-51., -44., 16.], [-20., -48., -15.], [-75., -52., 35.]] {
            let mesh = cave.mesh(world(point), &[]).unwrap();
            assert!(!mesh.is_empty());
            assert!(mesh.len() <= 10_000);
        }
    }

    #[test]
    fn experimental_surface_net_samples_authored_cave_field() {
        let data: Value = serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        let cave = Field::parse(&data).unwrap().unwrap();
        let center = world([-51., -44., 16.]);
        let c = window(center).map(|n| n as f64 * 4.);
        let origin = [c[0] - 18., -78., c[2] - 18.];
        let mesh = super::super::surface_net::extract(origin, [12, 32, 12], 3., |local| {
            let p = world(local);
            cave.distance(p, super::super::height(unit(p)))
        }).unwrap();
        let tetra = cave.mesh_at_spacing(center, &[], 12, 32, 3.).unwrap();
        assert!(!mesh.triangles.is_empty() && !tetra.is_empty());
        assert!(mesh.triangles.len() < tetra.len(), "surface net {} tetrahedra {}", mesh.triangles.len(), tetra.len());
        eprintln!("cave chamber at 3 m: surface net {} triangles, tetrahedra {} triangles", mesh.triangles.len(), tetra.len());
    }

    #[test]
    fn selectable_surface_net_cave_mesh_stays_in_patch_and_near_collision_field() {
        let mut data: Value = serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        data["meshing"] = serde_json::json!("surface_net");
        let cave=Field::parse(&data).unwrap().unwrap();
        for center in [[12.,2.,-8.],[-51.,-44.,16.],[-75.,-52.,35.]] {
            let point=world(center);
            let c=window(point).map(|n|n as f64*4.);
            let triangles=cave.mesh(point,&[]).unwrap();
            assert!(!triangles.is_empty() && triangles.len()<=10_000);
            for t in &triangles { for v in t {
                let p=[v[0],v[1],v[2]];
                let half=cave.patch_half_extent(point);
                assert!(p[0]>=c[0]-half-1e-7 && p[0]<=c[0]+half+1e-7 && p[2]>=c[2]-half-1e-7 && p[2]<=c[2]+half+1e-7);
                assert!(cave.distance(p, super::super::height(unit(p))).abs()<4.5,"mesh residual at {:?}: {}",local(p),cave.distance(p,super::super::height(unit(p))));
            } }
        }
    }

    #[test]
    fn bounded_organic_caves_share_collision_and_surface_net_field() {
        let mut data: Value = serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        data["meshing"] = serde_json::json!("surface_net");
        data["organic_noise"] = serde_json::json!({"enabled":true,"center":[-25.,-12.],"radius":80.,"min_depth":5.,"max_depth":36.});
        let cave=Field::parse(&data).unwrap().unwrap();
        let mut found=None;
        for x in (-80..=20).step_by(4) { for z in (-60..=35).step_by(4) {
            let normal=unit(world([x as f64,0.,z as f64]));
            let h=super::super::height(normal);
            let p=world([x as f64,h-20.,z as f64]);
            if cave.void_distance(p)>1. && cave.distance(p,h)>0.1 { found=Some(p); break; }
        } if found.is_some(){break;} }
        let point=found.expect("organic field must create air beyond the authored tunnels");
        assert!(cave.affected(point,0.));
        let mesh=cave.mesh(point,&[]).unwrap();
        assert!(!mesh.is_empty() && mesh.len()<10_000);
        for t in &mesh { for v in t { let p=[v[0],v[1],v[2]]; assert!(cave.distance(p,super::super::height(unit(p))).abs()<4.5,"mesh residual at {:?}: {}",local(p),cave.distance(p,super::super::height(unit(p)))); } }
    }

    #[test]
    fn organic_air_overlaps_existing_cave_route() {
        let data: Value = serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        let cave=Field::parse(&data).unwrap().unwrap();
        let mut overlap=None;
        for x in (-80..=40).step_by(2) { for z in (-70..=40).step_by(2) { for depth in (6..=34).step_by(2) {
            let n=unit(world([x as f64,0.,z as f64])); let h=super::super::height(n);
            let local=[x as f64,h-depth as f64,z as f64]; let p=world(local);
            let inside_bounds = (local[0]+25.).hypot(local[2]+12.) < 79.;
            if inside_bounds && cave.void_distance(p) < -1. && super::super::organic_cave_prototype::air_score(local,depth as f64)>0.05 {
                overlap=Some(local); break;
            }
        } if overlap.is_some(){break;} } if overlap.is_some(){break;} }
        assert!(overlap.is_some(),"organic air must meet an existing tunnel or chamber");
    }

    #[test]
    fn cave_surface_stays_world_anchored_across_window_shifts() {
        let data: Value = serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        let cave=Field::parse(&data).unwrap().unwrap();
        let a=world([12.,-8.,-8.]); let b=world([16.,-8.,-8.]);
        let mesh_a=cave.mesh(a,&[]).unwrap(); let mesh_b=cave.mesh(b,&[]).unwrap();
        let key=|v:&Vertex| [(v[0]*100000.).round() as i64,(v[1]*100000.).round() as i64,(v[2]*100000.).round() as i64];
        let interior=|v:&Vertex| v[0]>0. && v[0]<28. && v[2]>-24. && v[2]<8.;
        let vertices_a:std::collections::BTreeSet<_>=mesh_a.iter().flatten().filter(|v|interior(v)).map(key).collect();
        let vertices_b:std::collections::BTreeSet<_>=mesh_b.iter().flatten().filter(|v|interior(v)).map(key).collect();
        assert!(!vertices_a.is_empty());
        assert_eq!(vertices_a,vertices_b,"overlapping cave windows must sample the same world-anchored field");
        assert_ne!(mesh_window(world([12.,-8.,-8.])),mesh_window(world([12.,-40.,-8.])));
    }

    #[test]
    fn cave_patch_exposes_boundary_samples_for_surface_join() {
        let data: Value=serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        let cave=Field::parse(&data).unwrap().unwrap();
        let point=world([12.,-8.,-8.]);
        let center=window(point).map(|n|n as f64*4.);
        let mesh=cave.mesh(point,&[]).unwrap();
        let mut near_ground=0usize; let mut maximum_gap=0f64;
        for v in mesh.iter().flatten() {
            if (v[0]-(center[0]-18.)).abs()>1e-6 && (v[0]-(center[0]+18.)).abs()>1e-6 && (v[2]-(center[2]-18.)).abs()>1e-6 && (v[2]-(center[2]+18.)).abs()>1e-6 {continue;}
            let p=[v[0],v[1],v[2]]; let h=super::super::height(unit(p));
            let gap=(length(p)-super::super::RADIUS-h).abs();
            let surface=world([p[0],h,p[2]]);
            if gap<0.5 && cave.distance(surface,h)<=1e-6 {near_ground+=1;maximum_gap=maximum_gap.max(gap);}
        }
        assert!(near_ground>0);
        assert!(maximum_gap<1e-5,"solid cave-boundary surface should meet the analytic outdoor height: {maximum_gap}");
        eprintln!("cave patch boundary: {near_ground} ground-adjacent vertices, maximum analytic height difference {maximum_gap:.3} m");
    }
}
#[derive(Clone)]
struct Chamber {
    center: V,
    radii: V,
}
#[derive(Clone)]
pub(crate) struct Field {
    pub(crate) formations: Vec<super::cave_formations::Instance>,
    formation_contacts:std::sync::Arc<super::cave_formations::ContactIndex>,
    tunnels: Vec<Tunnel>,
    chambers: Vec<Chamber>,
    organic: Option<OrganicBounds>,
    void_index: std::sync::Arc<void_index::VoidIndex>,
    source: Value,
    domains:Vec<super::material_domain::Domain>,
}
#[derive(Clone, Copy)]
struct OrganicBounds { center: [f64; 2], radius: f64, min_depth: f64, max_depth: f64, legacy_perlin: bool }
pub(crate) fn local(point: V) -> V {
    [point[0], length(point) - super::RADIUS, point[2]]
}
pub(crate) fn world(point: V) -> V {
    [
        point[0],
        ((super::RADIUS + point[1]).powi(2) - point[0].powi(2) - point[2].powi(2))
            .max(0.)
            .sqrt(),
        point[2],
    ]
}
/// Deep views cannot see the exterior height-field through the finite cave mesh.
pub(crate) fn deep_view(point:V)->bool {
    local(point)[1]< -18.
        && super::height(unit(point))-local(point)[1]>18.
}
/// Outdoor-light transmission below the surface roof. Half a metre of cover
/// fully block the sky; the shallow transition avoids an altitude threshold pop.
pub(crate) fn sky_exposure(point:V)->f64 {
    let cover=super::height(unit(point))-local(point)[1];
    let t=((cover-0.05)/0.45).clamp(0.,1.);
    1.-t*t*(3.-2.*t)
}
pub(crate) fn window(point: V) -> [i64; 3] {
    local(point).map(|x| (x / 4.).floor() as i64)
}
/// Cave mesh footprint only moves horizontally; its vertical extent is fixed.
pub(crate) fn mesh_window(point: V) -> [i64; 3] {
    let w = window(point);
    [w[0], w[2], if local(point)[1] < -18. {1} else {0}]
}
fn capsule(p: V, a: V, b: V, r: f64) -> f64 {
    let d = sub(b, a);
    let t = (dot(sub(p, a), d) / dot(d, d).max(1e-12)).clamp(0., 1.);
    length(sub(p, add(a, mul(d, t)))) - r
}
#[derive(Default)]
struct SampleCache {source:Value,edits:Vec<Value>,values:std::collections::HashMap<[u64;3],f64>}
thread_local!{static CAVE_SAMPLES:std::cell::RefCell<SampleCache>=std::cell::RefCell::new(SampleCache::default());}
impl Field {
    fn organic_bounds(&self) -> Option<OrganicBounds> {
        self.organic
    }
    pub(crate) fn parse(value: &Value) -> Result<Option<Self>, String> {
        if value.is_null() {
            return Ok(None);
        }
        if value["schema_version"] != 1 || !value["enabled"].is_boolean() {
            return Err("unsupported cave profile".into());
        }
        if value["enabled"] == false {
            return Ok(None);
        }
        if !value["meshing"].is_null() && !matches!(value["meshing"].as_str(),Some("surface_net" | "marching_tetrahedra")) {
            return Err("unsupported cave meshing method".into());
        }
        let points = |key: &str| {
            value[key]
                .as_array()
                .filter(|v| v.len() <= 4096)
                .ok_or_else(|| format!("bounded cave {key} required"))
        };
        let mut tunnels = Vec::new();
        for row in points("tunnels")? {
            let start = vector(&row["start"])?;
            let end = vector(&row["end"])?;
            let radius = row["radius"]
                .as_f64()
                .filter(|r| r.is_finite() && (1.0..=20.0).contains(r))
                .ok_or("cave radius must be1..20 metres")?;
            if length(sub(end, start)) < 0.01
                || start.iter().chain(end.iter()).any(|x| x.abs() > 2048.)
            {
                return Err("invalid cave tunnel".into());
            }
            tunnels.push(Tunnel { start, end, radius });
        }
        let mut chambers = Vec::new();
        for row in points("chambers")? {
            let center = vector(&row["center"])?;
            let radii = vector(&row["radii"])?;
            if center.iter().any(|x| x.abs() > 2048.)
                || radii.iter().any(|r| !(2.0..=96.0).contains(r))
            {
                return Err("invalid cave chamber".into());
            }
            chambers.push(Chamber { center, radii });
        }
        if tunnels.is_empty() || chambers.is_empty() {
            return Err("connected cave profile requires tunnels and chambers".into());
        }
        let organic=&value["organic_noise"];
        if !organic.is_null() {
            let valid=organic["enabled"].is_boolean()
                && organic["center"].as_array().is_some_and(|a| a.len()==2 && a.iter().all(|v|v.as_f64().is_some_and(|n|n.is_finite() && n.abs()<=1024.)))
                && organic["radius"].as_f64().is_some_and(|n| n.is_finite() && (5. ..=2048.).contains(&n))
                && organic["min_depth"].as_f64().is_some_and(|n| n.is_finite() && (0. ..=80.).contains(&n))
                && organic["max_depth"].as_f64().is_some_and(|n| n.is_finite() && (0. ..=80.).contains(&n))
                && organic["min_depth"].as_f64() < organic["max_depth"].as_f64();
            if !valid || (!organic["field"].is_null() && !matches!(organic["field"].as_str(), Some("legacy_perlin" | "value_noise"))) { return Err("invalid bounded organic cave profile".into()); }
        }
        let organic = if organic["enabled"] == true {
            Some(OrganicBounds { center:[organic["center"][0].as_f64().unwrap(),organic["center"][1].as_f64().unwrap()], radius:organic["radius"].as_f64().unwrap(), min_depth:organic["min_depth"].as_f64().unwrap(), max_depth:organic["max_depth"].as_f64().unwrap(), legacy_perlin:organic["field"] == "legacy_perlin" })
        } else { None };
        let mut field=Self {
            formations:Vec::new(),
            formation_contacts:std::sync::Arc::new(super::cave_formations::ContactIndex::build(&[])),
            void_index:std::sync::Arc::new(void_index::VoidIndex::build(&tunnels,&chambers)),
            tunnels,
            chambers,
            organic,
            source: value.clone(),
            domains:super::material_domain::from_edits(value["material_domains"].as_array().map(Vec::as_slice).unwrap_or(&[])),
        };
        if value["formations"]["enabled"]==true {
            let f=&value["formations"];
            if !f["spacing_metres"].as_f64().is_some_and(|v|v.is_finite() && (6.0..=24.0).contains(&v))
                || !f["max_instances"].as_u64().is_some_and(|n|(6..=240).contains(&n)) {return Err("invalid bounded cave formations".into());}
            field.formations=super::cave_formations::place(&field,value);
            field.formation_contacts=std::sync::Arc::new(super::cave_formations::ContactIndex::build(&field.formations));
        }
        Ok(Some(field))
    }
    pub(crate) fn set_material_domains(&mut self, edits:&[Value]) {self.domains=super::material_domain::from_edits(edits);}
    pub(crate) fn snapshot(&self) -> Value {
        self.source.clone()
    }
    pub(crate) fn affected(&self, point: V, margin: f64) -> bool {
        if super::nightworld::active(point){return true;}
        if self.domains.iter().any(|d|d.distance(point)<margin) || length(point)-super::RADIUS < super::BEDROCK_ELEVATION+margin {return true;}
        if point[1] <= 0. {
            return false;
        }
        let p = local(point);
        p[1] < -18. || self.organic_bounds().is_some_and(|b| (p[0]-b.center[0]).hypot(p[2]-b.center[1]) < b.radius+margin) || self.tunnels.iter().any(|t| {
            capsule(
                [p[0], 0., p[2]],
                [t.start[0], 0., t.start[2]],
                [t.end[0], 0., t.end[2]],
                t.radius,
            ) < margin
        }) || self.chambers.iter().any(|c| {
            ((p[0] - c.center[0]).powi(2) + (p[2] - c.center[2]).powi(2)).sqrt()
                < c.radii[0].max(c.radii[2]) + margin
        })
    }
    pub(crate) fn void_distance(&self, point: V) -> f64 {
        if point[1] <= 0. {
            return f64::INFINITY;
        }
        self.void_index.distance(local(point),&self.tunnels,&self.chambers)
    }
    #[cfg(test)]
    fn void_distance_reference(&self,point:V)->f64 {
        if point[1]<=0.{return f64::INFINITY;}
        let p = local(point);
        let mut distance = f64::INFINITY;
        for t in &self.tunnels {
            distance = distance.min(capsule(p, t.start, t.end, t.radius));
        }
        for c in &self.chambers {
            let delta = sub(p, c.center);
            let k0 = length(std::array::from_fn(|i| delta[i] / c.radii[i]));
            let k1 = length(std::array::from_fn(|i| delta[i] / c.radii[i].powi(2)));
            let d = if k1 > 1e-12 {
                k0 * (k0 - 1.) / k1
            } else {
                -c.radii.iter().copied().fold(f64::INFINITY, f64::min)
            };
            distance = distance.min(d);
        }
        distance
    }
    /// Positive is air, negative is material. Ground height includes accepted edits.
    pub(crate) fn distance(&self, point: V, height: f64) -> f64 {
        super::material_domain::carve_bedrock(point,self.formation_contacts.distance(&self.formations,point,self.geological_distance(point,height)),&self.domains)
    }
    pub(crate) fn geological_distance(&self, point:V,height:f64)->f64 {
        if super::nightworld::active(point){return super::nightworld::distance(point,height);}
        let mut base=(length(point) - super::RADIUS - height).max(-self.void_distance(point));
        let p=local(point);
        // Bound the SAME capped fields before evaluating expensive noise. A
        // term whose upper bound cannot exceed base cannot change the max.
        let cave_cap=(-20.-p[1]).min(p[1]+64.);
        if cave_cap>base {
            let cave=(super::depths::cave_score(p)*32.).min(-20.-p[1]).min(p[1]+64.);
            base=base.max(cave);
        }
        // Depths::river caps every ceiling at -21 m. This is a geometric
        // rejection, not a reduced sample budget or a changed cave boundary.
        if -21.-p[1]>base {
            let river=super::depths::river(p[0],p[2]).map_or(f64::NEG_INFINITY, |r|
                (p[1]-r.bottom).min(r.ceiling-p[1]).min(r.shore));
            base=base.max(river);
        }
        let Some(b)=self.organic_bounds() else {return base};
        let depth=height-p[1];
        let radial=b.radius-(p[0]-b.center[0]).hypot(p[2]-b.center[1]);
        let cap=radial.min(depth-b.min_depth).min(b.max_depth-depth)
            .min(if b.legacy_perlin {p[1]+24.} else {f64::INFINITY});
        if cap<=base {return base;}
        let score=if b.legacy_perlin {super::depths::upper_score(p,depth)} else {super::organic_cave_prototype::air_score(p,depth)};
        let organic=(score*32.)
            .min(radial).min(depth-b.min_depth).min(b.max_depth-depth)
            .min(if b.legacy_perlin {p[1]+24.} else {f64::INFINITY});
        base.max(organic)
    }
    pub(crate) fn normal(&self, point: V, height: &impl Fn(V) -> f64) -> V {
        unit(std::array::from_fn(|axis| {
            let mut a = point;
            let mut b = point;
            a[axis] += 0.025;
            b[axis] -= 0.025;
            self.distance(a, height(unit(a))) - self.distance(b, height(unit(b)))
        }))
    }
    pub(crate) fn support(&self,point:V,eye_height:f64,height:&impl Fn(V)->f64)->Option<f64>{
        if !self.affected(point,4.){return None;}
        let up=unit(point);let start=add(point,mul(up,-eye_height+0.45));
        if self.distance(start,height(unit(start)))<=0.{return None;}
        let mut previous=0.;
        for i in 1..=640 {
            let distance=i as f64*0.2;let p=add(start,mul(up,-distance));
            if self.distance(p,height(unit(p)))<=0.{
                let(mut a,mut b)=(previous,distance);
                for _ in 0..18 {let m=(a+b)*0.5;let p=add(start,mul(up,-m));if self.distance(p,height(unit(p)))>0.{a=m;}else{b=m;}}
                return Some(length(add(start,mul(up,-(a+b)*0.5)))-super::RADIUS);
            }
            previous=distance;
        }
        None
    }
    pub(crate) fn mesh(&self, center: V, edits: &[Value]) -> Result<Vec<Triangle>, String> {
        if super::nightworld::active(center){return super::nightworld::mesh(self,center,edits);}
        if !self.affected(center, 32.) {return Ok(Vec::new());}
        let mut triangles=if self.source["meshing"] == "surface_net" {
            self.surface_net_mesh(center, edits)?
        } else {
            self.mesh_at_spacing(center, edits, 18, 48, 2.)
                .or_else(|error| if error == "local cave triangle budget exceeded" {
                    self.mesh_at_spacing(center, edits, 12, 32, 3.)
                } else {Err(error)})?
        };
        // Fine formations retain the offline Blender silhouette rather than
        // collapsing into the three-metre geological extraction grid.
        super::cave_formations::append_mesh(&self.formations,center,self.patch_half_extent(center),&mut triangles);
        if triangles.len()>16000 {return Err("cave and formation triangle budget exceeded".into());}
        Ok(triangles)
    }
    /// Experimental mesh of the same geological field used for contact.
    /// Sample one cell beyond the horizontal patch before clipping, so the
    /// contour reaches the patch boundary rather than ending a cell short.
    fn patch_half_extent(&self, center:V)->f64 {
        if self.source["meshing"]=="surface_net" && local(center)[1]< -18. {36.} else {18.}
    }
    fn surface_net_mesh(&self, center: V, edits: &[Value]) -> Result<Vec<Triangle>, String> {
        self.surface_net_mesh_cached(center,edits,!cfg!(test)&&std::env::var("WORMWOOD_CAVE_SAMPLE_CACHE").as_deref()!=Ok("0"))
    }
    fn surface_net_mesh_cached(&self,center:V,edits:&[Value],cached:bool)->Result<Vec<Triangle>,String>{
        let half=self.patch_half_extent(center);
        let c = window(center).map(|n| n as f64 * 4.);
        let height = |n| super::tool_cut::height(n, super::height(n), edits, true);
        // Anchor samples to the world, not the moving 4 m window. Otherwise a
        // window shift changes the grid phase and cave walls visibly jump.
        let step=3.;
        let x0=((c[0]-half)/step).floor()*step-step;
        let z0=((c[2]-half)/step).floor()*step-step;
        let nx=((c[0]+half-x0)/step).ceil() as usize+1;
        let nz=((c[2]+half-z0)/step).ceil() as usize+1;
        let geological=|p:V|{let point=world(p);self.geological_distance(point,height(unit(point)))};
        let carve=|p:V,d:f64|super::material_domain::carve_bedrock(world(p),d,&self.domains);
        let mesh=if !cached {
            super::surface_net::extract([x0,-78.,z0],[nx,32,nz],step,|p|carve(p,geological(p)))?
        }else{CAVE_SAMPLES.with(|slot|{
            let mut cache=slot.borrow_mut();
            if cache.source!=self.source || cache.edits!=edits {cache.values.clear();cache.source=self.source.clone();cache.edits=edits.to_vec();}
            // Retain overlapping world-aligned samples, with bounded memory.
            // Every edit/profile change invalidates; material carving stays live.
            if cache.values.len()>65536 {cache.values.retain(|key,_|{let p=key.map(f64::from_bits);p[0]>=x0-18.&&p[0]<=x0+nx as f64*step+18.&&p[2]>=z0-18.&&p[2]<=z0+nz as f64*step+18.});}
            let values=std::cell::RefCell::new(&mut cache.values);
            super::surface_net::extract([x0,-78.,z0],[nx,32,nz],step,|p|{
                let key=p.map(f64::to_bits);
                let d=*values.borrow_mut().entry(key).or_insert_with(||geological(p));carve(p,d)
            })
        })?};
        let mut triangles = Vec::new();
        for face in &mesh.triangles {
            let mut poly: Vec<V> = face.iter().map(|&i| mesh.vertices[i as usize]).collect();
            for (axis, edge, sign) in [(0, c[0]-half, 1.), (0, c[0]+half, -1.), (2, c[2]-half, 1.), (2, c[2]+half, -1.)] {
                if poly.is_empty() { break; }
                let mut clipped = Vec::new();
                for i in 0..poly.len() {
                    let a=poly[i]; let b=poly[(i+1)%poly.len()];
                    let da=(a[axis]-edge)*sign; let db=(b[axis]-edge)*sign;
                    if da>=0. { clipped.push(a); }
                    if (da>=0.) != (db>=0.) { clipped.push(add(a,mul(sub(b,a),da/(da-db)))); }
                }
                poly=clipped;
            }
            // On solid surface portions of the patch boundary, meet the
            // outdoor height instead of leaving a gap from coarse sampling.
            // An actual cave opening must remain open.
            for p in &mut poly {
                let on_edge=(p[0]-(c[0]-half)).abs()<1e-7 || (p[0]-(c[0]+half)).abs()<1e-7
                    || (p[2]-(c[2]-half)).abs()<1e-7 || (p[2]-(c[2]+half)).abs()<1e-7;
                if on_edge {
                    let n=unit(world(*p)); let h=height(n); let surface=world([p[0],h,p[2]]);
                    if (p[1]-h).abs()<0.5 && self.distance(surface,h)<=1e-6 { p[1]=h; }
                }
            }
            for i in 1..poly.len().saturating_sub(1) {
                let points=[world(poly[0]),world(poly[i]),world(poly[i+1])];
                if length(cross(sub(points[1],points[0]),sub(points[2],points[0]))) < 1e-8 { continue; }
                let centroid=mul(points.iter().copied().fold([0.;3],add),1./3.);
                let normal=self.normal(centroid,&height);
                let depth=local(centroid)[1];
                let color=super::rock::color(centroid).map(|c|c as f64);
                let light=(0.22+0.65*dot(normal,unit([0.4,0.8,0.2])).max(0.)).max(if depth < -6. {0.3} else {0.});
                triangles.push(points.map(|p| [p[0],p[1],p[2],color[0]*light,color[1]*light,color[2]*light]));
                if triangles.len()>10000 { return Err("local cave triangle budget exceeded".into()); }
            }
        }
        Ok(triangles)
    }
    fn mesh_at_spacing(&self, center: V, edits: &[Value], nx: usize, ny: usize, step: f64) -> Result<Vec<Triangle>, String> {
        let center = window(center).map(|n| n as f64 * 4.);
        let origin = [center[0] - 18., -78., center[2] - 18.];
        let height = |n| super::tool_cut::height(n, super::height(n), edits, true);
        let index = |x: usize, y: usize, z: usize| x + (nx + 1) * (z + (nx + 1) * y);
        let mut samples = Vec::with_capacity((nx + 1) * (nx + 1) * (ny + 1));
        for y in 0..=ny {
            for z in 0..=nx {
                for x in 0..=nx {
                    let p = world(add(
                        origin,
                        [x as f64 * step, y as f64 * step, z as f64 * step],
                    ));
                    samples.push((p, super::material_domain::carve_bedrock(p,self.geological_distance(p,height(unit(p))),&self.domains)));
                }
            }
        }
        let mut triangles = Vec::new();
        for y in 0..ny {
            for z in 0..nx {
                for x in 0..nx {
                    let cube = [
                        index(x, y, z),
                        index(x + 1, y, z),
                        index(x, y + 1, z),
                        index(x + 1, y + 1, z),
                        index(x, y, z + 1),
                        index(x + 1, y, z + 1),
                        index(x, y + 1, z + 1),
                        index(x + 1, y + 1, z + 1),
                    ];
                    for ids in [
                        [0, 1, 3, 7],
                        [0, 3, 2, 7],
                        [0, 2, 6, 7],
                        [0, 6, 4, 7],
                        [0, 4, 5, 7],
                        [0, 5, 1, 7],
                    ] {
                        let tet = ids.map(|i| samples[cube[i]]);
                        let mut ring = Vec::new();
                        for [a, b] in [[0, 1], [0, 2], [0, 3], [1, 2], [1, 3], [2, 3]] {
                            if (tet[a].1 > 0.) != (tet[b].1 > 0.) {
                                let t = (tet[a].1 / (tet[a].1 - tet[b].1)).clamp(0., 1.);
                                let p = add(tet[a].0, mul(sub(tet[b].0, tet[a].0), t));
                                if !ring.iter().any(|q| length(sub(*q, p)) < 1e-8) {
                                    ring.push(p);
                                }
                            }
                        }
                        if ring.len() < 3 {
                            continue;
                        }
                        let centroid = mul(
                            ring.iter().copied().fold([0.; 3], add),
                            1. / ring.len() as f64,
                        );
                        let normal = self.normal(centroid, &height);
                        let e = unit(cross(
                            if normal[1].abs() > 0.9 {
                                [1., 0., 0.]
                            } else {
                                [0., 1., 0.]
                            },
                            normal,
                        ));
                        let n = cross(normal, e);
                        ring.sort_by(|a, b| {
                            let a = sub(*a, centroid);
                            let b = sub(*b, centroid);
                            dot(a, n)
                                .atan2(dot(a, e))
                                .total_cmp(&dot(b, n).atan2(dot(b, e)))
                        });
                        let depth = local(centroid)[1];
                        let color = if depth < -22. {
                            [0.19, 0.20, 0.23]
                        } else {
                            [0.36, 0.34, 0.29]
                        };
                        let light = (0.22 + 0.65 * dot(normal, unit([0.4, 0.8, 0.2])).max(0.))
                            .max(if depth < -6. { 0.3 } else { 0. });
                        let vertex = |p: V| {
                            [
                                p[0],
                                p[1],
                                p[2],
                                color[0] * light,
                                color[1] * light,
                                color[2] * light,
                            ]
                        };
                        for i in 1..ring.len() - 1 {
                            triangles.push([vertex(ring[0]), vertex(ring[i]), vertex(ring[i + 1])]);
                        }
                        if triangles.len() > 10000 {
                            return Err("local cave triangle budget exceeded".into());
                        }
                    }
                }
            }
        }
        Ok(triangles)
    }
    /// Substitute a local volume surface for the old single-valued ground mesh.
    pub(crate) fn replace_patch(
        &self,
        triangles: Vec<Triangle>,
        center: V,
        edits: &[Value],
    ) -> Result<Vec<Triangle>, String> {
        if !self.affected(center, 32.) {
            return Ok(triangles);
        }
        let c = window(center).map(|n| n as f64 * 4.);
        let half=self.patch_half_extent(center);
        let bounds = [c[0] - half, c[0] + half, c[2] - half, c[2] + half];
        let mut output = Vec::new();
        for triangle in triangles {
            let mut inside = triangle.to_vec();
            for (axis, edge, sign) in [
                (0, bounds[0], 1.),
                (0, bounds[1], -1.),
                (2, bounds[2], 1.),
                (2, bounds[3], -1.),
            ] {
                let mut kept = Vec::new();
                let mut outside = Vec::new();
                for i in 0..inside.len() {
                    let a = inside[i];
                    let b = inside[(i + 1) % inside.len()];
                    let da = (a[axis] - edge) * sign;
                    let db = (b[axis] - edge) * sign;
                    if da >= 0. {
                        kept.push(a);
                    } else {
                        outside.push(a);
                    }
                    if (da >= 0.) != (db >= 0.) {
                        let t = da / (da - db);
                        let p = std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t);
                        kept.push(p);
                        outside.push(p);
                    }
                }
                for i in 1..outside.len().saturating_sub(1) {
                    output.push([outside[0], outside[i], outside[i + 1]]);
                }
                inside = kept;
                if inside.is_empty() {
                    break;
                }
            }
        }
        output.extend(self.mesh(center, edits)?);
        Ok(output)
    }
}

#[cfg(test)]
impl Field {
    fn reference_geological_distance(&self, point:V,height:f64)->f64 {
        let base=(length(point) - super::RADIUS - height).max(-self.void_distance(point));
        let p=local(point);
        // The Depths use Cardinal's sea-level band, independent of the
        // elevation of the landscape above it. The caps preserve its roof and floor.
        let cave=(super::depths::cave_score(p)*32.).min(-20.-p[1]).min(p[1]+64.);
        let river_data=super::depths::river(p[0],p[2]);
        let river=river_data.map_or(f64::NEG_INFINITY, |r|
            (p[1]-r.bottom).min(r.ceiling-p[1]).min(r.shore));
        let base=base.max(cave).max(river);
        let Some(b)=self.organic_bounds() else {return base};
        let depth=height-p[1];
        let radial=b.radius-(p[0]-b.center[0]).hypot(p[2]-b.center[1]);
        // The noise score is an approximate field, not a calibrated distance;
        // cap it by the bounds so air terminates continuously at their edges.
        let score=if b.legacy_perlin {super::depths::upper_score(p,depth)} else {super::organic_cave_prototype::air_score(p,depth)};
        let organic=(score*32.)
            .min(radial).min(depth-b.min_depth).min(b.max_depth-depth)
            .min(if b.legacy_perlin {p[1]+24.} else {f64::INFINITY});
        base.max(organic)
    }
}

#[cfg(test)]
mod bounded_field_tests {
    use super::*;
    #[test]
    fn capped_noise_rejection_preserves_original_field_bits() {
        let source:Value=serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        for legacy in [false,true] {
            let mut source=source.clone();
            source["organic_noise"]["field"]=serde_json::json!(if legacy {"legacy_perlin"} else {"value_noise"});
            let f=Field::parse(&source).unwrap().unwrap();
            for x in [-1800.,-500.,-40.,0.,35.,300.,1800.] {
                for z in [-1700.,-200.,0.,50.,800.,1700.] {
                    for y in [-90.,-64.,-63.999,-45.,-24.,-21.,-20.,-19.999,-5.,0.,12.,80.] {
                        let p=world([x,y,z]);
                        let h=super::super::height(unit(p));
                        assert_eq!(f.geological_distance(p,h).to_bits(),f.reference_geological_distance(p,h).to_bits(),"{x} {y} {z} legacy={legacy}");
                    }
                }
            }
        }
    }
}

#[cfg(test)]mod void_index_tests {
 use super::*;
 #[test] fn indexed_cave_distance_matches_original_union_bits(){
  let profile:Value=serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();let field=Field::parse(&profile).unwrap().unwrap();
  let start=std::time::Instant::now();let mut points=Vec::new();let mut seed=2718281828u64;
  for _ in 0..24000{let p=std::array::from_fn(|i|{seed=seed.wrapping_mul(6364136223846793005).wrapping_add(1);let x=(seed>>11)as f64/(1u64<<53)as f64;if i==1{-120.+x*220.}else{-600.+x*1200.}});points.push(world(p));}
  for t in &field.tunnels {for t0 in [0.,0.25,0.5,0.75,1.]{let c=add(t.start,mul(sub(t.end,t.start),t0));for axis in 0..3 {for offset in [-t.radius-1e-8,0.,t.radius+1e-8]{let mut p=c;p[axis]+=offset;points.push(world(p));}}}}
  for c in &field.chambers {points.push(world(c.center));for axis in 0..3{for d in [-1.00000001,-1.,-0.99999999,0.99999999,1.,1.00000001]{let mut p=c.center;p[axis]+=c.radii[axis]*d;points.push(world(p));}}}
  for &p in &points {assert_eq!(field.void_distance(p).to_bits(),field.void_distance_reference(p).to_bits(),"point {:?}",local(p));}
  eprintln!("{} exact indexed/reference cave queries checked in {:?}",points.len(),start.elapsed());
  let begin=std::time::Instant::now();for &p in &points{std::hint::black_box(field.void_distance_reference(std::hint::black_box(p)));}let linear=begin.elapsed();
  let begin=std::time::Instant::now();for &p in &points{std::hint::black_box(field.void_distance(std::hint::black_box(p)));}let indexed=begin.elapsed();eprintln!("linear {:?}, indexed {:?}, speedup {:.2}",linear,indexed,linear.as_secs_f64()/indexed.as_secs_f64());
 }
}

#[cfg(test)]mod sample_cache_tests {
 use super::*;
 #[test]fn overlapping_cave_windows_reuse_exact_field_samples(){
  let source:Value=serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();let field=Field::parse(&source).unwrap().unwrap();
  for p in [[0.,-32.,-13.],[4.,-32.,-13.],[8.,-32.,-13.],[8.,-32.,-9.],[400.,-40.,400.]] {
   let center=world(p);let a=field.surface_net_mesh_cached(center,&[],false).unwrap();let b=field.surface_net_mesh_cached(center,&[],true).unwrap();assert_eq!(a.len(),b.len());
   for (a,b)in a.iter().zip(&b){for (a,b)in a.iter().flatten().zip(b.iter().flatten()){assert_eq!(a.to_bits(),b.to_bits());}}
  }
  let edits=vec![serde_json::json!({"normal":unit(world([8.,0.,-13.])),"radius":20.,"depth":4.})];
  for edits in [edits,vec![]] {let center=world([8.,-32.,-13.]);let a=field.surface_net_mesh_cached(center,&edits,false).unwrap();let b=field.surface_net_mesh_cached(center,&edits,true).unwrap();assert_eq!(a,b);}

 }
}
