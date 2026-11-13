//! Homogeneous clipping and depth separation for the theatre camera backdrop.
//! Foreground gems keep their original depth; the feed uses ordinary rasterization.
use serde_json::Value;
use std::sync::Arc;

/// One immutable venue and exact camera input produce one complete triangle
/// packet. Projection stays in layer four through the caller's build closure.
/// Keeping the Arc prevents allocator address reuse from masquerading as a hit.
pub(crate) struct Cache<V> {
 entry: Option<Entry<V>>,
}
struct Entry<V> {
 camera: Value,
 origin_bits: [u64; 3],
 venue: Arc<V>,
 triangles: Vec<[f32; 24]>,
}
impl<V> Default for Cache<V> {
 fn default() -> Self { Self { entry: None } }
}
// Value equality alone equates +0.0 and -0.0. Preserve that distinction too:
// it can affect projection arithmetic even when the camera looks unchanged.
fn camera_equal(a: &Value, b: &Value) -> bool {
 if a != b { return false; }
 match (a, b) {
  (Value::Number(a), Value::Number(b)) => a.as_f64().map(f64::to_bits) == b.as_f64().map(f64::to_bits),
  (Value::Array(a), Value::Array(b)) => a.iter().zip(b).all(|(a,b)| camera_equal(a,b)),
  (Value::Object(a), Value::Object(b)) => a.iter().all(|(key,a)| b.get(key).is_some_and(|b| camera_equal(a,b))),
  _ => true,
 }
}
impl<V> Cache<V> {
 pub(crate) fn clear(&mut self) { self.entry = None; }
 pub(crate) fn get_or_build(
  &mut self, camera: &Value, origin: [f64; 3], venue: &Arc<V>,
  build: impl FnOnce() -> Result<Vec<[f32; 24]>, String>,
 ) -> Result<&[[f32; 24]], String> {
  let origin_bits = origin.map(f64::to_bits);
  // The original theatre shader samples wall time when no explicit clock
  // is supplied. Such a view cannot reuse a packet keyed only by camera pose.
  let clock_is_explicit = camera["day_night"]["fraction"].as_f64().is_some_and(f64::is_finite);
  let hit = clock_is_explicit && self.entry.as_ref().is_some_and(|entry|
   entry.origin_bits == origin_bits && Arc::ptr_eq(&entry.venue, venue)
    && camera_equal(&entry.camera, camera));
  if !hit {
   // Failure returns an error rather than publishing a new identity paired
   // with stale geometry. The previous complete entry may still be reused.
   let triangles = build()?;
   self.entry = Some(Entry { camera: camera.clone(), origin_bits,
    venue: venue.clone(), triangles });
  }
  Ok(&self.entry.as_ref().unwrap().triangles)
 }
}

pub(crate) fn prepare(triangles:&[[f32;24]])->Vec<[f32;24]>{
 let mut out=Vec::with_capacity(triangles.len());
 for t in triangles{
  // A three-vertex cycle emits at most four vertices at the first plane;
  // a four-vertex cycle emits at most six at the second. Six also covers
  // nonfinite/degenerate inputs without relying on polygon convexity.
  let mut polygon=[[0.;8];6];let mut clipped=[[0.;8];6];let mut length=3;
  for i in 0..3{polygon[i]=std::array::from_fn(|j|if j<4{t[i*4+j]}else{t[12+i*4+j-4]});}
  for far in [false,true]{
   let distance=|v:&[f32;8]|if far{v[3]-v[2]}else{v[2]};
   if length==0{break;}let mut clipped_length=0;
   for i in 0..length{let a=polygon[i];let b=polygon[(i+1)%length];let da=distance(&a);let db=distance(&b);
    if da>=0.{clipped[clipped_length]=a;clipped_length+=1;}if (da>=0.)!=(db>=0.){let u=da/(da-db);clipped[clipped_length]=std::array::from_fn(|j|a[j]+u*(b[j]-a[j]));clipped_length+=1;}
   }std::mem::swap(&mut polygon,&mut clipped);length=clipped_length;
  }
  for i in 1..length.saturating_sub(1){let mut triangle=[0.;24];for (v,p) in [polygon[0],polygon[i],polygon[i+1]].into_iter().enumerate(){triangle[v*4..v*4+4].copy_from_slice(&p[..4]);triangle[v*4+2]=0.6*p[3]+0.39*p[2];triangle[12+v*4..16+v*4].copy_from_slice(&[p[4],p[5],p[6],if (100.0..=104.0).contains(&t[15]) || (106.0..=111.0).contains(&t[15]) { t[15] } else { 90. }]);}out.push(triangle);}
 }out
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn near_crossing_is_clipped_before_depth_remap(){let mut t=[0.;24];for (i,z) in [-0.1,0.5,0.5].into_iter().enumerate(){t[i*4..i*4+4].copy_from_slice(&[i as f32,0.,z,1.]);t[12+i*4..16+i*4].copy_from_slice(&[0.3,0.4,0.5,1.]);}let r=prepare(&[t]);assert_eq!(r.len(),2);for t in r{for i in 0..3{assert!((0.6..=0.99).contains(&(t[i*4+2]/t[i*4+3])));assert_eq!(t[15+i*4],90.);}}}
 #[test]fn geometry_behind_camera_is_rejected(){let mut t=[0.;24];for i in 0..3{t[i*4+2]=-2.;t[i*4+3]=-1.;}assert!(prepare(&[t]).is_empty());}
}

