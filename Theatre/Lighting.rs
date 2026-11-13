//! Fixed Cardinal cone/range attenuation, evaluated once for authored stage vertices.
//! RGB/intensity are cheap control-side coefficients, never audio callback work.
use crate::audio::{sub,unit,dot,length};
use serde_json::{Value,json};
use std::sync::OnceLock;
pub(crate) const COUNT:usize=6;
pub(crate) type Levels=[[f64;4];COUNT]; // RGB and normalized intensity.
#[derive(Debug)]
pub(crate) struct Fixture {pub id:u64,pub label:String,pub kind:String,pub position:[f64;3],pub direction:[f64;3],pub color:[f64;3],pub intensity:f64,pub range:f64,pub inner:f64,pub outer:f64}
pub(crate) fn fixtures()->&'static [Fixture] {
 static DATA:OnceLock<Vec<Fixture>>=OnceLock::new();DATA.get_or_init(||{
  let v:Value=serde_json::from_str(include_str!("../Procedures/theatre_lighting.json")).expect("authored lighting JSON");
  v["fixtures"].as_array().unwrap().iter().map(|f|{
   let position=crate::audio::vector(&f["position"]).unwrap();
   Fixture{id:f["id"].as_u64().unwrap(),label:f["label"].as_str().unwrap().into(),kind:f["kind"].as_str().unwrap().into(),position,
    direction:unit(sub(crate::audio::vector(&f["target"]).unwrap(),position)),color:crate::audio::vector(&f["color"]).unwrap(),intensity:f["intensity"].as_f64().unwrap(),range:f["range"].as_f64().unwrap(),
    inner:f["inner_degrees"].as_f64().unwrap().to_radians().cos(),outer:f["outer_degrees"].as_f64().unwrap().to_radians().cos()}
  }).collect()
 })
}
pub(crate) fn defaults()->Levels {std::array::from_fn(|i|{let f=&fixtures()[i];[f.color[0],f.color[1],f.color[2],f.intensity]})}
pub(crate) fn levels(view:&Value)->Levels {
 serde_json::from_value::<Levels>(view["stage_lighting"].clone()).ok().filter(|v|v.iter().flatten().all(|n|n.is_finite()&&(0.0..=1.).contains(n))).unwrap_or_else(defaults)
}
pub(crate) fn catalog()->&'static Value {
 static C:OnceLock<Value>=OnceLock::new();C.get_or_init(||json!({"devices":fixtures().iter().map(|f|json!({"id":f.id,"label":format!("LIGHT {} {}",f.id,f.label),"workspace":true,
 "parameters":[{"id":0,"label":"INTENSITY","writable":true},{"id":1,"label":"RED","writable":true},{"id":2,"label":"GREEN","writable":true},{"id":3,"label":"BLUE","writable":true}]})).collect::<Vec<_>>()}))
}
/// Fixtures are aimed at the unobstructed stage deck/backing. Limit their
/// receiving volume explicitly; no fictitious light transmission through walls.
pub(crate) fn weights(p:[f64;3],n:[f64;3])->[f64;COUNT] {
 if p[0].abs()>34.||p[2]<19.8||p[2]>50.1||p[1]<3.||p[1]>17. {return [0.;COUNT];}
 std::array::from_fn(|i|{
  let f=&fixtures()[i];let d=sub(p,f.position);let distance=length(d);
  if distance<0.05||distance>=f.range{return 0.;}
  let ray=unit(d);let cos=dot(ray,f.direction);
  let edge=((cos-f.outer)/(f.inner-f.outer)).clamp(0.,1.);let cone=edge*edge*(3.-2.*edge);
  let fade=(1.-distance/f.range).max(0.);let diffuse=(-dot(n,ray)).abs();
  cone*fade*fade*diffuse*6./(1.+distance*distance/250.)
 })
}
pub(crate) fn shade(weights:[f64;COUNT],levels:&Levels)->[f64;3] {
 std::array::from_fn(|c|(0..COUNT).map(|i|weights[i]*levels[i][c]*levels[i][3]).sum())
}
#[cfg(test)]mod tests {
 use super::*;
 #[test]fn fixed_cones_are_bounded_and_leave_the_room_outside_the_stage_dark(){
  assert_eq!(fixtures().len(),COUNT);assert_eq!(weights([0.,0.,-40.],[0.,1.,0.]),[0.;COUNT]);
  let f=&fixtures()[0];let p=std::array::from_fn(|j|f.position[j]+f.direction[j]*15.);
  assert!(weights(p,[0.,1.,0.])[0]>0.);assert_eq!(shade(weights(p,[0.,1.,0.]),&[[0.;4];COUNT]),[0.;3]);
  assert_eq!(levels(&json!({"stage_lighting":[[999]]})),defaults());
 }
}

