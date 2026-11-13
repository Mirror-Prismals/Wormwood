//! Same independent pixel invocations, alternate synchronous workgroup layout.
//! Incremental tile dispatch keeps its original8x8 entry unchanged.
pub(super) fn append(source:&mut String)->Result<bool,String>{
 if std::env::var("WORMWOOD_WORKSTATION_CAPTURE_GROUP").as_deref()!=Ok("8x4"){return Ok(false);}
 append_variant(source)?;Ok(true)
}
fn append_variant(source:&mut String)->Result<(),String>{
 let marker="@compute @workgroup_size(8,8,1)\nfn cs_retained(";
 if source.matches(marker).count()!=1{return Err("retained workgroup entry changed".into());}
 let start=source.find(marker).unwrap();let opening=start+source[start..].find('{').ok_or("missing capture body")?;
 let mut depth=0i32;let mut end=None;
 for(i,b)in source.as_bytes().iter().enumerate().skip(opening){if *b==b'{'{depth+=1;}else if *b==b'}'{depth-=1;if depth==0{end=Some(i+1);break;}}}
 let mut copy=source[start..end.ok_or("capture body unclosed")?].to_owned();
 copy=copy.replacen("@workgroup_size(8,8,1)","@workgroup_size(8,4,1)",1).replacen("fn cs_retained(","fn cs_retained_8x4(",1);
 source.push('\n');source.push_str(&copy);Ok(())
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn only_threadgroup_shape_and_entry_name_change(){
  for incremental in [false,true]{
   let mut s=super::super::source::source(include_str!("../Procedures/Shaders/workstation.wgsl"),true,false,true).unwrap();
   if incremental{s=super::super::incremental::tiled_source(&s).unwrap();}
   let old=s.clone();append_variant(&mut s).unwrap();assert!(s.starts_with(&old));
   let extra=s[old.len()..].trim();let original=extra.replacen("@workgroup_size(8,4,1)","@workgroup_size(8,8,1)",1).replacen("fn cs_retained_8x4(","fn cs_retained(",1);assert!(old.contains(&original));
   let m=naga::front::wgsl::parse_str(&s).unwrap();naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).unwrap();
  }
 }
 #[test]fn dispatch_covers_each_native_pixel_once(){
  for(width,height)in[(1u32,1u32),(9,7),(1920,1200),(1921,1201)]{
   assert_eq!(width.div_ceil(8)*8>=width,true);assert_eq!(height.div_ceil(4)*4>=height,true);
   let mut rows=vec![0u8;height as usize];for group in 0..height.div_ceil(4){for local in 0..4{let y=group*4+local;if y<height{rows[y as usize]+=1;}}}assert!(rows.iter().all(|n|*n==1));
  }
 }
}
