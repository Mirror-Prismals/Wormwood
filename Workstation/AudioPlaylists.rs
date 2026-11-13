//! Original audio comp lanes: the main arrangement stays audible. Alternate
//! lanes are silent edit/record targets, not switches for the playback renderer.
use super::document::{Clip,Content,State as Document};
use serde_json::{json,Value};
use std::collections::{BTreeMap,BTreeSet};
const MAX_LANES:usize=128;
const MAX_CLIPS:usize=512;
#[derive(Clone)]
struct Lane {name:String,clips:Vec<Clip>}
#[derive(Clone)]
struct Track {lanes:Vec<Lane>,selected:usize,record_target:Option<usize>,expanded:bool,record_controls:bool}
impl Default for Track {fn default()->Self{Self{lanes:vec![Lane{name:"Main".into(),clips:vec![]}],selected:0,record_target:None,expanded:false,record_controls:false}}}
#[derive(Clone,Default)]
pub(super) struct State {tracks:BTreeMap<String,Track>}
impl State {
    pub fn track_clip_ids(&self,id:&str)->Vec<String>{self.tracks.get(id).into_iter().flat_map(|t|t.lanes.iter().skip(1)).flat_map(|l|l.clips.iter().map(|c|c.id.clone())).collect()}
    pub fn remove_track(&mut self,id:&str){self.tracks.remove(id);}

