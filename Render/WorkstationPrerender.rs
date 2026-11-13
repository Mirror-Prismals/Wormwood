//! Legacy optical research alternative, selected explicitly with REFERENCE_OPTICS.
//! Production DAW uses WorkstationBaked: never re-enable this refresh policy
//! to improve screenshot equivalence. The author accepts fixed baked lighting.
use super::*;
#[path="WorkstationPrerenderSource.rs"] mod source;
#[path="WorkstationPrerenderRefresh.rs"] mod refresh;
#[path="WorkstationPrerenderAtlas.rs"] mod atlas;
#[path="WorkstationIncrementalBake.rs"] mod incremental;
#[path="WorkstationRetainedContent.rs"] mod content;
#[path="WorkstationAtlasExport.rs"] mod atlas_export;
#[path="WorkstationRayQueue.rs"] mod ray_queue;
#[path="WorkstationCaptureGroup.rs"] mod capture_group;
#[path="WorkstationGroupDispatch.rs"] mod group_dispatch;
#[path="WorkstationBarPlanes.rs"]mod bar_planes;
#[path="WorkstationCaptureSplit.rs"]mod capture_split;
#[path="WorkstationCaptureAttribution.rs"]mod capture_attribution;
const GROUPS:u32=6;
#[path="WorkstationFurniture.rs"] mod furniture;
const META_ROWS:usize=furniture::META_ROWS;
pub(super) struct Pipeline {
 furniture_images:Option<furniture::Images>,
 atlas_export:Option<atlas_export::Export>,
 pub(super) ledger:Option<super::meter_ledger_runtime::Runtime>,
 api:&'static Api,queue:WGPUQueue,
 offline_faders:bool,detail_buffer:WGPUBuffer,history_view:WGPUTextureView,fade_start:Option<Instant>,
 device:WGPUDevice,compact:bool,dimension_limit:u32,atlas_extent:[u32;2],atlas_layers:u32,packing:Option<atlas::Layout>,reservation:atlas::Reservation,atlas_reallocations:u64,atlas_peak_replacement_bytes:u64,
 compute_entries:Vec<WGPUBindGroupEntry>,render_entries:Vec<WGPUBindGroupEntry>,
 shader:WGPUShaderModule,
 compute:WGPUComputePipeline,
 capture_compute:WGPUComputePipeline,
 pub render:WGPURenderPipeline,
 pub camera_render:WGPURenderPipeline,
 pub meter_base_render:WGPURenderPipeline,pub meter_overlay_render:WGPURenderPipeline,
 pub meter_regions:Option<Vec<super::meter_regions::Rect>>,
 pub final_safe:bool,
 pub bind_group:WGPUBindGroup,
 layout:WGPUBindGroupLayout,
 compute_layout:WGPUBindGroupLayout,
 compute_pipeline_layout:WGPUPipelineLayout,
 compute_group:WGPUBindGroup,
 group_dispatch:Option<group_dispatch::Bindings>,
 capture_split:Option<capture_split::Runtime>,
 capture_attribution:Option<capture_attribution::Runtime>,
 density_zero_guard:bool,
 zero_alpha_light:bool,
 primary_group_gates:bool,
 primary_witness:bool,
 primary_witness_version:u32,
 pipeline_layout:WGPUPipelineLayout,
 texture:WGPUTexture,
 texture_view:WGPUTextureView,
 depth_texture:WGPUTexture,depth_view:WGPUTextureView,depth_first:bool,
 metadata:WGPUBuffer,metadata_bytes:u64,bar_planes:Option<bar_planes::Runtime>,
 width:u32,height:u32,capture_groups:u32,transport_layers:bool,
 rows:[[f32;4];META_ROWS],
 previous:Vec<[f32;4]>,
 captured:Vec<[f32;4]>,
 full_anchor:Vec<[f32;4]>,capture_mask:u32,partial_captures:u64,captured_groups:u64,migration:Option<[[f32;4];META_ROWS]>,repack_pending:bool,
 pub incremental_enabled:bool,
 content_refreshes:u64,
 zero_tile:WGPUBuffer,incremental_pipelines:[WGPUComputePipeline;3],generation:Option<incremental::Generation>,
 stage_requested:bool,stage_idle:bool,next_generation:u64,staged_cancelled:u64,staged_published:u64,staged_peak_extra_bytes:u64,staged_convergence_ms:Vec<f64>,
 pub prepared:bool,
 pub pending:bool,
 pub supported:bool,
 pub bakes:u64,
 pub held_frames:u64,
 pub bake_wall_ms:Vec<f64>,
 refresh:refresh::Refresh,
 idle_refresh_pending:bool,
 idle_refreshes:u64,
}
fn panel_capture_rect(t:[f32;4],width:u32,height:u32)->[f32;4] {
 let mut r=[(t[0]-t[2]*0.5-3.).floor(),(t[1]-t[3]*0.5-3.).floor(),(t[2]+8.).ceil(),(t[3]+8.).ceil()];
 for axis in 0..2 {let limit=[width,height][axis] as f32;if r[axis+2]>limit && t[axis+2]<=limit+0.01 {r[axis]=0.;r[axis+2]=limit;}}
 r
}

