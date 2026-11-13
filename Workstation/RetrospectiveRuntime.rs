//! One global preparation-worker consumer of accepted native capture blocks.
//! EnvironmentAudioSystem owns Service for the audio lifetime, including while
//! the DAW is closed. UI owns only its existing bounded display mirror.
use super::{retrospective_buffer::Buffer,loopback_history::{History,Source,Copy as AudioCopy}};
use crate::host::Host;
use serde_json::{Value,json};
#[derive(Default)]pub(crate) struct Service{pub buffer:Buffer}
impl Service{
 pub(crate) fn observe(&mut self,block:&Value)->Result<(),String>{self.buffer.ingest(&json!({"blocks":[block]})).map(|_|())}
 pub(crate) fn command(&mut self,host:&mut Host,input:&Value,rate:u32,playhead:u64,recording:bool)->Result<Value,String>{
  self.buffer.set_recording(recording);
  match input["action"].as_str(){
   Some("source")=>{let source=match input["source"].as_str(){Some("MIC")=>Source::Microphone,Some("PLAYER")|Some("PLAYER AUDIO")=>Source::Player,_=>return Err("retrospective source must be MIC or PLAYER".into())};self.buffer.request_source(source);Ok(self.buffer.status())},
   Some("clear")=>{self.buffer.clear();Ok(self.buffer.status())},
   Some("read")=>Ok(self.buffer.read_window(input["from_frame"].as_u64().unwrap_or(0))),
   Some("status")=>Ok(self.buffer.status()),
   Some("recall_audio")=>{let track=input["track"].as_str().ok_or("Select an audio lane, then Recall Audio.")?;let audio=self.buffer.recall_audio(input["seconds"].as_u64().unwrap_or(30).clamp(5,30) as u32,rate,playhead)?;let mut result=upload_audio(host,track,audio.start_frame,rate,audio.channels,&audio.interleaved)?;result["notice"]=json!(if audio.mapped{"Recalled audio at its original timeline position."}else{"Recalled stopped audio at the playhead."});Ok(result)},
   Some("recall_midi")=>{let track=input["track"].as_str().ok_or("Select a MIDI note lane, then Recall MIDI.")?;let(range,notes)=self.buffer.recall_midi(input["seconds"].as_u64().unwrap_or(30).clamp(5,30) as u32,rate,playhead)?;let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;let clip=input["clip_id"].as_str().and_then(|id|snapshot["clips"].as_array()?.iter().find(|c|c["id"]==id&&c["track"]==track&&c["kind"]=="midi"));
    // Same exact elapsed-time encoding used by staged MidiCapture:120/4:4 is a
    // local storage basis when no destination piano roll exists, not a project clock.
    let bpm=clip.and_then(|c|c["bpm"].as_f64()).unwrap_or(120.);let meter=clip.map(|c|c["meter"].clone()).unwrap_or_else(||json!({"numerator":4,"denominator":4}));
    let mut result=host.invoke("WorkstationDataSystem.InsertRetrospectiveMidi",json!({"document_revision":snapshot["revision"],"track":track,"sample_rate":rate,"start_frame":range.start_frame,"frames":range.length,"bpm":bpm,"meter":meter,"notes":notes.iter().map(|n|json!({"pitch":n.pitch,"start_frame":n.start_frame,"length_frames":n.length,"velocity":n.velocity,"tuning_cents":n.tuning_cents})).collect::<Vec<_>>()}))?;
    result["notice"]=json!(if range.mapped{"Recalled MIDI at its original timeline position."}else{"Recalled stopped MIDI at the playhead."});Ok(result)},
   Some("undo")|Some("redo")=>{let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;host.invoke(if input["action"]=="undo"{"WorkstationDataSystem.UndoRetrospective"}else{"WorkstationDataSystem.RedoRetrospective"},json!({"document_revision":snapshot["revision"]}))},
   _=>Err("unknown retrospective action".into())
  }
 }
}
/// Uses bounded upload hooks; only the final Commit can create source/clip data.
fn upload_audio(host:&mut Host,track:&str,start:u64,rate:u32,channels:usize,samples:&[f32])->Result<Value,String>{
 if !(1..=2).contains(&channels)||samples.len()%channels!=0{return Err("incomplete retrospective audio frames".into())}
 let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
 let upload=host.invoke("WorkstationDataSystem.BeginRetrospectiveAudio",json!({"document_revision":snapshot["revision"],"track":track,"start_frame":start,"sample_rate":rate,"channels":channels,"frames":samples.len()/channels}))?;
 let token=upload["upload"].clone();let run=(||{for chunk in samples.chunks(393216){host.invoke("WorkstationDataSystem.AppendRetrospectiveAudio",json!({"upload":token,"samples":chunk}))?;}host.invoke("WorkstationDataSystem.CommitRetrospectiveAudio",json!({"document_revision":snapshot["revision"],"upload":token}))})();
 if run.is_err(){let _=host.invoke("WorkstationDataSystem.AbortRetrospectiveAudio",json!({"upload":token}));}run
}
pub(crate) fn update_display(host:&mut Host,display:&mut History)->Result<Value,String>{
 let data=host.invoke("EnvironmentAudioSystem.Retrospective",json!({"action":"read","from_frame":display.head()}))?;
 let rate=data["sample_rate"].as_u64().unwrap_or(0) as u32;if rate==0{return Ok(data["status"].clone())}
 let source=if data["source"]=="MIC"{Source::Microphone}else{Source::Player};display.configure(source,rate)?;
 let first=data["first_frame"].as_u64().ok_or("retrospective read first frame required")?;let floor=data["floor_frame"].as_u64().ok_or("retrospective read floor required")?;let head=data["head_frame"].as_u64().ok_or("retrospective read head required")?;
 let frames=data["frames"].as_array().ok_or("retrospective read frames required")?;if frames.len()>65536||head-first!=frames.len() as u64{return Err("invalid retrospective read span".into())}
 display.drain(source,floor,head,|at|{let v=frames.get(at.checked_sub(first)? as usize)?;Some([v[0].as_f64()? as f32,v[1].as_f64()? as f32])})?;Ok(data["status"].clone())
}
/// Arrangement owns pointer->audio-lane/start/snapping. A released copy already
/// owns its PCM, so later ring wrap cannot change the requested dropped audio.
pub(crate) fn insert_copy(host:&mut Host,audio:&AudioCopy,track:&str,start:u64,rate:u32)->Result<Value,String>{
 if !(8000..=192000).contains(&rate)||!(8000..=192000).contains(&audio.sample_rate)||!(1..=2).contains(&audio.channels)||audio.interleaved.len()%audio.channels!=0{return Err("invalid loopback copy format".into())}
 if audio.sample_rate==rate{return upload_audio(host,track,start,rate,audio.channels,&audio.interleaved)}
 let count=audio.interleaved.len()/audio.channels;let frames=(count as f64*rate as f64/audio.sample_rate as f64).round() as usize;if count==0||frames==0||frames>30*rate as usize{return Err("loopback copy exceeds bounded duration".into())}
 let mut pcm=Vec::with_capacity(frames*audio.channels);for i in 0..frames{let p=i as f64*count as f64/frames as f64;let a=(p as usize).min(count-1);let b=(a+1).min(count-1);let f=(p-a as f64)as f32;for ch in 0..audio.channels{let x=audio.interleaved[a*audio.channels+ch];let y=audio.interleaved[b*audio.channels+ch];pcm.push(x+(y-x)*f);}}
 upload_audio(host,track,start,rate,audio.channels,&pcm)
}
