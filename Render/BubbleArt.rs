//! Immutable Cycles flipbooks. Frame selection is independent of placement age;
//! no Blender, file I/O, optical work or world-cache refresh occurs on a tick.
use std::path::Path;
use serde_json::Value;
#[derive(Clone,Debug)]
pub(super) struct Family {
 pub length:f32,pub pad:f32,pub low_y:f32,pub high_y:f32,
 pub width:usize,pub height:usize,pub offset:usize,
}
pub(super) struct Bank {
 pub families:Vec<Family>,pub bytes:usize,
 fps:f64,placement:usize,idle:usize,crossfade:usize,settle:f64,
}
fn integer(v:&Value,key:&str)->Result<usize,String>{v[key].as_u64().and_then(|v|usize::try_from(v).ok()).ok_or_else(||format!("invalid bubble art {key}"))}
fn number(v:&Value,key:&str)->Result<f32,String>{v[key].as_f64().filter(|v|v.is_finite()).map(|v|v as f32).filter(|v|v.is_finite()).ok_or_else(||format!("invalid bubble art {key}"))}
pub(super) fn placement_age(encoded:f32)->(f32,bool){
 let selected=encoded>=4.;
 // Decode the packed settled sentinel BEFORE subtracting the selection bit.
 // 5.35f32 - 4 is slightly below1.35; treating it as a new placement every
 // frame would repeatedly restart the selected note's film clock.
 let age=if encoded>=if selected{5.35}else{1.35}{1.35}else{(encoded-if selected{4.}else{0.}).max(0.)};
 (age,selected)
}
impl Bank {
 pub fn load(root:&Path)->Result<(Self,Vec<u8>),String>{Self::load_mode(root,true)}
 pub fn load_mode(root:&Path,images:bool)->Result<(Self,Vec<u8>),String>{
  let path=root.join("Art/AudioWorkstation/BubbleNotes/Baked");
  let d:Value=serde_json::from_slice(&std::fs::read(path.join("manifest.json")).map_err(|e|format!("missing offline bubble artwork: {e}"))?).map_err(|e|e.to_string())?;
  if d["schema"]!=2{return Err("unsupported bubble artwork schema".into());}
  let fps=number(&d,"fps")? as f64;let settle=d["settle_seconds"].as_f64().filter(|v|v.is_finite()).ok_or("invalid bubble settling time")?;
  let placement=integer(&d,"placement_frames")?;let idle=integer(&d,"idle_frames")?;let crossfade=integer(&d,"loop_crossfade_frames")?;
  let bytes=integer(&d,"bytes")?;
  if !(1. ..=60.).contains(&fps)||!(0.1..=5.).contains(&settle)||placement<2||placement>128||idle<4||idle>256||crossfade==0||crossfade*2>=idle||bytes==0||bytes>128*1024*1024||bytes%4!=0{return Err("invalid bounded bubble artwork".into());}
  let entries=d["families"].as_array().filter(|v|v.len()==4).ok_or("four authored bubble duration families required")?;
  let mut families=Vec::new();let mut expected=0;
  for entry in entries {
   let family=Family{length:number(entry,"length")?,pad:number(entry,"pad")?,low_y:number(entry,"low_y")?,high_y:number(entry,"high_y")?,width:integer(entry,"width")?,height:integer(entry,"height")?,offset:integer(entry,"offset")?};
   if !(0.001..=1.).contains(&family.length)||!(0. ..=0.02).contains(&family.pad)||family.low_y>=family.high_y||family.width==0||family.width>2048||family.height==0||family.height>512||family.offset!=expected||integer(entry,"frames")?!=placement+idle{return Err("invalid bubble family bounds".into());}
   let pixels=family.width*family.height;
   for frame in 0..placement+idle {let offset=family.offset+frame*pixels;if offset as f32 as usize!=offset{return Err("bubble image address is not exactly representable".into());}}
   expected=expected.checked_add(pixels*(placement+idle)).ok_or("bubble image overflow")?;
   families.push(family);
  }
  if expected.checked_mul(4)!=Some(bytes){return Err("bubble image layout disagrees with manifest".into());}
  let pixels=if images{std::fs::read(path.join("images.rgba8")).map_err(|e|e.to_string())?}else{vec![0;4]};
  if images&&pixels.len()!=bytes{return Err("bubble image byte count disagrees with manifest".into());}
  Ok((Self{families,bytes:if images{bytes}else{0},fps,placement,idle,crossfade,settle},pixels))
 }
 pub fn family(&self,width:f32,height:f32)->usize {
  let length=(width*0.018/height).max(1e-12);
  self.families.iter().enumerate().min_by(|(_,a),(_,b)|(a.length/length).ln().abs().total_cmp(&(b.length/length).ln().abs())).map(|(i,_)|i).unwrap_or(0)
 }
 /// Four image addresses permit temporal interpolation and a soft wrap of the
 /// original nonperiodic flowing material, without reversing its animation.
 pub fn frames(&self,family:usize,age:f64,clock:f64)->([f32;4],[f32;4]) {
  let f=&self.families[family];let pixels=f.width*f.height;
  let address=|frame:usize|(f.offset+frame*pixels)as f32;
  let (indices,fraction,wrap)=if age<self.settle {
   let position=(age.max(0.)*self.fps).min((self.placement-1)as f64);
   let i=position.floor()as usize;let next=(i+1).min(self.placement-1);
   ([i,next,i,next],position.fract()as f32,0.)
  }else{
   let period=self.idle-self.crossfade;
   let position=(clock.max(0.)*self.fps).rem_euclid(period as f64);
   let i=position.floor()as usize;let next=i+1;
   let wrapping=clock*self.fps>=period as f64;
   let j=if wrapping&&i<self.crossfade {period+i}else{i};
   ([self.placement+j,self.placement+(j+1).min(self.idle-1),self.placement+i,self.placement+next],position.fract()as f32,
    if wrapping&&i<self.crossfade {(position/self.crossfade as f64)as f32}else{0.})
  };
  (indices.map(address),[fraction,wrap,f.width as f32,f.height as f32])
 }
}

