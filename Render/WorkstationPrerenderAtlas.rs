//! Exact integer packing of the existing conservative capture rectangles.
#[derive(Clone,Debug,PartialEq,Eq)]
pub(super) struct Layout {pub extent:[u32;2],pub origins:Vec<[u32;2]>,pub used_texels:u64}
pub(super) fn pack(sizes:&[[u32;2]],limit:u32)->Option<Layout>{
 if sizes.is_empty()||sizes.len()>6||sizes.iter().any(|s|s[0]==0||s[1]==0||s[0]>limit||s[1]>limit){return None;}
 let mut widths=Vec::new();
 for mask in 1..(1usize<<sizes.len()) {let sum:u64=sizes.iter().enumerate().filter(|(i,_)|mask&(1<<i)!=0).map(|(_,s)|u64::from(s[0])).sum();if sum<=u64::from(limit){widths.push(sum as u32);}}
 widths.sort_unstable();widths.dedup();
 let used_texels=sizes.iter().map(|s|u64::from(s[0])*u64::from(s[1])).sum();
 let mut order:Vec<_>=(0..sizes.len()).collect();let mut best:Option<Layout>=None;
 fn visit(order:&mut [usize],at:usize,sizes:&[[u32;2]],widths:&[u32],limit:u32,used:u64,best:&mut Option<Layout>){
  if at<order.len(){for i in at..order.len(){order.swap(at,i);visit(order,at+1,sizes,widths,limit,used,best);order.swap(at,i);}return;}
  for &width in widths {
   let(mut x,mut y,mut row,mut max_x)=(0u32,0u32,0u32,0u32);let mut origins=vec![[0;2];sizes.len()];let mut valid=true;
   for &g in order.iter(){let [w,h]=sizes[g];if w>width{valid=false;break;}
    if u64::from(x)+u64::from(w)>u64::from(width){y=match y.checked_add(row){Some(v)=>v,None=>{valid=false;break;}};x=0;row=0;}
    if u64::from(y)+u64::from(h)>u64::from(limit){valid=false;break;}
    origins[g]=[x,y];x+=w;row=row.max(h);max_x=max_x.max(x);
   }
   if !valid{continue;}
   let extent=[max_x,y+row];let area=u64::from(extent[0])*u64::from(extent[1]);
   if best.as_ref().is_none_or(|b|area<u64::from(b.extent[0])*u64::from(b.extent[1])){*best=Some(Layout{extent,origins,used_texels:used});}
  }
 }
 visit(&mut order,0,sizes,&widths,limit,used_texels,&mut best);best
}
/// Keep slot capacity while content shrinks. Only actual growth repacks the
/// atlas; capture rectangles and sample addresses within each slot stay exact.
#[derive(Default)]
pub(super) struct Reservation { sizes: Vec<[u32;2]>, layout: Option<Layout> }
impl Reservation {
 pub fn clear(&mut self){self.sizes.clear();self.layout=None;}
 pub fn reserve(&mut self,sizes:&[[u32;2]],limit:u32)->Option<Layout>{
  if sizes.is_empty()||sizes.iter().any(|s|s[0]==0||s[1]==0){return None;}
  let capacity:Vec<_>=sizes.iter().enumerate().map(|(i,s)|{
   let old=self.sizes.get(i).copied().unwrap_or([0;2]);[s[0].max(old[0]),s[1].max(old[1])]
  }).collect();
  if capacity!=self.sizes||self.layout.as_ref().is_none_or(|l|l.extent.iter().any(|v|*v>limit)) {
   // Do not accumulate a large history of expanded panels. Reservation is
   // bounded to 256MiB; larger required views use their current exact packing.
   let grown=pack(&capacity,limit).filter(|l|u64::from(l.extent[0])*u64::from(l.extent[1])*32<=256*1024*1024);
   let (planned,reserved)=match grown{Some(l)=>(l,capacity),None=>(pack(sizes,limit)?,sizes.to_vec())};
   self.sizes=reserved;
   self.layout=Some(planned);
  }
  let mut layout=self.layout.clone()?;
  layout.used_texels=sizes.iter().map(|s|u64::from(s[0])*u64::from(s[1])).sum();Some(layout)
 }
}
#[cfg(test)] mod compact_atlas_tests {
 use super::*;
 fn verify(sizes:&[[u32;2]],layout:&Layout,limit:u32){
  assert!(layout.extent[0]<=limit&&layout.extent[1]<=limit);
  for (a,size) in sizes.iter().enumerate(){let p=layout.origins[a];assert!(p[0]+size[0]<=layout.extent[0]&&p[1]+size[1]<=layout.extent[1]);
   for b in 0..a {let q=layout.origins[b];let other=sizes[b];assert!(p[0]+size[0]<=q[0]||q[0]+other[0]<=p[0]||p[1]+size[1]<=q[1]||q[1]+other[1]<=p[1]);}
  }
 }
 #[test] fn shrinking_and_regrowing_capture_rects_reuse_exact_slots(){
  let mut cache=Reservation::default();let large=[[960,600],[567,189],[567,189],[201,591],[201,591],[567,254]];
  let first=cache.reserve(&large,8192).unwrap();
  for n in 1..50 {let mut sizes=large;for s in &mut sizes[1..]{s[0]=(s[0]-n).max(1);s[1]=(s[1]-n).max(1);}
   let smaller=cache.reserve(&sizes,8192).unwrap();assert_eq!(smaller.origins,first.origins);assert_eq!(smaller.extent,first.extent);verify(&sizes,&smaller,8192);
  }
  assert_eq!(cache.reserve(&large,8192).unwrap(),first);
  let mut grown=large;grown[3]=[960,600];let next=cache.reserve(&grown,8192).unwrap();verify(&grown,&next,8192);
  cache.clear();assert_eq!(cache.reserve(&large,8192).unwrap(),first);
 }
 #[test] fn compact_atlas_preserves_captured_rectangles_with_substantial_saving(){let sizes=[[1920,1200],[1143,369],[1143,369],[393,1191],[393,1191]];let p=pack(&sizes,8192).unwrap();verify(&sizes,&p,8192);assert!(u64::from(p.extent[0])*u64::from(p.extent[1])*32<140*1024*1024);}
 #[test] fn compact_atlas_addresses_are_unique_for_both_samples_and_border_texels(){
  let sizes=[[53,37],[17,9],[17,9],[7,35],[7,35]];let p=pack(&sizes,128).unwrap();verify(&sizes,&p,128);let mut addresses=std::collections::HashSet::new();
  for sample in 0..2 {for (g,size) in sizes.iter().enumerate(){for y in 0..size[1]{for x in 0..size[0]{let [ox,oy]=p.origins[g];let address=(sample*p.extent[1]+oy+y)*p.extent[0]+ox+x;assert!(addresses.insert(address));}}}}
  assert_eq!(addresses.len() as u64,p.used_texels*2);
 }
 #[test] fn compact_atlas_resize_repacking_never_overlaps_and_is_deterministic(){
  for step in 0..32 {let sizes=[[191+step,127],[61+step,23],[53,37+step],[23,113],[17+step,101]];let a=pack(&sizes,1024).unwrap();let b=pack(&sizes,1024).unwrap();verify(&sizes,&a,1024);assert_eq!(a,b);}
 }
 #[test] fn compact_atlas_rejects_empty_oversize_and_unfittable_inputs(){assert!(pack(&[],128).is_none());assert!(pack(&[[0,4]],128).is_none());assert!(pack(&[[129,1]],128).is_none());assert!(pack(&[[128,128],[128,128]],128).is_none());assert!(pack(&[[u32::MAX,u32::MAX];6],128).is_none());}
}
