//! Explicit finite diagnostic: snapshot one full-quality frame, then compare
//! its frozen compositor against a single unfiltered display-color texture read.
//! Never enabled by an ordinary launcher; CPU/audio updates continue unchanged.
use super::*;
pub(super) fn requested(value:Option<&str>,offscreen:bool)->Result<u8,String>{
 match value {
  None|Some("0")=>Ok(0),
  Some("compose") if offscreen=>Ok(1),
  Some("copy") if offscreen=>Ok(2),
  _=>Err("Frozen diagnostic must be 0, compose or copy; compose/copy require serial offscreen diagnostic".into()),
 }
}
const SHADER:&str=r#"
struct SceneVertex {position:vec4<f32>,data:vec4<f32>};
struct Appearance {vertices:array<SceneVertex>};
@group(0) @binding(0) var<storage,read> appearance:Appearance;
@group(0) @binding(1) var frozen:texture_2d<f32>;
@vertex fn vs_copy(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32>{
 return appearance.vertices[index].position;
}
@fragment fn fs_copy(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32>{
 return textureLoad(frozen,vec2<i32>(floor(p.xy)),0);
}
"#;
pub(super) struct Control {
 api:&'static Api,pub mode:u8,pub ready:bool,pub image_ready:bool,
 buffers:Vec<WGPUBuffer>,frozen_group:WGPUBindGroup,copy_group:WGPUBindGroup,
 layout:WGPUBindGroupLayout,pipeline_layout:WGPUPipelineLayout,pipeline:WGPURenderPipeline,
 shader:WGPUShaderModule,texture:WGPUTexture,view:WGPUTextureView,
 count:u32,bakes:u64,buffer_bytes:u64,copies:u64,draws:u64,
 latest_rows:Value,frozen_rows:Value,
}
impl Control {
 pub fn new(api:&'static Api,mode:u8)->Self{Self{api,mode,ready:false,image_ready:false,buffers:vec![],frozen_group:ptr::null_mut(),copy_group:ptr::null_mut(),layout:ptr::null_mut(),pipeline_layout:ptr::null_mut(),pipeline:ptr::null_mut(),shader:ptr::null_mut(),texture:ptr::null_mut(),view:ptr::null_mut(),count:0,bakes:0,buffer_bytes:0,copies:0,draws:0,latest_rows:Value::Null,frozen_rows:Value::Null}}
 pub fn observe_rows(&mut self,rows:&[[f32;4]]){if !self.ready&&rows.len()>254{self.latest_rows=json!({"style_245":rows[245],"ruler_250":rows[250],"playhead_252":rows[252],"pixel_253":rows[253],"ticks_254":rows[254]});}}
 pub fn is_copy(&self)->bool{self.mode==2&&self.image_ready}
 pub unsafe fn prepare(&mut self,device:WGPUDevice,encoder:WGPUCommandEncoder,p:&prerender::Pipeline,width:u32,height:u32,format:WGPUTextureFormat,count:u32)->Result<(),String>{
  if self.ready {if p.bakes!=self.bakes||p.pending||count!=self.count{return Err("Frozen fixture changed bake generation or vertex count".into());}return Ok(());}
  if p.pending{return Err("Cannot freeze an unfinished appearance bake".into());}
  let api=self.api;let (render_layout,mut entries)=p.diagnostic_bindings();
  // Validated reachable call graph uses only vertex binding 0 and fragment
  // bindings 1,5,6. Bindings 2/3 are not read by either retained entry point.
  for entry in &mut entries {if [0,1,6].contains(&entry.binding){
   let buffer=(api.wgpuDeviceCreateBuffer)(device,&WGPUBufferDescriptor{label:view("Frozen diagnostic input snapshot"),size:entry.size,usage:WGPUBufferUsage_Storage|WGPUBufferUsage_CopyDst,..Default::default()});
   if buffer.is_null(){return Err("Frozen input allocation failed".into());}
   self.buffers.push(buffer);self.buffer_bytes+=entry.size;
   (api.wgpuCommandEncoderCopyBufferToBuffer)(encoder,entry.buffer,entry.offset,buffer,0,entry.size);
   entry.buffer=buffer;entry.offset=0;
  }}
  self.frozen_group=(api.wgpuDeviceCreateBindGroup)(device,&WGPUBindGroupDescriptor{label:view("Frozen original compositor inputs"),layout:render_layout,entryCount:entries.len(),entries:entries.as_ptr(),..Default::default()});
  self.texture=(api.wgpuDeviceCreateTexture)(device,&WGPUTextureDescriptor{label:view("Frozen display-color reference"),usage:WGPUTextureUsage_TextureBinding|WGPUTextureUsage_CopyDst|WGPUTextureUsage_CopySrc,dimension:WGPUTextureDimension_2D,size:WGPUExtent3D{width,height,depthOrArrayLayers:1},format,mipLevelCount:1,sampleCount:1,..Default::default()});
  self.view=(api.wgpuTextureCreateView)(self.texture,ptr::null());
  let ls=[WGPUBindGroupLayoutEntry{binding:0,visibility:WGPUShaderStage_Vertex,buffer:WGPUBufferBindingLayout{type_:WGPUBufferBindingType_ReadOnlyStorage,minBindingSize:entries.iter().find(|e|e.binding==0).unwrap().size,..Default::default()},..Default::default()},WGPUBindGroupLayoutEntry{binding:1,visibility:WGPUShaderStage_Fragment,texture:WGPUTextureBindingLayout{sampleType:WGPUTextureSampleType_UnfilterableFloat,viewDimension:WGPUTextureViewDimension_2D,multisampled:0,..Default::default()},..Default::default()}];
  self.layout=(api.wgpuDeviceCreateBindGroupLayout)(device,&WGPUBindGroupLayoutDescriptor{entryCount:ls.len(),entries:ls.as_ptr(),..Default::default()});
  self.pipeline_layout=(api.wgpuDeviceCreatePipelineLayout)(device,&WGPUPipelineLayoutDescriptor{bindGroupLayoutCount:1,bindGroupLayouts:&self.layout,..Default::default()});
  let vertex=*entries.iter().find(|e|e.binding==0).unwrap();
  let bs=[vertex,WGPUBindGroupEntry{binding:1,textureView:self.view,..Default::default()}];
  self.copy_group=(api.wgpuDeviceCreateBindGroup)(device,&WGPUBindGroupDescriptor{layout:self.layout,entryCount:bs.len(),entries:bs.as_ptr(),..Default::default()});
  let wgsl=WGPUShaderSourceWGSL{chain:WGPUChainedStruct{next:ptr::null(),sType:WGPUSType_ShaderSourceWGSL},code:view(SHADER)};
  self.shader=(api.wgpuDeviceCreateShaderModule)(device,&WGPUShaderModuleDescriptor{nextInChain:&wgsl.chain,label:view("Frozen diagnostic one-texel copy")});
  let target=WGPUColorTargetState{format,writeMask:WGPUColorWriteMask_All,..Default::default()};
  let fragment=WGPUFragmentState{module:self.shader,entryPoint:view("fs_copy"),targetCount:1,targets:&target,..Default::default()};
  let depth=WGPUDepthStencilState{format:WGPUTextureFormat_Depth32Float,depthWriteEnabled:WGPUOptionalBool_True,depthCompare:WGPUCompareFunction_Less,..Default::default()};
  self.pipeline=(api.wgpuDeviceCreateRenderPipeline)(device,&WGPURenderPipelineDescriptor{layout:self.pipeline_layout,vertex:WGPUVertexState{module:self.shader,entryPoint:view("vs_copy"),..Default::default()},primitive:WGPUPrimitiveState{topology:WGPUPrimitiveTopology_TriangleList,frontFace:WGPUFrontFace_CCW,cullMode:WGPUCullMode_None,..Default::default()},multisample:WGPUMultisampleState{count:1,mask:u32::MAX,..Default::default()},fragment:&fragment,depthStencil:&depth,..Default::default()});
  if [self.frozen_group.cast::<()>(),self.copy_group.cast(),self.texture.cast(),self.view.cast(),self.layout.cast(),self.pipeline_layout.cast(),self.shader.cast(),self.pipeline.cast()].iter().any(|x|x.is_null()){return Err("Frozen diagnostic resource creation failed".into());}
  check_errors()?;self.count=count;self.bakes=p.bakes;self.frozen_rows=self.latest_rows.clone();self.ready=true;Ok(())
 }
 pub fn selection(&mut self,original:WGPURenderPipeline)->(WGPURenderPipeline,WGPUBindGroup){self.draws+=1;if self.is_copy(){(self.pipeline,self.copy_group)}else{(original,self.frozen_group)}}
 pub unsafe fn capture_reference(&mut self,encoder:WGPUCommandEncoder,target:WGPUTexture,width:u32,height:u32){
  if self.ready&&!self.image_ready{
   copy_frozen_texture(self.api,encoder,target,self.texture,width,height);
   self.image_ready=true;self.copies+=1;
  }
 }
 pub fn info(&self)->Value{json!({"mode":if self.mode==2{"copy"}else{"compose"},"ready":self.ready,"image_ready":self.image_ready,"freeze_submission":4,"frozen_bindings":[0,1,6],"snapshot_buffer_bytes":self.buffer_bytes,"reference_copies":self.copies,"draws_using_frozen_inputs":self.draws,"frozen_fixture_rows":self.frozen_rows,"scope":"Frozen full-frame diagnostic only. CPU/audio updates and uploads continue; snapshot buffers feed the original compositor or one unfiltered display-color texture load. Freeze/copy setup is warmup, not measured work."})}
}
impl Drop for Control {fn drop(&mut self){unsafe{
 let a=self.api;
 for g in [self.frozen_group,self.copy_group]{if !g.is_null(){(a.wgpuBindGroupRelease)(g);}}
 if !self.pipeline.is_null(){(a.wgpuRenderPipelineRelease)(self.pipeline);}
 if !self.pipeline_layout.is_null(){(a.wgpuPipelineLayoutRelease)(self.pipeline_layout);}
 if !self.layout.is_null(){(a.wgpuBindGroupLayoutRelease)(self.layout);}
 if !self.shader.is_null(){(a.wgpuShaderModuleRelease)(self.shader);}
 if !self.view.is_null(){(a.wgpuTextureViewRelease)(self.view);}
 if !self.texture.is_null(){(a.wgpuTextureRelease)(self.texture);}
 for b in self.buffers.drain(..){(a.wgpuBufferRelease)(b);}
}}}
#[cfg(test)] mod frozen_diagnostic_tests {
 use super::*;
 #[test] fn frozen_control_is_explicit_and_offscreen_only(){assert_eq!(requested(None,false).unwrap(),0);assert_eq!(requested(Some("0"),false).unwrap(),0);assert_eq!(requested(Some("compose"),true).unwrap(),1);assert_eq!(requested(Some("copy"),true).unwrap(),2);for v in ["compose","copy","1","yes"]{assert!(requested(Some(v),false).is_err());}}
 #[test] fn one_load_shader_validates(){let m=naga::front::wgsl::parse_str(SHADER).unwrap();naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).unwrap();assert_eq!(SHADER.matches("textureLoad(").count(),1);}
}

// Outside the measured loop, compare the last drawn target against the exact
// display-color image saved at freeze. No measured-frame readback is added.
pub(super) unsafe fn verify(control:&Control,device:WGPUDevice,instance:WGPUInstance,queue:WGPUQueue,target:WGPUTexture,read:WGPUBuffer,width:u32,height:u32,stride:u32,output:&Path)->Result<(),String>{
 let api=control.api;
 unsafe fn pixels(api:&Api,device:WGPUDevice,instance:WGPUInstance,queue:WGPUQueue,texture:WGPUTexture,read:WGPUBuffer,width:u32,height:u32,stride:u32)->Result<Vec<u8>,String>{
  let encoder=(api.wgpuDeviceCreateCommandEncoder)(device,ptr::null());
  (api.wgpuCommandEncoderCopyTextureToBuffer)(encoder,&WGPUTexelCopyTextureInfo{texture,aspect:WGPUTextureAspect_All,..Default::default()},&WGPUTexelCopyBufferInfo{buffer:read,layout:WGPUTexelCopyBufferLayout{bytesPerRow:stride,rowsPerImage:height,..Default::default()}},&WGPUExtent3D{width,height,depthOrArrayLayers:1});
  let command=(api.wgpuCommandEncoderFinish)(encoder,ptr::null());
  (api.wgpuQueueSubmit)(queue,1,&command);(api.wgpuCommandBufferRelease)(command);(api.wgpuCommandEncoderRelease)(encoder);
  (api.wgpuDevicePoll)(device,1,ptr::null());
  let size=stride as usize*height as usize;let mut state=Box::new(Wait::default());
  (api.wgpuBufferMapAsync)(read,WGPUMapMode_Read,0,size,WGPUBufferMapCallbackInfo{mode:WGPUCallbackMode_AllowProcessEvents,callback:Some(mapped),userdata1:(&mut *state as *mut Wait).cast(),..Default::default()});
  wait(api,instance,device,state)?;
  let data=(api.wgpuBufferGetConstMappedRange)(read,0,size);if data.is_null(){return Err("Frozen verification map failed".into());}
  let mut result=Vec::with_capacity(width as usize*height as usize*4);
  for row in std::slice::from_raw_parts(data.cast::<u8>(),size).chunks_exact(stride as usize){result.extend_from_slice(&row[..width as usize*4]);}
  (api.wgpuBufferUnmap)(read);check_errors()?;Ok(result)
 }
 if !control.image_ready{return Err("Frozen reference was not completed".into());}
 let original=pixels(api,device,instance,queue,control.texture,read,width,height,stride)?;
 let final_pixels=pixels(api,device,instance,queue,target,read,width,height,stride)?;
 let max=original.iter().zip(&final_pixels).map(|(a,b)|a.abs_diff(*b)).max().unwrap_or(0);
 let changed=original.chunks_exact(4).zip(final_pixels.chunks_exact(4)).filter(|(a,b)|a!=b).count();
 let report=json!({"complete":true,"max_u8":max,"changed_rgba_pixels":changed,"width":width,"height":height,"compared_channels":original.len(),"verification_copy_submissions":2,"scope":"Exact final render target versus original full compositor's freeze-frame GPU texture, including alpha. Two copy/readback submissions after benchmark completion, outside measured frame counts and timestamps.","frozen_control":control.info()});
 std::fs::write(output.join("frozen-image-verification.json"),serde_json::to_vec_pretty(&report).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
 Ok(())
}
