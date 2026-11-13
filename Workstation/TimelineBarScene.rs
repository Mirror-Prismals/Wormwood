//! Two original Cardinal ruler gem faces; the old LOOP slot is reserved.
//! Reuses the approved optical_shape cut, top-panel material and studio light.
//! No mesh stretching, new material, asset replacement or cache redesign.
use super::{engraving,panel_layout::{self,Viewport,Options},panel_motion::Snapshot,piano_roll::Rect};
use serde_json::Value;
// Existing packet gap: monitors own [368,432), Edison owns [448,512).
// Keep every immutable geometry/density address and FRAME_ROWS unchanged.
pub(crate) const BASE:usize=engraving::BASE+432;
pub(crate) const ROWS:usize=10;
pub(crate) const TAG:f32=10532.;
const _: () = assert!(BASE+ROWS <= super::transport_pose::META_BASE);
pub(crate) const ID:i32=720000;
fn rect(v:&Value)->Result<Rect,String>{
 let a=v.as_array().filter(|a|a.len()==4).ok_or("timeline bar rectangle required")?;
 let mut r=[0.;4];for(i,v)in a.iter().enumerate(){r[i]=v.as_f64().filter(|x|x.is_finite()&&x.abs()<1e6).ok_or("invalid timeline bar rectangle")?;}
 if r[2]<=0.||r[3]<=0.{return Err("empty timeline bar rectangle".into());}
 Ok(Rect{left:r[0],top:r[1],width:r[2],height:r[3]})
}
pub(crate) fn append(rows:&mut[[f32;4]],view:Viewport,motion:&Snapshot,_options:Options,timeline:&Value,control:&Value)->Result<(),String>{
 if rows.len()<BASE+ROWS{return Err("timeline bar packet missing reserved rows".into());}
 rows[BASE..BASE+ROWS].fill([0.;4]);
 // Full/extended panels can leave no timeline body during their transition.
 // Empty display geometry is hidden, not an invalid application command.
 if !motion.timeline_visible() || timeline["bar_body"].as_array().is_some_and(|r|
     r.get(2).and_then(Value::as_f64).is_some_and(|w|w<=0.) ||
     r.get(3).and_then(Value::as_f64).is_some_and(|h|h<=0.)) {return Ok(());}

 let mpp=1.3/view.height;
 let reference_unit=(rows[35][0].min(rows[35][1]) as f64-0.000001).max(1e-12);
 let world=|x:f64,y:f64|[(x-view.width*0.5)*mpp,(view.height*0.5-y)*mpp];
 let bars=&timeline["timeline_bars"];
 let mut faces=[None,None,None];
 if bars.is_object(){faces[0]=Some(rect(&bars["zoom"])?);faces[1]=Some(rect(&bars["loop"])?);faces[2]=bars.get("vertical").filter(|v|v.is_array()).map(rect).transpose()?;}
 // The loop toggle now belongs to right-panel page two; its old slot stays empty.
 for(i,r)in faces.into_iter().enumerate(){if let Some(r)=r{
  let shape=panel_layout::optical_shape(r.width*mpp,r.height*mpp)?;
  let mut c=world(r.left+r.width*0.5,r.top+r.height*0.5);let u=shape.local_unit_m;
  let press=0.;
  if i==2{c[0]-=10.*press*mpp;}
  // Existing top panel/control plane is z=0; float each new table just above it.
  let z=0.001+shape.dimensions_m[2]-press*u*0.5;
  rows[BASE+i]=[c[0] as f32,c[1] as f32,z as f32,u as f32];
  rows[BASE+3+i]=[(r.width*mpp/(2.*u))as f32,(r.height*mpp/(2.*u))as f32,(u/reference_unit)as f32,1.];
 }}
 rows[BASE+8]=[32.,0.,TAG,0.];
 if bars.is_object(){
  let body=if bars.is_object()&&timeline["bar_body"].is_array(){rect(&timeline["bar_body"])?}else{super::arrangement::clip_body(Rect{left:timeline["canvas"]["left"].as_f64().unwrap_or(0.),top:timeline["canvas"]["top"].as_f64().unwrap_or(0.),width:timeline["canvas"]["width"].as_f64().unwrap_or(0.),height:timeline["canvas"]["height"].as_f64().unwrap_or(0.)})};
  let a=bars["loop_start_seconds"].as_f64().unwrap_or(0.);let b=bars["loop_end_seconds"].as_f64().unwrap_or(0.);
  let start=timeline["start_seconds"].as_f64().unwrap_or(0.);let span=timeline["seconds_per_width"].as_f64().filter(|s|*s>0.).unwrap_or(1.);
  let x=|t:f64|world(body.left+(t-start)/span*body.width,0.)[0];
  rows[BASE+6]=[x(a)as f32,x(b)as f32,world(0.,body.top)[1]as f32,world(0.,body.top+body.height)[1]as f32];
  rows[BASE+7][0]=if bars["loop_enabled"]==true{1.}else{0.};
  if let Ok(h)=rect(&bars["playhead_handle"]){rows[BASE+7][1]=(world(0.,h.top+h.height*0.5)[1])as f32;rows[BASE+7][2]=(h.height*0.5*mpp)as f32;rows[BASE+7][3]=world(h.left+h.width*0.5,0.)[0]as f32;rows[BASE+8][1]=1.;}
  rows[BASE+9]=[world(body.left,0.)[0]as f32,world(body.left+body.width,0.)[0]as f32,mpp as f32,1.];
 }
 if rows[BASE..BASE+ROWS].iter().flatten().any(|v|!v.is_finite()){return Err("nonfinite timeline bar packet".into());}
 rows[BASE+8][3]=faces.iter().filter(|r|r.is_some()).count()as f32;
 Ok(())
}