    pub fn clip(&self,id:&str)->Option<&Clip>{self.tracks.values().flat_map(|t|t.lanes.iter().skip(1)).flat_map(|l|&l.clips).find(|c|c.id==id)}
    pub fn clip_mut(&mut self,id:&str)->Option<&mut Clip>{self.tracks.values_mut().flat_map(|t|t.lanes.iter_mut().skip(1)).flat_map(|l|&mut l.clips).find(|c|c.id==id)}
    pub fn append_clip(&mut self,track:&str,lane:usize,clip:Clip)->Result<(),String>{
        self.tracks.get_mut(track).and_then(|t|t.lanes.get_mut(lane)).ok_or("unknown comp lane")?.clips.push(clip);Ok(())
    }
    pub fn rebase(&mut self,shift:f64)->Result<(),String>{
        if self.tracks.values().flat_map(|t|t.lanes.iter().skip(1)).flat_map(|l|&l.clips).any(|c|c.start+c.duration()+shift>86400.){return Err("comp lane rebase exceeds one day".into());}
        for t in self.tracks.values_mut(){for l in t.lanes.iter_mut().skip(1){for c in &mut l.clips{c.start+=shift;}}}Ok(())
    }
    pub fn lane_count(&self,id:&str)->usize{self.tracks.get(id).map_or(0,|t|t.lanes.len().saturating_sub(1))}
    pub fn first_record_lane(&self,id:&str)->usize{usize::from(self.lane_count(id)>0)}
    pub fn record_target(&self,id:&str)->usize{self.tracks.get(id).and_then(|t|t.record_target.filter(|i|*i<t.lanes.len())).unwrap_or(0)}
    pub fn extra_ids(&self)->BTreeSet<String>{self.tracks.values().flat_map(|t|t.lanes.iter().skip(1)).flat_map(|l|l.clips.iter().map(|c|c.id.clone())).collect()}
    pub fn extra_count(&self)->usize{self.tracks.values().flat_map(|t|t.lanes.iter().skip(1)).map(|l|l.clips.len()).sum()}
    pub fn contains(&self,id:&str)->bool{self.tracks.values().flat_map(|t|t.lanes.iter().skip(1)).any(|l|l.clips.iter().any(|c|c.id==id))}
    pub fn snapshot(&self,document:&Document)->Value {
        json!(document.track_order.iter().filter(|id|document.tracks[*id].kind=="audio").map(|id|{
            let default=Track::default();let t=self.tracks.get(id).unwrap_or(&default);
            json!({"track":id,"active":0,"selected":t.selected,"record_target":self.record_target(id),"expanded":t.expanded,"record_controls_visible":t.record_controls,
                "lanes":t.lanes.iter().enumerate().map(|(index,l)|json!({"index":index,"name":l.name,"active":index==0,"clips":if index==0 {document.clip_order.iter().filter_map(|key|{let c=&document.clips[key];(c.track==*id).then(||c.snapshot())}).collect::<Vec<_>>()}else{l.clips.iter().map(|c|{let mut v=c.snapshot();if let Content::Audio{source,offset,duration}=&c.content {if let Some(audio)=document.sources.get(source){v["rainbow_waveform"]=super::rainbow_waveform::analyze(audio,*offset,*duration);}}v}).collect::<Vec<_>>()}})).collect::<Vec<_>>()})
        }).collect::<Vec<_>>())
    }
    pub fn insert_capture(&mut self,track:&str,playlist:usize,incoming:Clip,reserved:&BTreeSet<String>)->Result<(),String>{
        if playlist==0{return Err("main playlist uses the active document insertion path".into());}
        let lane=self.tracks.get_mut(track).and_then(|t|t.lanes.get_mut(playlist)).ok_or("recording playlist no longer exists")?;
        let a=incoming.start;let b=a+incoming.duration();let mut result=Vec::new();
        let mut used=reserved.clone();used.extend(lane.clips.iter().map(|c|c.id.clone()));used.insert(incoming.id.clone());
        for old in &lane.clips {
            let end=old.start+old.duration();
            if end<=a||old.start>=b {result.push(old.clone());continue;}
            let Content::Audio{source,offset,..}=&old.content else{return Err("audio playlist contains a nonaudio clip".into());};
            if old.start<a {let mut left=old.clone();left.content=Content::Audio{source:source.clone(),offset:*offset,duration:a-old.start};result.push(left);}
            if end>b {let mut right=old.clone();if old.start<a {right.id=identity("playlist-split",&used);used.insert(right.id.clone());}right.start=b;right.content=Content::Audio{source:source.clone(),offset:*offset+b-old.start,duration:end-b};result.push(right);}
        }
        result.push(incoming);result.sort_by(|a,b|a.start.total_cmp(&b.start));lane.clips=result;Ok(())
    }
}
fn identity(prefix:&str,used:&BTreeSet<String>)->String{(1..=used.len()+1).map(|n|format!("{prefix}-{n}")).find(|s|!used.contains(s)).unwrap()}
pub(super) fn command(document:&mut Document,input:&Value)->Result<Value,String>{
    let id=input["id"].as_str().ok_or("audio playlist track required")?;
    if !document.tracks.get(id).is_some_and(|t|t.kind=="audio"){return Err("playlists require an audio track".into());}
    let action=input["action"].as_str().ok_or("playlist action required")?;
    if matches!(action,"move_clip"|"remove_clip"|"split_clip"|"fade_clip"){return edit_clip(document,id,action,input);}
    if document.recording.busy(id)&&!matches!(action,"toggle_lanes"|"toggle_record_controls"){return Err("audio playlist is busy recording or awaiting commit".into());}
    let mut next=document.audio_playlists.clone();
    let total_extra=next.extra_count();let mut used=next.extra_ids();used.extend(document.clips.keys().cloned());
    let t=next.tracks.entry(id.into()).or_default();
    // The main row is authoritative: refresh its mirror before clone/menu edits.
    t.lanes[0].clips=document.clip_order.iter().filter_map(|key|{let c=&document.clips[key];(c.track==id).then(||c.clone())}).collect();
    let index=input.get("index").map(|v|v.as_u64().map(|n|n as usize).ok_or("playlist index must be nonnegative")).transpose()?.unwrap_or(t.selected);
    if index>=t.lanes.len(){return Err("unknown audio playlist".into());}
    match action {
        "new"|"duplicate"=>{
            if t.lanes.len()>=MAX_LANES{return Err("audio playlist capacity reached".into());}
            let clips=if action=="duplicate"{t.lanes[index].clips.iter().map(|c|{let mut c=c.clone();c.id=identity("playlist-copy",&used);used.insert(c.id.clone());c}).collect()}else{Vec::new()};
            if document.clips.len()+total_extra+clips.len()>MAX_CLIPS{return Err("audio playlist duplication exceeds document clip capacity".into());}
            t.lanes.push(Lane{name:format!("Take {}",t.lanes.len()),clips});t.record_target=Some(t.lanes.len()-1);t.expanded=true;
        },
        "select"=>t.selected=index,
        "record_target"=>t.record_target=Some(index),
        "rename"=>{let name=input["name"].as_str().filter(|n|!n.is_empty()&&n.chars().count()<=40&&!n.chars().any(char::is_control)).ok_or("playlist name requires 1..40 printable characters")?;t.lanes[index].name=name.into();},
        "delete"=>{if index==0{return Err("the active main playlist cannot be deleted".into());}t.lanes.remove(index);t.selected=0;t.record_target=t.record_target.and_then(|target|if target==index{None}else{Some(target-usize::from(target>index))});},
        "toggle_lanes"=>t.expanded=!t.expanded,
        "toggle_record_controls"=>{t.record_controls=!t.record_controls;if t.record_controls{t.expanded=true;}},
        _=>return Err("unknown audio playlist action".into()),
    }
    document.audio_playlists=next;document.recording_changed();Ok(json!({"revision":document.revision,"audio_playlists":document.audio_playlists.snapshot(document)}))
}
#[cfg(test)]mod tests{
 use super::*;use super::super::document;
 fn edit(d:&mut Document,hook:&str,mut v:Value)->Value{v["document_revision"]=json!(d.revision);document::dispatch(d,hook,&v).unwrap()}
 fn setup()->Document{let mut d=Document::default();edit(&mut d,"PutTrack",json!({"id":"a","name":"Audio","kind":"audio"}));d}
 #[test]fn new_lane_is_silent_and_sets_only_record_destination(){let mut d=setup();edit(&mut d,"AudioPlaylist",json!({"id":"a","action":"new"}));let s=d.audio_playlists.snapshot(&d);assert_eq!(s[0]["active"],0);assert_eq!(s[0]["selected"],0);assert_eq!(s[0]["record_target"],1);assert_eq!(s[0]["lanes"][1]["name"],"Take 1");edit(&mut d,"AudioPlaylist",json!({"id":"a","action":"select","index":1}));assert_eq!(d.audio_playlists.snapshot(&d)[0]["active"],0);assert!(d.clips.is_empty());}
 #[test]fn deletion_reindexes_record_target_and_keeps_main(){let mut d=setup();for _ in 0..2{edit(&mut d,"AudioPlaylist",json!({"id":"a","action":"new"}));}edit(&mut d,"AudioPlaylist",json!({"id":"a","action":"delete","index":1}));assert_eq!(d.audio_playlists.record_target("a"),1);edit(&mut d,"AudioPlaylist",json!({"id":"a","action":"delete","index":1}));assert_eq!(d.audio_playlists.record_target("a"),0);assert!(command(&mut d,&json!({"id":"a","action":"delete","index":0})).is_err());}
 #[test]fn alternate_lane_capture_trims_exact_offsets_without_touching_main(){let mut d=setup();edit(&mut d,"AudioPlaylist",json!({"id":"a","action":"new"}));let clip=|id:&str,start,duration|Clip { output:String::new(),lane:0., gain:1., fade_in:0., fade_out:0.,id:id.into(),track:"a".into(),start,content:Content::Audio{source:"s".into(),offset:5.,duration}};d.audio_playlists.insert_capture("a",1,clip("old",0.,10.),&BTreeSet::new()).unwrap();d.audio_playlists.insert_capture("a",1,clip("new",3.,2.),&BTreeSet::new()).unwrap();let s=d.audio_playlists.snapshot(&d);let c=s[0]["lanes"][1]["clips"].as_array().unwrap();assert_eq!(c.len(),3);assert_eq!(c[2]["source_offset_seconds"],10.);assert!(d.clips.is_empty());}
}

