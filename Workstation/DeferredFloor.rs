//! Host-only deferred floor-field request. Header row 255.w is otherwise unused
//! by the optical shaders. Placeholder rows must be resolved before any optical
//! bake or original draw; only retained image composition may omit resolution.
use super::floor_light;
use std::{borrow::Cow,time::Instant};
const TAG:f32=-8192.;
const START:usize=256;
pub(crate) fn requested(rows:&[[f32;4]])->bool{rows.len()>255&&rows[255][3]==TAG}
pub(crate) fn append_placeholder(rows:&mut Vec<[f32;4]>) {
 assert_eq!(rows.len(),START);
 rows.resize(START+floor_light::ROWS,[1.;4]);
}
/// Seal the request after timeline and all other packet writers have finished.
/// Timeline owns and reconstructs rows 248..256, including this reserved slot.
pub(crate) fn mark_request(rows:&mut [[f32;4]])->Result<(),String>{
 validate(rows)?;rows[255][3]=TAG;Ok(())
}
fn validate(rows:&[[f32;4]])->Result<(),String>{
 if rows.len()<START+floor_light::ROWS||rows[10][0]!=START as f32||rows[10][1]!=floor_light::EDGE as f32 {
  return Err("deferred floor field requires the complete standard field descriptor".into());
 }Ok(())
}
pub(crate) fn complete(rows:&[[f32;4]])->Result<Cow<'_,[[f32;4]]>,String>{
 if !requested(rows){return Ok(Cow::Borrowed(rows));}
 validate(rows)?;let mut out=rows.to_vec();
 out[START..START+floor_light::ROWS].copy_from_slice(&floor_light::build(rows));
 Ok(Cow::Owned(out))
}
#[derive(Default)]
pub(crate) struct Resolver {
 cache:floor_light::Cache,
 latest:Option<Vec<[f32;4]>>,
 resolved:bool,
 pub skipped:u64,
 pub resolutions:u64,
 pub cpu_ms:f64,
}
impl Resolver {
 pub fn ingest<'a>(&mut self,rows:&'a [[f32;4]],hold_images:bool)->Result<Cow<'a,[[f32;4]]>,String>{
  if !requested(rows){self.latest=None;self.resolved=true;return Ok(Cow::Borrowed(rows));}
  validate(rows)?;
  let latest=self.latest.get_or_insert_with(Vec::new);latest.clear();latest.extend_from_slice(rows);self.resolved=false;
  if hold_images {self.skipped+=1;return Ok(Cow::Borrowed(rows));}
  Ok(Cow::Owned(self.resolve_pending()?.expect("new deferred input")))
 }
 pub fn resolve_pending(&mut self)->Result<Option<Vec<[f32;4]>>,String>{
  if self.resolved{return Ok(None);}
  let Some(rows)=&self.latest else{return Ok(None);};validate(rows)?;
  let start=Instant::now();let mut out=rows.clone();
  out[START..START+floor_light::ROWS].copy_from_slice(self.cache.field(rows));
  self.cpu_ms+=start.elapsed().as_secs_f64()*1000.;self.resolutions+=1;self.resolved=true;
  Ok(Some(out))
 }
 pub fn invalidate(&mut self){self.latest=None;self.resolved=true;}
}
/// These live geometry families require the original current-input renderer.
/// Timeline and authored controls remain part of the retained component layers.
pub(crate) fn retained_geometry_supported(rows:&[[f32;4]])->bool{
 (rows.len()<=super::piano_scene::BASE||(rows[super::piano_scene::BASE][0]==0. && rows[super::piano_scene::BASE][3]==0.))&&rows.len()>248&&rows[234][0]==0.&&rows[240][0]==0.&&rows[245][0]==0.&&rows[245][2]==0.
}
/// Moving panels may carry their illumination. Explicit edits to lights, floor
/// material or environment/camera are semantic changes and refresh immediately.
pub(crate) fn lighting_changed(a:&[[f32;4]],b:&[[f32;4]])->bool{
 if a.len()<256||b.len()<256{return true;}
 a[1..5]!=b[1..5]||a[5][..3]!=b[5][..3]||a[6]!=b[6]||a[9][..3]!=b[9][..3]||a[16..31]!=b[16..31]
}
#[cfg(test)]mod tests{
 use super::*;
 fn fixture()->Vec<[f32;4]>{
  let mut r=vec![[0.;4];256];r[0][1]=1.;r[2][3]=0.75;r[3][3]=0.75;r[5][3]=-2.;r[9][0]=1.;
  r[16]=[0.,0.,2.,0.];r[17]=[1.,0.,0.,0.2];r[18]=[0.,1.,0.,0.2];r[19]=[0.,0.,-1.,0.];
  r[32]=[1.,1.766,0.,1.];r[33]=[0.2,0.8,2.,0.];r[35]=[4.,4.,0.5,0.];
  let planes=[[1.,0.,0.,4.],[-1.,0.,0.,4.],[0.,1.,0.,4.],[0.,-1.,0.,4.],[0.,0.,1.,0.5],[0.,0.,-1.,0.5]];
  for i in 0..26{r[38+i]=planes[i%6];}r[10]=[256.,64.,0.,0.];append_placeholder(&mut r);mark_request(&mut r).unwrap();r
 }
 fn field(r:&[[f32;4]])->&[[f32;4]]{&r[256..256+floor_light::ROWS]}
 #[test]fn full_view_coverage_is_not_a_lighting_edit(){let a=fixture();let mut b=a.clone();b[9][3]=0.5;assert!(!lighting_changed(&a,&b));b[9][0]+=1.;assert!(lighting_changed(&a,&b));}
 #[test]fn request_is_sealed_after_timeline_replaces_reserved_header(){
  let mut r=fixture();r[248..256].fill([0.;4]);
  assert!(!requested(&r));mark_request(&mut r).unwrap();assert!(requested(&r));
  let mut d=Resolver::default();let actual=d.ingest(&r,false).unwrap();
  assert_eq!(field(&actual),floor_light::build(&r));assert_eq!(d.resolutions,1);
 }
 #[test]fn motion_then_no_input_resolves_latest_exact_field_once(){
  let mut r=fixture();let mut d=Resolver::default();let first=d.ingest(&r,false).unwrap().into_owned();assert_eq!(field(&first),floor_light::build(&r));
  for x in [0.2,0.6,1.0]{r[16][0]=x;d.ingest(&r,true).unwrap();}assert_eq!(d.resolutions,1);assert_eq!(d.skipped,3);
  let settled=d.resolve_pending().unwrap().unwrap();assert_eq!(field(&settled),floor_light::build(&r));assert_ne!(field(&first),field(&settled));assert!(d.resolve_pending().unwrap().is_none());
 }
 #[test]fn explicit_lighting_edit_refreshes_even_during_panel_motion(){
  let before=fixture();let mut after=before.clone();after[34][0]=0.2;assert!(!lighting_changed(&before,&after));after[16][0]=1.;assert!(lighting_changed(&before,&after));
  let mut d=Resolver::default();d.ingest(&before,false).unwrap();let new=d.ingest(&after,false).unwrap();assert_eq!(field(&new),floor_light::build(&after));
 }
 #[test]fn unsupported_geometry_and_other_backends_get_complete_original_field(){
  for row in [234,240,245]{let mut r=fixture();r[row][0]=1.;assert!(!retained_geometry_supported(&r));let mut d=Resolver::default();d.ingest(&r,true).unwrap();let fallback=d.resolve_pending().unwrap().unwrap();assert_eq!(field(&fallback),floor_light::build(&r));assert_eq!(field(&complete(&r).unwrap()),field(&fallback));}
 }
 #[test]fn invalid_descriptor_is_rejected_and_external_input_clears_pending(){
  let mut r=fixture();let mut d=Resolver::default();d.ingest(&r,true).unwrap();d.invalidate();assert!(d.resolve_pending().unwrap().is_none());r[10][0]=257.;assert!(d.ingest(&r,true).is_err());assert!(complete(&r).is_err());
 }
}

#[cfg(test)]mod piano_room_retention_tests{#[test]fn empty_piano_room_cannot_reuse_timeline_geometry(){let mut rows=vec![[0.;4];super::super::piano_scene::END];assert!(super::retained_geometry_supported(&rows));rows[super::super::piano_scene::BASE][0]=1.;assert!(!super::retained_geometry_supported(&rows));}}
