//! Bounded original final-color foreground bank for supported pass-through views.
//! This is not a general interaction cache. Root owns canonical pose selection,
//! complete immutable-resource epochs, live camera admission and fallback.
use super::*;

const MAX_STATES: usize = 32;
const CAP_BYTES: u64 = 384 * 1024 * 1024;
const SHADER: &str = r#"
@group(0) @binding(0) var bank_image:texture_2d<f32>;
@vertex fn vs_motion_bank(@builtin(vertex_index) i:u32)->@builtin(position) vec4<f32>{
 let p=array<vec2<f32>,3>(vec2<f32>(-1.0,-1.0),vec2<f32>(3.0,-1.0),vec2<f32>(-1.0,3.0));
 // Same depth as the validated original fullscreen tail. Camera depth stays live.
 return vec4<f32>(p[i],0.5,1.0);
}
@fragment fn fs_motion_bank(@builtin(position) p:vec4<f32>)->@location(0) vec4<f32>{
 let color=textureLoad(bank_image,vec2<i32>(floor(p.xy)),0);
 // Original fs_retained either discards or returns alpha1. No blending/filtering.
 if(color.a==0.0){discard;}
 return color;
}
"#;

// BEGIN PURE BANK ADMISSION
#[derive(Clone, Debug, PartialEq, Eq)]
struct Key { epoch: u64, rows: Vec<[u32;4]> }
impl Key {
 fn new(epoch:u64,rows:&[[f32;4]],expected:usize)->Option<Self>{
  if rows.len()!=expected || rows.iter().flatten().any(|v|!v.is_finite()){return None;}
  Some(Self{epoch,rows:rows.iter().map(|r|r.map(f32::to_bits)).collect()})
 }
}
fn texture_bytes(width:u32,height:u32)->Option<u64>{
 if width==0||height==0{return None;}
 u64::from(width).checked_mul(u64::from(height))?.checked_mul(4)
}
fn admits_allocation(current:u64,count:usize,next:u64)->bool{
 count<MAX_STATES&&next>0&&current.checked_add(next).is_some_and(|v|v<=CAP_BYTES)
}
fn normalized_playhead(rows:&[[f32;4]])->Option<f64>{
 if rows.len()<=252||rows[252][3]<=0.5{return None;}
 let left=rows[250][0]as f64;let width=rows[250][1]as f64-left;
 let p=(rows[252][0]as f64-left)/width;
 (width>0.&&p.is_finite()).then_some(p)
}
fn recycle_slot(entries:&[(bool,u64)],can_grow:bool)->Option<usize>{
 entries.iter().position(|e|!e.0).or_else(||if can_grow{None}else{entries.iter().enumerate().min_by_key(|(_,e)|e.1).map(|(i,_)|i)})
}
// END PURE BANK ADMISSION

struct Entry {
 api:&'static Api,key:Key,texture:WGPUTexture,view:WGPUTextureView,
 group:WGPUBindGroup,ready:bool,submission:u64,last_used:u64,
}
impl Drop for Entry {fn drop(&mut self){unsafe{
 if !self.group.is_null(){(self.api.wgpuBindGroupRelease)(self.group);}
 if !self.view.is_null(){(self.api.wgpuTextureViewRelease)(self.view);}
 if !self.texture.is_null(){(self.api.wgpuTextureRelease)(self.texture);}
}}}
struct Pending { slot:usize, serial:Option<u64> }

