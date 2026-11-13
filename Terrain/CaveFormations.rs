//! Offline Blender convex meshes: identical visible faces and contact half-spaces.
//! Placement probes geology once on the control side; never runs in audio callbacks.
use super::underground::{Field, Triangle, local, world};
use crate::audio::{V, add, cross, dot, length, mul, sub, unit};
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

struct Mesh {
    vertices: Vec<V>,
    triangles: Vec<[usize; 3]>,
    planes: Vec<[f64; 4]>,
    color: [f32; 3],
}
fn meshes() -> &'static std::collections::BTreeMap<String, Mesh> {
    static M: OnceLock<std::collections::BTreeMap<String, Mesh>> = OnceLock::new();
    M.get_or_init(|| {
        let v: Value =
            serde_json::from_str(include_str!("../Art/Terrain/CaveFormations/meshes.json"))
                .expect("checked offline cave meshes");
        v["assets"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, row)| {
                (
                    name.clone(),
                    Mesh {
                        vertices: serde_json::from_value(row["vertices"].clone()).unwrap(),
                        triangles: serde_json::from_value(row["triangles"].clone()).unwrap(),
                        planes: serde_json::from_value(row["planes"].clone()).unwrap(),
                        color: serde_json::from_value(row["color"].clone()).unwrap(),
                    },
                )
            })
            .collect()
    })
}
#[derive(Clone, Debug)]
pub(crate) struct Instance {
    pub origin: V,
    scale: V,
    mesh: &'static str,
    pub mineral: Option<&'static str>,
}
impl Instance {
    pub(crate) fn distance(&self, p: V) -> f64 {
        self.local_distance(local(p))
    }
    fn local_distance(&self, p: V) -> f64 {
        let p = sub(p, self.origin);
        let bounds = (p[0].abs() - self.scale[0].abs() * 1.1)
            .max(p[2].abs() - self.scale[2].abs() * 1.1)
            .max(self.scale[1].min(0.) - p[1])
            .max(p[1] - self.scale[1].max(0.));
        if bounds > 1. {
            return bounds;
        }
        // Transform plane normals too, preserving a conservative metric distance.
        meshes()[self.mesh]
            .planes
            .iter()
            .map(|plane| {
                let n: V = std::array::from_fn(|k| plane[k] / self.scale[k]);
                (dot(n, p) - plane[3]) / length(n)
            })
            .fold(f64::NEG_INFINITY, f64::max)
    }
    fn color(&self) -> [f32; 3] {
        if self.mineral.is_some() {
            meshes()[self.mesh].color
        } else {
            super::rock::base_color(world(self.origin))
        }
    }
    fn triangles(&self) -> Vec<Triangle> {
        let mesh = &meshes()[self.mesh];
        let color = self.color().map(f64::from);
        mesh.triangles
            .iter()
            .map(|ids| {
                let mut points = ids.map(|i| {
                    world(add(
                        self.origin,
                        std::array::from_fn(|k| mesh.vertices[i][k] * self.scale[k]),
                    ))
                });
                if self.scale[1] < 0. {
                    points.swap(1, 2);
                }
                points.map(|p| [p[0], p[1], p[2], color[0], color[1], color[2]])
            })
            .collect()
    }
}
fn seed(x: i32, z: i32) -> u32 {
    let mut s = (x as u32).wrapping_mul(374761393) ^ (z as u32).wrapping_mul(668265263);
    s = (s ^ (s >> 13)).wrapping_mul(1274126177);
    s ^ (s >> 16)
}
fn build(field: &Field, profile: &Value) -> Vec<Instance> {
    if profile["formations"]["enabled"] != true {
        return vec![];
    }
    let mut out = Vec::new();
    let spacing = profile["formations"]["spacing_metres"]
        .as_f64()
        .unwrap_or(9.);
    let limit = profile["formations"]["max_instances"]
        .as_u64()
        .unwrap_or(240) as usize;
    for chamber in profile["chambers"].as_array().into_iter().flatten() {
        let c: V = serde_json::from_value(chamber["center"].clone()).unwrap();
        let r: V = serde_json::from_value(chamber["radii"].clone()).unwrap();
        for ix in -3..=3 {
            for iz in -3..=3 {
                let x = c[0] + ix as f64 * spacing;
                let z = c[2] + iz as f64 * spacing;
                if ((x - c[0]) / r[0]).powi(2) + ((z - c[2]) / r[2]).powi(2) > 0.72 {
                    continue;
                }
                // A generous corridor keeps original traversal routes open.
                if profile["tunnels"].as_array().unwrap().iter().any(|t| {
                    let a: V = serde_json::from_value(t["start"].clone()).unwrap();
                    let b: V = serde_json::from_value(t["end"].clone()).unwrap();
                    let d = [b[0] - a[0], 0., b[2] - a[2]];
                    let p = [x - a[0], 0., z - a[2]];
                    let k = (dot(p, d) / dot(d, d).max(1e-9)).clamp(0., 1.);
                    length(sub(p, mul(d, k))) < 2.6
                }) {
                    continue;
                }
                let s = seed(x.round() as i32, z.round() as i32);
                let h = super::height(unit(world([x, 0., z])));
                let lo = (c[1] - r[1] - 4.).max(-70.);
                let hi = (c[1] + r[1] + 4.).min(h - 3.);
                let distance = |y| field.geological_distance(world([x, y, z]), h);
                // Choose a solid->air floor and its next air->solid ceiling. Refine both.
                let mut floor = None;
                let mut previous = lo;
                let mut pd = distance(lo);
                for step in 1..=((hi - lo) / 0.5).ceil().max(0.) as usize {
                    let y = (lo + step as f64 * 0.5).min(hi);
                    let d = distance(y);
                    if pd <= 0. && d > 0. {
                        let (mut a, mut b) = (previous, y);
                        for _ in 0..16 {
                            let m = (a + b) * 0.5;
                            if distance(m) > 0. { b = m } else { a = m }
                        }
                        floor = Some((a + b) * 0.5);
                    }
                    if pd > 0. && d <= 0. {
                        if let Some(f) = floor.take() {
                            let (mut a, mut b) = (previous, y);
                            for _ in 0..16 {
                                let m = (a + b) * 0.5;
                                if distance(m) > 0. { a = m } else { b = m }
                            }
                            let ceiling = (a + b) * 0.5;
                            let clearance = ceiling - f;
                            if clearance < 4.5 {
                                continue;
                            }
                            let height = (1.5 + (s % 170) as f64 * 0.01).min(clearance * 0.30);
                            let radius = 0.48 + (s % 45) as f64 * 0.01;
                            let rock = if s & 1 == 0 {
                                "stalagmite_a"
                            } else {
                                "stalagmite_b"
                            };
                            out.push(Instance {
                                origin: [x, f - 0.18, z],
                                scale: [radius, height, radius * 0.85],
                                mesh: rock,
                                mineral: None,
                            });
                            out.push(Instance {
                                origin: [x, ceiling + 0.18, z],
                                scale: [radius * 0.7, -height * 0.85, radius * 0.65],
                                mesh: "stalactite",
                                mineral: None,
                            });
                            if f > -20. {
                                let mineral = if s & 2 == 0 { "ruby" } else { "silver" };
                                for j in 0..3 {
                                    out.push(Instance {
                                        origin: [
                                            x + radius * 0.62 + j as f64 * 0.06,
                                            f + height * (0.30 + j as f64 * 0.11),
                                            z - radius * 0.38,
                                        ],
                                        scale: [0.20, 0.36, 0.22],
                                        mesh: mineral,
                                        mineral: Some(mineral),
                                    });
                                }
                            } else {
                                for j in 0..4 {
                                    let mineral = if (s + j) & 1 == 0 {
                                        "blue_crystal"
                                    } else {
                                        "magenta_crystal"
                                    };
                                    let cx = x - radius + 0.32 * j as f64;
                                    let cz = z + 0.8;
                                    // Each cluster root probes its own floor; offsetting a
                                    // cluster from a dripstone must not leave it floating.
                                    let ground =
                                        |y| field.geological_distance(world([cx, y, cz]), h);
                                    let mut root = None;
                                    for k in 0..32 {
                                        let a = f - 2. + k as f64 * 0.125;
                                        let b = a + 0.125;
                                        if ground(a) <= 0. && ground(b) > 0. {
                                            let (mut a, mut b) = (a, b);
                                            for _ in 0..16 {
                                                let m = (a + b) * 0.5;
                                                if ground(m) > 0. { b = m } else { a = m }
                                            }
                                            root = Some((a + b) * 0.5);
                                            break;
                                        }
                                    }
                                    let Some(root) = root else { continue };
                                    out.push(Instance {
                                        origin: [cx, root - 0.18, cz],
                                        scale: [0.25, 0.9 + (j % 3) as f64 * 0.4, 0.28],
                                        mesh: mineral,
                                        mineral: Some(mineral),
                                    });
                                }
                            }
                            if out.len() + 6 > limit {
                                return out;
                            }
                            break;
                        }
                    }
                    previous = y;
                    pd = d;
                }
            }
        }
    }
    out
}
pub(crate) fn place(field: &Field, profile: &Value) -> Vec<Instance> {
    static CACHE: Mutex<Option<(Value, Vec<Instance>)>> = Mutex::new(None);
    let mut key = profile.clone();
    key.as_object_mut().unwrap().remove("material_domains");
    let mut cache = CACHE.lock().unwrap();
    if let Some((old, instances)) = &*cache {
        if *old == key {
            return instances.clone();
        }
    }
    let instances = build(field, profile);
    *cache = Some((key, instances.clone()));
    instances
}
fn defaults() -> &'static Vec<Instance> {
    static DEFAULTS: OnceLock<Vec<Instance>> = OnceLock::new();
    DEFAULTS.get_or_init(|| {
        let profile: Value =
            serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        Field::parse(&profile).unwrap().unwrap().formations
    })
}
/// Immutable material-space identity; moving fracture cells retain their sampled tint.
pub(crate) fn mineral_material(p: V) -> Option<(&'static str, [f32; 3])> {
    let p = local(p);
    defaults()
        .iter()
        .filter(|i| i.mineral.is_some())
        .find(|i| i.local_distance(p) < 0.065)
        .map(|i| (i.mineral.unwrap(), i.color()))
}
pub(crate) fn mineral_color(p: V) -> Option<[f32; 3]> {
    mineral_material(p).map(|(_, color)| color)
}
/// Union query shares one radial transform across all candidate instances.
pub(crate) fn solid_distance(instances: &[Instance], point: V, base: f64) -> f64 {
    let p = local(point);
    instances
        .iter()
        .fold(base, |d, i| d.min(i.local_distance(p)))
}

