//! Clip-local linear amplitude envelopes, shared by playback and export.
use super::document::{Clip,Content,State};
use serde_json::{Value,json};
#[derive(Default)]pub(super) struct History {revision:Option<u64>,undo:Vec<Change>,redo:Vec<Change>}
#[derive(Clone)]struct Change{id:String,before:[f64;2],after:[f64;2]}
pub(super) fn validate(c:&Clip)->Result<(),String>{
 let f=[c.fade_in,c.fade_out];
 if f.iter().any(|v|!v.is_finite()||*v<0.||*v>c.duration()) {return Err("fade duration must be finite within the clip duration".into());}
 if !matches!(c.content,Content::Audio{..})&&f!=[0.,0.] {return Err("fades belong to audio clips".into());}Ok(())
}
pub(super) fn edit(d:&mut State,v:&Value)->Result<Value,String>{
 let action=v["action"].as_str().unwrap_or("set");
 let change=match action {
  "set"=>{let id=v["id"].as_str().ok_or("audio clip id required")?;let c=d.clips.get(id).ok_or("unknown clip")?;
   if !matches!(c.content,Content::Audio{..}){return Err("fades belong to audio clips".into());}
   let get=|k:&str|v[k].as_f64().filter(|n|n.is_finite()&&*n>=0.&&*n<=c.duration()).ok_or_else(||format!("{k} must be within clip duration"));
   Change{id:id.into(),before:[c.fade_in,c.fade_out],after:[get("fade_in_seconds")?,get("fade_out_seconds")?]}},
  "undo"|"redo"=>{if d.fade_history.revision!=Some(d.revision){return Err("fade history changed after another document edit".into());}
   let stack=if action=="undo"{&d.fade_history.undo}else{&d.fade_history.redo};stack.last().cloned().ok_or("no fade edit to undo or redo")?},
  _=>return Err("unknown fade action".into())};
 let values=if action=="undo"{change.before}else{change.after};
 let c=d.clips.get_mut(&change.id).ok_or("fade clip no longer exists")?;
 let mut next=c.clone();next.fade_in=values[0];next.fade_out=values[1];validate(&next)?;
 if action=="set"&&change.before==change.after{return Ok(json!({"revision":d.revision}));}
 *c=next;
 if action=="set" {if d.fade_history.revision!=Some(d.revision){d.fade_history.undo.clear();}d.fade_history.redo.clear();d.fade_history.undo.push(change);if d.fade_history.undo.len()>32{d.fade_history.undo.remove(0);}}
 else if action=="undo"{d.fade_history.undo.pop();d.fade_history.redo.push(change);}else{d.fade_history.redo.pop();d.fade_history.undo.push(change);}
 let result=d.changed();d.fade_history.revision=Some(d.revision);Ok(result)
}
/// Endpoints use the actual clip sample extent, independent of source offset.
pub(super) fn gain(relative:u64,length:u64,fade_in:f64,fade_out:f64,rate:u32)->f32 {
 let inside=if fade_in>0.{(relative as f64/(fade_in*rate as f64)).clamp(0.,1.)}else{1.};
 let outside=if fade_out>0.{(length.saturating_sub(1).saturating_sub(relative) as f64/(fade_out*rate as f64)).clamp(0.,1.)}else{1.};
 (inside*outside)as f32
}

