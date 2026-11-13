//! Cardinal general-warp document adapter and single cancellable worker.
//! Clock conversions belong to the shared tempo/rubato map. No default tempo
//! is introduced. Source/spectral PCM is immutable, and stale jobs never publish.
use super::{document::{Content, State}, playback::Source, warp_dsp, warp_map::{Map, Marker}};
use serde_json::{Value,json};
use std::{collections::BTreeMap, sync::{Arc,atomic::{AtomicBool,Ordering}},thread::JoinHandle};
const MAX_VALUES:usize=16_777_216;
/// Immutable authoritative clock snapshot. Breakpoints must contain every
/// slope change between the requested seconds; conversions must extrapolate.
pub(crate) trait Clock:Send+Sync {
 fn revision(&self)->u64;
 fn cache_identity(&self)->Vec<u64>{vec![self.revision()]}
 fn for_sample_rate(&self,_rate:u32)->Result<Option<Arc<dyn Clock>>,String>{Ok(None)}
 fn seconds_to_beat(&self,seconds:f64)->Result<f64,String>;
 fn beat_to_seconds(&self,beat:f64)->Result<f64,String>;
 fn breakpoints(&self,start:f64,end:f64)->Result<Vec<f64>,String>;
}
#[derive(Clone,PartialEq,Eq)] struct Key(Vec<u64>);
#[derive(Clone)] struct Cache {key:Key, source:Source, input:Source}
#[derive(Clone)] pub(super) struct Entry {pub source_id:String,pub map:Map,cache:Option<Cache>,failed:Option<(Key,String,Source)>}
struct Work {id:String,key:Key,input:Source,cancel:Arc<AtomicBool>,handle:JoinHandle<Result<Option<Source>,String>>}
#[derive(Default)] pub(super) struct Engine {pub clock:Option<Arc<dyn Clock>>,pub entries:BTreeMap<String,Entry>,work:Option<Work>}
impl Drop for Engine {fn drop(&mut self){if let Some(w)=&self.work{w.cancel.store(true,Ordering::Relaxed);}}}
impl Engine {pub(super) fn frozen(&self)->Self{Self{clock:self.clock.clone(),entries:self.entries.clone(),work:None}}}
fn clock(state:&State,rate:u32)->Result<Arc<dyn Clock>,String>{let current=state.warp.clock.as_ref().ok_or_else(||"Warp requires an authoritative project tempo map.".to_owned())?.clone();Ok(current.for_sample_rate(rate)?.unwrap_or(current))}
fn data<'a>(state:&'a State,id:&str)->Result<(&'a str,f64,f64,f64,&'a Source),String>{
 let c=state.clips.get(id).ok_or("unknown audio clip")?;
 let Content::Audio{source,offset,duration}=&c.content else{return Err("warp requires an audio clip".into())};
 Ok((source, c.start,*offset,*duration,super::clip_treatment::render_source(state,id,source)?))
}
fn map_local(clock:&dyn Clock,map:&Map,start:f64,offset:f64,rate:u32,local:f64)->Result<f64,String>{
 let base=clock.seconds_to_beat(start)?;
 let beat=clock.seconds_to_beat(start+local)?-base+map.beat_at_source(offset*rate as f64);
 Ok(map.source_at_beat(beat))
}
pub(super) fn source_span(state:&State,id:&str)->Result<(usize,usize),String>{
 let (_,start,offset,duration,source)=data(state,id)?;let rate=source.sample_rate;
 let first=(offset*rate as f64).round() as usize;
 let end=if let Some(e)=state.warp.entries.get(id).filter(|e|e.map.enabled){map_local(clock(state,rate)?.as_ref(),&e.map,start,offset,rate,duration)?}else{(offset+duration)*rate as f64};
 if !end.is_finite()||end<=first as f64||end.round()>i64::MAX as f64/4.{return Err("warp range exceeds supported source coordinates".into())}
 Ok((first,end.round() as usize))
}
fn duration_for(clock:&dyn Clock,map:&Map,start:f64,offset:f64,rate:u32,source_end:f64)->Result<f64,String>{
 let base=clock.seconds_to_beat(start)?;let span=map.beat_at_source(source_end)-map.beat_at_source(offset*rate as f64);
 let duration=clock.beat_to_seconds(base+span)?-start;
 if !duration.is_finite()||duration<=0.||duration*rate as f64>MAX_VALUES as f64/2.||start+duration>86400.{return Err("warped clip exceeds time or cache bounds".into())}Ok(duration)
}
fn initial(state:&State,id:&str)->Result<Entry,String>{
 let (source_id,start,offset,duration,source)=data(state,id)?;let c=clock(state,source.sample_rate)?;
 let beats=c.seconds_to_beat(start+duration)?-c.seconds_to_beat(start)?;
 if !beats.is_finite()||beats<=0.{return Err("warp clock must increase over the clip".into())}
 let mut markers=vec![Marker{beat:0.,source:offset*source.sample_rate as f64,manual:true},Marker{beat:beats,source:(offset+duration)*source.sample_rate as f64,manual:true}];
 let breaks=c.breakpoints(start,start+duration)?;if breaks.len()>4094{return Err("tempo schedule exceeds warp marker bounds".into())}
 for time in breaks {if time>start&&time<start+duration{markers.push(Marker{beat:c.seconds_to_beat(time)?-c.seconds_to_beat(start)?,source:(offset+time-start)*source.sample_rate as f64,manual:false});}}
 let map=Map::from_markers(markers)?;
 Ok(Entry{source_id:source_id.into(),map,cache:None,failed:None})
}
pub(super) fn edit(state:&mut State,hook:&str,input:&Value)->Result<(),String>{
 let id=input["id"].as_str().ok_or("audio clip id required")?;let mut entry=state.warp.entries.get(id).cloned().map(Ok).unwrap_or_else(||initial(state,id))?;
 let (_,start,offset,duration,source)=data(state,id)?;let rate=source.sample_rate;
 let source_end=if entry.map.enabled{map_local(clock(state,rate)?.as_ref(),&entry.map,start,offset,rate,duration)?}else{(offset+duration)*rate as f64};
 if hook=="SetClipWarp" {entry.map.enabled=input["enabled"].as_bool().ok_or("enabled boolean required")?;}
 else if hook=="SetWarpMap" {
  let markers=input["markers"].as_array().filter(|m|m.len()<=4096).ok_or("bounded markers required")?.iter().map(|m|Ok(Marker{beat:m["destination_beat"].as_f64().ok_or("destination_beat required")?,source:m["source_frame"].as_f64().ok_or("source_frame required")?,manual:m["manual"]!=false})).collect::<Result<Vec<_>,String>>()?;
  entry.map.replace(markers)?;
  if let Some(p)=input.get("transpose_semitones"){let p=p.as_f64().filter(|p|p.is_finite()&&(-24. ..=24.).contains(p)).ok_or("transpose must be -24..24 semitones")?;entry.map.transpose_semitones=p;}
 }else{return Err("unknown warp edit".into())}
 let duration=if hook!="SetClipWarp"{duration}else if entry.map.enabled{duration_for(clock(state,rate)?.as_ref(),&entry.map,start,offset,rate,source_end)?}else{source_end/rate as f64-offset};
 if duration<=0.{return Err("warp range exceeds decoded source".into())}
 let retained:usize=state.warp.entries.iter().filter(|(k,_)|k.as_str()!=id).map(|(_,e)|e.map.markers().len()).sum();
 if retained+entry.map.markers().len()>65_536{return Err("document warp marker capacity reached".into())}
 entry.cache=None;entry.failed=None;
 if let Content::Audio{duration:d,..}=&mut state.clips.get_mut(id).unwrap().content{*d=duration;}
 state.warp.entries.insert(id.into(),entry);Ok(())
}
pub(super) fn moved_duration(state:&State,id:&str,start:f64)->Result<Option<f64>,String>{
 let Some(e)=state.warp.entries.get(id).filter(|e|e.map.enabled)else{return Ok(None)};
 let(_,_,offset,_,source)=data(state,id)?;let(_,end)=source_span(state,id)?;
 duration_for(clock(state,source.sample_rate)?.as_ref(),&e.map,start,offset,source.sample_rate,end as f64).map(Some)
}
pub(super) fn validate_source_replacement(state:&State,id:&str,source:&Source)->Result<(),String>{
 for e in state.warp.entries.values().filter(|e|e.source_id==id){
  if state.sources.get(id).is_some_and(|old|old.sample_rate!=source.sample_rate){return Err("warp source replacement must retain its sample rate".into())}
  if state.sources.get(id).is_some_and(|old|source.samples.len()/source.channels<old.samples.len()/old.channels)&&e.map.markers().iter().any(|m|m.source>source.samples.len() as f64/source.channels as f64){return Err("replacement source would truncate warp markers".into())}
 }Ok(())
}
pub(super) struct ClockChange {next:Arc<dyn Clock>,durations:Vec<(String,f64)>,caches:Vec<(String,Cache)>,pending_key:Option<Key>}
pub(super) fn prepare_clock_change(state:&State,next:Arc<dyn Clock>,shift:f64)->Result<ClockChange,String>{
 if !shift.is_finite()||shift<0.{return Err("invalid clock translation".into())}
 let mut durations=Vec::new();let mut caches=Vec::new();let mut pending_key=None;
 for(id,e)in &state.warp.entries{if e.map.enabled{
  let(_,start,offset,duration,source)=data(state,id)?;
  let old=clock(state,source.sample_rate)?;let end=map_local(old.as_ref(),&e.map,start,offset,source.sample_rate,duration)?;
  let target=next.for_sample_rate(source.sample_rate)?.unwrap_or_else(||next.clone());
  let duration_next=duration_for(target.as_ref(),&e.map,start+shift,offset,source.sample_rate,end)?;
  let old_plan=plan_at(state,id,old,start,duration)?;
  let new_plan=plan_at(state,id,target,start+shift,duration_next)?;
  // A pure origin translation can introduce sub-ULP cancellation in absolute
  // seconds. Keep the prior PCM for sub-1e-7-sample anchor differences during
  // rebases only; actual rate/origin changes of a sample still invalidate it.
  let same=old_plan.3==new_plan.3&&old_plan.4.to_bits()==new_plan.4.to_bits()
    &&old_plan.2.len()==new_plan.2.len()&&old_plan.2.iter().zip(&new_plan.2).all(|(a,b)|(a.destination.to_bits()==b.destination.to_bits()||(shift>0.&&(a.destination-b.destination).abs()<=1e-7))&&a.source.to_bits()==b.source.to_bits());
  if same{
   if let Some(c)=e.cache.as_ref().filter(|c|c.key==old_plan.0){let mut c=c.clone();c.key=new_plan.0.clone();caches.push((id.clone(),c));}
   if state.warp.work.as_ref().is_some_and(|w|w.id==*id&&w.key==old_plan.0&&!w.cancel.load(Ordering::Relaxed)){pending_key=Some(new_plan.0);}
  }
  durations.push((id.clone(),duration_next));
 }}Ok(ClockChange{next,durations,caches,pending_key})
}
pub(super) fn apply_clock_change(state:&mut State,change:ClockChange){
 for(id,d)in change.durations{if let Content::Audio{duration,..}=&mut state.clips.get_mut(&id).expect("preflighted warp clip").content{*duration=d;}}
 state.warp.clock=Some(change.next);for e in state.warp.entries.values_mut(){e.cache=None;e.failed=None;}
 for(id,cache)in change.caches{state.warp.entries.get_mut(&id).expect("preflighted warp cache").cache=Some(cache);}
 if let(Some(key),Some(work))=(change.pending_key,state.warp.work.as_mut()){work.key=key;}
}
pub(super) fn install_clock(state:&mut State,next:Arc<dyn Clock>)->Result<(),String>{let change=prepare_clock_change(state,next,0.)?;apply_clock_change(state,change);Ok(())}

