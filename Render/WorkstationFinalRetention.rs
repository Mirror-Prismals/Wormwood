//! Final display-color retention, exact playhead damage only. All other packet
//! or resource changes use the original full compositor. No CPU pixel uploads.
use super::*;
use super::final_damage as damage;
use damage::{Context,Damage,Rect,Tracker};
const COPY_SHADER:&str=r#"
@group(0) @binding(0) var image:texture_2d<f32>;
@vertex fn vs_final_copy(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32>{
 let p=array<vec2<f32>,3>(vec2<f32>(-1.0,-1.0),vec2<f32>(3.0,-1.0),vec2<f32>(-1.0,3.0));
 return vec4<f32>(p[i],0.0,1.0);
}
@fragment fn fs_final_copy(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32>{
 return textureLoad(image,vec2<i32>(floor(p.xy)),0);
}
"#;
pub(super) struct Plan {pub eligible:bool,pub damage:Damage,reason:&'static str,context:Context,dependencies:Vec<u64>}
impl Plan {
 pub fn partial(&self)->Option<Rect>{if let Damage::Rect(r)=self.damage{Some(r)}else{None}}
 pub fn copy_first(&self)->bool{self.eligible&&self.damage!=Damage::Full}
 pub fn original_draw(&self)->bool{!self.eligible||self.damage!=Damage::Unchanged}
 pub fn label(&self)->&'static str{if !self.eligible{"fallback"}else{match self.damage{Damage::Full=>"full",Damage::Rect(_)=>"playhead_damage",Damage::Unchanged=>"unchanged"}}}
}
struct Pending {serial:u64,rows:Vec<[f32;4]>,context:Context,dependencies:Vec<u64>}
pub(super) struct Retention {
 api:&'static Api,texture:WGPUTexture,view:WGPUTextureView,shader:WGPUShaderModule,
 layout:WGPUBindGroupLayout,pipeline_layout:WGPUPipelineLayout,group:WGPUBindGroup,pipeline:WGPURenderPipeline,
 latest:Vec<[f32;4]>,tracker:Tracker,committed_dependencies:Vec<u64>,pending:Option<Pending>,external_epoch:u64,
 width:u32,height:u32,format:WGPUTextureFormat,
 full:u64,partial:u64,unchanged:u64,fallback:u64,damage_pixels:u64,gpu_copy_bytes:u64,committed_serial:u64,last:Value,
 pub direct:bool, pub validate:bool, pub complete:bool,reference:WGPUTexture,reference_view:WGPUTextureView,verification:Vec<Value>,output:PathBuf,
}
impl Retention {
 pub unsafe fn new(c:&Client,validate:bool)->Result<Self,String>{Self::create(c,validate,false)}
 pub unsafe fn composition(c:&Client,validate:bool)->Result<Self,String>{Self::create(c,validate,true)}
 unsafe fn create(c:&Client,validate:bool,complete:bool)->Result<Self,String>{
  let direct=complete||std::env::var("WORMWOOD_WORKSTATION_FINAL_DIRECT").as_deref()==Ok("1");
  let a=c.api;let mut r=Self{api:a,texture:ptr::null_mut(),view:ptr::null_mut(),shader:ptr::null_mut(),layout:ptr::null_mut(),pipeline_layout:ptr::null_mut(),group:ptr::null_mut(),pipeline:ptr::null_mut(),latest:vec![],tracker:Tracker::default(),committed_dependencies:vec![],pending:None,external_epoch:0,width:c.width,height:c.height,format:c.format,full:0,partial:0,unchanged:0,fallback:0,damage_pixels:0,gpu_copy_bytes:0,committed_serial:0,last:Value::Null,direct,validate,complete,reference:ptr::null_mut(),reference_view:ptr::null_mut(),verification:vec![],output:c.capture.parent().unwrap().to_owned()};
  let descriptor=WGPUTextureDescriptor{label:view("Retained exact final display color"),usage:WGPUTextureUsage_TextureBinding|if direct{WGPUTextureUsage_RenderAttachment}else{WGPUTextureUsage_CopyDst},dimension:WGPUTextureDimension_2D,size:WGPUExtent3D{width:c.width,height:c.height,depthOrArrayLayers:1},format:c.format,mipLevelCount:1,sampleCount:1,..Default::default()};
  r.texture=(a.wgpuDeviceCreateTexture)(c.device,&descriptor);if r.texture.is_null(){return Err("Final retention allocation failed".into());}
  r.view=(a.wgpuTextureCreateView)(r.texture,ptr::null());
  let entry=WGPUBindGroupLayoutEntry{binding:0,visibility:WGPUShaderStage_Fragment,texture:WGPUTextureBindingLayout{sampleType:WGPUTextureSampleType_UnfilterableFloat,viewDimension:WGPUTextureViewDimension_2D,multisampled:0,..Default::default()},..Default::default()};
  r.layout=(a.wgpuDeviceCreateBindGroupLayout)(c.device,&WGPUBindGroupLayoutDescriptor{entryCount:1,entries:&entry,..Default::default()});
  r.pipeline_layout=(a.wgpuDeviceCreatePipelineLayout)(c.device,&WGPUPipelineLayoutDescriptor{bindGroupLayoutCount:1,bindGroupLayouts:&r.layout,..Default::default()});
  let entry=WGPUBindGroupEntry{binding:0,textureView:r.view,..Default::default()};
  r.group=(a.wgpuDeviceCreateBindGroup)(c.device,&WGPUBindGroupDescriptor{layout:r.layout,entryCount:1,entries:&entry,..Default::default()});
  let wgsl=WGPUShaderSourceWGSL{chain:WGPUChainedStruct{next:ptr::null(),sType:WGPUSType_ShaderSourceWGSL},code:view(COPY_SHADER)};
  r.shader=(a.wgpuDeviceCreateShaderModule)(c.device,&WGPUShaderModuleDescriptor{nextInChain:&wgsl.chain,label:view("Unfiltered original final color copy")});
  let target=WGPUColorTargetState{format:c.format,writeMask:WGPUColorWriteMask_All,..Default::default()};
  let fs=WGPUFragmentState{module:r.shader,entryPoint:view("fs_final_copy"),targetCount:1,targets:&target,..Default::default()};
  // The copy must not consume depth. The original scissored draw subsequently
  // sees the same cleared depth and full viewport as the complete compositor.
  let depth=WGPUDepthStencilState{format:WGPUTextureFormat_Depth32Float,depthWriteEnabled:WGPUOptionalBool_False,depthCompare:WGPUCompareFunction_Always,..Default::default()};
  r.pipeline=(a.wgpuDeviceCreateRenderPipeline)(c.device,&WGPURenderPipelineDescriptor{layout:r.pipeline_layout,vertex:WGPUVertexState{module:r.shader,entryPoint:view("vs_final_copy"),..Default::default()},primitive:WGPUPrimitiveState{topology:WGPUPrimitiveTopology_TriangleList,frontFace:WGPUFrontFace_CCW,cullMode:WGPUCullMode_None,..Default::default()},multisample:WGPUMultisampleState{count:1,mask:u32::MAX,..Default::default()},fragment:&fs,depthStencil:&depth,..Default::default()});
  if validate {
   let mut d=descriptor;d.label=view("Validation original full compositor");d.usage=WGPUTextureUsage_RenderAttachment|WGPUTextureUsage_CopySrc;
   r.reference=(a.wgpuDeviceCreateTexture)(c.device,&d);r.reference_view=(a.wgpuTextureCreateView)(r.reference,ptr::null());
  }
  if r.view.is_null()||r.group.is_null()||r.pipeline.is_null(){return Err("Final retention pipeline allocation failed".into());}
  check_errors()?;Ok(r)
 }
 pub fn observe(&mut self,rows:&[[f32;4]]){self.latest.clear();self.latest.extend_from_slice(rows);}
 pub fn invalidate(&mut self){self.tracker.invalidate();self.pending=None;self.committed_dependencies.clear();self.external_epoch=self.external_epoch.wrapping_add(1);}
 pub fn completed(&mut self,serial:u64){if self.pending.as_ref().is_some_and(|p|p.serial<=serial){let p=self.pending.take().unwrap();if self.complete{self.tracker.completed_complete(&p.rows,p.context);}else{self.tracker.completed(&p.rows,p.context);}self.committed_dependencies=p.dependencies;self.committed_serial=p.serial;}}
 pub fn plan(&mut self,dependencies:Vec<u64>,reusable:bool)->Plan{
  let mut context=Context{width:self.width,height:self.height,format:self.format,packet_rows:super::super::workstation::FRAME_ROWS,resource_epoch:self.external_epoch,reusable};
  context.reusable&=(if self.complete{damage::supported_complete(&self.latest,context)}else{damage::supported(&self.latest,context)})&&self.pending.is_none();
  if self.committed_dependencies!=dependencies{self.tracker.invalidate();}
  let damage=if self.complete{self.tracker.plan_complete(&self.latest,context)}else{self.tracker.plan(&self.latest,context)};
  let reason=if !reusable{"pipeline_or_resource_state"}else if self.pending.is_some(){"uncompleted_generation"}else if !context.reusable{"unsupported_packet_camera_or_style"}else if self.committed_dependencies.is_empty(){"no_completed_image"}else if self.committed_dependencies!=dependencies{"resource_metadata_or_generation"}else{match damage{Damage::Full=>"non_playhead_packet_change",Damage::Rect(_)=>"playhead_only",Damage::Unchanged=>"identical_pixels"}};
  Plan{eligible:context.reusable,damage,reason,context,dependencies}
 }
 pub unsafe fn copy_first(&self,pass:WGPURenderPassEncoder){let a=self.api;(a.wgpuRenderPassEncoderSetPipeline)(pass,self.pipeline);(a.wgpuRenderPassEncoderSetBindGroup)(pass,0,self.group,0,ptr::null());(a.wgpuRenderPassEncoderDraw)(pass,3,1,0,0);}
 pub fn target_view(&self)->WGPUTextureView{self.view}
 pub unsafe fn present_direct(&self,encoder:WGPUCommandEncoder,target:WGPUTextureView,depth_view:WGPUTextureView,timestamps:Option<&WGPURenderPassTimestampWrites>)->Result<(),String>{
  // Separate read-only presentation pass after retained attachment writes end.
  // The first pass writes query 6; this pass writes query 7, so the complete
  // dependency interval includes both draws. UINT32_MAX is the installed API's
  // WGPU_QUERY_SET_INDEX_UNDEFINED sentinel (webgpu.h).
  let color=WGPURenderPassColorAttachment{view:target,depthSlice:u32::MAX,loadOp:WGPULoadOp_Clear,storeOp:WGPUStoreOp_Store,clearValue:WGPUColor{r:0.,g:0.,b:0.,a:1.},..Default::default()};
  let depth=WGPURenderPassDepthStencilAttachment{view:depth_view,depthLoadOp:WGPULoadOp_Clear,depthStoreOp:WGPUStoreOp_Store,depthClearValue:1.,..Default::default()};
  let a=self.api;let pass=(a.wgpuCommandEncoderBeginRenderPass)(encoder,&WGPURenderPassDescriptor{colorAttachmentCount:1,colorAttachments:&color,depthStencilAttachment:&depth,timestampWrites:timestamps.map_or(ptr::null(),|t|t),..Default::default()});
  if pass.is_null(){return Err("Direct retained presentation pass failed".into());}
  self.copy_first(pass);(a.wgpuRenderPassEncoderEnd)(pass);(a.wgpuRenderPassEncoderRelease)(pass);Ok(())
 }
 pub unsafe fn update_image(&self,encoder:WGPUCommandEncoder,source:WGPUTexture,plan:&Plan){
  if self.direct||!plan.eligible{return;}let rect=match plan.damage{Damage::Full=>Rect{x:0,y:0,width:self.width,height:self.height},Damage::Rect(r)=>r,Damage::Unchanged=>return};
  copy_final_region(self.api,encoder,source,self.texture,rect.x,rect.y,rect.width,rect.height);
 }
 pub fn submitted(&mut self,serial:u64,plan:Plan){
  let area=u64::from(self.width)*u64::from(self.height);let pixels=if plan.eligible{match plan.damage{Damage::Full=>area,Damage::Rect(r)=>r.pixels(),Damage::Unchanged=>0}}else{area};
  self.last=json!({"mode":plan.label(),"reason":plan.reason,"eligible":plan.eligible,"damage_pixels":pixels,"coverage_fraction":pixels as f64/area as f64,"rect":plan.partial().map(|r|[r.x,r.y,r.width,r.height]),"submission":serial});
  if !plan.eligible{self.fallback+=1;self.damage_pixels+=pixels;self.invalidate();return;}
  match plan.damage{Damage::Full=>self.full+=1,Damage::Rect(_)=>self.partial+=1,Damage::Unchanged=>self.unchanged+=1};
  self.damage_pixels+=pixels;if !self.direct{self.gpu_copy_bytes+=pixels*4;}
  self.pending=Some(Pending{serial,rows:self.latest.clone(),context:plan.context,dependencies:plan.dependencies});
 }
 pub fn info(&self)->Value{json!({"enabled":true,"direct_retained_target":self.direct,"all_gem_outputs":self.complete,"cache_transfer_commands":if self.direct{0}else{self.full+self.partial},"full_compositions":self.full,"playhead_damage_draws":self.partial,"unchanged_copies":self.unchanged,"fallback_draws":self.fallback,"original_fragment_pixels":self.damage_pixels,"cache_update_gpu_bytes":self.gpu_copy_bytes,"texture_bytes":u64::from(self.width)*u64::from(self.height)*4,"committed_submission":self.committed_serial,"pending_submission":self.pending.as_ref().map(|p|p.serial),"last":self.last,"validation_enabled":self.validate,"validated_frames":self.verification.len(),"scope":if self.direct{"Direct native-format retained target: original full draw or LoadOp::Load plus original scissored damage; one unfiltered presentation draw afterward. No surface-to-cache transfers. GPU interval spans first retained-pass start through final presentation-draw end, including both passes/dependencies. Completed native wall includes acquisition, encoding and completion. All generations commit after completion; no normal CPU readback."}else{"Exact final-color reuse only after completion; complete packet bit equality except row 252, exact metadata/geometry/resource identities and bake generation. One render pass copies previous color then redraws original playhead damage. GPU region copy updates cache afterward; GPU pass timestamps exclude that transfer, while completed-frame wall includes it. No normal per-frame CPU readback."}})}
 pub unsafe fn encode_reference(&self,encoder:WGPUCommandEncoder,depth_view:WGPUTextureView,pipeline:WGPURenderPipeline,group:WGPUBindGroup,count:u32,clear:[f32;4])->Result<(),String>{
  if !self.validate{return Ok(());}
  let color=WGPURenderPassColorAttachment{view:self.reference_view,depthSlice:u32::MAX,loadOp:WGPULoadOp_Clear,storeOp:WGPUStoreOp_Store,clearValue:WGPUColor{r:clear[0] as f64,g:clear[1] as f64,b:clear[2] as f64,a:1.},..Default::default()};
  let depth=WGPURenderPassDepthStencilAttachment{view:depth_view,depthLoadOp:WGPULoadOp_Clear,depthStoreOp:WGPUStoreOp_Store,depthClearValue:1.,..Default::default()};
  let a=self.api;let pass=(a.wgpuCommandEncoderBeginRenderPass)(encoder,&WGPURenderPassDescriptor{colorAttachmentCount:1,colorAttachments:&color,depthStencilAttachment:&depth,..Default::default()});
  if pass.is_null(){return Err("Final retention reference pass failed".into());}
  (a.wgpuRenderPassEncoderSetPipeline)(pass,pipeline);(a.wgpuRenderPassEncoderSetBindGroup)(pass,0,group,0,ptr::null());(a.wgpuRenderPassEncoderDraw)(pass,count,1,0,0);(a.wgpuRenderPassEncoderEnd)(pass);(a.wgpuRenderPassEncoderRelease)(pass);Ok(())
 }
}
impl Drop for Retention{fn drop(&mut self){unsafe{let a=self.api;
 if !self.group.is_null(){(a.wgpuBindGroupRelease)(self.group);}if !self.pipeline.is_null(){(a.wgpuRenderPipelineRelease)(self.pipeline);}if !self.pipeline_layout.is_null(){(a.wgpuPipelineLayoutRelease)(self.pipeline_layout);}if !self.layout.is_null(){(a.wgpuBindGroupLayoutRelease)(self.layout);}if !self.shader.is_null(){(a.wgpuShaderModuleRelease)(self.shader);}
 for v in [self.view,self.reference_view]{if !v.is_null(){(a.wgpuTextureViewRelease)(v);}}for t in [self.texture,self.reference]{if !t.is_null(){(a.wgpuTextureRelease)(t);}}
} let _=std::fs::write(self.output.join("final-retention-validation.json"),json!({"enabled":self.validate,"frames":self.verification,"final":self.info()}).to_string());}}
#[cfg(test)]mod final_retention_tests {
 use super::*;
 #[test]fn copy_preserves_display_values_without_lighting(){let m=naga::front::wgsl::parse_str(COPY_SHADER).unwrap();naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).unwrap();assert!(!COPY_SHADER.contains("gemDisplay"));assert_eq!(COPY_SHADER.matches("textureLoad").count(),1);}
}
impl Retention {
 // Validation only. No readback or extra reference pass exists in normal use.
 pub unsafe fn verify(&mut self,device:WGPUDevice,instance:WGPUInstance,queue:WGPUQueue,target:WGPUTexture,read:WGPUBuffer,stride:u32,serial:u64)->Result<(),String>{
  if !self.validate{return Ok(());}
  unsafe fn pixels(a:&Api,device:WGPUDevice,instance:WGPUInstance,queue:WGPUQueue,t:WGPUTexture,read:WGPUBuffer,w:u32,h:u32,stride:u32)->Result<Vec<u8>,String>{
   let e=(a.wgpuDeviceCreateCommandEncoder)(device,ptr::null());
   (a.wgpuCommandEncoderCopyTextureToBuffer)(e,&WGPUTexelCopyTextureInfo{texture:t,aspect:WGPUTextureAspect_All,..Default::default()},&WGPUTexelCopyBufferInfo{buffer:read,layout:WGPUTexelCopyBufferLayout{bytesPerRow:stride,rowsPerImage:h,..Default::default()}},&WGPUExtent3D{width:w,height:h,depthOrArrayLayers:1});
   let command=(a.wgpuCommandEncoderFinish)(e,ptr::null());(a.wgpuQueueSubmit)(queue,1,&command);(a.wgpuCommandBufferRelease)(command);(a.wgpuCommandEncoderRelease)(e);(a.wgpuDevicePoll)(device,1,ptr::null());
   let size=stride as usize*h as usize;let mut state=Box::new(Wait::default());
   (a.wgpuBufferMapAsync)(read,WGPUMapMode_Read,0,size,WGPUBufferMapCallbackInfo{mode:WGPUCallbackMode_AllowProcessEvents,callback:Some(mapped),userdata1:(&mut *state as *mut Wait).cast(),..Default::default()});wait(a,instance,device,state)?;
   let data=(a.wgpuBufferGetConstMappedRange)(read,0,size);if data.is_null(){return Err("Final validation readback is null".into());}
   let mut out=Vec::with_capacity(w as usize*h as usize*4);for row in std::slice::from_raw_parts(data.cast::<u8>(),size).chunks_exact(stride as usize){out.extend_from_slice(&row[..w as usize*4]);}(a.wgpuBufferUnmap)(read);check_errors()?;Ok(out)
  }
  let actual=pixels(self.api,device,instance,queue,target,read,self.width,self.height,stride)?;
  let reference=pixels(self.api,device,instance,queue,self.reference,read,self.width,self.height,stride)?;
  let changed=actual.chunks_exact(4).zip(reference.chunks_exact(4)).filter(|(a,b)|a!=b).count();let max=actual.iter().zip(&reference).map(|(a,b)|a.abs_diff(*b)).max().unwrap_or(0);
  self.verification.push(json!({"submission":serial,"max_u8":max,"changed_rgba_pixels":changed,"plan":self.last,"row252":self.latest.get(252),"packet_rows":self.latest.len()}));
  std::fs::write(self.output.join("final-retention-validation.json"),json!({"enabled":true,"frames":self.verification,"final":self.info(),"scope":"Each validation frame compared with original full compositor using identical current GPU inputs; additional reference draw and two copy/readbacks excluded from performance experiments."}).to_string()).map_err(|e|e.to_string())?;
  if changed!=0{
   std::fs::write(self.output.join("damage-actual.rgba"),actual).map_err(|e|e.to_string())?;std::fs::write(self.output.join("damage-reference.rgba"),reference).map_err(|e|e.to_string())?;
   self.invalidate();return Err(format!("Final retention pixel mismatch at {serial}: {changed} pixels, max {max}"));
  }Ok(())
 }
}
// Deterministic correctness probe, only enabled explicitly by the finite runner.
// Actual benchmark runs never set it; ordinary inputs and playback are untouched.
pub(super) fn probe_rows(rows:&[[f32;4]],frame:u64)->Vec<[f32;4]>{
 let mut r=rows.to_vec();if r.len()<256{return r;}let pixel=r[253][0];
 match frame {
  4|5=>{r[252][0]+=pixel*(frame as f32-3.0)*0.25;r[252][3]=1.;},
  6=>r[252][3]=0.,7=>{r[252][0]-=pixel*2.5;r[252][3]=1.;},
  8=>r[0][3]+=0.5,9=>r[245][2]=1.,10=>r[245][0]=1.,
  11=>r[1][3]=1.,12=>r[250][3]=0.,13=>r[252][0]+=pixel*0.75,
  14=>r[253][0]*=1.5,15=>r[5][0]+=0.02,_=>()
 }r
}
