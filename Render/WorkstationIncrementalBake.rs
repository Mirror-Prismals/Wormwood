//! A single isolated, GPU-resident bake generation. The caller drains the prior
//! frame before completion/cancellation/publication. No live atlas is written.
use super::*;
#[path="WorkstationBakeTiles.rs"] pub(super) mod tiles;
use tiles::{Entry,Phase,Plan};
pub(super) fn tiled_source(source:&str)->Result<String,String>{
 let mut out=source.to_owned();
 out.push_str("\n@group(0) @binding(7) var<uniform> incrementalTile:vec4<u32>;\n");
 for name in ["cs_ruby_response","cs_ruby_response_general","cs_ruby_resolve","cs_retained"]{
  let old=format!("fn {name}(@builtin(global_invocation_id) id:vec3<u32>) {{");
  if out.matches(&old).count()!=1{return Err(format!("incremental entry changed: {name}"));}
  out=out.replacen(&old,&format!("fn {name}(@builtin(global_invocation_id) rawId:vec3<u32>) {{\n let id=rawId+incrementalTile.xyz;"),1);
 }
 let m=naga::front::wgsl::parse_str(&out).map_err(|e|e.emit_to_string(&out))?;
 naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).map_err(|e|format!("incremental shader validation: {e:?}"))?;Ok(out)
}
pub(super) struct Generation{
 api:&'static Api,
 pub id:u64,pub source_rows:Vec<[f32;4]>,pub rows:[[f32;4];META_ROWS],pub packing:Option<atlas::Layout>,pub extent:[u32;2],
 pub plan:Plan,pub cancelled:bool,pub started:Instant,pub requested_bytes:u64,
 snapshots:Vec<(WGPUBuffer,WGPUBuffer,u64)>,metadata:WGPUBuffer,tile:WGPUBuffer,group:WGPUBindGroup,
 pub texture:WGPUTexture,pub view:WGPUTextureView,pub future_compute:WGPUBindGroup,pub future_render:WGPUBindGroup,
 pub encoded:bool,
}
impl Generation{
 pub unsafe fn new(p:&Pipeline,id:u64,source_rows:Vec<[f32;4]>,rows:[[f32;4];META_ROWS],packing:atlas::Layout,entries:&[Entry])->Result<Self,String>{
  let sizes:Vec<_>=(0..p.capture_groups as usize).map(|g|[rows[2+g*4][2] as u32,rows[2+g*4][3] as u32]).collect();
  // Full-view canonical glass is expensive: use small 2D jobs for large
  // visible panel captures, rather than an entire viewport-width stripe.
  let bounded=if p.detail_buffer.is_null(){0}else{(1..=4).fold(0,|mask,g| {
   if source_rows[32+40*(g-1)][0]>0. && sizes[g][0]*sizes[g][1]>200_000 {mask|(1<<g)}else{mask}
  })};
  let plan=Plan::with_bounded_capture(entries,&sizes,64,bounded).map_err(str::to_owned)?;
  let extent=packing.extent;let a=p.api;let mut g=Self{api:a,id,source_rows,rows,packing:Some(packing),extent,plan,cancelled:false,started:Instant::now(),requested_bytes:extent[0] as u64*extent[1] as u64*32,snapshots:vec![],metadata:ptr::null_mut(),tile:ptr::null_mut(),group:ptr::null_mut(),texture:ptr::null_mut(),view:ptr::null_mut(),future_compute:ptr::null_mut(),future_render:ptr::null_mut(),encoded:false};
  let mut cs=p.compute_entries.clone();
  for binding in 0..4{
   let entry=cs.iter_mut().find(|e|e.binding==binding).ok_or("missing bake source buffer")?;
   let buffer=(a.wgpuDeviceCreateBuffer)(p.device,&WGPUBufferDescriptor{label:view("Immutable bake input generation"),size:entry.size,usage:WGPUBufferUsage_Storage|WGPUBufferUsage_CopyDst,..Default::default()});
   if buffer.is_null(){return Err("bake snapshot allocation failed".into());}
   g.requested_bytes+=entry.size;g.snapshots.push((entry.buffer,buffer,entry.size));entry.buffer=buffer;
  }
  g.metadata=(a.wgpuDeviceCreateBuffer)(p.device,&WGPUBufferDescriptor{label:view("Immutable bake capture metadata"),size:p.metadata_bytes,usage:WGPUBufferUsage_Storage|WGPUBufferUsage_CopyDst,..Default::default()});
  g.tile=(a.wgpuDeviceCreateBuffer)(p.device,&WGPUBufferDescriptor{label:view("Bounded bake dispatch offset"),size:16,usage:WGPUBufferUsage_Uniform|WGPUBufferUsage_CopyDst,..Default::default()});
  g.requested_bytes+=p.metadata_bytes+16;
  // Optional plane-table tail is zero-initialized in this immutable generation;
  // its guarded shader uses the original function rather than live scratch.
  g.texture=(a.wgpuDeviceCreateTexture)(p.device,&WGPUTextureDescriptor{label:view("Unpublished original-radiance generation"),usage:WGPUTextureUsage_StorageBinding|WGPUTextureUsage_TextureBinding|WGPUTextureUsage_CopySrc|WGPUTextureUsage_CopyDst|super::atlas_export::copy_usage(),dimension:WGPUTextureDimension_2D,size:WGPUExtent3D{width:extent[0],height:extent[1],depthOrArrayLayers:2},format:WGPUTextureFormat_RGBA32Float,mipLevelCount:1,sampleCount:1,..Default::default()});
  if g.metadata.is_null()||g.tile.is_null()||g.texture.is_null(){return Err("bake generation allocation failed".into());}
  g.view=(a.wgpuTextureCreateView)(g.texture,&WGPUTextureViewDescriptor{dimension:WGPUTextureViewDimension_2DArray,arrayLayerCount:2,mipLevelCount:1,aspect:WGPUTextureAspect_All,..Default::default()});
  cs.iter_mut().find(|e|e.binding==4).unwrap().textureView=g.view;
  cs.iter_mut().find(|e|e.binding==6).unwrap().buffer=g.metadata;
  cs.iter_mut().find(|e|e.binding==7).unwrap().buffer=g.tile;
  g.group=(a.wgpuDeviceCreateBindGroup)(p.device,&WGPUBindGroupDescriptor{layout:p.compute_layout,entryCount:cs.len(),entries:cs.as_ptr(),..Default::default()});
  let mut future_cs=p.compute_entries.clone();future_cs.iter_mut().find(|e|e.binding==4).unwrap().textureView=g.view;
  let mut future_rs=p.render_entries.clone();future_rs.iter_mut().find(|e|e.binding==5).unwrap().textureView=g.view;
  if let Some(old)=future_rs.iter_mut().find(|e|e.binding==16){old.textureView=p.texture_view;}
  g.future_compute=(a.wgpuDeviceCreateBindGroup)(p.device,&WGPUBindGroupDescriptor{layout:p.compute_layout,entryCount:future_cs.len(),entries:future_cs.as_ptr(),..Default::default()});
  g.future_render=(a.wgpuDeviceCreateBindGroup)(p.device,&WGPUBindGroupDescriptor{layout:p.layout,entryCount:future_rs.len(),entries:future_rs.as_ptr(),..Default::default()});
  if g.view.is_null()||g.group.is_null()||g.future_compute.is_null()||g.future_render.is_null(){return Err("bake generation bindings failed".into());}check_errors()?;Ok(g)
 }
 pub unsafe fn encode(&mut self,encoder:WGPUCommandEncoder,queue:WGPUQueue,pipelines:[WGPUComputePipeline;4])->Result<bool,String>{
  if self.cancelled{return Ok(false);}let Some(tile)=self.plan.next()else{return Ok(false);};let a=self.api;
  if tile.phase==Phase::Snapshot{
   for &(source,target,size) in &self.snapshots{super::super::copy_bake_buffer(a,encoder,source,target,size);}
   (a.wgpuQueueWriteBuffer)(queue,self.metadata,0,self.rows.as_ptr().cast(),META_ROWS*16);
  }else{
   let offset=[tile.origin[0],tile.origin[1],tile.origin[2],0];(a.wgpuQueueWriteBuffer)(queue,self.tile,0,offset.as_ptr().cast(),16);
   let index=match tile.phase{Phase::Panel=>0,Phase::General=>1,Phase::Resolve=>2,Phase::Capture=>3,_=>unreachable!()};
   let pass=(a.wgpuCommandEncoderBeginComputePass)(encoder,&WGPUComputePassDescriptor{label:view("Bounded unpublished gem bake tile"),..Default::default()});
   if pass.is_null(){return Err("bake tile compute pass failed".into());}
   (a.wgpuComputePassEncoderSetPipeline)(pass,pipelines[index]);(a.wgpuComputePassEncoderSetBindGroup)(pass,0,self.group,0,ptr::null());
   (a.wgpuComputePassEncoderDispatchWorkgroups)(pass,tile.workgroups[0],tile.workgroups[1],tile.workgroups[2]);
   (a.wgpuComputePassEncoderEnd)(pass);(a.wgpuComputePassEncoderRelease)(pass);
  }
  self.encoded=true;Ok(true)
 }
 pub fn submitted(&mut self,serial:u64)->Result<(),String>{if self.encoded{self.plan.submitted(serial).map_err(str::to_owned)?;self.encoded=false;}Ok(())}
 pub fn completed(&mut self,serial:u64){self.plan.completed(serial);}
 pub fn cancel(&mut self){self.cancelled=true;self.plan.cancel();}
 pub fn info(&self)->Value{json!({"id":self.id,"completed_tiles":self.plan.progress(),"total_tiles":self.plan.total(),"next_phase":self.plan.next().map(|t|format!("{:?}",t.phase)),"pending_phase":self.plan.pending_tile().map(|t|format!("{:?}",t.phase)),"cancelled":self.cancelled,"gpu_release_safe":self.plan.can_release(),"publishable":self.plan.publishable(),"requested_bytes":self.requested_bytes,"elapsed_ms":self.started.elapsed().as_secs_f64()*1000.})}
}
impl Drop for Generation{fn drop(&mut self){unsafe{let a=self.api;for g in [self.group,self.future_compute,self.future_render]{if !g.is_null(){(a.wgpuBindGroupRelease)(g);}}for &(_,b,_) in &self.snapshots{(a.wgpuBufferRelease)(b);}for b in [self.metadata,self.tile]{if !b.is_null(){(a.wgpuBufferRelease)(b);}}if !self.view.is_null(){(a.wgpuTextureViewRelease)(self.view);}if !self.texture.is_null(){(a.wgpuTextureRelease)(self.texture);}}}}