impl Pipeline {
 pub(super) fn final_dependencies(&self)->Vec<u64>{
  let mut key=vec![self.render as usize as u64,self.bind_group as usize as u64,self.bakes,self.texture as usize as u64,self.texture_view as usize as u64,self.metadata as usize as u64];
  key.extend(self.rows.iter().flatten().map(|v|v.to_bits() as u64));
  // The optional detail transition updates an auxiliary uniform each draw.
  // Its changing weight must never reuse a previous final-color image.
  if let Some(start)=self.fade_start {key.push(start.elapsed().as_nanos() as u64);}
  for e in &self.render_entries{key.extend([e.binding as u64,e.buffer as usize as u64,e.offset,e.size,e.textureView as usize as u64]);}key
 }
 pub(super) fn diagnostic_bindings(&self)->(WGPUBindGroupLayout,Vec<WGPUBindGroupEntry>){(self.layout,self.render_entries.clone())}
 pub unsafe fn new(c:&Client,original:&str,response:&RubyResponsePipeline,dimension_limit:u32)->Result<Self,String>{
  let api=c.api;let merge_static=crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_MERGE_STATIC");let depth_first=merge_static&&std::env::var("WORMWOOD_WORKSTATION_DEPTH_FIRST").as_deref()==Ok("1");let transport_layers=merge_static&&!depth_first;let capture_groups=if merge_static&&!transport_layers{GROUPS-1}else{GROUPS};let compact=crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_COMPACT_ATLAS");let ledger_requested=super::meter_ledger_runtime::Runtime::requested()&&compact&&merge_static&&!depth_first&&c.frozen_mode==0&&!c.final_mode;
  let furniture_enabled=compact&&transport_layers&&c.frozen_mode==0&&!c.final_mode&&c.width as u64*c.height as u64<=2_500_000
      &&crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_COMPONENT_MOTION");
  let ledger_requested=ledger_requested&&!furniture_enabled;
  let mut s=source::source(original,merge_static,depth_first,compact)?;
  if furniture_enabled{s=furniture::source(&s)?;}
  let detail=compact && c.incremental_mode && std::env::var("WORMWOOD_PANEL_DETAIL").as_deref()==Ok("1");
  if detail{s=source::panel_detail(&s,original)?;}
  if ledger_requested{s=source::with_meter_ledger(&s)?;}
  let split_source=capture_split::requested().then(||s.clone());
  if c.incremental_mode{if !compact||!merge_static||depth_first||std::env::var("WORMWOOD_WORKSTATION_PIXEL_TRANSPORT").as_deref()==Ok("1"){return Err("incremental bake requires merged compact atlas and intrinsic response mode".into());}s=incremental::tiled_source(&s)?;}
  let atlas_extent=if compact{[1,1]}else{[c.width,c.height]};let atlas_layers=if compact{2}else{capture_groups*2};
  let mut p=Self{furniture_images:None,offline_faders:compact && source::offline_faders_enabled(),detail_buffer:ptr::null_mut(),history_view:ptr::null_mut(),fade_start:None,atlas_export:atlas_export::Export::selected(),ledger:None,api,queue:c.queue,device:c.device,compact,dimension_limit,atlas_extent,atlas_layers,packing:None,reservation:atlas::Reservation::default(),atlas_reallocations:0,atlas_peak_replacement_bytes:atlas_extent[0] as u64*atlas_extent[1] as u64*atlas_layers as u64*16,compute_entries:vec![],render_entries:vec![],shader:ptr::null_mut(),compute:ptr::null_mut(),capture_compute:ptr::null_mut(),render:ptr::null_mut(),camera_render:ptr::null_mut(),meter_base_render:ptr::null_mut(),meter_overlay_render:ptr::null_mut(),meter_regions:None,final_safe:!c.final_mode||source::diagnostic_call_graph(&s).is_ok(),bind_group:ptr::null_mut(),layout:ptr::null_mut(),compute_layout:ptr::null_mut(),compute_pipeline_layout:ptr::null_mut(),compute_group:ptr::null_mut(),group_dispatch:None,capture_split:None,capture_attribution:None,density_zero_guard:s.contains("DENSITY_ZERO_CERTIFICATE_V1"),zero_alpha_light:s.contains("ZERO_ALPHA_IO_LIGHT_V1"),primary_group_gates:s.contains("// PRIMARY_GROUP_GATES_V1"),primary_witness:s.contains("// PRIMARY_WITNESS_V1"),primary_witness_version:if s.contains("// PRIMARY_WITNESS_GENERIC_ONLY_V3"){3}else if s.contains("// PRIMARY_WITNESS_TIMELINE_PREFIX_V2"){2}else if s.contains("// PRIMARY_WITNESS_V1"){1}else{0},pipeline_layout:ptr::null_mut(),texture:ptr::null_mut(),texture_view:ptr::null_mut(),depth_texture:ptr::null_mut(),depth_view:ptr::null_mut(),depth_first,metadata:ptr::null_mut(),metadata_bytes:bar_planes::metadata_bytes(bar_planes::requested()).max((META_ROWS*16)as u64),bar_planes:None,width:c.width,height:c.height,capture_groups,transport_layers,rows:[[0.;4];META_ROWS],previous:vec![],captured:vec![],full_anchor:vec![],capture_mask:0,partial_captures:0,captured_groups:0,migration:None,repack_pending:false,incremental_enabled:c.incremental_mode,content_refreshes:0,zero_tile:ptr::null_mut(),incremental_pipelines:[ptr::null_mut();3],generation:None,stage_requested:false,stage_idle:false,next_generation:1,staged_cancelled:0,staged_published:0,staged_peak_extra_bytes:0,staged_convergence_ms:vec![],prepared:false,pending:false,supported:false,bakes:0,held_frames:0,bake_wall_ms:vec![],refresh:refresh::Refresh::default(),idle_refresh_pending:false,idle_refreshes:0};
  if ledger_requested {p.ledger=Some(super::meter_ledger_runtime::Runtime::new(c)?);}
  if furniture_enabled{p.furniture_images=Some(furniture::Images::new(api,c.device,atlas_extent)?);}
  p.texture=(api.wgpuDeviceCreateTexture)(c.device,&WGPUTextureDescriptor{label:view("Original per-sample component radiance"),usage:WGPUTextureUsage_StorageBinding|WGPUTextureUsage_TextureBinding|WGPUTextureUsage_CopySrc|WGPUTextureUsage_CopyDst|atlas_export::copy_usage(),dimension:WGPUTextureDimension_2D,size:WGPUExtent3D{width:atlas_extent[0],height:atlas_extent[1],depthOrArrayLayers:atlas_layers},format:WGPUTextureFormat_RGBA32Float,mipLevelCount:1,sampleCount:1,..Default::default()});
  p.texture_view=(api.wgpuTextureCreateView)(p.texture,&WGPUTextureViewDescriptor{dimension:WGPUTextureViewDimension_2DArray,arrayLayerCount:atlas_layers,mipLevelCount:1,aspect:WGPUTextureAspect_All,..Default::default()});
  if depth_first {
   p.depth_texture=(api.wgpuDeviceCreateTexture)(c.device,&WGPUTextureDescriptor{label:view("Exact retained depth for visibility selection"),usage:WGPUTextureUsage_StorageBinding|WGPUTextureUsage_TextureBinding|WGPUTextureUsage_CopySrc|WGPUTextureUsage_CopyDst|atlas_export::copy_usage(),dimension:WGPUTextureDimension_2D,size:WGPUExtent3D{width:c.width,height:c.height,depthOrArrayLayers:capture_groups*2},format:WGPUTextureFormat_R32Float,mipLevelCount:1,sampleCount:1,..Default::default()});
   p.depth_view=(api.wgpuTextureCreateView)(p.depth_texture,&WGPUTextureViewDescriptor{dimension:WGPUTextureViewDimension_2DArray,arrayLayerCount:capture_groups*2,mipLevelCount:1,aspect:WGPUTextureAspect_All,..Default::default()});
  }
  p.metadata=(api.wgpuDeviceCreateBuffer)(c.device,&WGPUBufferDescriptor{label:view("Retained component transforms"),size:p.metadata_bytes,usage:WGPUBufferUsage_Storage|WGPUBufferUsage_CopyDst|if c.frozen_mode!=0{WGPUBufferUsage_CopySrc}else{0},..Default::default()});
  let buffers=[c.vertex_buffer,c.texture_buffer,response.response_buffer,response.plan_buffer];
  let sizes=[crate::scene_limits::SCENE_BUFFER_BYTES,c.texture_capacity as u64,crate::workstation::ruby_response_plan::CACHE_BYTES as u64,(crate::workstation::ruby_response_plan::PLAN_ROWS*16) as u64];
  let mut ls=vec![];let mut bs=vec![];
  for i in 0..4 {
   ls.push(WGPUBindGroupLayoutEntry{binding:i as u32,visibility:if i==0{WGPUShaderStage_Vertex}else{WGPUShaderStage_Compute|WGPUShaderStage_Fragment},buffer:WGPUBufferBindingLayout{type_:if i==2{WGPUBufferBindingType_Storage}else{WGPUBufferBindingType_ReadOnlyStorage},minBindingSize:sizes[i],..Default::default()},..Default::default()});
   bs.push(WGPUBindGroupEntry{binding:i as u32,buffer:buffers[i],size:sizes[i],..Default::default()});
  }
  ls.push(WGPUBindGroupLayoutEntry{binding:4,visibility:WGPUShaderStage_Compute,storageTexture:WGPUStorageTextureBindingLayout{access:WGPUStorageTextureAccess_WriteOnly,format:WGPUTextureFormat_RGBA32Float,viewDimension:WGPUTextureViewDimension_2DArray,..Default::default()},..Default::default()});
  ls.push(WGPUBindGroupLayoutEntry{binding:5,visibility:WGPUShaderStage_Fragment,texture:WGPUTextureBindingLayout{sampleType:WGPUTextureSampleType_UnfilterableFloat,viewDimension:WGPUTextureViewDimension_2DArray,multisampled:0,..Default::default()},..Default::default()});
  ls.push(WGPUBindGroupLayoutEntry{binding:6,visibility:WGPUShaderStage_Compute|WGPUShaderStage_Fragment,buffer:WGPUBufferBindingLayout{type_:WGPUBufferBindingType_ReadOnlyStorage,minBindingSize:(META_ROWS*16) as u64,..Default::default()},..Default::default()});
  bs.push(WGPUBindGroupEntry{binding:4,textureView:p.texture_view,..Default::default()});bs.push(WGPUBindGroupEntry{binding:5,textureView:p.texture_view,..Default::default()});bs.push(WGPUBindGroupEntry{binding:6,buffer:p.metadata,size:p.metadata_bytes,..Default::default()});
  if depth_first {
   ls.push(WGPUBindGroupLayoutEntry{binding:7,visibility:WGPUShaderStage_Compute,storageTexture:WGPUStorageTextureBindingLayout{access:WGPUStorageTextureAccess_WriteOnly,format:WGPUTextureFormat_R32Float,viewDimension:WGPUTextureViewDimension_2DArray,..Default::default()},..Default::default()});
   ls.push(WGPUBindGroupLayoutEntry{binding:8,visibility:WGPUShaderStage_Fragment,texture:WGPUTextureBindingLayout{sampleType:WGPUTextureSampleType_UnfilterableFloat,viewDimension:WGPUTextureViewDimension_2DArray,multisampled:0,..Default::default()},..Default::default()});
   bs.push(WGPUBindGroupEntry{binding:7,textureView:p.depth_view,..Default::default()});bs.push(WGPUBindGroupEntry{binding:8,textureView:p.depth_view,..Default::default()});
  }
  if p.incremental_enabled{
   p.zero_tile=(api.wgpuDeviceCreateBuffer)(c.device,&WGPUBufferDescriptor{label:view("Zero bake dispatch offset"),size:16,usage:WGPUBufferUsage_Uniform|WGPUBufferUsage_CopyDst,..Default::default()});
   if p.zero_tile.is_null(){return Err("incremental dispatch buffer allocation failed".into());}
   let zero=[0u32;4];(api.wgpuQueueWriteBuffer)(c.queue,p.zero_tile,0,zero.as_ptr().cast(),16);
   ls.push(WGPUBindGroupLayoutEntry{binding:7,visibility:WGPUShaderStage_Compute,buffer:WGPUBufferBindingLayout{type_:WGPUBufferBindingType_Uniform,minBindingSize:16,..Default::default()},..Default::default()});
   bs.push(WGPUBindGroupEntry{binding:7,buffer:p.zero_tile,size:16,..Default::default()});
  }
  if let Some(images)=&p.furniture_images{images.bindings(&mut ls,&mut bs);}
  let compute_ls:Vec<_>=ls.iter().copied().filter(|e|e.binding!=5&&e.binding!=8&&e.binding!=19&&e.binding!=21).collect();
  let compute_bs:Vec<_>=bs.iter().copied().filter(|e|e.binding!=5&&e.binding!=8&&e.binding!=19&&e.binding!=21).collect();
  p.compute_layout=(api.wgpuDeviceCreateBindGroupLayout)(c.device,&WGPUBindGroupLayoutDescriptor{label:view("Retained bake write-only bindings"),entryCount:compute_ls.len(),entries:compute_ls.as_ptr(),..Default::default()});
  p.compute_pipeline_layout=(api.wgpuDeviceCreatePipelineLayout)(c.device,&WGPUPipelineLayoutDescriptor{bindGroupLayoutCount:1,bindGroupLayouts:&p.compute_layout,..Default::default()});
  p.compute_group=(api.wgpuDeviceCreateBindGroup)(c.device,&WGPUBindGroupDescriptor{layout:p.compute_layout,entryCount:compute_bs.len(),entries:compute_bs.as_ptr(),..Default::default()});
  ls.retain(|e|e.binding!=4&&e.binding!=7&&e.binding!=18&&e.binding!=20);bs.retain(|e|e.binding!=4&&e.binding!=7&&e.binding!=18&&e.binding!=20);
  if detail {
   p.detail_buffer=(api.wgpuDeviceCreateBuffer)(c.device,&WGPUBufferDescriptor{label:view("Panel lighting transition"),size:208,usage:WGPUBufferUsage_Uniform|WGPUBufferUsage_CopyDst,..Default::default()});
   if p.detail_buffer.is_null(){return Err("panel lighting uniform allocation failed".into());}
   ls.push(WGPUBindGroupLayoutEntry{binding:16,visibility:WGPUShaderStage_Fragment,texture:WGPUTextureBindingLayout{sampleType:WGPUTextureSampleType_UnfilterableFloat,viewDimension:WGPUTextureViewDimension_2DArray,..Default::default()},..Default::default()});
   ls.push(WGPUBindGroupLayoutEntry{binding:17,visibility:WGPUShaderStage_Fragment,buffer:WGPUBufferBindingLayout{type_:WGPUBufferBindingType_Uniform,minBindingSize:208,..Default::default()},..Default::default()});
   bs.push(WGPUBindGroupEntry{binding:16,textureView:p.texture_view,..Default::default()});
   bs.push(WGPUBindGroupEntry{binding:17,buffer:p.detail_buffer,size:208,..Default::default()});
  }
  if let Some(ledger)=&p.ledger{ledger.render_bindings(&mut ls,&mut bs);}
  p.layout=(api.wgpuDeviceCreateBindGroupLayout)(c.device,&WGPUBindGroupLayoutDescriptor{label:view("Retained original appearance bindings"),entryCount:ls.len(),entries:ls.as_ptr(),..Default::default()});
  p.pipeline_layout=(api.wgpuDeviceCreatePipelineLayout)(c.device,&WGPUPipelineLayoutDescriptor{bindGroupLayoutCount:1,bindGroupLayouts:&p.layout,..Default::default()});
  p.bind_group=(api.wgpuDeviceCreateBindGroup)(c.device,&WGPUBindGroupDescriptor{layout:p.layout,entryCount:bs.len(),entries:bs.as_ptr(),..Default::default()});
  p.compute_entries=compute_bs;p.render_entries=bs;
  if let Some(split_source)=split_source{p.capture_split=Some(capture_split::Runtime::new(c,&split_source,&compute_ls)?);}
  if bar_planes::requested(){p.bar_planes=Some(bar_planes::Runtime::new(c,p.metadata,original)?);}
  if p.incremental_enabled&&group_dispatch::requested(){
   p.group_dispatch=Some(group_dispatch::Bindings::new(api,c.device,c.queue,p.compute_layout,&p.compute_entries)?);
  }
  if c.frozen_mode!=0 {
   let graph=source::diagnostic_call_graph(&s)?;
   std::fs::write(c.capture.with_file_name("frozen-fragment-callgraph.json"),serde_json::to_vec_pretty(&graph).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
   std::fs::write(c.capture.with_file_name("frozen-retained.wgsl"),&s).map_err(|e|e.to_string())?;
  }
  let camera_split=crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_CAMERA_SPLIT");
  if camera_split{s.push_str(super::camera_split::ENTRY);}
  let meter_split=crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_METER_SPLIT")&&!depth_first;
  if meter_split{source::append_meter_base(&mut s)?;}
  s=ray_queue::selected(&s)?;
  let small_capture=capture_group::append(&mut s)?;
  let wgsl=WGPUShaderSourceWGSL{chain:WGPUChainedStruct{next:ptr::null(),sType:WGPUSType_ShaderSourceWGSL},code:view(&s)};
  p.shader=(api.wgpuDeviceCreateShaderModule)(c.device,&WGPUShaderModuleDescriptor{nextInChain:&wgsl.chain,label:view("Original radiance capture and retained composition")});
  if let Some(ledger)=&mut p.ledger{
   if let Some(capture_source)=source::meter_ledger_capture_source(&s,ledger.playhead_certificate())?{
    // Instrumentation may affect compiler arithmetic even when disabled by a
    // private flag. Only the ledger capture pipeline may see this module.
    let capture_wgsl=WGPUShaderSourceWGSL{chain:WGPUChainedStruct{next:ptr::null(),sType:WGPUSType_ShaderSourceWGSL},code:view(&capture_source)};
    let capture_shader=(api.wgpuDeviceCreateShaderModule)(c.device,&WGPUShaderModuleDescriptor{nextInChain:&capture_wgsl.chain,label:view("Isolated meter playback certificate capture")});
    if capture_shader.is_null(){return Err("meter certificate shader creation failed".into());}
    let result=ledger.create_pipeline(capture_shader,c.texture_capacity as u64,&capture_source);
    // Pipeline creation retains its shader dependency. Release our temporary
    // module on both success and error before propagating the result.
    (api.wgpuShaderModuleRelease)(capture_shader);
    result?;
   }else{ledger.create_pipeline(p.shader,c.texture_capacity as u64,&s)?;}
  }
  p.compute=(api.wgpuDeviceCreateComputePipeline)(c.device,&WGPUComputePipelineDescriptor{layout:p.compute_pipeline_layout,compute:WGPUProgrammableStageDescriptor{module:p.shader,entryPoint:view("cs_retained"),..Default::default()},..Default::default()});
  p.capture_attribution=capture_attribution::Runtime::new(c,&s,p.compute_pipeline_layout,small_capture||p.capture_split.is_some()||p.bar_planes.is_some())?;
  if small_capture{p.capture_compute=(api.wgpuDeviceCreateComputePipeline)(c.device,&WGPUComputePipelineDescriptor{layout:p.compute_pipeline_layout,compute:WGPUProgrammableStageDescriptor{module:p.shader,entryPoint:view("cs_retained_8x4"),..Default::default()},..Default::default()});if p.capture_compute.is_null(){return Err("alternate retained capture pipeline failed".into());}}
  if p.incremental_enabled{for (i,name) in ["cs_ruby_response","cs_ruby_response_general","cs_ruby_resolve"].iter().enumerate(){p.incremental_pipelines[i]=(api.wgpuDeviceCreateComputePipeline)(c.device,&WGPUComputePipelineDescriptor{layout:p.compute_pipeline_layout,compute:WGPUProgrammableStageDescriptor{module:p.shader,entryPoint:view(name),..Default::default()},..Default::default()});if p.incremental_pipelines[i].is_null(){return Err("incremental optical pipeline creation failed".into());}}}
  let target=WGPUColorTargetState{format:c.format,writeMask:WGPUColorWriteMask_All,..Default::default()};
  let fragment=WGPUFragmentState{module:p.shader,entryPoint:view("fs_retained"),targetCount:1,targets:&target,..Default::default()};
  let depth=WGPUDepthStencilState{format:WGPUTextureFormat_Depth32Float,depthWriteEnabled:WGPUOptionalBool_True,depthCompare:WGPUCompareFunction_Less,..Default::default()};
  p.render=(api.wgpuDeviceCreateRenderPipeline)(c.device,&WGPURenderPipelineDescriptor{layout:p.pipeline_layout,vertex:WGPUVertexState{module:p.shader,entryPoint:view("vs_main"),..Default::default()},primitive:WGPUPrimitiveState{topology:WGPUPrimitiveTopology_TriangleList,frontFace:WGPUFrontFace_CCW,cullMode:WGPUCullMode_None,..Default::default()},multisample:WGPUMultisampleState{count:1,mask:u32::MAX,..Default::default()},fragment:&fragment,depthStencil:&depth,..Default::default()});
  if camera_split {
   let fragment=WGPUFragmentState{module:p.shader,entryPoint:view("fs_camera_color"),targetCount:1,targets:&target,..Default::default()};
   p.camera_render=(api.wgpuDeviceCreateRenderPipeline)(c.device,&WGPURenderPipelineDescriptor{layout:p.pipeline_layout,vertex:WGPUVertexState{module:p.shader,entryPoint:view("vs_main"),..Default::default()},primitive:WGPUPrimitiveState{topology:WGPUPrimitiveTopology_TriangleList,frontFace:WGPUFrontFace_CCW,cullMode:WGPUCullMode_None,..Default::default()},multisample:WGPUMultisampleState{count:1,mask:u32::MAX,..Default::default()},fragment:&fragment,depthStencil:&depth,..Default::default()});
   if p.camera_render.is_null(){return Err("camera color pipeline creation failed".into());}
  }
  if meter_split {
   for(overlay,name)in[(false,"fs_retained_without_meters"),(true,"fs_retained")]{
    let fragment=WGPUFragmentState{module:p.shader,entryPoint:view(name),targetCount:1,targets:&target,..Default::default()};
    let depth=WGPUDepthStencilState{format:WGPUTextureFormat_Depth32Float,depthWriteEnabled:WGPUOptionalBool_True,depthCompare:if overlay{WGPUCompareFunction_LessEqual}else{WGPUCompareFunction_Less},..Default::default()};
    let pipeline=(api.wgpuDeviceCreateRenderPipeline)(c.device,&WGPURenderPipelineDescriptor{layout:p.pipeline_layout,vertex:WGPUVertexState{module:p.shader,entryPoint:view("vs_main"),..Default::default()},primitive:WGPUPrimitiveState{topology:WGPUPrimitiveTopology_TriangleList,frontFace:WGPUFrontFace_CCW,cullMode:WGPUCullMode_None,..Default::default()},multisample:WGPUMultisampleState{count:1,mask:u32::MAX,..Default::default()},fragment:&fragment,depthStencil:&depth,..Default::default()});
    if pipeline.is_null(){return Err("meter partition pipeline creation failed".into());}
    if overlay{p.meter_overlay_render=pipeline;}else{p.meter_base_render=pipeline;}
   }
  }
  c.wait()?;check_errors()?;Ok(p)
 }
 fn transform(&self,frame:&[[f32;4]],group:usize)->[f32;4]{
  let group=if self.transport_layers&&group==5{1}else{group};
  if !(1..=4).contains(&group){return [0.,0.,1.,1.];}
  let b=32+40*(group-1);let center=frame[b+2];let extent=frame[b+3];let w=frame[8][0];let h=frame[8][1];
  [(center[0]/frame[2][3]+1.)*w*0.5,(1.-center[1]/frame[3][3])*h*0.5,extent[0]/frame[2][3]*w,extent[1]/frame[3][3]*h]
 }
 fn panel_borders(rows:&mut [[f32;4];META_ROWS],captured:&[[f32;4]],current:&[[f32;4]]) {
  for g in 1..=4 {
   let border=|frame:&[[f32;4]]| {
    let unit=(frame[35+40*(g-1)][2]-0.000001).max(0.)/0.575;
    0.4*unit*frame[8][1]/(2.*frame[3][3])
   };
   rows[5+g*4][2]=border(captured);rows[5+g*4][3]=border(current);
  }
 }
 fn button_bounds(frame:&[[f32;4]],rows:&mut [[f32;4];META_ROWS]){
  // All four approved transport assets have exact local XY bounds +/-0.02m.
  // Capture the actual pose, even if a button is already depressed. Rows
  // 32..48 are group IDs; 49..52 bounds; 53..56 poses; 57 camera origin Z.
  debug_assert!(META_ROWS > 57);
  rows[57]=[frame[1][2],0.,0.,0.];
  for i in 0..4 {let p=frame[192+2*i];rows[53+i]=p;rows[49+i]=[
   (p[0]/frame[2][3]+1.)*frame[8][0]*0.5,
   (1.-p[1]/frame[3][3])*frame[8][1]*0.5,
   0.02*p[3]/frame[2][3]*frame[8][0]*0.5+1.,
   0.02*p[3]/frame[3][3]*frame[8][1]*0.5+1.];}
 }
 // Append three independent ruler tiles below the transport capture. Rows
 // 58..61 describe world rectangles; 61..64 store Z, first tile row and pixel size.
 fn ruler_tiles(frame:&[[f32;4]], rows:&mut [[f32;4];META_ROWS]) {
  let base=crate::workstation::timeline_bar_scene::BASE;
  let scale=[frame[8][0]/(2.*frame[2][3]),frame[8][1]/(2.*frame[3][3])];
  for face in 0..3 {
   let pose=frame[base+face];let shape=frame[base+3+face];
   rows[58+face]=[pose[0],pose[1],shape[0]*pose[3],shape[1]*pose[3]];
   rows[61+face]=[pose[2],rows[22][3],0.,0.];
   if shape[3]<=0. {continue;}
   let size=[(2.*rows[58+face][2]*scale[0]).ceil().max(1.),
             (2.*rows[58+face][3]*scale[1]).ceil().max(1.)];
   rows[61+face][2..].copy_from_slice(&size);
   rows[22][2]=rows[22][2].max(size[0]);
   rows[22][3]+=size[1];
  }
 }
 fn changed(a:&[[f32;4]],b:&[[f32;4]],transport_layers:bool)->bool{
  crate::workstation::transport_pose::optics_changed(a,b,transport_layers)
 }
 pub unsafe fn update(&mut self,queue:WGPUQueue,frame:&[[f32;4]]) {
  self.meter_regions=if !self.meter_base_render.is_null(){super::meter_regions::regions(frame,self.width,self.height)}else{None};
  self.supported=frame.len()==super::super::workstation::FRAME_ROWS&&frame[0][2]==2.&&frame[1][3]==0.&&frame[8][0]==self.width as f32&&frame[8][1]==self.height as f32&&crate::workstation::deferred_floor::retained_geometry_supported(frame)
   &&(!self.transport_layers||crate::workstation::transport_pose::retained_camera(frame));
  if self.supported {self.supported=(1..=4).all(|g|{let t=self.transform(frame,g);t.iter().all(|v|v.is_finite())&&t[2]<=self.width as f32+0.01&&t[3]<=self.height as f32+0.01});}
  if !self.supported {self.cancel_stage();self.stage_requested=false;self.pending=false;self.refresh.observe(false);return;}
  let moving=!self.previous.is_empty()&&(1..=4).any(|g|self.transform(&self.previous,g)!=self.transform(frame,g));
  let captured_layout_changed=self.prepared&&(1..=4).any(|g|self.transform(&self.captured,g)!=self.transform(frame,g));
  let anchor=if self.full_anchor.is_empty(){&self.captured}else{&self.full_anchor};
  let dirty=self.prepared&&Self::changed(anchor,frame,self.transport_layers);
  self.refresh.observe(dirty);
  let lighting_changed=crate::workstation::deferred_floor::lighting_changed(&self.previous,frame);
  // Unpacked composition keeps the rulers in its stationary capture.
  let new_face=self.compact && (0..3).any(|i|frame[crate::workstation::timeline_bar_scene::BASE+3+i][3]>0. && self.rows[61+i][2]==0.);
  let urgent_content=self.prepared && (new_face || (frame[9][3]==0. && content::timeline_changed(&self.captured,frame)) || (!self.compact && content::rulers_changed(&self.captured,frame))
   || (self.incremental_enabled && content::compare(&self.captured,frame,self.compact && captured_layout_changed,self.offline_faders)));
  let old_rows=self.rows;
  let partial=if crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_COMPONENT_MOTION")&&urgent_content&&!lighting_changed&&!new_face&&self.compact&&self.transport_layers
      &&self.capture_split.is_none()&&self.capture_attribution.is_none(){
   content::content_groups(&self.captured,frame,captured_layout_changed,self.offline_faders)
  }else{None};
  let preserve_pending=self.pending;
  self.capture_mask=if preserve_pending{0}else{partial.unwrap_or(0)};

  if urgent_content{self.cancel_stage();self.stage_requested=false;self.content_refreshes+=1;}
  if self.incremental_enabled&&self.prepared&&!urgent_content&&!preserve_pending{
   if self.generation.as_ref().is_some_and(|g|Self::changed(&g.source_rows,frame,self.transport_layers)){self.cancel_stage();}
   self.stage_requested=dirty&&((!moving && !self.dragging_fader(frame))||lighting_changed);
   self.pending=false;
   if moving{self.held_frames+=1;}
   for group in 0..6{self.rows[2+group*4+2]=self.transform(frame,group);}
   Self::panel_borders(&mut self.rows,&self.captured,frame);
   for group in 1..=4{self.rows[5+group*4][2]=old_rows[5+group*4][2];}
   if self.furniture_images.is_some()&&!furniture::update(&mut self.rows,&old_rows,frame,None){self.supported=false;self.prepared=false;return;}
   self.previous=frame.to_vec();
   (self.api.wgpuQueueWriteBuffer)(queue,self.metadata,0,self.rows.as_ptr().cast(),META_ROWS*16);
   return;
  }
  self.pending=urgent_content||preserve_pending||!self.prepared||(((!moving && !self.dragging_fader(frame))||lighting_changed)&&dirty);
  if moving&&self.prepared {self.held_frames+=1;}
  for g in 0..6 {let t=self.transform(frame,g);self.rows[2+g*4+2]=t;}
  if self.pending {
   for g in 0..6 {
    let t=self.transform(frame,g);let mut rect=[0.,0.,self.width as f32,self.height as f32];
    if (1..=4).contains(&g)||(self.transport_layers&&g==5){rect=panel_capture_rect(t,self.width,self.height);}
    if rect[2]>self.width as f32||rect[3]>self.height as f32 {self.supported=false;self.pending=false;self.refresh.observe(false);return;}
    self.rows[2+g*4]=rect;self.rows[2+g*4+1]=t;
   }
   if self.capture_mask!=0 {
    // Keep each untouched image's own capture rectangle/pose, not the latest
    // packet's pose. Current transforms remain live in the next metadata row.
    for group in 0..self.capture_groups as usize {
     if self.capture_mask&(1<<group)==0 {
      self.rows[2+group*4]=old_rows[2+group*4];self.rows[3+group*4]=old_rows[3+group*4];
     }
    }
    // Ruler tiles and transport bounds belong to group5 and its old capture.
    self.rows[49..64].copy_from_slice(&old_rows[49..64]);
   }
   if self.compact {
    if self.capture_mask==0{Self::ruler_tiles(frame,&mut self.rows);}
    let sizes:Vec<_>=(0..self.capture_groups as usize).map(|g|{let r=self.rows[2+g*4];[r[2] as u32,r[3] as u32]}).collect();
    let Some(packing)=self.reservation.reserve(&sizes,self.dimension_limit) else {self.supported=false;self.pending=false;self.refresh.observe(false);return;};
    for (g,origin) in packing.origins.iter().enumerate(){self.rows[5+g*4]=[origin[0] as f32,origin[1] as f32,0.,0.];}
    self.packing=Some(packing);
   }
   if self.transport_layers&&self.capture_mask==0 {Self::button_bounds(frame,&mut self.rows);}
   // Buttons have their own retained layer; other controls follow their panels.
   for instance in 0..65 {let group=if instance<4{if self.transport_layers{6.}else{1.}}else if instance<10||(13..29).contains(&instance)||instance>=61{2.}else{5.};self.rows[32+instance/4][instance%4]=group;}
   self.captured=frame.to_vec();
  }
  Self::panel_borders(&mut self.rows,&self.captured,frame);
  for group in 1..=4{if !self.pending||self.capture_mask!=0&&self.capture_mask&(1<<group)==0{self.rows[5+group*4][2]=old_rows[5+group*4][2];}}
  if self.pending&&self.capture_mask!=0 {
   self.migration=Some(old_rows);
   self.repack_pending=(0..self.capture_groups as usize).any(|g|self.rows[5+g*4][..2]!=old_rows[5+g*4][..2]);
  }
  self.rows[1][0]=self.capture_mask as f32;
  if self.furniture_images.is_some()&&!furniture::update(&mut self.rows,&old_rows,frame,self.pending.then_some(self.capture_mask)){self.supported=false;self.prepared=false;return;}
  self.previous=frame.to_vec();
  (self.api.wgpuQueueWriteBuffer)(queue,self.metadata,0,self.rows.as_ptr().cast(),META_ROWS*16);
 }
 pub unsafe fn resized(&mut self,queue:WGPUQueue,serial:u64,width:u32,height:u32)->Result<(),String>{
  self.cancel_stage();self.drain_stage(serial);self.stage_completed(queue,serial)?;self.reservation.clear();
  self.invalidate();self.width=width;self.height=height;self.previous.clear();self.captured.clear();Ok(())
 }
 pub fn invalidate(&mut self){self.full_anchor.clear();self.capture_mask=0;self.migration=None;self.repack_pending=false;self.ledger_external_changed();self.cancel_stage();self.stage_requested=false;self.supported=false;self.prepared=false;self.pending=false;self.refresh.observe(false);}
 pub fn needs_redraw(&self)->bool{self.supported&&(self.refresh.needs_redraw()||self.fade_start.is_some())}
  pub unsafe fn comparison_capture_current(&mut self,queue:WGPUQueue){
   if self.supported&&(!self.prepared||Self::changed(&self.captured,&self.previous,self.transport_layers)){
    // Bank misses must store fresh original optics, not an in-progress retained
    // approximation. Cancel the immutable stage and request a synchronous bake
    // only for this admitted camera/foreground path; other views keep their
    // ordinary incremental policy. Caller has already drained prior GPU work.
    self.cancel_stage();self.stage_requested=false;
    let incremental=self.incremental_enabled;self.incremental_enabled=false;
    self.prepared=false;let rows=self.previous.clone();self.update(queue,&rows);
    self.incremental_enabled=incremental;
   }
  }
  pub unsafe fn prepare_draw(&mut self,queue:WGPUQueue)->Result<(),String>{
  if !self.detail_buffer.is_null() {
   let elapsed=self.fade_start.map(|t|t.elapsed().as_secs_f32()/0.24).unwrap_or(1.);
   let weight=if self.pending {0.}else{1.-elapsed.clamp(0.,1.)};
   let weight=weight*weight*(3.-2.*weight);
   (self.api.wgpuQueueWriteBuffer)(queue,self.detail_buffer,0,(&weight as *const f32).cast(),4);
   if weight==0.{self.fade_start=None;}
  }
  if self.furniture_images.is_some()&&self.prepared&&!self.pending&&(self.stage_requested||self.refresh.idle_due()){
   self.cancel_stage();self.stage_requested=false;self.prepared=false;
   let current=self.previous.clone();self.update(queue,&current);
  }
  if self.incremental_enabled&&self.prepared&&!self.pending{
   if self.supported&&!self.dragging_fader(&self.previous)&&self.refresh.idle_due()&&Self::changed(if self.full_anchor.is_empty(){&self.captured}else{&self.full_anchor},&self.previous,self.transport_layers){self.stage_requested=true;self.idle_refresh_pending=true;}
   return Ok(());
  }
  if !self.pending&&self.supported&&self.refresh.idle_due(){
   // Last uploaded packet is still current on GPU. Re-evaluate it as stable;
   // do not request another application View or change optical inputs.
   let current=self.previous.clone();self.update(queue,&current);
   self.idle_refresh_pending=self.pending;
  }
  if self.compact&&self.pending&&self.supported {let extent=self.packing.as_ref().ok_or("missing compact atlas plan")?.extent;if extent!=self.atlas_extent||self.repack_pending{self.resize_atlas(extent)?;}}
  Ok(())
 }
 unsafe fn resize_atlas(&mut self,extent:[u32;2])->Result<(),String>{
  // Called after the previous GPU submission is drained, before any new bake
  // or composition is encoded. The complete new generation is baked this draw.
  let a=self.api;
  let texture=(a.wgpuDeviceCreateTexture)(self.device,&WGPUTextureDescriptor{label:view("Packed original FP32 samples"),usage:WGPUTextureUsage_StorageBinding|WGPUTextureUsage_TextureBinding|WGPUTextureUsage_CopySrc|WGPUTextureUsage_CopyDst|atlas_export::copy_usage(),dimension:WGPUTextureDimension_2D,size:WGPUExtent3D{width:extent[0],height:extent[1],depthOrArrayLayers:2},format:WGPUTextureFormat_RGBA32Float,mipLevelCount:1,sampleCount:1,..Default::default()});
  if texture.is_null(){return Err("compact atlas allocation failed".into());}
  let texture_view=(a.wgpuTextureCreateView)(texture,&WGPUTextureViewDescriptor{dimension:WGPUTextureViewDimension_2DArray,arrayLayerCount:2,mipLevelCount:1,aspect:WGPUTextureAspect_All,..Default::default()});
  if texture_view.is_null(){(a.wgpuTextureRelease)(texture);return Err("compact atlas view failed".into());}
  let next_images=if self.furniture_images.is_some(){Some(furniture::Images::new(a,self.device,extent)?)}else{None};
  let mut cs=self.compute_entries.clone();let mut rs=self.render_entries.clone();
  if let Some(images)=&next_images{for i in 0..2{cs.iter_mut().find(|e|e.binding==18+2*i).unwrap().textureView=images.views[i as usize];rs.iter_mut().find(|e|e.binding==19+2*i).unwrap().textureView=images.views[i as usize];}}

  cs.iter_mut().find(|e|e.binding==4).unwrap().textureView=texture_view;rs.iter_mut().find(|e|e.binding==5).unwrap().textureView=texture_view;
  if let Some(old)=rs.iter_mut().find(|e|e.binding==16){old.textureView=texture_view;}
  self.fade_start=None;
  let compute_group=(a.wgpuDeviceCreateBindGroup)(self.device,&WGPUBindGroupDescriptor{layout:self.compute_layout,entryCount:cs.len(),entries:cs.as_ptr(),..Default::default()});
  let render_group=(a.wgpuDeviceCreateBindGroup)(self.device,&WGPUBindGroupDescriptor{layout:self.layout,entryCount:rs.len(),entries:rs.as_ptr(),..Default::default()});
  let bounded_groups=self.group_dispatch.as_ref().map(|b|b.prepare_groups(self.device,self.compute_layout,&cs)).transpose();
  let error=check_errors().err().or_else(||bounded_groups.as_ref().err().cloned());
  if compute_group.is_null()||render_group.is_null()||error.is_some(){if !compute_group.is_null(){(a.wgpuBindGroupRelease)(compute_group);}if !render_group.is_null(){(a.wgpuBindGroupRelease)(render_group);}(a.wgpuTextureViewRelease)(texture_view);(a.wgpuTextureRelease)(texture);return Err(error.unwrap_or("compact atlas binding failed".into()));}
  if let(Some(bank),Some(groups))=(&mut self.group_dispatch,bounded_groups?){bank.commit_groups(groups);}
  if self.capture_mask!=0 {
   let previous=self.migration.as_ref().ok_or("partial atlas migration lost its source metadata")?;
   let encoder=(a.wgpuDeviceCreateCommandEncoder)(self.device,ptr::null());
   if encoder.is_null(){return Err("partial atlas copy encoder failed".into());}
   for group in 0..self.capture_groups as usize {
    if self.capture_mask&(1<<group)!=0{continue;}
    let from=previous[5+group*4];let to=self.rows[5+group*4];let size=previous[2+group*4];
    let source=WGPUTexelCopyTextureInfo{texture:self.texture,origin:WGPUOrigin3D{x:from[0]as u32,y:from[1]as u32,z:0},aspect:WGPUTextureAspect_All,..Default::default()};
    let destination=WGPUTexelCopyTextureInfo{texture,origin:WGPUOrigin3D{x:to[0]as u32,y:to[1]as u32,z:0},aspect:WGPUTextureAspect_All,..Default::default()};
    (a.wgpuCommandEncoderCopyTextureToTexture)(encoder,&source,&destination,&WGPUExtent3D{width:size[2]as u32,height:size[3]as u32,depthOrArrayLayers:2});
    if let(Some(old),Some(new))=(&self.furniture_images,&next_images){for i in 0..2{
     let mut source=source;source.texture=old.textures[i];let mut destination=destination;destination.texture=new.textures[i];
     (a.wgpuCommandEncoderCopyTextureToTexture)(encoder,&source,&destination,&WGPUExtent3D{width:size[2]as u32,height:size[3]as u32,depthOrArrayLayers:2});
    }}
   }
   let command=(a.wgpuCommandEncoderFinish)(encoder,ptr::null());
   (a.wgpuQueueSubmit)(self.queue,1,&command);
   (a.wgpuCommandBufferRelease)(command);(a.wgpuCommandEncoderRelease)(encoder);check_errors()?;
  }
  self.atlas_peak_replacement_bytes=self.atlas_peak_replacement_bytes.max((self.atlas_extent[0] as u64*self.atlas_extent[1] as u64+extent[0] as u64*extent[1] as u64)*32);
  (a.wgpuBindGroupRelease)(self.compute_group);(a.wgpuBindGroupRelease)(self.bind_group);(a.wgpuTextureViewRelease)(self.texture_view);(a.wgpuTextureRelease)(self.texture);
  self.ledger_external_changed();
  if let Some(split)=&mut self.capture_split{split.invalidate_bindings();}
  self.furniture_images=next_images;self.texture=texture;self.texture_view=texture_view;self.compute_group=compute_group;self.bind_group=render_group;self.compute_entries=cs;self.render_entries=rs;self.atlas_extent=extent;self.atlas_reallocations+=1;Ok(())
 }
 fn dragging_fader(&self,frame:&[[f32;4]])->bool{
  self.offline_faders && frame.get(crate::workstation::fader_components::BASE+6).is_some_and(|r|r[0]>0.5)
 }
 pub fn cancel_stage(&mut self){if let Some(g)=&mut self.generation{g.cancel();}}
 pub fn needs_stage_snapshot(&self)->bool{self.furniture_images.is_none()&&self.incremental_enabled&&self.prepared&&self.supported&&self.stage_requested&&self.generation.is_none()}
 pub unsafe fn stage_completed(&mut self,queue:WGPUQueue,serial:u64)->Result<(),String>{
  if let Some(g)=&mut self.generation{g.completed(serial);}
  let remove=self.generation.as_ref().is_some_and(|g|(g.cancelled||g.plan.publishable())&&g.plan.can_release());
  if !remove{return Ok(());}let mut g=self.generation.take().unwrap();
  if g.cancelled||!self.supported||Self::changed(&g.source_rows,&self.previous,self.transport_layers){self.staged_cancelled+=1;return Ok(());}
  let a=self.api;
  let bounded_groups=if let Some(bank)=&self.group_dispatch{
   let mut entries=self.compute_entries.clone();entries.iter_mut().find(|e|e.binding==4).unwrap().textureView=g.view;
   Some(bank.prepare_groups(self.device,self.compute_layout,&entries)?)
  }else{None};
  if self.group_dispatch.is_some(){check_errors()?;}
  if let(Some(bank),Some(groups))=(&mut self.group_dispatch,bounded_groups){bank.commit_groups(groups);}
  self.ledger_external_changed();
  // This is the only publication point, after the final tile's GPU completion.
  if !self.detail_buffer.is_null(){
   let mut history=[[0f32;4];13];history[0][0]=1.;
   for group in 1..=4 {
    let i=1+(group-1)*3;let at=2+group*4;
    history[i]=self.rows[at];history[i+1]=self.rows[at+1];history[i+2]=self.rows[at+3];
   }
   (a.wgpuQueueWriteBuffer)(queue,self.detail_buffer,0,history.as_ptr().cast(),208);
   if !self.history_view.is_null(){(a.wgpuTextureViewRelease)(self.history_view);}
   self.history_view=self.texture_view;
   // Transfer ownership of the previous atlas view until the next publication.
   self.render_entries.iter_mut().find(|e|e.binding==16).unwrap().textureView=self.history_view;
   self.fade_start=Some(Instant::now());
  }
  (a.wgpuBindGroupRelease)(self.compute_group);(a.wgpuBindGroupRelease)(self.bind_group);if self.detail_buffer.is_null(){(a.wgpuTextureViewRelease)(self.texture_view);}(a.wgpuTextureRelease)(self.texture);
  if let Some(split)=&mut self.capture_split{split.invalidate_bindings();}
  self.texture=std::mem::replace(&mut g.texture,ptr::null_mut());self.texture_view=std::mem::replace(&mut g.view,ptr::null_mut());self.compute_group=std::mem::replace(&mut g.future_compute,ptr::null_mut());self.bind_group=std::mem::replace(&mut g.future_render,ptr::null_mut());
  self.compute_entries.iter_mut().find(|e|e.binding==4).unwrap().textureView=self.texture_view;self.render_entries.iter_mut().find(|e|e.binding==5).unwrap().textureView=self.texture_view;
  self.rows=g.rows;for group in 0..6{self.rows[2+group*4+2]=self.transform(&self.previous,group);}
  Self::panel_borders(&mut self.rows,&g.source_rows,&self.previous);
  self.reservation.clear();self.full_anchor=g.source_rows.clone();self.capture_mask=0;self.migration=None;self.repack_pending=false;self.captured=g.source_rows.clone();self.atlas_extent=g.extent;self.packing=g.packing.take();self.atlas_reallocations+=1;
  (a.wgpuQueueWriteBuffer)(queue,self.metadata,0,self.rows.as_ptr().cast(),META_ROWS*16);
  self.stage_requested=false;self.pending=false;self.prepared=true;self.refresh.submitted();self.bakes+=1;self.staged_published+=1;
  if self.stage_idle{self.idle_refreshes+=1;}self.stage_idle=false;self.idle_refresh_pending=false;self.staged_convergence_ms.push(g.started.elapsed().as_secs_f64()*1000.);check_errors()
 }
 pub unsafe fn encode_stage(&mut self,encoder:WGPUCommandEncoder,queue:WGPUQueue,entries:&[[u32;4]])->Result<bool,String>{
  if !self.incremental_enabled||!self.prepared||!self.supported{return Ok(false);}
  if self.needs_stage_snapshot(){
   let frame=&self.previous;let mut rows=self.rows;rows[1][0]=0.;
   for group in 0..6{let t=self.transform(frame,group);let mut rect=[0.,0.,self.width as f32,self.height as f32];if (1..=4).contains(&group)||(self.transport_layers&&group==5){rect=panel_capture_rect(t,self.width,self.height);}if rect[2]>self.width as f32||rect[3]>self.height as f32{return Err("incremental capture exceeds supported viewport".into());}rows[2+group*4]=rect;rows[3+group*4]=t;rows[4+group*4]=t;}
   if self.transport_layers {Self::button_bounds(frame,&mut rows);}
   Self::ruler_tiles(frame,&mut rows);
   let sizes:Vec<_>=(0..self.capture_groups as usize).map(|g|[rows[2+g*4][2]as u32,rows[2+g*4][3]as u32]).collect();let packing=atlas::pack(&sizes,self.dimension_limit).ok_or("incremental compact atlas packing failed")?;
   for (g,origin)in packing.origins.iter().enumerate(){rows[5+g*4]=[origin[0]as f32,origin[1]as f32,0.,0.];}
   Self::panel_borders(&mut rows,frame,frame);
   let entries:Vec<_>=entries.iter().map(|e|incremental::tiles::Entry{slot:e[0],edge:e[1],dirty:e[2]!=0,ready:e[3]!=0}).collect();
   let generation=incremental::Generation::new(self,self.next_generation,frame.clone(),rows,packing,&entries)?;self.next_generation+=1;self.staged_peak_extra_bytes=self.staged_peak_extra_bytes.max(generation.requested_bytes);self.stage_idle=self.idle_refresh_pending;self.generation=Some(generation);self.stage_requested=false;
  }
  let pipelines=[self.incremental_pipelines[0],self.incremental_pipelines[1],self.incremental_pipelines[2],self.compute];
  if let Some(g)=&mut self.generation{g.encode(encoder,queue,pipelines)}else{Ok(false)}
 }
 pub fn drain_stage(&mut self,serial:u64){if let Some(g)=&mut self.generation{g.completed(serial);}}
 pub fn stage_submitted(&mut self,serial:u64)->Result<(),String>{if let Some(g)=&mut self.generation{g.submitted(serial)?;}Ok(())}
 pub fn drawn(&mut self){self.refresh.drawn();}
 pub fn selected(&self,width:u32,height:u32)->bool {self.supported&&(self.prepared||self.pending)&&self.width==width&&self.height==height}
 pub unsafe fn encode(&mut self,encoder:WGPUCommandEncoder,timestamps:Option<&WGPUComputePassTimestampWrites>)->Result<bool,String>{
  if !self.pending||!self.supported{return Ok(false);}
  let split=if self.rows[22][2]>self.width as f32 || self.rows[22][3]>self.height as f32 {false}else if let Some(runtime)=&mut self.capture_split{runtime.prepare(&self.compute_entries,&self.rows,self.width,self.height,self.capture_groups)?}else{false};
  let a=self.api;let pass=(a.wgpuCommandEncoderBeginComputePass)(encoder,&WGPUComputePassDescriptor{label:view("Bake resident original component appearance"),timestampWrites:timestamps.map_or(ptr::null(),|writes|writes),..Default::default()});
  if pass.is_null(){return Err("retained bake pass failed".into());}
  if let Some(table)=&mut self.bar_planes{table.encode(pass,&self.previous);}
  if split{self.capture_split.as_mut().unwrap().encode(pass,self.width,self.height,self.capture_groups);(a.wgpuComputePassEncoderEnd)(pass);(a.wgpuComputePassEncoderRelease)(pass);return Ok(true);}
  (a.wgpuComputePassEncoderSetPipeline)(pass,if let Some(d)=&mut self.capture_attribution{d.pipeline()}else if self.capture_compute.is_null(){self.compute}else{self.capture_compute});
  let group_y=if self.capture_compute.is_null(){8}else{4};
  let bounded=self.group_dispatch.as_ref().and_then(|bank|group_dispatch::plan(&self.rows,self.width,self.height,self.capture_groups,group_y).map(|jobs|(bank,jobs)));
  if let Some((bank,jobs))=bounded{
   for job in jobs{
    (a.wgpuComputePassEncoderSetBindGroup)(pass,0,bank.group(job.group),0,ptr::null());
    (a.wgpuComputePassEncoderDispatchWorkgroups)(pass,job.workgroups[0],job.workgroups[1],job.workgroups[2]);
   }
  }else{
   (a.wgpuComputePassEncoderSetBindGroup)(pass,0,self.compute_group,0,ptr::null());
   (a.wgpuComputePassEncoderDispatchWorkgroups)(pass,self.width.max(self.rows[22][2] as u32).div_ceil(8),self.height.max(self.rows[22][3] as u32).div_ceil(group_y),self.capture_groups*2);
  }
  (a.wgpuComputePassEncoderEnd)(pass);(a.wgpuComputePassEncoderRelease)(pass);Ok(true)
 }
 pub unsafe fn export_atlas(&mut self,instance:WGPUInstance,queue:WGPUQueue,directory:&std::path::Path)->Result<(),String>{if let Some(mut export)=self.atlas_export.take(){let result=export.write(self,instance,queue,directory);self.atlas_export=Some(export);result?;}Ok(())}
 pub fn submitted(&mut self,elapsed:f64){
  if self.capture_mask==0{self.full_anchor=self.captured.clone();self.captured_groups+=self.capture_groups as u64;}
  else{self.partial_captures+=1;self.captured_groups+=self.capture_mask.count_ones()as u64;}
  self.migration=None;self.repack_pending=false;
  self.ledger_external_changed();self.refresh.submitted();
  if self.capture_mask!=0{self.refresh.observe(true);self.refresh.drawn();}if self.idle_refresh_pending{self.idle_refreshes+=1;self.idle_refresh_pending=false;}self.prepared=true;self.pending=false;self.bakes+=1;self.bake_wall_ms.push(elapsed);}
 pub(super) fn ledger_packet(&mut self,rows:&[[f32;4]]){if let Some(l)=&mut self.ledger{l.observe_packet(rows);}}
 pub(super) fn ledger_response_plan(&mut self,rows:&[[f32;4]]){if let Some(l)=&mut self.ledger{l.response_plan(rows);}}
 pub(super) fn ledger_external_changed(&mut self){if let Some(l)=&mut self.ledger{l.external_changed();}}
 pub(super) fn render_bind_group(&self)->WGPUBindGroup{self.ledger.as_ref().map_or(self.bind_group,|l|l.render_group(self.bind_group))}
 pub(super) unsafe fn encode_ledger(&mut self,encoder:WGPUCommandEncoder,queue:WGPUQueue,completed:u64,
     client_eligible:bool,external_dependencies:&[u64])->Result<bool,String>{
  if self.ledger.is_none(){return Ok(false);}
  let mut dependencies=self.final_dependencies();dependencies.extend_from_slice(external_dependencies);
  let eligible=client_eligible&&self.selected(self.width,self.height)&&self.prepared&&!self.pending
      &&!self.stage_requested&&self.generation.is_none()&&self.meter_regions.is_some();
  self.ledger.as_mut().unwrap().prepare(encoder,queue,self.width,self.height,dependencies,eligible,completed,self.layout,&self.render_entries)
 }
 pub(super) fn ledger_submitted(&mut self,serial:u64)->Result<(),String>{self.ledger.as_mut().ok_or("meter ledger absent at submission")?.submitted(serial)}
 pub fn info(&self)->Value{let mut result=json!({"full_detail_panels":!self.detail_buffer.is_null(),"primary_group_gates":self.primary_group_gates,"primary_witness_version":self.primary_witness_version,"primary_witness":self.primary_witness,"zero_alpha_light":self.zero_alpha_light,"density_zero_guard":self.density_zero_guard,"capture_attribution":self.capture_attribution.as_ref().map(|d|d.info()),"capture_split":{"enabled":self.capture_split.is_some(),"details":self.capture_split.as_ref().map(|s|s.info())},"bar_plane_table":{"enabled":self.bar_planes.is_some(),"details":self.bar_planes.as_ref().map(|p|p.info())},"bounded_capture_groups":{"requested":group_dispatch::requested(),"available":self.group_dispatch.is_some(),"valid_plan":self.group_dispatch.is_some()&&group_dispatch::plan(&self.rows,self.width,self.height,self.capture_groups,if self.capture_compute.is_null(){8}else{4}).is_some(),"offset_buffer_bytes":if self.group_dispatch.is_some(){96}else{0}},"meter_ledger":self.ledger.as_ref().map(|l|l.info()).unwrap_or_else(||json!({"enabled":false})),"capture_workgroup":if self.capture_compute.is_null(){[8,8]}else{[8,4]},"meter_split":!self.meter_base_render.is_null(),"meter_region_count":self.meter_regions.as_ref().map(Vec::len),"meter_region_pixels":self.meter_regions.as_ref().map(|rs|rs.iter().map(|r|r.width as u64*r.height as u64).sum::<u64>()),"camera_split":!self.camera_render.is_null(),"selected":self.supported,"prepared":self.prepared,"bakes":self.bakes,"held_motion_frames":self.held_frames,"idle_settle_refreshes":self.idle_refreshes,"atlas_bytes":self.atlas_extent[0] as u64*self.atlas_extent[1] as u64*self.atlas_layers as u64*16,
 "incremental_bake":{"enabled":self.incremental_enabled,"content_synchronous_refreshes":self.content_refreshes,"pending":self.generation.as_ref().map(|g|g.info()),"requested":self.stage_requested,"published":self.staged_published,"cancelled":self.staged_cancelled,"peak_extra_bytes":self.staged_peak_extra_bytes,"convergence_ms":self.staged_convergence_ms,"scope":"One immutable GPU snapshot/staging atlas generation; bounded phase tiles; atomic atlas publication only after completion. Main-thread audio is serviced by normal loop between chunks. Startup and unsupported states use original synchronous path."},"compact_atlas":self.compact,"atlas_extent":self.atlas_extent,"atlas_layers":self.atlas_layers,"atlas_reallocations":self.atlas_reallocations,"atlas_peak_replacement_bytes":self.atlas_peak_replacement_bytes,
 "capture_rects":(0..self.capture_groups as usize).map(|g|self.rows[2+g*4]).collect::<Vec<_>>(),
 "packed_origins":self.packing.as_ref().map(|p|&p.origins),"used_sample_bytes":self.packing.as_ref().map(|p|p.used_texels*32),
 "allocation_scope":"Requested RGBA32F texture texel storage from actual descriptors; driver padding/physical heap residency not queried. No extra depth texture in compact mode.","stationary_groups_merged":self.transport_layers||self.capture_groups==5,"independent_transport_layer":self.transport_layers,"depth_first":self.depth_first,"depth_atlas_bytes":if self.depth_first{self.width as u64*self.height as u64*self.capture_groups as u64*2*4}else{0},"bake_submission_wall_ms":self.bake_wall_ms,"scope":"bake draw wall includes idle deferred-floor resolution, response preparation, capture, compose and GPU completion; initial update field CPU is recorded separately","moving_appearance":"panel images carry illumination; settled semantic change refreshes","labels":"original engraved pixels retained with panel; content refresh at rest","unsupported":"nonorthographic camera, non-two-sample, custom styles, oversized group or viewport resize use original"});result["component_cache"]=json!({"independent_controls":self.furniture_images.is_some(),"control_count":self.rows[0][1],"partial_content_captures":self.partial_captures,"captured_groups":self.captured_groups,"capture_mask":self.capture_mask});result}
}
impl Drop for Pipeline {fn drop(&mut self){self.generation.take();self.ledger.take();self.group_dispatch.take();self.bar_planes.take();self.capture_split.take();self.capture_attribution.take();unsafe{let a=self.api;if !self.history_view.is_null(){(a.wgpuTextureViewRelease)(self.history_view);}if !self.detail_buffer.is_null(){(a.wgpuBufferRelease)(self.detail_buffer);}if !self.zero_tile.is_null(){(a.wgpuBufferRelease)(self.zero_tile);}for p in self.incremental_pipelines{if !p.is_null(){(a.wgpuComputePipelineRelease)(p);}}for h in [self.bind_group,self.compute_group]{if !h.is_null(){(a.wgpuBindGroupRelease)(h);}}if !self.meter_base_render.is_null(){(a.wgpuRenderPipelineRelease)(self.meter_base_render);}if !self.meter_overlay_render.is_null(){(a.wgpuRenderPipelineRelease)(self.meter_overlay_render);}if !self.camera_render.is_null(){(a.wgpuRenderPipelineRelease)(self.camera_render);}if !self.render.is_null(){(a.wgpuRenderPipelineRelease)(self.render);}if !self.capture_compute.is_null(){(a.wgpuComputePipelineRelease)(self.capture_compute);}if !self.compute.is_null(){(a.wgpuComputePipelineRelease)(self.compute);}if !self.shader.is_null(){(a.wgpuShaderModuleRelease)(self.shader);}if !self.compute_pipeline_layout.is_null(){(a.wgpuPipelineLayoutRelease)(self.compute_pipeline_layout);}if !self.compute_layout.is_null(){(a.wgpuBindGroupLayoutRelease)(self.compute_layout);}if !self.pipeline_layout.is_null(){(a.wgpuPipelineLayoutRelease)(self.pipeline_layout);}if !self.layout.is_null(){(a.wgpuBindGroupLayoutRelease)(self.layout);}if !self.depth_view.is_null(){(a.wgpuTextureViewRelease)(self.depth_view);}if !self.depth_texture.is_null(){(a.wgpuTextureRelease)(self.depth_texture);}if !self.texture_view.is_null(){(a.wgpuTextureViewRelease)(self.texture_view);}if !self.texture.is_null(){(a.wgpuTextureRelease)(self.texture);}if !self.metadata.is_null(){(a.wgpuBufferRelease)(self.metadata);}}}}

#[cfg(test)] mod fullscreen_quality_tests {
 use super::*;
 #[test] fn fullscreen_padding_keeps_retention_supported_at_viewport_edges(){
  for (w,h) in [(960,600),(1920,1200),(600,960)] {
   let r=panel_capture_rect([w as f32*0.5,h as f32*0.5,w as f32+0.002,h as f32+0.002],w,h);
   assert_eq!(r,[0.,0.,w as f32,h as f32]);
  }
  assert_eq!(panel_capture_rect([100.,200.,120.,180.],960,600),[37.,107.,128.,188.]);
 }
}
