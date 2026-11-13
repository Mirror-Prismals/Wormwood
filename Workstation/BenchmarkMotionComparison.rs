//! Explicit finite comparison only. Normal interactive motion is unchanged.
use crate::workstation::panel_motion::{State,Edge};
use std::time::Instant;
pub(crate) const STATES:usize=32;
#[derive(Clone,Copy,Debug,PartialEq,Eq)] pub(crate) enum Mode { Bank, Reference, Continuous }
pub(crate) struct Comparison {
 pub mode:Mode,pub capture:bool,pub replay:bool,start:Option<Instant>,last_capture:f64,
 pub records:Vec<serde_json::Value>,pub position:f64,pub elapsed:f64,
}
pub(crate) fn pose(position:f64)->State {
 let p=position.clamp(0.,31.);let mut state=State::default();
 state.set_all_open(true);state.advance(1.);state.set_open(Edge::Left,false);
 for _ in 0..p.floor() as usize{state.advance(0.05);}
 state.advance((p-p.floor())*0.05);state
}
/// Finite closed-pose path; replay reverses the pose parameter, not Cardinal's
/// input-history integrator. This deliberate experimental behavior is reported.
pub(crate) fn position(seconds:f64)->f64 {
 let t=seconds.max(0.)%3.;
 if t<1. {31.*t} else if t<1.3 {31.-17.*(t-1.)/0.3}
 else if t<1.6 {14.+11.*(t-1.3)/0.3}
 else if t<2.6 {25.*(2.6-t)} else {0.}
}
impl Comparison {
 pub fn selected(finite:bool)->Result<Option<Self>,String>{
  let Ok(mode)=std::env::var("WORMWOOD_WORKSTATION_MOTION_COMPARE") else{return Ok(None);};
  if !finite{return Err("Motion comparison requires the explicit finite benchmark".into());}
  let mode=match mode.as_str(){"bank"=>Mode::Bank,"reference"=>Mode::Reference,"continuous"=>Mode::Continuous,_=>return Err("Motion comparison mode must be bank, reference or continuous".into())};
  Ok(Some(Self{mode,capture:std::env::var("WORMWOOD_WORKSTATION_MOTION_FOOTAGE").as_deref()==Ok("1"),replay:std::env::var("WORMWOOD_WORKSTATION_MOTION_POSE_REPLAY").as_deref()==Ok("1"),start:None,last_capture:-1.,records:vec![],position:0.,elapsed:0.}))
 }
 pub fn frame(&mut self,frame:u64,warmup:u64)->State {
  if frame<warmup{self.position=(frame%STATES as u64) as f64;self.elapsed=0.;}
  else{let start=*self.start.get_or_insert_with(Instant::now);self.elapsed=start.elapsed().as_secs_f64();
   self.position=if self.replay{((frame-warmup)%STATES as u64) as f64}else{let p=position(self.elapsed);if self.mode==Mode::Continuous{p}else{p.round()}};
  }
  pose(self.position)
 }
 pub fn capture_due(&mut self,frame:u64,warmup:u64)->bool{
  if !self.capture||frame<warmup||self.records.len()>=72{return false;}
  if self.replay||self.last_capture<0.||self.elapsed-self.last_capture>=1./20.{self.last_capture=self.elapsed;true}else{false}
 }
 pub fn record(&mut self,file:String,draw:u64){let elapsed=self.start.map_or(0.,|s|s.elapsed().as_secs_f64());
  self.records.push(serde_json::json!({"file":file,"at_seconds":elapsed,"draw_index":draw,"pose":self.position}));
 }
 pub fn report(&self)->serde_json::Value{serde_json::json!({"mode":format!("{:?}",self.mode),"states":STATES,"pose_dt":0.05,"cycle_seconds":3.,"end_seconds":self.start.map_or(0.,|s|s.elapsed().as_secs_f64()),"time_basis":"actual monotonic capture-completion timestamps; readbacks perturb timing; separate no-readback FPS run required","label":format!("{:?}: original optical lighting",self.mode),"frames":self.records,"pose_replay":self.replay,"scope":"One empty stopped DAW, canonical left-panel slide with two mid-motion direction reversals. Bank/reference use nearest of32 poses; continuous uses fractional original-integrator pose. Reversing this stored path differs from the normal history-dependent interactive reversal. Live camera/daylight remain separate; exact packet/resource mismatch falls back."})}
}
#[cfg(test)] mod tests {
 use super::*;
 #[test]fn stored_pose_is_repeatable_and_changes_actual_geometry(){for i in 0..STATES{let a=pose(i as f64).snapshot();let b=pose(i as f64).snapshot();assert_eq!(a.extensions,b.extensions);assert_eq!(a.opening_order,b.opening_order);}assert!(pose(31.).snapshot().extensions[2]<0.002);assert_eq!(pose(0.).snapshot().extensions[2],1.);}
 #[test]fn path_has_declared_reversals_and_bounds(){assert_eq!(position(0.),0.);assert!((position(1.)-31.).abs()<1e-10);assert!((position(1.3)-14.).abs()<1e-10);assert!((position(1.6)-25.).abs()<1e-10);assert_eq!(position(2.6),0.);for i in 0..3000{assert!((0.0..=31.).contains(&position(i as f64/1000.)));}}
 #[test]fn fractional_pose_keeps_stored_endpoints(){for i in 0..31{let a=pose(i as f64).snapshot().extensions[2];let m=pose(i as f64+0.5).snapshot().extensions[2];let b=pose(i as f64+1.).snapshot().extensions[2];assert!(m<=a+0.02&&m>=b-0.02);}}
}
