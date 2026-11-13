//! Lane copies of the existing baked mixer and crosswalk hardware. Local view
//! flags never enter the audio document; edits use stable track IDs/revisions.
use serde_json::{Value,json};
use super::{arrangement, bottom_panel, fader_components, track_io_scene,panel_layout::Viewport};
pub(crate) const BASE:usize=super::bubble_notes::IDENTITY_END;
pub(crate) const IO:usize=BASE+fader_components::ROWS;
pub(crate) const END:usize=IO+track_io_scene::END-track_io_scene::HEADER;
pub(crate) fn flags(v:&Value)->[bool;3]{std::array::from_fn(|i|v["lane_controls"][i]==true)}
pub(crate) fn reserve(f:[bool;3])->f64{(if f[0]{14.}else{0.})+(if f[1]{32.}else{0.})+(if f[2]{40.}else{0.})}
pub(crate) fn columns(mut x:f64,y:f64,h:f64,f:[bool;3])->([f64;4],[f64;4]) {
 if f[0]{x+=14.;}let fader=[x+4.,y,24.,h];if f[1]{x+=32.;}
 let size=36f64.min(h);(fader,[x+2.,y+(h-size)*0.5,size,size])
}
pub(crate) fn knob(r:[f64;4],gain:f64)->[f64;4]{let size=18f64.min(r[3]*0.4);[r[0]+(r[2]-size)*0.5,r[1]+(1.-bottom_panel::gain_to_fader(gain))*(r[3]-size),size,size]}
pub(crate) fn drag_gain(center:f64,y:f64,h:f64)->f64{let size=18f64.min(h*0.4);bottom_panel::fader_to_gain((1.-(center-y-size*0.5)/(h-size).max(1.)).clamp(0.,1.))}
pub(crate) fn packets(t:&Value,bottom:&Value)->(Value,Value){
 let f=flags(t);let c=&t["canvas"];let left=c["left"].as_f64().unwrap_or(0.);let top=c["top"].as_f64().unwrap_or(0.);
 let width=c["width"].as_f64().unwrap_or(0.);let height=c["height"].as_f64().unwrap_or(0.);
 let panel=[left,top+arrangement::RULER_HEIGHT,width,(height-arrangement::RULER_HEIGHT).max(0.)];
 let mut faders=vec![];let mut controls=vec![];
 if let Some(tracks)=t["tracks"].as_array().filter(|_|width>arrangement::BODY_OFFSET+arrangement::MONITOR_GUTTER_WIDTH+reserve(f)) {
  let scroll=t["scroll_tracks"].as_u64().unwrap_or(0) as usize;
  let visible=arrangement::lane_count(tracks,scroll,panel[3]);
  let x=left+width-reserve(f);
  for (row,track) in tracks.iter().enumerate().skip(scroll).take(visible){
   if track["trackless"]==true||track["comp_parent"].is_string()||!matches!(track["kind"].as_str(),Some("audio"|"midi")){continue;}
   let y=panel[1]+arrangement::lane_top(tracks,scroll,row)+4.;let h=arrangement::lane_height(track)-8.;if h<12.{continue;}
   let (housing,arm)=columns(x,y,h,f);let mut cap=knob(housing,track["gain"].as_f64().unwrap_or(1.));let mut housing=housing;
   if !f[1]{housing[2]=0.;cap[2]=0.;}
   let meter=if f[0]{[[x+2.,y,4.,h],[x+6.,y,4.,h]]}else{[[x,y,0.,0.];2]};
   if f[0]||f[1]{let levels=bottom["track_levels"].as_array().and_then(|a|a.iter().find(|v|v["track"]==track["id"])).map(|v|v["levels"].clone()).unwrap_or(json!([0.,0.]));
    faders.push(json!({"housing":housing,"knob":cap,"meters":meter,"levels":levels,"normalized":bottom_panel::gain_to_fader(track["gain"].as_f64().unwrap_or(1.))}));}
   if f[2]{controls.push(json!({"key":"arm","text":"","rect":arm,"crosswalk":true,"crosswalk_palette":super::track_io::crosswalk_palette(track["arm_mode"].as_u64().unwrap_or(0) as u8,t["lane_recording"]==true && t["lane_record_tracks"].as_array().is_some_and(|a|a.iter().any(|id|*id==track["id"])),if t["lane_time"]==true{0.}else{0.5})}));}
  }
 }
 (json!({"page":0,"panel":panel,"content":panel,"faders":faders,"master_meters":[]}),json!({"page":0,"panel":panel,"controls":controls}))
}
pub(crate) fn append(rows:&mut Vec<[f32;4]>,view:Viewport,t:&Value,bottom:&Value,faders:&fader_components::Kit,io:&track_io_scene::Kit)->Result<(),String>{
 let (f,i)=packets(t,bottom);let old_f=rows[fader_components::BASE..fader_components::BASE+fader_components::ROWS].to_vec();let old_i=rows[track_io_scene::HEADER..track_io_scene::END].to_vec();
 rows.resize(END,[0.;4]);
 let result=(||{faders.append(rows,[view.width,view.height],0.004,&f)?;io.append(rows,view,&i)?;Ok(())})();
 let copy=rows[fader_components::BASE..fader_components::BASE+fader_components::ROWS].to_vec();rows[BASE..IO].copy_from_slice(&copy);
 let copy=rows[track_io_scene::HEADER..track_io_scene::END].to_vec();rows[IO..END].copy_from_slice(&copy);
 rows[fader_components::BASE..fader_components::BASE+fader_components::ROWS].copy_from_slice(&old_f);rows[track_io_scene::HEADER..track_io_scene::END].copy_from_slice(&old_i);result
}