pub(super) struct Bank {
 api:&'static Api,device:WGPUDevice,width:u32,height:u32,format:WGPUTextureFormat,
 layout:WGPUBindGroupLayout,pipeline_layout:WGPUPipelineLayout,
 shader:WGPUShaderModule,pipeline:WGPURenderPipeline,
 entries:Vec<Entry>,latest:Option<Key>,epoch:u64,pending:Option<Pending>,
 bytes_per_state:u64,allocated_bytes:u64,observations:u64,draws:u64,playhead:Option<f64>,dynamic_playhead:bool,admission_skips:u64,
 captures:u64,published:u64,invalidations:u64,capacity_misses:u64,rejected:u64,recycled:u64,
}
impl Bank {
 pub(super) unsafe fn new(c:&Client)->Result<Self,String>{
  if !matches!(c.format,WGPUTextureFormat_BGRA8Unorm|WGPUTextureFormat_RGBA8Unorm
      |WGPUTextureFormat_BGRA8UnormSrgb|WGPUTextureFormat_RGBA8UnormSrgb){
   return Err("Motion bank requires the exact four-byte surface color format".into());
  }
  let bytes_per_state=texture_bytes(c.width,c.height).ok_or("Motion bank dimensions overflow")?;
  if !admits_allocation(0,0,bytes_per_state){return Err("Motion bank state exceeds384MiB cap".into());}
  let a=c.api;
  let mut b=Self{api:a,device:c.device,width:c.width,height:c.height,format:c.format,
   layout:ptr::null_mut(),pipeline_layout:ptr::null_mut(),shader:ptr::null_mut(),pipeline:ptr::null_mut(),
   entries:vec![],latest:None,epoch:0,pending:None,bytes_per_state,allocated_bytes:0,
   observations:0,draws:0,playhead:None,dynamic_playhead:false,admission_skips:0,captures:0,published:0,invalidations:0,capacity_misses:0,rejected:0,recycled:0};
  let entry=WGPUBindGroupLayoutEntry{binding:0,visibility:WGPUShaderStage_Fragment,
   texture:WGPUTextureBindingLayout{sampleType:WGPUTextureSampleType_UnfilterableFloat,
    viewDimension:WGPUTextureViewDimension_2D,multisampled:0,..Default::default()},..Default::default()};
  b.layout=(a.wgpuDeviceCreateBindGroupLayout)(c.device,&WGPUBindGroupLayoutDescriptor{
   label:view("Finite original foreground bank"),entryCount:1,entries:&entry,..Default::default()});
  if b.layout.is_null(){return Err("Motion bank layout allocation failed".into());}
  b.pipeline_layout=(a.wgpuDeviceCreatePipelineLayout)(c.device,&WGPUPipelineLayoutDescriptor{
   bindGroupLayoutCount:1,bindGroupLayouts:&b.layout,..Default::default()});
  if b.pipeline_layout.is_null(){return Err("Motion bank pipeline layout allocation failed".into());}
  let wgsl=WGPUShaderSourceWGSL{chain:WGPUChainedStruct{next:ptr::null(),sType:WGPUSType_ShaderSourceWGSL},code:view(SHADER)};
  b.shader=(a.wgpuDeviceCreateShaderModule)(c.device,&WGPUShaderModuleDescriptor{
   nextInChain:&wgsl.chain,label:view("Original foreground texel playback, live camera holes")});
  if b.shader.is_null(){return Err("Motion bank shader allocation failed".into());}
  let color=WGPUColorTargetState{format:c.format,writeMask:WGPUColorWriteMask_All,..Default::default()};
  let fragment=WGPUFragmentState{module:b.shader,entryPoint:view("fs_motion_bank"),targetCount:1,targets:&color,..Default::default()};
  let depth=WGPUDepthStencilState{format:WGPUTextureFormat_Depth32Float,depthWriteEnabled:WGPUOptionalBool_True,
   depthCompare:WGPUCompareFunction_Less,..Default::default()};
  b.pipeline=(a.wgpuDeviceCreateRenderPipeline)(c.device,&WGPURenderPipelineDescriptor{
   layout:b.pipeline_layout,vertex:WGPUVertexState{module:b.shader,entryPoint:view("vs_motion_bank"),..Default::default()},
   primitive:WGPUPrimitiveState{topology:WGPUPrimitiveTopology_TriangleList,frontFace:WGPUFrontFace_CCW,cullMode:WGPUCullMode_None,..Default::default()},
   multisample:WGPUMultisampleState{count:1,mask:u32::MAX,..Default::default()},fragment:&fragment,depthStencil:&depth,..Default::default()});
  if b.pipeline.is_null(){return Err("Motion bank playback pipeline allocation failed".into());}
  check_errors()?;Ok(b)
 }

