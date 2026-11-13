//! A bounded output block has one monotonic sequence and possibly wrapped
//! timeline addresses. Natural wraps never imply an instrument/effect reset.
use serde_json::Value;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) struct Sequence {pub generation:u64,pub frame:u64}
#[derive(Clone,Copy,Debug)]
pub(crate) struct Span {pub offset:usize,pub frames:usize,pub start:u64,pub sequence:Option<Sequence>}
#[derive(Clone,Debug)]
pub(crate) struct Clock {pub start:u64,pub frames:usize,pub sequence:Option<Sequence>,pub loop_range:Option<(u64,u64)>}
impl Clock {
 pub fn parse(input:&Value,start:u64,frames:usize)->Result<Self,String>{
  let sequence=if let Some(c)=input.get("playback_clock"){Some(Sequence{generation:c["generation"].as_u64().ok_or("playback generation required")?,frame:c["sequence"].as_u64().ok_or("playback sequence required")?})}else{None};
  if sequence.is_some_and(|s|s.frame.checked_add(frames as u64).is_none()){return Err("playback sequence exhausted".into());}
  let loop_range=if let Some(r)=input.get("loop_range"){
   let a=r["start_frame"].as_u64().ok_or("loop start required")?;let b=r["end_frame"].as_u64().ok_or("loop end required")?;
   if sequence.is_none()||a>=b||start>=b||b>input["sample_rate"].as_u64().unwrap_or(192000).min(192000)*86400{return Err("loop range requires a playback clock and start before loop end".into());}Some((a,b))
  }else{None};Ok(Self{start,frames,sequence,loop_range})
 }
 pub fn at(&self,offset:usize)->u64{let t=self.start+offset as u64;if let Some((a,b))=self.loop_range{if t>=b{return a+(t-b)%(b-a);}}t}
 pub fn end(&self)->u64{self.at(self.frames)}
 pub fn bounds(&self)->(u64,u64){let mut lo=self.start;let mut hi=self.start;for i in 0..self.frames{let a=self.at(i);lo=lo.min(a);hi=hi.max(a);} (lo,hi+1)}
 pub fn offsets(&self,at:u64)->Vec<usize>{
  let mut result=Vec::new();
  if let Some((a,b))=self.loop_range{
   if at>=self.start&&at<b{let offset=at-self.start;if offset<self.frames as u64{result.push(offset as usize);}}
   if at>=a&&at<b{let mut offset=b-self.start+at-a;while offset<self.frames as u64{result.push(offset as usize);offset+=b-a;}}
  }else if at>=self.start&&at-self.start<self.frames as u64{result.push((at-self.start)as usize);}
  result
 }
 pub fn spans(&self)->Vec<Span>{let mut result=Vec::new();let mut offset=0;while offset<self.frames{
  let start=self.at(offset);let frames=self.loop_range.map_or(self.frames-offset,|(_,b)|(b-start).min((self.frames-offset)as u64)as usize);
  result.push(Span{offset,frames,start,sequence:self.sequence.map(|s|Sequence{generation:s.generation,frame:s.frame+offset as u64})});offset+=frames;
 }result}
}
/// No timeline-position comparison: loops reuse state; seeks, replaced queued
/// generations, sample-rate changes and skipped output reset it explicitly.
#[derive(Clone,Copy,Default,Debug)]
pub(crate) struct Continuity {last:Option<(u64,u64,u32)>}
impl Continuity {
 pub fn discontinuity(&self,s:Sequence,rate:u32)->bool{self.last!=Some((s.generation,s.frame,rate))}
 pub fn commit(&mut self,s:Sequence,frames:usize,rate:u32){self.last=Some((s.generation,s.frame+frames as u64,rate));}
}
#[cfg(test)]mod tests{
 use super::*;use serde_json::json;
 #[test]fn one_sample_loop_fills_one_bounded_request_with_monotonic_sequence(){let c=Clock::parse(&json!({"playback_clock":{"generation":4,"sequence":100},"loop_range":{"start_frame":7,"end_frame":8}}),7,4096).unwrap();assert_eq!(c.bounds(),(7,8));assert_eq!(c.end(),7);assert!(c.spans().iter().enumerate().all(|(i,s)|s.start==7&&s.frames==1&&s.sequence.unwrap().frame==100+i as u64));}
 #[test]fn natural_wrap_preserves_a_stateful_tail_and_generation_change_resets(){let mut c=Continuity::default();let mut tail=0.;let mut resets=0;let mut output=Vec::new();for i in 0..8{let s=Sequence{generation:1,frame:i};if c.discontinuity(s,48000){tail=0.;resets+=1;}let input=if i==0{1.}else{0.};tail=input+tail*0.5;output.push(tail);c.commit(s,1,48000);}assert_eq!(resets,1);assert_eq!(output,[1.,0.5,0.25,0.125,0.0625,0.03125,0.015625,0.0078125]);assert!(c.discontinuity(Sequence{generation:2,frame:8},48000));assert!(c.discontinuity(Sequence{generation:1,frame:9},48000));assert!(c.discontinuity(Sequence{generation:1,frame:8},44100));}
}
