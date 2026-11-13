//! Opt-in candidate and finite tests through the real Step key/target route.
use serde_json::{json,Value};
use std::time::Instant;
use crate::workstation::panel_motion::State;
fn selection(value:Option<&str>,_macos:bool,_comparison:bool)->Option<bool>{
 match value{Some("bank")=>Some(true),Some("reference")=>Some(false),Some(_)=>None,None=>None}
}
fn selected_mode()->Option<bool>{selection(std::env::var("WORMWOOD_WORKSTATION_MOTION_INTERACTIVE").ok().as_deref(),cfg!(target_os="macos"),std::env::var_os("WORMWOOD_WORKSTATION_MOTION_COMPARE").is_some())}
pub(crate) fn enabled()->bool{selected_mode().is_some()}
pub(crate) fn bank_enabled()->bool{selected_mode()==Some(true)}
pub(crate) fn context(embedded:bool,passthrough:bool,animated:bool,backend:&str)->bool{embedded&&passthrough&&!animated&&backend=="WebGPU"}
const EVENTS:[f64;6]=[0.2,0.55,0.9,1.25,2.0,3.2];
pub(crate) struct Replay{pub gate:bool,changes:bool,start:Option<Instant>,next:usize,release:bool,pub measured_frame:u64,elapsed:f64,content_done:bool,light_stage:u8,pending_content:bool,pending_light:Option<f64>,records:Vec<Value>,events:Vec<Value>,measured:bool}
impl Replay{
 pub fn selected(finite:bool)->Result<Option<Self>,String>{
  let Ok(mode)=std::env::var("WORMWOOD_WORKSTATION_MOTION_INPUT_TEST")else{return Ok(None)};
  if !finite{return Err("Motion input test requires a finite benchmark".into());}
  if !["motion","changes","gate"].contains(&mode.as_str()){return Err("Motion input test must be motion, changes or gate".into());}
  Ok(Some(Self{gate:mode=="gate",changes:mode!="motion",start:None,next:0,release:false,measured_frame:0,elapsed:0.,content_done:false,light_stage:0,pending_content:false,pending_light:None,records:vec![],events:vec![],measured:false}))
 }
 pub fn keys(&mut self,frame:u64,warmup:u64)->[bool;5]{
  self.measured=frame>=warmup;if !self.measured{return [false;5];}
  self.measured_frame=frame-warmup;let start=*self.start.get_or_insert_with(Instant::now);
  self.elapsed=if self.gate{self.measured_frame as f64/30.}else{start.elapsed().as_secs_f64()};
  if self.changes{
   let content=if self.gate{60}else{120};let light=if self.gate{72.0/30.}else{6.};let reset=if self.gate{84.0/30.}else{8.};
   if !self.content_done&&(if self.gate{self.measured_frame>=content}else{self.elapsed>=4.}){self.content_done=true;self.pending_content=true;self.events.push(json!({"action":"add_midi_track","at":self.elapsed}));}
   if self.light_stage==0&&self.elapsed>=light{self.light_stage=1;self.pending_light=Some(0.65);self.events.push(json!({"action":"light_brightness","value":0.65,"at":self.elapsed}));}
   else if self.light_stage==1&&self.elapsed>=reset{self.light_stage=2;self.pending_light=Some(1.);self.events.push(json!({"action":"light_brightness","value":1.,"at":self.elapsed}));}
  }
  let due=if self.gate{[2,14,26,38,48,96].get(self.next).map(|f|*f as f64/30.)}else{Some((self.next/6)as f64*4.4+EVENTS[self.next%6])};
  let mut keys=[false;5];
  if self.release{self.release=false;}else if due.is_some_and(|t|self.elapsed>=t){keys[2]=true;self.next+=1;self.release=true;self.events.push(json!({"action":"left_key_press","requested_at":due,"delivered_at":self.elapsed,"frame":self.measured_frame}));}
  keys
 }
 pub fn take_content_change(&mut self)->bool{std::mem::take(&mut self.pending_content)}
 pub fn take_light_change(&mut self)->Option<f64>{self.pending_light.take()}
 pub fn capture_due(&self)->bool{self.measured&&self.gate&&[12,22,34,46,62,74,86,110].contains(&self.measured_frame)}
 pub fn record(&mut self,m:&State,window:&Value,wall_ms:f64){if !self.measured||window["drawn"]!=true||self.records.len()>=20_000{return;}
  let s=m.snapshot();let bank=&window["device"]["workstation_shader"]["motion_bank"];
  self.records.push(json!({"frame":self.measured_frame,"at":self.elapsed,"wall_ms":wall_ms,"open":s.open,"extensions":s.extensions,"order":s.opening_order,"phase":m.canonical_left_phase(),"bank_draws":bank["playback_draws"],"bank_captures":bank["capture_draws"],"bank_recycled":bank["recycled_slots"],"bank_invalidations":bank["external_invalidations"],"bank_states":bank["states"],"bake":window["device"]["frame_attribution"]["retained_bake"]}));
 }
 pub fn report(&self)->Value{json!({"gate":self.gate,"changes":self.changes,"events":self.events,"frames":self.records,"scope":"Actual Step rising-edge left-key input, real target/order state and dt, no pose reset/prewarm sweep. Gate uses deterministic dt and light/content changes for images only. Ordinary timing uses monotonic dt; delayed input delivery is recorded, not hidden. Left unexpanded path only; other actions retain original fallback."})}
}
#[cfg(test)]mod tests{
 #[test]fn only_actual_webgpu_passthrough_uses_stored_panel_motion(){
  assert!(super::context(true,true,false,"WebGPU"));
  for args in [(false,true,false,"WebGPU"),(true,false,false,"WebGPU"),(true,true,true,"WebGPU"),(true,true,false,"OpenGL"),(true,true,false,"Vulkan")]{assert!(!super::context(args.0,args.1,args.2,args.3));}
 }

 #[test]fn default_uses_continuous_composition_and_explicit_bank_remains(){
  assert_eq!(super::selection(None,true,false),None);assert_eq!(super::selection(None,false,false),None);
  assert_eq!(super::selection(None,true,true),None);assert_eq!(super::selection(Some("0"),true,false),None);
  assert_eq!(super::selection(Some("reference"),true,false),Some(false));
  assert_eq!(super::selection(Some("bank"),true,false),Some(true));
 }

 #[test]fn event_script_has_midflight_reversals_and_finite_spacing(){assert!(super::EVENTS.windows(2).all(|w|w[1]>w[0]));assert!(super::EVENTS[1]-super::EVENTS[0]<1.);assert!(super::EVENTS[5]<4.4);}
}
