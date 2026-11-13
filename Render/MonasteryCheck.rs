//! Native production-shader views of monastery geometry and incense stages.
use super::Session;
use crate::audio::{add,sub,unit,cross};
use serde_json::json;
use std::{path::Path,sync::Arc};
pub(super) fn run(root:&Path,output:&Path,backend:&str)->Result<(),String>{
 std::fs::create_dir_all(output).map_err(|e|e.to_string())?;
 let origin=crate::theatre::waterpark::theatre_origin();let mut v=crate::theatre::load(root)?;
 let n=unit([0.,crate::terrain::RADIUS,-94.]);let outside=(n[1]*(crate::terrain::RADIUS+crate::terrain::height(n))-origin[1]).min(0.);
 v.add_approach(outside)?;v.add_waterpark(root,crate::theatre::waterpark::site_offset(origin),outside,origin)?;v.add_town(root,origin)?;
 let y=v.add_civic_buildings(root,origin,outside)?;v.add_marsh(root,origin,y)?;v.add_plan(root,origin,y)?;v.add_laboratory(root,origin)?;v.add_monastery(origin)?;
 let v=Arc::new(v);let site=v.monastery.unwrap();
 let entity=crate::base_entity::Entity::from_spec(1,&json!({"kind":"visual","shader":"planet_surface","vertices":[[-1.,-1.,0.,1.,1.,1.],[1.,-1.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]]}))?;
 let mut session=Session::prepare(root,output,backend,&entity)?;session.environment("planet_surface");session.texture(&[1,0,0,0,1,0,0,0,255,255,255,255])?;
 let base=site.incense();let mut reports=Vec::new();
 for (name,eye,target,lit,spent,ignition,now) in [
  ("monastery-exterior",add(site.center,[0.,16.,42.]),add(site.center,[0.,1.,0.]),None,false,None,0.),
  ("zen-garden",add(site.center,[3.,4.,15.]),add(site.center,[17.,0.,-4.]),None,false,None,0.),
  ("incense-unlit",add(base,[0.35,0.24,0.5]),add(base,[0.,0.09,0.]),None,false,None,0.),
  ("incense-ignition",add(base,[0.35,0.24,0.5]),add(base,[0.,0.09,0.]),None,false,Some(0.),1.),
  ("incense-burning",add(base,[0.35,0.24,0.5]),add(base,[0.,0.14,0.]),Some(0.),false,None,10.),
  ("incense-half-burned",add(base,[0.35,0.24,0.5]),add(base,[0.,0.14,0.]),Some(0.),false,None,900.),
  ("incense-spent",add(base,[0.35,0.24,0.5]),add(base,[0.,0.09,0.]),None,true,None,1806.),
 ] {
  let forward=unit(sub(target,eye));let right=unit(cross(forward,[0.,1.,0.]));let up=unit(cross(right,forward));
  let view=json!({"entity":1,"position":add(origin,eye),"table_eye":eye,"yaw":0.,"pitch":0.,"right":right,"up":up,"forward":forward,"aspect":1.5,"day_night":{"fraction":0.5}});
  let mut triangles=crate::base_system::layer_four::appearance_system::theatre_shared_mesh(origin,&v,&view)?;
  let mut state=crate::base_system::layer_five::monastery_system::State::default();state.lit=lit;state.spent=spent;state.ignition=ignition;state.lighter=ignition.is_some();
  triangles.extend(crate::base_system::layer_four::monastery_render_system::geometry(&state,add(origin,base),add(origin,site.lighter()),&view,now)?);
  let sky=crate::base_system::layer_four::atmosphere::Lighting::from_view(&view).sky(&view);crate::base_system::layer_four::appearance_system::insert_sky_after_opaque(&mut triangles,sky);
  session.table_eye(&view)?;session.set_batch(&triangles)?;session.capture_next();let frame=session.pump(true,true)?;
  std::fs::copy(output.join(format!("triangle-{backend}.ppm")),output.join(format!("{name}.ppm"))).map_err(|e|e.to_string())?;
  reports.push(json!({"name":name,"triangles":triangles.len(),"frame":frame}));
 }
 session.finish()?;std::fs::write(output.join("monastery-review.json"),serde_json::to_vec_pretty(&reports).unwrap()).map_err(|e|e.to_string())?;Ok(())
}