 /// CPU-only observation is safe while a prior frame is in flight. The caller
 /// admits only complete standard optical packets; no live lanes are exempted.
 pub(super) fn observe(&mut self,rows:&[[f32;4]]){
  self.observations+=1;
  let position=normalized_playhead(rows);
  self.dynamic_playhead=match(self.playhead,position){(Some(a),Some(b))=>(a-b).abs()>1.0e-5,_=>false};
  self.playhead=position;
  let supported=rows.len()==super::super::workstation::FRAME_ROWS
   &&crate::workstation::transport_pose::retained_camera(rows)
   &&crate::workstation::deferred_floor::retained_geometry_supported(rows)
   &&rows[8][0]==self.width as f32&&rows[8][1]==self.height as f32&&rows[9][2]>0.5;
  self.latest=if supported{Key::new(self.epoch,rows,super::super::workstation::FRAME_ROWS)}else{None};
  if self.latest.is_none(){self.rejected+=1;}
 }
 pub(super) fn ready_slot(&self)->Option<usize>{
  let key=self.latest.as_ref()?;
  self.entries.iter().position(|e|e.ready&&&e.key==key)
 }
 /// Only changes admission state. It releases no resource and can run during
 /// an in-flight frame. Upstream may exempt ONLY proven separate camera RGB.
 pub(super) fn invalidate(&mut self){
  self.epoch=self.epoch.wrapping_add(1);self.latest=None;self.invalidations+=1;
  for entry in &mut self.entries{entry.ready=false;}
 }
 pub(super) fn dimensions_match(&self,width:u32,height:u32,format:WGPUTextureFormat)->bool{
  self.width==width&&self.height==height&&self.format==format
 }

