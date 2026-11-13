//! A completed image of the real paused game world, kept separate from DAW art.
//! UI changes must not re-run world water/reflection/shadow passes. Explicit
//! camera/world changes invalidate this image. Gameplay resumes normally.
use super::*;
const SOURCE:&str=r#"
@group(0) @binding(0) var image:texture_2d<f32>;
@vertex fn vs_world_image(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32>{
 let p=array<vec2<f32>,3>(vec2<f32>(-1.0,-1.0),vec2<f32>(3.0,-1.0),vec2<f32>(-1.0,3.0));return vec4<f32>(p[i],0.9999,1.0);
}
@fragment fn fs_world_image(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32>{return textureLoad(image,vec2<i32>(floor(p.xy)),0);}
"#;
pub(super) struct Image{
 api:&'static Api,depth:WGPUTexture,texture:WGPUTexture,view:WGPUTextureView,layout:WGPUBindGroupLayout,pipeline_layout:WGPUPipelineLayout,shader:WGPUShaderModule,pipeline:WGPURenderPipeline,group:WGPUBindGroup,
 pub size:[u32;2],pub format:WGPUTextureFormat,pub ready:bool,pending:bool,pub captures:u64,pub draws:u64,
}
impl Image{
 pub unsafe fn new(c:&Client)->Result<Self,String>{let a=c.api;let mut p=Self{api:a,depth:ptr::null_mut(),texture:ptr::null_mut(),view:ptr::null_mut(),layout:ptr::null_mut(),pipeline_layout:ptr::null_mut(),shader:ptr::null_mut(),pipeline:ptr::null_mut(),group:ptr::null_mut(),size:[c.width,c.height],format:c.format,ready:false,pending:false,captures:0,draws:0};
  p.texture=(a.wgpuDeviceCreateTexture)(c.device,&WGPUTextureDescriptor{label:view("Real paused game background image"),usage:WGPUTextureUsage_CopyDst|WGPUTextureUsage_TextureBinding,dimension:WGPUTextureDimension_2D,size:WGPUExtent3D{width:c.width,height:c.height,depthOrArrayLayers:1},format:c.format,mipLevelCount:1,sampleCount:1,..Default::default()});
  p.depth=(a.wgpuDeviceCreateTexture)(c.device,&WGPUTextureDescriptor{usage:WGPUTextureUsage_CopyDst|WGPUTextureUsage_CopySrc,dimension:WGPUTextureDimension_2D,size:WGPUExtent3D{width:c.width,height:c.height,depthOrArrayLayers:1},format:WGPUTextureFormat_Depth32Float,mipLevelCount:1,sampleCount:1,..Default::default()});
  p.view=(a.wgpuTextureCreateView)(p.texture,ptr::null());
  let entry=WGPUBindGroupLayoutEntry{binding:0,visibility:WGPUShaderStage_Fragment,texture:WGPUTextureBindingLayout{sampleType:WGPUTextureSampleType_UnfilterableFloat,viewDimension:WGPUTextureViewDimension_2D,multisampled:0,..Default::default()},..Default::default()};
  p.layout=(a.wgpuDeviceCreateBindGroupLayout)(c.device,&WGPUBindGroupLayoutDescriptor{entryCount:1,entries:&entry,..Default::default()});
  p.pipeline_layout=(a.wgpuDeviceCreatePipelineLayout)(c.device,&WGPUPipelineLayoutDescriptor{bindGroupLayoutCount:1,bindGroupLayouts:&p.layout,..Default::default()});
  let binding=WGPUBindGroupEntry{binding:0,textureView:p.view,..Default::default()};p.group=(a.wgpuDeviceCreateBindGroup)(c.device,&WGPUBindGroupDescriptor{layout:p.layout,entryCount:1,entries:&binding,..Default::default()});
  let wgsl=WGPUShaderSourceWGSL{chain:WGPUChainedStruct{next:ptr::null(),sType:WGPUSType_ShaderSourceWGSL},code:view(SOURCE)};p.shader=(a.wgpuDeviceCreateShaderModule)(c.device,&WGPUShaderModuleDescriptor{nextInChain:&wgsl.chain,label:view("Paused world image composition")});
  let target=WGPUColorTargetState{format:c.format,writeMask:WGPUColorWriteMask_All,..Default::default()};let fragment=WGPUFragmentState{module:p.shader,entryPoint:view("fs_world_image"),targetCount:1,targets:&target,..Default::default()};let depth=WGPUDepthStencilState{format:WGPUTextureFormat_Depth32Float,depthWriteEnabled:WGPUOptionalBool_True,depthCompare:WGPUCompareFunction_Less,..Default::default()};
  p.pipeline=(a.wgpuDeviceCreateRenderPipeline)(c.device,&WGPURenderPipelineDescriptor{layout:p.pipeline_layout,vertex:WGPUVertexState{module:p.shader,entryPoint:view("vs_world_image"),..Default::default()},fragment:&fragment,depthStencil:&depth,primitive:WGPUPrimitiveState{topology:WGPUPrimitiveTopology_TriangleList,frontFace:WGPUFrontFace_CCW,cullMode:WGPUCullMode_None,..Default::default()},multisample:WGPUMultisampleState{count:1,mask:u32::MAX,..Default::default()},..Default::default()});
  if p.texture.is_null()||p.view.is_null()||p.group.is_null()||p.pipeline.is_null(){return Err("paused world image allocation failed".into());}check_errors()?;Ok(p)
 }
 pub unsafe fn depth_copy(&self,encoder:WGPUCommandEncoder,live:WGPUTexture,restore:bool){let (source,target)=if restore{(self.depth,live)}else{(live,self.depth)};let a=WGPUTexelCopyTextureInfo{texture:source,aspect:WGPUTextureAspect_DepthOnly,..Default::default()};let b=WGPUTexelCopyTextureInfo{texture:target,aspect:WGPUTextureAspect_DepthOnly,..Default::default()};(self.api.wgpuCommandEncoderCopyTextureToTexture)(encoder,&a,&b,&WGPUExtent3D{width:self.size[0],height:self.size[1],depthOrArrayLayers:1});}
 pub fn invalidate(&mut self){self.ready=false;self.pending=false;}
 pub unsafe fn capture(&mut self,encoder:WGPUCommandEncoder,source:WGPUTexture){let a=WGPUTexelCopyTextureInfo{texture:source,aspect:WGPUTextureAspect_All,..Default::default()};let b=WGPUTexelCopyTextureInfo{texture:self.texture,aspect:WGPUTextureAspect_All,..Default::default()};(self.api.wgpuCommandEncoderCopyTextureToTexture)(encoder,&a,&b,&WGPUExtent3D{width:self.size[0],height:self.size[1],depthOrArrayLayers:1});self.pending=true;}
 pub fn submitted(&mut self){if self.pending{self.ready=true;self.pending=false;self.captures+=1;}}
 pub unsafe fn draw(&mut self,pass:WGPURenderPassEncoder){(self.api.wgpuRenderPassEncoderSetPipeline)(pass,self.pipeline);(self.api.wgpuRenderPassEncoderSetBindGroup)(pass,0,self.group,0,ptr::null());(self.api.wgpuRenderPassEncoderDraw)(pass,3,1,0,0);self.draws+=1;}
 pub fn info(&self)->Value{json!({"ready":self.ready,"world_captures":self.captures,"image_draws":self.draws,"size":self.size,"scope":"Actual paused game color image copied before DAW overlay; refreshed on camera/world change or resize; ordinary gameplay renderer unchanged."})}
}
impl Drop for Image{fn drop(&mut self){unsafe{let a=self.api;if !self.depth.is_null(){(a.wgpuTextureRelease)(self.depth);}if !self.group.is_null(){(a.wgpuBindGroupRelease)(self.group);}if !self.pipeline.is_null(){(a.wgpuRenderPipelineRelease)(self.pipeline);}if !self.shader.is_null(){(a.wgpuShaderModuleRelease)(self.shader);}if !self.pipeline_layout.is_null(){(a.wgpuPipelineLayoutRelease)(self.pipeline_layout);}if !self.layout.is_null(){(a.wgpuBindGroupLayoutRelease)(self.layout);}if !self.view.is_null(){(a.wgpuTextureViewRelease)(self.view);}if !self.texture.is_null(){(a.wgpuTextureRelease)(self.texture);}}}}
#[cfg(test)]mod tests{
 #[test]fn paused_world_shader_is_a_texel_copy_with_no_scene_or_optical_work(){let s=super::SOURCE;let m=naga::front::wgsl::parse_str(s).unwrap();naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).unwrap();assert!(s.contains("textureLoad"));assert!(!s.contains("gem"));}
}
