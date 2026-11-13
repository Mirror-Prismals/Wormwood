//! Main-thread workstation pacing. No callback scheduling or display-mode changes.
use serde_json::{Value,json};
use std::time::{Duration,Instant};
// Explicit finite diagnostic only; one process-local monotonic clock shared by
// the application, event pump and renderer. It never controls scheduling.
pub(crate) fn timeline_enabled()->bool{static ENABLED:std::sync::OnceLock<bool>=std::sync::OnceLock::new();*ENABLED.get_or_init(||std::env::var("WORMWOOD_WORKSTATION_FRAME_TIMELINE").as_deref()==Ok("1"))}
pub(crate) fn timeline_stamp(t:Instant)->f64{static ORIGIN:std::sync::OnceLock<Instant>=std::sync::OnceLock::new();t.saturating_duration_since(*ORIGIN.get_or_init(||t)).as_secs_f64()*1000.}

#[derive(Clone,Copy,Default,Debug)]
pub(crate) struct Display {
 pub current_mode_hz:f64,pub maximum_hz:f64,pub minimum_interval:f64,pub maximum_interval:f64,
 pub id:u32,pub source:&'static str,
}
fn valid(hz:f64)->bool {hz.is_finite()&&(24.0..=1000.0).contains(&hz)}
impl Display {
 pub fn target_hz(self)->f64 {
  if valid(self.current_mode_hz){self.current_mode_hz}
  else if self.minimum_interval>0.&&valid(1./self.minimum_interval){1./self.minimum_interval}
  else if valid(self.maximum_hz){self.maximum_hz}else{60.}
 }
 pub fn info(self)->Value {json!({"current_mode_hz":self.current_mode_hz,"maximum_hz":self.maximum_hz,
  "minimum_refresh_interval_seconds":self.minimum_interval,"maximum_refresh_interval_seconds":self.maximum_interval,
  "display_id":self.id,"source":self.source,"active_target_hz":self.target_hz(),"physical_scanout_hz":Value::Null,
  "scope":"Window screen metadata; a supported variable-refresh ceiling is not measured scanout. Rechecked every0.5s; unknown information falls back to60Hz."})}
}
pub(crate) fn budget(display_aware:bool,active:bool,target_hz:f64)->Duration {
 // Keep the existing60Hz audio/input service cadence when quiet, suspended or
 // on a slower display. Do not spin to catch up after an over-budget frame.
 let hz=if display_aware&&active&&valid(target_hz){target_hz.max(60.)}else{60.};
 Duration::from_secs_f64(1./hz)
}
#[derive(Default)]
pub(crate) struct Stats {
 pub loops:u64,pub draws:u64,pub paced:u64,pub idle:u64,pub sleep_seconds:f64,
 pub active_target_hz:f64,pub display:Value,pub last_pace:Value,
}
impl Stats {
 pub fn observe(&mut self,window:&Value){self.loops+=1;self.draws+=u64::from(window["drawn"]==true);self.display=window["display_cadence"].clone();}
 pub fn pace(&mut self,enabled:bool,window:&Value,elapsed:Duration){
  let active=window["drawn"]==true&&window["suspended"]!=true;
  let target=window["display_cadence"]["active_target_hz"].as_f64().unwrap_or(60.);
  let period=budget(enabled,active,target);self.paced+=1;self.idle+=u64::from(!active);self.active_target_hz=1./period.as_secs_f64();
  let trace=timeline_enabled();let call_started=trace.then(Instant::now);
  let requested=period.checked_sub(elapsed);let mut sleep_times=None;
  if let Some(remaining)=requested{let start=Instant::now();std::thread::sleep(remaining);let end=Instant::now();self.sleep_seconds+=end.duration_since(start).as_secs_f64();if trace{sleep_times=Some([timeline_stamp(start),timeline_stamp(end)]);}}
  if let Some(start)=call_started{self.last_pace=json!({"call_ms":[timeline_stamp(start),timeline_stamp(Instant::now())],"budget_ms":period.as_secs_f64()*1000.,"elapsed_argument_ms":elapsed.as_secs_f64()*1000.,"requested_sleep_ms":requested.map_or(0.,|d|d.as_secs_f64()*1000.),"sleep_ms":sleep_times,"anchor":"current loop start; period minus all elapsed work before this call"});}
 }
 pub fn info(&self,enabled:bool,benchmark_paced:bool)->Value {json!({"display_aware":enabled,"benchmark_paced":benchmark_paced,
  "loops":self.loops,"draws":self.draws,"paced_iterations":self.paced,"idle_paced_iterations":self.idle,
  "sleep_seconds":self.sleep_seconds,"last_target_hz":self.active_target_hz,"display":self.display,
  "scope":"Main-thread sleep after input processing; audio callback and elapsed-time animation unchanged. Quiet/suspended polling remains60Hz, redraw stays invalidation driven. Benchmark pacing is explicit; ordinary benchmarks remain unpaced."})}
}
#[cfg(test)] mod cadence_tests {
 use super::*;
 #[test] fn fixed_display_mode_wins_over_hardware_maximum(){let d=Display{current_mode_hz:60.,maximum_hz:120.,minimum_interval:1./120.,..Default::default()};assert_eq!(d.target_hz(),60.);}
 #[test] fn variable_display_uses_reported_ceiling(){let d=Display{minimum_interval:1./120.,maximum_interval:1./48.,..Default::default()};assert!((d.target_hz()-120.).abs()<1e-9);}
 #[test] fn unknown_or_invalid_metadata_falls_back(){for hz in [0.,-1.,f64::NAN,f64::INFINITY,10001.]{assert_eq!(Display{current_mode_hz:hz,maximum_hz:hz,..Default::default()}.target_hz(),60.);}}
 #[test] fn high_refresh_only_applies_to_active_opt_in_frames(){assert!(budget(true,true,120.)<Duration::from_millis(9));assert!(budget(true,true,165.)<Duration::from_millis(7));for (enabled,active) in [(false,true),(true,false),(false,false)]{assert_eq!(budget(enabled,active,120.),budget(false,false,60.));}}
 #[test] fn slow_display_keeps_audio_service_and_overrun_does_not_catch_up(){assert_eq!(budget(true,true,30.),budget(false,false,60.));assert!(budget(true,true,120.).checked_sub(Duration::from_millis(10)).is_none());}
}