#[cfg(test)]mod tests {
 use super::*;use super::super::document::dispatch;
 fn edit(d:&mut State,hook:&str,mut v:Value)->Result<Value,String>{v["document_revision"]=json!(d.revision);dispatch(d,hook,&v)}
 fn fixture()->State{let mut d=State::default();edit(&mut d,"PutTrack",json!({"id":"a","name":"Audio","kind":"audio"})).unwrap();edit(&mut d,"SetTrackMix",json!({"id":"a","output_bus_l":"L","output_bus_r":"R"})).unwrap();edit(&mut d,"PutSource",json!({"id":"s","sample_rate":8000,"channels":1,"samples":vec![1.;32]})).unwrap();edit(&mut d,"PutClip",json!({"id":"c","track":"a","kind":"audio","source":"s","source_offset_seconds":0.001,"start_seconds":0.001,"duration_seconds":0.002})).unwrap();d}
 fn pcm(d:&mut State,start:u64,n:u64)->Vec<Value>{edit(d,"RenderRange",json!({"sample_rate":8000,"start_frame":start,"frames":n})).unwrap()["lsffr"].as_array().unwrap().clone()}
 #[test]fn fades_render_stereo_at_clip_edges_and_survive_blocks_moves_and_snapshot_restore(){let mut d=fixture();assert_eq!(d.clips["c"].fade_in,0.);edit(&mut d,"SetClipFades",json!({"id":"c","fade_in_seconds":0.0005,"fade_out_seconds":0.0005})).unwrap();let whole=pcm(&mut d,8,16);for(i,expected)in [(0,0.),(1,0.25),(4,1.),(11,1.),(14,0.25),(15,0.)]{assert_eq!(whole[i][0],expected);assert_eq!(whole[i][3],expected);}
 let mut split=pcm(&mut d,8,7);split.extend(pcm(&mut d,15,9));assert_eq!(split,whole);
 let mut snapshot=d.clips["c"].snapshot();snapshot["start_seconds"]=json!(0.002);edit(&mut d,"PutClip",snapshot).unwrap();assert_eq!(pcm(&mut d,16,16),whole);
 edit(&mut d,"MoveClip",json!({"id":"c","start_seconds":0.003})).unwrap();assert_eq!(pcm(&mut d,24,16),whole);}
 #[test]fn fades_undo_redo_and_rejections_are_atomic(){let mut d=fixture();let set=json!({"id":"c","fade_in_seconds":0.001,"fade_out_seconds":0.0015});edit(&mut d,"SetClipFades",set.clone()).unwrap();edit(&mut d,"SetClipFades",json!({"action":"undo"})).unwrap();assert_eq!(d.clips["c"].fade_in,0.);edit(&mut d,"SetClipFades",json!({"action":"redo"})).unwrap();assert_eq!(d.clips["c"].fade_out,0.0015);
 for invalid in [-1.,0.003]{let before=dispatch(&mut d,"Snapshot",&Value::Null).unwrap();let mut bad=set.clone();bad["fade_out_seconds"]=json!(invalid);assert!(edit(&mut d,"SetClipFades",bad).is_err());assert_eq!(dispatch(&mut d,"Snapshot",&Value::Null).unwrap(),before);}
 let before=dispatch(&mut d,"Snapshot",&Value::Null).unwrap();assert!(dispatch(&mut d,"SetClipFades",&json!({"document_revision":0,"action":"undo"})).is_err());assert_eq!(dispatch(&mut d,"Snapshot",&Value::Null).unwrap(),before);
 edit(&mut d,"MoveClip",json!({"id":"c","start_seconds":0.002})).unwrap();assert!(edit(&mut d,"SetClipFades",json!({"action":"undo"})).is_err());}
 #[test]fn overlapping_fades_multiply_without_boost(){assert_eq!(gain(4,9,0.001,0.001,8000),0.25);assert_eq!(gain(0,1,0.,0.,8000),1.);assert_eq!(gain(0,1,0.001,0.001,8000),0.);}
}

