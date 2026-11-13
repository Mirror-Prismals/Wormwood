//! Legacy REFERENCE_OPTICS content keys only; production baked artwork never
//! uses these optical-refresh requirements. See WorkstationBaked and AGENTS.md.
//! Content identities must not lag the control state. Pose-only animation may
//! retain appearance, but changed glyphs, visible controls or control assets
//! require a complete synchronous image before presentation.
fn integer(v:f32,limit:usize)->Option<usize>{(v.is_finite()&&v>=0.&&v.fract()==0.&&(v as usize)<=limit).then_some(v as usize)}
pub(super) fn timeline_changed(a:&[[f32;4]],b:&[[f32;4]])->bool {
 use crate::workstation::timeline_scene::{HEADER,DYNAMIC_BASE,END};
 // With zero timeline instances the clip, mesh and waveform rows are never
 // consumed by a timeline surface. Rulers/playhead have separate compositor
 // paths. Layout changes to an empty editor cannot dirty its absent artwork.
 if a.get(HEADER).is_some_and(|r|r[0]==0.&&r[3]==0.)
    &&b.get(HEADER).is_some_and(|r|r[0]==0.&&r[3]==0.) {return false;}

 // Track handles, tapes, ticks and waveform windows are baked into the
 // stationary image. They must follow pan/zoom in the current frame rather
 // than waiting for an incremental capture to finish after the drag stops.
 // The playhead (HEADER+4) has its own live compositor path.
 for range in [HEADER..HEADER+4,HEADER+5..HEADER+7,DYNAMIC_BASE..END] {
  match (a.get(range.clone()),b.get(range)) {
   (Some(a),Some(b)) if a==b=>{},
   _=>return true,
  }
 }
 false
}
pub(super) fn rulers_changed(a:&[[f32;4]], b:&[[f32;4]]) -> bool {
 let base=crate::workstation::timeline_bar_scene::BASE;
 // The depth-first variant bakes all three faces, including the LOOP control.
 (0..6).any(|offset| a.get(base+offset)!=b.get(base+offset))
}
fn key(rows:&[[f32;4]],panel_motion:bool,offline_faders:bool,offscreen:[bool;4],engraving:bool)->Option<Vec<u32>>{
 if rows.len()<256{return None;}let mut out=Vec::with_capacity(800);
 let count=integer(rows[212][2],1536)?;let gpu_base=rows[212][1];
 let dynamic=gpu_base==(crate::workstation::piano_scene::GPU_END) as f32;
 let base=if dynamic {crate::workstation::piano_scene::END}else{integer(gpu_base,rows.len())?};
 if base.checked_add(count*2)?>rows.len(){return None;}
 if engraving {
  for panel in 2..=3 {if !offscreen[panel]{out.extend(rows[217][(panel-2)*2..(panel-1)*2].iter().map(|v|v.to_bits()));}}
 }
 for i in 0..if engraving{count}else{0}{
  let panel=integer(rows[base+i*2+1][1],3)?;
  if offscreen[panel]{continue;}
  out.extend(rows[base+i*2+1].map(f32::to_bits));
  // Catalog scrolling can change poses without changing the visible words.
  if dynamic {out.extend(rows[base+i*2].map(f32::to_bits));}
 }
 let ordinary=integer(rows[12][0],65)?;let edison=integer(rows[232][0],rows.len())?;let navigation=integer(rows[232][2],4)?;
 let meters=integer(rows[247][1],32)?;let meter_base=integer(rows[247][0],rows.len())?;
 out.extend([ordinary as u32,if offscreen[1]{0}else{navigation as u32},meters as u32]);
 for instance in 0..65{
  let owner=if instance<4 {Some(0)} else if instance<10 || (13..29).contains(&instance) || instance>=61 {Some(1)} else {None};
  if owner.is_some_and(|i|offscreen[i]) {continue;}
  let active=if instance>=61{instance-61<navigation}else if instance>=29{instance-29<meters}else{instance<ordinary};
  out.push(u32::from(active));if !active{continue;}
  let at=if instance>=61{edison+32+2*(instance-61)}else if instance>=29{meter_base+2*(instance-29)}else if instance>=13{edison+2*(instance-13)}else if instance>=10{226+2*(instance-10)}else{192+2*instance};
  let pose=rows.get(at)?;out.push(u32::from(pose[3]>0.));let mut tree=*rows.get(at+1)?;
  if panel_motion && instance<4 {tree[3]=0.;} // neutral transport Z
  out.extend(tree.map(f32::to_bits));
 }
 // I/O glyphs, arming palettes and visible identities must be synchronous.
 // Their positions can still follow the right panel's retained transform.
 let io=crate::workstation::track_io_scene::HEADER;
 if rows.len()>io && !offscreen[3] {
  let count=integer(rows.get(io)?[1],crate::workstation::track_io::MAX_CONTROLS)?;
  out.push(count as u32);
  for i in 0..count {let at=crate::workstation::track_io_scene::POSE_BASE+i*crate::workstation::track_io_scene::STRIDE;
   out.push(u32::from(rows.get(at)?[3]>0.));
   for (index,row) in rows.get(at+1..at+crate::workstation::track_io_scene::STRIDE)?.iter().enumerate() {
    let mut fields=*row;
    if panel_motion && index==0 {fields[2]=0.;} // font scale
    out.extend(fields.map(f32::to_bits));
   }
  }
 }else{out.push(0);}
 // The compositor draws the ruler assembly and LOOP from the current packet.
 // Bottom semantics/data must update in the same presented frame. This
 // deliberately includes pressure until bottom controls have isolated layers.
 if !offscreen[1] {
 out.extend(rows[255][..3].iter().map(|v|v.to_bits()));
 if rows[255][1]==1.{let base=integer(rows[255][0],rows.len())?;if base.checked_add(316)?>rows.len(){return None;}for at in base..base+316{for lane in 0..4{
   let offset=at-base;
   if panel_motion && (offset<=1 || (offset==2&&lane==3) || (8..=9).contains(&offset)
    || ((12..48).contains(&offset) && ((offset-12)%3==0 || ((offset-12)%3==2 && lane==3)))) {continue;}
if !crate::workstation::live_panel_data::live_field(rows,at,lane){out.push(rows[at][lane].to_bits());}}}
  if rows[base+5][0]>0.5{let preview=integer(rows[base+5][0],rows.len())?;if preview.checked_add(2160)?>rows.len(){return None;}for at in preview..preview+2160{for lane in 0..4{if !crate::workstation::live_panel_data::live_field(rows,at,lane){out.push(rows[at][lane].to_bits());}}}}}
 }
 // Original fader components, levels and labels are an independent family.
 if rows.len()>=43664 && !offscreen[1]{for at in 43152..43664{for lane in 0..4{
  let offset=at-43152;
  if offline_faders && (offset==6 || (offset>=8 && ((offset-8)%6==1 && lane==1 || (offset-8)%6==5 && lane==1))) {continue;}
  if panel_motion && ((3..=4).contains(&offset) || (offset>=8 && ((offset-8)%6<3 || (offset-8)%6==4 || ((offset-8)%6==3 && lane==0)))) {continue;}
if !crate::workstation::live_panel_data::live_field(rows,at,lane){out.push(rows[at][lane].to_bits());}}}}
 // The original KEYBOARD control occupies right-panel page one independently
 // of page-zero I/O. Header state bits invalidate content without panel motion.
 let keyboard=44592usize;
 if rows.len()>keyboard && !offscreen[3] {
  let header=rows.get(keyboard)?;let count=integer(header[1],3)?;
  out.extend(header.map(f32::to_bits));
  let base=integer(header[0],rows.len())?;
  for i in 0..count {
   let pose=base+i*5;
   out.push(u32::from(rows.get(pose)?[3]>0.));
   for (index,row) in rows.get(pose+1..pose+5)?.iter().enumerate() {
    let mut fields=*row;
    if panel_motion && index==0 {fields[2]=0.;}
    out.extend(fields.map(f32::to_bits));
   }
  }
 }else{out.extend([0;4]);}
 Some(out)
}
pub(super) fn changed(a:&[[f32;4]],b:&[[f32;4]])->bool{changed_for_motion(a,b,false)}
pub(super) fn changed_for_motion(a:&[[f32;4]],b:&[[f32;4]],motion:bool)->bool{compare(a,b,motion,false)}
pub(super) fn compare(a:&[[f32;4]],b:&[[f32;4]],motion:bool,offline_faders:bool)->bool{
 // Only skip an entire panel group already parked outside the current view.
 // Its retained image is transformed offscreen too. A visible content change
 // still requires synchronous publication, including during ordinary motion.
 let offscreen=std::array::from_fn(|i| {
  if b.len()<192 || b[2][3]<=0. || b[3][3]<=0. {return false;}
  let p=b[34+40*i];let half=b[35+40*i];
  p[0]+half[0]< -b[2][3] || p[0]-half[0]>b[2][3] || p[1]+half[1]< -b[3][3] || p[1]-half[1]>b[3][3]
 });
 match(key(a,motion,offline_faders,offscreen,true),key(b,motion,offline_faders,offscreen,true)){(Some(a),Some(b))=>a!=b,_=>true}
}
/// Isolate lettering changes without weakening the synchronous content rule.
/// A layer is refreshed from the original shader in the current frame; all
/// other image layers keep the established retained-motion policy.
pub(super) fn lettering_groups(a:&[[f32;4]],b:&[[f32;4]],motion:bool,offline:bool)->Option<u32>{
 let offscreen=[false;4];
 if key(a,motion,offline,offscreen,false)?!=key(b,motion,offline,offscreen,false)?{return None;}
 fn groups(rows:&[[f32;4]])->Option<[Vec<u32>;2]>{
  let count=integer(rows.get(212)?[2],1536)?;
  let gpu=rows[212][1];let base=if gpu==(crate::workstation::piano_scene::GPU_END)as f32{
   crate::workstation::piano_scene::END
  }else{integer(gpu,rows.len())?};
  if base.checked_add(count*2)?>rows.len(){return None;}
  let mut keys:[Vec<u32>;2]=Default::default();
  for i in 0..count{
   let descriptor=rows[base+i*2+1];let panel=integer(descriptor[1],3)?;
   if !(2..=3).contains(&panel){return None;}
   keys[panel-2].extend(rows[base+i*2].map(f32::to_bits));keys[panel-2].extend(descriptor.map(f32::to_bits));
  }Some(keys)
 }
 let old=groups(a)?;let new=groups(b)?;let mask=(u32::from(old[0]!=new[0])<<3)|(u32::from(old[1]!=new[1])<<4);
 (mask!=0).then_some(mask)
}
/// Conservative owner masks for synchronous content refreshes. Shared or
/// unclassified fields can select multiple groups; malformed packets fail closed.
/// Secondary illumination is refreshed by the existing full settle capture.
pub(super) fn content_groups(a:&[[f32;4]],b:&[[f32;4]],motion:bool,offline:bool)->Option<u32>{
 // Compact atlas group zero owns stationary timeline/monitor content;
 // group five is the independent transport/ruler image, not the editor.
 let mut mask=0;
 for group in 1..=5 {
  let offscreen=std::array::from_fn(|i|group==5||i!=group-1);
  if key(a,motion,offline,offscreen,true)?!=key(b,motion,offline,offscreen,true)?{
   mask|=1<<if group==5{0}else{group};
  }
 }
 if timeline_changed(a,b){mask|=1;}
 (mask!=0).then_some(mask)
}
#[cfg(test)]mod content_tests{
 use super::*;
 fn fixture()->Vec<[f32;4]>{let mut r=vec![[0.;4];1024];r[212]=[0.,512.,1.,1.];r[513]=[52.,2.,0.,0.];r[12][0]=1.;r[192]=[0.,0.,0.,1.];r[193]=[100.,200.,0.,0.];r}
 #[test]fn content_refresh_masks_keep_unrelated_gems_resident(){
  let a=fixture();let mut b=a.clone();b[513][0]+=1.;
  assert_eq!(content_groups(&a,&b,true,true),Some(1<<3));
  b[217][1]+=1.;assert_eq!(content_groups(&a,&b,true,true),Some(1<<3));
  let mut b=a.clone();b[193][0]+=1.;
  assert_eq!(content_groups(&a,&b,true,true),Some(1<<1));
  let mut a=fixture();a[12][0]=5.;a[200]=[0.,0.,0.,1.];
  let mut b=a.clone();b[201][0]=42.;
  assert_eq!(content_groups(&a,&b,true,true),Some(1<<2));
  use crate::workstation::timeline_scene::{HEADER,END};
  let mut a=fixture();a.resize(END,[0.;4]);a[HEADER][0]=1.;
  let mut b=a.clone();b[HEADER+1][0]+=0.1;
  assert_eq!(content_groups(&a,&b,true,true),Some(1));
  b[513][0]+=1.;assert_eq!(content_groups(&a,&b,true,true),Some(1|(1<<3)));
  assert_eq!(content_groups(&a,&[],true,true),None);
 }
 #[test]fn timeline_drag_content_cannot_reuse_a_settled_capture(){
  use crate::workstation::timeline_scene::{HEADER,DYNAMIC_BASE,WAVE_BASE,END};
  let mut a=vec![[0.;4];END];a[HEADER][0]=1.;
  assert!(!timeline_changed(&a,&a));
  for at in [HEADER,HEADER+2,HEADER+6,DYNAMIC_BASE,DYNAMIC_BASE+1,WAVE_BASE,WAVE_BASE+1,END-1] {
   let mut b=a.clone();b[at][0]+=1.;assert!(timeline_changed(&a,&b),"row {at}");
  }
  let mut b=a.clone();b[HEADER+4][0]=1.;assert!(!timeline_changed(&a,&b));
  assert!(timeline_changed(&a,&[]));
 }
 #[test]fn changed_lettering_refreshes_only_its_real_panel_and_other_content_fails_closed(){
  let a=fixture();let mut b=a.clone();b[513][0]=53.;
  assert_eq!(lettering_groups(&a,&b,true,true),Some(1<<3));
  b[512][1]+=0.01;assert_eq!(lettering_groups(&a,&b,true,true),Some(1<<3));
  b[193][0]+=1.;assert_eq!(lettering_groups(&a,&b,true,true),None);
  let mut a=fixture();a[212][2]=2.;a[514]=[0.2,0.3,0.,0.01];a[515]=[42.,3.,0.,0.];
  let mut b=a.clone();b[514][0]+=0.02;assert_eq!(lettering_groups(&a,&b,true,true),Some(1<<4));
  b[513][0]+=1.;assert_eq!(lettering_groups(&a,&b,true,true),Some((1<<3)|(1<<4)));
  b[515][1]=1.;assert_eq!(lettering_groups(&a,&b,true,true),None);
 }
 #[test]fn an_empty_timeline_has_no_retained_mesh_pixels_to_invalidate(){
  use crate::workstation::timeline_scene::{HEADER,END};
  let a=vec![[0.;4];END];let mut b=a.clone();b[HEADER+1]=[-0.2,-0.3,0.4,0.5];
  b[HEADER+2]=[0.1,0.2,0.3,1.];b[HEADER+6]=[0.,0.1,0.2,10.];
  assert!(!timeline_changed(&a,&b));
  b[HEADER][0]=1.;assert!(timeline_changed(&a,&b));
 }
 #[test]fn parked_panel_visibility_does_not_force_a_synchronous_capture(){
  let mut a=fixture();a[2][3]=1.;a[3][3]=1.;
  let mut b=a.clone();b[34]=[-3.,0.,0.,0.];b[35]=[0.1,0.1,0.1,0.];b[192][3]=0.;
  assert!(!changed_for_motion(&a,&b,true));
  b[34][0]=0.;assert!(changed_for_motion(&a,&b,true));
 }
 #[test]fn ruler_motion_and_visibility_invalidate_stationary_capture(){
  let base=crate::workstation::timeline_bar_scene::BASE;
  let a=vec![[0.;4];base+10];
  for offset in 0..6{let mut b=a.clone();b[base+offset][3]=1.;assert!(rulers_changed(&a,&b));assert!(!changed(&a,&b));}
  let mut b=a.clone();b[base+7][3]=1.;assert!(!rulers_changed(&a,&b));
 }
 #[test]fn offline_cap_changes_position_without_rebuilding_the_mixer(){
  use crate::workstation::fader_components as fader;
  let mut a=fixture();a.resize(fader::BASE+fader::ROWS,[0.;4]);
  a[fader::BASE]=[1.,12544.,0.,1.];
  let mut b=a.clone();
  b[fader::BASE+9][1]=0.2;
  b[fader::BASE+13][1]=0.8;
  b[fader::BASE+6][0]=1.;
  assert!(changed(&a,&b));assert!(!compare(&a,&b,false,true));
  b[fader::BASE+9][2]=0.5;
  assert!(compare(&a,&b,false,true));
 }
 #[test]fn panel_motion_only_reuses_layout_changes(){
  use crate::workstation::{bottom_panel_scene as bottom,fader_components as faders};
  let mut a=fixture();a.resize(bottom::END,[0.;4]);
  a[255]=[bottom::BASE as f32,1.,1.,0.];
  a[faders::BASE]=[1.,12544.,0.,1.];
  for at in [bottom::BASE,bottom::POSES,faders::BASE+8] {
   let mut b=a.clone();b[at][0]+=1.;
   assert!(changed(&a,&b));assert!(!changed_for_motion(&a,&b,true));
  }
  for at in [bottom::LABELS,bottom::FADERS,faders::BASE+8+5] {
   let mut b=a.clone();b[at][1]+=0.25;
   assert!(changed_for_motion(&a,&b,true));
  }
 }
 #[test]fn glyph_and_control_identity_change_cannot_reuse_old_picture(){let a=fixture();for(at,c)in[(513,0),(513,1),(193,0),(193,2),(12,0)]{let mut b=a.clone();b[at][c]+=1.;assert!(changed(&a,&b));}let mut b=a.clone();b[192][3]=0.;assert!(changed(&a,&b));}
 #[test]fn unchanged_content_can_move_and_relight(){let a=fixture();let mut b=a.clone();b[512]=[1.,2.,3.,4.];b[192][0]=0.2;b[192][3]=0.7;b[5][0]=0.9;b[34][0]=0.3;assert!(!changed(&a,&b));}
 #[test]fn physical_transport_depth_is_pose_not_new_content(){
  let mut base=fixture();base.resize(crate::workstation::transport_pose::META_BASE+crate::workstation::transport_pose::META_ROWS,[0.;4]);base[12][0]=4.;
  for i in 0..4{base[192+2*i]=[i as f32*0.055,0.,0.0235,1.];base[193+2*i]=[100.,200.,0.,0.];}
  let mut neutral=base.clone();crate::workstation::transport_pose::apply(&mut neutral,[0.;4],1.3/780.).unwrap();
  let mut pressed=base;crate::workstation::transport_pose::apply(&mut pressed,[0.5,0.25,0.,0.5],1.3/780.).unwrap();
  assert_ne!(neutral[192][0],pressed[192][0]);assert_ne!(neutral[192][2],pressed[192][2]);assert_eq!(neutral[193],pressed[193]);
  assert!(!changed(&neutral,&pressed));pressed[193][0]+=1.;assert!(changed(&neutral,&pressed));
 }
  #[test]fn io_palette_labels_and_visibility_require_new_content_without_hiding_transport_motion(){
   let io=crate::workstation::track_io_scene::HEADER;let pose=crate::workstation::track_io_scene::POSE_BASE;
   let mut a=fixture();a.resize(crate::workstation::engraving::BASE+crate::workstation::engraving::ROWS,[0.;4]);
   a[io]=[pose as f32,1.,0.,0.];a[pose]=[0.5,0.,0.1,1.];a[pose+1]=[3501000.,3501100.,1.,3500544.];a[pose+2]=[73.,78.,49.,0.];
   for(row,col)in[(pose+1,3),(pose+2,2),(io,1)]{let mut b=a.clone();b[row][col]+=1.;assert!(changed(&a,&b));}
   let mut b=a.clone();b[pose][3]=0.;assert!(changed(&a,&b));
   b=a.clone();b[192][0]+=0.001;b[192][2]-=0.001;assert!(!changed(&a,&b));
  }
 #[test]fn keyboard_toggle_visibility_state_and_label_refresh_independently_from_io(){
  let mut a=fixture();a.resize(44611,[0.;4]);a[44592]=[44596.,3.,0.,0.];a[44596]=[1.,2.,3.,1.];a[44597]=[50.,60.,9.,100.];a[44598]=[75.,69.,89.,66.];a[44599]=[79.,65.,82.,68.];
  for (at,c) in [(44592,1),(44592,2),(44592,3),(44597,2),(44597,3),(44598,0),(44599,3),(44600,0)]{let mut b=a.clone();b[at][c]+=1.;assert!(changed(&a,&b));}
  let mut moved=a.clone();moved[44596][0]+=1.;moved[44596][1]-=1.;assert!(!changed(&a,&moved));
  let mut hidden=a.clone();hidden[44592][1]=0.;assert!(changed(&a,&hidden));
 }
 #[test]fn invalid_or_missing_content_is_not_reused(){let a=fixture();let mut b=a.clone();b[212][1]=f32::NAN;assert!(changed(&a,&b));b=a.clone();b[212][2]=1537.;assert!(changed(&a,&b));assert!(changed(&a,&[]));}
}
