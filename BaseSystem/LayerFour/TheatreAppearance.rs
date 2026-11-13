//! Retain camera-independent venue lighting work; projection and world light stay live.
use crate::audio::{add, sub, mul, dot, cross, unit, length, vector};
use serde_json::Value;
use std::cell::RefCell;
type V = [f64; 3];
struct Face {
    center: V,
    normal: V,
    inside: bool,
    auditorium_inside:bool,
    uv: [[f64; 2]; 3],
    // Both sides are needed: the original shading normal faces the observer.
    lamp: [[[f64; 3]; 3]; 2],
    stage: [[f64;crate::theatre::lighting::COUNT];3],
}
#[derive(Default)]
struct Cache { source: Vec<[f32; 7]>, faces: Vec<Face>, bounds:Vec<Bounds>, owner:Option<std::sync::Arc<crate::theatre::Venue>> }
struct Bounds {min:V,max:V,begin:usize,end:usize,children:Option<[usize;2]>}
fn bounds(vertices:&[[f32;7]],begin:usize,end:usize,nodes:&mut Vec<Bounds>)->usize {
 let min=std::array::from_fn(|k|vertices[begin*3..end*3].iter().map(|v|v[k]as f64).fold(f64::INFINITY,f64::min));
 let max=std::array::from_fn(|k|vertices[begin*3..end*3].iter().map(|v|v[k]as f64).fold(f64::NEG_INFINITY,f64::max));
 let id=nodes.len();nodes.push(Bounds{min,max,begin,end,children:None});
 if end-begin>64{let mid=(begin+end)/2;let a=bounds(vertices,begin,mid,nodes);let b=bounds(vertices,mid,end,nodes);nodes[id].children=Some([a,b]);}id
}
// make_interior.py: opaque side walls x=+-34, back z=50, ceiling y=18.
// The sole room portal is in the front wall z=-58. Keep its complete potential
// view cone by never applying this rejection from the front of that wall.
fn auditorium_hidden(camera:V)->bool {
 camera[2]>-57.3&&(camera[0].abs()>34.3||camera[2]>50.4||camera[1]>18.5||camera[1]< -0.2)
}
fn reflected_bounds(min:V,max:V,plane:[V;2])->(V,V) {
 let n=unit(plane[0]);let mut low=[f64::INFINITY;3];let mut high=[f64::NEG_INFINITY;3];
 for corner in 0..8 {let p=std::array::from_fn(|i|if corner&(1<<i)==0{min[i]}else{max[i]});let q=sub(p,mul(n,2.*dot(n,sub(p,plane[1]))));for i in 0..3{low[i]=low[i].min(q[i]);high[i]=high[i].max(q[i]);}}
 (low,high)
}
fn visible_ranges(cache:&Cache,camera:V,projector:&super::projection::Projector,cull:bool)->Vec<std::ops::Range<usize>> {
 visible_reflected_ranges(cache,camera,projector,cull,&[])
}
fn visible_reflected_ranges(cache:&Cache,camera:V,projector:&super::projection::Projector,cull:bool,planes:&[[V;2]])->Vec<std::ops::Range<usize>> {
 if !cull || std::env::var("WORMWOOD_TOWN_HIERARCHY").as_deref()==Ok("0"){return vec![0..cache.faces.len()];}
 let mut ranges=Vec::new();let mut stack=vec![0];
 while let Some(i)=stack.pop(){let Some(b)=cache.bounds.get(i)else{continue;};
  if !projector.bounds_visible(b.min,b.max,camera)&&!planes.iter().any(|p|{let (min,max)=reflected_bounds(b.min,b.max,*p);projector.bounds_visible(min,max,camera)}){continue;}
  if let Some([a,b])=b.children{stack.push(b);stack.push(a);}else{ranges.push(b.begin..b.end);}
 }ranges
}
thread_local! { static CACHE: RefCell<Cache> = RefCell::new(Cache::default()); static CHAIR_CACHE: RefCell<Cache> = RefCell::new(Cache::default()); static LAB_CACHE:RefCell<Cache>=RefCell::new(Cache::default()); static ZOO_CACHE:RefCell<Cache>=RefCell::new(Cache::default()); }
fn prepare(vertices: &[[f32; 7]]) -> Cache {
    let faces = vertices.chunks_exact(3).map(|triangle| {
        let p: [V; 3] = std::array::from_fn(|i| std::array::from_fn(|j| triangle[i][j] as f64));
        let center = mul(add(add(p[0], p[1]), p[2]), 1. / 3.);
        let normal = unit(cross(sub(p[1], p[0]), sub(p[2], p[0])));
        let theatre_inside = center[0].hypot(center[2]) < 75.8 && center[1] < 17.9;
        // Civic interiors keep the same stable ambient fill as the venue.
        // Their local Y contains the surveyed foundation height, so only the
        // horizontal footprint classifies these static interior faces.
        let civic_inside = ((center[0]+240.).abs()<50.1 && center[2].abs()<90.5)
            || ((center[0]-330.).abs()<150.1 && center[2].abs()<118.5);
        let regional=triangle[0][6]==crate::theatre::regional_cities::CITY_TAG || triangle[0][6]==crate::theatre::regional_cities::GLASS_TAG || triangle[0][6]==72.;
        let inside = !regional && (theatre_inside || civic_inside);
        let metres = match triangle[0][6] { 102.0 => 2., 103.0 => 3., 114.0 => 0.08, 104.0 => 0.55, _ => 1. };
        let axes = normal.map(f64::abs);
        let uv = p.map(|p| {
            let uv = if axes[1] >= axes[0].max(axes[2]) { [p[0],p[2]] }
                else if axes[0] >= axes[2] { [p[2],-p[1]] } else { [p[0],-p[1]] };
            uv.map(|n| n / metres)
        });
        let lamp = std::array::from_fn(|side| p.map(|p| {
            let mut shade = if civic_inside {[0.65,0.60,0.54]}else{[0.34,0.30,0.26]};
            if inside {
                let n = if side == 0 { normal } else { mul(normal,-1.) };
                for lamp in [[-23.,15.,-24.],[23.,15.,-24.],[-18.,15.,18.],[18.,15.,18.],[0.,13.,-49.]] {
                    let delta = sub(lamp,p);
                    let gain = dot(n,unit(delta)).max(0.) * 1.2 / (1. + dot(delta,delta) / 250.);
                    for i in 0..3 { shade[i] += gain * [1.,0.86,0.68][i]; }
                }
            }
            shade
        }));
        let stage=p.map(|point|crate::theatre::lighting::weights(point,normal));
        Face { center, normal, inside, auditorium_inside:!regional && p.iter().all(|v|v[0].abs()<33.6&&v[2]>-57.5&&v[2]<49.7&&v[1]>0.02&&v[1]<17.8), uv, lamp, stage }
    }).collect();
    let mut nodes=Vec::new();if !vertices.is_empty(){bounds(vertices,0,vertices.len()/3,&mut nodes);}
    Cache { source: vertices.to_vec(), faces, bounds:nodes,owner:None }
}
pub(super) fn project(origin: V, vertices: &[[f32; 7]], input: &Value) -> Result<Vec<[f32; 24]>,String> {
    project_owned(origin,vertices,input,None)
}
pub(super) fn project_shared(origin:V,venue:&std::sync::Arc<crate::theatre::Venue>,input:&Value)->Result<Vec<[f32;24]>,String>{
    let mut out=project_owned(origin,&venue.vertices,input,Some(venue))?;
    if let Some(site)=&venue.laboratory {
        let camera=sub(vector(&input["position"])?,origin);
        for (center,mesh,cache) in [(site.lab,&site.lab_vertices,&LAB_CACHE),(site.zoo,&site.zoo_vertices,&ZOO_CACHE)] {
            if length(sub(camera,center))<500. {out.extend(project_cached(origin,mesh,input,Some(venue),cache)?);}
        }
    }
    Ok(out)
}
fn project_owned(origin:V,vertices:&[[f32;7]],input:&Value,owner:Option<&std::sync::Arc<crate::theatre::Venue>>)->Result<Vec<[f32;24]>,String>{
    project_cached(origin,vertices,input,owner,&CACHE)
}
pub(super) fn project_chair(origin:V,vertices:&[[f32;7]],input:&Value)->Result<Vec<[f32;24]>,String>{
    project_cached(origin,vertices,input,None,&CHAIR_CACHE)
}
fn project_cached(origin:V,vertices:&[[f32;7]],input:&Value,owner:Option<&std::sync::Arc<crate::theatre::Venue>>,storage:&'static std::thread::LocalKey<RefCell<Cache>>)->Result<Vec<[f32;24]>,String>{
    if vertices.len() > crate::theatre::MAX_TRIANGLES * 3 || vertices.len() % 3 != 0 {
        return Err("theatre mesh budget exceeded".into());
    }
    let stage_active=input["stage_lighting"].is_array();
    let stage_levels=crate::theatre::lighting::levels(input);
    let eye = vector(&input["position"])?;
    let camera = sub(eye,origin);
    let deep=crate::terrain::underground::deep_view(eye);
    let lighting = crate::base_system::layer_four::atmosphere::Lighting::from_view(input);
    storage.with(|cache| {
        let mut cache = cache.borrow_mut();
        // Exact contents, not an address-only key: reloads and in-place edits invalidate.
        // One bounded venue is retained; no per-frame lighting or camera snapshots.
        let shared=owner.is_some_and(|v|cache.owner.as_ref().is_some_and(|old|std::sync::Arc::ptr_eq(v,old)));
        if !shared {
            if cache.source != vertices { *cache = prepare(vertices); }
            // Retaining the immutable owner prevents pointer reuse or in-place
            // mutation from turning this identity into a stale-content key.
            cache.owner=owner.cloned();
        }
        let mut view=input.clone();view["position"]=serde_json::json!([0.,0.,0.]);
        view["two_sided"]=serde_json::json!(true);
        view["far"]=serde_json::json!(length(eye)+2.*crate::terrain::RADIUS);
        let projector=super::projection::Projector::new(&view);
        let mut out=Vec::with_capacity(vertices.len()/3);
        // Reflections may need faces outside the primary camera. Until their
        // reflected frustum is supplied, retain the complete venue for water.
        let cull=if input["retain_offscreen_reflections"].is_boolean(){input["retain_offscreen_reflections"]!=true}
            else{input["water_instances"].as_array().is_none_or(Vec::is_empty)};
        let planes:Vec<[V;2]>=serde_json::from_value(input["reflection_planes"].clone()).ok().filter(|p:&Vec<[V;2]>|p.len()<=16&&p.iter().all(|p|p.iter().flatten().all(|n|n.is_finite())&&length(p[0])>0.9)).unwrap_or_default();
        let reflected=!planes.is_empty()&&std::env::var("WORMWOOD_REFLECTION_CULL").as_deref()!=Ok("0");
        let hide_auditorium=std::env::var("WORMWOOD_AUDITORIUM_OCCLUSION").as_deref()!=Ok("0")
            && auditorium_hidden(camera)&&(cull||!planes.is_empty())
            && planes.iter().all(|plane|{let n=unit(plane[0]);auditorium_hidden(sub(camera,mul(n,2.*dot(n,sub(camera,plane[1])))))});

        for range in visible_reflected_ranges(&cache,camera,&projector,cull||reflected,if reflected{&planes}else{&[]}) {
        for index in range {
            let triangle=&vertices[index*3..index*3+3];let face=&cache.faces[index];
            if face.auditorium_inside&&hide_auditorium {continue;}
            // The finite deep terrain mesh replaces the exterior surface. Keep
            // subterranean slide/grotto faces without leaking the surface town
            // through the intentionally unmeshed distant world.
            if deep && triangle.iter().all(|v|crate::terrain::underground::local(add(origin,[v[0] as f64,v[1] as f64,v[2] as f64]))[1]>=-18.) {continue;}

            let mut local=[[0.;6];3];
            for (i,v) in triangle.iter().enumerate() {
                let r=sub(std::array::from_fn(|j|v[j] as f64),camera);
                for j in 0..3 {local[i][j]=r[j] as f32;}
            }
            let mut projected=projector.triangle(&local).unwrap();
            if cull && !super::projection::in_frustum(&projected) {continue;}
            let side = usize::from(dot(face.normal,sub(camera,face.center)) < 0.);
            let normal = if side == 0 { face.normal } else { mul(face.normal,-1.) };
            let tag = triangle[0][6];
            let outside = if !face.inside {
                let mut shade = lighting.tint.map(|n| n * 0.25 * lighting.brightness());
                let gain = dot(normal,lighting.primary).max(0.) * lighting.brightness();
                for i in 0..3 { shade[i] += gain * lighting.tint[i]; }
                shade
            } else { [0.;3] };
            for (i,v) in triangle.iter().enumerate() {
                let p = std::array::from_fn(|j| v[j] as f64);
                let r = sub(p,camera);
                if stage_active && (201.0..=206.0).contains(&tag) {
                    let l=stage_levels[(tag-201.) as usize];
                    local[i]=[r[0] as f32,r[1] as f32,r[2] as f32,
                        (l[0]*(0.08+l[3]*0.92)) as f32,(l[1]*(0.08+l[3]*0.92)) as f32,(l[2]*(0.08+l[3]*0.92)) as f32];continue;
                }
                if (106.0..=113.0).contains(&tag) || (115.0..=120.0).contains(&tag) {
                    local[i]=[r[0] as f32,r[1] as f32,r[2] as f32,v[3],v[4],v[5]];
                    continue;
                }
                let mut shade = if face.inside { face.lamp[side][i] } else { outside };
                if stage_active && p[0].abs()<=34. && (19.8..=50.1).contains(&p[2]) && (3.0..=17.0).contains(&p[1]) && tag==0. {
                    let lit=crate::theatre::lighting::shade(face.stage[i],&stage_levels);
                    for c in 0..3 {shade[c]=0.08+lit[c]*2.5;}
                }
                if input["diagnostic_cave_fullbright"]==true {shade=[1.;3];}
                else if !face.inside {
                    let exposure=crate::terrain::underground::sky_exposure(add(origin,p));
                    shade=shade.map(|v|v*exposure);
                }
                if deep && input["diagnostic_cave_fullbright"]!=true {
                    let t=((length(r)-24.)/10.).clamp(0.,1.);
                    shade=shade.map(|v|v*(1.-t*t*(3.-2.*t)));
                }
                if tag == crate::theatre::regional_cities::GLASS_TAG { shade=shade.map(|v|v.max(0.8)); }
                if tag == 72. {shade=[1.;3];}
                if tag >= 100. {
                    local[i]=[r[0] as f32,r[1] as f32,r[2] as f32,face.uv[i][0] as f32,
                        face.uv[i][1] as f32,(shade[0]*0.35+shade[1]*0.55+shade[2]*0.10) as f32];
                } else {
                    local[i]=[r[0] as f32,r[1] as f32,r[2] as f32,
                        (v[3] as f64*shade[0]).min(1.) as f32,(v[4] as f64*shade[1]).min(1.) as f32,
                        (v[5] as f64*shade[2]).min(1.) as f32];
                }
            }
            for i in 0..3 {
                projected[12+i*4..16+i*4].copy_from_slice(&[local[i][3],local[i][4],local[i][5],1.]);
            }
            if tag>=100. && !(stage_active&&(201.0..=206.0).contains(&tag)) {for i in [15,19,23] {projected[i]=tag;}}
            out.push(projected);
        }
        }
        Ok(out)
    })
}