// Contact queries used to test every formation for every geological sample.
// These bounds reject only instances that cannot lower the existing field.
// Leaf formulas and their original evaluation order remain unchanged.
struct IndexNode {min:V,max:V,children:Option<[usize;2]>,ids:Vec<usize>}
pub(crate) struct ContactIndex {nodes:Vec<IndexNode>}
impl ContactIndex {
    pub fn build(instances:&[Instance])->Self {
        let mut index=Self{nodes:Vec::new()};
        if instances.is_empty()||instances.len()>256{return index;}
        let mut ids:Vec<_>=(0..instances.len()).collect();index.node(instances,&mut ids);index
    }
    fn bounds(i:&Instance)->(V,V){
        // local_distance returns the box distance whenever it exceeds 1 m.
        // Expanding that box by 1 m also encloses its plane-evaluation region.
        (add(i.origin,[-i.scale[0].abs()*1.1-1.,i.scale[1].min(0.)-1.,-i.scale[2].abs()*1.1-1.]),
         add(i.origin,[i.scale[0].abs()*1.1+1.,i.scale[1].max(0.)+1.,i.scale[2].abs()*1.1+1.]))
    }
    fn node(&mut self,instances:&[Instance],ids:&mut [usize])->usize {
        let mut min=[f64::INFINITY;3];let mut max=[f64::NEG_INFINITY;3];
        for &id in ids.iter(){let(a,b)=Self::bounds(&instances[id]);for k in 0..3{min[k]=min[k].min(a[k]);max[k]=max[k].max(b[k]);}}
        let n=self.nodes.len();self.nodes.push(IndexNode{min,max,children:None,ids:Vec::new()});
        if ids.len()<=4 {self.nodes[n].ids=ids.to_vec();}
        else {let axis=(0..3).max_by(|&a,&b|(max[a]-min[a]).total_cmp(&(max[b]-min[b]))).unwrap();
            ids.sort_unstable_by(|&a,&b|instances[a].origin[axis].total_cmp(&instances[b].origin[axis]));
            let mid=ids.len()/2;let(a,b)=ids.split_at_mut(mid);let a=self.node(instances,a);let b=self.node(instances,b);self.nodes[n].children=Some([a,b]);}
        n
    }
    fn candidates(&self,n:usize,p:V,base:f64,mask:&mut [u64;4]){
        let node=&self.nodes[n];
        let gap=(0..3).map(|k|(node.min[k]-p[k]).max(p[k]-node.max[k])).fold(f64::NEG_INFINITY,f64::max);
        // Inside a node we make no lower-bound claim about its signed field.
        if gap>0. && gap-1e-9*(1.+gap.abs())>base{return;}
        if let Some([a,b])=node.children{self.candidates(a,p,base,mask);self.candidates(b,p,base,mask);}
        else {for &id in &node.ids{mask[id/64]|=1u64<<(id%64);}}
    }
    pub fn distance(&self,instances:&[Instance],point:V,base:f64)->f64 {
        if self.nodes.is_empty(){return solid_distance(instances,point,base);}
        let p=local(point);let mut mask=[0u64;4];self.candidates(0,p,base,&mut mask);
        let mut result=base;
        for (word,mut bits)in mask.into_iter().enumerate(){while bits!=0{let bit=bits.trailing_zeros()as usize;bits&=bits-1;result=result.min(instances[word*64+bit].local_distance(p));}}
        result
    }

}

