//! Preparation-worker history fed only by accepted EnvironmentAudioSystem capture blocks.
//! Raw capture remains owned by native I/O. The display may fill dropped spans
//! with silence; recall never manufactures PCM for a missing accepted block.
use super::{loopback_history::{History as Display,Source},retrospective_recall as recall};
use serde_json::{Value,json};
use std::collections::{BTreeMap,VecDeque};
#[derive(Clone)] struct Frame{serial:u64,audio:[f32;2],song:u64}
struct Block{rate:u32,start:u64,timeline:u64,playing:bool,recording:bool,mic:Vec<f32>,player:Vec<[f32;2]>,midi:Vec<(String,recall::MidiEvent,bool)>,epoch:Option<u64>,loop_range:Option<[u64;2]>}
pub(crate) struct Buffer{
 source:Source,requested:Source,rate:u32,frames:Vec<Frame>,head:u64,source_start:u64,audio_clear:u64,midi_clear:u64,last_playing:Option<u64>,
 // A new capture epoch reserves serials above the preceding epoch's entire
 // device-clock span. Old ring cells then become invalid without clearing or
 // reallocating thirty seconds of storage, even when the clock moves back.
 serial_base:u64,serial_start:u64,
 midi:VecDeque<recall::MidiEvent>,held:BTreeMap<(String,u8,bool),recall::MidiEvent>,aggregate:[Option<recall::MidiEvent>;128],tracks:BTreeMap<String,i32>,next_track:i32,epoch:Option<u64>,pub recording:bool,
 pub dropped_blocks:u64,pub native_dropped_frames:u64,
}

#[cfg(test)]
mod retrospective_storage_tests {
 use super::*;
 fn block(start:u64,epoch:Option<u64>,samples:&[f32])->Value {
  json!({"sample_rate":8000,"frames":samples.len(),"capture_start_frame":start,
   "timeline_start_frame":1000+start,"playing":true,"recording":false,
   "capture_epoch":epoch,"live_inputs":{"physical":[samples]},
   "player_audio":[samples,samples],"midi_events":[]})
 }
 fn ingest(buffer:&mut Buffer,block:Value){buffer.ingest(&json!({"blocks":[block]})).unwrap();}

 #[test]
 fn prepared_history_keeps_first_capture_chronology_and_reuses_storage() {
  let mut buffer=Buffer::default();buffer.prepare(8000).unwrap();
  let address=buffer.frames.as_ptr();let capacity=buffer.frames.capacity();
  assert_eq!(buffer.rate(),0);assert_eq!(buffer.head(),0);
  ingest(&mut buffer,block(100,Some(1),&[0.25,-0.5]));
  assert_eq!(buffer.frames.as_ptr(),address);assert_eq!(buffer.frames.capacity(),capacity);
  assert_eq!(buffer.floor(),100);assert_eq!(buffer.head(),102);
  assert_eq!(buffer.audio(100),Some([0.25;2]));assert_eq!(buffer.song(101),Some(Some(1101)));
  assert_eq!(buffer.audio(99),None);
  // A forward epoch invalidates the old logical history without touching the
  // already allocated ring or shifting the new block's accepted clock.
  ingest(&mut buffer,block(200,Some(2),&[0.75]));
  assert_eq!(buffer.frames.as_ptr(),address);assert_eq!(buffer.frames.capacity(),capacity);
  assert_eq!(buffer.audio(100),None);assert_eq!(buffer.song(100),None);
  assert_eq!(buffer.audio(200),Some([0.75;2]));assert_eq!(buffer.floor(),200);
 }

