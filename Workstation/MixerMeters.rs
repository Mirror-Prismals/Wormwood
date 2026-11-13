//! Cardinal DecibelMeterSystem attack/decay and DawMeterStyle routed channels.
//! Targets are real post-route peaks from the existing render observation.
use super::bottom_panel::{Context,meter_step};
use serde_json::Value;
use std::collections::BTreeMap;
#[derive(Default)]pub(crate) struct State{tracks:BTreeMap<String,f64>,master:[f64;6]}
fn peak(v:&Value)->f64{v.as_f64().filter(|v|v.is_finite()&&*v>=0.).unwrap_or(0.)}
fn normalized(amplitude:f64)->f64{((20.*amplitude.max(0.000001).log10()+60.)/60.).clamp(0.,1.)}
fn routed(level:f64,left:&str,right:&str)->[f64;2]{let mut out=[0.;2];let mut add=|bus:&str,gain:f64|match bus{"L"=>out[0]+=gain,"R"=>out[1]+=gain,"S"|"FF"=>{out[0]+=gain;out[1]+=gain},_=>{}};add(left,level);if left!=right{add(right,level)}else if matches!(right,"S"|"FF"){add(right,level*0.5)}out.map(|a:f64|a.clamp(0.,1.))}
impl State{
 pub(crate) fn apply(&mut self,context:&mut Context,snapshot:&Value,observation:&Value,dt:f64){
  self.tracks.retain(|id,_|id.starts_with("bus:")||context.tracks.iter().any(|t|t.id==*id));
  for track in &mut context.tracks{
   let observed=observation["tracks"].as_array().into_iter().flatten().find(|t|t["track"]==track.id);
   let target=observed.map_or(0.,|v|peak(&v["meter_peak"]));let level=self.tracks.entry(track.id.clone()).or_default();*level=meter_step(*level,target,dt,12.,1.5);
   let route=snapshot["tracks"].as_array().into_iter().flatten().find(|t|t["id"]==track.id);
   track.meter=route.map(|r|routed(*level,r["output_bus_l"].as_str().unwrap_or("OFF"),r["output_bus_r"].as_str().unwrap_or("OFF")).map(normalized)).unwrap_or([0.;2]);
  }
  for bus in &mut context.buses {
   let id=bus["id"].as_str().unwrap_or("").to_owned();
   let observed=observation["buses"].as_array().into_iter().flatten().find(|b|b["id"]==id);
   let mut levels=[0.;2];for ch in 0..2{let key=format!("bus:{id}:{ch}");let level=self.tracks.entry(key).or_default();*level=meter_step(*level,observed.map_or(0.,|o|peak(&o["peaks"][ch])),dt,12.,1.5);levels[ch]=normalized(*level);}
   bus["meter"]=serde_json::json!(levels);
  }
  for track in &mut context.tracks{if let Some(bus)=context.buses.iter().find(|b|b["id"]==track.id&&b["kind"]=="channel"){track.meter=[peak(&bus["meter"][0]),peak(&bus["meter"][1])];}}
  for i in 0..6{let target=if i<4{peak(&observation["master_meter_peaks"][i])}else{peak(&observation["monitor_meter_peaks"][i-4])};self.master[i]=meter_step(self.master[i],target,dt,12.,1.5);context.master_meters[i]=normalized(self.master[i]);}
 }
}
#[cfg(test)]mod tests{use super::*;#[test]fn route_matches_original_shared_bus_boost_and_off(){assert!(routed(0.4,"FF","FF").iter().all(|v|(*v-0.6).abs()<1e-12));assert_eq!(routed(0.4,"L","R"),[0.4,0.4]);assert_eq!(routed(0.4,"OFF","R"),[0.,0.4]);}#[test]fn scale_is_original_sixty_db_display(){assert_eq!(normalized(0.),0.);assert!((normalized(0.001)).abs()<1e-12);assert!((normalized(0.1)-2./3.).abs()<1e-12);assert_eq!(normalized(2.),1.);}}