#[cfg(test)]
mod tests {
use super::*;
use crate::base_system::layer_four::appearance_system::projection;
fn reference(origin:[f64;3],vertices:&[[f32;7]],input:&Value)->Result<Vec<[f32;24]>,String> {
    use crate::audio::{add,sub,mul,dot,cross,unit,length,vector};
    if vertices.len()>crate::theatre::MAX_TRIANGLES*3 || vertices.len()%3!=0{return Err("theatre mesh budget exceeded".into());}
    let eye=vector(&input["position"])?;
    let camera=sub(eye,origin);
    let lighting=crate::base_system::layer_four::atmosphere::Lighting::from_view(input);
    let mut local=Vec::with_capacity(vertices.len());
    for triangle in vertices.chunks_exact(3) {
        let p: [[f64;3];3]=std::array::from_fn(|i|std::array::from_fn(|j|triangle[i][j] as f64));
        let center=mul(add(add(p[0],p[1]),p[2]),1./3.);
        let mut normal=unit(cross(sub(p[1],p[0]),sub(p[2],p[0])));
        if dot(normal,sub(camera,center))<0.{normal=mul(normal,-1.);}
        let inside=center[0].hypot(center[2])<75.8 && center[1]<17.9;
        let tag = triangle[0][6];
        let metres = match tag { 102.0 => 2.0, 103.0 => 3.0, 114.0 => 0.08, 104.0 => 0.55, _ => 1.0 };
        let axes = normal.map(f64::abs);
        for (v,p) in triangle.iter().zip(p) {
            if (106.0..=113.0).contains(&tag) || (115.0..=120.0).contains(&tag) {
                let r=sub(p,camera);
                local.push([r[0] as f32,r[1] as f32,r[2] as f32,v[3],v[4],v[5]]);
                continue;
            }
            // Evaluate smooth illumination at shared vertices, avoiding visible
            // lighting diagonals across large Blender-authored planar surfaces.
            let mut shade=if inside{[0.34,0.30,0.26]}else{lighting.tint.map(|n|n*0.25*lighting.brightness())};
            if inside {
                for lamp in [[-23.,15.,-24.],[23.,15.,-24.],[-18.,15.,18.],[18.,15.,18.],[0.,13.,-49.]] {
                    let delta=sub(lamp,p);let gain=dot(normal,unit(delta)).max(0.)*1.2/(1.+dot(delta,delta)/250.);
                    for i in 0..3{shade[i]+=gain*[1.,0.86,0.68][i];}
                }
            }else{
                let gain=dot(normal,lighting.primary).max(0.)*lighting.brightness();
                for i in 0..3{shade[i]+=gain*lighting.tint[i];}
            }
            if tag >= 100.0 {
                let uv = if axes[1] >= axes[0].max(axes[2]) { [p[0], p[2]] }
                    else if axes[0] >= axes[2] { [p[2], -p[1]] } else { [p[0], -p[1]] };
                let r = sub(p,camera);
                local.push([r[0] as f32,r[1] as f32,r[2] as f32,(uv[0]/metres) as f32,
                    (uv[1]/metres) as f32,(shade[0]*0.35+shade[1]*0.55+shade[2]*0.10) as f32]);
                continue;
            }
            let relative=sub(p,camera);
            local.push([relative[0]as f32,relative[1]as f32,relative[2]as f32,
                (v[3]as f64*shade[0]).min(1.)as f32,(v[4]as f64*shade[1]).min(1.)as f32,(v[5]as f64*shade[2]).min(1.)as f32]);
        }
    }
    let mut view=input.clone();view["position"]=serde_json::json!([0.,0.,0.]);view["two_sided"]=serde_json::json!(true);
    view["far"]=serde_json::json!(length(eye)+2.*crate::terrain::RADIUS);
    let mut projected = projection::project(&local,&view);
    // two_sided projection preserves triangle order.
    for (face, triangle) in projected.iter_mut().zip(vertices.chunks_exact(3)) {
        let tag=triangle[0][6];
        if tag >= 100.0 { for i in [15,19,23] { face[i]=tag; } }
    }
    Ok(projected)
}

#[test]
fn cached_venue_matches_original_across_views_light_and_replacement() {
    let venue = crate::theatre::load(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    let mut vertices = venue.vertices.clone();
    let origin = [0.,crate::terrain::RADIUS,0.];
    for (index, camera) in [[0.,6.,-52.],[0.,25.,100.],[30.,8.,20.],[-40.,8.,-20.]].into_iter().enumerate() {
        for fraction in [0.,0.25,0.5,0.75] {
            let input = serde_json::json!({"position":add(origin,camera),"yaw":0.2,"pitch":-0.1,"aspect":1.6,"day_night":{"fraction":fraction}});
            let expected: Vec<_> = reference(origin,&vertices,&input).unwrap().into_iter()
                .filter(super::super::projection::in_frustum).collect();
            for _ in 0..2 { assert_eq!(project(origin,&vertices,&input).unwrap(),expected,"camera {index}, fraction {fraction}"); }
        }
        if !vertices.is_empty() { vertices[0][1] += 0.125; }
    }
}
}

#[cfg(test)]mod reflection_culling_tests {
 use super::*;
 #[test]fn offscreen_venue_is_preserved_when_planar_reflections_need_it(){
  let origin=[0.,crate::terrain::RADIUS,0.];
  let vertices=vec![[0.,0.,10.,1.,1.,1.,102.],[1.,0.,10.,1.,1.,1.,102.],[0.,1.,10.,1.,1.,1.,102.]];
  let mut view=serde_json::json!({"position":origin,"yaw":0.,"pitch":0.,"aspect":1.6,"retain_offscreen_reflections":false});
  assert!(project(origin,&vertices,&view).unwrap().is_empty());
  view["retain_offscreen_reflections"]=serde_json::json!(true);
  assert_eq!(project(origin,&vertices,&view).unwrap().len(),1);
 }
}