fn plan(state:&State,id:&str)->Result<(Key,Source,Vec<warp_dsp::Anchor>,usize,f64),String>{
 let(_,start,_,duration,source)=data(state,id)?;plan_at(state,id,clock(state,source.sample_rate)?,start,duration)
}
fn plan_at(state:&State,id:&str,clock:Arc<dyn Clock>,start:f64,duration:f64)->Result<(Key,Source,Vec<warp_dsp::Anchor>,usize,f64),String>{
 let e=state.warp.entries.get(id).ok_or("missing warp map")?;let(_,_,offset,_,source)=data(state,id)?;let rate=source.sample_rate as f64;
 let end=map_local(clock.as_ref(),&e.map,start,offset,source.sample_rate,duration)?;
 if !end.is_finite()||end<=offset*rate||end>i64::MAX as f64/4.{return Err("warp range exceeds supported source coordinates".into())}
 let base=clock.seconds_to_beat(start)?;let origin=e.map.beat_at_source(offset*rate);
 let mut anchors=Vec::new();for m in e.map.markers(){anchors.push(warp_dsp::Anchor{destination:(clock.beat_to_seconds(base+m.beat-origin)?-start)*rate,source:m.source});}
 let first=anchors.first().ok_or("empty map")?.destination/rate+start;let last=anchors.last().unwrap().destination/rate+start;
 let breaks=clock.breakpoints(first,last)?;if breaks.len()>4096{return Err("tempo schedule exceeds4096 breakpoints".into())}for s in breaks {if s>first&&s<last{anchors.push(warp_dsp::Anchor{destination:(s-start)*rate,source:map_local(clock.as_ref(),&e.map,start,offset,source.sample_rate,s-start)?});}}
 anchors.sort_by(|a,b|a.destination.total_cmp(&b.destination));anchors.dedup_by(|a,b|a.destination==b.destination);
 if anchors.len()>4096{return Err("tempo/warp schedule exceeds4096 anchors".into())}
 let frames=(duration*rate).round() as usize;let pitch=2f64.powf(e.map.transpose_semitones/12.);
 let mut words=clock.cache_identity();words.extend([start.to_bits(),offset.to_bits(),duration.to_bits(),rate.to_bits(),pitch.to_bits(),source.samples.as_ptr() as usize as u64,source.samples.len() as u64,source.channels as u64]);
 for a in &anchors{words.extend([a.destination.to_bits(),a.source.to_bits()]);}
 Ok((Key(words),source.clone(),anchors,frames,pitch))
}
fn identity(anchors:&[warp_dsp::Anchor],pitch:f64)->bool{if (pitch-1.).abs()>1e-8{return false}let origin=anchors[0].source-anchors[0].destination;anchors.iter().all(|a|(a.source-origin-a.destination).abs()<1e-5)}
/// At most one OS worker per document. Called on the control thread once/frame.
/// Returns true only when a current result/error is published.
pub(super) fn pump(state:&mut State)->bool{
 let plans:Vec<_>=state.warp.entries.iter().filter(|(_,e)|e.map.enabled).map(|(id,_)|(id.clone(),plan(state,id))).collect();
 let mut changed=false;
 for(id,p)in &plans{if let Some(e)=state.warp.entries.get_mut(id){if e.cache.as_ref().is_some_and(|c|!p.as_ref().is_ok_and(|p|p.0==c.key)){e.cache=None;}if e.failed.as_ref().is_some_and(|f|!p.as_ref().is_ok_and(|p|p.0==f.0)){e.failed=None;}}}
 if let Some(work)=&state.warp.work{let wanted=plans.iter().any(|(id,p)|*id==work.id&&p.as_ref().is_ok_and(|p|p.0==work.key));if !wanted{work.cancel.store(true,Ordering::Relaxed);}}
 if state.warp.work.as_ref().is_some_and(|w|w.handle.is_finished()){
  let work=state.warp.work.take().unwrap();let wanted=plans.iter().any(|(id,p)|*id==work.id&&p.as_ref().is_ok_and(|p|p.0==work.key));
  let result=work.handle.join().unwrap_or_else(|_|Err("warp worker failed".into()));
  if wanted{if let Some(e)=state.warp.entries.get_mut(&work.id){match result{Ok(Some(source))=>{e.cache=Some(Cache{key:work.key,source,input:work.input});e.failed=None;changed=true},Ok(None)=>{},Err(error)=>{e.failed=Some((work.key,error,work.input));changed=true}}}}
 }
 if state.warp.work.is_none(){
  for(id,plan)in plans{
   let(key,source,anchors,frames,pitch)=match plan{Ok(p)=>p,Err(_)=>continue};
   let e=&state.warp.entries[&id];if identity(&anchors,pitch){continue}if e.cache.as_ref().is_some_and(|c|c.key==key)||e.failed.as_ref().is_some_and(|e|e.0==key){continue}
   let retained:usize=state.warp.entries.values().filter_map(|e|e.cache.as_ref()).map(|c|c.source.samples.len()).sum();
   let budget=frames.checked_mul(2).and_then(|v|v.checked_add(retained)).is_some_and(|v|v<=MAX_VALUES);
   let working=source.samples.len()/source.channels*8+((frames as f64*pitch).ceil() as usize)*12+frames*8+4*1024*1024+retained*4;
   if !budget||working>256*1024*1024{state.warp.entries.get_mut(&id).unwrap().failed=Some((key,"warp cache memory capacity reached".into(),source.clone()));changed=true;continue}
   let cancel=Arc::new(AtomicBool::new(false));let worker_cancel=cancel.clone();let input=source.clone();let rate=source.sample_rate;let channels=source.channels;
   let handle=std::thread::Builder::new().name("Cardinal warp".into()).spawn(move||{
    let left:Vec<f32>=source.samples.chunks_exact(channels).map(|f|f[0]).collect();let right:Vec<f32>=if channels>1{source.samples.chunks_exact(channels).map(|f|f[1]).collect()}else{Vec::new()};
    let result=warp_dsp::render(warp_dsp::Input{left:&left,right:&right,anchors:&anchors,frames,sample_rate:rate as f64,pitch},Some(&worker_cancel))?;
    Ok(result.map(|o|Source{sample_rate:rate,channels:2,samples:o.left.into_iter().zip(o.right).flat_map(|(a,b)|[a,b]).collect::<Vec<_>>().into()}))
   });
   match handle{Ok(handle)=>state.warp.work=Some(Work{id,key,input,cancel,handle}),Err(error)=>{state.warp.entries.get_mut(&id).unwrap().failed=Some((key,error.to_string(),input));changed=true}}
   break;
  }
 }changed
}
/// Cardinal's hybrid mode is silent for this clip until its matching cache is
/// ready. It never substitutes pitched linear resampling or an old job's PCM.
pub(super) fn render_source<'a>(state:&'a State,id:&str,source_id:&str,offset:f64)->Result<Option<(&'a Source,f64)>,String>{
 let Some(e)=state.warp.entries.get(id).filter(|e|e.map.enabled)else{return Ok(Some((super::clip_treatment::render_source(state,id,source_id)?,offset)))};
 let(key,_,anchors,_,pitch)=plan(state,id)?;if identity(&anchors,pitch){return Ok(Some((super::clip_treatment::render_source(state,id,source_id)?,offset)))}Ok(e.cache.as_ref().filter(|c|c.key==key).map(|c|(&c.source,0.)))
}
pub(super) fn ready_for_export(state:&State)->Result<(),String>{for(id,e)in &state.warp.entries{if e.map.enabled{let(key,_,anchors,_,pitch)=plan(state,id)?;if identity(&anchors,pitch){continue}if !e.cache.as_ref().is_some_and(|c|c.key==key){return Err(format!("Warp audio for {id} is not ready for export."))}}}Ok(())}
pub(super) fn snapshot(state:&State)->Value{json!(state.warp.entries.iter().map(|(id,e)|{
 let plan=plan(state,id);let status=if !e.map.enabled{"disabled"}else if plan.as_ref().is_ok_and(|p|identity(&p.2,p.4)||e.cache.as_ref().is_some_and(|c|c.key==p.0)){"ready"}else if plan.is_err()||plan.as_ref().is_ok_and(|p|e.failed.as_ref().is_some_and(|e|e.0==p.0)){"error"}else{"preparing"};
 let error=plan.err().or_else(||e.failed.as_ref().map(|e|e.1.clone()));
 json!({"id":id,"enabled":e.map.enabled,"status":status,"error":error,"transpose_semitones":e.map.transpose_semitones,"markers":e.map.markers().iter().map(|m|json!({"destination_beat":m.beat,"source_frame":m.source,"manual":m.manual})).collect::<Vec<_>>()})
}).collect::<Vec<_>>())}
