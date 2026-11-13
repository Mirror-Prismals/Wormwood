//! Song-clock lighting evaluation. Pure seek-safe, independent of audio rendering.
use super::{document::{State,Content},track_io::CcLane};
use crate::theatre::lighting::{self,Levels};
pub(crate) fn evaluate(state:&State,seconds:f64)->Levels {
 let mut out=lighting::defaults();if !seconds.is_finite(){return out;}
 for id in &state.track_order {
  let track=&state.tracks[id];if track.kind!="cc"||track.cc_lane!=CcLane::Workspace{continue;}
  let fixture=track.cc_device_slot;if fixture>=lighting::COUNT||track.cc_parameter_slot>3{continue;}
  for clip_id in &state.clip_order {
   let clip=&state.clips[clip_id];if clip.track!=*id{continue;}
   if let Content::Cc{duration,points}=&clip.content {
    if seconds<clip.start||seconds>=clip.start+duration{continue;}
    let value=super::automation::evaluate(points,seconds-clip.start);
    let channel=if track.cc_parameter_slot==0{3}else{track.cc_parameter_slot-1};
    out[fixture][channel]=value;break;
   }
  }
 }
 out
}
#[cfg(test)]mod tests {
 use super::*;use super::super::document::dispatch;use serde_json::{Value,json};
 fn edit(s:&mut State,hook:&str,mut v:Value){v["document_revision"]=dispatch(s,"Snapshot",&Value::Null).unwrap()["revision"].clone();dispatch(s,hook,&v).unwrap();}
 #[test]fn workspace_selectors_drive_interpolated_fixture_color_and_intensity_on_seek(){
  let mut s=State::default();
  for (id,param) in [("dim",0),("red",1)] {
   edit(&mut s,"PutTrack",json!({"id":id,"kind":"cc","name":id}));
   edit(&mut s,"SetTrackMix",json!({"id":id,"cc_action":"target_lane"}));
   edit(&mut s,"SetTrackMix",json!({"id":id,"cc_action":"target_param_pick","cc_parameter_slot":param}));
   edit(&mut s,"PutClip",json!({"id":format!("{id}-clip"),"track":id,"kind":"cc","start_seconds":2.,"duration_seconds":4.,"points":[{"offset_seconds":0.,"value":0.},{"offset_seconds":4.,"value":1.}]}));
  }
  let at=evaluate(&s,4.);assert_eq!(at[0][3],0.5);assert_eq!(at[0][0],0.5);assert_eq!(at[1],lighting::defaults()[1]);
  assert_eq!(evaluate(&s,1.),lighting::defaults());assert_eq!(evaluate(&s,8.),lighting::defaults());assert_eq!(evaluate(&s,2.)[0][3],0.);
  assert_eq!(evaluate(&s,4.),at);
  let snapshot=dispatch(&mut s,"Snapshot",&Value::Null).unwrap();assert_eq!(snapshot["tracks"][0]["cc_workspace_available"],true);
 }
}

#[cfg(test)]mod selector_tests {
 use super::*;use super::super::document::dispatch;use serde_json::{Value,json};
 fn edit(s:&mut State,mut v:Value){v["document_revision"]=dispatch(s,"Snapshot",&Value::Null).unwrap()["revision"].clone();dispatch(s,"SetTrackMix",&v).unwrap();}
 #[test]fn all_six_workspace_devices_cycle_and_query_is_read_only(){
  let mut s=State::default();dispatch(&mut s,"PutTrack",&json!({"document_revision":0,"id":"cc","kind":"cc","name":"Light"})).unwrap();
  edit(&mut s,json!({"id":"cc","cc_action":"target_lane"}));
  for index in 0..6 {
   let v=dispatch(&mut s,"Snapshot",&Value::Null).unwrap();assert_eq!(v["tracks"][0]["cc_target_device"],index+1);assert_eq!(v["tracks"][0]["cc_parameters"].as_array().unwrap().len(),4);
   edit(&mut s,json!({"id":"cc","cc_action":"target_device"}));
  }
  let before=dispatch(&mut s,"Snapshot",&Value::Null).unwrap();assert_eq!(before["tracks"][0]["cc_target_device"],1);
  let lights=dispatch(&mut s,"StageLighting",&json!({"seconds":-1.})).unwrap();assert_eq!(lights["levels"],json!(lighting::defaults()));
  assert!(dispatch(&mut s,"StageLighting",&json!({"seconds":null})).is_err());
  assert_eq!(before,dispatch(&mut s,"Snapshot",&Value::Null).unwrap());
 }
}