#[cfg(test)] mod cache_tests {
 use super::*;
 use serde_json::json;
 use std::cell::Cell;
 fn camera() -> Value { json!({"position":[0.,10.,-47.],"yaw":0.,"pitch":0.,"aspect":1.6,"near":0.05,"far":1000.,"two_sided":true,"forward":[0.,0.,1.],"id":"foh-left","page":"timeline","day_night":{"fraction":0.265}}) }
 fn bits(packet: &[[f32;24]]) -> Vec<[u32;24]> { packet.iter().map(|r|r.map(f32::to_bits)).collect() }
 fn packet() -> Vec<[f32;24]> {
  let mut triangle=[0.;24];
  for (i,z) in [-0.1,0.5,0.5].into_iter().enumerate() {
   triangle[i*4..i*4+4].copy_from_slice(&[i as f32,-0.0,z,1.]);
   triangle[12+i*4..16+i*4].copy_from_slice(&[0.3,0.4,0.5,1.]);
  }
  let mut result=prepare(&[triangle]);
  // A caller may append the exact two fullscreen primitives to this packet.
  result.extend([[17.;24],[23.;24]]);result
 }
 #[test] fn identical_inputs_skip_builder_and_preserve_every_packet_bit_and_order() {
  let venue=Arc::new(vec![1u8]);let mut cache=Cache::default();let calls=Cell::new(0);
  let expected=packet();let build=|| {calls.set(calls.get()+1);Ok(expected.clone())};
  assert_eq!(bits(cache.get_or_build(&camera(),[0.;3],&venue,build).unwrap()),bits(&expected));
  assert_eq!(bits(cache.get_or_build(&camera(),[0.;3],&venue,build).unwrap()),bits(&expected));
  assert_eq!(calls.get(),1);
 }
 #[test] fn implicit_or_invalid_clock_always_rebuilds_original_lighting() {
  let venue=Arc::new(());
  for daylight in [None,Some(Value::Null),Some(json!({"fraction":null})),Some(json!({"fraction":"0.265"}))] {
   let mut view=camera();view.as_object_mut().unwrap().remove("day_night");
   if let Some(value)=daylight{view["day_night"]=value;}
   let mut cache=Cache::default();let calls=Cell::new(0);
   for _ in 0..3 {cache.get_or_build(&view,[0.;3],&venue,||{calls.set(calls.get()+1);Ok(packet())}).unwrap();}
   assert_eq!(calls.get(),3);
  }
 }
 #[test] fn explicit_clock_changes_rebuild_even_with_identical_camera_pose() {
  let venue=Arc::new(());let mut cache=Cache::default();let a=camera();let mut b=a.clone();b["day_night"]["fraction"]=json!(0.2651);
  cache.get_or_build(&a,[0.;3],&venue,||Ok(packet())).unwrap();
  let mut changed=packet();changed[0][12]+=0.01;let expected=changed.clone();
  assert_eq!(bits(cache.get_or_build(&b,[0.;3],&venue,||Ok(changed)).unwrap()),bits(&expected));
  cache.get_or_build(&b,[0.;3],&venue,||panic!("explicit unchanged time must reuse")).unwrap();
 }
 #[test] fn every_camera_field_and_nested_value_invalidates() {
  let venue=Arc::new(());let baseline=camera();
  for key in baseline.as_object().unwrap().keys() {
   let mut cache=Cache::default();cache.get_or_build(&baseline,[0.;3],&venue,||Ok(packet())).unwrap();
   let mut changed=baseline.clone();changed[key]=json!({"changed":true});let called=Cell::new(false);
   cache.get_or_build(&changed,[0.;3],&venue,||{called.set(true);Ok(packet())}).unwrap();
   assert!(called.get(),"{key}");
  }
  let mut changed=baseline.clone();changed["forward"][2]=json!(0.999);
  assert!(!camera_equal(&baseline,&changed));
  changed=baseline.clone();changed["aspect"]=json!(1.7);assert!(!camera_equal(&baseline,&changed));
  changed=baseline.clone();changed["yaw"]=json!(-0.0);assert!(!camera_equal(&baseline,&changed));
  changed=baseline.clone();changed["position"][0]=json!(-0.0);assert!(!camera_equal(&baseline,&changed));
  changed=baseline.clone();changed["new_dependency"]=json!(1);assert!(!camera_equal(&baseline,&changed));
 }
 #[test] fn every_origin_axis_and_signed_zero_invalidates() {
  let venue=Arc::new(());
  for axis in 0..3 { for value in [1.,-0.] {
   let mut cache=Cache::default();cache.get_or_build(&camera(),[0.;3],&venue,||Ok(packet())).unwrap();
   let mut origin=[0.;3];origin[axis]=value;let called=Cell::new(false);
   cache.get_or_build(&camera(),origin,&venue,||{called.set(true);Ok(packet())}).unwrap();assert!(called.get());
  }}
 }
 #[test] fn venue_identity_is_retained_and_replacement_invalidates_even_equal_contents() {
  let venue=Arc::new(vec![1]);let same=venue.clone();let replacement=Arc::new(vec![1]);let mut cache=Cache::default();
  cache.get_or_build(&camera(),[0.;3],&venue,||Ok(packet())).unwrap();assert_eq!(Arc::strong_count(&venue),3);
  cache.get_or_build(&camera(),[0.;3],&same,||panic!("same Arc must hit")).unwrap();
  let called=Cell::new(false);cache.get_or_build(&camera(),[0.;3],&replacement,||{called.set(true);Ok(packet())}).unwrap();
  assert!(called.get());assert_eq!(Arc::strong_count(&venue),2);
 }
 #[test] fn failed_build_does_not_publish_or_return_the_old_packet_for_new_inputs() {
  let venue=Arc::new(());let a=camera();let mut b=a.clone();b["aspect"]=json!(2.);let mut cache=Cache::default();
  let expected=packet();cache.get_or_build(&a,[0.;3],&venue,||Ok(expected.clone())).unwrap();
  assert_eq!(cache.get_or_build(&b,[0.;3],&venue,||Err("projection failed".into())).unwrap_err(),"projection failed");
  assert_eq!(bits(cache.get_or_build(&a,[0.;3],&venue,||panic!("failed replacement lost old entry")).unwrap()),bits(&expected));
  let calls=Cell::new(0);cache.get_or_build(&b,[0.;3],&venue,||{calls.set(calls.get()+1);Ok(vec![[7.;24]])}).unwrap();
  assert_eq!(calls.get(),1);
 }
 #[test] fn failed_initial_build_and_clear_require_fresh_builds() {
  let venue=Arc::new(());let mut cache=Cache::default();
  assert!(cache.get_or_build(&camera(),[0.;3],&venue,||Err("no frame".into())).is_err());assert!(cache.entry.is_none());
  cache.get_or_build(&camera(),[0.;3],&venue,||Ok(packet())).unwrap();cache.clear();
  assert_eq!(Arc::strong_count(&venue),1);assert!(cache.entry.is_none());
  let called=Cell::new(false);cache.get_or_build(&camera(),[0.;3],&venue,||{called.set(true);Ok(packet())}).unwrap();assert!(called.get());
 }
}
