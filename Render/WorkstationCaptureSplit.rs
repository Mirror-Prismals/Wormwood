//! Opt-in synchronous capture specialization. Ordinary and incremental capture
//! remain available; no scene data or optical calculation is approximated.
use super::*;
#[path="WorkstationCaptureSplitSource.rs"] mod source;
const JOB_CAP:u64=128*1024*1024;
pub(super) fn requested()->bool{std::env::var("WORMWOOD_WORKSTATION_CAPTURE_SPLIT").as_deref()==Ok("1")}
fn job_bytes(rows:&[[f32;4]],width:u32,height:u32,groups:u32,limit:u64)->Option<u64>{
 if width==0||height==0||width>16384||height>16384||groups==0||groups>6{return None;}
 width.checked_mul(height)?.checked_mul(groups.checked_mul(2)?)?;
 let mut jobs=0u64;
 for g in 0..groups as usize{
  let r=rows.get(2+g*4)?;
  if r.iter().any(|v|!v.is_finite())||r[2]<0.||r[3]<0.||r[2].fract()!=0.||r[3].fract()!=0.||r[2]>width as f32||r[3]>height as f32{return None;}
  jobs=jobs.checked_add((r[2]as u64).checked_mul(r[3]as u64)?.checked_mul(2)?)?;
 }
 let bytes=jobs.checked_mul(4)?.max(4);(bytes<=limit.min(JOB_CAP)).then_some(bytes)
}
type BindingKey=Vec<(u32,usize,u64,u64,usize)>;
fn binding_key(entries:&[WGPUBindGroupEntry])->BindingKey{entries.iter().map(|e|(e.binding,e.buffer as usize,e.offset,e.size,e.textureView as usize)).collect()}
pub(super) struct Runtime{
 api:&'static Api,device:WGPUDevice,queue:WGPUQueue,
 layouts:[WGPUBindGroupLayout;2],pipeline_layouts:[WGPUPipelineLayout;2],
 shaders:[WGPUShaderModule;2],pipelines:[WGPUComputePipeline;2],groups:[WGPUBindGroup;2],
 jobs:WGPUBuffer,counter:WGPUBuffer,bytes:u64,limit:u64,key:BindingKey,
 encoded:u64,rejections:u64,
}
impl Runtime{
 pub(super) unsafe fn new(c:&Client,input:&str,base:&[WGPUBindGroupLayoutEntry])->Result<Self,String>{
  let modules=source::sources(input)?;let a=c.api;
  let mut limits=WGPULimits::default();
  if(a.wgpuDeviceGetLimits)(c.device,&mut limits)!=WGPUStatus_Success{return Err("capture split limits unavailable".into());}
  if limits.maxComputeWorkgroupsPerDimension<65535{return Err("capture split requires standard65535 dispatch limit".into());}
  let limit=limits.maxBufferSize.min(limits.maxStorageBufferBindingSize).min(JOB_CAP);
  let mut s=Self{api:a,device:c.device,queue:c.queue,layouts:[ptr::null_mut();2],pipeline_layouts:[ptr::null_mut();2],shaders:[ptr::null_mut();2],pipelines:[ptr::null_mut();2],groups:[ptr::null_mut();2],jobs:ptr::null_mut(),counter:ptr::null_mut(),bytes:0,limit,key:vec![],encoded:0,rejections:0};
  for(i,code)in[modules.fast,modules.fallback].iter().enumerate(){
   let mut ls=base.to_vec();
   for(binding,size)in[(14,4),(15,16)]{ls.push(WGPUBindGroupLayoutEntry{binding,visibility:WGPUShaderStage_Compute,buffer:WGPUBufferBindingLayout{type_:if i==0{WGPUBufferBindingType_Storage}else{WGPUBufferBindingType_ReadOnlyStorage},minBindingSize:size,..Default::default()},..Default::default()});}
   s.layouts[i]=(a.wgpuDeviceCreateBindGroupLayout)(c.device,&WGPUBindGroupLayoutDescriptor{label:view("Specialized retained capture bindings"),entryCount:ls.len(),entries:ls.as_ptr(),..Default::default()});
   if s.layouts[i].is_null(){return Err("capture split layout allocation failed".into());}
   s.pipeline_layouts[i]=(a.wgpuDeviceCreatePipelineLayout)(c.device,&WGPUPipelineLayoutDescriptor{bindGroupLayoutCount:1,bindGroupLayouts:&s.layouts[i],..Default::default()});
   if s.pipeline_layouts[i].is_null(){return Err("capture split pipeline layout failed".into());}
   let wgsl=WGPUShaderSourceWGSL{chain:WGPUChainedStruct{next:ptr::null(),sType:WGPUSType_ShaderSourceWGSL},code:view(code)};
   s.shaders[i]=(a.wgpuDeviceCreateShaderModule)(c.device,&WGPUShaderModuleDescriptor{nextInChain:&wgsl.chain,label:view("Specialized original retained capture")});
   if s.shaders[i].is_null(){return Err("capture split shader allocation failed".into());}
   s.pipelines[i]=(a.wgpuDeviceCreateComputePipeline)(c.device,&WGPUComputePipelineDescriptor{layout:s.pipeline_layouts[i],compute:WGPUProgrammableStageDescriptor{module:s.shaders[i],entryPoint:view(if i==0{"cs_retained_fast"}else{"cs_retained_fallback"}),..Default::default()},..Default::default()});
   if s.pipelines[i].is_null(){return Err("capture split compute pipeline failed".into());}check_errors()?;
  }
  s.counter=(a.wgpuDeviceCreateBuffer)(c.device,&WGPUBufferDescriptor{label:view("Bounded optical fallback count and indirect dispatch"),size:16,usage:WGPUBufferUsage_Storage|WGPUBufferUsage_Indirect|WGPUBufferUsage_CopyDst,..Default::default()});
  if s.counter.is_null(){return Err("capture split count allocation failed".into());}check_errors()?;Ok(s)
 }
 pub(super) unsafe fn invalidate_bindings(&mut self){for g in &mut self.groups{if !g.is_null(){(self.api.wgpuBindGroupRelease)(*g);*g=ptr::null_mut();}}self.key.clear();}
 /// Called only after the renderer drains preceding work for a synchronous
 /// bake. Jobs are bounded by all admitted rectangle samples, not an estimate.
 pub(super) unsafe fn prepare(&mut self,entries:&[WGPUBindGroupEntry],rows:&[[f32;4]],width:u32,height:u32,groups:u32)->Result<bool,String>{
  let Some(required)=job_bytes(rows,width,height,groups,self.limit)else{self.rejections+=1;return Ok(false);};let a=self.api;
  if required>self.bytes{
   let bytes=required.next_power_of_two().min(self.limit);debug_assert!(bytes>=required);
   let jobs=(a.wgpuDeviceCreateBuffer)(self.device,&WGPUBufferDescriptor{label:view("Bounded canonical capture fallback jobs"),size:bytes,usage:WGPUBufferUsage_Storage,..Default::default()});
   if jobs.is_null(){return Err("capture split jobs allocation failed".into());}if let Err(error)=check_errors(){(a.wgpuBufferRelease)(jobs);return Err(error);}
   self.invalidate_bindings();if !self.jobs.is_null(){(a.wgpuBufferRelease)(self.jobs);}self.jobs=jobs;self.bytes=bytes;
  }
  let key=binding_key(entries);
  if self.key!=key||self.groups[0].is_null(){
   let mut es=entries.to_vec();es.push(WGPUBindGroupEntry{binding:14,buffer:self.jobs,size:self.bytes,..Default::default()});es.push(WGPUBindGroupEntry{binding:15,buffer:self.counter,size:16,..Default::default()});
   let mut next=[ptr::null_mut();2];
   for i in 0..2{next[i]=(a.wgpuDeviceCreateBindGroup)(self.device,&WGPUBindGroupDescriptor{layout:self.layouts[i],entryCount:es.len(),entries:es.as_ptr(),..Default::default()});}
   let error=check_errors().err();
   if next.iter().any(|h|h.is_null())||error.is_some(){for h in next{if !h.is_null(){(a.wgpuBindGroupRelease)(h);}}return Err(error.unwrap_or("capture split bind group failed".into()));}
   self.invalidate_bindings();self.groups=next;self.key=key;
  }
  // Queue order keeps the reset after prior submitted users and before this
  // capture. Never reset between the two encoded compute dispatches.
  let reset=[0u32,0,1,1];(a.wgpuQueueWriteBuffer)(self.queue,self.counter,0,reset.as_ptr().cast(),16);Ok(true)
 }
 pub(super) unsafe fn encode(&mut self,pass:WGPUComputePassEncoder,width:u32,height:u32,groups:u32){
  let a=self.api;(a.wgpuComputePassEncoderSetPipeline)(pass,self.pipelines[0]);(a.wgpuComputePassEncoderSetBindGroup)(pass,0,self.groups[0],0,ptr::null());
  (a.wgpuComputePassEncoderDispatchWorkgroups)(pass,width.div_ceil(8),height.div_ceil(8),groups*2);
  (a.wgpuComputePassEncoderSetPipeline)(pass,self.pipelines[1]);(a.wgpuComputePassEncoderSetBindGroup)(pass,0,self.groups[1],0,ptr::null());
  super::super::dispatch_capture_jobs(a,pass,self.counter);self.encoded+=1;
 }
 pub(super) fn info(&self)->Value{json!({"encoded_captures":self.encoded,"unsupported_captures":self.rejections,"allocated_job_bytes":self.bytes,"counter_bytes":16,"hard_job_cap_bytes":self.limit,"scope":"Synchronous captures only; fast pipeline excludes fallback callgraph; negative fast results queue complete canonical invocations; both dispatches inside original capture timestamps. Incremental generations remain canonical."})}
}
impl Drop for Runtime{fn drop(&mut self){unsafe{let a=self.api;self.invalidate_bindings();for h in self.pipelines{if !h.is_null(){(a.wgpuComputePipelineRelease)(h);}}for h in self.shaders{if !h.is_null(){(a.wgpuShaderModuleRelease)(h);}}for h in self.pipeline_layouts{if !h.is_null(){(a.wgpuPipelineLayoutRelease)(h);}}for h in self.layouts{if !h.is_null(){(a.wgpuBindGroupLayoutRelease)(h);}}for h in[self.jobs,self.counter]{if !h.is_null(){(a.wgpuBufferRelease)(h);}}}}}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn production_compact_and_ledger_shader_split_validates(){let raw=include_str!("../Procedures/Shaders/workstation.wgsl");let base=super::super::source::source(raw,true,false,true).unwrap();let ledger=super::super::source::with_meter_ledger(&base).unwrap();let split=source::sources(&ledger).unwrap();assert!(split.fast.contains("cs_retained_fast"));assert!(split.fallback.contains("cs_retained_fallback"));}
 #[test]fn job_capacity_covers_exactly_both_samples_without_unbounded_allocation(){let mut r=[[0.;4];64];for i in 0..6{r[2+i*4]=[0.,0.,1920.,1200.];}assert_eq!(job_bytes(&r,1920,1200,6,JOB_CAP),Some(1920*1200*12*4));let mut huge=r;for i in 0..6{huge[2+i*4]=[0.,0.,16384.,16384.];}assert!(job_bytes(&huge,16384,16384,6,JOB_CAP).is_none());r[2][2]=1920.5;assert!(job_bytes(&r,1920,1200,6,JOB_CAP).is_none());r[2][2]=f32::NAN;assert!(job_bytes(&r,1920,1200,6,JOB_CAP).is_none());}
 #[test]fn compact_rectangles_zero_and_limits_are_checked(){let mut r=[[0.;4];64];r[2]=[-3.,-3.,101.,99.];assert_eq!(job_bytes(&r,1920,1200,1,JOB_CAP),Some(101*99*8));assert!(job_bytes(&r,100,1200,1,JOB_CAP).is_none());assert!(job_bytes(&r,1920,1200,0,JOB_CAP).is_none());assert!(job_bytes(&r,1920,1200,7,JOB_CAP).is_none());assert!(job_bytes(&r,1920,1200,1,4).is_none());r[2]=[0.;4];assert_eq!(job_bytes(&r,1920,1200,1,JOB_CAP),Some(4));}
}