// Manual take editing. Alternate lanes remain silent until a clip is moved to
// the parent arrangement. All validation precedes publication of either owner.
fn edit_clip(d:&mut Document,track:&str,action:&str,v:&Value)->Result<Value,String>{
 if d.recording.busy(track){return Err("take lanes are busy recording or awaiting commit".into());}
 let id=v["clip_id"].as_str().ok_or("clip_id required")?;
 let mut lanes=d.audio_playlists.clone();let mut clips=d.clips.clone();let mut order=d.clip_order.clone();
 let source_lane=lanes.tracks.get(track).and_then(|t|t.lanes.iter().enumerate().skip(1).find(|(_,l)|l.clips.iter().any(|c|c.id==id)).map(|(i,_)|i)).unwrap_or(0);
 let mut c=if source_lane==0{clips.get(id).filter(|c|c.track==track).cloned().ok_or("unknown audio clip")?}else{lanes.tracks[track].lanes[source_lane].clips.iter().find(|c|c.id==id).unwrap().clone()};
 if !matches!(c.content,Content::Audio{..}){return Err("take editing requires audio".into());}
 if d.warp.entries.contains_key(id)||d.clip_treatments.contains_key(id){return Err("Render or clear clip treatments before take editing".into());}
 let mut target=source_lane;let mut right=None;
 match action {
  "move_clip"=>{let start=v["start_seconds"].as_f64().filter(|n|n.is_finite()&&*n>=0.&&*n+c.duration()<=86400.).ok_or("invalid clip start")?;c.start=start;
   if let Some(index)=v.get("lane"){target=index.as_u64().and_then(|n|usize::try_from(n).ok()).ok_or("invalid take lane")?;}
   if target>lanes.lane_count(track){return Err("unknown take lane".into());}},
  "split_clip"=>{let at=v["at_seconds"].as_f64().filter(|n|n.is_finite()&&*n>c.start&&*n<c.start+c.duration()).ok_or("slice must be inside clip")?;
   let used=clips.keys().cloned().chain(lanes.extra_ids()).collect();let mut second=c.clone();second.id=identity("slice",&used);second.start=at;
   let Content::Audio{offset,duration,..}=&mut second.content else{unreachable!()};*offset+=at-c.start;*duration-=at-c.start;
   let Content::Audio{duration,..}=&mut c.content else{unreachable!()};*duration=at-c.start;
   c.fade_in=c.fade_in.min(c.duration());c.fade_out=0.;second.fade_in=0.;second.fade_out=second.fade_out.min(second.duration());right=Some(second);},
  "fade_clip"=>{c.fade_in=v["fade_in_seconds"].as_f64().ok_or("fade in required")?;c.fade_out=v["fade_out_seconds"].as_f64().ok_or("fade out required")?;super::clip_fades::validate(&c)?;},
  "remove_clip"=>{},_=>unreachable!()
 }
 if source_lane==0{clips.remove(id);order.retain(|key|key!=id);}else{lanes.tracks.get_mut(track).unwrap().lanes[source_lane].clips.retain(|c|c.id!=id);}
 if action!="remove_clip"{for c in std::iter::once(c).chain(right){if target==0{order.push(c.id.clone());clips.insert(c.id.clone(),c);}else{lanes.tracks.get_mut(track).unwrap().lanes[target].clips.push(c);}}}
 if clips.len()+lanes.extra_count()>MAX_CLIPS{return Err("document clip capacity reached".into());}
 d.clips=clips;d.clip_order=order;d.audio_playlists=lanes;Ok(d.changed())
}

