//! Native screenshot review of the actual shared scene builder and water pass.
use super::Session;
use crate::audio::{V,sub,unit,cross};
use crate::terrain::{nightworld as realm,RADIUS};
use serde_json::json;
use std::path::Path;
pub(super) fn run(root:&Path,output:&Path,backend:&str)->Result<(),String>{
 std::fs::create_dir_all(output).map_err(|e|e.to_string())?;
 let entity=crate::base_entity::Entity::from_spec(1,&json!({"kind":"visual","shader":"planet_surface","vertices":[[-1.,-1.,0.,1.,1.,1.],[1.,-1.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]]}))?;
 let mut session=Session::prepare(root,output,backend,&entity)?;session.environment("planet_surface");session.texture(&[1,0,0,0,1,0,0,0,255,255,255,255])?;
 let c=realm::shaft();let point=|p:V|realm::world([c[0]+p[0],p[1],c[2]+p[2]]);
 let cases=[("nightworld-landing",point([0.,-90.,1.8]),point([0.,-95.2,0.]),false),
 ("nightworld-shaft",point([0.,-45.,0.]),point([4.,-30.,-3.]),false),
 ("nightworld-dunes",point([35.,60.,55.]),point([130.,25.,-140.]),false),
 ("nightworld-loading",point([0.,-92.7,0.]),point([4.,-89.,-3.]),true)];
 let mut report=Vec::new();
 for (name,eye,target,loading) in cases {
  let forward=unit(sub(target,eye));let right=unit(cross(forward,unit(eye)));let up=unit(cross(right,forward));
  let mut view=json!({"entity":1,"position":eye,"forward":forward,"right":right,"up":up,"yaw":0.,"pitch":0.,"aspect":1.5,"day_night":{"fraction":0.5},"nightworld_loading":if loading{0.4}else{0.}});
  view["clouds"]=serde_json::Value::Null;
  let scene=crate::base_system::layer_four::appearance_system::nightworld_review_scene(root,view.clone())?;
  session.table_eye(&scene.view)?;session.set_batch(&scene.triangles)?;
  if !loading {
   let bodies=crate::water::bodies::State::parse(&json!({"ocean":false,"generated":false,"rivers":[],"sources":[],"pools":[{"min":[c[0]-4.7,-(RADIUS-94.6),c[2]-4.7],"max":[c[0]+4.7,-(RADIUS-95.6),c[2]+4.7]}]}))?;
   let (frame,mesh)=crate::water::surface_mesh::free_surfaces(eye,forward,&bodies,0.,&Default::default(),&Default::default());
   let mut packet=crate::water::runtime::mesh_packet(frame,&mesh);let o=crate::water::optics::Settings::default();
   packet["optics"]=json!({"time":2.,"murkiness":0.,"sky_color_intensity":o.sky_color_intensity,"neutral_reflection_intensity":o.neutral_reflection_intensity,"sky_top":o.sky_top,"sky_bottom":o.sky_bottom,"light":o.light,"planar_reflection":false,"micro_phase":[0.,0.,0.],"mean_height":0.});packet["ripples"]=json!([]);
   session.set_water(root,&json!([packet]),&scene.view,&scene.triangles)?;
  }else{session.set_water(root,&json!([]),&scene.view,&scene.triangles)?;}
  session.capture_next();let frame=session.pump(true,true)?;
  std::fs::copy(output.join(format!("triangle-{backend}.ppm")),output.join(format!("{name}.ppm"))).map_err(|e|e.to_string())?;
  report.push(json!({"name":name,"triangles":scene.triangles.len(),"frame":frame}));
 }
 session.finish()?;std::fs::write(output.join("nightworld-review.json"),serde_json::to_vec_pretty(&report).unwrap()).map_err(|e|e.to_string())?;Ok(())
}