pub(crate) fn clip_body(mut canvas:super::piano_roll::Rect,f:[bool;3])->super::piano_roll::Rect{canvas.width-=reserve(f);arrangement::clip_body(canvas)}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn independent_flags_keep_columns_outside_elapsed_time_and_share_track_levels(){
  for bits in 0..8{let flags=std::array::from_fn(|i|bits&(1<<i)!=0);
   let t=json!({"canvas":{"left":100.,"top":50.,"width":900.,"height":400.},"lane_controls":flags,"scroll_tracks":0,"tracks":[{"id":"m","kind":"midi","gain":0.5,"arm_mode":3},{"id":"c","kind":"cc"},{"id":"take","kind":"audio","comp_parent":"a"},{"id":"a","kind":"audio","gain":1.,"arm_mode":1}]});
   let b=json!({"track_levels":[{"track":"a","levels":[0.2,0.7]},{"track":"m","levels":[0.8,0.3]}]});let(f,i)=packets(&t,&b);
   assert_eq!(f["faders"].as_array().unwrap().len(),if flags[0]||flags[1]{2}else{0});assert_eq!(i["controls"].as_array().unwrap().len(),if flags[2]{2}else{0});
   if flags[0]||flags[1]{assert_eq!(f["faders"][0]["levels"],json!([0.8,0.3]));assert_eq!(f["faders"][1]["levels"],json!([0.2,0.7]));}
   if flags[2]{assert_eq!(i["controls"][0]["crosswalk_palette"],1);assert_eq!(i["controls"][1]["crosswalk_palette"],2);}
   assert_eq!(clip_body(super::super::piano_roll::Rect{left:100.,top:50.,width:900.,height:400.},flags).width,900.-arrangement::BODY_OFFSET-arrangement::MONITOR_GUTTER_WIDTH-reserve(flags));
  }
 }
 #[test]fn lane_fader_law_and_picking_geometry_agree(){for h in [24.,56.,88.,248.]{for normalized in [0.,0.2,0.5,0.8,1.]{let gain=bottom_panel::fader_to_gain(normalized);let r=[100.,80.,24.,h];let k=knob(r,gain);assert!((drag_gain(k[1]+k[3]*0.5,r[1],h)-gain).abs()<1e-9);}}}
}
#[cfg(test)]mod packet_tests{
 use super::*;
 #[test]fn authored_packets_preserve_bottom_and_right_hardware_and_stay_bounded(){
  let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));let f=fader_components::Kit::load(root).unwrap();let i=track_io_scene::Kit::load(root).unwrap();
  let t=json!({"canvas":{"left":100.,"top":50.,"width":900.,"height":400.},"lane_controls":[true,true,true],"scroll_tracks":0,"tracks":[{"id":"a","kind":"audio","gain":1.,"arm_mode":3}]});
  let mut rows=vec![[42.;4];BASE];let old_f=rows[fader_components::BASE..fader_components::BASE+fader_components::ROWS].to_vec();let old_i=rows[track_io_scene::HEADER..track_io_scene::END].to_vec();
  append(&mut rows,Viewport{width:1600.,height:1000.,canvas_scale:1.},&t,&json!({"track_levels":[{"track":"a","levels":[0.2,0.7]}]}),&f,&i).unwrap();
  assert_eq!(rows.len(),END);assert_eq!(rows[BASE][0],1.);assert_eq!(rows[IO][1],1.);assert_eq!(rows[BASE+11][2],0.2);assert_eq!(rows[BASE+11][3],0.7);
  assert_eq!(&rows[fader_components::BASE..fader_components::BASE+fader_components::ROWS],old_f.as_slice());assert_eq!(&rows[track_io_scene::HEADER..track_io_scene::END],old_i.as_slice());assert!(rows[BASE..].iter().flatten().all(|v|v.is_finite()));
  // Every root/material pair must resolve to shipped persistent artwork.
  let art:Value=serde_json::from_slice(&std::fs::read(root.join("Art/AudioWorkstation/Baked/manifest.json")).unwrap()).unwrap();let entries=art["entries"].as_array().unwrap();
  for (root,material) in [(rows[BASE+1][0],12544.),(rows[BASE+1][2],12544.),(rows[BASE+2][0],12544.),(rows[IO+5][0],rows[IO+5][3])]{assert!(entries.iter().any(|e|e["root"].as_f64()==Some(root as f64)&&e["material"].as_f64()==Some(material as f64)),"missing shipped art {root}/{material}");}
 }
}