#[cfg(test)]mod comp_lane_tests{
 use super::*;use super::super::document::dispatch;
 fn edit(d:&mut Document,hook:&str,mut v:Value)->Result<Value,String>{v["document_revision"]=json!(d.revision);dispatch(d,hook,&v)}
 #[test]fn comp_lane_manual_slice_transfer_and_invalid_edit_are_atomic(){
  let mut d=Document::default();edit(&mut d,"PutTrack",json!({"id":"a","name":"Audio","kind":"audio"})).unwrap();
  edit(&mut d,"PutSource",json!({"id":"s","sample_rate":8000,"channels":1,"samples":vec![0.5;32]})).unwrap();
  edit(&mut d,"PutClip",json!({"id":"c","track":"a","kind":"audio","source":"s","source_offset_seconds":0.,"start_seconds":0.,"duration_seconds":0.004})).unwrap();
  edit(&mut d,"AudioPlaylist",json!({"id":"a","action":"new"})).unwrap();
  edit(&mut d,"AudioPlaylist",json!({"id":"a","action":"move_clip","clip_id":"c","start_seconds":0.,"lane":1})).unwrap();assert!(d.clips.is_empty());
  let before=dispatch(&mut d,"Snapshot",&Value::Null).unwrap();assert!(edit(&mut d,"AudioPlaylist",json!({"id":"a","action":"split_clip","clip_id":"c","at_seconds":1.})).is_err());assert_eq!(before,dispatch(&mut d,"Snapshot",&Value::Null).unwrap());
  edit(&mut d,"AudioPlaylist",json!({"id":"a","action":"split_clip","clip_id":"c","at_seconds":0.002})).unwrap();assert_eq!(d.audio_playlists.extra_count(),2);
  let right=d.audio_playlists.snapshot(&d)[0]["lanes"][1]["clips"][1].clone();assert_eq!(right["source_offset_seconds"],0.002);
  edit(&mut d,"AudioPlaylist",json!({"id":"a","action":"move_clip","clip_id":right["id"],"start_seconds":0.002,"lane":0})).unwrap();assert_eq!(d.clips.len(),1);assert_eq!(d.audio_playlists.extra_count(),1);
  assert_eq!(dispatch(&mut d,"Snapshot",&Value::Null).unwrap()["audio_playlists"][0]["lanes"][1]["clips"][0]["rainbow_waveform"].as_array().unwrap().len(),128);
 }
}
