//! Application camera selection; projection remains a layer-four responsibility.
use crate::host::Host;
use serde_json::{Value,json};
pub(crate) fn dispatch(host:&mut Host,hook:&str,input:&Value)->Result<Value,String>{match hook{
 "View"=>{
  if input["enabled"]!=true||input["embedded"]!=true||host.theatre.venue.is_none(){host.security_cameras.cancel_keys();return Ok(Value::Null);}
  let page=input["page"].as_str().ok_or("camera page required")?;
  let aspect=input["aspect"].as_f64().ok_or("camera aspect required")?;
  let dt=input["dt"].as_f64().ok_or("camera dt required")?;
  let anchor=host.theatre.venue.as_ref().unwrap().computer_position;
  let mut camera=host.security_cameras.update(page,host.theatre.origin,anchor,&input["keys"],dt,input["blocked"]==true,aspect)?;
  // Fixed stage rehearsal updates only the real theatre geometry/material pass.
  // Outdoor cameras retain their completed paused image and water animation.
  let rehearsal=camera["id"]=="theatre";
  if rehearsal {
   let audio=host.invoke("EnvironmentAudioSystem.Status",Value::Null)?;
 let transport=&audio["transport"];
   let seconds=transport["position_frame"].as_u64().zip(transport["sample_rate"].as_u64().filter(|r|*r>0)).map_or(0.,|(f,r)|f as f64/r as f64);
   camera["stage_rehearsal"]=json!(true);
   camera["stage_lighting"]=host.invoke("WorkstationDataSystem.StageLighting",json!({"seconds":seconds}))?["levels"].clone();
  }
  // Retain the live scene identity and clock; only the observer changes.
  let world=host.environment.view();
  let retained=crate::workstation::paused_world::enabled(host.backend());
  let key=crate::workstation::paused_world::key(&camera,&world,host.vegetation.revision);
  if retained&&host.security_cameras.retained_key==key&&host.security_cameras.retained_view.is_object(){return Ok(host.security_cameras.retained_view.clone());}
  for key in ["entity","instances","time_override_minutes","time"] {camera[key]=world[key].clone();}
  if !rehearsal && host.system_enabled("WaterSystem") && camera["entity"].is_u64() {
   let status=host.invoke("WaterSystem.Frame",Value::Null)?;
   if status["configured"]!=true {host.invoke("WaterSystem.Configure",Value::Null)?;}
   if status["paused"]==true {host.invoke("WaterSystem.Pause",json!({"paused":false}))?;}
   let water=host.invoke("WaterSystem.Step",json!({"position":camera["position"],"forward":camera["forward"],"dt":dt.min(0.1)}))?;
   camera["water_instances"]=water["instances"].clone();
   camera["time"]=water["time"].clone();
  }
  let camera=crate::base_system::layer_four::atmosphere::Lighting::enrich(&camera);
  if retained{host.security_cameras.retained_key=key;host.security_cameras.retained_view=camera.clone();}
  Ok(camera)
 },
 "Status"=>Ok(json!({"available":host.theatre.venue.is_some(),"camera_ids":crate::theatre::cameras::CAMERA_IDS})),
 _=>Err("unknown security camera hook".into())
}}
