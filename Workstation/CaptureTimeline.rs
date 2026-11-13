//! Capture uses the timeline lane's exact Clock mapping. Device frames remain
//! monotonic when song frames wrap; this adapter never renders or copies loops.
use serde_json::{Value,json};
use super::render_clock::Clock;
pub(crate) struct CaptureClock {clock:Clock,playing:bool,zero:u64}
impl CaptureClock {
 pub fn new(status:&Value,start:u64,frames:usize,playing:bool,generation:u64,sequence:u64)->Result<Self,String>{
  let mut request=json!({"playback_clock":{"generation":generation,"sequence":sequence}});
  if playing && status["loop_enabled"]==true {request["loop_range"]=json!({"start_frame":status["loop_start_frame"],"end_frame":status["loop_end_frame"]});}
  Ok(Self{clock:Clock::parse(&request,start,frames)?,playing,zero:status["timeline_zero_frame"].as_u64().unwrap_or(0)})
 }
 pub fn at(&self,offset:usize)->u64{if self.playing{self.clock.at(offset)}else{self.clock.start}}
 pub fn annotate(&self,block:&mut Value){
  block["timeline_zero_frame"]=json!(self.zero);
  if let Some(s)=self.clock.sequence{block["playback_clock"]=json!({"generation":s.generation,"sequence":s.frame});}
  if let Some((a,b))=self.clock.loop_range{block["loop_start_frame"]=json!(a);block["loop_end_frame"]=json!(b);}
 }
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn capture_one_sample_loop_stays_one_bounded_block(){let c=CaptureClock::new(&json!({"loop_enabled":true,"loop_start_frame":7,"loop_end_frame":8,"timeline_zero_frame":4}),7,4096,true,5,100).unwrap();assert!((0..4096).all(|i|c.at(i)==7));let mut v=json!({});c.annotate(&mut v);assert_eq!(v["loop_start_frame"],7);assert_eq!(v["playback_clock"]["sequence"],100);assert_eq!(v["timeline_zero_frame"],4);}
 #[test]fn preloop_capture_and_stopped_clock_keep_source_mapping(){let s=json!({"loop_enabled":true,"loop_start_frame":10,"loop_end_frame":13});let c=CaptureClock::new(&s,8,9,true,1,0).unwrap();assert_eq!((0..9).map(|i|c.at(i)).collect::<Vec<_>>(),[8,9,10,11,12,10,11,12,10]);let c=CaptureClock::new(&s,8,9,false,1,0).unwrap();assert!((0..9).all(|i|c.at(i)==8));}
}