#[cfg(test)]mod contact_index_tests {
 use super::*;
 #[test]fn formation_contact_index_matches_original_field_bits(){
  let profile:Value=serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();let field=Field::parse(&profile).unwrap().unwrap();let instances=&field.formations;let index=ContactIndex::build(instances);
  let mut points=Vec::new();for x in (-160..=100).step_by(7){for z in (-100..=80).step_by(7){for y in [-90.,-66.,-49.,-32.,-20.,-9.,2.,50.]{points.push(world([x as f64,y,z as f64]));}}}
  for i in instances {let(a,b)=ContactIndex::bounds(i);for p in [i.origin,a,b,add(a,[-1e-8,0.,0.]),add(b,[1e-8,0.,0.])]{points.push(world(p));}}
  let start=std::time::Instant::now();let expected:Vec<_>=points.iter().enumerate().map(|(i,&p)|solid_distance(instances,p,[-10.,-0.,0.,0.1,1.,10.,f64::INFINITY][i%7]).to_bits()).collect();let linear=start.elapsed();
  let start=std::time::Instant::now();let actual:Vec<_>=points.iter().enumerate().map(|(i,&p)|index.distance(instances,p,[-10.,-0.,0.,0.1,1.,10.,f64::INFINITY][i%7]).to_bits()).collect();let indexed=start.elapsed();assert_eq!(actual,expected);
  eprintln!("{} formation queries, {} instances: linear {:?}, indexed {:?}",points.len(),instances.len(),linear,indexed);
 }
}
pub(crate) fn append_mesh(instances: &[Instance], center: V, half: f64, out: &mut Vec<Triangle>) {
    let c = super::underground::window(center).map(|n| n as f64 * 4.);
    for instance in instances {
        // Include only whole specimens inside the window: no newly exposed cutoff faces.
        let radius = instance.scale[0].abs().max(instance.scale[2].abs()) * 1.1;
        if (instance.origin[0] - c[0]).abs() + radius > half
            || (instance.origin[2] - c[2]).abs() + radius > half
        {
            continue;
        }
        out.extend(instance.triangles());
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cave_formations_are_attached_clear_and_include_all_minerals() {
        let data: Value = serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        let field = Field::parse(&data).unwrap().unwrap();
        let forms = &field.formations;
        assert!(
            forms.len() > 20 && forms.len() <= 240,
            "{} formations",
            forms.len()
        );
        for material in ["ruby", "silver", "blue_crystal", "magenta_crystal"] {
            assert!(
                forms.iter().any(|i| i.mineral == Some(material)),
                "missing {material}"
            );
        }
        for i in forms.iter().filter(|i| {
            i.mineral.is_none() || matches!(i.mineral, Some("blue_crystal" | "magenta_crystal"))
        }) {
            let base = world(i.origin);
            let h = super::super::height(unit(base));
            assert!(
                field.geological_distance(base, h) <= 0.01,
                "unattached {:?}",
                i
            );
            let p = world(add(i.origin, [0., i.scale[1] * 0.4, 0.]));
            assert!(i.distance(p) < 0. && field.distance(p, h) < 0.);
        }
        eprintln!("{} attached cave formations/inclusions", forms.len());
    }
    #[test]
    fn blender_halfspaces_agree_with_faces() {
        for (name, m) in meshes() {
            for ids in &m.triangles {
                let p = ids.map(|i| m.vertices[i]);
                let center = mul(p.iter().copied().fold([0.; 3], add), 1. / 3.);
                let d = m
                    .planes
                    .iter()
                    .map(|n| dot([n[0], n[1], n[2]], center) - n[3])
                    .fold(f64::NEG_INFINITY, f64::max);
                assert!(d.abs() < 0.0005, "{name}: {d}");
                let n = unit(cross(sub(p[1], p[0]), sub(p[2], p[0])));
                assert!(dot(n, sub(center, [0., 0.4, 0.])) > 0.);
            }
        }
    }
}

#[cfg(test)]
mod route_checks {
    use super::*;
    #[test]
    fn additional_entrances_reach_surface_and_join_the_old_route() {
        let data: Value = serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        let cave = Field::parse(&data).unwrap().unwrap();
        for t in data["tunnels"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["type"].is_string())
        {
            let a: V = serde_json::from_value(t["start"].clone()).unwrap();
            let b: V = serde_json::from_value(t["end"].clone()).unwrap();
            for step in 0..=40 {
                let p = world(add(a, mul(sub(b, a), step as f64 / 40.)));
                assert!(
                    cave.distance(p, super::super::height(unit(p))) > 0.,
                    "blocked {} at {:?}",
                    t["type"],
                    local(p)
                );
            }
            if a[1] > 0. {
                let h = super::super::height(unit(world([a[0], 0., a[2]])));
                assert!(a[1] > h, "entrance must begin above its real hillside");
            }
        }
    }
    #[test]
    fn accepted_mining_domains_remove_formation_volume_and_faces() {
        let data: Value = serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
        let mut cave = Field::parse(&data).unwrap().unwrap();
        let f = cave
            .formations
            .iter()
            .find(|f| f.mineral == Some("ruby"))
            .unwrap();
        let p = world(add(f.origin, [0., f.scale[1] * 0.5, 0.]));
        let h = super::super::height(unit(p));
        assert!(cave.distance(p, h) < 0.);
        let edits = vec![
            serde_json::json!({"mined":true,"domain":{"origin":p,"axes":[[1,0,0],[0,1,0],[0,0,1]],"min":[-0.3,-0.3,-0.3],"max":[0.3,0.3,0.3]}}),
        ];
        cave.set_material_domains(&edits);
        assert!(cave.distance(p, h) > 0.);
        let faces = super::super::material_domain::clip(
            cave.mesh(p, &[]).unwrap(),
            &super::super::material_domain::from_edits(&edits),
        );
        for t in faces {
            let center = mul(
                t.iter().map(|v| [v[0], v[1], v[2]]).fold([0.; 3], add),
                1. / 3.,
            );
            assert!(length(sub(center, p)) > 0.299);
        }
    }
}