#[cfg(test)]mod audio_clip_editing {
 use super::*;use super::super::document::dispatch;
 fn call(d:&mut State,hook:&str,mut v:Value)->Result<Value,String>{v["document_revision"]=json!(d.revision);dispatch(d,hook,&v)}
 #[test]fn split_preserves_source_offsets_and_rendered_pcm(){let mut d=State::default();call(&mut d,"PutTrack",json!({"id":"a","name":"Audio","kind":"audio"})).unwrap();call(&mut d,"PutSource",json!({"id":"s","sample_rate":8000,"channels":1,"samples":(0..64).map(|i|i as f32/64.).collect::<Vec<_>>()})).unwrap();call(&mut d,"PutClip",json!({"id":"c","track":"a","kind":"audio","source":"s","start_seconds":0.001,"source_offset_seconds":0.001,"duration_seconds":0.004})).unwrap();let request=json!({"sample_rate":8000,"start_frame":0,"frames":64});let before=call(&mut d,"RenderRange",request.clone()).unwrap()["lsffr"].clone();let result=call(&mut d,"SplitClip",json!({"id":"c","time_seconds":0.003})).unwrap();let right=result["right_clip"].as_str().unwrap();assert_eq!(d.clips["c"].duration(),0.002);assert_eq!(d.clips[right].snapshot()["source_offset_seconds"],0.003);assert_eq!(before,call(&mut d,"RenderRange",request.clone()).unwrap()["lsffr"]);let snapshot=dispatch(&mut d,"Snapshot",&Value::Null).unwrap();for time in [0.001,0.003,f64::INFINITY]{assert!(call(&mut d,"SplitClip",json!({"id":"c","time_seconds":time})).is_err());assert_eq!(snapshot,dispatch(&mut d,"Snapshot",&Value::Null).unwrap());}}
}

#[cfg(test)]mod clip_gain_editing {
 use super::*;use super::super::document::dispatch;
 fn call(d:&mut State,h:&str,mut v:Value)->Result<Value,String>{v["document_revision"]=json!(d.revision);dispatch(d,h,&v)}
 #[test]fn gain_and_trim_are_nondestructive_revisioned_and_shared_by_split(){let mut d=State::default();call(&mut d,"PutTrack",json!({"id":"a","kind":"audio","name":"Audio"})).unwrap();call(&mut d,"PutSource",json!({"id":"s","sample_rate":8000,"channels":1,"samples":vec![0.5;64]})).unwrap();call(&mut d,"PutClip",json!({"id":"c","track":"a","kind":"audio","source":"s","start_seconds":0.,"source_offset_seconds":0.,"duration_seconds":0.008})).unwrap();let pcm=json!({"sample_rate":8000,"start_frame":0,"frames":64});let before=call(&mut d,"RenderRange",pcm.clone()).unwrap()["lsffr"].clone();call(&mut d,"SetClipGain",json!({"id":"c","gain":0.5})).unwrap();let after=call(&mut d,"RenderRange",pcm.clone()).unwrap()["lsffr"].clone();for(a,b)in before.as_array().unwrap().iter().zip(after.as_array().unwrap()){for i in 0..4{assert_eq!(a[i].as_f64().unwrap()*0.5,b[i].as_f64().unwrap());}}assert!(before.as_array().unwrap().iter().any(|f|f[2].as_f64().unwrap()!=0.));let source=d.sources["s"].samples.clone();call(&mut d,"TrimAudioClip",json!({"id":"c","start_fraction":0.25,"end_fraction":0.75})).unwrap();assert_eq!(d.clips["c"].start,0.002);assert_eq!(d.clips["c"].snapshot()["source_offset_seconds"],0.002);assert_eq!(d.clips["c"].duration(),0.004);let split=call(&mut d,"SplitClip",json!({"id":"c","time_seconds":0.004})).unwrap();assert_eq!(d.clips[split["right_clip"].as_str().unwrap()].gain,0.5);assert!(std::sync::Arc::ptr_eq(&source,&d.sources["s"].samples));let snapshot=dispatch(&mut d,"Snapshot",&Value::Null).unwrap();for gain in [-1.,17.]{assert!(call(&mut d,"SetClipGain",json!({"id":"c","gain":gain})).is_err());assert_eq!(snapshot,dispatch(&mut d,"Snapshot",&Value::Null).unwrap());}assert!(dispatch(&mut d,"SetClipGain",&json!({"document_revision":0,"id":"c","gain":1.})).is_err());}
}
