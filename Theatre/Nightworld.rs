//! Immutable Blender render/collision asset, shared with scene workers.
use crate::audio::{V,sub};
use crate::terrain::{nightworld as realm, RADIUS};
use serde_json::{Value,json};
use std::path::Path;
pub(crate) struct Site {
 pub over:Vec<[f32;7]>,pub night:Vec<[f32;7]>,portal:Vec<[f32;7]>,
 over_collision:super::waterpark::Park,night_collision:super::waterpark::Park,
}
impl Site {
 pub(crate) fn load(root:&Path)->Result<Self,String>{
  let bytes=std::fs::read(root.join("Procedures/Assets/Nightworld/site.json")).map_err(|e|format!("Nightworld asset: {e}"))?;
  if bytes.len()>32*1024*1024{return Err("Nightworld asset byte budget".into());}
  let data:Value=serde_json::from_slice(&bytes).map_err(|e|e.to_string())?;
  if data["schema_version"]!=1{return Err("unsupported Nightworld asset".into());}
  let origin=super::waterpark::theatre_origin();let offset=super::waterpark::site_offset(origin);
  let datum=origin[1]-RADIUS+offset[1];
  if (data["datum"].as_f64().ok_or("Nightworld datum required")?-datum).abs()>0.02{return Err("Nightworld tunnel must be rebuilt for the current waterpark datum".into());}
  let vertices=|key:&str|->Result<Vec<[f32;7]>,String>{let rows:Vec<[f32;7]>=serde_json::from_value(data[key].clone()).map_err(|e|e.to_string())?;if rows.len()%3!=0||rows.len()>180000||rows.iter().flatten().any(|v|!v.is_finite()){return Err("Nightworld vertex budget".into());}Ok(rows)};
  let collision=|key:&str|->Result<super::waterpark::Park,String>{let rows:Vec<[V;3]>=serde_json::from_value(data[key].clone()).map_err(|e|e.to_string())?;if rows.len()>60000||rows.iter().flatten().flatten().any(|v|!v.is_finite()){return Err("Nightworld collision budget".into());}Ok(super::waterpark::Park::collision_mesh(rows))};
  Ok(Self{over:vertices("overworld")?,night:vertices("night")?,portal:vertices("portal")?,over_collision:collision("collision_overworld")?,night_collision:collision("collision_night")?})
 }
 pub(crate) fn resolve(&self,previous:V,desired:V,eye:f64)->(V,Vec<V>){
  if realm::active(previous)!=realm::active(desired){return(desired,Vec::new());}
  let night=realm::active(desired);let c=realm::shaft();
  let convert=|p:V|{let mut q=realm::local(p);q[0]-=c[0];q[2]-=c[2];q};
  let a=convert(previous);let b=convert(desired);
  if b[0].abs()>10. || b[2]< -10. || b[2]>if night{10.}else{36.}{return(desired,Vec::new());}
  let (mut q,normals)=if night{self.night_collision.resolve(a,b,eye)}else{self.over_collision.resolve(a,b,eye)};
  q[0]+=c[0];q[2]+=c[2];let position=if night{realm::world(q)}else{crate::terrain::underground::world(q)};
  (position,normals.into_iter().map(|n|if night{[n[0],-n[1],n[2]]}else{n}).collect())
 }
 pub(crate) fn collision_vertices(&self,focus:V)->Vec<V>{self.collision_vertices_radius(focus,24.)}
 pub(crate) fn collision_vertices_radius(&self,focus:V,radius:f64)->Vec<V>{
  let c=realm::shaft();let mut q=realm::local(focus);q[0]-=c[0];q[2]-=c[2];
  let night=realm::active(focus);let park=if night{&self.night_collision}else{&self.over_collision};
  park.collision_vertices_radius(q,[0.;3],radius).into_iter().map(|mut p|{p[0]+=c[0];p[2]+=c[2];if night{realm::world(p)}else{crate::terrain::underground::world(p)}}).collect()
 }
 /// Subtract the world origin in f64 before packing graphics vertices.
 pub(crate) fn geometry(&self,view:&Value)->Result<(Vec<[f32;6]>,Value),String>{
  let eye=crate::audio::vector(&view["position"])?;let night=realm::active(eye);let c=realm::shaft();
  let origin=[c[0],if night{-RADIUS}else{RADIUS},c[2]];
  let local_eye=sub(eye,origin);
  let mut extra=Vec::new();
  let q=realm::local(eye);
  if q[1]< -85. && realm::shaft_radius(q)>8. {
   let x=(eye[0]/8.).round()*8.-c[0];let z=(eye[2]/8.).round()*8.-c[2];
   extra.extend(self.portal.iter().map(|v|{let mut v=*v;v[0]+=x as f32;v[2]+=z as f32;v}));
  }
  let rows=if night{&self.night}else{&self.over};
  let light=crate::base_system::layer_four::atmosphere::Lighting::from_view(view);
  let combined:Vec<_>=rows.iter().chain(extra.iter()).copied().collect();
  let vertices=combined.chunks_exact(3).filter(|t|{
   let p=[t[0][0] as f64,if night{-(t[0][1] as f64)}else{t[0][1] as f64},t[0][2] as f64];crate::audio::length(sub(p,local_eye))<200.
  }).flat_map(|t|{
   let p:[V;3]=std::array::from_fn(|i|[t[i][0] as f64,if night{-(t[i][1] as f64)}else{t[i][1] as f64},t[i][2] as f64]);
   let normal=crate::audio::unit(crate::audio::cross(sub(p[1],p[0]),sub(p[2],p[0])));
   let brightness=0.32+0.45*crate::audio::dot(normal,light.primary).abs()*light.daylight;
   std::array::from_fn::<_,3,_>(|i|[p[i][0] as f32,p[i][1] as f32,p[i][2] as f32,t[i][3]*brightness as f32,t[i][4]*brightness as f32,t[i][5]*brightness as f32])
  }).collect();
  let mut v=view.clone();v["position"]=json!(local_eye);v["two_sided"]=json!(true);v["far"]=json!(2000.);
  Ok((vertices,v))
 }
}

