//! Explicit finite research export. No default readback or codec work.
use super::*;
use std::io::Write;
const TOTAL_CAP:u64=384*1024*1024;
const CHUNK_CAP:u64=16*1024*1024;
pub(super) fn copy_usage()->WGPUTextureUsage {
 if std::env::var("WORMWOOD_WORKSTATION_EXPORT_ATLAS").as_deref()==Ok("1"){WGPUTextureUsage_CopySrc}else{0}
}
pub(super) struct Export {bytes:u64,count:u32}
impl Export {
 pub(super) fn selected()->Option<Self>{(copy_usage()!=0).then_some(Self{bytes:0,count:0})}
 pub(super) unsafe fn write(&mut self,p:&Pipeline,instance:WGPUInstance,queue:WGPUQueue,directory:&std::path::Path)->Result<(),String>{
  if ![1,32,64].contains(&p.bakes)||self.count>=3{return Ok(());}
  let [width,height]=p.atlas_extent;
  let Some((size,stride,chunk_rows))=plan(width,height,self.bytes) else{return Ok(());};
  let raw=directory.join(format!("retained-bake-{:05}-sample0.rgba32f",p.bakes));
  let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&raw).map_err(|e|e.to_string())?;
  let a=p.api;let capacity=u64::from(stride)*u64::from(chunk_rows);
  let buffer=(a.wgpuDeviceCreateBuffer)(p.device,&WGPUBufferDescriptor{label:view("Bounded optical state export"),size:capacity,usage:WGPUBufferUsage_CopyDst|WGPUBufferUsage_MapRead,..Default::default()});
  if buffer.is_null(){return Err("atlas export readback allocation failed".into());}
  struct Guard(&'static Api,WGPUBuffer);
  impl Drop for Guard {fn drop(&mut self){unsafe{(self.0.wgpuBufferRelease)(self.1)}}}
  let _guard=Guard(a,buffer);let start=Instant::now();
  for y in (0..height).step_by(chunk_rows as usize){
   let rows=chunk_rows.min(height-y);let bytes=stride as usize*rows as usize;
   let encoder=(a.wgpuDeviceCreateCommandEncoder)(p.device,ptr::null());
   if encoder.is_null(){return Err("atlas export encoder failed".into());}
   let source=WGPUTexelCopyTextureInfo{texture:p.texture,origin:WGPUOrigin3D{x:0,y,z:0},aspect:WGPUTextureAspect_All,..Default::default()};
   let destination=WGPUTexelCopyBufferInfo{buffer,layout:WGPUTexelCopyBufferLayout{bytesPerRow:stride,rowsPerImage:rows,..Default::default()}};
   let extent=WGPUExtent3D{width,height:rows,depthOrArrayLayers:1};
   (a.wgpuCommandEncoderCopyTextureToBuffer)(encoder,&source,&destination,&extent);
   let command=(a.wgpuCommandEncoderFinish)(encoder,ptr::null());(a.wgpuCommandEncoderRelease)(encoder);
   if command.is_null(){return Err("atlas export command failed".into());}
   (a.wgpuQueueSubmit)(queue,1,&command);(a.wgpuCommandBufferRelease)(command);
   let mut state=Box::new(Wait::default());let callback=WGPUBufferMapCallbackInfo{mode:WGPUCallbackMode_AllowProcessEvents,callback:Some(mapped),userdata1:(&mut *state as *mut Wait).cast(),..Default::default()};
   (a.wgpuBufferMapAsync)(buffer,WGPUMapMode_Read,0,bytes,callback);wait(a,instance,p.device,state)?;
   let mapped=(a.wgpuBufferGetConstMappedRange)(buffer,0,bytes);
   if mapped.is_null(){return Err("atlas export mapped range failed".into());}
   let data=std::slice::from_raw_parts(mapped.cast::<u8>(),bytes);
   let result=data.chunks_exact(stride as usize).try_for_each(|row|file.write_all(&row[..width as usize*16]));
   (a.wgpuBufferUnmap)(buffer);result.map_err(|e|e.to_string())?;check_errors()?;
  }
  file.flush().map_err(|e|e.to_string())?;self.bytes+=size;self.count+=1;
  let meta=json!({"sample":0,"format":"RGBA32Float little-endian; original linear RGB and depth","extent":[width,height],"bytes":size,"total_export_bytes":self.bytes,"metadata_rows":p.rows.to_vec(),"capture_groups":p.capture_groups,"capture_packet":p.captured,"atlas_layers":p.atlas_layers,"bake":p.bakes,"readback_write_ms":start.elapsed().as_secs_f64()*1000.,"scope":"Diagnostic lighting/coverage data for lossless compression measurement. One original sample plane only, not an animation cache or application FPS run. No quantization/interpolation; original shader result bytes."});
  std::fs::write(raw.with_extension("json"),serde_json::to_vec(&meta).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
  Ok(())
 }
}
fn plan(width:u32,height:u32,previous:u64)->Option<(u64,u32,u32)>{
 if width==0||height==0{return None;}
 let row=u64::from(width).checked_mul(16)?;let size=row.checked_mul(u64::from(height))?;
 if previous.checked_add(size)?>TOTAL_CAP{return None;}
 let stride=row.checked_add(255)?/256*256;if stride>CHUNK_CAP{return None;}
 let rows=(CHUNK_CAP/stride).min(u64::from(height));
 Some((size,u32::try_from(stride).ok()?,u32::try_from(rows).ok()?))
}
#[cfg(test)] mod tests {
 use super::*;
 #[test]fn bounded_chunks_preserve_tight_rows_and_cap_total_output(){for(w,h)in[(1920,1200),(3979,2500),(1,1),(8192,8192)]{if let Some((size,stride,rows))=plan(w,h,0){assert_eq!(size,u64::from(w)*u64::from(h)*16);assert_eq!(stride%256,0);assert!(u64::from(stride)*u64::from(rows)<=CHUNK_CAP);assert!(size<=TOTAL_CAP);assert!(rows>0);}}}
 #[test]fn invalid_and_over_budget_sizes_do_not_allocate(){for(w,h,prior)in[(0,1,0),(1,0,0),(u32::MAX,u32::MAX,0),(1920,1200,TOTAL_CAP),(1,1,u64::MAX)]{assert!(plan(w,h,prior).is_none());}}
}