 #[test]
 fn reset_epoch_or_rewound_clock_cannot_recall_stale_audio_across_a_gap() {
  for epochs in [(Some(1),Some(2)),(None,None)] {
   let mut buffer=Buffer::default();buffer.prepare(8000).unwrap();
   let mut first=block(0,epochs.0,&[0.9;4]);
   first["midi_events"]=json!([{"capture_frame":0,"track":"midi","pitch":60,"velocity":1.0,"source":"keyboard"}]);
   ingest(&mut buffer,first);assert!(!buffer.held.is_empty());
   let address=buffer.frames.as_ptr();let capacity=buffer.frames.capacity();
   ingest(&mut buffer,block(0,epochs.1,&[0.1]));
   assert!(buffer.held.is_empty());assert!(buffer.midi.is_empty());
   ingest(&mut buffer,block(3,epochs.1,&[0.4]));
   assert_eq!(buffer.frames.as_ptr(),address);assert_eq!(buffer.frames.capacity(),capacity);
   assert_eq!(buffer.audio(0),Some([0.1;2]));assert_eq!(buffer.audio(3),Some([0.4;2]));
   for gap in [1,2] {assert_eq!(buffer.audio(gap),None);assert_eq!(buffer.song(gap),None);}
   assert_eq!(buffer.song(0),Some(Some(1000)));assert_eq!(buffer.song(3),Some(Some(1003)));
   let recalled=buffer.recall_audio(5,8000,0).unwrap();
   assert_eq!(recalled.start_frame,1003);assert_eq!(recalled.interleaved,vec![0.4]);
   assert!(recall::audio(recall::Range{first:0,end:4,start_frame:1000,length:4,mapped:true},false,|frame|buffer.audio(frame)).is_err());
  }
 }
}
impl Default for Buffer{fn default()->Self{Self{source:Source::Microphone,requested:Source::Microphone,rate:0,frames:Vec::new(),head:0,source_start:0,audio_clear:0,midi_clear:0,last_playing:None,serial_base:0,serial_start:0,midi:VecDeque::new(),held:BTreeMap::new(),aggregate:[None;128],tracks:BTreeMap::new(),next_track:0,epoch:None,recording:false,dropped_blocks:0,native_dropped_frames:0}}}
fn unsigned(v:&Value,key:&str)->Result<u64,String>{v[key].as_u64().ok_or_else(||format!("capture {key} required"))}
fn channel(v:&Value,n:usize)->Result<Vec<f32>,String>{v.as_array().filter(|a|a.len()==n).ok_or("capture channel length mismatch")?.iter().map(|x|x.as_f64().filter(|x|x.is_finite()&&x.abs()<=f32::MAX as f64).map(|x|x as f32).ok_or_else(||"invalid capture PCM".into())).collect()}
impl Block{fn parse(v:&Value)->Result<Self,String>{
 let rate=unsigned(v,"sample_rate")?;if !(8000..=192000).contains(&rate){return Err("invalid capture rate".into())}
 let n=unsigned(v,"frames")? as usize;if n>4096{return Err("capture block exceeds4096frames".into())}
 let start=unsigned(v,"capture_start_frame")?;let timeline=unsigned(v,"timeline_start_frame")?;start.checked_add(n as u64).ok_or("capture clock overflow")?;timeline.checked_add(n as u64).ok_or("timeline clock overflow")?;
 let playing=v["playing"].as_bool().ok_or("capture playing required")?;let recording=v["recording"].as_bool().ok_or("capture recording required")?;
 // Original AudioSystem primaryInput is physical input0. Absent physical input
 // is zero in Cardinal's retrospective callback; VM1 is a different producer.
 let mic=if let Some(v)=v["live_inputs"]["physical"].as_array().and_then(|a|a.first()){channel(v,n)?}else{vec![0.;n]};
 let player=if n==0{Vec::new()}else{let a=channel(&v["player_audio"][0],n)?;let b=channel(&v["player_audio"][1],n)?;a.into_iter().zip(b).map(|(l,r)|[l,r]).collect()};
 let events=v["midi_events"].as_array().map_or(&[][..],Vec::as_slice);if events.len()>8192{return Err("capture MIDI block exceeds8192events".into())}
 let mut midi=Vec::new();for e in events{let frame=unsigned(e,"capture_frame")?;if frame<start||frame>=start+n as u64{return Err("MIDI event outside accepted capture block".into())}
  let track=e["track"].as_str().filter(|s|!s.is_empty()&&s.len()<=128).ok_or("capture MIDI track required")?.to_owned();
  let pitch=unsigned(e,"pitch")?;if pitch>127{return Err("invalid MIDI pitch".into())}let velocity=e["velocity"].as_f64().filter(|v|v.is_finite()&&(0. ..=1.).contains(v)).ok_or("invalid MIDI velocity")?;
  let tuning=e.get("tuning_cents").map_or(Some(0.),Value::as_f64).filter(|x|x.is_finite()).ok_or("invalid MIDI tuning")?;
  let preview=match e["source"].as_str(){None|Some("keyboard")=>false,Some("preview")=>true,_=>return Err("unsupported MIDI capture source".into())};
  midi.push((track,recall::MidiEvent{frame,track:0,pitch:pitch as u8,velocity:velocity as f32,tuning_cents:tuning},preview));}
 if midi.windows(2).any(|p|p[0].1.frame>p[1].1.frame){return Err("MIDI capture order must be monotonic".into())}
 let loop_range=if let Some(r)=v.get("loop_range").filter(|r|!r.is_null()){let a=unsigned(r,"start_frame")?;let b=unsigned(r,"end_frame")?;if b<=a{return Err("invalid retrospective loop range".into())}Some([a,b])}else{None};
 Ok(Self{rate:rate as u32,start,timeline,playing,recording,mic,player,midi,epoch:v.get("capture_epoch").and_then(Value::as_u64),loop_range})
}}
impl Buffer{
 /// Prepare the full ring before native callbacks start. Until the first
 /// accepted block arrives, history still has no rate, head, or epoch.
 pub(crate) fn prepare(&mut self,rate:u32)->Result<(),String>{
  if self.rate!=0&&self.rate!=rate{return Err("cannot preallocate a different active retrospective rate".into())}
  self.prepare_storage(rate)
 }
 fn prepare_storage(&mut self,rate:u32)->Result<(),String>{
  if !(8000..=192000).contains(&rate){return Err("invalid retrospective sample rate".into())}
  let count=rate as usize*30;
  if self.frames.len()!=count {
   let mut frames=Vec::new();frames.try_reserve_exact(count).map_err(|_|"cannot allocate retrospective audio history")?;
   frames.resize(count,Frame{serial:0,audio:[0.;2],song:u64::MAX});self.frames=frames;
  }
  Ok(())
 }
 fn serial(&self,frame:u64)->Option<u64>{self.serial_base.checked_add(frame.checked_sub(self.serial_start)?)}
 fn emit(&mut self,event:recall::MidiEvent){if self.midi.len()==8192{self.midi.pop_front();}self.midi.push_back(event);}
 fn publish_pitch(&mut self,pitch:u8,frame:u64){
  let next=self.held.iter().filter(|((_,p,_),_)|*p==pitch).max_by(|(a,x),(b,y)|x.velocity.total_cmp(&y.velocity).then(a.2.cmp(&b.2))).map(|(_,e)|*e);
  let old=self.aggregate[pitch as usize];let changed=match(old,next){(Some(a),Some(b))=>a.track!=b.track||a.tuning_cents!=b.tuning_cents,(None,None)=>false,_=>true};
  if changed{if let Some(mut e)=old{e.frame=frame;e.velocity=0.;self.emit(e);}if let Some(mut e)=next{e.frame=frame;self.emit(e);}}self.aggregate[pitch as usize]=next;
 }
 pub(crate) fn release_keyboard(&mut self,frame:u64){let pitches:std::collections::BTreeSet<_>=self.held.keys().filter(|k|!k.2).map(|k|k.1).collect();self.held.retain(|k,_|k.2);for pitch in pitches{self.publish_pitch(pitch,frame);}}
 pub(crate) fn set_recording(&mut self,recording:bool){self.recording=recording;}
 pub(crate) fn request_source(&mut self,source:Source){self.requested=source;}
 pub(crate) fn clear(&mut self){self.audio_clear=self.head;self.midi_clear=self.head;}
 pub(crate) fn source(&self)->Source{self.source}
 pub(crate) fn rate(&self)->u32{self.rate}
 pub(crate) fn waiting(&self)->bool{self.source!=self.requested}
 pub(crate) fn head(&self)->u64{self.head}
 pub(crate) fn floor(&self)->u64{self.head.saturating_sub(self.frames.len() as u64).max(self.source_start).max(self.audio_clear)}
 pub(crate) fn audio(&self,frame:u64)->Option<[f32;2]>{if frame<self.floor()||frame>=self.head||self.frames.is_empty(){return None}let f=&self.frames[frame as usize%self.frames.len()];(Some(f.serial)==self.serial(frame)).then_some(f.audio)}
 fn song(&self,frame:u64)->Option<Option<u64>>{if frame>=self.head||self.frames.is_empty()||frame<self.head.saturating_sub(self.frames.len() as u64){return None}let f=&self.frames[frame as usize%self.frames.len()];(Some(f.serial)==self.serial(frame)).then_some((f.song!=u64::MAX).then_some(f.song))}
 fn reset(&mut self,rate:u32,start:u64,epoch:Option<u64>)->Result<(),String>{
  let serial_base=self.serial(self.head).and_then(|v|v.checked_add(1)).ok_or("retrospective capture serial exhausted")?;
  self.prepare_storage(rate)?;
  self.serial_base=serial_base;self.serial_start=start;
  self.rate=rate;self.head=start;self.source_start=start;self.audio_clear=start;self.midi_clear=start;self.last_playing=None;self.midi.clear();self.held.clear();self.aggregate=[None;128];self.tracks.clear();self.next_track=0;self.epoch=epoch;Ok(())
 }
 /// Validates the complete bounded drain before publishing any history.
 pub(crate) fn ingest(&mut self,drain:&Value)->Result<usize,String>{
  let blocks=drain["blocks"].as_array().filter(|a|a.len()<=8).ok_or("DrainCapture requires at most8blocks")?.iter().map(Block::parse).collect::<Result<Vec<_>,_>>()?;
  let mut accepted=0;for block in blocks{
   let changed_epoch=block.epoch.is_some()&&self.epoch.is_some()&&block.epoch!=self.epoch;
   if self.rate!=block.rate||self.frames.is_empty()||changed_epoch||block.start<self.head{self.reset(block.rate,block.start,block.epoch)?;}
   self.serial(block.start+block.player.len() as u64).ok_or("retrospective capture serial exhausted")?;
   if self.source!=self.requested{self.source=self.requested;self.source_start=block.start;}
   self.recording=block.recording;if block.epoch.is_some(){self.epoch=block.epoch;}
   if block.start>self.head{self.midi.clear();self.held.clear();self.aggregate=[None;128];}
   for(i,player)in block.player.iter().enumerate(){let frame=block.start+i as u64;let audio=if self.source==Source::Microphone{[block.mic[i];2]}else{*player};let slot=frame as usize%self.frames.len();self.frames[slot]=Frame{serial:self.serial(frame).unwrap(),audio,song:if block.playing{let at=block.timeline+i as u64;if let Some([a,b])=block.loop_range{if at>=b{a+(at-a)%(b-a)}else{at}}else{at}}else{u64::MAX}};}
   self.head=block.start+block.player.len() as u64;if block.playing&&!block.player.is_empty(){self.last_playing=Some(self.head-1);}
   for(track,mut event,preview)in block.midi{let id=if let Some(id)=self.tracks.get(&track){*id}else{let id=self.next_track;self.next_track=self.next_track.checked_add(1).ok_or("capture track identity capacity")?;self.tracks.insert(track.clone(),id);id};event.track=id;
    let key=(track,event.pitch,preview);if event.velocity>0.{self.held.insert(key,event);}else{self.held.remove(&key);}
    self.publish_pitch(event.pitch,event.frame);
   }
   if self.tracks.len()>8192{let used:std::collections::BTreeSet<_>=self.midi.iter().map(|e|e.track).collect();self.tracks.retain(|_,id|used.contains(id));}
   accepted+=block.player.len();
  }
  self.dropped_blocks=drain["dropped_blocks"].as_u64().unwrap_or(self.dropped_blocks);self.native_dropped_frames=drain["native_dropped_frames"].as_u64().unwrap_or(self.native_dropped_frames);Ok(accepted)
 }
 pub(crate) fn display(&self,display:&mut Display)->Result<usize,String>{if self.rate==0{return Ok(0)}display.configure(self.source,self.rate)?;display.drain(self.source,self.floor(),self.head,|frame|self.audio(frame))}
 fn range(&self,seconds:u32,audio:bool,rate:u32,playhead:u64)->Result<recall::Range,String>{if self.rate==0{return Err("Audio history is not available.".into())}if self.recording{return Err("Stop recording before recalling history.".into())}if audio&&self.waiting(){return Err("Waiting for the new capture source.".into())}recall::range(recall::Clock{head:self.head,capacity:self.frames.len() as u64,capture_rate:self.rate,document_rate:rate,source_start:self.source_start,audio_clear:self.audio_clear,midi_clear:self.midi_clear,last_playing_frame:self.last_playing,playhead_frame:playhead},seconds,audio,|frame|self.song(frame))}
 pub(crate) fn recall_audio(&self,seconds:u32,rate:u32,playhead:u64)->Result<recall::Audio,String>{let range=self.range(seconds,true,rate,playhead)?;recall::audio(range,self.source==Source::Player,|frame|self.audio(frame))}
 pub(crate) fn recall_midi(&self,seconds:u32,rate:u32,playhead:u64)->Result<(recall::Range,Vec<recall::Note>),String>{let range=self.range(seconds,false,rate,playhead)?;for frame in range.first..range.end{if self.song(frame).is_none(){return Err("MIDI history contains dropped capture frames.".into())}}let events:Vec<_>=self.midi.iter().copied().collect();let notes=recall::midi(range,self.rate,rate,&events)?;Ok((range,notes))}
 pub(crate) fn read_window(&self,requested:u64)->Value{let floor=self.floor();let first=if requested<floor||requested>self.head{floor}else{requested};let end=self.head.min(first.saturating_add(65536));json!({"sample_rate":self.rate,"source":self.source.name(),"floor_frame":floor,"first_frame":first,"head_frame":end,"capture_head_frame":self.head,"frames":(first..end).map(|i|self.audio(i)).collect::<Vec<_>>(),"status":self.status()})}
 pub(crate) fn status(&self)->Value{json!({"sample_rate":self.rate,"head_frame":self.head,"source_start_frame":self.source_start,"audio_clear_frame":self.audio_clear,"midi_clear_frame":self.midi_clear,"requested_source":self.requested.name(),"applied_source":self.source.name(),"waiting_for_source":self.waiting(),"dropped_blocks":self.dropped_blocks,"native_dropped_frames":self.native_dropped_frames})}
}
