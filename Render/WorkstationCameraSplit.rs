//! Exact draw partition: unchanged camera vertex colors precede unchanged gems.
//! Unexpected/mixed material order keeps the original single draw.
pub(super) const ENTRY:&str = include_str!("WorkstationCameraColor.wgsl");
fn camera_face(tags:[f32;3])->bool {
 tags==[tags[0];3] && (tags[0]==90. || ((100.0..=104.0).contains(&tags[0]) || (106.0..=120.0).contains(&tags[0])))
}
pub(super) fn prefix(triangles:&[[f32;24]])->u32 {
 let mut camera=0usize;let mut foreground=false;
 for t in triangles {
  let tags=[t[15],t[19],t[23]];
  if camera_face(tags) {if foreground{return 0;}camera+=1;}
  else if tags.iter().all(|v|v.is_finite()&&*v<=89.5){foreground=true;}
  else{return 0;}
 }
 if foreground{(camera*3)as u32}else{0}
}
#[cfg(test)] mod tests {
 use super::*;
 fn tri(tag:f32)->[f32;24]{let mut t=[0.;24];for i in [15,19,23]{t[i]=tag;}t}
 #[test] fn partition_preserves_all_vertices_and_order(){let c=tri(90.);let g=tri(0.);for n in 1..100{let mut v=vec![c;n];v.extend([g,g]);let p=prefix(&v);assert_eq!(p,n as u32*3);assert_eq!(p as usize+(v.len()*3-p as usize),v.len()*3);}assert_eq!(prefix(&[g,g]),0);assert_eq!(prefix(&[c,c]),0);}
 #[test] fn mixed_unknown_nonfinite_and_interleaved_tags_fall_back(){let c=tri(90.);let g=tri(0.);assert_eq!(prefix(&[c,g,c]),0);for tag in [89.75,91.,f32::NAN,f32::INFINITY]{assert_eq!(prefix(&[tri(tag),g]),0);}let mut mixed=c;mixed[19]=0.;assert_eq!(prefix(&[mixed,g]),0);}
 #[test] fn trivial_fragment_validates_without_optical_calls(){let text=format!("{}{}",include_str!("../Procedures/Shaders/workstation.wgsl"),ENTRY);let m=naga::front::wgsl::parse_str(&text).unwrap();naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).unwrap();let _e=m.entry_points.iter().find(|e|e.name=="fs_camera_color").unwrap();assert!(!ENTRY.contains("workstationShade"));}
}

/// Only this authored fullscreen tail may be redrawn at equal depth.
/// Arbitrary foreground triangles keep the original one-pass renderer.
pub(super) fn retained_batch(triangles:&[[f32;24]])->bool {
 if triangles.len()<2{return false;}
 let n=triangles.len();
 if triangles[..n-2].iter().any(|t|!camera_face([t[15],t[19],t[23]])){return false;}
 let corners=[[[-1.,-1.],[1.,-1.],[1.,1.]],[[-1.,-1.],[1.,1.],[-1.,1.]]];
 for (t,cs) in triangles[n-2..].iter().zip(corners) {
  for (i,p) in cs.iter().enumerate(){if t[i*4..i*4+4]!=[p[0],p[1],0.5,1.]{return false;}}
  if t[12..].iter().any(|x|*x!=0.){return false;}
 }true
}
#[cfg(test)]mod tail_tests{
 use super::*;
 #[test]fn unsupported_foreground_depth_or_payload_keeps_original_shader(){
  let mut ts=vec![[0.;24];2];for(t,cs)in ts.iter_mut().zip([[[-1.,-1.],[1.,-1.],[1.,1.]],[[-1.,-1.],[1.,1.],[-1.,1.]]]){for(i,p)in cs.iter().enumerate(){t[i*4..i*4+4].copy_from_slice(&[p[0],p[1],0.5,1.]);}}
  assert!(retained_batch(&ts));for (at,lane)in[(0,2),(0,12),(1,0)]{let mut altered=ts.clone();altered[at][lane]+=0.01;assert!(!retained_batch(&altered));}
  let mut camera=[0.;24];for i in[15,19,23]{camera[i]=90.;}ts.insert(0,camera);assert!(retained_batch(&ts));ts[0][15]=0.;assert!(!retained_batch(&ts));
 }
}
