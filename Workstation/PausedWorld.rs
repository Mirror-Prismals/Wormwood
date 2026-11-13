//! The in-game DAW owns a paused game view, not another full game render loop.
//! Retain that real view between observer/world changes; UI motion must not
//! rebuild terrain, vegetation, water, shadows and reflections on every frame.
use serde_json::{Value,json};
pub(crate) fn enabled(backend:&str)->bool{cfg!(target_os="macos")&&backend=="WebGPU"&&std::env::var("WORMWOOD_WORKSTATION_REFERENCE_OPTICS").as_deref()!=Ok("1")&&std::env::var("WORMWOOD_WORKSTATION_WORLD_CACHE").as_deref()!=Ok("0")}
pub(crate) fn key(camera:&Value,world:&Value,vegetation_revision:u64)->Value{
 json!({"camera":camera,"game_entity":world["entity"],"instances":world["instances"],"time":world["time"],"time_override_minutes":world["time_override_minutes"],"vegetation_revision":vegetation_revision})
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn paused_world_key_tracks_observer_world_and_resume_clock(){
  let c=json!({"id":"riverbank","position":[1.,2.,3.],"yaw":0.2,"aspect":1.6});let w=json!({"entity":4,"instances":[],"time":5.});let a=key(&c,&w,2);
  assert_eq!(a,key(&c,&w,2));for k in ["id","position","yaw","aspect"]{let mut c=c.clone();c[k]=json!(99);assert_ne!(a,key(&c,&w,2));}
  for k in ["entity","instances","time","time_override_minutes"]{let mut w=w.clone();w[k]=json!(99);assert_ne!(a,key(&c,&w,2));}assert_ne!(a,key(&c,&w,3));
 }
}