 /// Call ONLY after prior GPU work drained and the current original optical
 /// capture was encoded (or already completed for precisely this packet).
 /// `eligible` must certify camera_split::retained_batch, supported projection,
 /// immutable resources, no extra foreground/water/vegetation and pass-through.
 /// Source pipeline/group are ORIGINAL p.render/p.bind_group; using the full
 /// fs_retained preserves even live-meter/surface branches at the stored pose.
 /// Shared depth may be borrowed because this pass clears it and the ordinary
 /// subsequent surface pass clears it again before drawing the live camera.
 pub(super) unsafe fn encode_capture(&mut self,encoder:WGPUCommandEncoder,depth_view:WGPUTextureView,
   pipeline:WGPURenderPipeline,group:WGPUBindGroup,count:u32,camera_vertices:u32,eligible:bool,overlay:WGPURenderPipeline,regions:Option<&[super::meter_regions::Rect]>)->Result<bool,String>{
  if !eligible||count<=6||camera_vertices!=count-6||pipeline.is_null()||group.is_null()||depth_view.is_null(){
   self.rejected+=1;return Ok(false);
  }
  if self.ready_slot().is_some()||self.pending.is_some(){return Ok(false);}
  // Admission only: changing live content stays on the original render path.
  // Do not spend an extra full compositor pass or evict useful states for a
  // moving playhead. This never loosens full-bit equality for cache playback.
  if self.dynamic_playhead{self.admission_skips+=1;return Ok(false);}
  let Some(key)=self.latest.clone() else{return Ok(false);};
  // This function is called only after the previous GPU submission drained.
  // Reuse invalid or least-recently-used storage rather than filling the bank
  // permanently after the first edit. No in-flight texture can be overwritten.
  let can_grow=admits_allocation(self.allocated_bytes,self.entries.len(),self.bytes_per_state);
  let reuse=recycle_slot(&self.entries.iter().map(|e|(e.ready,e.last_used)).collect::<Vec<_>>(),can_grow);
  let a=self.api;
  let mut entry=if let Some(slot)=reuse {
   self.recycled+=1;let mut e=self.entries.remove(slot);e.key=key;e.ready=false;e.submission=0;e.last_used=self.observations;e
  } else {
  if !can_grow{self.capacity_misses+=1;return Ok(false);}
  let mut entry=Entry{api:a,key,texture:ptr::null_mut(),view:ptr::null_mut(),group:ptr::null_mut(),ready:false,submission:0,last_used:self.observations};
  entry.texture=(a.wgpuDeviceCreateTexture)(self.device,&WGPUTextureDescriptor{
   label:view("Original foreground at one canonical pose"),usage:WGPUTextureUsage_RenderAttachment|WGPUTextureUsage_TextureBinding|WGPUTextureUsage_CopySrc,
   dimension:WGPUTextureDimension_2D,size:WGPUExtent3D{width:self.width,height:self.height,depthOrArrayLayers:1},
   format:self.format,mipLevelCount:1,sampleCount:1,..Default::default()});
  if entry.texture.is_null(){return Err("Motion bank foreground allocation failed".into());}
  entry.view=(a.wgpuTextureCreateView)(entry.texture,ptr::null());
  if entry.view.is_null(){return Err("Motion bank foreground view failed".into());}
  let binding=WGPUBindGroupEntry{binding:0,textureView:entry.view,..Default::default()};
  entry.group=(a.wgpuDeviceCreateBindGroup)(self.device,&WGPUBindGroupDescriptor{
   label:view("One canonical foreground texture"),layout:self.layout,entryCount:1,entries:&binding,..Default::default()});
  if entry.group.is_null(){return Err("Motion bank foreground binding failed".into());}
  check_errors()?;entry
  };
  let color=WGPURenderPassColorAttachment{view:entry.view,depthSlice:u32::MAX,loadOp:WGPULoadOp_Clear,storeOp:WGPUStoreOp_Store,
   clearValue:WGPUColor{r:0.,g:0.,b:0.,a:0.},..Default::default()};
  let depth=WGPURenderPassDepthStencilAttachment{view:depth_view,depthLoadOp:WGPULoadOp_Clear,
   depthStoreOp:WGPUStoreOp_Store,depthClearValue:1.,..Default::default()};
  let pass=(a.wgpuCommandEncoderBeginRenderPass)(encoder,&WGPURenderPassDescriptor{
   label:view("Store original optical foreground, no camera"),colorAttachmentCount:1,colorAttachments:&color,
   depthStencilAttachment:&depth,..Default::default()});
  if pass.is_null(){return Err("Motion bank foreground capture pass failed".into());}
  (a.wgpuRenderPassEncoderSetPipeline)(pass,pipeline);
  (a.wgpuRenderPassEncoderSetBindGroup)(pass,0,group,0,ptr::null());
  (a.wgpuRenderPassEncoderDraw)(pass,6,1,count-6,0);
  // Reuse the actual production fragment entries and their split-pass structure.
  if !overlay.is_null(){if let Some(regions)=regions{
   (a.wgpuRenderPassEncoderSetPipeline)(pass,overlay);
   for r in regions{(a.wgpuRenderPassEncoderSetScissorRect)(pass,r.x,r.y,r.width,r.height);(a.wgpuRenderPassEncoderDraw)(pass,6,1,count-6,0);}
   (a.wgpuRenderPassEncoderSetScissorRect)(pass,0,0,self.width,self.height);
  }}
  (a.wgpuRenderPassEncoderEnd)(pass);(a.wgpuRenderPassEncoderRelease)(pass);
  let slot=self.entries.len();self.entries.push(entry);if reuse.is_none(){self.allocated_bytes+=self.bytes_per_state;}
  self.pending=Some(Pending{slot,serial:None});self.captures+=1;
  Ok(true)
 }
 pub(super) fn submitted(&mut self,serial:u64)->Result<(),String>{
  let pending=self.pending.as_mut().ok_or("Motion bank submission has no encoded capture")?;
  if pending.serial.is_some()||serial==0{return Err("Motion bank capture submitted twice or with zero serial".into());}
  pending.serial=Some(serial);self.entries[pending.slot].submission=serial;Ok(())
 }
 /// Publication requires an actual completed serial, not merely encoded work.
 pub(super) fn completed(&mut self,completed:u64){
  if !self.pending.as_ref().is_some_and(|p|p.serial.is_some_and(|s|s<=completed)){return;}
  let pending=self.pending.take().unwrap();let entry=&mut self.entries[pending.slot];
  if entry.key.epoch==self.epoch{entry.ready=true;self.published+=1;}
 }
 /// Camera must already have drawn into this pass with its live geometry/color.
 /// Draw only the currently matching completed pose; false means original fallback.
 pub(super) unsafe fn draw(&mut self,pass:WGPURenderPassEncoder,slot:usize)->bool{
  if self.ready_slot()!=Some(slot){return false;}
  let a=self.api;(a.wgpuRenderPassEncoderSetPipeline)(pass,self.pipeline);
  (a.wgpuRenderPassEncoderSetBindGroup)(pass,0,self.entries[slot].group,0,ptr::null());
  (a.wgpuRenderPassEncoderDraw)(pass,3,1,0,0);self.draws+=1;self.entries[slot].last_used=self.observations;true
 }
 pub(super) fn info(&self)->Value{json!({
  "enabled":true,"states":self.entries.len(),"ready_states":self.entries.iter().filter(|e|e.ready).count(),
  "selected_slot":self.ready_slot(),"pending_slot":self.pending.as_ref().map(|p|p.slot),
  "pending_serial":self.pending.as_ref().and_then(|p|p.serial),"max_states":MAX_STATES,"cap_bytes":CAP_BYTES,
  "texture_bytes":self.allocated_bytes,"texture_bytes_per_state":self.bytes_per_state,"format":self.format,
  "cpu_key_bytes":self.entries.iter().map(|e|e.key.rows.len()*16).sum::<usize>(),
  "observations":self.observations,"playback_draws":self.draws,"capture_draws":self.captures,
  "dynamic_playhead_admission_skips":self.admission_skips,"recycled_slots":self.recycled,"published":self.published,"external_invalidations":self.invalidations,"capacity_misses":self.capacity_misses,"rejections":self.rejected,
  "scope":"Bounded complete-state reuse; original two-sample/AgX foreground captured in exact surface format, transparent camera holes, unfiltered replay at original fullscreen depth. All packet bits and external epoch keyed. No per-frame decompress/upload; camera is separately live. Unknown states use original renderer. Texture bytes are requested texel storage, not driver residency. This includes live compositor content in the key and is not a general arbitrary-interaction cache."
 })}
}
impl Drop for Bank {fn drop(&mut self){self.entries.clear();unsafe{
 let a=self.api;if !self.pipeline.is_null(){(a.wgpuRenderPipelineRelease)(self.pipeline);}
 if !self.shader.is_null(){(a.wgpuShaderModuleRelease)(self.shader);}
 if !self.pipeline_layout.is_null(){(a.wgpuPipelineLayoutRelease)(self.pipeline_layout);}
 if !self.layout.is_null(){(a.wgpuBindGroupLayoutRelease)(self.layout);}
}}}