#[cfg(test)] mod tests {
 use super::*;
 fn site()->Site{Site::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap()}
 #[test] fn nightworld_blender_platforms_support_landings_without_auto_climbing(){
  let site=site();let c=realm::shaft();let eye=1.84;
  let place=|x,y,z|realm::world([c[0]+x,y,c[2]+z]);
  // First platform top is -93.84. Fall onto its center and recover a real
  // support normal in the inverted world chart.
  let (p,n)=site.resolve(place(4.1,-93.84+eye+0.3,0.),place(4.1,-93.84+eye-0.15,0.),eye);
  assert!((realm::local(p)[1]-(-93.84+eye)).abs()<0.04,"{:?}",realm::local(p));
  assert!(n.iter().any(|v|v[1]< -0.9));
  // 0.72 m rise cannot be consumed by the 0.205 m step helper.
  let a=0.37f64;let (_,normals)=site.resolve(place(4.1,-93.84+eye,0.),place(4.1*a.cos(),-93.84+eye,4.1*a.sin()),eye);
  assert!(normals.iter().any(|n|n[1].abs()<0.8));
 }
 #[test] fn nightworld_secret_passage_and_arrival_have_geological_clearance(){
  let data:Value=serde_json::from_str(include_str!("../Procedures/caves.json")).unwrap();
  let f=crate::terrain::underground::Field::parse(&data).unwrap().unwrap();let c=realm::shaft();
  let origin=super::super::waterpark::theatre_origin();let off=super::super::waterpark::site_offset(origin);
  let top=origin[1]-RADIUS+off[1]-86.;
  for i in 0..=32 {let t=i as f64/32.;let p=crate::terrain::underground::world([c[0],top+(-94.45-top)*t+1.5,c[2]+33.*(1.-t)]);assert!(f.distance(p,crate::terrain::height(crate::audio::unit(p)))>0.3,"blocked tunnel {i}");}
  for y in [-94.,-60.,-25.,0.,30.] {let p=realm::world([c[0],y,c[2]]);assert!(f.distance(p,crate::terrain::height(crate::audio::unit(p)))>0.);}
 }
 #[test] fn nightworld_asset_projection_is_finite_and_relative_to_eye(){
  let s=site();let c=realm::shaft();let v=json!({"entity":1,"position":realm::world([c[0],-92.,c[2]]),"forward":[0.,0.,-1.],"up":[0.,-1.,0.],"right":[-1.,0.,0.],"yaw":0.,"pitch":0.,"aspect":1.6,"day_night":{"fraction":0.5}});
  let (vertices,view)=s.geometry(&v).unwrap();assert!(vertices.len()>1000);assert!(vertices.iter().flatten().all(|v|v.is_finite()));assert!(view["position"][1].as_f64().unwrap().abs()<100.);
 }
}

#[cfg(test)] mod nightworld_route_tests {
 use super::*;
 #[test] fn nightworld_parkour_route_reaches_above_the_tube_rim(){
  let root=Path::new(env!("CARGO_MANIFEST_DIR"));
  let data:Value=serde_json::from_slice(&std::fs::read(root.join("Procedures/Assets/Nightworld/site.json")).unwrap()).unwrap();
  let blocks:Vec<_>=data["boxes"].as_array().unwrap().iter().filter(|b|b["name"].as_str().is_some_and(|s|s.starts_with("Suspended parkour block"))).collect();
  assert_eq!(blocks.len(),222);
  let center=|b:&Value|->V{std::array::from_fn(|i|(b["min"][i].as_f64().unwrap()+b["max"][i].as_f64().unwrap())*0.5)};
  let jump=8f64;let gravity=21f64;
  for pair in blocks.windows(2){let a=center(pair[0]);let b=center(pair[1]);let rise=b[1]-a[1];assert!((0.70..0.74).contains(&rise));
   let flight=(jump+(jump*jump-2.*gravity*rise).sqrt())/gravity;
   assert!((b[0]-a[0]).hypot(b[2]-a[2])<5.04*flight,"platform must be reachable at ordinary walking speed");
  }
  assert!(blocks.last().unwrap()["max"][1].as_f64().unwrap()>65.,"the final platforms must clear the solid tube rim");
 }
}
