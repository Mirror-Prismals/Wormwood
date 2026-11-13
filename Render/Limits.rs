//! Shared bounds for the complete composed scene, rather than one contributor.
//! Terrain, venue, rigs and overlays share this storage without dropping faces.
pub(crate) const MAX_SCENE_TRIANGLES: usize = 262_144;
// Clipping one source triangle against the water plane can produce two faces.
pub(crate) const MAX_REFLECTED_TRIANGLES: usize = MAX_SCENE_TRIANGLES * 2;
pub(crate) const SCENE_BUFFER_BYTES: u64 = MAX_SCENE_TRIANGLES as u64 * 24 * 4;
pub(crate) const REFLECTION_BUFFER_BYTES: u64 = MAX_REFLECTED_TRIANGLES as u64 * 24 * 4;

pub(crate) fn validate(triangles: &[[f32;24]], backdrop: bool) -> Result<(),String> {
    let extra=if backdrop {2}else{0};
    if triangles.len()>MAX_SCENE_TRIANGLES-extra {
        return Err(format!("scene triangle budget exceeded: {} + {extra} backdrop triangles, capacity {MAX_SCENE_TRIANGLES}",triangles.len()));
    }
    // A non-short-circuit reduction lets LLVM check finite values in vectors.
    // Only the exceptional block needs a scalar search for the original index.
    for (block,rows) in triangles.chunks(128).enumerate() {
        if rows.as_flattened().iter().fold(false,|bad,v|bad | !v.is_finite()) {
            let index=block*128+rows.iter().position(|t|t.iter().any(|v|!v.is_finite())).unwrap();
            return Err(format!("non-finite scene triangle {index}"));
        }
    }
    Ok(())
}

#[cfg(test)]mod tests {
 use super::*;
 #[test]fn rejected_exterior_count_and_capacity_boundaries(){
  // Recorded when turning back toward the theatre from its entrance steps.
  let mut triangles=vec![[0.;24];131_099];
  assert!(validate(&triangles,false).is_ok());
  triangles.resize(MAX_SCENE_TRIANGLES,[0.;24]);
  assert!(validate(&triangles,false).is_ok());
  assert!(validate(&triangles,true).is_err());
  triangles.truncate(MAX_SCENE_TRIANGLES-2);
  assert!(validate(&triangles,true).is_ok());
  triangles.resize(MAX_SCENE_TRIANGLES+1,[0.;24]);
  assert!(validate(&triangles,false).is_err());
 }
 #[test]fn nonfinite_geometry_is_still_rejected(){
  for value in [f32::NAN,f32::INFINITY,f32::NEG_INFINITY] {
   let mut triangles=[[0.;24];2];triangles[1][23]=value;
   assert_eq!(validate(&triangles,false).unwrap_err(),"non-finite scene triangle 1");
  }
  assert_eq!(REFLECTION_BUFFER_BYTES,SCENE_BUFFER_BYTES*2);
 }
 #[test]fn vector_finite_check_preserves_first_bad_triangle_across_blocks(){
  let mut rows=vec![[1.;24];MAX_SCENE_TRIANGLES-2];
  assert!(validate(&rows,true).is_ok());
  for index in [0,127,128,255,256,rows.len()-1]{
   rows[index][23]=f32::from_bits(0xffc01234);
   assert_eq!(validate(&rows,true).unwrap_err(),format!("non-finite scene triangle {index}"));rows[index][23]=1.;
  }
  rows[129][0]=f32::INFINITY;rows[300][1]=f32::NAN;
  assert_eq!(validate(&rows,false).unwrap_err(),"non-finite scene triangle 129");
 }
}