#[cfg(test)]mod incremental_tests{
 use super::*;
 #[test]fn tiled_original_shader_validates_and_preserves_sample_formula(){
  let base=source::source(include_str!("../Procedures/Shaders/workstation.wgsl"),true,false,true).unwrap();
  let tiled=tiled_source(&base).unwrap();assert_eq!(tiled.matches("let id=rawId+incrementalTile.xyz;").count(),4);
  assert!(tiled.contains("let origin=retainedRay(vec2<f32>(id.xy)+rect.xy,sample);"));
  assert!(tiled.contains("gemDisplay(total*0.5)"));
 }
 #[test]fn generation_identity_ignores_only_existing_live_playhead_and_clock(){
  let a=vec![[0.;4];256];let mut b=a.clone();b[252]=[1.,2.,3.,4.];b[245][3]=0.5;
  assert!(!Pipeline::changed(&a,&b,true));
  for (r,c) in [(5,0),(34,0),(35,0),(245,0),(250,3),(253,0)]{let mut x=b.clone();x[r][c]+=1.;assert!(Pipeline::changed(&a,&x,true));}
 }
}

#[cfg(test)]mod transport_pose_abi_tests {
 use super::*;
 #[test]fn transport_capture_metadata_does_not_overlap_groups_or_bounds(){
  let mut frame=vec![[0.;4];256];frame[1][2]=3.;frame[2][3]=1.;frame[3][3]=1.;frame[8]=[1200.,780.,0.,0.];
  for i in 0..4{frame[192+2*i]=[i as f32*0.1,0.,0.02-i as f32*0.001,1.];}
  let mut rows=[[7.;4];META_ROWS];Pipeline::button_bounds(&frame,&mut rows);
  assert!(rows[..49].iter().all(|r|*r==[7.;4]));
  for i in 0..4{assert_eq!(rows[53+i],frame[192+2*i]);assert!(rows[49+i][2]>0.);}
  assert_eq!(rows[57],[3.,0.,0.,0.]);assert!(rows[58..].iter().all(|r|*r==[7.;4]));
 }
 #[test]fn bounds_and_unclassified_geometry_still_invalidate_capture(){
  let a=vec![[0.;4];256];
  for(row,col)in[(13,0),(14,0),(192,0),(192,2),(193,0),(193,3)]{let mut b=a.clone();b[row][col]=0.1;assert!(Pipeline::changed(&a,&b,true));}
 }
}
