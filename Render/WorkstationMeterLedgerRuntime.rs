//! One-generation owner for the exact ordered meter ledger.
//! All methods run on the existing renderer owner. No worker, readback or queue.
use super::*;
use super::meter_ledger_plan::{Key, Layout};
use super::meter_ledger_state::{Budget, Limits, State, SLOT_BYTES};

struct Generation {
    api: &'static Api, slots:WGPUBuffer,jobs:WGPUBuffer,map:WGPUBuffer,
    compute:WGPUBindGroup, render:WGPUBindGroup, budget:Budget,
}
impl Drop for Generation { fn drop(&mut self) { unsafe {
    for h in [self.compute,self.render] {if !h.is_null(){(self.api.wgpuBindGroupRelease)(h);}}
    for h in [self.slots,self.jobs,self.map] {if !h.is_null(){(self.api.wgpuBufferRelease)(h);}}
}}}
pub(super) struct Runtime {
    api:&'static Api,device:WGPUDevice,limits:Limits,
    dummy_slots:WGPUBuffer,dummy_map:WGPUBuffer,
    compute_layout:WGPUBindGroupLayout,pipeline_layout:WGPUPipelineLayout,pipeline:WGPUComputePipeline,
    playhead_certificate:bool,
    diagnostic_force_fallback:bool,
    state:State<Key>,generation:Option<Generation>,packet:Option<Vec<[f32;4]>>,
    response_bits:Vec<[u32;4]>,external_epoch:u64,exhausted:bool,
    builds:u64,hits:u64,rejections:u64,ineligible_draws:u64,peak_bytes:u64,last_rejection:Option<String>,last_admission:String,
}
impl Runtime {
    pub(super) fn playhead_certificate(&self)->bool {self.playhead_certificate}
    pub(super) fn requested()->bool {crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_METER_LEDGER")}
    pub(super) unsafe fn new(c:&Client)->Result<Self,String> {
        let a=c.api;let mut raw=WGPULimits::default();
        if (a.wgpuDeviceGetLimits)(c.device,&mut raw)!=WGPUStatus_Success {return Err("meter ledger device limits unavailable".into());}
        let limits=Limits{binding_bytes:raw.maxStorageBufferBindingSize as u64,buffer_bytes:raw.maxBufferSize,
            storage_per_stage:raw.maxStorageBuffersPerShaderStage,workgroups:raw.maxComputeWorkgroupsPerDimension};
        if limits.storage_per_stage<6 || limits.binding_bytes<SLOT_BYTES || limits.buffer_bytes<SLOT_BYTES {
            return Err("meter ledger explicit layout exceeds device limits".into());
        }
        let mut r=Self{api:a,device:c.device,limits,dummy_slots:ptr::null_mut(),dummy_map:ptr::null_mut(),
            compute_layout:ptr::null_mut(),pipeline_layout:ptr::null_mut(),pipeline:ptr::null_mut(),
            playhead_certificate:crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_METER_LEDGER_PLAYHEAD"),
            diagnostic_force_fallback:std::env::var("WORMWOOD_WORKSTATION_METER_LEDGER_FORCE_FALLBACK").as_deref()==Ok("1"),
            state:State::default(),generation:None,packet:None,response_bits:vec![],external_epoch:0,exhausted:false,
            builds:0,hits:0,rejections:0,ineligible_draws:0,peak_bytes:super::meter_ledger_state::DUMMY_BYTES,last_rejection:None,last_admission:"not drawn".into()};
        r.dummy_slots=r.buffer("Unpublished meter ledger slot",SLOT_BYTES);
        r.dummy_map=r.buffer("Unmapped meter ledger",4);
        if r.dummy_slots.is_null()||r.dummy_map.is_null(){return Err("meter ledger dummy allocation failed".into());}
        // WebGPU zero-initializes storage. Map[0] is explicitly unmapped;
        // all other pixels fail arrayLength before touching any slot.
        let unmapped=u32::MAX;(a.wgpuQueueWriteBuffer)(c.queue,r.dummy_map,0,(&unmapped as *const u32).cast(),4);
        Ok(r)
    }
    unsafe fn buffer(&self,label:&str,size:u64)->WGPUBuffer {
        (self.api.wgpuDeviceCreateBuffer)(self.device,&WGPUBufferDescriptor{label:view(label),size,
            usage:WGPUBufferUsage_Storage|WGPUBufferUsage_CopyDst,..Default::default()})
    }
    pub(super) fn render_bindings(&self,layout:&mut Vec<WGPUBindGroupLayoutEntry>,entries:&mut Vec<WGPUBindGroupEntry>) {
        for (binding,buffer,size,kind) in [(9,self.dummy_slots,SLOT_BYTES,WGPUBufferBindingType_Storage),
            (13,self.dummy_map,4,WGPUBufferBindingType_ReadOnlyStorage)] {
            layout.push(WGPUBindGroupLayoutEntry{binding,visibility:WGPUShaderStage_Fragment,
                buffer:WGPUBufferBindingLayout{type_:kind,minBindingSize:size,..Default::default()},..Default::default()});
            entries.push(WGPUBindGroupEntry{binding,buffer,size,..Default::default()});
        }
    }
    pub(super) unsafe fn create_pipeline(&mut self,shader:WGPUShaderModule,scene_bytes:u64,source:&str)->Result<(),String> {
        if source.contains("// METER_LEDGER_PLAYBACK_CERTIFICATE")!=self.playhead_certificate{
            return Err("meter ledger playhead key/shader mode mismatch".into());
        }
        let a=self.api;
        let entries:Vec<_>=[(1,scene_bytes,WGPUBufferBindingType_ReadOnlyStorage),
            (6,64*16,WGPUBufferBindingType_ReadOnlyStorage),(9,SLOT_BYTES,WGPUBufferBindingType_Storage),
            (10,16,WGPUBufferBindingType_ReadOnlyStorage)].into_iter().map(|(binding,size,kind)|
                WGPUBindGroupLayoutEntry{binding,visibility:WGPUShaderStage_Compute,
                    buffer:WGPUBufferBindingLayout{type_:kind,minBindingSize:size,..Default::default()},..Default::default()}).collect();
        self.compute_layout=(a.wgpuDeviceCreateBindGroupLayout)(self.device,&WGPUBindGroupLayoutDescriptor{
            label:view("Meter ledger capture bindings"),entryCount:entries.len(),entries:entries.as_ptr(),..Default::default()});
        self.pipeline_layout=(a.wgpuDeviceCreatePipelineLayout)(self.device,&WGPUPipelineLayoutDescriptor{
            bindGroupLayoutCount:1,bindGroupLayouts:&self.compute_layout,..Default::default()});
        self.pipeline=(a.wgpuDeviceCreateComputePipeline)(self.device,&WGPUComputePipelineDescriptor{
            label:view("Exact ordered meter ledger capture"),layout:self.pipeline_layout,
            compute:WGPUProgrammableStageDescriptor{module:shader,entryPoint:view("cs_meter_ledger_capture"),..Default::default()},..Default::default()});
        if self.compute_layout.is_null()||self.pipeline_layout.is_null()||self.pipeline.is_null(){return Err("meter ledger capture pipeline failed".into());}
        check_errors()
    }
    pub(super) fn external_changed(&mut self) {
        match self.external_epoch.checked_add(1) {Some(v)=>self.external_epoch=v,None=>self.exhausted=true}
        self.state.invalidate();
    }
    pub(super) fn observe_packet(&mut self,rows:&[[f32;4]]) {
        self.state.deselect();
        if rows.len()==super::super::workstation::FRAME_ROWS {
            if super::super::workstation::validate_frame(rows).is_ok(){self.packet=Some(rows.to_vec());}
            else {self.packet=None;self.state.invalidate();}
        } else {self.packet=None;self.external_changed();}
    }
    pub(super) fn response_plan(&mut self,rows:&[[f32;4]]) {
        let same=self.response_bits.len()==rows.len()&&self.response_bits.iter().zip(rows).all(|(a,b)|*a==b.map(f32::to_bits));
        if !same {self.response_bits=rows.iter().map(|r|r.map(f32::to_bits)).collect();self.external_changed();}
    }
    pub(super) fn render_group(&self,fallback:WGPUBindGroup)->WGPUBindGroup {
        if self.state.selected()&&!self.diagnostic_force_fallback{self.generation.as_ref().map_or(fallback,|g|g.render)}else{fallback}
    }
    /// Called only after Client::draw drains the preceding submission. Writes
    /// queued before this call precede capture; future QueueWriteBuffer calls
    /// follow this submission. No submitted generation sees mutable host input.
    pub(super) unsafe fn prepare(&mut self,encoder:WGPUCommandEncoder,queue:WGPUQueue,
        width:u32,height:u32,mut dependencies:Vec<u64>,eligible:bool,completed:u64,
        render_layout:WGPUBindGroupLayout,render_entries:&[WGPUBindGroupEntry])->Result<bool,String> {
        dependencies.extend([self.external_epoch,self.pipeline as usize as u64]);
        let key=if !eligible {self.last_admission="unsupported frame or pending retained capture/refresh".into();None}
            else if self.exhausted {self.last_admission="external content epoch exhausted".into();None}
            else if let Some(rows)=&self.packet {match Key::with_playhead_certificate(rows,width,height,&dependencies,self.playhead_certificate){
                Ok(key)=>{self.last_admission="waiting for two stable draws or completion".into();Some(key)},
                Err(e)=>{self.last_admission=format!("packet/key rejected: {e:?}");None}
            }}else{self.last_admission="no validated current frame packet".into();None};
        if key.is_none(){self.ineligible_draws+=1;}
        let decision=self.state.observe(key,completed);
        if decision.release {self.generation.take();}
        if decision.cached {self.hits+=1;self.last_admission="complete generation and exact current key".into();return Ok(false);}
        if !decision.build {return Ok(false);}
        debug_assert!(self.generation.is_none());
        let build=(||->Result<Generation,String>{
            let rows=self.packet.as_ref().ok_or("meter ledger has no current packet")?;
            let regions=super::meter_regions::regions(rows,width,height).ok_or("meter ledger unsupported camera")?;
            let layout=Layout::new(width,height,&regions).map_err(|e|format!("meter ledger layout: {e:?}"))?;
            let budget=Budget::new(width,height,layout.pixels,self.limits).map_err(|e|format!("meter ledger GPU budget: {e:?}"))?;
            let(map,jobs)=super::meter_ledger_state::uploads(&layout,budget).map_err(|e|format!("meter ledger uploads: {e:?}"))?;
            let a=self.api;
            let mut g=Generation{api:a,slots:ptr::null_mut(),jobs:ptr::null_mut(),map:ptr::null_mut(),compute:ptr::null_mut(),render:ptr::null_mut(),budget};
            g.slots=self.buffer("Ordered meter radiance contributions",budget.slots);
            g.jobs=self.buffer("Meter ledger full-frame sample jobs",budget.jobs);
            g.map=self.buffer("Meter ledger full-frame base-slot map",budget.map);
            if [g.slots,g.jobs,g.map].iter().any(|h|h.is_null()){return Err("meter ledger GPU allocation failed".into());}
            (a.wgpuQueueWriteBuffer)(queue,g.jobs,0,jobs.as_ptr().cast(),jobs.len()*16);
            (a.wgpuQueueWriteBuffer)(queue,g.map,0,map.as_ptr().cast(),map.len()*4);
            let mut ce:Vec<_>=render_entries.iter().copied().filter(|e|e.binding==1||e.binding==6).collect();
            if ce.len()!=2{return Err("meter ledger scene/metadata binding absent".into());}
            ce.extend([WGPUBindGroupEntry{binding:9,buffer:g.slots,size:budget.slots,..Default::default()},
                WGPUBindGroupEntry{binding:10,buffer:g.jobs,size:budget.jobs,..Default::default()}]);
            g.compute=(a.wgpuDeviceCreateBindGroup)(self.device,&WGPUBindGroupDescriptor{layout:self.compute_layout,
                entryCount:ce.len(),entries:ce.as_ptr(),..Default::default()});
            let mut re=render_entries.to_vec();
            let slot=re.iter_mut().find(|e|e.binding==9).ok_or("meter ledger slot binding absent")?;
            slot.buffer=g.slots;slot.size=budget.slots;
            let map=re.iter_mut().find(|e|e.binding==13).ok_or("meter ledger map binding absent")?;
            map.buffer=g.map;map.size=budget.map;
            g.render=(a.wgpuDeviceCreateBindGroup)(self.device,&WGPUBindGroupDescriptor{layout:render_layout,
                entryCount:re.len(),entries:re.as_ptr(),..Default::default()});
            if g.compute.is_null()||g.render.is_null(){return Err("meter ledger bind group failed".into());}
            check_errors()?;Ok(g)
        })();
        let g=match build {Ok(g)=>g,Err(error)=>{
            self.state.failed_before_encode();self.rejections+=1;self.last_admission="build rejected; canonical fallback".into();self.last_rejection=Some(error);check_errors()?;return Ok(false);
        }};
        let a=self.api;let pass=(a.wgpuCommandEncoderBeginComputePass)(encoder,&WGPUComputePassDescriptor{
            label:view("Build exact meter ledger after stable inputs"),..Default::default()});
        if pass.is_null(){self.state.failed_before_encode();return Err("meter ledger capture pass failed".into());}
        (a.wgpuComputePassEncoderSetPipeline)(pass,self.pipeline);
        (a.wgpuComputePassEncoderSetBindGroup)(pass,0,g.compute,0,ptr::null());
        (a.wgpuComputePassEncoderDispatchWorkgroups)(pass,g.budget.groups,1,1);
        (a.wgpuComputePassEncoderEnd)(pass);(a.wgpuComputePassEncoderRelease)(pass);
        self.peak_bytes=self.peak_bytes.max(g.budget.total);self.builds+=1;self.generation=Some(g);self.last_admission="capture encoded; publication pending completion and key check".into();
        // Build draw still uses dummy map. Publication requires later completed
        // serial AND another exact key check, never just command encoding.
        Ok(true)
    }
    pub(super) fn submitted(&mut self,serial:u64)->Result<(),String>{self.state.submitted(serial).map_err(str::to_string)}
    pub(super) fn info(&self)->Value {json!({"enabled":true,"playhead_certificate":self.playhead_certificate,"diagnostic_force_fallback":self.diagnostic_force_fallback,"selected":self.state.selected(),"builds":self.builds,"cache_draws":self.hits,
        "rejections":self.rejections,"ineligible_draws":self.ineligible_draws,"admission":self.last_admission,"last_rejection":self.last_rejection,"peak_requested_bytes":self.peak_bytes,
        "requested_bytes":self.generation.as_ref().map_or(super::meter_ledger_state::DUMMY_BYTES,|g|g.budget.total),
        "slot_bytes":SLOT_BYTES,"cap_bytes":super::meter_ledger_plan::CAP_BYTES,"stable_draws_required":2,
        "scope":if self.playhead_certificate{"One bounded GPU generation; full key except validated meter levels and per-sample proven-independent playhead X/disabled custom clock; uncertified/overflow samples execute canonical trace"}else{"One GPU generation; full packet except validated active meter levels; canonical per-sample overflow fallback"}})}
}
impl Drop for Runtime {fn drop(&mut self){self.generation.take();unsafe{
    let a=self.api;
    if !self.pipeline.is_null(){(a.wgpuComputePipelineRelease)(self.pipeline);}
    if !self.pipeline_layout.is_null(){(a.wgpuPipelineLayoutRelease)(self.pipeline_layout);}
    if !self.compute_layout.is_null(){(a.wgpuBindGroupLayoutRelease)(self.compute_layout);}
    for h in [self.dummy_slots,self.dummy_map]{if !h.is_null(){(a.wgpuBufferRelease)(h);}}
}}}
