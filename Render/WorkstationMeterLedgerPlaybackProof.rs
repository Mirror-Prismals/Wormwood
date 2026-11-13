//! Executable finite-f32 check of the sufficient predicate, not GPU validation.
#[derive(Clone,Copy,Debug)] pub struct Query {pub origin:[f32;3],pub ray:[f32;3],pub center:[f32;3],pub extent:[f32;3],pub limit:f32}
fn inverse(x:f32)->f32{if x<0.{-1.0/x.abs().max(1e-8)}else{1.0/x.abs().max(1e-8)}}
fn intervals(q:Query)->([f32;3],[f32;3],[f32;3],[f32;3]){
 let a=std::array::from_fn(|i|(q.center[i]-q.extent[i]-q.origin[i])*inverse(q.ray[i]));
 let b=std::array::from_fn(|i|(q.center[i]+q.extent[i]-q.origin[i])*inverse(q.ray[i]));
 let lo=std::array::from_fn(|i|a[i].min(b[i]));let hi=std::array::from_fn(|i|a[i].max(b[i]));(a,b,lo,hi)
}
pub fn original_hit(q:Query)->bool{let(_,_,lo,hi)=intervals(q);let near=lo[0].max(lo[1]).max(lo[2]);let far=hi[0].min(hi[1]).min(hi[2]);near>1e-7&&far>=near&&near<q.limit}
pub fn certified(q:Query)->bool{
 let(a,b,lo,hi)=intervals(q);
 let bounded=q.origin.into_iter().chain(q.ray).chain(q.center).all(|v|v.is_finite()&&v.abs()<=1e6)
  &&q.extent.into_iter().all(|v|v.is_finite()&&(0.0..=1e6).contains(&v))&&q.limit.is_finite()
  &&a.into_iter().chain(b).all(f32::is_finite);
 let lower=lo[1].max(lo[2]);let upper=hi[1].min(hi[2]);
 bounded&&(upper<lower||lower>=q.limit||upper<=1e-7)
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn strict_far_ties_and_entry_epsilon_are_safe(){
  let mut q=Query{origin:[0.,0.,3.],ray:[0.,0.,-1.],center:[0.,0.,0.015],extent:[0.001,1.,0.0005],limit:1e20};
  assert!(original_hit(q));assert!(!certified(q));
  let(_,_,lo,_)=intervals(q);q.limit=lo[2];assert!(!original_hit(q));assert!(certified(q));
  q.limit=f32::from_bits(q.limit.to_bits()+1);assert!(original_hit(q));assert!(!certified(q));
  q.origin=[0.,0.,0.015];q.ray=[0.,0.,1.];assert!(!certified(q)); // interior origin not guessed as a miss
  q.origin=[0.,0.,1.];assert!(certified(q));assert!(!original_hit(q));
 }
 #[test]fn unproved_actual_marker_hit_always_rejects_and_nan_rejects(){
  for z in [0.015,0.020]{let mut q=Query{origin:[0.,0.,3.],ray:[0.,0.,-1.],center:[0.,0.,z],extent:[0.001,0.01,0.001],limit:1e20};
   assert!(original_hit(q));assert!(!certified(q));q.ray[0]=f32::NAN;assert!(!certified(q));q.ray[0]=0.;q.origin[0]=1e7;assert!(!certified(q));}
 }
 #[test]fn clamped_inverse_must_not_be_replaced_by_physical_parallel_ray_logic(){
  let q=Query{origin:[0.,0.,0.],ray:[0.,0.,0.],center:[0.,0.,0.015],extent:[1.,1.,0.0005],limit:1e20};
  assert!(original_hit(q));assert!(!certified(q));
 }
 #[test]fn randomized_marker_queries_preserve_original_hit_for_every_tested_x(){
  let mut seed=0x51dd_22aau32;let mut next=||{seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);seed};
  let mut certified_count=0;let mut queries=0;
  for n in 0..80_000{
   let mut value=||((next()>>8)as f32/16777216.0)*8.0-4.0;
   let q=Query{origin:[value(),value(),value()],ray:[value(),value(),if n%7==0{1e-10}else{value()}],
    center:[value(),value(),if n%2==0{0.015}else{0.020}],extent:[0.001,(value().abs()*0.2).max(0.001),if n%2==0{0.0005}else{0.001}],limit:if n%3==0{value().abs()}else{1e20}};
   if certified(q){certified_count+=1;for x in [-1e6,-1000.,-4.,-0.1,-0.,0.1,4.,1000.,1e6]{let candidate=Query{center:[x,q.center[1],q.center[2]],..q};assert!(!original_hit(candidate),"{candidate:?}");queries+=1;}}
  }
  assert!(certified_count>50_000);println!("{certified_count} certified queries, {queries} alternate-X checks");
 }
 #[test]fn finite_interval_implication_covers_exact_touch_and_negative_zero(){
  let values=[-1e20,-1.,-1e-7,-0.,0.,1e-7,f32::from_bits(1e-7f32.to_bits()+1),1.,1e20];
  let mut checked=0;
  for ly in values{for hy in values{if hy<ly{continue;}for lz in values{for hz in values{if hz<lz{continue;}for limit in values{
   let l=ly.max(lz);let h=hy.min(hz);if !(h<l||l>=limit||h<=1e-7){continue;}
   for lx in values{for hx in values{if hx<lx{continue;}let near=lx.max(ly).max(lz);let far=hx.min(hy).min(hz);assert!(!(near>1e-7&&far>=near&&near<limit));checked+=1;}}
  }}}}}
  assert!(checked>100_000);println!("{checked} finite interval implications");
 }
}
