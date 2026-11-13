//! GPU construction of the original bar planes, independent of ray/lighting.
//! Only synchronous retained captures prime the optional table. Immutable
//! incremental generations start with a zero tail and use canonical fallback.
use super::*;
#[path="WorkstationBarPlanesSource.rs"]mod shader;
pub(super) fn requested()->bool{std::env::var("WORMWOOD_WORKSTATION_BAR_PLANE_TABLE").as_deref()==Ok("1")}
pub(super) fn metadata_bytes(enabled:bool)->u64{(if enabled{shader::TOTAL_ROWS}else{shader::CPU_META_ROWS}*16)as u64}
pub(super) fn transform(source:&str)->Result<String,String>{shader::transform(source)}
fn shape_key(frame:&[[f32;4]])->Option<[[u32;2];3]>{
 if frame.len()<=45749{return None;}
 Some(std::array::from_fn(|i|[frame[45747+i][0].to_bits(),frame[45747+i][1].to_bits()]))
}
pub(super) struct Runtime{
 api:&'static Api,shader:WGPUShaderModule,layout:WGPUBindGroupLayout,
 pipeline_layout:WGPUPipelineLayout,pipeline:WGPUComputePipeline,group:WGPUBindGroup,
 key:Option<[[u32;2];3]>,dispatches:u64,
}
impl Runtime{
 pub(super) unsafe fn new(c:&Client,metadata:WGPUBuffer,original:&str)->Result<Self,String>{
  let source=shader::compute_source(original)?;let a=c.api;
  let mut s=Self{api:a,shader:ptr::null_mut(),layout:ptr::null_mut(),pipeline_layout:ptr::null_mut(),pipeline:ptr::null_mut(),group:ptr::null_mut(),key:None,dispatches:0};
  let sizes=[c.texture_capacity as u64,metadata_bytes(true)];
  let layouts:[WGPUBindGroupLayoutEntry;2]=std::array::from_fn(|i|WGPUBindGroupLayoutEntry{binding:i as u32,visibility:WGPUShaderStage_Compute,buffer:WGPUBufferBindingLayout{type_:if i==0{WGPUBufferBindingType_ReadOnlyStorage}else{WGPUBufferBindingType_Storage},minBindingSize:sizes[i],..Default::default()},..Default::default()});
  s.layout=(a.wgpuDeviceCreateBindGroupLayout)(c.device,&WGPUBindGroupLayoutDescriptor{label:view("Original bar plane table bindings"),entryCount:2,entries:layouts.as_ptr(),..Default::default()});
  s.pipeline_layout=(a.wgpuDeviceCreatePipelineLayout)(c.device,&WGPUPipelineLayoutDescriptor{bindGroupLayoutCount:1,bindGroupLayouts:&s.layout,..Default::default()});
  let buffers=[c.texture_buffer,metadata];let entries:[WGPUBindGroupEntry;2]=std::array::from_fn(|i|WGPUBindGroupEntry{binding:i as u32,buffer:buffers[i],size:sizes[i],..Default::default()});
  s.group=(a.wgpuDeviceCreateBindGroup)(c.device,&WGPUBindGroupDescriptor{layout:s.layout,entryCount:2,entries:entries.as_ptr(),..Default::default()});
  let wgsl=WGPUShaderSourceWGSL{chain:WGPUChainedStruct{next:ptr::null(),sType:WGPUSType_ShaderSourceWGSL},code:view(&source)};
  s.shader=(a.wgpuDeviceCreateShaderModule)(c.device,&WGPUShaderModuleDescriptor{nextInChain:&wgsl.chain,label:view("Original bar plane construction")});
  s.pipeline=(a.wgpuDeviceCreateComputePipeline)(c.device,&WGPUComputePipelineDescriptor{layout:s.pipeline_layout,compute:WGPUProgrammableStageDescriptor{module:s.shader,entryPoint:view("cs_retained_bar_planes"),..Default::default()},..Default::default()});
  if s.layout.is_null()||s.pipeline_layout.is_null()||s.pipeline.is_null()||s.group.is_null()||s.shader.is_null(){return Err("bar plane table pipeline allocation failed".into());}check_errors()?;Ok(s)
 }
 /// Same capture pass and timestamps include table work. Compute dispatch
 /// storage dependencies order these81 writes before all subsequent consumers.
 pub(super) unsafe fn encode(&mut self,pass:WGPUComputePassEncoder,frame:&[[f32;4]])->bool{
  let Some(key)=shape_key(frame)else{return false;};if self.key==Some(key){return false;}
  let a=self.api;(a.wgpuComputePassEncoderSetPipeline)(pass,self.pipeline);
  (a.wgpuComputePassEncoderSetBindGroup)(pass,0,self.group,0,ptr::null());
  (a.wgpuComputePassEncoderDispatchWorkgroups)(pass,shader::COMPUTE_WORKGROUPS,1,1);
  self.key=Some(key);self.dispatches+=1;true
 }
 pub(super) fn info(&self)->Value{json!({"dispatches":self.dispatches,"plane_count":78,"extra_metadata_bytes":shader::TABLE_ROWS*16,"scope":"Unchanged GPU plane equations; exact halfSize header; original intersection order; synchronous capture timestamp includes table dispatch. Frozen incremental metadata starts zero and calculates planes canonically."})}
}
impl Drop for Runtime{fn drop(&mut self){unsafe{let a=self.api;if !self.group.is_null(){(a.wgpuBindGroupRelease)(self.group);}if !self.pipeline.is_null(){(a.wgpuComputePipelineRelease)(self.pipeline);}if !self.pipeline_layout.is_null(){(a.wgpuPipelineLayoutRelease)(self.pipeline_layout);}if !self.layout.is_null(){(a.wgpuBindGroupLayoutRelease)(self.layout);}if !self.shader.is_null(){(a.wgpuShaderModuleRelease)(self.shader);}}}}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn key_tracks_all_six_original_plane_inputs_bitwise_only(){
  let mut f=vec![[0.;4];45750];for i in 0..3{f[45747+i]=[20.+i as f32,1.,1.,1.];}let k=shape_key(&f).unwrap();
  for i in 0..3{for lane in 0..2{let mut x=f.clone();x[45747+i][lane]=f32::from_bits(x[45747+i][lane].to_bits()+1);assert_ne!(shape_key(&x),Some(k));}}
  for (i,lane)in[(45744,0),(45747,2),(45748,3),(10,1)]{let mut x=f.clone();x[i][lane]+=0.01;assert_eq!(shape_key(&x),Some(k));}
  assert!(shape_key(&f[..45749]).is_none());assert_eq!(metadata_bytes(false),1024);assert_eq!(metadata_bytes(true),2320);
 }
 #[test]fn full_retained_and_ledger_shaders_validate_with_same_bindings(){
  let original=include_str!("../Procedures/Shaders/workstation.wgsl");let ordinary=super::super::source::source(original,true,false,true).unwrap();let changed=transform(&ordinary).unwrap();
  let mut modes=vec![changed.clone(),super::super::source::with_meter_ledger(&changed).unwrap()];
  modes.push(super::super::source::meter_ledger_capture_source(&modes[1],true).unwrap().unwrap());
  for s in modes{let m=naga::front::wgsl::parse_str(&s).unwrap_or_else(|e|panic!("{}",e.emit_to_string(&s)));naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&m).unwrap();}
 }
}
