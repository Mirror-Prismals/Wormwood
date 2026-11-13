//! Atomic retrospective insertion behind bounded Host requests. Upload chunks
//! are not document sources until Commit; stale/failing commits publish nothing.
use super::{document::{self,State as Document,Clip,Content,Note},playback::Source};
use serde_json::{Value,json};
use std::collections::BTreeMap;
const MAX_VALUES:usize=16_777_216;
#[derive(Clone)] struct ContentSnapshot{clips:BTreeMap<String,Clip>,order:Vec<String>,treatments:BTreeMap<String,super::clip_treatment::Treatment>,warps:BTreeMap<String,super::warp_treatment::Entry>}
impl ContentSnapshot{fn take(d:&Document)->Self{Self{clips:d.clips.clone(),order:d.clip_order.clone(),treatments:d.clip_treatments.clone(),warps:d.warp.entries.clone()}}fn apply(self,d:&mut Document){d.clips=self.clips;d.clip_order=self.order;d.clip_treatments=self.treatments;d.warp.entries=self.warps;}}
struct Upload{revision:u64,track:String,start:u64,rate:u32,channels:usize,frames:usize,samples:Vec<f32>}
struct Edit{before:ContentSnapshot,after:ContentSnapshot,selected:String}
#[derive(Default)]pub(super) struct State{serial:u64,uploads:BTreeMap<u64,Upload>,undo:Vec<Edit>,redo:Vec<Edit>,journal_revision:Option<u64>}
fn number(v:&Value,k:&str)->Result<u64,String>{v[k].as_u64().ok_or_else(||format!("{k} must be an unsigned frame integer"))}
fn rate(v:&Value)->Result<u32,String>{number(v,"sample_rate").and_then(|r|if (8000..=192000).contains(&r){Ok(r as u32)}else{Err("sample_rate must be8–192kHz".into())})}
fn target(d:&mut Document,input:&Value,kind:&str)->Result<String,String>{
 if input["document_revision"].as_u64()!=Some(d.revision){return Err("stale or missing document_revision".into())}
 let id=input["track"].as_str().ok_or("recall destination track required")?;
 if !d.tracks.get(id).is_some_and(|t|t.kind==kind){return Err(format!("Select an {kind} lane before recalling history."))}
 let snapshot=document::dispatch(d,"Snapshot",&Value::Null)?;
 if snapshot["recording"]["recording"]==true{return Err("Stop recording before recalling history.".into())}
 if snapshot["tracks"].as_array().into_iter().flatten().find(|t|t["id"]==id).is_some_and(|t|t["frozen"]==true||t["freeze_state"]=="frozen"){return Err("Frozen lanes cannot accept recalled audio.".into())}
 Ok(id.into())
}
fn revision(d:&Document,v:&Value)->Result<(),String>{if v["document_revision"].as_u64()==Some(d.revision){Ok(())}else{Err("stale or missing document_revision".into())}}
fn duration(start:u64,frames:usize,rate:u32)->Result<(f64,f64),String>{let begin=start as f64/rate as f64;let duration=frames as f64/rate as f64;if frames==0||frames>30*rate as usize||!begin.is_finite()||begin+duration>86400.{return Err("recall range exceeds30seconds or document bounds".into())}Ok((begin,duration))}
fn scalar(v:&Value)->Result<f32,String>{v.as_f64().filter(|v|v.is_finite()&&v.abs()<=f32::MAX as f64).map(|v|v as f32).ok_or_else(||"recall PCM must fit a finite32-bit sample".into())}
fn unique(d:&Document,stem:&str,token:u64)->String{let mut suffix=0;loop{let id=format!("{stem}-{token}-{suffix}");if !d.clips.contains_key(&id)&&!d.sources.contains_key(&id){return id}suffix+=1;}}
fn remember(d:&mut Document,before:ContentSnapshot,selected:String){let after=ContentSnapshot::take(d);d.retrospective_insert.undo.push(Edit{before,after,selected});if d.retrospective_insert.undo.len()>16{d.retrospective_insert.undo.remove(0);}d.retrospective_insert.redo.clear();d.retrospective_changed();d.retrospective_insert.journal_revision=Some(d.revision);}
fn clipped_offset(d:&Document,old:&Clip,new_start:f64,source_id:&str,offset:f64)->Result<f64,String>{
 if let Some(warp)=d.warp.entries.get(&old.id).filter(|e|e.map.enabled){let clock=d.warp.clock.as_ref().ok_or("warp clock unavailable while trimming")?;let rate=d.sources.get(source_id).ok_or("source missing while trimming")?.sample_rate as f64;let beat=warp.map.beat_at_source(offset*rate)+clock.seconds_to_beat(new_start)?-clock.seconds_to_beat(old.start)?;Ok(warp.map.source_at_beat(beat)/rate)}else{Ok(offset+new_start-old.start)}
}
fn insert_audio(d:&mut Document,upload:&Upload,token:u64)->Result<Value,String>{
 let request=json!({"document_revision":upload.revision,"track":upload.track});target(d,&request,"audio")?;
 let (start,length)=duration(upload.start,upload.frames,upload.rate)?;
 if upload.samples.len()!=upload.frames*upload.channels{return Err("recall upload is incomplete".into())}
 if d.sources.len()>=256||d.sources.values().map(|s|s.samples.len()).sum::<usize>()+upload.samples.len()>MAX_VALUES{return Err("decoded source capacity reached; recall was not inserted".into())}
 let before=ContentSnapshot::take(d);let mut next=before.clone();let clip_id=unique(d,"recalled-audio",token);let source_id=unique(d,"retrospective-source",token);let end=start+length;
 let affected:Vec<Clip>=d.clips.values().filter(|c|c.track==upload.track&&c.start<end&&c.start+c.duration()>start).cloned().collect();
 for old in affected{
  let Content::Audio{source,offset,duration:old_duration}=&old.content else{return Err("audio lane contains nonaudio clip".into())};let old_end=old.start+old_duration;
  next.clips.remove(&old.id);next.treatments.remove(&old.id);next.warps.remove(&old.id);
  let mut pieces=Vec::new();if old.start<start{let mut left=old.clone();if let Content::Audio{duration,..}=&mut left.content{*duration=start-old.start;}pieces.push(left);}
  if old_end>end{let mut right=old.clone();if !pieces.is_empty(){right.id=format!("{}-right-{}",clip_id,pieces.len()+next.clips.len());}right.start=end;right.content=Content::Audio{source:source.clone(),offset:clipped_offset(d,&old,end,source,*offset)?,duration:old_end-end};pieces.push(right);}
  for piece in pieces{if let Some(t)=d.clip_treatments.get(&old.id){next.treatments.insert(piece.id.clone(),t.clone());}if let Some(w)=d.warp.entries.get(&old.id){next.warps.insert(piece.id.clone(),w.clone());}next.clips.insert(piece.id.clone(),piece);}
 }
 next.clips.insert(clip_id.clone(),Clip { output:String::new(),lane:0., gain:1., fade_in:0., fade_out:0.,id:clip_id.clone(),track:upload.track.clone(),start,content:Content::Audio{source:source_id.clone(),offset:0.,duration:length}});
 if next.clips.len()>512{return Err("clip capacity reached; recall was not inserted".into())}
 let mut target_order:Vec<_>=next.clips.values().filter(|c|c.track==upload.track).collect();target_order.sort_by(|a,b|a.start.total_cmp(&b.start).then(a.id.cmp(&b.id)));
 next.order.retain(|id|next.clips.get(id).is_some_and(|c|c.track!=upload.track));next.order.extend(target_order.iter().map(|c|c.id.clone()));
 let source=Source{sample_rate:upload.rate,channels:upload.channels,samples:upload.samples.clone().into()};
 next.apply(d);d.sources.insert(source_id,source);remember(d,before,clip_id.clone());Ok(json!({"revision":d.revision,"selected_clip":clip_id,"selected_track":upload.track}))
}
pub(super) fn dispatch(d:&mut Document,hook:&str,v:&Value)->Result<Value,String>{match hook{
 "BeginRetrospectiveAudio"=>{let track=target(d,v,"audio")?;let rate=rate(v)?;let frames=usize::try_from(number(v,"frames")?).map_err(|_|"capture frame overflow")?;let start=number(v,"start_frame")?;duration(start,frames,rate)?;let channels=number(v,"channels")? as usize;if !(1..=2).contains(&channels){return Err("recall audio needs one or two channels".into())}let reserved:usize=d.retrospective_insert.uploads.values().map(|u|u.frames*u.channels).sum();if d.retrospective_insert.uploads.len()>=8||reserved+frames*channels>MAX_VALUES{return Err("recall upload capacity reached".into())}let token=d.retrospective_insert.serial.checked_add(1).ok_or("recall transaction sequence exhausted")?;d.retrospective_insert.serial=token;d.retrospective_insert.uploads.insert(token,Upload{revision:d.revision,track,start,rate,channels,frames,samples:Vec::new()});Ok(json!({"upload":token,"accepted_values":0}))},
 "AppendRetrospectiveAudio"=>{let token=number(v,"upload")?;let chunk=v["samples"].as_array().filter(|a|!a.is_empty()&&a.len()<=393216).ok_or("recall chunk needs1..393216values")?;let values=chunk.iter().map(scalar).collect::<Result<Vec<_>,_>>()?;let upload=d.retrospective_insert.uploads.get_mut(&token).ok_or("unknown recall upload")?;if upload.samples.len()+values.len()>upload.frames*upload.channels{return Err("recall chunk exceeds declared length".into())}upload.samples.extend(values);Ok(json!({"upload":token,"accepted_values":upload.samples.len()}))},
 "CommitRetrospectiveAudio"=>{revision(d,v)?;let token=number(v,"upload")?;let upload=d.retrospective_insert.uploads.remove(&token).ok_or("unknown recall upload")?;let result=insert_audio(d,&upload,token);if result.is_err(){d.retrospective_insert.uploads.insert(token,upload);}result},
 "AbortRetrospectiveAudio"=>{let token=number(v,"upload")?;d.retrospective_insert.uploads.remove(&token);Ok(json!({"aborted":true}))},
 "InsertRetrospectiveMidi"=>{let track=target(d,v,"midi")?;let rate=rate(v)?;let start=number(v,"start_frame")?;let frames=usize::try_from(number(v,"frames")?).map_err(|_|"recall frame overflow")?;let(begin,length)=duration(start,frames,rate)?;let bpm=v["bpm"].as_f64().filter(|v|v.is_finite()&&(1. ..=1000.).contains(v)).ok_or("destination piano-roll BPM required")?;
  let input=v["notes"].as_array().filter(|n|!n.is_empty()&&n.len()<=8192).ok_or("recall MIDI needs1..8192notes")?;let count=d.clips.values().map(|c|match &c.content{Content::Midi{notes,..}=>notes.len(),_=>0}).sum::<usize>();if count+input.len()>8192||d.clips.len()>=512{return Err("MIDI recall exceeds document capacity".into())}
  let mut notes=Vec::new();for(i,n)in input.iter().enumerate(){let offset=number(n,"start_frame")?;let len=number(n,"length_frames")?;if len==0||offset>=frames as u64||offset.checked_add(len).is_none_or(|n|n>frames as u64){return Err("MIDI note exceeds recalled range".into())}let pitch=number(n,"pitch")?;if pitch>127{return Err("invalid MIDI note pitch".into())}let vel=n["velocity"].as_f64().filter(|v|v.is_finite()&&(0. ..=1.).contains(v)).ok_or("invalid MIDI velocity")?;let cents=super::piano_tuning::cents(&n["tuning_cents"],pitch as u8)?;notes.push(Note{bend:0.,tuning_cents:cents,id:format!("recalled-note-{i}"),quarter:offset as f64/rate as f64*bpm/60.,length:len as f64/rate as f64*bpm/60.,pitch:pitch as u8,velocity:vel});}
  let numerator=v["meter"]["numerator"].as_u64().unwrap_or(4);let denominator=v["meter"]["denominator"].as_u64().unwrap_or(4);if !(1..=32).contains(&numerator)||![1,2,4,8,16,32].contains(&denominator){return Err("invalid destination MIDI meter".into())}
  let token=d.retrospective_insert.serial.checked_add(1).ok_or("recall sequence exhausted")?;let id=unique(d,"recalled-midi",token);let before=ContentSnapshot::take(d);d.clips.insert(id.clone(),Clip { output:String::new(),lane:0., gain:1., fade_in:0., fade_out:0.,id:id.clone(),track:track.clone(),start:begin,content:Content::Midi{edo:12,scale:0,root:0,bpm,quarters:length*bpm/60.,numerator,denominator,notes}});d.clip_order.push(id.clone());d.retrospective_insert.serial=token;remember(d,before,id.clone());Ok(json!({"revision":d.revision,"selected_clip":id,"selected_track":track}))},
 "UndoRetrospective"|"RedoRetrospective"=>{revision(d,v)?;if d.retrospective_insert.journal_revision!=Some(d.revision){return Err("recall undo history changed after another document edit".into())}let undo=hook=="UndoRetrospective";let edit=if undo{d.retrospective_insert.undo.pop()}else{d.retrospective_insert.redo.pop()}.ok_or("no retrospective edit to undo or redo")?;let target=if undo{edit.before.clone()}else{edit.after.clone()};target.apply(d);let selected=(!undo).then_some(edit.selected.clone());if undo{d.retrospective_insert.redo.push(edit)}else{d.retrospective_insert.undo.push(edit)}d.retrospective_changed();d.retrospective_insert.journal_revision=Some(d.revision);Ok(json!({"revision":d.revision,"selected_clip":selected}))},
 _=>Err("unknown retrospective operation".into())}}