#[cfg(test)]mod hierarchy_tests {
 use super::*;
 #[test]fn hierarchy_never_drops_an_original_visible_face_and_retains_order(){
  let vertices:Vec<[f32;7]>=(0..4096).flat_map(|i|{let x=((i*73)%501)as f32-250.;let y=((i*31)%151)as f32-75.;let z=((i*59)%601)as f32-300.;[[x,y,z,1.,1.,1.,0.],[x+3.,y,z,1.,1.,1.,0.],[x,y+4.,z+1.,1.,1.,1.,0.]]}).collect();
  let cache=prepare(&vertices);
  for n in 0..48 {let eye=[(n as f64*0.7).sin()*300.,(n as f64*0.3).cos()*100.,(n as f64*0.7).cos()*300.];
   let view=serde_json::json!({"position":[0.,0.,0.],"yaw":n as f64*0.31,"pitch":(n as f64*0.4).sin(),"aspect":if n%2==0{1.6}else{0.7},"two_sided":true});
   let p=super::super::projection::Projector::new(&view);
   let mut candidate=vec![false;cache.faces.len()];let mut previous=0;
   for range in visible_ranges(&cache,eye,&p,true){assert!(range.start>=previous);previous=range.end;for i in range {candidate[i]=true;}}
   for (i,t)in vertices.chunks_exact(3).enumerate(){let local: [[f32;6];3]=std::array::from_fn(|j|std::array::from_fn(|k|if k<3{(t[j][k]as f64-eye[k])as f32}else{0.}));let projected=p.triangle(&local).unwrap();if super::super::projection::in_frustum(&projected){assert!(candidate[i],"view {n}, face {i}");}}
  }
 }
}