#[cfg(test)] mod tests {
 use super::*;
 #[test]fn playhead_admission_tracks_time_not_panel_geometry(){
  let mut r=vec![[0.;4];253];r[250]=[-1.,1.,0.,0.];r[252]=[0.,0.,0.,1.];assert_eq!(normalized_playhead(&r),Some(0.5));
  r[250]=[-0.5,1.5,0.,0.];r[252][0]=0.5;assert_eq!(normalized_playhead(&r),Some(0.5));
  r[252][0]=0.6;assert!(normalized_playhead(&r).unwrap()>0.54);
  r[252][3]=0.;assert_eq!(normalized_playhead(&r),None);
 }
 #[test]fn invalid_entries_recycle_before_lru_and_full_banks_remain_usable(){
  assert_eq!(recycle_slot(&[(true,8),(false,10),(true,1)],true),Some(1));
  assert_eq!(recycle_slot(&[(true,8),(true,10),(true,1)],true),None);
  assert_eq!(recycle_slot(&[(true,8),(true,10),(true,1)],false),Some(2));
  for epoch in 0..100 {let e=vec![(false,epoch);32];assert_eq!(recycle_slot(&e,false),Some(0));}
 }
 #[test]fn strict_keys_include_live_values_signed_zero_and_external_epoch(){
  let a=vec![[0.;4];3];let k=Key::new(0,&a,3).unwrap();
  for row in 0..3{for lane in 0..4{let mut b=a.clone();b[row][lane]=0.5;assert_ne!(Some(&k),Key::new(0,&b,3).as_ref());}}
  let mut b=a.clone();b[0][0]=-0.;assert_ne!(Some(&k),Key::new(0,&b,3).as_ref());
  assert_ne!(Some(&k),Key::new(1,&a,3).as_ref());assert_eq!(Some(&k),Key::new(0,&a,3).as_ref());
 }
 #[test]fn incomplete_and_nonfinite_packets_fail_closed(){
  assert!(Key::new(0,&[[0.;4];2],3).is_none());
  for v in [f32::NAN,f32::INFINITY,f32::NEG_INFINITY]{assert!(Key::new(0,&[[v,0.,0.,0.]],1).is_none());}
 }
 #[test]fn byte_and_count_caps_are_independent_and_overflow_safe(){
  assert_eq!(texture_bytes(1920,1200),Some(9_216_000));
  assert!(admits_allocation(31*9_216_000,31,9_216_000));assert!(!admits_allocation(0,32,1));
  assert!(!admits_allocation(CAP_BYTES,0,1));assert!(!admits_allocation(u64::MAX,0,1));
  assert!(texture_bytes(u32::MAX,u32::MAX).is_none());assert!(texture_bytes(0,1200).is_none());
 }
 #[test]fn playback_shader_keeps_original_opaque_mask_depth_and_single_texel(){
  let module=naga::front::wgsl::parse_str(SHADER).unwrap();
  naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&module).unwrap();
  assert_eq!(SHADER.matches("textureLoad(").count(),1);
  assert!(SHADER.contains("vec4<f32>(p[i],0.5,1.0)"));assert!(SHADER.contains("if(color.a==0.0){discard;}"));
  assert!(!SHADER.contains("gemDisplay"));assert!(!SHADER.contains("textureSample"));
 }
}