/// Add authored fixed fixtures and refine only the stage deck's top surface.
/// This is decorative optical geometry, not extra collision/acoustic facets.
pub(crate) fn install(root:&std::path::Path,vertices:&mut Vec<[f32;7]>)->Result<(),String> {
 let packet=super::read(root,"Procedures/Assets/Theatre/stage_lighting_mesh.json")?;
 if packet["schema_version"]!=1||packet["coordinate_system"]!="metres_x_right_y_up_z_front"{return Err("invalid lighting mesh schema".into());}
 let rows=packet["vertices"].as_array().filter(|a|!a.is_empty()&&a.len()%3==0&&a.len()<=78_000).ok_or("invalid lighting mesh budget")?;
 let mut mesh=Vec::with_capacity(rows.len());
 for row in rows {
  let v:[f32;7]=serde_json::from_value(row.clone()).map_err(|_|"invalid lighting vertex")?;
  if !v.iter().all(|v|v.is_finite())||v[0].abs()>35.||!(2.0..=18.).contains(&v[1])||!(15.0..=50.).contains(&v[2])||v[3..6].iter().any(|n|!(0.0..=1.).contains(n))||!(v[6]==0.||(201.0..=206.).contains(&v[6])&&v[6].fract()==0.){return Err("unbounded lighting mesh vertex".into());}
  mesh.push(v);
 }
 // Actual deck top consists of 64 separate boards. Preserve its exact extents,
 // gaps and colors, adding shared vertices every half metre along the boards.
 let mut retained=Vec::with_capacity(vertices.len()+24_000);
 let mut boards=std::collections::BTreeMap::<i32,([f32;3],[f32;3],[f32;3])>::new();
 for tri in vertices.chunks_exact(3) {
  let deck=tri.iter().all(|v|(v[1]-3.036).abs()<0.001&&v[0].abs()<24.&&v[2]>20.&&v[2]<50.&&v[6]==0.);
  if !deck{retained.extend_from_slice(tri);continue;}
  let key=((tri.iter().map(|v|v[0]).sum::<f32>()/3.+24.)/0.75).floor() as i32;
  let entry=boards.entry(key).or_insert(([f32::INFINITY;3],[f32::NEG_INFINITY;3],[tri[0][3],tri[0][4],tri[0][5]]));
  for v in tri{for j in 0..3{entry.0[j]=entry.0[j].min(v[j]);entry.1[j]=entry.1[j].max(v[j]);}}
 }
 if boards.len()!=64{return Err(format!("stage lighting expects 64 authored stage boards, found {}",boards.len()));}
 for (_,(a,b,c)) in boards {
  let steps=((b[2]-a[2])/0.5).ceil() as usize;
  for step in 0..steps {
   let z0=a[2]+(b[2]-a[2])*step as f32/steps as f32;let z1=a[2]+(b[2]-a[2])*(step+1) as f32/steps as f32;
   for p in [[a[0],a[1],z0],[a[0],a[1],z1],[b[0],a[1],z1],[a[0],a[1],z0],[b[0],a[1],z1],[b[0],a[1],z0]] {
    retained.push([p[0],p[1],p[2],c[0],c[1],c[2],0.]);
   }
  }
 }
 retained.extend(mesh);*vertices=retained;Ok(())
}