#[cfg(test)]mod reflected_hierarchy_tests {
 use super::*;
 #[test]fn reflection_union_keeps_every_primary_or_reflected_visible_face(){
  let vertices:Vec<[f32;7]>=(0..2048).flat_map(|i|{let x=((i*73)%501)as f32-250.;let y=((i*31)%151)as f32-75.;let z=((i*59)%601)as f32-300.;[[x,y,z,1.,1.,1.,0.],[x+3.,y,z,1.,1.,1.,0.],[x,y+4.,z+1.,1.,1.,1.,0.]]}).collect();
  let cache=prepare(&vertices);
  for n in 0..24 {
   let eye=[(n as f64*0.7).sin()*300.,30.,(n as f64*0.7).cos()*300.];
   let view=serde_json::json!({"position":[0.,0.,0.],"yaw":n as f64*0.31,"pitch":(n as f64*0.4).sin(),"aspect":1.6,"two_sided":true});
   let projector=super::super::projection::Projector::new(&view);
   let planes=[[[0.,1.,0.],[0.,-10.,0.]],[[0.01,0.9999,0.01],[25.,12.,30.]]];
   let mut retained=vec![false;vertices.len()/3];for range in visible_reflected_ranges(&cache,eye,&projector,true,&planes){for i in range{retained[i]=true;}}
   for (i,t) in vertices.chunks_exact(3).enumerate(){
    let visible=|plane:Option<[V;2]>| {
     let t:Vec<_>=t.iter().map(|v|{let p=[v[0]as f64,v[1]as f64,v[2]as f64];let p=if let Some(plane)=plane{let normal=unit(plane[0]);sub(p,mul(normal,2.*dot(normal,sub(p,plane[1]))))}else{p};let p=sub(p,eye);[p[0]as f32,p[1]as f32,p[2]as f32,1.,1.,1.]}).collect();
     super::super::projection::in_frustum(&projector.triangle(&t).unwrap())
    };
    if visible(None)||planes.iter().any(|p|visible(Some(*p))){assert!(retained[i],"reflection face {i} lost in view {n}");}
   }
  }
 }
}

#[cfg(test)]mod auditorium_occlusion_tests {
 use super::*;
 #[test]fn auditorium_interior_and_front_portal_are_preserved(){
  assert!(!auditorium_hidden([0.,10.,0.]));assert!(!auditorium_hidden([0.,12.,-100.]));
  assert!(!auditorium_hidden([120.,30.,-60.]));
  assert!(auditorium_hidden([580.,80.,355.]));assert!(auditorium_hidden([0.,30.,0.]));assert!(auditorium_hidden([0.,-5.,0.]));
 }
}
