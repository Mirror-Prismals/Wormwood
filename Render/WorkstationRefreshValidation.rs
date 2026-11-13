//! Finite native validation only. No production frame is modified unless the
//! explicit diagnostic flag is selected. Audio remains real; playhead pixels
//! are held constant here so reference and staged captures compare exactly.
use super::*;
pub(super) struct Validation{pub mode:String,pub raw:Vec<[f32;4]>,pub capture_at:Option<u64>,pub observations:Vec<Value>}
impl Validation{
 pub fn requested()->Result<Option<Self>,String>{let mode=std::env::var("WORMWOOD_WORKSTATION_REFRESH_VALIDATION").unwrap_or_default();if mode.is_empty()||mode=="0"{return Ok(None);}if !["lighting","content","resize","meters","playhead"].contains(&mode.as_str()){return Err("refresh validation must be lighting, content, resize, meters or playhead".into());}if !authorized(&mode,std::env::var("WORMWOOD_WORKSTATION_BENCHMARK_AUDIO").as_deref()==Ok("1"),std::env::var("WORMWOOD_COMBINED_CAPTURE").as_deref()==Ok("1")){return Err("refresh validation requires explicit native audio benchmark or combined meter capture".into());}Ok(Some(Self{mode,raw:vec![],capture_at:None,observations:vec![]}))}
 pub fn rows(&self,frame:u64)->Option<Vec<[f32;4]>>{if self.raw.len()<256{return None;}let mut r=self.raw.clone();r[252]=[0.;4];r[245][3]=0.;
  let state=match if matches!(self.mode.as_str(),"meters"|"playhead"){frame%120}else{frame}{0..=7=>0,8..=39=>1,40=>2,41=>3,42..=109=>2,_=>4};
  if self.mode=="lighting"||self.mode=="resize"{let strength=[1.,0.65,1.25,0.8,1.1][state];for c in 0..3{r[5][c]*=strength;}r[16][0]+=[0.,0.08,-0.05,0.03,0.06][state];}
  if self.mode=="content"{
   let glyph=[52.,50.,51.,52.,50.][state];
   let base=r[212][1]as usize;
   if r[212][2]>0.&&base+1<r.len(){r[base+1][0]=glyph;}
   // The engraving block can be empty in the normal Origami layout. Also
   // change an actual bottom button label to exercise a synchronous repack.
   let label=crate::workstation::bottom_panel_scene::LABELS;
   if r.len()>label && r[255][2]>0.{r[label][0]=glyph;}
  }
  if self.mode=="meters" || self.mode=="playhead" {
   let base=crate::workstation::fader_components::BASE;
   if r.len()>=base+crate::workstation::fader_components::ROWS {
    for i in 0..(r[base][0]as usize).min(70){
     let at=base+8+i*6+3;
     r[at][2]=[0.,0.9,0.4,0.01,0.7][state];r[at][3]=[0.,0.4,0.9,0.99,0.2][state];
    }
   }
  }
  if self.mode=="playhead" {
   // Deterministic optical validation only: real geometry and both actual
   // moving descriptors, with the same packet in reference/candidate runs.
   // These synthetic positions are never used to report transport FPS.
   r[252]=self.raw[252];
   let t=(frame%120)as f32/119.;
   let x=r[250][0]+(r[250][1]-r[250][0])*t;
   r[252][0]=x;r[245][3]=frame as f32/60.;
   let control=crate::workstation::timeline_bar_scene::BASE+7;
   if r.len()>control {r[control][3]=x;}
  }
  Some(r)
 }
 pub fn capture_due(&self,frame:u64)->bool{[0,4,8,9,32,33,40,41,42,43,110,111,112,300,599].contains(&frame)}
}

// Meter fixtures may use the already running embedded game audio. Other
// refresh scenarios retain their original standalone benchmark requirement.
fn authorized(mode:&str,audio:bool,combined_capture:bool)->bool{audio||(matches!(mode,"meters"|"playhead")&&combined_capture)}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn synthetic_fixture_requires_explicit_capture_or_audio(){assert!(!authorized("meters",false,false));assert!(authorized("meters",false,true));assert!(authorized("lighting",true,false));assert!(!authorized("lighting",false,true));}
 #[test]fn meter_fixture_varies_levels_without_touching_geometry(){let base=crate::workstation::fader_components::BASE;let mut raw=vec![[0.;4];base+crate::workstation::fader_components::ROWS];raw[base]=[1.,0.,0.,1.];raw[base+8+3]=[0.2,0.3,0.,0.];let v=Validation{mode:"meters".into(),raw:raw.clone(),capture_at:None,observations:vec![]};let mut seen=vec![];for f in[0,8,40,41,110,128]{let r=v.rows(f).unwrap();assert_eq!(r[base+8+3][..2],raw[base+8+3][..2]);seen.push([r[base+8+3][2],r[base+8+3][3]]);for i in 0..r.len(){if ![245,252,base+8+3].contains(&i){assert_eq!(r[i],raw[i]);}}}assert_eq!(seen[0],[0.,0.]);assert_eq!(seen[3],[0.01,0.99]);assert_eq!(seen[1],seen[5]);assert_ne!(seen[2],seen[4]);}
 #[test]fn playhead_fixture_moves_both_real_descriptors_and_preserves_other_geometry(){
  let base=crate::workstation::fader_components::BASE;
  let control=crate::workstation::timeline_bar_scene::BASE+7;
  let mut raw=vec![[0.;4];super::super::super::workstation::FRAME_ROWS];
  raw[base]=[1.,0.,0.,1.];raw[base+11]=[0.2,0.3,0.,0.];
  raw[250]=[-0.7,0.9,0.4,1.];raw[252]=[-0.2,-0.3,0.2,1.];raw[control]=[1.,0.3,0.01,-0.2];
  let v=Validation{mode:"playhead".into(),raw:raw.clone(),capture_at:None,observations:vec![]};
  assert!(authorized("playhead",false,true));assert!(!authorized("playhead",false,false));
  for f in [0,8,40,41,110,119,128]{let r=v.rows(f).unwrap();
   assert_eq!(r[252][1..],raw[252][1..]);assert_eq!(r[control][..3],raw[control][..3]);
   assert_eq!(r[252][0],r[control][3]);assert_eq!(r[245][3],f as f32/60.);
   for i in 0..r.len(){if ![245,252,control,base+11].contains(&i){assert_eq!(r[i],raw[i],"row {i}");}}
  }
  assert_eq!(v.rows(0).unwrap()[252][0],-0.7);
  assert!((v.rows(119).unwrap()[252][0]-0.9).abs()<0.000001);
 }

}