#[cfg(test)]mod tests{
 use super::*;use serde_json::json;
 #[test]fn faces_use_original_cut_shared_material_and_only_reserved_packet_space(){
  let view=Viewport{width:1280.,height:720.,canvas_scale:1.};let motion=super::super::panel_motion::State::default().snapshot();
  let mut rows=vec![[0.;4];BASE+ROWS+8];rows[35]=[0.2,0.05,0.,0.];rows[255]=[11.,12.,13.,14.];rows[BASE-1]=[7.;4];rows[BASE+ROWS]=[9.;4];
  let t=json!({"canvas":{"left":100.,"top":200.,"width":900.,"height":400.},"start_seconds":0.,"seconds_per_width":16.,"timeline_bars":{"zoom":[213.,223.,745.,26.],"loop":[228.,200.,446.875,13.],"playhead_handle":[222.,243.,12.,12.],"loop_start_seconds":0.,"loop_end_seconds":10.,"loop_enabled":true}});
  append(&mut rows,view,&motion,Options::default(),&t,&json!({"pressure":0.})).unwrap();
  assert_eq!(rows[BASE+8],[32.,1.,TAG,2.]);assert_eq!(rows[BASE+8][0],32.);assert_eq!(rows[BASE-1],[7.;4]);assert_eq!(rows[BASE+ROWS],[9.;4]);assert_eq!(rows[255],[11.,12.,13.,14.]);
  assert_eq!(rows[BASE+3][1],1.);assert_eq!(rows[BASE+4][1],1.);assert_eq!(rows[BASE+7][0],1.);assert_eq!(rows[BASE+8][1],1.);
  let neutral=rows[BASE+2];append(&mut rows,view,&motion,Options::default(),&t,&json!({"pressure":0.5})).unwrap();assert_eq!(rows[BASE+2],neutral);assert_eq!(neutral,[0.;4]);
  append(&mut rows,view,&motion,Options::default(),&Value::Null,&json!({"pressure":0.})).unwrap();assert_eq!(rows[BASE+8][3],0.);assert_eq!(rows[BASE],[0.;4]);assert_eq!(rows[BASE+7][3],0.);assert_eq!(rows[BASE+8][1],0.);
 }
}