#[cfg(test)]mod tests {
 use super::*;
 #[test]fn selected_settled_sentinel_cannot_restart_the_film_clock(){
  assert!(((5.35f32-4.)as f64)<1.35);
  for selected in [false,true]{
   let encoded=(1.35+if selected{4.}else{0.})as f32;
   let (age,decoded)=placement_age(encoded);
   assert_eq!(decoded,selected);assert!(age as f64>=1.35);
  }
  assert_eq!(placement_age(4.5),(0.5,true));
 }
 #[test]fn source_flipbooks_preserve_transparency_motion_and_bounded_addresses(){
  let root=Path::new(env!("CARGO_MANIFEST_DIR"));let (bank,pixels)=Bank::load(root).unwrap();
  for f in &bank.families {
   let stride=f.width*f.height*4;let rest=(f.offset+bank.placement*f.width*f.height)*4;
   let frame=&pixels[rest..rest+stride];
   let settled=&pixels[rest-stride..rest];
   assert!(frame.iter().zip(settled).all(|(a,b)|a.abs_diff(*b)<=1),"placement must join the settled film without a visual jump");
   assert!(frame.chunks_exact(4).any(|v|v[3]==0));
   assert!(frame.chunks_exact(4).filter(|v|v[..3].iter().copied().max().unwrap()>v[3].saturating_add(16)).count()>10,"bright reflected light must not be attenuated by transmission alpha twice");
   let translucent=frame.chunks_exact(4).filter(|v|v[3]>0&&v[3]<250).count();
   assert!(translucent>f.width*f.height/10,"source membrane must transmit its background");
   assert_ne!(frame,&pixels[rest+stride*12..rest+stride*13],"film must keep flowing after placement settles");
  }
  let (a,_)=bank.frames(0,0.1,4.);let(b,_)=bank.frames(0,0.5,4.);assert_ne!(a,b);
  let (a,_)=bank.frames(0,1.35,0.2);let(b,_)=bank.frames(0,1.35,0.8);assert_ne!(a,b,"settled age cannot freeze film clock");
 }
 #[test]fn durations_choose_authored_shapes_and_loop_addresses_remain_in_range(){
  let (bank,_)=Bank::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
  for (i,f) in bank.families.iter().enumerate(){
   assert_eq!(bank.family(f.length,0.018),i);
   for time in [0.,0.124,0.125,1.349,1.35,2.,100000.] {
    let (addresses,weights)=bank.frames(i,time,time);
    assert!(weights[0]>=0.&&weights[0]<=1.&&weights[1]>=0.&&weights[1]<=1.);
    for offset in addresses{assert!((offset as usize+f.width*f.height)*4<=bank.bytes);}
   }
  }
 }
}
