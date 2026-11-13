#![allow(non_upper_case_globals)]
//! Native WebGPU client with a surface, pipeline, and verified readback.
use super::{wgpu::*, window::Window};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    ffi::{CStr, c_void},
    path::{Path, PathBuf},
    ptr,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
#[path="WorkstationBaked.rs"] mod baked;
#[path="WorkstationPrerender.rs"]
mod prerender;
#[path="WorkstationCameraSplit.rs"] mod camera_split;
#[path="WorkstationMeterRegions.rs"] mod meter_regions;
#[path="WorkstationMeterLedgerPlan.rs"] mod meter_ledger_plan;
#[path="WorkstationMeterLedgerState.rs"] mod meter_ledger_state;
#[path="WorkstationMeterLedgerRuntime.rs"] mod meter_ledger_runtime;
#[path="WorkstationBackdrop.rs"] mod backdrop;
#[path="CloudResolution.rs"] mod cloud_resolution;
static API: OnceLock<Result<Api, String>> = OnceLock::new();
static ERRORS: Mutex<Vec<String>> = Mutex::new(Vec::new());
fn api() -> Result<&'static Api, String> {
    API.get_or_init(|| unsafe {
        #[cfg(not(target_os = "ios"))]
        let path = std::env::var("WORMWOOD_WEBGPU_LIBRARY").unwrap_or_else(|_| {
            if cfg!(target_os = "macos") {
                "/opt/homebrew/lib/libwgpu_native.dylib"
            } else if cfg!(target_os = "windows") {
                "wgpu_native.dll"
            } else {
                "libwgpu_native.so"
            }
            .into()
        });
        #[cfg(not(target_os = "ios"))]
        {
            Api::new(path).map_err(|e| e.to_string())
        }
        #[cfg(target_os = "ios")]
        {
            Api::from_library(libloading::os::unix::Library::this()).map_err(|e| e.to_string())
        }
    })
    .as_ref()
    .map_err(Clone::clone)
}
// Explicit diagnostic only. Both targets use the same native device, format,
// shaders, dimensions and CPU loop. Offscreen never acquires/presents a drawable
// and completes each submission before returning; it is not an app FPS mode.
#[path="WorkstationFrozenDiagnostic.rs"] mod frozen;
#[path="WorkstationFinalRetention.rs"] mod final_retention;
#[path="WorkstationChangedUploads.rs"] mod changed_uploads;
#[path="WorkstationMotionBank.rs"] mod motion_bank;
#[path="WorkstationRefreshValidation.rs"] mod refresh_validation;
#[path="WorkstationPlayheadDamage.rs"] mod final_damage;
// Named here so the native binding allowlist includes this core WebGPU API.
unsafe fn dispatch_capture_jobs(a:&Api,pass:WGPUComputePassEncoder,buffer:WGPUBuffer){(a.wgpuComputePassEncoderDispatchWorkgroupsIndirect)(pass,buffer,4);}
unsafe fn copy_bake_buffer(a:&Api,e:WGPUCommandEncoder,source:WGPUBuffer,target:WGPUBuffer,size:u64){(a.wgpuCommandEncoderCopyBufferToBuffer)(e,source,0,target,0,size);}
unsafe fn copy_final_region(a:&Api,e:WGPUCommandEncoder,from:WGPUTexture,to:WGPUTexture,x:u32,y:u32,width:u32,height:u32){
 let origin=WGPUOrigin3D{x,y,z:0};
 (a.wgpuCommandEncoderCopyTextureToTexture)(e,&WGPUTexelCopyTextureInfo{texture:from,origin,aspect:WGPUTextureAspect_All,..Default::default()},&WGPUTexelCopyTextureInfo{texture:to,origin,aspect:WGPUTextureAspect_All,..Default::default()},&WGPUExtent3D{width,height,depthOrArrayLayers:1});
}
unsafe fn copy_frozen_texture(api:&Api,encoder:WGPUCommandEncoder,from:WGPUTexture,to:WGPUTexture,width:u32,height:u32){
 (api.wgpuCommandEncoderCopyTextureToTexture)(encoder,&WGPUTexelCopyTextureInfo{texture:from,aspect:WGPUTextureAspect_All,..Default::default()},&WGPUTexelCopyTextureInfo{texture:to,aspect:WGPUTextureAspect_All,..Default::default()},&WGPUExtent3D{width,height,depthOrArrayLayers:1});
}
fn diagnostic_offscreen_requested(workstation:bool, value:Option<&str>, overlap:bool)->Result<bool,String>{
 match value {
  None|Some("0")=>Ok(false),
  Some("1") if workstation&&!overlap=>Ok(true),
  Some("1")=>Err("Offscreen diagnostic requires workstation mode and FRAME_OVERLAP=0 (serial completed frames)".into()),
  _=>Err("WORMWOOD_WORKSTATION_DIAGNOSTIC_OFFSCREEN must be 0 or 1".into()),
 }
}

fn requested_surface_latency(value:Option<&str>)->Result<Option<u32>,String>{
    match value {None|Some("default")=>Ok(None),Some("1")=>Ok(Some(1)),Some("2")=>Ok(Some(2)),Some("3")=>Ok(Some(3)),
        _=>Err("WORMWOOD_WORKSTATION_SURFACE_LATENCY must be default, 1, 2 or 3".into())}
}
#[cfg(target_os="macos")]
unsafe fn metal_presentation_observation(window:&Window)->Result<Value,String>{
    // Read the actual attached CAMetalLayer, without changing its settings.
    let objc=libloading::Library::new("/usr/lib/libobjc.A.dylib").map_err(|e|e.to_string())?;
    let selector=*objc.get::<unsafe extern "C" fn(*const i8)->*mut c_void>(b"sel_registerName\0").map_err(|e|e.to_string())?;
    let object=*objc.get::<unsafe extern "C" fn(*mut c_void,*mut c_void)->*mut c_void>(b"objc_msgSend\0").map_err(|e|e.to_string())?;
    let responds=*objc.get::<unsafe extern "C" fn(*mut c_void,*mut c_void,*mut c_void)->bool>(b"objc_msgSend\0").map_err(|e|e.to_string())?;
    let integer=*objc.get::<unsafe extern "C" fn(*mut c_void,*mut c_void)->usize>(b"objc_msgSend\0").map_err(|e|e.to_string())?;
    let boolean=*objc.get::<unsafe extern "C" fn(*mut c_void,*mut c_void)->bool>(b"objc_msgSend\0").map_err(|e|e.to_string())?;
    let cocoa=window.symbol::<unsafe extern "C" fn(*mut c_void)->*mut c_void>(b"glfwGetCocoaWindow\0")?(window.handle);
    let content=object(cocoa,selector(c"contentView".as_ptr()));let layer=object(content,selector(c"layer".as_ptr()));
    if layer.is_null(){return Ok(Value::Null);}
    let count_selector=selector(c"maximumDrawableCount".as_ptr());let sync_selector=selector(c"displaySyncEnabled".as_ptr());
    let responds_selector=selector(c"respondsToSelector:".as_ptr());
    let count=responds(layer,responds_selector,count_selector).then(||integer(layer,count_selector));
    let sync=responds(layer,responds_selector,sync_selector).then(||boolean(layer,sync_selector));
    Ok(json!({"maximum_drawable_count":count,"display_sync_enabled":sync,
        "latency_from_drawable_capacity":count.and_then(|n|n.checked_sub(1)),
        "scope":"Read-back CAMetalLayer capacity after surface configuration. wgpu Metal maps frame latency to drawable count minus one; this is not measured queued frames, input latency or physical scanout."}))
}
fn view(text: &str) -> WGPUStringView {
    WGPUStringView {
        data: text.as_ptr().cast(),
        length: text.len(),
    }
}
unsafe fn string(text: WGPUStringView) -> String {
    if text.data.is_null() {
        return String::new();
    }
    if text.length == usize::MAX {
        CStr::from_ptr(text.data).to_string_lossy().into_owned()
    } else {
        String::from_utf8_lossy(std::slice::from_raw_parts(text.data.cast(), text.length))
            .into_owned()
    }
}
unsafe extern "C" fn error(
    _device: *const WGPUDevice,
    kind: WGPUErrorType,
    message: WGPUStringView,
    _a: *mut c_void,
    _b: *mut c_void,
) {
    if let Ok(mut errors) = ERRORS.lock() {
        errors.push(format!("WebGPU error {kind}: {}", string(message)));
    }
}
fn check_errors() -> Result<(), String> {
    let mut errors = ERRORS.lock().map_err(|_| "WebGPU error queue poisoned")?;
    if errors.is_empty() {
        Ok(())
    } else {
        Err(std::mem::take(&mut *errors).join("; "))
    }
}

fn surface_acquire_status(status: WGPUSurfaceGetCurrentTextureStatus) -> (&'static str, bool) {
    match status {
        WGPUSurfaceGetCurrentTextureStatus_SuccessOptimal => ("success_optimal", false),
        WGPUSurfaceGetCurrentTextureStatus_SuccessSuboptimal => ("success_suboptimal", false),
        WGPUSurfaceGetCurrentTextureStatus_Timeout => ("surface_timeout", true),
        WGPUSurfaceGetCurrentTextureStatus_Outdated => ("surface_outdated", true),
        WGPUSurfaceGetCurrentTextureStatus_Lost => ("surface_lost", true),
        WGPUSurfaceGetCurrentTextureStatus_OutOfMemory => ("out_of_memory", false),
        WGPUSurfaceGetCurrentTextureStatus_DeviceLost => ("device_lost", false),
        WGPUSurfaceGetCurrentTextureStatus_Error => ("surface_error", false),
        _ => ("unknown_surface_status", false),
    }
}

#[cfg(test)]
mod surface_status_tests {
    use super::*;

    #[test]
    fn surface_diagnostics_do_not_label_device_failures_as_window_recovery() {
        for status in [WGPUSurfaceGetCurrentTextureStatus_Timeout,
            WGPUSurfaceGetCurrentTextureStatus_Outdated,WGPUSurfaceGetCurrentTextureStatus_Lost] {
            assert!(surface_acquire_status(status).1);
        }
        for status in [WGPUSurfaceGetCurrentTextureStatus_OutOfMemory,
            WGPUSurfaceGetCurrentTextureStatus_DeviceLost,WGPUSurfaceGetCurrentTextureStatus_Error,
            WGPUSurfaceGetCurrentTextureStatus_SuccessOptimal,
            WGPUSurfaceGetCurrentTextureStatus_SuccessSuboptimal,0] {
            assert!(!surface_acquire_status(status).1);
        }
        assert_eq!(surface_acquire_status(WGPUSurfaceGetCurrentTextureStatus_DeviceLost).0,"device_lost");
        assert_eq!(surface_acquire_status(WGPUSurfaceGetCurrentTextureStatus_OutOfMemory).0,"out_of_memory");
    }
}
#[derive(Default)]
struct Wait {
    done: std::sync::atomic::AtomicBool,
    status: std::sync::atomic::AtomicU32,
    handle: std::sync::atomic::AtomicPtr<c_void>,
    abandoned: std::sync::atomic::AtomicBool,
}
unsafe extern "C" fn adapter_ready(
    status: WGPURequestAdapterStatus,
    handle: WGPUAdapter,
    _message: WGPUStringView,
    data: *mut c_void,
    _: *mut c_void,
) {
    let state = &*data.cast::<Wait>();
    if state.abandoned.load(std::sync::atomic::Ordering::Acquire) {
        if !handle.is_null() {
            if let Ok(api) = api() {
                (api.wgpuAdapterRelease)(handle);
            }
        }
        return;
    }
    state.status.store(status,std::sync::atomic::Ordering::Relaxed);
    state.handle.store(handle.cast(),std::sync::atomic::Ordering::Relaxed);
    state.done.store(true,std::sync::atomic::Ordering::Release);
}
unsafe extern "C" fn device_ready(
    status: WGPURequestDeviceStatus,
    handle: WGPUDevice,
    _message: WGPUStringView,
    data: *mut c_void,
    _: *mut c_void,
) {
    let state = &*data.cast::<Wait>();
    if state.abandoned.load(std::sync::atomic::Ordering::Acquire) {
        if !handle.is_null() {
            if let Ok(api) = api() {
                (api.wgpuDeviceRelease)(handle);
            }
        }
        return;
    }
    state.status.store(status,std::sync::atomic::Ordering::Relaxed);
    state.handle.store(handle.cast(),std::sync::atomic::Ordering::Relaxed);
    state.done.store(true,std::sync::atomic::Ordering::Release);
}
unsafe extern "C" fn mapped(
    status: WGPUMapAsyncStatus,
    _message: WGPUStringView,
    data: *mut c_void,
    _: *mut c_void,
) {
    let state = &*data.cast::<Wait>();
    state.status.store(status,std::sync::atomic::Ordering::Relaxed);
    state.done.store(true,std::sync::atomic::Ordering::Release);
}
unsafe fn wait(
    api: &Api,
    instance: WGPUInstance,
    device: WGPUDevice,
    mut state: Box<Wait>,
) -> Result<*mut c_void, String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !state.done.load(std::sync::atomic::Ordering::Acquire) {
        if !device.is_null() {
            (api.wgpuDevicePoll)(device, 1, ptr::null());
        }
        (api.wgpuInstanceProcessEvents)(instance);
        if Instant::now() > deadline {
            // A pending callback may still hold userdata. Keep it valid on the
            // error path; API code remains mapped for the process lifetime.
            state.abandoned.store(true,std::sync::atomic::Ordering::Release);
            Box::leak(state);
            return Err("WebGPU callback timed out; callback storage retained safely".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    if state.status.load(std::sync::atomic::Ordering::Relaxed) != 1 {
        return Err(format!(
            "WebGPU asynchronous operation failed: {}",
            state.status.load(std::sync::atomic::Ordering::Relaxed)
        ));
    }
    Ok(state.handle.load(std::sync::atomic::Ordering::Relaxed))
}
// Bounded delayed readback, adapted from Cardinal's GPU timing ring. Only
// profiling creates these resources; no timing-only wait occurs during draw.
struct TimingSlot {
    api: &'static Api,
    queries: WGPUQuerySet,
    resolve: WGPUBuffer,
    readback: WGPUBuffer,
    state: Box<Wait>,
    serial: u64,
    mapping: bool,
    active_passes: [bool; 4],
    query_count:u32,
    readback_bytes:usize,
    capture_active:bool,
    inside_active:bool,
    base_includes_camera:bool,
}
impl TimingSlot {
    unsafe fn new(api: &'static Api, device: WGPUDevice, attribution:bool) -> Result<Self, String> {
        let mut slot = Self {
            api,
            queries: ptr::null_mut(),
            resolve: ptr::null_mut(),
            readback: ptr::null_mut(),
            state: Box::new(Wait::default()),
            serial: 0,
            mapping: false,
            active_passes: [false, false, false, true],
            query_count:attribution_query_count(attribution),
            readback_bytes:attribution_query_count(attribution) as usize*8,
            capture_active:false,inside_active:false,base_includes_camera:false,
        };
        slot.queries = (api.wgpuDeviceCreateQuerySet)(
            device,
            &WGPUQuerySetDescriptor {
                label: view("GPU frame timestamps"),
                type_: WGPUQueryType_Timestamp,
                count: slot.query_count,
                ..Default::default()
            },
        );
        slot.resolve = (api.wgpuDeviceCreateBuffer)(
            device,
            &WGPUBufferDescriptor {
                label: view("GPU timestamp resolve"),
                size: 256,
                usage: WGPUBufferUsage_QueryResolve | WGPUBufferUsage_CopySrc,
                ..Default::default()
            },
        );
        slot.readback = (api.wgpuDeviceCreateBuffer)(
            device,
            &WGPUBufferDescriptor {
                label: view("GPU timestamp readback"),
                size: slot.readback_bytes as u64,
                usage: WGPUBufferUsage_CopyDst | WGPUBufferUsage_MapRead,
                ..Default::default()
            },
        );
        if slot.queries.is_null() || slot.resolve.is_null() || slot.readback.is_null() {
            return Err("WebGPU timestamp resource allocation failed".into());
        }
        check_errors()?;
        Ok(slot)
    }
}
impl Drop for TimingSlot {
    fn drop(&mut self) {
        unsafe {
            if self.mapping && !self.state.done.load(std::sync::atomic::Ordering::Acquire) {
                // Error teardown can cancel a map after this owner disappears.
                // Keep callback userdata alive rather than leave a dangling pointer.
                Box::leak(std::mem::take(&mut self.state));
            }
            if self.mapping && self.state.done.load(std::sync::atomic::Ordering::Acquire) && self.state.status.load(std::sync::atomic::Ordering::Relaxed) == 1 {
                (self.api.wgpuBufferUnmap)(self.readback);
            }
            for buffer in [self.readback, self.resolve] {
                if !buffer.is_null() {
                    (self.api.wgpuBufferRelease)(buffer);
                }
            }
            if !self.queries.is_null() {
                (self.api.wgpuQuerySetRelease)(self.queries);
            }
        }
    }
}
const GPU_INTERVAL_SEMANTICS: &str = "active_pass_interval_envelope_v2";
const ATTRIBUTION_QUERY_COUNT: u32 = 14;
const ATTRIBUTION_READBACK_BYTES: usize = ATTRIBUTION_QUERY_COUNT as usize * 8;
type InsidePassWrite = unsafe extern "C" fn(WGPURenderPassEncoder, WGPUQuerySet, u32);
struct InsidePassWriter {
    // Keep the same native library alive for the optional extension function.
    _library: libloading::Library,
    write: InsidePassWrite,
}
impl InsidePassWriter {
    unsafe fn load() -> Result<Self,String> {
        #[cfg(not(target_os="ios"))]
        let library = libloading::Library::new(std::env::var("WORMWOOD_WEBGPU_LIBRARY").unwrap_or_else(|_| {
            if cfg!(target_os="macos") { "/opt/homebrew/lib/libwgpu_native.dylib" }
            else if cfg!(target_os="windows") { "wgpu_native.dll" }
            else { "libwgpu_native.so" }.into()
        })).map_err(|e|e.to_string())?;
        #[cfg(target_os="ios")]
        let library:libloading::Library=libloading::os::unix::Library::this().into();
        // Header ABI: void (... WGPURenderPassEncoder, WGPUQuerySet, uint32_t).
        let write=*library.get::<InsidePassWrite>(b"wgpuRenderPassEncoderWriteTimestamp\0").map_err(|e|e.to_string())?;
        Ok(Self{_library:library,write})
    }
}

// Inside-pass timestamps are diagnostic observations, not additive/exclusive
// shader times. Reject reversed, overlapping or out-of-pass results explicitly.
fn checked_inside_intervals(ticks:&[u64;14], active:bool)->Option<[u64;2]> {
    if !active || !(ticks[6]<=ticks[8] && ticks[8]<=ticks[9]
        && ticks[9]<=ticks[10] && ticks[10]<=ticks[11] && ticks[11]<=ticks[7]) { return None; }
    Some([ticks[9]-ticks[8],ticks[11]-ticks[10]])
}

fn attribution_query_count(requested:bool)->u32 { if requested { ATTRIBUTION_QUERY_COUNT } else { 8 } }


#[derive(Debug, PartialEq, Eq)]
struct TimestampIntervalSummary {
    envelope_ticks: u64,
    union_ticks: u64,
    overlap_ticks: u128,
    uncovered_ticks: u64,
}

// Pass boundaries can overlap or occur out of encoding order, notably Metal's
// start-of-vertex/end-of-fragment interval. These are spans, not exclusive work.
fn timestamp_interval_summary(
    ticks: &[u64],
    active: &[bool],
) -> Option<TimestampIntervalSummary> {
    if ticks.len()!=active.len()*2 {return None;}
    let mut intervals = vec![(0_u64, 0_u64); active.len()];
    let mut count = 0;
    let mut sum = 0_u128;
    for index in 0..active.len() {
        if !active[index] { continue; }
        let (start, end) = (ticks[index * 2], ticks[index * 2 + 1]);
        sum += u128::from(end.checked_sub(start)?);
        intervals[count] = (start, end);
        count += 1;
    }
    if count == 0 { return None; }
    intervals[..count].sort_unstable();
    let first = intervals[0].0;
    let (mut begin, mut end) = intervals[0];
    let mut union = 0_u64;
    for &(next_begin, next_end) in &intervals[1..count] {
        if next_begin > end {
            union += end - begin;
            begin = next_begin;
            end = next_end;
        } else {
            end = end.max(next_end);
        }
    }
    union += end - begin;
    let envelope = end - first;
    Some(TimestampIntervalSummary {
        envelope_ticks: envelope,
        union_ticks: union,
        overlap_ticks: sum - u128::from(union),
        uncovered_ticks: envelope - union,
    })
}


#[cfg(test)]
mod timestamp_interval_tests {
    use super::{timestamp_interval_summary as summary, TimestampIntervalSummary};
    fn expected(envelope: u64, union: u64, overlap: u128, uncovered: u64) -> TimestampIntervalSummary {
        TimestampIntervalSummary { envelope_ticks: envelope, union_ticks: union,
            overlap_ticks: overlap, uncovered_ticks: uncovered }
    }
    #[test]
    fn render_begin_can_precede_encoded_compute_begin() {
        assert_eq!(summary(&[110,120, 0,0, 120,125, 100,200], &[true,false,true,true]),
            Some(expected(100,100,15,0)));
    }
    #[test]
    fn disjoint_spans_preserve_uncovered_time() {
        assert_eq!(summary(&[10,20, 30,40, 0,0, 50,60], &[true,true,false,true]),
            Some(expected(50,30,0,20)));
    }
    #[test]
    fn overlap_and_gaps_are_separate() {
        assert_eq!(summary(&[10,30, 20,40, 50,60, 0,0], &[true,true,true,false]),
            Some(expected(50,40,10,10)));
    }
    #[test]
    fn inactive_garbage_is_ignored() {
        assert_eq!(summary(&[u64::MAX,0, 80,1, 1000,0, 10,20], &[false,false,false,true]),
            Some(expected(10,10,0,0)));
    }
    #[test]
    fn reversed_active_interval_is_invalid() {
        assert_eq!(summary(&[20,10, 0,0, 0,0, 10,30], &[true,false,false,true]), None);
    }
    #[test]
    fn zero_duration_is_valid_but_no_active_interval_is_not() {
        assert_eq!(summary(&[0,0, 0,0, 0,0, 7,7], &[false,false,false,true]),
            Some(expected(0,0,0,0)));
        assert_eq!(summary(&[0;8], &[false;4]), None);
    }
    #[test]
    fn interval_sum_does_not_overflow_u64() {
        assert_eq!(summary(&[0,u64::MAX, 0,u64::MAX, 0,u64::MAX, 0,u64::MAX], &[true;4]),
            Some(expected(u64::MAX,u64::MAX,3*u128::from(u64::MAX),0)));
    }
}

struct GpuTiming {
    api: &'static Api,
    slots: Vec<TimingSlot>,
    current: Option<usize>,
    period_ns: f64,
    output: PathBuf,
    dimensions: [u32; 2],
    samples: Vec<(u64, f64)>,
    serial: u64,
    supported: bool,
    legacy: bool,
    log: Option<super::performance::GpuLog>,
    attribution_requested:bool,
    full_game_frame:bool,
    inside_pass_writer:Option<InsidePassWriter>,
    inside_pass_reason:Option<String>,
}
impl GpuTiming {
    const SAMPLE_LIMIT: usize = 512;
    unsafe fn new(
        api: &'static Api,
        device: WGPUDevice,
        queue: WGPUQueue,
        output: &Path,
        dimensions: [u32; 2],
        workstation: bool,
        legacy: bool,
        supported: bool,
        attribution_requested:bool,
        inside_pass_writer:Option<InsidePassWriter>,
        inside_pass_reason:Option<String>,
    ) -> Result<Self, String> {
        let period_ns = if supported {
            (api.wgpuQueueGetTimestampPeriod)(queue) as f64
        } else {
            0.
        };
        let supported = supported && period_ns.is_finite() && period_ns > 0.;
        let full_game_frame=!workstation&&std::env::var("WORMWOOD_GAME_GPU_FULL_FRAME").as_deref()==Ok("1");
        let scope = if workstation {
            "Envelope of active workstation GPU pass-boundary intervals (v2): minimum active begin to maximum active end. Intervals can overlap and include dependency waits; not exclusive shader cost or GPU busy time. Excludes CPU, presentation, query resolve and readback."
        } else if full_game_frame {
            "Game render envelope from the first opaque/reflection pass begin through the last vegetation/water pass end. Includes gaps and dependencies between those passes; excludes CPU, presentation and readback. Not exclusive shader cost or GPU occupancy."
        } else {
            "Primary opaque render pass, measured by render-pass begin/end timestamp writes; reflection, vegetation, water, presentation and readback are not included."
        };
        let mut log = super::performance::enabled().then(|| {
            super::performance::GpuLog::new(
                output,
                "WebGPU",
                workstation,
                supported,
                "async_timestamp",
                scope,
            )
        });
        if let Some(log) = &mut log {
            log.set_query_context(json!({
                "query_schema_version": 2,
                "gpu_duration_semantics": GPU_INTERVAL_SEMANTICS,
                "pass_intervals_exclusive": false,
                "pass_intervals_additive": false,
                "metal_render_boundary_scope": "start_of_vertex_to_end_of_fragment",
                "full_game_frame":full_game_frame,
            }));
        }
        let mut timing = Self {
            api,
            slots: Vec::new(),
            current: None,
            period_ns,
            output: output.join("workstation-gpu-timing-WebGPU.json"),
            dimensions,
            samples: Vec::new(),
            serial: 0,
            supported,
            legacy,
            log,
            attribution_requested,full_game_frame,inside_pass_writer,inside_pass_reason,
        };
        if supported {
            for _ in 0..4 {
                timing.slots.push(TimingSlot::new(api, device, attribution_requested)?);
            }
        }
        let attribution=timing.attribution_info();
        if let Some(log)=&mut timing.log {
            log.set_query_context(json!({
                "query_schema_version":2,"gpu_duration_semantics":GPU_INTERVAL_SEMANTICS,
                "pass_intervals_exclusive":false,"pass_intervals_additive":false,
                "metal_render_boundary_scope":"start_of_vertex_to_end_of_fragment",
                "attribution":attribution,
            }));
        }
        timing.write_legacy_report()?;
        Ok(timing)
    }
    fn begin(&mut self) {
        self.serial += 1;
        if let Some(log) = &mut self.log {
            log.submitted(self.serial);
        }
        if !self.supported || (self.log.is_none() && self.serial > Self::SAMPLE_LIMIT as u64) {
            return;
        }
        self.current = self.slots.iter().position(|slot| slot.serial == 0);
        if let Some(index) = self.current {
            let slot = &mut self.slots[index];
            slot.serial = self.serial;
            slot.active_passes = [false, false, false, true];
            slot.capture_active=false;slot.inside_active=false;slot.base_includes_camera=false;
            *slot.state = Wait::default();
        } else if let Some(log) = &mut self.log {
            log.missing(self.serial);
        }
    }
    fn writes(&self) -> Option<WGPURenderPassTimestampWrites> {
        self.current.map(|index| WGPURenderPassTimestampWrites {
            querySet: self.slots[index].queries,
            beginningOfPassWriteIndex: 6,
            endOfPassWriteIndex: 7,
        })
    }
    fn game_writes(&self,pass:u32)->Option<WGPURenderPassTimestampWrites>{
        self.current.filter(|_|self.full_game_frame&&pass<3).map(|i|WGPURenderPassTimestampWrites{
            querySet:self.slots[i].queries,beginningOfPassWriteIndex:pass*2,endOfPassWriteIndex:pass*2+1})
    }
    fn compute_writes(&self, pass: u32) -> Option<WGPUComputePassTimestampWrites> {
        self.current.filter(|_| pass < 3).map(|index| WGPUComputePassTimestampWrites {
            querySet: self.slots[index].queries,
            beginningOfPassWriteIndex: pass * 2,
            endOfPassWriteIndex: pass * 2 + 1,
        })
    }
    fn attribution_info(&self)->Value {
        json!({"requested":self.attribution_requested,
            "inside_pass_supported":self.supported&&self.inside_pass_writer.is_some(),
            "inside_pass_skip_reason":if !self.supported {Some("Usable timestamp queries unavailable".to_string())} else {self.inside_pass_reason.clone()},
            "capture_pass_supported":self.attribution_requested&&self.supported,
            "query_count":attribution_query_count(self.attribution_requested),
            "query_pairs":{"base_without_meters":[8,9],"meter_overlay":[10,11],"retained_capture":[12,13]},
            "scope":"Optional same-render-pass timestamp observations; no added render pass or encoder timestamps. Base begins after the separate camera draw (or includes camera if unsplit). Meter span includes every original scissored overlay draw. Capture uses compute pass begin/end. Subintervals never expand the all-pass envelope or its union. Unsupported adapters retain the original render path."})
    }
    fn capture_writes(&self)->Option<WGPUComputePassTimestampWrites> {
        if !self.attribution_requested{return None;}
        self.current.map(|i|WGPUComputePassTimestampWrites{querySet:self.slots[i].queries,
            beginningOfPassWriteIndex:12,endOfPassWriteIndex:13})
    }
    fn capture_pass(&mut self,active:bool) {
        if let Some(i)=self.current {self.slots[i].capture_active=active&&self.attribution_requested;}
    }
    unsafe fn inside_begin(&mut self,pass:WGPURenderPassEncoder,includes_camera:bool) {
        if let (Some(i),Some(writer))=(self.current,&self.inside_pass_writer) {
            let slot=&mut self.slots[i];slot.inside_active=true;slot.base_includes_camera=includes_camera;
            (writer.write)(pass,slot.queries,8);
        }
    }
    unsafe fn inside_mark(&self,pass:WGPURenderPassEncoder,index:u32) {
        if !(9..=11).contains(&index){return;}
        if let (Some(i),Some(writer))=(self.current,&self.inside_pass_writer) {
            let slot=&self.slots[i];if slot.inside_active{(writer.write)(pass,slot.queries,index);}
        }
    }
    fn optical_passes(&mut self, baked: bool, resolved: bool) {
        if let Some(index) = self.current {
            self.slots[index].active_passes = [baked, baked, resolved, true];
        }
    }
    unsafe fn encode_resolve(&self, encoder: WGPUCommandEncoder) {
        let Some(index) = self.current else {
            return;
        };
        let slot = &self.slots[index];
        (self.api.wgpuCommandEncoderResolveQuerySet)(encoder, slot.queries, 0, slot.query_count, slot.resolve, 0);
        (self.api.wgpuCommandEncoderCopyBufferToBuffer)(
            encoder,
            slot.resolve,
            0,
            slot.readback,
            0,
            slot.readback_bytes as u64,
        );
    }
    unsafe fn submitted(&mut self) {
        let Some(index) = self.current.take() else {
            return;
        };
        let slot = &mut self.slots[index];
        slot.mapping = true;
        let callback = WGPUBufferMapCallbackInfo {
            mode: WGPUCallbackMode_AllowProcessEvents,
            callback: Some(mapped),
            userdata1: (&mut *slot.state as *mut Wait).cast(),
            ..Default::default()
        };
        (self.api.wgpuBufferMapAsync)(slot.readback, WGPUMapMode_Read, 0, slot.readback_bytes, callback);
    }
    unsafe fn poll(&mut self) -> Result<(), String> {
        let mut completed = self
            .slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.mapping && slot.state.done.load(std::sync::atomic::Ordering::Acquire))
            .map(|(index, slot)| (slot.serial, index))
            .collect::<Vec<_>>();
        completed.sort_unstable();
        let changed = !completed.is_empty();
        for (serial, index) in completed {
            let slot = &mut self.slots[index];
            let mut duration = None;
            if slot.state.status.load(std::sync::atomic::Ordering::Relaxed) == 1 {
                let mapped = (self.api.wgpuBufferGetConstMappedRange)(slot.readback, 0, slot.readback_bytes);
                if !mapped.is_null() {
                    let all_ticks:[u64;14]=std::array::from_fn(|i|if i<slot.query_count as usize {
                        ptr::read_unaligned(mapped.cast::<u64>().add(i))}else{0});
                    // Preserve the original eight-query payload and pair meanings.
                    let ticks:[u64;8]=std::array::from_fn(|i|all_ticks[i]);
                    let envelope_ticks:[u64;10]=std::array::from_fn(|i|if i<8{ticks[i]}else{all_ticks[i+4]});
                    let envelope_active=[slot.active_passes[0],slot.active_passes[1],slot.active_passes[2],slot.active_passes[3],slot.capture_active];
                    let inside=checked_inside_intervals(&all_ticks,slot.inside_active);
                    let milliseconds = |start: u64, end: u64| end.checked_sub(start)
                        .map(|value| value as f64 * self.period_ns / 1_000_000.);
                    let tick_ms = |value: u128| value as f64 * self.period_ns / 1_000_000.;
                    let first = slot.active_passes.iter().position(|active| *active).unwrap_or(3);
                    let legacy_duration = milliseconds(ticks[first*2], ticks[7]);
                    let intervals = timestamp_interval_summary(&envelope_ticks, &envelope_active);
                    duration = intervals.as_ref().map(|value| tick_ms(u128::from(value.envelope_ticks)));
                    if let Some(log) = &mut self.log {
                        let per_pass_ms: [Option<f64>; 4] = std::array::from_fn(|i|
                            slot.active_passes[i].then(|| milliseconds(ticks[i*2], ticks[i*2+1])).flatten());
                        log.observe_query(serial, json!({
                            "query_schema_version":2,
                            "gpu_duration_semantics":GPU_INTERVAL_SEMANTICS,
                            "pass_order":if self.full_game_frame{["vegetation_shadow_and_color","water_surface_and_composite","reflected_opaque","primary_opaque"]}else{["panel_bake","general_bake","lighting_resolve","optical_render_pass"]},
                            "active_passes":slot.active_passes,"timestamp_ticks":ticks,
                            "attribution":{
                                "inside_active":slot.inside_active,
                                "inside_intervals_valid":inside.is_some(),
                                "inside_invalid_reason":(slot.inside_active&&inside.is_none()).then_some("Inside-pass timestamps reversed, overlapping or outside enclosing render pass; no subinterval cost reported"),
                                "inside_timestamp_ticks":slot.inside_active.then_some([all_ticks[8],all_ticks[9],all_ticks[10],all_ticks[11]]),
                                "inside_ms":inside.map(|v|[tick_ms(u128::from(v[0])),tick_ms(u128::from(v[1]))]),
                                "inside_order":["base_without_meters","meter_overlay"],
                                "base_includes_camera":slot.base_includes_camera,
                                "capture_active":slot.capture_active,
                                "capture_timestamp_ticks":slot.capture_active.then_some([all_ticks[12],all_ticks[13]]),
                                "capture_ms":slot.capture_active.then(||milliseconds(all_ticks[12],all_ticks[13])).flatten(),
                                "envelope_pair_order":if self.full_game_frame{["vegetation_shadow_and_color","water_surface_and_composite","reflected_opaque","primary_opaque","retained_capture"]}else{["panel_bake","general_bake","lighting_resolve","optical_render_pass","retained_capture"]},
                                "subintervals_exclusive":false,"subintervals_additive":false,
                            },
                            "timestamp_period_ns":self.period_ns,"per_pass_ms":per_pass_ms,
                            "all_passes_ms":duration,
                            "all_passes_ms_semantics":"active_pass_interval_envelope_v2",
                            "active_pass_envelope_ms":duration,
                            "legacy_first_active_to_render_end_ms":legacy_duration,
                            "active_intervals_valid":intervals.is_some(),
                            "interval_union_ms":intervals.as_ref().map(|value|tick_ms(u128::from(value.union_ticks))),
                            "interval_overlap_ms":intervals.as_ref().map(|value|tick_ms(value.overlap_ticks)),
                            "interval_uncovered_ms":intervals.as_ref().map(|value|tick_ms(u128::from(value.uncovered_ticks))),
                            "pass_intervals_exclusive":false,"pass_intervals_additive":false,
                            "scope":"Raw GPU pass-boundary intervals. Metal render spans vertex start to fragment end and can overlap compute or include dependency waits. Overlap is repeated interval coverage, not parallel ALU occupancy; uncovered time is not proof of GPU idleness. Excludes CPU, presentation and readback."}));
                    }
                }
                (self.api.wgpuBufferUnmap)(slot.readback);
            }
            slot.mapping = false;
            slot.serial = 0;
            if let Some(ms) = duration.filter(|ms| ms.is_finite() && *ms > 0.) {
                if self.legacy && serial <= Self::SAMPLE_LIMIT as u64 {
                    self.samples.push((serial, ms));
                }
                if let Some(log) = &mut self.log {
                    log.record(serial, ms);
                }
            } else if let Some(log) = &mut self.log {
                log.missing(serial);
            }
        }
        // Legacy finite diagnostics retain their per-draw JSON. Ordinary
        // telemetry writes its bounded report only when the session closes.
        if self.legacy && changed {
            self.write_legacy_report()?;
        }
        Ok(())
    }
    fn write_legacy_report(&self) -> Result<(), String> {
        if !self.legacy {
            return Ok(());
        }
        let mut samples = self.samples.clone();
        samples.sort_unstable_by_key(|sample| sample.0);
        let values = samples.iter().map(|sample| sample.1).collect::<Vec<_>>();
        let report = json!({"enabled": true, "supported": self.supported,
            "backend": "WebGPU", "dimensions": self.dimensions,
            "attribution":self.attribution_info(),
            "scope": "Envelope of active GPU pass-boundary intervals, which may overlap and include dependency waits. Not exclusive shader cost, GPU busy time or presentation latency.",
            "timestamp_source": GPU_INTERVAL_SEMANTICS,
            "gpu_duration_semantics": GPU_INTERVAL_SEMANTICS,
            "reason": (!self.supported).then_some("Adapter does not expose usable timestamp queries; GPU time was not measured."),
            "timestamp_period_ns": self.period_ns, "draws": values.len(),
            "submitted_draws": self.serial, "sample_limit": Self::SAMPLE_LIMIT,
            "capture_complete": self.serial >= Self::SAMPLE_LIMIT as u64,
            "draw_indices": samples.iter().map(|sample| sample.0).collect::<Vec<_>>(), "gpu_ms": values,
            "mean_ms": (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64),
            "minimum_ms": values.iter().copied().reduce(f64::min),
            "maximum_ms": values.iter().copied().reduce(f64::max)});
        std::fs::write(
            &self.output,
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("{}: {e}", self.output.display()))
    }
}
impl Drop for GpuTiming {
    fn drop(&mut self) {
        unsafe {
            let _ = self.poll();
        }
        let _ = self.write_legacy_report();
        if let Some(log) = &mut self.log {
            log.finish();
        }
    }
}

#[derive(Default)]
struct SpecializationScope {
    done: std::sync::atomic::AtomicBool,
    error: std::sync::Mutex<Option<String>>,
}
// wgpu-native 27 accepts these two filters. Its headers also declare Internal,
// but passing that value aborts inside the C ABI rather than returning an error.
const SPECIALIZATION_ERROR_FILTERS: [WGPUErrorFilter; 2] = [
    WGPUErrorFilter_Validation,
    WGPUErrorFilter_OutOfMemory,
];
unsafe extern "C" fn specialization_scope_ready(
    status: WGPUPopErrorScopeStatus, kind: WGPUErrorType, message: WGPUStringView,
    data: *mut c_void, _: *mut c_void,
) {
    let state = &*data.cast::<SpecializationScope>();
    if status != WGPUPopErrorScopeStatus_Success || kind != WGPUErrorType_NoError {
        *state.error.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(format!("optional workstation pipeline: scope {status}, error {kind}: {}", string(message)));
    }
    state.done.store(true,std::sync::atomic::Ordering::Release);
}
unsafe fn finish_specialization_scopes(api: &Api, instance: WGPUInstance, device: WGPUDevice) -> Result<(), String> {
    let mut states: Vec<Box<SpecializationScope>> = SPECIALIZATION_ERROR_FILTERS.iter()
        .map(|_| Box::default()).collect();
    // Pop every scope before polling so even an optional compile failure leaves
    // the device's ordinary error handling intact.
    for state in &mut states {
        (api.wgpuDevicePopErrorScope)(device, WGPUPopErrorScopeCallbackInfo {
            mode: WGPUCallbackMode_AllowProcessEvents,
            callback: Some(specialization_scope_ready),
            userdata1: (&mut **state as *mut SpecializationScope).cast(),
            ..Default::default()
        });
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    while states.iter().any(|state| !state.done.load(std::sync::atomic::Ordering::Acquire)) {
        (api.wgpuDevicePoll)(device, 0, ptr::null());
        (api.wgpuInstanceProcessEvents)(instance);
        if Instant::now() >= deadline {
            // AllowProcessEvents callbacks may still arrive during later draws
            // or shutdown. Retain only pending userdata, never a stack pointer.
            for state in states { if !state.done.load(std::sync::atomic::Ordering::Acquire) { Box::leak(state); } }
            return Err("optional workstation pipeline error-scope callback timed out; pending userdata retained".into());
        }
        if states.iter().any(|state| !state.done.load(std::sync::atomic::Ordering::Acquire)) { std::thread::sleep(Duration::from_millis(1)); }
    }
    let errors: Vec<String> = states.into_iter().filter_map(|state| state.error.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner)).collect();
    if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
}

struct ControlsPipeline {
    api: &'static Api,
    shader: WGPUShaderModule,
    pipeline: WGPURenderPipeline,
    bind_group: WGPUBindGroup,
}
impl ControlsPipeline {
    unsafe fn new(client: &Client, original: &str) -> Result<Self, String> {
        let source = crate::workstation::shader_specialization::controls_only_source(original)?;
        Self::from_source(client, &source, true)
    }
    unsafe fn from_source(client: &Client, source: &str, blended: bool) -> Result<Self,String> {


        let api = client.api;
        let mut result = Self { api, shader: ptr::null_mut(), pipeline: ptr::null_mut(), bind_group: ptr::null_mut() };
        for filter in SPECIALIZATION_ERROR_FILTERS {
            (api.wgpuDevicePushErrorScope)(client.device, filter);
        }
        let created = (|| -> Result<(), String> {
            let wgsl = WGPUShaderSourceWGSL {
                chain: WGPUChainedStruct { next: ptr::null(), sType: WGPUSType_ShaderSourceWGSL },
                code: view(&source),
            };
            result.shader = (api.wgpuDeviceCreateShaderModule)(client.device, &WGPUShaderModuleDescriptor {
                nextInChain: &wgsl.chain, label: view("Workstation controls-only optional shader"),
            });
            if result.shader.is_null() { return Err("optional workstation shader creation failed".into()); }
            let blend = WGPUBlendState {
                color: WGPUBlendComponent { operation: WGPUBlendOperation_Add,
                    srcFactor: WGPUBlendFactor_SrcAlpha, dstFactor: WGPUBlendFactor_OneMinusSrcAlpha },
                alpha: WGPUBlendComponent { operation: WGPUBlendOperation_Add,
                    srcFactor: WGPUBlendFactor_One, dstFactor: WGPUBlendFactor_OneMinusSrcAlpha },
            };
            let target = WGPUColorTargetState {
                blend: if blended {&blend} else {ptr::null()}, format: client.format, writeMask: WGPUColorWriteMask_All, ..Default::default()
            };
            let fragment = WGPUFragmentState {
                module: result.shader, entryPoint: view("fs_main"), targetCount: 1, targets: &target, ..Default::default()
            };
            let depth = WGPUDepthStencilState {
                format: WGPUTextureFormat_Depth32Float, depthWriteEnabled: WGPUOptionalBool_True,
                depthCompare: WGPUCompareFunction_Less, ..Default::default()
            };
            result.pipeline = (api.wgpuDeviceCreateRenderPipeline)(client.device, &WGPURenderPipelineDescriptor {
                label: view("Workstation controls-only optional pipeline"),
                vertex: WGPUVertexState { module: result.shader, entryPoint: view("vs_main"), ..Default::default() },
                primitive: WGPUPrimitiveState { topology: WGPUPrimitiveTopology_TriangleList,
                    frontFace: WGPUFrontFace_CCW, cullMode: WGPUCullMode_None, ..Default::default() },
                multisample: WGPUMultisampleState { count: 1, mask: u32::MAX, ..Default::default() },
                fragment: &fragment, depthStencil: &depth, ..Default::default()
            });
            if result.pipeline.is_null() { return Err("optional workstation pipeline creation failed".into()); }
            // Auto layouts belong to their pipeline. Never reuse the full
            // shader's auto-layout bind group, even when bindings look equal.
            let layout = (api.wgpuRenderPipelineGetBindGroupLayout)(result.pipeline, 0);
            if layout.is_null() { return Err("optional workstation bind layout failed".into()); }
            let entries = [
                WGPUBindGroupEntry { binding: 0, buffer: client.vertex_buffer, size: crate::scene_limits::SCENE_BUFFER_BYTES, ..Default::default() },
                WGPUBindGroupEntry { binding: 1, buffer: client.texture_buffer,
                    size: client.texture_capacity as u64, ..Default::default() },
            ];
            result.bind_group = (api.wgpuDeviceCreateBindGroup)(client.device, &WGPUBindGroupDescriptor {
                layout, entryCount: entries.len(), entries: entries.as_ptr(), ..Default::default()
            });
            (api.wgpuBindGroupLayoutRelease)(layout);
            if result.bind_group.is_null() { return Err("optional workstation bind group failed".into()); }
            Ok(())
        })();
        let scoped = finish_specialization_scopes(api, client.instance, client.device);
        created?;
        scoped?;
        Ok(result)
    }
}
impl Drop for ControlsPipeline {
    fn drop(&mut self) {
        unsafe {
            if !self.bind_group.is_null() { (self.api.wgpuBindGroupRelease)(self.bind_group); }
            if !self.pipeline.is_null() { (self.api.wgpuRenderPipelineRelease)(self.pipeline); }
            if !self.shader.is_null() { (self.api.wgpuShaderModuleRelease)(self.shader); }
        }
    }
}

// Runtime optical responses own their storage independently of the scene
// packet. Ordered panel and general compute passes complete changed responses
// before lighting and rendering; ordinary frames reuse the same allocation.
struct RubyResponsePipeline {
    api: &'static Api,
    shader: WGPUShaderModule,
    render: WGPURenderPipeline,
    compute: WGPUComputePipeline,
    general_bake: WGPUComputePipeline,
    resolve: WGPUComputePipeline,
    bind_layout: WGPUBindGroupLayout,
    pipeline_layout: WGPUPipelineLayout,
    bind_group: WGPUBindGroup,
    response_buffer: WGPUBuffer,
    plan_buffer: WGPUBuffer,
    incremental_entries:Vec<[u32;4]>,track_incremental:bool,
    entry_count: u32,
    ready_count: usize,
    admitted_pixel_entries: usize,
    admitted_timeline_entries: usize,
    admitted_pixels: usize,
    allocated_pixel_rows: usize,
    bake_pending: bool,
    bake_submissions: u64,
    resolve_submissions: u64,
    resolve_reuse: crate::workstation::ruby_resolve_reuse::Reuse,
}
impl RubyResponsePipeline {
    unsafe fn new(client: &Client, original: &str) -> Result<Self, String> {
        use crate::workstation::ruby_response_plan::{CACHE_BYTES, PLAN_ROWS};
        let source = crate::workstation::ruby_response_shader::source(original)?;
        let api = client.api;
        let mut result = Self {
            api, shader: ptr::null_mut(), render: ptr::null_mut(), compute: ptr::null_mut(),
            general_bake: ptr::null_mut(), resolve: ptr::null_mut(),
            bind_layout: ptr::null_mut(), pipeline_layout: ptr::null_mut(), bind_group: ptr::null_mut(),
            response_buffer: ptr::null_mut(), plan_buffer: ptr::null_mut(),
            incremental_entries:vec![],track_incremental:client.incremental_mode,entry_count: 0, ready_count: 0, bake_pending: false, bake_submissions: 0,
            admitted_pixel_entries: 0, admitted_timeline_entries: 0,
            admitted_pixels: 0, allocated_pixel_rows: 0,
            resolve_submissions: 0,
            resolve_reuse: crate::workstation::ruby_resolve_reuse::Reuse::new(),
        };
        for filter in SPECIALIZATION_ERROR_FILTERS {
            (api.wgpuDevicePushErrorScope)(client.device, filter);
        }
        let created = (|| -> Result<(), String> {
            result.response_buffer = (api.wgpuDeviceCreateBuffer)(client.device, &WGPUBufferDescriptor {
                label: view("Workstation ruby optical responses"), size: CACHE_BYTES as u64,
                usage: WGPUBufferUsage_Storage | if client.incremental_mode{WGPUBufferUsage_CopySrc}else{0}, ..Default::default()
            });
            result.plan_buffer = (api.wgpuDeviceCreateBuffer)(client.device, &WGPUBufferDescriptor {
                label: view("Workstation ruby response plan"), size: (PLAN_ROWS * 16) as u64,
                usage: WGPUBufferUsage_Storage | WGPUBufferUsage_CopyDst | if client.incremental_mode{WGPUBufferUsage_CopySrc}else{0}, ..Default::default()
            });
            if result.response_buffer.is_null() || result.plan_buffer.is_null() {
                return Err("optional ruby response allocation failed".into());
            }
            let buffers = [client.vertex_buffer, client.texture_buffer, result.response_buffer, result.plan_buffer];
            let sizes = [crate::scene_limits::SCENE_BUFFER_BYTES, client.texture_capacity as u64, CACHE_BYTES as u64, (PLAN_ROWS * 16) as u64];
            let mut layouts = [WGPUBindGroupLayoutEntry::default(); 4];
            let mut bindings = [WGPUBindGroupEntry::default(); 4];
            for index in 0..4 {
                layouts[index].binding = index as u32;
                layouts[index].visibility = if index == 0 { WGPUShaderStage_Vertex }
                    else { WGPUShaderStage_Compute | WGPUShaderStage_Fragment };
                layouts[index].buffer = WGPUBufferBindingLayout {
                    type_: if index == 2 { WGPUBufferBindingType_Storage } else { WGPUBufferBindingType_ReadOnlyStorage },
                    minBindingSize: sizes[index], ..Default::default()
                };
                bindings[index] = WGPUBindGroupEntry {
                    binding: index as u32, buffer: buffers[index], size: sizes[index], ..Default::default()
                };
            }
            result.bind_layout = (api.wgpuDeviceCreateBindGroupLayout)(client.device, &WGPUBindGroupLayoutDescriptor {
                label: view("Workstation ruby response bindings"), entryCount: layouts.len(),
                entries: layouts.as_ptr(), ..Default::default()
            });
            if result.bind_layout.is_null() { return Err("optional ruby response bind layout failed".into()); }
            result.pipeline_layout = (api.wgpuDeviceCreatePipelineLayout)(client.device, &WGPUPipelineLayoutDescriptor {
                label: view("Workstation ruby response pipeline layout"), bindGroupLayoutCount: 1,
                bindGroupLayouts: &result.bind_layout, ..Default::default()
            });
            if result.pipeline_layout.is_null() { return Err("optional ruby response pipeline layout failed".into()); }
            result.bind_group = (api.wgpuDeviceCreateBindGroup)(client.device, &WGPUBindGroupDescriptor {
                label: view("Workstation ruby response scene and cache"), layout: result.bind_layout,
                entryCount: bindings.len(), entries: bindings.as_ptr(), ..Default::default()
            });
            if result.bind_group.is_null() { return Err("optional ruby response bind group failed".into()); }
            let wgsl = WGPUShaderSourceWGSL {
                chain: WGPUChainedStruct { next: ptr::null(), sType: WGPUSType_ShaderSourceWGSL }, code: view(&source),
            };
            result.shader = (api.wgpuDeviceCreateShaderModule)(client.device, &WGPUShaderModuleDescriptor {
                nextInChain: &wgsl.chain, label: view("Workstation ruby response shader"),
            });
            if result.shader.is_null() { return Err("optional ruby response shader creation failed".into()); }
            result.compute = (api.wgpuDeviceCreateComputePipeline)(client.device, &WGPUComputePipelineDescriptor {
                label: view("Workstation ruby response bake"), layout: result.pipeline_layout,
                compute: WGPUProgrammableStageDescriptor {
                    module: result.shader, entryPoint: view("cs_ruby_response"), ..Default::default()
                }, ..Default::default()
            });
            if result.compute.is_null() { return Err("optional ruby response compute pipeline failed".into()); }
            result.general_bake = (api.wgpuDeviceCreateComputePipeline)(client.device, &WGPUComputePipelineDescriptor {
                label: view("Workstation ruby controls and rejected panel paths"), layout: result.pipeline_layout,
                compute: WGPUProgrammableStageDescriptor {
                    module: result.shader, entryPoint: view("cs_ruby_response_general"), ..Default::default()
                }, ..Default::default()
            });
            if result.general_bake.is_null() { return Err("optional ruby general bake pipeline failed".into()); }
            result.resolve = (api.wgpuDeviceCreateComputePipeline)(client.device, &WGPUComputePipelineDescriptor {
                label: view("Workstation ruby response lighting"), layout: result.pipeline_layout,
                compute: WGPUProgrammableStageDescriptor {
                    module: result.shader, entryPoint: view("cs_ruby_resolve"), ..Default::default()
                }, ..Default::default()
            });
            if result.resolve.is_null() { return Err("optional ruby response resolve pipeline failed".into()); }
            let blend = WGPUBlendState {
                color: WGPUBlendComponent { operation: WGPUBlendOperation_Add,
                    srcFactor: WGPUBlendFactor_SrcAlpha, dstFactor: WGPUBlendFactor_OneMinusSrcAlpha },
                alpha: WGPUBlendComponent { operation: WGPUBlendOperation_Add,
                    srcFactor: WGPUBlendFactor_One, dstFactor: WGPUBlendFactor_OneMinusSrcAlpha },
            };
            let target = WGPUColorTargetState {
                blend: &blend, format: client.format, writeMask: WGPUColorWriteMask_All, ..Default::default()
            };
            let fragment = WGPUFragmentState {
                module: result.shader, entryPoint: view("fs_main"), targetCount: 1, targets: &target, ..Default::default()
            };
            let depth = WGPUDepthStencilState {
                format: WGPUTextureFormat_Depth32Float, depthWriteEnabled: WGPUOptionalBool_True,
                depthCompare: WGPUCompareFunction_Less, ..Default::default()
            };
            result.render = (api.wgpuDeviceCreateRenderPipeline)(client.device, &WGPURenderPipelineDescriptor {
                label: view("Workstation ruby response playback"), layout: result.pipeline_layout,
                vertex: WGPUVertexState { module: result.shader, entryPoint: view("vs_main"), ..Default::default() },
                primitive: WGPUPrimitiveState { topology: WGPUPrimitiveTopology_TriangleList,
                    frontFace: WGPUFrontFace_CCW, cullMode: WGPUCullMode_None, ..Default::default() },
                multisample: WGPUMultisampleState { count: 1, mask: u32::MAX, ..Default::default() },
                fragment: &fragment, depthStencil: &depth, ..Default::default()
            });
            if result.render.is_null() { return Err("optional ruby response render pipeline failed".into()); }
            Ok(())
        })();
        let scoped = finish_specialization_scopes(api, client.instance, client.device);
        created?;
        scoped?;
        Ok(result)
    }
    unsafe fn upload(&mut self, queue: WGPUQueue, update: &crate::workstation::ruby_response_plan::Update,
                     frame: &[[f32; 4]]) -> Result<(), String> {
        let size = std::mem::size_of_val(update.rows.as_slice());
        if size > crate::workstation::ruby_response_plan::PLAN_ROWS * 16 || size == 0 {
            return Err("ruby response plan exceeds its storage".into());
        }
        (self.api.wgpuQueueWriteBuffer)(queue, self.plan_buffer, 0, update.rows.as_ptr().cast(), size);
        self.entry_count = update.entry_count;
        if self.track_incremental{self.incremental_entries=update.rows[1..].chunks_exact(8).take(update.entry_count as usize).enumerate().filter(|(_,d)|d[0][1]==64.||d[0][1]==128.).map(|(i,d)|[i as u32,d[0][1]as u32,u32::from(d[0][3]==2.),u32::from(d[0][3]>=0.5)]).collect();}
        self.ready_count = update.ready_count;
        self.admitted_pixel_entries = 0;
        self.admitted_timeline_entries = 0;
        self.admitted_pixels = 0;
        self.allocated_pixel_rows = 0;
        // Admission describes CPU plan coverage, not successful GPU cache hits.
        // A bake can still reject individual paths and use canonical tracing.
        for descriptor in update.rows[1..].chunks_exact(8).take(update.entry_count as usize) {
            if descriptor[0][3] < 1. || descriptor[4][3] != 1. { continue; }
            self.admitted_pixel_entries += 1;
            self.admitted_timeline_entries += usize::from((700_000.0..704_096.0).contains(&descriptor[0][0]));
            self.admitted_pixels += descriptor[4][0] as usize * descriptor[4][1] as usize;
            self.allocated_pixel_rows += descriptor[6][0] as usize;
        }
        self.bake_pending = update.dirty && update.entry_count > 0;
        self.resolve_reuse.update(frame, &update.rows, update.entry_count, update.ready_count);
        Ok(())
    }
    unsafe fn encode_bake(&self, encoder: WGPUCommandEncoder, timestamps: Option<&WGPUComputePassTimestampWrites>, general_timestamps: Option<&WGPUComputePassTimestampWrites>) -> Result<bool, String> {
        if !self.bake_pending { return Ok(false); }
        let pass = (self.api.wgpuCommandEncoderBeginComputePass)(encoder, &WGPUComputePassDescriptor {
            label: view("Changed ruby optical response bake"),
            timestampWrites: timestamps.map_or(ptr::null(), |writes| writes), ..Default::default()
        });
        if pass.is_null() { return Err("ruby response compute pass failed".into()); }
        (self.api.wgpuComputePassEncoderSetPipeline)(pass, self.compute);
        (self.api.wgpuComputePassEncoderSetBindGroup)(pass, 0, self.bind_group, 0, ptr::null());
        (self.api.wgpuComputePassEncoderDispatchWorkgroups)(pass, 16, 16, self.entry_count);
        (self.api.wgpuComputePassEncoderEnd)(pass);
        (self.api.wgpuComputePassEncoderRelease)(pass);
        // The panel kernel has no reachable general ray queue. Its rejection
        // flags and speculative rows must be visible before this separate pass
        // replaces rejected samples and bakes dirty controls canonically.
        // Frame timing starts at the first pass and ends after drawing, so this
        // fallback work and the inter-pass dependency remain measured.
        let general = (self.api.wgpuCommandEncoderBeginComputePass)(encoder, &WGPUComputePassDescriptor {
            label: view("Ruby controls and rejected panel response bake"),
            timestampWrites: general_timestamps.map_or(ptr::null(), |writes| writes), ..Default::default()
        });
        if general.is_null() { return Err("ruby general bake pass failed".into()); }
        (self.api.wgpuComputePassEncoderSetPipeline)(general, self.general_bake);
        (self.api.wgpuComputePassEncoderSetBindGroup)(general, 0, self.bind_group, 0, ptr::null());
        (self.api.wgpuComputePassEncoderDispatchWorkgroups)(general, 16, 16, self.entry_count);
        (self.api.wgpuComputePassEncoderEnd)(general);
        (self.api.wgpuComputePassEncoderRelease)(general);
        Ok(true)
    }
    unsafe fn encode_resolve(&self, encoder: WGPUCommandEncoder, timestamps: Option<&WGPUComputePassTimestampWrites>) -> Result<bool, String> {
        if self.ready_count == 0 || !self.resolve_reuse.required(self.bake_pending) { return Ok(false); }
        // Lighting and floor visibility remain live even when object-local
        // glass paths survive translation. A separate pass orders writes after
        // baking and before fragment playback, including first use.
        let pass = (self.api.wgpuCommandEncoderBeginComputePass)(encoder, &WGPUComputePassDescriptor {
            label: view("Ruby response live lighting"),
            timestampWrites: timestamps.map_or(ptr::null(), |writes| writes), ..Default::default()
        });
        if pass.is_null() { return Err("ruby response lighting pass failed".into()); }
        (self.api.wgpuComputePassEncoderSetPipeline)(pass, self.resolve);
        (self.api.wgpuComputePassEncoderSetBindGroup)(pass, 0, self.bind_group, 0, ptr::null());
        (self.api.wgpuComputePassEncoderDispatchWorkgroups)(pass, 16, 16, self.entry_count);
        (self.api.wgpuComputePassEncoderEnd)(pass);
        (self.api.wgpuComputePassEncoderRelease)(pass);
        Ok(true)
    }
}
impl Drop for RubyResponsePipeline {
    fn drop(&mut self) {
        unsafe {
            if !self.bind_group.is_null() { (self.api.wgpuBindGroupRelease)(self.bind_group); }
            if !self.render.is_null() { (self.api.wgpuRenderPipelineRelease)(self.render); }
            if !self.compute.is_null() { (self.api.wgpuComputePipelineRelease)(self.compute); }
            if !self.general_bake.is_null() { (self.api.wgpuComputePipelineRelease)(self.general_bake); }
            if !self.resolve.is_null() { (self.api.wgpuComputePipelineRelease)(self.resolve); }
            if !self.pipeline_layout.is_null() { (self.api.wgpuPipelineLayoutRelease)(self.pipeline_layout); }
            if !self.bind_layout.is_null() { (self.api.wgpuBindGroupLayoutRelease)(self.bind_layout); }
            if !self.shader.is_null() { (self.api.wgpuShaderModuleRelease)(self.shader); }
            for buffer in [self.response_buffer, self.plan_buffer] {
                if !buffer.is_null() { (self.api.wgpuBufferRelease)(buffer); }
            }
        }
    }
}
#[path="WorldBackdropWebGPU.rs"] mod world_backdrop;
#[path="WorkstationWorldImage.rs"] mod world_image;
pub(super) struct Client {
    world: Option<world_backdrop::World>,
    world_active: bool,
    pub window: Window,
    water: Option<super::water::webgpu::Client>,
    water_frames: Vec<super::water::data::DrawFrame>,
    sky_target: Option<cloud_resolution::Sky>,
    scene_colors: [WGPUTexture; 2],
    scene_views: [WGPUTextureView; 2],
    reflection_depth: WGPUTexture,
    reflection_depth_view: WGPUTextureView,
    reflection_buffer: WGPUBuffer,
    reflection_group: WGPUBindGroup,
    reflection_count: u32,
    reflection_vegetation: Option<super::vegetation::webgpu::Client>,
    reflection_plants_active: bool,
    vegetation: Option<super::vegetation::webgpu::Client>,
    vegetation_active: bool,
    api: &'static Api,
    instance: WGPUInstance,
    surface: WGPUSurface,
    diagnostic_offscreen:bool,
    frozen_mode:u8, frozen_diagnostic:Option<frozen::Control>,
    incremental_mode:bool,
    refresh_validation:Option<refresh_validation::Validation>,surface_resizes:u64,
    workstation_uploaded:Vec<[f32;4]>,workstation_upload_bytes:u64,workstation_upload_calls:u64,
    final_mode:bool, final_retention:Option<final_retention::Retention>, final_probe:bool,
    motion_comparison:bool,motion_bank:Option<motion_bank::Bank>,
    diagnostic_target:WGPUTexture,
    hidden_scene:bool,
    background_target:WGPUTexture,
    background_size:[u32;2],
    surface_acquisitions:u64,
    adapter: WGPUAdapter,
    device: WGPUDevice,
    queue: WGPUQueue,
    shader: WGPUShaderModule,
    pipeline: WGPURenderPipeline,
    controls_pipeline: Option<ControlsPipeline>,
    opaque_pipeline: Option<ControlsPipeline>,
    opaque_vertices: u32,
    ruby_response: Option<RubyResponsePipeline>,
    baked: Option<baked::Pipeline>,
    theme_root:std::path::PathBuf,
    theme_source:Option<String>,
    theme_generation:u64,
    prerender: Option<prerender::Pipeline>,
    deferred_floor: crate::workstation::deferred_floor::Resolver,
    frame_overlap:bool,
    game_fifo:bool,
    frame_in_flight:Cell<bool>,
    acquire_before_wait:bool,
    frame_submissions:u64,frame_completions:Cell<u64>,present_calls:u64,successful_presents:u64,
    input_observed:Option<Instant>,previous_input_observed:Option<Instant>,input_observations:u64,
    overlap_submissions:u64,
    prior_frame_waits:u64,
    ruby_plan: Option<crate::workstation::ruby_response_plan::Plan>,
    ruby_response_error: Option<String>,
    controls_frame: bool,
    feature_counts: Option<[f32; 4]>,
    specialization_error: Option<String>,
    vertex_buffer: WGPUBuffer,
    vertices: Vec<f32>,
    camera_vertices:u32,
    fullscreen_tail:bool,meter_split_draws:u64,
    ledger_camera_rgb:bool,ledger_camera_rgb_updates:u64,
    texture_buffer: WGPUBuffer,
    texture_capacity: usize,
    bind_groups: Vec<WGPUBindGroup>,
    depth: WGPUTexture,
    depth_view: WGPUTextureView,
    read_buffer: WGPUBuffer,
    width: u32,
    height: u32,
    stride: u32,
    format: WGPUTextureFormat,
    pub(super) captured: bool,
    pub(super) monitor_capture: Option<Vec<u8>>,
    workstation: bool,
    gpu_timing: Option<GpuTiming>,
    frame_attribution: bool,
    pub(super) validate_triangle_center: bool,
    count: u32,
    pub(super) clear: [f32; 4],
    capture: PathBuf,
    // Keep Objective-C runtime and QuartzCore loaded while the surface lives.
    _native_libraries: Vec<libloading::Library>,
    pub info: Value,
}
impl Client {
    pub unsafe fn new(
        root: &Path,
        data: &[f32; 24],
        output: &Path,
        workstation: bool,
        prewarm: bool,
    ) -> Result<Self, String> {
        if !cfg!(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "ios"
        )) {
            return Err("WebGPU native window integration requires macOS, Windows, or iOS".into());
        }
        check_errors()?;
        let frame_overlap=workstation&&crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_FRAME_OVERLAP");
        let diagnostic_offscreen=diagnostic_offscreen_requested(workstation,
            std::env::var("WORMWOOD_WORKSTATION_DIAGNOSTIC_OFFSCREEN").ok().as_deref(),frame_overlap)?;
        let frozen_mode=frozen::requested(std::env::var("WORMWOOD_WORKSTATION_DIAGNOSTIC_FROZEN").ok().as_deref(),diagnostic_offscreen)?;
        let incremental_mode=workstation&&crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_INCREMENTAL_BAKE");
        let final_mode=workstation&&std::env::var("WORMWOOD_WORKSTATION_FINAL_RETENTION").as_deref()==Ok("1");
        if final_mode&&frozen_mode!=0{return Err("Final retention and frozen diagnostic are mutually exclusive".into());}
        let final_probe=std::env::var("WORMWOOD_WORKSTATION_FINAL_PROBE").as_deref()==Ok("1");
        if final_probe&&!final_mode{return Err("Final probe requires explicit final retention mode".into());}
        let window = {
            #[cfg(target_os="macos")]
            {if prewarm {Window::prewarm(c"Mirror — Audio workstation")?}else{Window::new(false,c"Wormwood — WebGPU",workstation)?}}
            #[cfg(not(target_os="macos"))]
            {let _=prewarm;Window::new(false,c"Wormwood — WebGPU",workstation)?}
        };
        let (w, h) = window.dimensions()?;
        let api = api()?;
        let mut c = Self {
            window,
            world:None,world_active:false,
            water: None,
            water_frames: Vec::new(),
            scene_colors: [ptr::null_mut(); 2],
            sky_target: None,
            scene_views: [ptr::null_mut(); 2],
            reflection_depth: ptr::null_mut(),
            reflection_depth_view: ptr::null_mut(),
            reflection_buffer: ptr::null_mut(),
            reflection_group: ptr::null_mut(),
            reflection_count: 0,
            reflection_vegetation: None,
            reflection_plants_active: false,
            vegetation: None,
            vegetation_active: false,
            api,
            instance: ptr::null_mut(),
            surface: ptr::null_mut(),
            diagnostic_offscreen,diagnostic_target:ptr::null_mut(),hidden_scene:false,background_target:ptr::null_mut(),background_size:[0;2],surface_acquisitions:0,
            frozen_mode,frozen_diagnostic:None,
            workstation_uploaded:vec![],workstation_upload_bytes:0,workstation_upload_calls:0,incremental_mode,refresh_validation:refresh_validation::Validation::requested()?,surface_resizes:0,final_mode,final_retention:None,final_probe,motion_comparison:std::env::var("WORMWOOD_WORKSTATION_MOTION_COMPARE").is_ok()||crate::workstation::benchmark::motion_input::enabled(),motion_bank:None,
            adapter: ptr::null_mut(),
            device: ptr::null_mut(),
            queue: ptr::null_mut(),
            shader: ptr::null_mut(),
            pipeline: ptr::null_mut(),
            controls_pipeline: None,
            opaque_pipeline: None,
            opaque_vertices: 0,
            ruby_response: None,
            baked: None,
            theme_root:root.to_path_buf(),theme_source:None,theme_generation:crate::workstation::cardinal_theme::generation(),
            prerender: None,
            deferred_floor: Default::default(),
            frame_overlap,
            game_fifo:!workstation&&std::env::var("WORMWOOD_GAME_FIFO_OVERLAP").as_deref()!=Ok("0"),
            frame_in_flight:Cell::new(false),overlap_submissions:0,prior_frame_waits:0,
            acquire_before_wait:workstation&&std::env::var("WORMWOOD_WORKSTATION_ACQUIRE_FIRST").as_deref()==Ok("1"),
            frame_submissions:0,frame_completions:Cell::new(0),present_calls:0,successful_presents:0,
            input_observed:None,previous_input_observed:None,input_observations:0,
            ruby_plan: None,
            ruby_response_error: None,
            controls_frame: false,
            feature_counts: None,
            specialization_error: None,
            vertex_buffer: ptr::null_mut(),
            vertices: Vec::new(),
            camera_vertices:0,
            fullscreen_tail:false,meter_split_draws:0,
            ledger_camera_rgb:crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_LEDGER_CAMERA_RGB"),ledger_camera_rgb_updates:0,
            texture_buffer: ptr::null_mut(),
            texture_capacity: if workstation {
                crate::scene_assets::SCENE_BYTES.max(super::workstation::STORAGE_BYTES + crate::scene_assets::TABLE.len() + crate::scene_assets::FINISHES.len())
            } else {
                crate::scene_assets::SCENE_BYTES
            },
            bind_groups: vec![],
            depth: ptr::null_mut(),
            depth_view: ptr::null_mut(),
            read_buffer: ptr::null_mut(),
            width: w,
            height: h,
            stride: (w * 4).div_ceil(256) * 256,
            format: 0,
            captured: false,
            monitor_capture: None,
            workstation,
            gpu_timing: None,
            frame_attribution: workstation && std::env::var("WORMWOOD_WORKSTATION_FRAME_ATTRIBUTION").as_deref() == Ok("1"),
            validate_triangle_center: true,
            count: 3,
            clear: [0.035, 0.045, 0.065, 1.],
            capture: output.join("triangle-WebGPU.ppm"),
            _native_libraries: vec![],
            info: Value::Null,
        };
        c.instance = (api.wgpuCreateInstance)(ptr::null());
        if c.instance.is_null() {
            return Err("WebGPU instance creation failed".into());
        }
        #[cfg(target_os = "macos")]
        {
            let objc =
                libloading::Library::new("/usr/lib/libobjc.A.dylib").map_err(|e| e.to_string())?;
            let quartz = libloading::Library::new(
                "/System/Library/Frameworks/QuartzCore.framework/QuartzCore",
            )
            .map_err(|e| e.to_string())?;
            let class = *objc
                .get::<unsafe extern "C" fn(*const i8) -> *mut c_void>(b"objc_getClass\0")
                .map_err(|e| e.to_string())?;
            let selector = *objc
                .get::<unsafe extern "C" fn(*const i8) -> *mut c_void>(b"sel_registerName\0")
                .map_err(|e| e.to_string())?;
            let object = *objc
                .get::<unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void>(
                    b"objc_msgSend\0",
                )
                .map_err(|e| e.to_string())?;
            let set_bool = *objc
                .get::<unsafe extern "C" fn(*mut c_void, *mut c_void, bool)>(b"objc_msgSend\0")
                .map_err(|e| e.to_string())?;
            let set_object = *objc
                .get::<unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void)>(
                    b"objc_msgSend\0",
                )
                .map_err(|e| e.to_string())?;
            let cocoa = c
                .window
                .symbol::<unsafe extern "C" fn(*mut c_void) -> *mut c_void>(
                    b"glfwGetCocoaWindow\0",
                )?(c.window.handle);
            let content = object(cocoa, selector(c"contentView".as_ptr()));
            let layer = object(class(c"CAMetalLayer".as_ptr()), selector(c"layer".as_ptr()));
            if cocoa.is_null() || content.is_null() || layer.is_null() {
                return Err("WebGPU Metal layer creation failed".into());
            }
            set_bool(content, selector(c"setWantsLayer:".as_ptr()), true);
            set_object(content, selector(c"setLayer:".as_ptr()), layer);
            c._native_libraries.push(objc);
            c._native_libraries.push(quartz);
            let metal = WGPUSurfaceSourceMetalLayer {
                chain: WGPUChainedStruct {
                    next: ptr::null(),
                    sType: WGPUSType_SurfaceSourceMetalLayer,
                },
                layer,
            };
            let descriptor = WGPUSurfaceDescriptor {
                nextInChain: &metal.chain,
                label: view("Wormwood surface"),
            };
            c.surface = (api.wgpuInstanceCreateSurface)(c.instance, &descriptor);
        }
        #[cfg(target_os = "ios")]
        {
            let metal = WGPUSurfaceSourceMetalLayer {
                chain: WGPUChainedStruct {
                    next: ptr::null(),
                    sType: WGPUSType_SurfaceSourceMetalLayer,
                },
                layer: c.window.metal_layer(),
            };
            let descriptor = WGPUSurfaceDescriptor {
                nextInChain: &metal.chain,
                label: view("Wormwood iOS surface"),
            };
            c.surface = (api.wgpuInstanceCreateSurface)(c.instance, &descriptor);
        }
        #[cfg(target_os = "windows")]
        {
            let hwnd = c
                .window
                .symbol::<unsafe extern "C" fn(*mut c_void) -> *mut c_void>(
                    b"glfwGetWin32Window\0",
                )?(c.window.handle);
            let kernel = libloading::Library::new("kernel32.dll").map_err(|e| e.to_string())?;
            let module = *kernel
                .get::<unsafe extern "system" fn(*const u16) -> *mut c_void>(b"GetModuleHandleW\0")
                .map_err(|e| e.to_string())?;
            let hinstance = module(ptr::null());
            if hwnd.is_null() || hinstance.is_null() {
                return Err("WebGPU Win32 window/module handle unavailable".into());
            }
            let source = WGPUSurfaceSourceWindowsHWND {
                chain: WGPUChainedStruct {
                    next: ptr::null(),
                    sType: WGPUSType_SurfaceSourceWindowsHWND,
                },
                hinstance,
                hwnd,
            };
            let descriptor = WGPUSurfaceDescriptor {
                nextInChain: &source.chain,
                label: view("Wormwood surface"),
            };
            c.surface = (api.wgpuInstanceCreateSurface)(c.instance, &descriptor);
        }
        if c.surface.is_null() {
            return Err("WebGPU surface creation failed".into());
        }
        let mut state = Box::new(Wait::default());
        let callback = WGPURequestAdapterCallbackInfo {
            mode: WGPUCallbackMode_AllowProcessEvents,
            callback: Some(adapter_ready),
            userdata1: (&mut *state as *mut Wait).cast(),
            ..Default::default()
        };
        let options = WGPURequestAdapterOptions {
            featureLevel: WGPUFeatureLevel_Core,
            backendType: if cfg!(target_os = "windows") {
                WGPUBackendType_D3D12
            } else {
                WGPUBackendType_Metal
            },
            compatibleSurface: c.surface,
            ..Default::default()
        };
        (api.wgpuInstanceRequestAdapter)(c.instance, &options, callback);
        c.adapter = wait(api, c.instance, ptr::null_mut(), state)?.cast();
        if c.adapter.is_null() {
            return Err("WebGPU returned a null adapter".into());
        }
        let legacy_timing =
            workstation && std::env::var("WORMWOOD_WORKSTATION_GPU_TIMING").as_deref() == Ok("1");
        let performance = super::performance::enabled();
        let attribution_requested=workstation&&std::env::var("WORMWOOD_WORKSTATION_GPU_ATTRIBUTION").as_deref()==Ok("1");
        let timing_requested = legacy_timing || performance || attribution_requested;
        let timestamp_feature = WGPUFeatureName_TimestampQuery;
        let timing_supported =
            timing_requested && (api.wgpuAdapterHasFeature)(c.adapter, timestamp_feature) != 0;
        // Use the render pass's explicit begin/end writes in both profiling
        // modes. Standalone encoder timestamps on the measured Metal backend
        // returned sub-millisecond intervals around a roughly 340 ms pass.
        // The pass writes bracket that work reliably; claiming an all-pass
        // interval from the encoder extension would misreport GPU execution.
        let inside_feature=WGPUNativeFeature_TimestampQueryInsidePasses as WGPUFeatureName;
        let (inside_pass_writer,inside_pass_reason)=if !attribution_requested {
            (None,Some("Diagnostic not requested".to_string()))
        }else if !timing_supported {
            (None,Some("Adapter lacks TimestampQuery".to_string()))
        }else if (api.wgpuAdapterHasFeature)(c.adapter,inside_feature)==0 {
            (None,Some("Adapter lacks TimestampQueryInsidePasses; capture pass timing remains enabled".to_string()))
        }else{match InsidePassWriter::load(){Ok(writer)=>(Some(writer),None),Err(e)=>(None,Some(format!("Optional inside-pass timestamp symbol unavailable: {e}")))}};
        let mut timing_features = Vec::new();
        if timing_supported {
            timing_features.push(timestamp_feature);
        }
        if inside_pass_writer.is_some(){timing_features.push(inside_feature);}
        let mut state = Box::new(Wait::default());
        let callback = WGPURequestDeviceCallbackInfo {
            mode: WGPUCallbackMode_AllowProcessEvents,
            callback: Some(device_ready),
            userdata1: (&mut *state as *mut Wait).cast(),
            ..Default::default()
        };
        let device = WGPUDeviceDescriptor {
            label: view("Wormwood device"),
            requiredFeatureCount: timing_features.len(),
            requiredFeatures: if !timing_features.is_empty() {
                timing_features.as_ptr()
            } else {
                ptr::null()
            },
            deviceLostCallbackInfo: WGPUDeviceLostCallbackInfo {
                mode: WGPUCallbackMode_AllowProcessEvents,
                ..Default::default()
            },
            uncapturedErrorCallbackInfo: WGPUUncapturedErrorCallbackInfo {
                callback: Some(error),
                ..Default::default()
            },
            ..Default::default()
        };
        (api.wgpuAdapterRequestDevice)(c.adapter, &device, callback);
        c.device = wait(api, c.instance, ptr::null_mut(), state)?.cast();
        if c.device.is_null() {
            return Err("WebGPU returned a null device".into());
        }
        c.queue = (api.wgpuDeviceGetQueue)(c.device);
        if c.queue.is_null() {
            return Err("WebGPU returned a null queue".into());
        }
        if timing_requested {
            c.gpu_timing = Some(GpuTiming::new(
                api,
                c.device,
                c.queue,
                output,
                [w, h],
                workstation,
                legacy_timing,
                timing_supported,
                attribution_requested,inside_pass_writer,inside_pass_reason,
            )?);
        }
        let mut caps = WGPUSurfaceCapabilities::default();
        if (api.wgpuSurfaceGetCapabilities)(c.surface, c.adapter, &mut caps) != WGPUStatus_Success {
            return Err("WebGPU surface capabilities failed".into());
        }
        if caps.formatCount == 0 || caps.formats.is_null() {
            (api.wgpuSurfaceCapabilitiesFreeMembers)(caps);
            return Err("WebGPU surface has no formats".into());
        }
        let formats = std::slice::from_raw_parts(caps.formats, caps.formatCount);
        let format = formats.iter().copied().find(|f| {
            matches!(
                *f,
                WGPUTextureFormat_BGRA8Unorm | WGPUTextureFormat_RGBA8Unorm
            )
        });
        let present_modes=if caps.presentModes.is_null(){vec![]}else{std::slice::from_raw_parts(caps.presentModes,caps.presentModeCount).to_vec()};
        let usages = caps.usages;
        (api.wgpuSurfaceCapabilitiesFreeMembers)(caps);
        c.format = format.ok_or("WebGPU surface has no supported 8-bit format")?;
        let usage = WGPUTextureUsage_RenderAttachment | WGPUTextureUsage_CopySrc;
        if usages & usage != usage {
            return Err("WebGPU surface does not support diagnostic readback".into());
        }
        let requested_latency=if !workstation&&std::env::var("WORMWOOD_GAME_FIFO_OVERLAP").as_deref()!=Ok("0"){Some(2)}else if workstation {requested_surface_latency(std::env::var("WORMWOOD_WORKSTATION_SURFACE_LATENCY").ok().as_deref())?}else{None};
        // Bindgen uses the installed wgpu.h ABI. Keep the chained descriptor
        // alive through the synchronous configure call; default passes no extras.
        let surface_extras=requested_latency.map(|latency|WGPUSurfaceConfigurationExtras{
            chain:WGPUChainedStruct{next:ptr::null(),sType:WGPUSType_SurfaceConfigurationExtras as WGPUSType},
            desiredMaximumFrameLatency:latency});
        let config = WGPUSurfaceConfiguration {
            nextInChain:surface_extras.as_ref().map_or(ptr::null(),|extras|&extras.chain),
            device: c.device,
            format: c.format,
            usage,
            width: w,
            height: h,
            alphaMode: WGPUCompositeAlphaMode_Auto,
            presentMode: WGPUPresentMode_Fifo,
            ..Default::default()
        };
        (api.wgpuSurfaceConfigure)(c.surface, &config);
        check_errors()?;
        #[cfg(target_os="macos")]
        let native_presentation=metal_presentation_observation(&c.window)?;
        #[cfg(not(target_os="macos"))]
        let native_presentation=Value::Null;
        let native_version=(api.wgpuGetVersion)();
        if diagnostic_offscreen {
            // Exactly the selected surface format, native dimensions, usage and
            // single output sample. The fragment shader still evaluates its two
            // original radiance samples and original display transform.
            c.diagnostic_target=(api.wgpuDeviceCreateTexture)(c.device,&WGPUTextureDescriptor{
                label:view("Diagnostic original full-scene offscreen target"),
                usage,dimension:WGPUTextureDimension_2D,
                size:WGPUExtent3D{width:w,height:h,depthOrArrayLayers:1},
                format:c.format,mipLevelCount:1,sampleCount:1,..Default::default()
            });
            if c.diagnostic_target.is_null(){return Err("Offscreen diagnostic target allocation failed".into());}
            check_errors()?;
        }
        let mut source = crate::workstation::style_shader::source(root, if workstation {
            "Procedures/Shaders/workstation.wgsl"
        } else {
            "Procedures/Shaders/triangle.wgsl"
        })?;
        if workstation&&crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_PLAIN_BACKDROP"){source=backdrop::source(&source)?;}
        let wgsl = WGPUShaderSourceWGSL {
            chain: WGPUChainedStruct {
                next: ptr::null(),
                sType: WGPUSType_ShaderSourceWGSL,
            },
            code: view(&source),
        };
        let shader = WGPUShaderModuleDescriptor {
            nextInChain: &wgsl.chain,
            label: view("Wormwood vertex color"),
        };
        c.shader = (api.wgpuDeviceCreateShaderModule)(c.device, &shader);
        if c.shader.is_null() {
            return Err("WebGPU shader creation failed".into());
        }
        let blend = WGPUBlendState {
            color: WGPUBlendComponent {
                operation: WGPUBlendOperation_Add,
                srcFactor: WGPUBlendFactor_SrcAlpha,
                dstFactor: WGPUBlendFactor_OneMinusSrcAlpha,
            },
            alpha: WGPUBlendComponent {
                operation: WGPUBlendOperation_Add,
                srcFactor: WGPUBlendFactor_One,
                dstFactor: WGPUBlendFactor_OneMinusSrcAlpha,
            },
        };
        let target = WGPUColorTargetState {
            blend: &blend,
            format: c.format,
            writeMask: WGPUColorWriteMask_All,
            ..Default::default()
        };
        let fragment = WGPUFragmentState {
            module: c.shader,
            entryPoint: view("fs_main"),
            targetCount: 1,
            targets: &target,
            ..Default::default()
        };
        let depth_stencil = WGPUDepthStencilState {
            format: WGPUTextureFormat_Depth32Float,
            depthWriteEnabled: WGPUOptionalBool_True,
            depthCompare: WGPUCompareFunction_Less,
            ..Default::default()
        };
        let pipeline = WGPURenderPipelineDescriptor {
            label: view("Wormwood triangle"),
            vertex: WGPUVertexState {
                module: c.shader,
                entryPoint: view("vs_main"),
                ..Default::default()
            },
            primitive: WGPUPrimitiveState {
                topology: WGPUPrimitiveTopology_TriangleList,
                frontFace: WGPUFrontFace_CCW,
                cullMode: WGPUCullMode_None,
                ..Default::default()
            },
            multisample: WGPUMultisampleState {
                count: 1,
                mask: u32::MAX,
                ..Default::default()
            },
            fragment: &fragment,
            depthStencil: &depth_stencil,
            ..Default::default()
        };
        c.pipeline = (api.wgpuDeviceCreateRenderPipeline)(c.device, &pipeline);
        if c.pipeline.is_null() {
            return Err("WebGPU pipeline creation failed".into());
        }
        let uniform = WGPUBufferDescriptor {
            label: view("Interleaved scene vertices"),
            size: crate::scene_limits::SCENE_BUFFER_BYTES,
            usage: WGPUBufferUsage_Storage | WGPUBufferUsage_CopyDst | if frozen_mode!=0||incremental_mode{WGPUBufferUsage_CopySrc}else{0},
            ..Default::default()
        };
        c.vertex_buffer = (api.wgpuDeviceCreateBuffer)(c.device, &uniform);
        if c.vertex_buffer.is_null() {
            return Err("WebGPU scene vertex allocation failed".into());
        }
        c.set_batch(std::slice::from_ref(data))?;
        let texture_desc = WGPUBufferDescriptor {
            size: c.texture_capacity as u64,
            usage: WGPUBufferUsage_Storage | WGPUBufferUsage_CopyDst | if frozen_mode!=0||incremental_mode{WGPUBufferUsage_CopySrc}else{0},
            ..Default::default()
        };
        c.texture_buffer = (api.wgpuDeviceCreateBuffer)(c.device, &texture_desc);
        if c.texture_buffer.is_null() {
            return Err("player texture storage allocation failed".into());
        }
        if workstation {
            let table=crate::scene_assets::TABLE;
            (api.wgpuQueueWriteBuffer)(c.queue,c.texture_buffer,(c.texture_capacity-crate::scene_assets::FINISHES.len()-table.len()) as u64,table.as_ptr().cast(),table.len());
            let finishes = crate::scene_assets::FINISHES;
            (api.wgpuQueueWriteBuffer)(c.queue,c.texture_buffer,(c.texture_capacity-finishes.len()) as u64,
                finishes.as_ptr().cast(), finishes.len());
            let initial = super::workstation::initial(root)?;
            if !baked::enabled(){c.ruby_plan = Some(crate::workstation::ruby_response_plan::Plan::new(&initial));}
            c.set_workstation(&initial)?;
        } else {
            c.texture(&[1, 0, 0, 0, 1, 0, 0, 0, 255, 255, 255, 255])?;
        }
        let layout = (api.wgpuRenderPipelineGetBindGroupLayout)(c.pipeline, 0);
        if layout.is_null() {
            return Err("WebGPU bind-group layout failed".into());
        }
        for _ in 0..1 {
            let entry = WGPUBindGroupEntry {
                binding: 0,
                buffer: c.vertex_buffer,
                offset: 0,
                size: crate::scene_limits::SCENE_BUFFER_BYTES,
                ..Default::default()
            };
            let entries = [
                entry,
                WGPUBindGroupEntry {
                    binding: 1,
                    buffer: c.texture_buffer,
                    size: c.texture_capacity as u64,
                    ..Default::default()
                },
            ];
            let group = WGPUBindGroupDescriptor {
                layout,
                entryCount: 2,
                entries: entries.as_ptr(),
                ..Default::default()
            };
            let bind = (api.wgpuDeviceCreateBindGroup)(c.device, &group);
            if bind.is_null() {
                return Err("WebGPU scene bind group failed".into());
            }
            c.bind_groups.push(bind);
        }
        (api.wgpuBindGroupLayoutRelease)(layout);
        let depth = WGPUTextureDescriptor {
            dimension: WGPUTextureDimension_2D,
            size: WGPUExtent3D {
                width: w,
                height: h,
                depthOrArrayLayers: 1,
            },
            format: WGPUTextureFormat_Depth32Float,
            mipLevelCount: 1,
            sampleCount: 1,
            usage: WGPUTextureUsage_RenderAttachment | WGPUTextureUsage_TextureBinding | WGPUTextureUsage_CopySrc | WGPUTextureUsage_CopyDst,
            ..Default::default()
        };
        c.depth = (api.wgpuDeviceCreateTexture)(c.device, &depth);
        if c.depth.is_null() {
            return Err("WebGPU depth texture failed".into());
        }
        c.depth_view = (api.wgpuTextureCreateView)(c.depth, ptr::null());
        if c.depth_view.is_null() {
            return Err("WebGPU depth view failed".into());
        }
        let readback = WGPUBufferDescriptor {
            label: view("Triangle readback"),
            size: c.stride as u64 * h as u64,
            usage: WGPUBufferUsage_MapRead | WGPUBufferUsage_CopyDst,
            ..Default::default()
        };
        c.read_buffer = (api.wgpuDeviceCreateBuffer)(c.device, &readback);
        if c.read_buffer.is_null() {
            return Err("WebGPU readback buffer failed".into());
        }
        let mut info = WGPUAdapterInfo::default();
        if (api.wgpuAdapterGetInfo)(c.adapter, &mut info) != WGPUStatus_Success {
            return Err("WebGPU adapter information failed".into());
        }
        c.info = json!({"api":"WebGPU","implementation":"wgpu-native","native_backend":info.backendType,"device":string(info.device),"description":string(info.description),"width":w,"height":h,"readback_verified":false,
            "diagnostic_render_target":{"offscreen":diagnostic_offscreen,"format":c.format,"width":w,"height":h,
                "output_sample_count":1,"original_radiance_samples":2,"texture_usage":usage,
                "scope":"Offscreen is an explicit serial completion diagnostic, not displayed application FPS. Same renderer, CPU loop, attachments and capture path; surface acquisition/presentation skipped only when selected."},
            "presentation":{"requested_maximum_frame_latency":requested_latency,"extension_supplied":surface_extras.is_some(),
                "native_version_raw":native_version,"native_configuration":native_presentation,"selected":"FIFO","selected_code":WGPUPresentMode_Fifo,"supported_codes":present_modes,"physical_scanout_hz":Value::Null,"scope":"Existing FIFO retained. Surface acquisition wall time is measured separately from main-thread pacing; present success is not a scanout timestamp."}});
        (api.wgpuAdapterInfoFreeMembers)(info);
        c.wait()?;
        if workstation && baked::enabled() {
            c.baked=Some(baked::Pipeline::new(&c,root,&source)?);
            c.theme_source=Some(source.clone());c.theme_generation=crate::workstation::cardinal_theme::generation();
            c.info["workstation_shader"]=c.workstation_shader_info();
        }
        if workstation && !baked::enabled() {
            if std::env::var("WORMWOOD_RUBY_RESPONSE").as_deref() == Ok("0") {
                c.ruby_response_error = Some("ruby response disabled by WORMWOOD_RUBY_RESPONSE=0".into());
            } else {
                match RubyResponsePipeline::new(&c, &source) {
                    Ok(pipeline) => c.ruby_response = Some(pipeline),
                    Err(error) => c.ruby_response_error = Some(error),
                }
            }
            if crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_PRERENDER") {
                let response=c.ruby_response.as_ref().ok_or("prerender requires original ruby response pipeline")?;
                let mut limits=WGPULimits::default();if (api.wgpuDeviceGetLimits)(c.device,&mut limits)!=WGPUStatus_Success{return Err("cannot query retained atlas device limits".into());}
                c.prerender=Some(prerender::Pipeline::new(&c,&source,response,limits.maxTextureDimension2D)?);
                if std::env::var("WORMWOOD_WORKSTATION_MOTION_COMPARE").as_deref()==Ok("bank")||crate::workstation::benchmark::motion_input::bank_enabled(){
                    if frozen_mode==0&&!final_mode{c.motion_bank=Some(motion_bank::Bank::new(&c)?);}
                }
                if frozen_mode!=0{c.frozen_diagnostic=Some(frozen::Control::new(api,frozen_mode));}
                if final_mode{c.final_retention=Some(final_retention::Retention::new(&c,std::env::var("WORMWOOD_WORKSTATION_FINAL_VALIDATE").as_deref()==Ok("1"))?);}
            }
            if c.ruby_response.is_none() {
                match ControlsPipeline::new(&c, &source) {
                    Ok(pipeline) => c.controls_pipeline = Some(pipeline),
                    Err(error) => c.specialization_error = Some(error),
                }
            }
            if frozen_mode==0&&!final_mode&&c.motion_bank.is_none()
                &&crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_GEM_COMPOSITION") {
                c.final_retention=Some(final_retention::Retention::composition(&c,
                    std::env::var("WORMWOOD_WORKSTATION_COMPOSITION_VALIDATE").as_deref()==Ok("1"))?);
            }
            c.info["workstation_shader"] = c.workstation_shader_info();
        }
        if !workstation && std::env::var("WORMWOOD_SCENE_OPAQUE_PIPELINE").as_deref()!=Ok("0") {
            c.opaque_pipeline=Some(ControlsPipeline::from_source(&c,&source,false)?);


        }
        Ok(c)
    }
    fn workstation_shader_info(&self) -> Value {
        let ruby = self.ruby_response.as_ref();
        let ruby_selected = ruby.is_some_and(|response| response.ready_count > 0);
        let mut info = json!({"specialized_available":self.controls_pipeline.is_some(),
            "presentation":self.info["presentation"],
            "selected":if ruby_selected{"ruby_response"}else if self.controls_frame && self.controls_pipeline.is_some(){"controls_only"}else{"full"},
            "error":self.specialization_error,"feature_counts":self.feature_counts,
            "feature_order":["bubbles","spheres","custom_panels","timeline_meshes"],
            "ruby_response":{"available":ruby.is_some(),"error":self.ruby_response_error,
                "exterior_policy":"studio_environment",
                "cache_entries":ruby.map_or(0,|response| response.entry_count),
                "ready_entries":ruby.map_or(0,|response| response.ready_count),
                "bake_pending":ruby.is_some_and(|response| response.bake_pending),
                "bake_submissions":ruby.map_or(0,|response| response.bake_submissions),
                "bake_kernel":"guarded_convex_panels",
                "pixel_transport":ruby.is_some() && std::env::var("WORMWOOD_WORKSTATION_PIXEL_TRANSPORT").as_deref()==Ok("1"),
                "pixel_transport_engraving":"all_canonical_interior_segments",
                "admitted_pixel_entries":ruby.map_or(0,|response| response.admitted_pixel_entries),
                "admitted_timeline_entries":ruby.map_or(0,|response| response.admitted_timeline_entries),
                "admitted_pixels":ruby.map_or(0,|response| response.admitted_pixels),
                "allocated_pixel_rows":ruby.map_or(0,|response| response.allocated_pixel_rows),
                "pixel_admission_scope":"CPU plan coverage; per-path GPU fallback remains possible",
                "general_bake_submissions":ruby.map_or(0,|response| response.bake_submissions),
                "resolve_submissions":ruby.map_or(0,|response| response.resolve_submissions),
                "resolve_reuse_enabled":ruby.is_some_and(|response| response.resolve_reuse.enabled()),
                "engraving":"full_resolution_first_refracted_segment",
                "cache_bytes":crate::workstation::ruby_response_plan::CACHE_BYTES,
                "timing_includes_bake":true,"timing_includes_general_bake":true,"timing_includes_resolve":true}});
        info["changed_uploads"]=json!({"enabled":crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_CHANGED_UPLOADS"),"bytes":self.workstation_upload_bytes,"calls":self.workstation_upload_calls,"block_rows":128,"scope":"Exact bitwise changes in packed optical ranges; immutable asset writes invalidate the shadow."});
        info["frame_overlap"]=self.frame_overlap_info();
        info["meter_partition"]=json!({"draws":self.meter_split_draws,"fullscreen_tail_valid":self.fullscreen_tail,"camera_prefix_vertices":self.camera_vertices,"ledger_camera_rgb_enabled":self.ledger_camera_rgb,"ledger_camera_rgb_updates":self.ledger_camera_rgb_updates});
        if let Some(control)=&self.frozen_diagnostic{info["frozen_diagnostic"]=control.info();}
        if let Some(retained)=&self.final_retention{info["final_retention"]=retained.info();}
        if let Some(bank)=&self.motion_bank{info["motion_bank"]=bank.info();}
        if let Some(p)=&self.baked {info["selected"]=json!("baked_images");info["baked_images"]=p.info();}
        if let Some(image)=self.world.as_ref().and_then(|w|w.image.as_ref()){info["paused_world_image"]=image.info();}
        if let Some(world)=&self.world{info["audio_ray_overlay_triangles"]=json!(world.overlay_count/3);}
        if let Some(p)=&self.prerender {info["prerender"]=p.info();}
        info["plain_backdrop"]=json!({"enabled":crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_PLAIN_BACKDROP"),"scope":"Camera-visible floor shading removed; retained background is live ambient color, not baked. Secondary gem floor illumination and its necessary refresh remain original."});
        info["deferred_floor"]=json!({"skipped_inputs":self.deferred_floor.skipped,"resolutions":self.deferred_floor.resolutions,"cpu_ms":self.deferred_floor.cpu_ms});
        if let Some(causes) = self.ruby_plan.as_ref().and_then(|plan| plan.cache_causes()) {
            info["ruby_response"]["cache_causes"] = causes.clone();
        }
        info
    }
    fn write_workstation_shader_report(&self, final_state: bool) -> Result<(), String> {
        let name = if final_state { "workstation-shader-WebGPU.json" }
            else { "workstation-shader-capture-WebGPU.json" };
        let path = self.capture.with_file_name(name);
        let mut report = self.workstation_shader_info();
        report["snapshot"] = json!(if final_state { "session_end" } else { "image_capture" });
        std::fs::write(&path, serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?)
            .map_err(|e| format!("{}: {e}", path.display()))
    }
    pub unsafe fn set_workstation(&mut self, rows: &[[f32; 4]]) -> Result<(), String> {
        // Immutable LUT/mesh uploads must reach the GPU before the diagnostic
        // intercepts dynamic frames; otherwise captures contain only black.
        if rows.len()==super::workstation::FRAME_ROWS {
            if let Some(v)=&mut self.refresh_validation{v.raw=rows.to_vec();return Ok(());}
        }
        self.set_workstation_inner(rows)
    }
    unsafe fn set_workstation_inner(&mut self, rows: &[[f32; 4]]) -> Result<(), String> {
        let generation=crate::workstation::cardinal_theme::generation();
        if self.baked.is_some()&&generation!=self.theme_generation {
            // Explicit Settings Apply replaces only render resources. Audio,
            // documents, selection, transport and input geometry are untouched.
            let replacement=baked::Pipeline::new(self,&self.theme_root,self.theme_source.as_deref().ok_or("missing theme shader source")?)?;
            self.wait()?;
            self.baked=Some(replacement);self.theme_generation=generation;
        }
        let probed=self.final_probe.then(||final_retention::probe_rows(rows,self.frame_submissions));
        let rows=probed.as_deref().unwrap_or(rows);
        if let Some(p)=&mut self.baked {
            // AUTHOR POLICY: upload live display/transform data only. Never
            // resolve floor lighting, plan optical transport, or request a bake.
            self.controls_frame=false;self.feature_counts=None;
            self.upload_workstation_rows(rows)?;
            self.baked.as_mut().unwrap().update(self.queue,rows)?;
            self.info["workstation_shader"]=self.workstation_shader_info();
            return Ok(());
        }
        if let Some(bank)=&mut self.motion_bank{bank.observe(rows);}
        self.controls_frame = false;
        self.feature_counts = None;
        if let Some(response) = &mut self.ruby_response { response.resolve_reuse.invalidate_current(); }
        if self.workstation {if let Some(p)=&mut self.prerender {p.update(self.queue,rows);}}
        let hold_images=self.motion_bank.as_ref().is_some_and(|b|b.ready_slot().is_some())||self.prerender.as_ref().is_some_and(|p|p.selected(self.width,self.height)&&!p.pending);
        let complete=self.deferred_floor.ingest(rows,hold_images)?;
        let rows=complete.as_ref();
        if let Some(p)=&mut self.prerender{p.ledger_packet(rows);}
        if let Some(control)=&mut self.frozen_diagnostic{control.observe_rows(rows);}
        if let Some(retained)=&mut self.final_retention{retained.observe(rows);}
        let size = std::mem::size_of_val(rows);
        if size > super::workstation::STORAGE_BYTES {
            return Err("workstation optical data exceeds storage".into());
        }
        self.upload_workstation_rows(rows)?;
        if self.workstation {
            self.feature_counts = crate::workstation::shader_specialization::feature_counts(rows, super::workstation::FRAME_ROWS);
            self.controls_frame = self.feature_counts.is_some_and(|counts| counts == [0.; 4]);
            if rows.len() == super::workstation::FRAME_ROWS {
                if let (Some(response), Some(plan)) = (&mut self.ruby_response, &mut self.ruby_plan) {
                    let update = plan.update(rows);
                    if let Some(p)=&mut self.prerender{p.ledger_response_plan(&update.rows);}
                    if let Err(error) = response.upload(self.queue, &update, rows) {
                        self.ruby_response_error = Some(error);
                        self.ruby_response.take();
                    }
                }
            }
            self.info["workstation_shader"] = self.workstation_shader_info();
        }
        Ok(())
    }
    unsafe fn upload_workstation_rows(&mut self,rows:&[[f32;4]])->Result<(),String>{
        let enabled=crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_CHANGED_UPLOADS");
        let previous=if enabled{self.workstation_uploaded.as_slice()}else{&[]};
        for (offset,range) in changed_uploads::ranges(rows,previous){
            let bytes=std::mem::size_of_val(range);
            (self.api.wgpuQueueWriteBuffer)(self.queue,self.texture_buffer,(offset*16)as u64,range.as_ptr().cast(),bytes);
            self.workstation_upload_bytes+=bytes as u64;self.workstation_upload_calls+=1;
        }
        if let Err(error)=check_errors(){self.workstation_uploaded.clear();return Err(error);}
        if enabled&&rows.len()==super::workstation::FRAME_ROWS{
            self.workstation_uploaded.clear();self.workstation_uploaded.extend_from_slice(rows);
        }else{self.workstation_uploaded.clear();}
        Ok(())
    }
    pub unsafe fn table_eye(&mut self, view:&Value)->Result<(),String>{
        let eye=crate::scene_assets::table_eye(view)?;
        let light=crate::scene_assets::exterior_light(view);
        (self.api.wgpuQueueWriteBuffer)(self.queue,self.scene_texture(),(self.scene_capacity()-crate::scene_assets::FINISHES.len()-crate::scene_assets::TABLE.len()+crate::scene_assets::EXTERIOR_OFFSET) as u64,light.as_ptr().cast(),32);
        if let Some(retained)=&mut self.final_retention{retained.invalidate();}
        if self.scene_texture()==self.texture_buffer{self.workstation_uploaded.clear();}
        (self.api.wgpuQueueWriteBuffer)(self.queue,self.scene_texture(),(self.scene_capacity()-crate::scene_assets::FINISHES.len()-crate::scene_assets::TABLE.len()) as u64,eye.as_ptr().cast(),16);
        check_errors()
    }
    pub unsafe fn monitor_image(&mut self, image: &[u8]) -> Result<(), String> {
        if let Some(retained)=&mut self.final_retention{retained.invalidate();}
        if self.scene_texture()==self.texture_buffer{self.workstation_uploaded.clear();}
        let offset = crate::scene_assets::SCREEN_OFFSET;
        (self.api.wgpuQueueWriteBuffer)(self.queue,self.scene_texture(),offset as u64,image.as_ptr().cast(),image.len());
        check_errors()
    }
    pub unsafe fn texture(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.workstation_uploaded.clear();
        if let Some(retained)=&mut self.final_retention{retained.invalidate();}if let Some(bank)=&mut self.motion_bank{bank.invalidate();}
        self.deferred_floor.invalidate();
        if let Some(p)=&mut self.prerender {p.invalidate();}
        self.controls_frame = false;
        self.feature_counts = None;
        if let Some(response) = &mut self.ruby_response { response.resolve_reuse.invalidate_current(); }
        let packed = crate::scene_assets::scene_bytes(bytes)?;
        let bytes = packed.as_slice();
        (self.api.wgpuQueueWriteBuffer)(
            self.queue,
            self.texture_buffer,
            0,
            bytes.as_ptr().cast(),
            bytes.len(),
        );
        check_errors()
    }
    pub unsafe fn set_batch(&mut self, triangles: &[[f32; 24]]) -> Result<(), String> {
        if triangles.len() > crate::scene_limits::MAX_SCENE_TRIANGLES {
            return Err(format!("mesh exceeds {} triangles",crate::scene_limits::MAX_SCENE_TRIANGLES));
        }
        self.fullscreen_tail=self.prerender.as_ref().is_some_and(|p|!p.meter_base_render.is_null())&&camera_split::retained_batch(triangles);
        self.camera_vertices=if self.prerender.as_ref().is_some_and(|p|!p.camera_render.is_null()) {camera_split::prefix(triangles)}else{0};
        if self.motion_bank.is_some()&&meter_ledger_state::vertex_content_changed(&self.vertices,triangles)
            &&!meter_ledger_state::camera_rgb_change_only(&self.vertices,triangles){self.motion_bank.as_mut().unwrap().invalidate();}
        if self.prerender.as_ref().is_some_and(|p|p.ledger.is_some())
            &&meter_ledger_state::vertex_content_changed(&self.vertices,triangles){
            // RGB is consumed by the separate camera draw, never the optical
            // capture/replay. Geometry, depth, tags and the full tail stay keyed.
            if self.ledger_camera_rgb&&self.fullscreen_tail&&self.camera_vertices>0
                &&meter_ledger_state::camera_rgb_change_only(&self.vertices,triangles){
                self.ledger_camera_rgb_updates+=1;
            }else{self.prerender.as_mut().unwrap().ledger_external_changed();}
        }
        super::scene::interleave_batch(triangles,&mut self.vertices);
        if !self.vertices.is_empty() {
            (self.api.wgpuQueueWriteBuffer)(
                self.queue,
                self.vertex_buffer,
                0,
                self.vertices.as_ptr().cast(),
                self.vertices.len() * 4,
            );
        }
        // Only the contiguous, proven alpha-one prefix may disable blending.
        // Player cutouts and all later effects keep the original pipeline/order.
        self.opaque_vertices = if self.opaque_pipeline.is_some() {
            (triangles.iter().take_while(|t| {
                let m=t[15];
                m != 121. && !(m > -2.5 && m < -1.5) && !(m <= -3.5 && m > -9.5)
            }).count()*3) as u32
        } else {0};
        self.count = (triangles.len() * 3) as u32;
        Ok(())
    }
    pub unsafe fn set_water(
        &mut self,
        root: &Path,
        packets: &[crate::water::presentation::Packet],
        view_input: &Value,
        triangles: &[[f32; 24]],
    ) -> Result<(), String> {
        if let Some(retained)=&mut self.final_retention{retained.invalidate();}if let Some(bank)=&mut self.motion_bank{bank.invalidate();}
        // This separate game-scene upload is outside the optical frame packet.
        if let Some(p)=&mut self.prerender{p.ledger_external_changed();}
        self.table_eye(view_input)?;
        let cloud_frame = crate::base_system::layer_four::cloud_system::read_frame(view_input)?;
        let bytes: Vec<u8> = cloud_frame
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        (self.api.wgpuQueueWriteBuffer)(
            self.queue,
            self.scene_texture(),
            crate::scene_assets::CLOUD_FRAME_OFFSET as u64,
            bytes.as_ptr().cast(),
            bytes.len(),
        );

        self.water_frames.clear();
        self.reflection_count = 0;
        if packets.is_empty() {
            return Ok(());
        }
        self.water_frames =
            super::scene::water_frames(packets, view_input, self.width, self.height, false)?;
        let api = self.api;
        if self.water.is_none() {
            self.water = Some(super::water::webgpu::Client::new(
                root,
                self.device,
                self.queue,
                &mut |name| api.resolve(name),
            )?);
        }
        self.ensure_scene_targets()?;
        if let Some(frame) = self
            .water_frames
            .iter()
            .find(|f| f.optics.planar_reflection)
        {
            let reflected = super::scene::reflected_batch(triangles, frame)?;
            let mut vertices = Vec::<f32>::with_capacity(reflected.len() * 24);
            for t in &reflected {
                for i in 0..3 {
                    vertices.extend_from_slice(&t[i * 4..i * 4 + 4]);
                    vertices.extend_from_slice(&t[12 + i * 4..16 + i * 4]);
                }
            }
            self.reflection_count = (reflected.len() * 3) as u32;
            if !vertices.is_empty() {
                (api.wgpuQueueWriteBuffer)(
                    self.queue,
                    self.reflection_buffer,
                    0,
                    vertices.as_ptr().cast(),
                    vertices.len() * 4,
                );
            }
        }
        Ok(())
    }
    unsafe fn ensure_scene_targets(&mut self)->Result<(),String>{
        let api=self.api;
        if self.scene_colors[0].is_null() {
            for i in 0..3 {
                let format = if i == 2 {
                    WGPUTextureFormat_Depth32Float
                } else {
                    self.format
                };
                let texture = (api.wgpuDeviceCreateTexture)(
                    self.device,
                    &WGPUTextureDescriptor {
                        usage: WGPUTextureUsage_RenderAttachment | WGPUTextureUsage_TextureBinding | WGPUTextureUsage_CopySrc | WGPUTextureUsage_CopyDst,
                        dimension: WGPUTextureDimension_2D,
                        size: WGPUExtent3D {
                            width: self.width,
                            height: self.height,
                            depthOrArrayLayers: 1,
                        },
                        format,
                        mipLevelCount: 1,
                        sampleCount: 1,
                        ..Default::default()
                    },
                );
                if texture.is_null() {
                    return Err("water opaque texture allocation failed".into());
                }
                let texture_view = (api.wgpuTextureCreateView)(texture, ptr::null());
                if i == 2 {
                    self.reflection_depth = texture;
                    self.reflection_depth_view = texture_view;
                } else {
                    self.scene_colors[i] = texture;
                    self.scene_views[i] = texture_view;
                }
                if texture_view.is_null() {
                    return Err("water opaque texture view failed".into());
                }
            }
        }
        if self.reflection_buffer.is_null() {
            self.reflection_buffer = (api.wgpuDeviceCreateBuffer)(
                self.device,
                &WGPUBufferDescriptor {
                    size: crate::scene_limits::REFLECTION_BUFFER_BYTES,
                    usage: WGPUBufferUsage_Storage | WGPUBufferUsage_CopyDst,
                    ..Default::default()
                },
            );
            if self.reflection_buffer.is_null() {
                return Err("water reflection buffer failed".into());
            }
            let layout = (api.wgpuRenderPipelineGetBindGroupLayout)(self.scene_pipeline(), 0);
            let entries = [
                WGPUBindGroupEntry {
                    binding: 0,
                    buffer: self.reflection_buffer,
                    size: crate::scene_limits::REFLECTION_BUFFER_BYTES,
                    ..Default::default()
                },
                WGPUBindGroupEntry {
                    binding: 1,
                    buffer: self.scene_texture(),
                    size: self.scene_capacity() as u64,
                    ..Default::default()
                },
            ];
            self.reflection_group = (api.wgpuDeviceCreateBindGroup)(
                self.device,
                &WGPUBindGroupDescriptor {
                    layout,
                    entryCount: 2,
                    entries: entries.as_ptr(),
                    ..Default::default()
                },
            );
            (api.wgpuBindGroupLayoutRelease)(layout);
            if self.reflection_group.is_null() {
                return Err("water reflection binding failed".into());
            }
        }
        Ok(())
    }
    unsafe fn release_scene_targets(&mut self){
        for v in &mut self.scene_views {if !v.is_null(){(self.api.wgpuTextureViewRelease)(*v);*v=ptr::null_mut();}}
        for t in &mut self.scene_colors {if !t.is_null(){(self.api.wgpuTextureRelease)(*t);*t=ptr::null_mut();}}
        if !self.reflection_depth_view.is_null(){(self.api.wgpuTextureViewRelease)(self.reflection_depth_view);self.reflection_depth_view=ptr::null_mut();}
        if !self.reflection_depth.is_null(){(self.api.wgpuTextureRelease)(self.reflection_depth);self.reflection_depth=ptr::null_mut();}
    }
    pub unsafe fn set_vegetation(
        &mut self,
        root: &Path,
        packet: Option<crate::vegetation::packet::Packet>,
    ) -> Result<(), String> {
        if let Some(retained)=&mut self.final_retention{retained.invalidate();}if let Some(bank)=&mut self.motion_bank{bank.invalidate();}
        if let Some(p)=&mut self.prerender{p.ledger_external_changed();}
        self.vegetation_active = packet.is_some();
        self.reflection_plants_active = false;
        if let (Some(packet), Some(frame)) = (
            &packet,
            self.water_frames
                .iter()
                .find(|f| f.optics.planar_reflection),
        ) {
            let reflected = super::scene::reflected_plants(packet, frame)?;
            if self.reflection_vegetation.is_none() {
                let api = self.api;
                self.reflection_vegetation = Some(super::vegetation::webgpu::Client::new(
                    root,
                    self.device,
                    self.queue,
                    self.scene_texture(),
                    self.format,
                    &mut |name| api.resolve(name),
                )?);
            }
            self.reflection_vegetation
                .as_mut()
                .unwrap()
                .upload(&reflected)?;
            self.reflection_plants_active = true;
        }

        if let Some(packet) = packet {
            if self.vegetation.is_none() {
                let api = self.api;
                self.vegetation = Some(super::vegetation::webgpu::Client::new(
                    root,
                    self.device,
                    self.queue,
                    self.scene_texture(),
                    self.format,
                    &mut |name| api.resolve(name),
                )?);
            }
            self.vegetation.as_mut().unwrap().upload(&packet)?;
        }
        Ok(())
    }
    pub(super) fn observe_workstation_input(&mut self,observed:Instant){
        if self.frame_attribution {self.previous_input_observed=self.input_observed;self.input_observed=Some(observed);self.input_observations+=1;}
    }
    fn frame_overlap_info(&self)->Value {json!({"enabled":self.frame_overlap,
        "pending_frames":if self.game_fifo{Value::Null}else{json!(u32::from(self.frame_in_flight.get()))},"pending_limit":if self.game_fifo{2}else{1},"game_fifo_surface_pacing":self.game_fifo,
        "overlap_submissions":self.overlap_submissions,"prior_frame_waits":self.prior_frame_waits,
        "acquire_before_wait":self.acquire_before_wait&&self.frame_overlap,
        "submitted_frames":self.frame_submissions,"completed_frames":self.frame_completions.get(),
        "present_calls":self.present_calls,"successful_presents":self.successful_presents,
        "diagnostic_offscreen":self.diagnostic_offscreen,"surface_acquire_calls":self.surface_acquisitions,
        "counter_scope":"Submitted draw commands; completions confirmed by blocking device drain. Successful surface present calls are not measured scanout. At most one submitted frame remains unconfirmed; native surface queue depth is not measured.",
        "policy":"Only captured, retained, non-bake frames may return after submission; complete the previous frame before encoding the next. Captures, bakes and fallback remain synchronous."})}
    pub(super) fn needs_redraw(&self)->bool {self.baked.as_ref().is_some_and(|p|p.needs_redraw())||self.prerender.as_ref().is_some_and(|p|p.selected(self.width,self.height)&&p.needs_redraw())}
    unsafe fn acquire_frame_surface(&mut self,frame:&mut Frame)->Result<f64,String>{
        let api=self.api;let mut surface=WGPUSurfaceTexture::default();
        self.surface_acquisitions+=1;let started=Instant::now();
        (api.wgpuSurfaceGetCurrentTexture)(self.surface,&mut surface);
        let acquire_ended=Instant::now();let elapsed=acquire_ended.duration_since(started).as_secs_f64()*1000.;
        let (reason,recoverable)=surface_acquire_status(surface.status);
        self.info["surface_acquire"]=json!({"reason":reason,"code":surface.status,"recoverable":recoverable,"texture_available":!surface.texture.is_null(),"call_wall_ms":elapsed});
        if crate::workstation::frame_cadence::timeline_enabled(){let stamp=crate::workstation::frame_cadence::timeline_stamp;self.info["surface_acquire"]["timeline_ms"]=json!([stamp(started),stamp(acquire_ended)]);}
        if surface.texture.is_null()||!matches!(surface.status,WGPUSurfaceGetCurrentTextureStatus_SuccessOptimal|WGPUSurfaceGetCurrentTextureStatus_SuccessSuboptimal){
            if !surface.texture.is_null(){(api.wgpuTextureRelease)(surface.texture);}
            let backend_error=check_errors().err();if let Some(retained)=&mut self.final_retention{retained.invalidate();}if let Some(bank)=&mut self.motion_bank{bank.invalidate();}
            return Err(format!("WebGPU surface acquire failed: {reason} ({}); recoverable={recoverable}{}",surface.status,backend_error.map(|e|format!("; {e}")).unwrap_or_default()));
        }
        frame.texture=surface.texture;frame.view=(api.wgpuTextureCreateView)(frame.texture,ptr::null());
        if frame.view.is_null(){if let Some(retained)=&mut self.final_retention{retained.invalidate();}if let Some(bank)=&mut self.motion_bank{bank.invalidate();}return Err("WebGPU texture view failed".into());}
        Ok(elapsed)
    }

    pub(super) unsafe fn validation_prepare(&mut self)->Result<(),String>{
        let Some(validation)=self.refresh_validation.as_ref()else{return Ok(());};
        let mode=validation.mode.clone();let index=self.frame_submissions;let rows=validation.rows(index);let capture=validation.capture_due(index);
        #[cfg(target_os="ios")]
        if mode=="resize"{return Err("Desktop scripted resize validation is unavailable on UIKit".into());}
        #[cfg(not(target_os="ios"))]
        if mode=="resize"{let size=match index{32=>Some([1100,700]),40=>Some([900,600]),41=>Some([1024,680]),42=>Some([800,600]),110=>Some([960,600]),_=>None};if let Some([w,h])=size{
            self.window.symbol::<unsafe extern "C" fn(*mut c_void,i32,i32)>(b"glfwSetWindowSize\0")?(self.window.handle,w,h);
        }}
        if let Some(rows)=rows{self.set_workstation_inner(&rows)?;}
        if capture{self.captured=false;self.refresh_validation.as_mut().unwrap().capture_at=Some(index);}
        Ok(())
    }
    unsafe fn refresh_surface_dimensions(&mut self)->Result<(),String>{
        if !self.workstation{return Ok(());}let Some((w,h))=self.window.available_dimensions()?else{return Ok(());};
        if (w,h)==(self.width,self.height){return Ok(());}
        if self.diagnostic_offscreen||self.frozen_mode!=0||self.final_mode||(!self.world_active&&!self.water_frames.is_empty()){return Err("workstation resize is unsupported with optional diagnostic/composite targets".into());}
        self.wait()?;let a=self.api;let stride=(w*4).div_ceil(256)*256;
        let depth=(a.wgpuDeviceCreateTexture)(self.device,&WGPUTextureDescriptor{label:view("Resized workstation depth"),dimension:WGPUTextureDimension_2D,size:WGPUExtent3D{width:w,height:h,depthOrArrayLayers:1},format:WGPUTextureFormat_Depth32Float,mipLevelCount:1,sampleCount:1,usage:WGPUTextureUsage_RenderAttachment|WGPUTextureUsage_TextureBinding|WGPUTextureUsage_CopySrc|WGPUTextureUsage_CopyDst,..Default::default()});
        if depth.is_null(){return Err("resized depth allocation failed".into());}
        let depth_view=(a.wgpuTextureCreateView)(depth,ptr::null());
        let read=(a.wgpuDeviceCreateBuffer)(self.device,&WGPUBufferDescriptor{label:view("Resized workstation readback"),size:stride as u64*h as u64,usage:WGPUBufferUsage_MapRead|WGPUBufferUsage_CopyDst,..Default::default()});
        if depth_view.is_null()||read.is_null(){if !depth_view.is_null(){(a.wgpuTextureViewRelease)(depth_view);}if !read.is_null(){(a.wgpuBufferRelease)(read);}(a.wgpuTextureRelease)(depth);return Err("resized target allocation failed".into());}
        if let Some(p)=&mut self.prerender{if let Err(e)=p.resized(self.queue,self.frame_completions.get(),w,h){(a.wgpuTextureViewRelease)(depth_view);(a.wgpuTextureRelease)(depth);(a.wgpuBufferRelease)(read);return Err(e);}}
        let extra=requested_surface_latency(std::env::var("WORMWOOD_WORKSTATION_SURFACE_LATENCY").ok().as_deref())?.map(|latency|WGPUSurfaceConfigurationExtras{chain:WGPUChainedStruct{next:ptr::null(),sType:WGPUSType_SurfaceConfigurationExtras as WGPUSType},desiredMaximumFrameLatency:latency});
        (a.wgpuSurfaceUnconfigure)(self.surface);
        (a.wgpuSurfaceConfigure)(self.surface,&WGPUSurfaceConfiguration{nextInChain:extra.as_ref().map_or(ptr::null(),|e|&e.chain),device:self.device,format:self.format,usage:WGPUTextureUsage_RenderAttachment|WGPUTextureUsage_CopySrc,width:w,height:h,alphaMode:WGPUCompositeAlphaMode_Auto,presentMode:WGPUPresentMode_Fifo,..Default::default()});
        (a.wgpuTextureViewRelease)(self.depth_view);(a.wgpuTextureRelease)(self.depth);(a.wgpuBufferRelease)(self.read_buffer);
        let from=[self.width,self.height];self.depth=depth;self.depth_view=depth_view;self.read_buffer=read;self.width=w;self.height=h;self.stride=stride;
        if self.final_retention.as_ref().is_some_and(|r|r.complete){
            let validate=self.final_retention.as_ref().unwrap().validate;
            self.final_retention.take();
            self.final_retention=Some(final_retention::Retention::composition(self,validate)?);
        }
        if self.world_active && !self.water_frames.is_empty(){
            self.release_scene_targets();self.ensure_scene_targets()?;
            for frame in &mut self.water_frames {frame.camera.width=w;frame.camera.height=h;}
        }
        self.info["width"]=json!(w);self.info["height"]=json!(h);self.surface_resizes+=1;
        self.info["surface_resize"]=json!({"count":self.surface_resizes,"from":from,"to":[w,h],"submission_drained":self.frame_completions.get(),"scope":"Actual GLFW dimensions; surface unconfigure/configure plus new depth/readback targets. Pending staging discarded after GPU drain."});check_errors()
    }

    pub unsafe fn draw_background(&mut self)->Result<(),String>{
        self.wait()?;
        if self.background_target.is_null() || self.background_size!=[self.width,self.height] {
            self.wait()?;
            if !self.background_target.is_null(){(self.api.wgpuTextureRelease)(self.background_target);self.background_target=ptr::null_mut();}
            self.background_target=(self.api.wgpuDeviceCreateTexture)(self.device,&WGPUTextureDescriptor{
                label:view("Resident baked DAW monitor"),
                usage:WGPUTextureUsage_RenderAttachment|WGPUTextureUsage_CopySrc|WGPUTextureUsage_CopyDst|WGPUTextureUsage_TextureBinding,
                dimension:WGPUTextureDimension_2D,size:WGPUExtent3D{width:self.width,height:self.height,depthOrArrayLayers:1},
                format:self.format,mipLevelCount:1,sampleCount:1,..Default::default()
            });
            if self.background_target.is_null(){return Err("resident monitor allocation failed".into());}
            self.background_size=[self.width,self.height];
        }
        // Production unlit compositor only. Keep the resident world's meshes,
        // water, vegetation, reflection pipelines and paused image intact.
        let api=self.api;
        let mut frame=Frame{api,texture:self.background_target,owns_texture:false,view:ptr::null_mut(),encoder:ptr::null_mut(),pass:ptr::null_mut(),command:ptr::null_mut()};
        frame.view=(api.wgpuTextureCreateView)(frame.texture,ptr::null());
        frame.encoder=(api.wgpuDeviceCreateCommandEncoder)(self.device,ptr::null());
        if frame.view.is_null()||frame.encoder.is_null(){return Err("resident display encoder/view failed".into());}
        let color=WGPURenderPassColorAttachment{view:frame.view,depthSlice:u32::MAX,loadOp:WGPULoadOp_Clear,storeOp:WGPUStoreOp_Store,clearValue:WGPUColor{r:0.015,g:0.015,b:0.02,a:1.},..Default::default()};
        let depth=WGPURenderPassDepthStencilAttachment{view:self.depth_view,depthLoadOp:WGPULoadOp_Clear,depthStoreOp:WGPUStoreOp_Store,depthClearValue:1.,..Default::default()};
        frame.pass=(api.wgpuCommandEncoderBeginRenderPass)(frame.encoder,&WGPURenderPassDescriptor{colorAttachmentCount:1,colorAttachments:&color,depthStencilAttachment:&depth,..Default::default()});
        if frame.pass.is_null(){return Err("resident display pass failed".into());}
        self.baked.as_mut().ok_or("resident display requires baked artwork")?.animate(self.queue);
        self.baked.as_ref().ok_or("resident display requires baked artwork")?.draw(frame.pass,0);
        (api.wgpuRenderPassEncoderEnd)(frame.pass);
        let source=WGPUTexelCopyTextureInfo{texture:frame.texture,aspect:WGPUTextureAspect_All,..Default::default()};
        let destination=WGPUTexelCopyBufferInfo{buffer:self.read_buffer,layout:WGPUTexelCopyBufferLayout{bytesPerRow:self.stride,rowsPerImage:self.height,..Default::default()}};
        (api.wgpuCommandEncoderCopyTextureToBuffer)(frame.encoder,&source,&destination,&WGPUExtent3D{width:self.width,height:self.height,depthOrArrayLayers:1});
        frame.command=(api.wgpuCommandEncoderFinish)(frame.encoder,ptr::null());
        if frame.command.is_null(){return Err("resident display command failed".into());}
        (api.wgpuQueueSubmit)(self.queue,1,&frame.command);
        let size=self.stride as usize*self.height as usize;
        let mut state=Box::new(Wait::default());
        (api.wgpuBufferMapAsync)(self.read_buffer,WGPUMapMode_Read,0,size,WGPUBufferMapCallbackInfo{mode:WGPUCallbackMode_AllowProcessEvents,callback:Some(mapped),userdata1:(&mut *state as *mut Wait).cast(),..Default::default()});
        wait(api,self.instance,self.device,state)?;
        let mapped=(api.wgpuBufferGetConstMappedRange)(self.read_buffer,0,size);
        if mapped.is_null(){return Err("resident display readback failed".into());}
        let bytes=std::slice::from_raw_parts(mapped.cast::<u8>(),size);
        let mut ppm=format!("P6\n{} {}\n255\n",self.width,self.height).into_bytes();
        for row in bytes.chunks_exact(self.stride as usize){for pixel in row[..self.width as usize*4].chunks_exact(4){
            if self.format==WGPUTextureFormat_BGRA8Unorm{ppm.extend_from_slice(&[pixel[2],pixel[1],pixel[0]]);}else{ppm.extend_from_slice(&pixel[..3]);}
        }}
        (api.wgpuBufferUnmap)(self.read_buffer);
        let image=crate::scene_assets::monitor_image(&ppm)?;
        self.monitor_capture=Some(image);
        self.captured=true;
        check_errors()
    }
    pub unsafe fn draw_hidden_scene(&mut self)->Result<(),String>{
        // The first background composition has already allocated this target.
        if self.background_target.is_null(){return Err("hidden scene needs initialized resident target".into());}
        self.hidden_scene=true;
        let result=self.draw().and_then(|_|self.wait());
        self.hidden_scene=false;
        result
    }
    pub unsafe fn draw(&mut self) -> Result<(), String> {
        // Independent of performance/query instrumentation; retain only this draw.
        let mut checkpoints = self.frame_attribution.then(|| [Instant::now(); 12]);
        let mut mark = |at: usize| { if let Some(times) = &mut checkpoints { times[at] = Instant::now(); } };
        if let Some(baked)=&mut self.baked{baked.animate(self.queue);}
        let timeline_enabled=crate::workstation::frame_cadence::timeline_enabled();
        let capture_requested = !self.captured;
        let mut query_recorded = false;
        // Queue writes and submissions are ordered by WebGPU. In the optional
        // game FIFO path, surface acquisition bounds frames in flight instead
        // of forcing the entire device idle before every draw. Captures,
        // teardown and workstation paths retain explicit completion waits.
        let waited_for_prior=self.frame_in_flight.get()&&!self.game_fifo;
        let offscreen=self.diagnostic_offscreen||self.hidden_scene;
        let late_acquire=self.final_retention.as_ref().is_some_and(|r|r.direct)&&!offscreen;
        let acquire_first=self.acquire_before_wait&&waited_for_prior&&!late_acquire;
        let mut prior_wait_ms=0.;
        if waited_for_prior&&!acquire_first {
            let prior_wait=Instant::now();self.wait()?;self.prior_frame_waits+=1;
            prior_wait_ms=prior_wait.elapsed().as_secs_f64()*1000.;
        }
        if !self.hidden_scene {self.refresh_surface_dimensions()?;}
        self.poll_performance()?;
        let retain_world=self.world_active&&self.baked.is_some()&&crate::workstation::paused_world::enabled("WebGPU");
        if retain_world&&self.world.as_ref().and_then(|w|w.image.as_ref()).is_none_or(|i|i.size!=[self.width,self.height]||i.format!=self.format){
            let image=world_image::Image::new(self)?;self.world.as_mut().unwrap().image=Some(image);
        }
        let replay_world=retain_world&&self.world.as_ref().and_then(|w|w.image.as_ref()).is_some_and(|i|i.ready);
        mark(1);
        let api = self.api;
        let mut frame=Frame{api,texture:if self.hidden_scene{self.background_target}else if self.diagnostic_offscreen{self.diagnostic_target}else{ptr::null_mut()},owns_texture:!offscreen,view:ptr::null_mut(),encoder:ptr::null_mut(),pass:ptr::null_mut(),command:ptr::null_mut()};
        let mut surface_acquire_ms=0.;
        if offscreen{
            self.info["surface_acquire"]=json!({"skipped":true,"reason":"offscreen_diagnostic"});
        }else if !late_acquire{surface_acquire_ms=self.acquire_frame_surface(&mut frame)?;}
        mark(2);
        // Acquisition can block while the previous GPU frame completes.
        // Own the acquired texture before any fallible wait, then drain before
        // reusing resources, timing slots, encoding or submitting another frame.
        if acquire_first {
            let prior_wait=Instant::now();self.wait()?;self.prior_frame_waits+=1;
            prior_wait_ms=prior_wait.elapsed().as_secs_f64()*1000.;
            self.poll_performance()?;
        }
        debug_assert!(!self.frame_in_flight.get()||self.game_fifo);
        if let Some(bank)=&mut self.motion_bank{bank.completed(self.frame_completions.get());}
        if let Some(retained)=&mut self.final_retention{retained.completed(self.frame_completions.get());}
        mark(3);
        if offscreen{
            frame.view=(api.wgpuTextureCreateView)(frame.texture,ptr::null());
            if frame.view.is_null(){return Err("WebGPU texture view failed".into());}
        }
        frame.encoder = (api.wgpuDeviceCreateCommandEncoder)(self.device, ptr::null());
        if frame.encoder.is_null() {
            return Err("WebGPU command encoder failed".into());
        }
        let retained_started=Instant::now();
        let motion_eligible=!self.world_active&&self.motion_comparison&&self.fullscreen_tail&&self.camera_vertices>0&&self.camera_vertices+6==self.count
            &&self.water_frames.is_empty()&&!self.vegetation_active&&!self.final_mode&&self.frozen_mode==0
            &&self.prerender.as_ref().is_some_and(|p|p.selected(self.width,self.height));
        let motion_slot=if motion_eligible{self.motion_bank.as_ref().filter(|b|b.dimensions_match(self.width,self.height,self.format)).and_then(|b|b.ready_slot())}else{None};
        if let Some(p)=&mut self.prerender {p.stage_completed(self.queue,self.frame_completions.get())?;
            if motion_slot.is_none(){if motion_eligible{p.comparison_capture_current(self.queue);}p.prepare_draw(self.queue)?;}
        }
        // A final refresh can be requested without another application packet.
        // Resolve the most recent input before either optical bake or fallback.
        let retained_only=self.baked.is_some()||motion_slot.is_some()||self.prerender.as_ref().is_some_and(|p|p.selected(self.width,self.height)&&!p.pending&&!p.needs_stage_snapshot());
        if !retained_only {
            if let Some(rows)=self.deferred_floor.resolve_pending()? {
                if let Some(p)=&mut self.prerender{p.ledger_packet(&rows);}
                if let Some(retained)=&mut self.final_retention{retained.observe(&rows);}
                self.upload_workstation_rows(&rows)?;
                if let (Some(response),Some(plan))=(&mut self.ruby_response,&mut self.ruby_plan){
                    let update=plan.update(&rows);
                    if let Some(p)=&mut self.prerender{p.ledger_response_plan(&update.rows);}
                    response.upload(self.queue,&update,&rows)?;
                }
            }
        }
        if let Some(timing) = &mut self.gpu_timing {
            timing.begin();
            query_recorded = timing.current.is_some();
        }
        let bake_timestamps = self.gpu_timing.as_ref().and_then(|timing|timing.compute_writes(0));
        let general_timestamps = self.gpu_timing.as_ref().and_then(|timing|timing.compute_writes(1));
        let resolve_timestamps = self.gpu_timing.as_ref().and_then(|timing|timing.compute_writes(2));
        let retained_selected=self.prerender.as_ref().is_some_and(|p|p.selected(self.width,self.height));
        let retained_bake=motion_slot.is_none()&&retained_selected&&self.prerender.as_ref().is_some_and(|p|p.pending);
        let ruby_baked = if let Some(response) = self.ruby_response.as_ref().filter(|_|!retained_selected||retained_bake) {
            response.encode_bake(frame.encoder, bake_timestamps.as_ref(), general_timestamps.as_ref())?
        } else { false };
        let ruby_resolved = if let Some(response) = self.ruby_response.as_ref().filter(|_|!retained_selected||retained_bake) {
            response.encode_resolve(frame.encoder, resolve_timestamps.as_ref())?
        } else { false };
        if let Some(timing) = &mut self.gpu_timing {
            timing.optical_passes(ruby_baked, ruby_resolved);
        }
        let capture_timestamps=self.gpu_timing.as_ref().and_then(GpuTiming::capture_writes);
        let captured=if retained_bake {self.prerender.as_mut().unwrap().encode(frame.encoder,capture_timestamps.as_ref())?}else{false};
        if let Some(timing)=&mut self.gpu_timing{timing.capture_pass(captured);}
        let mut incremental_chunk=false;
        if !retained_selected{if let Some(p)=&mut self.prerender{p.cancel_stage();}}
        if motion_slot.is_none()&&retained_selected&&!retained_bake{if let (Some(p),Some(response))=(&mut self.prerender,&self.ruby_response){incremental_chunk=p.encode_stage(frame.encoder,self.queue,&response.incremental_entries)?;}}

        // At most one prior frame was in flight and it completed at draw entry.
        // Capture precedes fragment work; this draw still uses the dummy map.
        let ledger_encoded=if motion_slot.is_none()&&self.prerender.as_ref().is_some_and(|p|p.ledger.is_some()){
            let eligible=retained_selected&&!retained_bake&&!incremental_chunk&&self.fullscreen_tail
                &&self.frozen_mode==0&&!self.final_mode&&self.water_frames.is_empty()&&!self.vegetation_active
                &&self.vertices.iter().all(|v|v.is_finite());
            let dependencies=[self.vertex_buffer as usize as u64,self.texture_buffer as usize as u64,
                self.count as u64,self.camera_vertices as u64,self.fullscreen_tail as u64,self.format as u64,
                self.ruby_response.as_ref().map_or(0,|r|r.bake_submissions),
                self.ruby_response.as_ref().map_or(0,|r|r.resolve_submissions)];
            self.prerender.as_mut().unwrap().encode_ledger(frame.encoder,self.queue,self.frame_completions.get(),eligible,&dependencies)?
        }else{false};
        let full_game_timing=self.gpu_timing.as_ref().is_some_and(|t|t.full_game_frame);
        let game_vegetation=!replay_world&&self.vegetation_active;
        let game_water=!replay_world&&!self.water_frames.is_empty();
        let game_reflection=!replay_world&&self.water_frames.iter().any(|f|f.optics.planar_reflection);
        let game_begin=self.gpu_timing.as_ref().filter(|_|full_game_timing&&game_reflection).and_then(GpuTiming::writes)
            .and_then(|_|self.gpu_timing.as_ref().and_then(|t|t.game_writes(2)));
        let game_plant_writes=self.gpu_timing.as_ref().and_then(|t|t.game_writes(0));
        let game_water_writes=self.gpu_timing.as_ref().and_then(|t|t.game_writes(1));
        if full_game_timing {if let Some(i)=self.gpu_timing.as_ref().and_then(|t|t.current){self.gpu_timing.as_mut().unwrap().slots[i].active_passes=[game_vegetation,game_water,game_reflection,true];}}
        if game_reflection {
            let color = WGPURenderPassColorAttachment {
                view: self.scene_views[1],
                depthSlice: u32::MAX,
                loadOp: WGPULoadOp_Clear,
                storeOp: WGPUStoreOp_Store,
                clearValue: WGPUColor {
                    r: self.clear[0] as f64,
                    g: self.clear[1] as f64,
                    b: self.clear[2] as f64,
                    a: 1.,
                },
                ..Default::default()
            };
            let depth = WGPURenderPassDepthStencilAttachment {
                view: self.reflection_depth_view,
                depthLoadOp: WGPULoadOp_Clear,
                depthStoreOp: WGPUStoreOp_Store,
                depthClearValue: 1.,
                ..Default::default()
            };
            let pass = (api.wgpuCommandEncoderBeginRenderPass)(
                frame.encoder,
                &WGPURenderPassDescriptor {
                    colorAttachmentCount: 1,
                    colorAttachments: &color,
                    depthStencilAttachment: &depth,
                    timestampWrites:game_begin.as_ref().map_or(ptr::null(),|t|t),
                    ..Default::default()
                },
            );
            if pass.is_null() {
                return Err("water reflected scene pass failed".into());
            }
            (api.wgpuRenderPassEncoderSetPipeline)(pass, self.scene_pipeline());
            (api.wgpuRenderPassEncoderSetBindGroup)(pass, 0, self.reflection_group, 0, ptr::null());
            (api.wgpuRenderPassEncoderDraw)(pass, self.reflection_count, 1, 0, 0);
            (api.wgpuRenderPassEncoderEnd)(pass);
            (api.wgpuRenderPassEncoderRelease)(pass);
            if self.reflection_plants_active {
                self.reflection_vegetation.as_ref().unwrap().draw(
                    frame.encoder,
                    self.scene_views[1],
                    self.reflection_depth_view,
                )?;
            }
        }
        if let Some(control)=&mut self.frozen_diagnostic {
            // The normal startup draw can precede retained-pipeline admission.
            // Freeze only at the fourth warmup submission, after that bootstrap.
            if self.frame_submissions>=3{
                if !retained_selected{return Err("Frozen diagnostic requires selected retained pipeline at freeze".into());}
                control.prepare(self.device,frame.encoder,self.prerender.as_ref().unwrap(),self.width,self.height,self.format,self.count)?;
            }
        }
        let frozen_copy_draw=self.frozen_diagnostic.as_ref().is_some_and(|c|c.is_copy());
        let frozen_ready=self.frozen_diagnostic.as_ref().is_some_and(|c|c.ready);
        let mut final_plan=None;
        if !self.world_active&&self.final_retention.is_some(){
            let dependencies=self.prerender.as_ref().map(|p|p.final_dependencies()).unwrap_or_default();
            let mut dependencies=dependencies;
            dependencies.extend([self.vertex_buffer as usize as u64,self.texture_buffer as usize as u64,self.count as u64,self.format as u64]);
            dependencies.extend(self.vertices.iter().map(|v|v.to_bits() as u64));
            dependencies.extend(self.clear.iter().map(|v|v.to_bits() as u64));
            let complete_composition=self.final_retention.as_ref().is_some_and(|r|r.complete);
            // Key the actual selected optical program and its resolved caches,
            // including fallback families outside the retained-panel pipeline.
            let selected_program=if retained_selected {
                let p=self.prerender.as_ref().unwrap();[p.render as usize as u64,p.render_bind_group() as usize as u64]
            }else if let Some(r)=self.ruby_response.as_ref().filter(|r|r.ready_count>0){
                [r.render as usize as u64,r.bind_group as usize as u64]
            }else if let Some(c)=self.controls_pipeline.as_ref().filter(|_|self.controls_frame){
                [c.pipeline as usize as u64,c.bind_group as usize as u64]
            }else{[self.pipeline as usize as u64,self.bind_groups[0] as usize as u64]};
            dependencies.extend(selected_program);
            if let Some(r)=&self.ruby_response{dependencies.extend([r.bake_submissions,r.resolve_submissions]);}
            let reusable=(complete_composition||(retained_selected&&!retained_bake&&!ruby_baked&&!ruby_resolved
                &&self.prerender.as_ref().is_some_and(|p|p.final_safe&&p.prepared)))
                &&self.water_frames.is_empty()&&!self.vegetation_active&&!self.reflection_plants_active
                &&self.vertices.iter().all(|v|v.is_finite());
            final_plan=Some(self.final_retention.as_mut().unwrap().plan(dependencies,reusable));
        }
        let final_validate=self.final_retention.as_ref().is_some_and(|r|r.validate);
        let direct_target=self.final_retention.as_ref().is_some_and(|r|r.direct);
        let direct_cache_draw=direct_target&&final_plan.as_ref().is_some_and(|p|p.eligible&&p.original_draw());
        // Encode retained-target work without owning a drawable. Fallback and
        // unchanged-copy frames acquire just before their sole surface pass.
        if late_acquire&&!direct_cache_draw{surface_acquire_ms=self.acquire_frame_surface(&mut frame)?;}
        let opaque_view = if direct_cache_draw {
            self.final_retention.as_ref().unwrap().target_view()
        } else if replay_world||self.water_frames.is_empty() {
            frame.view
        } else {
            self.scene_views[0]
        };
        let motion_captured=if motion_slot.is_none()&&motion_eligible{
            if let (Some(bank),Some(p))=(&mut self.motion_bank,&self.prerender){
                let split=!p.meter_base_render.is_null()&&p.meter_regions.is_some();
                bank.encode_capture(frame.encoder,self.depth_view,if split{p.meter_base_render}else{p.render},p.render_bind_group(),self.count,self.camera_vertices,true,if split{p.meter_overlay_render}else{ptr::null_mut()},p.meter_regions.as_deref())?
            }else{false}
        }else{false};
        let cloud_scale = if !self.workstation && self.count>=6 &&
            self.vertices[..48].chunks_exact(8).all(|v|v[7]<= -9.5) {
            cloud_resolution::divisor()
        } else {1};
        if cloud_scale > 1 {
            let size=(self.width.div_ceil(cloud_scale),self.height.div_ceil(cloud_scale));
            if self.sky_target.as_ref().is_none_or(|s|s.size!=size){
                self.sky_target=Some(cloud_resolution::Sky::new(self,size)?);
            }
            self.sky_target.as_ref().unwrap().encode(self,frame.encoder)?;
        } else { self.sky_target.take(); }
        let attachment = WGPURenderPassColorAttachment {
            view: opaque_view,
            depthSlice: u32::MAX,
            loadOp: if direct_cache_draw&&final_plan.as_ref().unwrap().partial().is_some(){WGPULoadOp_Load}else{WGPULoadOp_Clear},
            storeOp: WGPUStoreOp_Store,
            clearValue: WGPUColor {
                r: self.clear[0] as f64,
                g: self.clear[1] as f64,
                b: self.clear[2] as f64,
                a: 1.0,
            },
            ..Default::default()
        };
        let depth_attachment = WGPURenderPassDepthStencilAttachment {
            view: self.depth_view,
            depthLoadOp: WGPULoadOp_Clear,
            depthStoreOp: WGPUStoreOp_Store,
            depthClearValue: 1.,
            ..Default::default()
        };
        let timestamps = self.gpu_timing.as_ref().and_then(GpuTiming::writes);
        let first_timestamps=timestamps.as_ref().map(|t|WGPURenderPassTimestampWrites{querySet:t.querySet,
            beginningOfPassWriteIndex:t.beginningOfPassWriteIndex,
            endOfPassWriteIndex:if direct_cache_draw{u32::MAX}else{t.endOfPassWriteIndex}});
        let pass = WGPURenderPassDescriptor {
            colorAttachmentCount: 1,
            colorAttachments: &attachment,
            depthStencilAttachment: &depth_attachment,
            timestampWrites: first_timestamps.as_ref().map_or(ptr::null(), |writes| writes),
            ..Default::default()
        };
        frame.pass = (api.wgpuCommandEncoderBeginRenderPass)(frame.encoder, &pass);
        if frame.pass.is_null() {
            return Err("WebGPU render pass failed".into());
        }
        let meter_split=retained_selected&&!frozen_ready&&!self.final_mode&&self.fullscreen_tail
            &&self.prerender.as_ref().is_some_and(|p|!p.meter_base_render.is_null()&&p.meter_regions.is_some());
        let (pipeline, bind_group) = if frozen_ready {
            self.frozen_diagnostic.as_mut().unwrap().selection(self.prerender.as_ref().unwrap().render)
        } else if retained_selected {
            let p=self.prerender.as_ref().unwrap();(if meter_split{p.meter_base_render}else{p.render},p.render_bind_group())
        } else if let Some(response) = self.ruby_response.as_ref().filter(|response| response.ready_count > 0) {
            (response.render, response.bind_group)
        } else {
            self.controls_pipeline.as_ref().filter(|_| self.controls_frame)
                .map(|variant| (variant.pipeline, variant.bind_group)).unwrap_or((self.pipeline, self.bind_groups[0]))
        };
        if !direct_cache_draw&&final_plan.as_ref().is_some_and(|p|p.copy_first()){self.final_retention.as_ref().unwrap().copy_first(frame.pass);}
        if replay_world{
            self.world.as_mut().unwrap().image.as_mut().unwrap().draw(frame.pass);
        }else if self.world_active {
            let world=self.world.as_ref().unwrap();
            (api.wgpuRenderPassEncoderSetPipeline)(frame.pass,world.pipeline);
            (api.wgpuRenderPassEncoderSetBindGroup)(frame.pass,0,world.group,0,ptr::null());
            (api.wgpuRenderPassEncoderDraw)(frame.pass,self.count-6,1,0,0);
        } else if let Some(baked)=&self.baked {
            if self.count>6 {
                (api.wgpuRenderPassEncoderSetPipeline)(frame.pass,self.pipeline);
                (api.wgpuRenderPassEncoderSetBindGroup)(frame.pass,0,self.bind_groups[0],0,ptr::null());
                (api.wgpuRenderPassEncoderDraw)(frame.pass,self.count-6,1,0,0);
            }
            baked.draw(frame.pass,self.count.saturating_sub(6));
        } else if final_plan.as_ref().is_none_or(|p|p.original_draw()){
            if let Some(r)=final_plan.as_ref().and_then(|p|p.partial()).filter(|_|final_plan.as_ref().unwrap().eligible){
                (api.wgpuRenderPassEncoderSetScissorRect)(frame.pass,r.x,r.y,r.width,r.height);
            }
            let camera_prefix=if retained_selected&&!frozen_ready&&self.camera_vertices>0 {
                let camera=self.prerender.as_ref().unwrap().camera_render;
                if !camera.is_null(){
                    (api.wgpuRenderPassEncoderSetPipeline)(frame.pass,camera);
                    (api.wgpuRenderPassEncoderSetBindGroup)(frame.pass,0,bind_group,0,ptr::null());
                    (api.wgpuRenderPassEncoderDraw)(frame.pass,self.camera_vertices,1,0,0);
                    self.camera_vertices
                }else{0}
            }else{0};
            if let Some(slot)=motion_slot {
                if !self.motion_bank.as_mut().unwrap().draw(frame.pass,slot){return Err("Motion bank admitted state changed within draw".into());}
            }else{
            if meter_split{if let Some(timing)=&mut self.gpu_timing{timing.inside_begin(frame.pass,camera_prefix==0&&self.count>6);}}
            (api.wgpuRenderPassEncoderSetPipeline)(frame.pass, pipeline);
            (api.wgpuRenderPassEncoderSetBindGroup)(frame.pass, 0, bind_group, 0, ptr::null());
            let first_vertex = if let Some(sky)=&self.sky_target {
                sky.composite(frame.pass);
                (api.wgpuRenderPassEncoderSetPipeline)(frame.pass,pipeline);
                (api.wgpuRenderPassEncoderSetBindGroup)(frame.pass,0,bind_group,0,ptr::null());
                6
            } else {camera_prefix};
            if let Some(opaque)=self.opaque_pipeline.as_ref().filter(|_| !self.workstation && pipeline==self.pipeline && self.opaque_vertices>first_vertex) {
                (api.wgpuRenderPassEncoderSetPipeline)(frame.pass,opaque.pipeline);
                (api.wgpuRenderPassEncoderSetBindGroup)(frame.pass,0,opaque.bind_group,0,ptr::null());
                (api.wgpuRenderPassEncoderDraw)(frame.pass,self.opaque_vertices-first_vertex,1,first_vertex,0);
                (api.wgpuRenderPassEncoderSetPipeline)(frame.pass,pipeline);
                (api.wgpuRenderPassEncoderSetBindGroup)(frame.pass,0,bind_group,0,ptr::null());
                if self.count>self.opaque_vertices {
                    (api.wgpuRenderPassEncoderDraw)(frame.pass,self.count-self.opaque_vertices,1,self.opaque_vertices,0);
                }
            } else {
                (api.wgpuRenderPassEncoderDraw)(frame.pass, self.count-first_vertex, 1, first_vertex, 0);
            }
            if meter_split {
                if let Some(timing)=&self.gpu_timing{timing.inside_mark(frame.pass,9);timing.inside_mark(frame.pass,10);}
                let p=self.prerender.as_ref().unwrap();
                (api.wgpuRenderPassEncoderSetPipeline)(frame.pass,p.meter_overlay_render);
                for r in p.meter_regions.as_ref().unwrap(){
                    (api.wgpuRenderPassEncoderSetScissorRect)(frame.pass,r.x,r.y,r.width,r.height);
                    (api.wgpuRenderPassEncoderDraw)(frame.pass,6,1,self.count-6,0);
                }
                (api.wgpuRenderPassEncoderSetScissorRect)(frame.pass,0,0,self.width,self.height);
                if let Some(timing)=&self.gpu_timing{timing.inside_mark(frame.pass,11);}
                self.meter_split_draws+=1;
            }
            }
        }
        (api.wgpuRenderPassEncoderEnd)(frame.pass);
        if !replay_world&&self.vegetation_active {
            if let Some(client) = &self.vegetation {
                client.draw_timed(frame.encoder, opaque_view, self.depth_view,game_plant_writes.as_ref())?;
            }
        }
        if !replay_world&&!self.water_frames.is_empty() {
            self.water.as_mut().unwrap().encode_batch_timed(
                frame.encoder,
                &self.water_frames,
                super::water::BorrowedAttachments {
                    scene_color: self.scene_views[0],
                    scene_depth: self.depth_view,
                    planar_reflection: self
                        .water_frames
                        .iter()
                        .any(|f| f.optics.planar_reflection)
                        .then_some(self.scene_views[1]),
                    output: frame.view,
                },
                self.format,
                WGPUTextureFormat_Depth32Float,
                game_water_writes.as_ref(),
            )?;
        }
        if retain_world&&!replay_world{self.world.as_mut().unwrap().image.as_mut().unwrap().capture(frame.encoder,frame.texture);}
        if retain_world{self.world.as_ref().unwrap().image.as_ref().unwrap().depth_copy(frame.encoder,self.depth,replay_world);}
        if self.world_active && self.world.as_ref().unwrap().overlay_count>0 {
            let color=WGPURenderPassColorAttachment{view:frame.view,depthSlice:u32::MAX,loadOp:WGPULoadOp_Load,storeOp:WGPUStoreOp_Store,..Default::default()};
            let depth=WGPURenderPassDepthStencilAttachment{view:self.depth_view,depthLoadOp:WGPULoadOp_Load,depthStoreOp:WGPUStoreOp_Store,..Default::default()};
            let pass=(api.wgpuCommandEncoderBeginRenderPass)(frame.encoder,&WGPURenderPassDescriptor{colorAttachmentCount:1,colorAttachments:&color,depthStencilAttachment:&depth,..Default::default()});
            self.world.as_ref().unwrap().draw_audio(pass);(api.wgpuRenderPassEncoderEnd)(pass);(api.wgpuRenderPassEncoderRelease)(pass);
        }
        if self.world_active {
            let color=WGPURenderPassColorAttachment{view:frame.view,depthSlice:u32::MAX,loadOp:WGPULoadOp_Load,storeOp:WGPUStoreOp_Store,..Default::default()};
            let depth=WGPURenderPassDepthStencilAttachment{view:self.depth_view,depthLoadOp:WGPULoadOp_Clear,depthStoreOp:WGPUStoreOp_Store,depthClearValue:1.,..Default::default()};
            let overlay=(api.wgpuCommandEncoderBeginRenderPass)(frame.encoder,&WGPURenderPassDescriptor{colorAttachmentCount:1,colorAttachments:&color,depthStencilAttachment:&depth,..Default::default()});
            if overlay.is_null(){return Err("world workstation overlay pass failed".into());}
            if let Some(baked)=&self.baked{baked.draw(overlay,self.count-6);}else{
            (api.wgpuRenderPassEncoderSetPipeline)(overlay,pipeline);
            (api.wgpuRenderPassEncoderSetBindGroup)(overlay,0,bind_group,0,ptr::null());
            (api.wgpuRenderPassEncoderDraw)(overlay,6,1,self.count-6,0);
            }
            (api.wgpuRenderPassEncoderEnd)(overlay);(api.wgpuRenderPassEncoderRelease)(overlay);
        }
        if direct_cache_draw{
            if late_acquire{surface_acquire_ms=self.acquire_frame_surface(&mut frame)?;}
            let copy_timestamps=timestamps.as_ref().map(|t|WGPURenderPassTimestampWrites{querySet:t.querySet,beginningOfPassWriteIndex:u32::MAX,endOfPassWriteIndex:t.endOfPassWriteIndex});
            self.final_retention.as_ref().unwrap().present_direct(frame.encoder,frame.view,self.depth_view,copy_timestamps.as_ref())?;
        }
        if let (Some(retained),Some(plan))=(&self.final_retention,&final_plan){
            retained.update_image(frame.encoder,frame.texture,plan);
            let reference_pipeline=if retained.complete&&retained_selected {
                self.prerender.as_ref().unwrap().render
            }else{pipeline};
            retained.encode_reference(frame.encoder,self.depth_view,reference_pipeline,bind_group,self.count,self.clear)?;
        }
        if let Some(control)=&mut self.frozen_diagnostic{control.capture_reference(frame.encoder,frame.texture,self.width,self.height); }
        if let Some(timing) = &self.gpu_timing {
            timing.encode_resolve(frame.encoder);
        }
        if !self.captured {
            let source = WGPUTexelCopyTextureInfo {
                texture: frame.texture,
                aspect: WGPUTextureAspect_All,
                ..Default::default()
            };
            let destination = WGPUTexelCopyBufferInfo {
                buffer: self.read_buffer,
                layout: WGPUTexelCopyBufferLayout {
                    bytesPerRow: self.stride,
                    rowsPerImage: self.height,
                    ..Default::default()
                },
            };
            let size = WGPUExtent3D {
                width: self.width,
                height: self.height,
                depthOrArrayLayers: 1,
            };
            (api.wgpuCommandEncoderCopyTextureToBuffer)(
                frame.encoder,
                &source,
                &destination,
                &size,
            );
        }
        mark(4);
        frame.command = (api.wgpuCommandEncoderFinish)(frame.encoder, ptr::null());
        if frame.command.is_null() {
            return Err("WebGPU command finish failed".into());
        }
        let submit_begin=timeline_enabled.then(Instant::now);
        (api.wgpuQueueSubmit)(self.queue, 1, &frame.command);
        if retain_world{self.world.as_mut().unwrap().image.as_mut().unwrap().submitted();}
        let submit_end=timeline_enabled.then(Instant::now);
        self.frame_submissions+=1;
        if motion_captured{self.motion_bank.as_mut().unwrap().submitted(self.frame_submissions)?;}
        if incremental_chunk{self.prerender.as_mut().unwrap().stage_submitted(self.frame_submissions)?;}
        if ledger_encoded{self.prerender.as_mut().unwrap().ledger_submitted(self.frame_submissions)?;}
        if let (Some(retained),Some(plan))=(&mut self.final_retention,final_plan.take()){retained.submitted(self.frame_submissions,plan);}
        let input_to_submit_ms=self.previous_input_observed.map(|t|t.elapsed().as_secs_f64()*1000.);
        mark(5);
        if let Some(response) = &mut self.ruby_response {
            response.resolve_reuse.submitted(ruby_resolved, ruby_baked);
        }
        if ruby_baked {
            if let Some(response) = &mut self.ruby_response {
                response.bake_pending = false;
                response.bake_submissions += 1;
            }
            if let Some(plan) = &mut self.ruby_plan { plan.submitted(); }
            self.info["workstation_shader"] = self.workstation_shader_info();
        }
        if ruby_resolved {
            if let Some(response) = &mut self.ruby_response { response.resolve_submissions += 1; }
            self.info["workstation_shader"] = self.workstation_shader_info();
        }
        if let Some(timing) = &mut self.gpu_timing {
            timing.submitted();
        }
        mark(6);
        // The prior submitted frame completed at draw entry. WebGPU copies
        // QueueWriteBuffer inputs immediately and orders writes with submissions;
        // submitted commands retain resources (as in the existing game path).
        // Ordinary retained draws can overlap the next main-thread audio/Step.
        // Optical bakes/readbacks/fallback keep their completion semantics.
        let overlap_this_frame=self.frame_overlap&&self.captured&&retained_selected&&!retained_bake&&!final_validate&&!motion_captured;
        if overlap_this_frame {
            debug_assert!(!self.frame_in_flight.get());
            self.frame_in_flight.set(true);self.overlap_submissions+=1;
            (api.wgpuDevicePoll)(self.device,0,ptr::null());
            (api.wgpuInstanceProcessEvents)(self.instance);check_errors()?;
        } else if !self.captured || self.workstation {
            self.wait()?;
        } else {
            // Let CPU simulation overlap this submission, but wait for it at
            // the next draw before queuing more work on the game device.
            self.frame_in_flight.set(true);
            (api.wgpuDevicePoll)(self.device, 0, ptr::null());
            (api.wgpuInstanceProcessEvents)(self.instance);
            check_errors()?;
        }
        if let Some(bank)=&mut self.motion_bank{bank.completed(self.frame_completions.get());}
        if retained_bake {
            self.prerender.as_mut().unwrap().submitted(retained_started.elapsed().as_secs_f64()*1000.);
            self.prerender.as_mut().unwrap().export_atlas(self.instance,self.queue,self.capture.parent().unwrap())?;
            self.info["workstation_shader"]=self.workstation_shader_info();
        }
        if let Some(retained)=&mut self.final_retention{retained.completed(self.frame_completions.get());}
        mark(7);
        self.poll_performance()?;
        mark(8);
        if !self.captured {
            let size = self.stride as usize * self.height as usize;
            let mut state = Box::new(Wait::default());
            let callback = WGPUBufferMapCallbackInfo {
                mode: WGPUCallbackMode_AllowProcessEvents,
                callback: Some(mapped),
                userdata1: (&mut *state as *mut Wait).cast(),
                ..Default::default()
            };
            (api.wgpuBufferMapAsync)(self.read_buffer, WGPUMapMode_Read, 0, size, callback);
            wait(api, self.instance, self.device, state)?;
            let mapped = (api.wgpuBufferGetConstMappedRange)(self.read_buffer, 0, size);
            if mapped.is_null() {
                return Err("WebGPU mapped readback is null".into());
            }
            let bytes = std::slice::from_raw_parts(mapped.cast::<u8>(), size).to_vec();
            (api.wgpuBufferUnmap)(self.read_buffer);
            let center =
                self.height as usize / 2 * self.stride as usize + self.width as usize / 2 * 4;
            if self.validate_triangle_center && bytes[center..center + 3].iter().all(|v| *v < 25) {
                return Err("WebGPU readback did not contain the triangle at its center".into());
            }
            let mut ppm = format!("P6\n{} {}\n255\n", self.width, self.height).into_bytes();
            for row in bytes.chunks_exact(self.stride as usize) {
                for p in row[..self.width as usize * 4].chunks_exact(4) {
                    if self.format == WGPUTextureFormat_BGRA8Unorm {
                        ppm.extend_from_slice(&[p[2], p[1], p[0]]);
                    } else {
                        ppm.extend_from_slice(&p[..3]);
                    }
                }
            }
            if let Some(monitor) = &mut self.monitor_capture {
                *monitor = crate::scene_assets::monitor_image(&ppm)?;
            } else {
                std::fs::write(&self.capture, ppm).map_err(|e| e.to_string())?;
            }
            if self.workstation {
                self.write_workstation_shader_report(false)?;
            }
            self.captured = true;
            self.info["readback_verified"] = json!(self.validate_triangle_center);
        }
        if let Some(retained)=&mut self.final_retention{retained.verify(self.device,self.instance,self.queue,frame.texture,self.read_buffer,self.stride,self.frame_submissions)?;}
        if let Some(v)=&mut self.refresh_validation{if let Some(index)=v.capture_at.take(){
            let name=format!("refresh-frame-{index:04}.ppm");std::fs::copy(&self.capture,self.capture.with_file_name(&name)).map_err(|e|e.to_string())?;
            v.observations.push(json!({"frame":index,"image":name,"dimensions":[self.width,self.height],"prerender":self.prerender.as_ref().map(|p|p.info()),"surface_resize":self.info["surface_resize"],"completed":self.frame_completions.get(),"submitted":self.frame_submissions}));
        }}

        mark(9);
        let mut present_times=None;
        if offscreen {
            self.info["surface_present"]=json!({"skipped":true,"reason":"offscreen_diagnostic"});
        }else{
        self.present_calls+=1;
        let present_begin=timeline_enabled.then(Instant::now);
        let present_status = (api.wgpuSurfacePresent)(self.surface);
        if let Some(start)=present_begin{let stamp=crate::workstation::frame_cadence::timeline_stamp;present_times=Some([stamp(start),stamp(Instant::now())]);}
        if present_status==WGPUStatus_Success {self.successful_presents+=1;}
        self.info["surface_present"] = json!({"code":present_status,"success":present_status==WGPUStatus_Success});
        if present_status != WGPUStatus_Success {
            if let Some(retained)=&mut self.final_retention{retained.invalidate();}if let Some(bank)=&mut self.motion_bank{bank.invalidate();}
            let backend_error=check_errors().err();
            return Err(format!("WebGPU surface presentation failed: status {present_status}{}",
                backend_error.map(|error|format!("; {error}")).unwrap_or_default()));
        }
        }
        mark(10);
        let result = check_errors();
        if let Some(control)=&self.frozen_diagnostic{self.info["diagnostic_frozen_frame"]=control.info();}
        if let Some(retained)=&mut self.final_retention{if result.is_err(){retained.invalidate();}self.info["workstation_shader"]["final_retention"]=retained.info();}
        if let Some(bank)=&self.motion_bank{self.info["workstation_shader"]["motion_bank"]=bank.info();}
        if self.workstation {self.info["workstation_shader"]["frame_overlap"]=self.frame_overlap_info();}
        if let Some(timing)=&self.gpu_timing{if timing.attribution_requested{self.info["gpu_attribution"]=timing.attribution_info();}}
        if self.incremental_mode{if let Some(p)=&self.prerender{self.info["workstation_shader"]["prerender"]=p.info();}}
        if result.is_ok(){if let Some(p)=&mut self.prerender{p.drawn();}}
        mark(11);
        if let Some(times) = checkpoints {
            let recording = Instant::now();
            let wall_ms: [f64; 11] = std::array::from_fn(|i| (times[i+1]-times[i]).as_secs_f64()*1000.);
            self.info["frame_attribution"] = json!({"version":2,"complete":result.is_ok(),
                "stage_names":["initial_progress_poll","surface_acquire","post_acquire_completion","encode",
                    "finish_submit","post_submit_bookkeeping_and_map_request","device_wait_or_progress",
                    "completed_query_poll","conditional_capture","present","tail_error_check"],
                "wall_ms":wall_ms,"total_wall_ms":(times[11]-times[0]).as_secs_f64()*1000.,
                "capture_requested":capture_requested,"capture_performed":capture_requested && self.captured,
                "timing_object_enabled":self.gpu_timing.is_some(),
                "queries_supported":self.gpu_timing.as_ref().is_some_and(|timing|timing.supported),
                "query_slot_recorded":query_recorded,"performance_enabled":super::performance::enabled(),
                "wait_branch":if overlap_this_frame {"bounded_previous_frame"}else if capture_requested || self.workstation {"blocking"}else{"nonblocking"},
                "prior_frame_wait_ms":prior_wait_ms,"prior_frame_waited":waited_for_prior,
                "acquire_before_wait":acquire_first,"submission_index":self.frame_submissions,
                "surface_acquire_call_ms":surface_acquire_ms,"surface_acquire_deferred":late_acquire,
                "direct_cache_draw":direct_cache_draw,"final_color_render_passes":if direct_cache_draw{2}else{1},
                "final_color_gpu_scope":if direct_cache_draw{"Query 6 at retained-pass start through query 7 at presentation-copy end; includes both passes and dependency interval. No surface/cache texture transfer commands."}else{"Single final-color render pass."},
                "completed_frames":self.frame_completions.get(),"present_calls":self.present_calls,"successful_presents":self.successful_presents,
                "previous_input_observation_to_submit_ms":input_to_submit_ms,
                "input_observation_index":self.input_observations.saturating_sub(1),
                "input_latency_scope":"Previous Pump's completed input-state observation to this submission, matching the interactive controller's next-iteration consumption. Includes intervening draw/pacing and CPU preparation. Benchmark targets are replayed separately, so this is a scheduling proxy, not measured hardware event response or scanout.",
                "pending_frames_after_draw":u32::from(self.frame_in_flight.get()),
                "retained_bake":retained_bake,"retained_selected":retained_selected,"incremental_chunk":incremental_chunk,"incremental_bake":self.prerender.as_ref().filter(|p|p.incremental_enabled).map(|p|p.info()["incremental_bake"].clone()),
                "frozen_diagnostic_ready":frozen_ready,"frozen_copy_draw":frozen_copy_draw,
                "final_retention":self.final_retention.as_ref().map(|r|r.info()),
                "diagnostic_offscreen":self.diagnostic_offscreen,"surface_acquire_calls":self.surface_acquisitions,
                "scope":"Adjacent monotonic CPU wall intervals, including blocking waits; not GPU durations. For deferred acquisition, encode includes the separately reported surface_acquire_call_ms; do not sum them. Encode includes query resolve and capture copy. Capture includes map, serialization and existing capture files. Excludes this record construction and implicit Frame release; outer Pump wall time retains them."});
            if timeline_enabled{
                let stamp=crate::workstation::frame_cadence::timeline_stamp;
                self.info["frame_attribution"]["timeline"]=json!({"clock":"shared process-local monotonic milliseconds","checkpoint_ms":times.map(stamp),"acquire_ms":self.info["surface_acquire"]["timeline_ms"],"submit_ms":[submit_begin.map(stamp),submit_end.map(stamp)],"present_ms":present_times});
            }
            self.info["frame_attribution"]["record_build_ms"] = json!(recording.elapsed().as_secs_f64()*1000.);
        }
        result
    }
    pub(super) fn gpu_draw_count(&self)->u64 {self.frame_submissions}
    pub(super) unsafe fn snapshot_performance(&mut self)->Result<(),String>{
        self.wait()?;self.poll_performance()?;
        if let Some(timing)=&mut self.gpu_timing {
            timing.write_legacy_report()?;
            if let Some(log)=&timing.log {log.snapshot();}
        }
        Ok(())
    }
    /// QueueWriteBuffer owns staging allocations until a queue submission.
    /// An unfocused/minimized DAW still publishes live audio/UI data, so a
    /// deferred draw must submit and complete uploads without acquiring a
    /// surface or presenting a frame. DevicePoll alone cannot submit them.
    pub(super) unsafe fn drain_deferred_uploads(&mut self)->Result<(),String>{
        (self.api.wgpuQueueSubmit)(self.queue,0,ptr::null());
        self.wait()?;
        let count=self.info["deferred_upload_drains"].as_u64().unwrap_or(0);
        self.info["deferred_upload_drains"]=json!(count+1);
        Ok(())
    }
    pub unsafe fn wait(&self) -> Result<(), String> {
        if !self.device.is_null() {
            (self.api.wgpuDevicePoll)(self.device, 1, ptr::null());
            (self.api.wgpuInstanceProcessEvents)(self.instance);
        }
        self.frame_in_flight.set(false);
        let result=check_errors();
        if result.is_ok(){self.frame_completions.set(self.frame_submissions);}
        result
    }
    pub(super) unsafe fn poll_performance(&mut self) -> Result<(), String> {
        if let Some(timing) = &mut self.gpu_timing {
            (self.api.wgpuDevicePoll)(self.device, 0, ptr::null());
            (self.api.wgpuInstanceProcessEvents)(self.instance);
            timing.poll()?;
        }
        // A pre-draw window gate must not conceal an already reported backend
        // failure and misclassify it as a recoverable nondrawable benchmark.
        check_errors()
    }
}
struct Frame {
    api: &'static Api,
    texture: WGPUTexture,
    owns_texture:bool,
    view: WGPUTextureView,
    encoder: WGPUCommandEncoder,
    pass: WGPURenderPassEncoder,
    command: WGPUCommandBuffer,
}
impl Drop for Frame {
    fn drop(&mut self) {
        unsafe {
            if !self.pass.is_null() {
                (self.api.wgpuRenderPassEncoderRelease)(self.pass);
            }
            if !self.command.is_null() {
                (self.api.wgpuCommandBufferRelease)(self.command);
            }
            if !self.encoder.is_null() {
                (self.api.wgpuCommandEncoderRelease)(self.encoder);
            }
            if !self.view.is_null() {
                (self.api.wgpuTextureViewRelease)(self.view);
            }
            if self.owns_texture && !self.texture.is_null() {
                (self.api.wgpuTextureRelease)(self.texture);
            }
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            self.opaque_pipeline.take();
            let _ = self.wait();
            if let Some(control)=&self.frozen_diagnostic{
                let output=self.capture.parent().unwrap();
                if let Err(error)=frozen::verify(control,self.device,self.instance,self.queue,self.diagnostic_target,self.read_buffer,self.width,self.height,self.stride,output){
                    let _=std::fs::write(output.join("frozen-image-verification.json"),json!({"complete":false,"error":error}).to_string());
                }
            }
            if let Some(retained)=&mut self.final_retention{retained.completed(self.frame_completions.get());}
            self.final_retention.take();self.motion_bank.take();
            self.frozen_diagnostic.take();
            if self.incremental_mode{if let Some(p)=&mut self.prerender{
                p.drain_stage(self.frame_completions.get());
                let _=std::fs::write(self.capture.with_file_name("incremental-bake-final.json"),json!({"drained":!self.frame_in_flight.get(),"prerender":p.info(),"submitted":self.frame_submissions,"completed":self.frame_completions.get()}).to_string());
            }}
            if let Some(v)=&self.refresh_validation{let _=std::fs::write(self.capture.with_file_name("refresh-validation.json"),json!({"mode":v.mode,"observations":v.observations,"surface_resizes":self.surface_resizes,"final_dimensions":[self.width,self.height],"submitted":self.frame_submissions,"completed":self.frame_completions.get(),"prerender":self.prerender.as_ref().map(|p|p.info())}).to_string());}
            self.prerender.take();
            // The existing shutdown wait completes GPU work; process ready
            // map callbacks before retiring the last profiling slots.
            let _ = self.poll_performance();
            // Capture diagnostics describe the captured image only. Persist the
            // final selection and cumulative bake count after all motion frames,
            // before retiring the response resources; never write per frame.
            if self.workstation && self.info.get("workstation_shader").is_some() {
                if let Err(error) = self.write_workstation_shader_report(true) {
                    eprintln!("workstation final shader diagnostics: {error}");
                }
            }
            self.world.take();
            self.sky_target.take();
            self.vegetation.take();
            self.reflection_vegetation.take();
            self.water.take();
            self.gpu_timing.take();
            self.baked.take();
            self.ruby_response.take();
            self.controls_pipeline.take();
            if !self.reflection_group.is_null() {
                (self.api.wgpuBindGroupRelease)(self.reflection_group);
            }
            if !self.reflection_buffer.is_null() {
                (self.api.wgpuBufferRelease)(self.reflection_buffer);
            }
            for view in [
                self.scene_views[0],
                self.scene_views[1],
                self.reflection_depth_view,
            ] {
                if !view.is_null() {
                    (self.api.wgpuTextureViewRelease)(view);
                }
            }
            for texture in [
                self.scene_colors[0],
                self.scene_colors[1],
                self.reflection_depth,
            ] {
                if !texture.is_null() {
                    (self.api.wgpuTextureRelease)(texture);
                }
            }
            if !self.device.is_null() {
                (self.api.wgpuDevicePoll)(self.device, 1, ptr::null());
            }
            for group in &self.bind_groups {
                (self.api.wgpuBindGroupRelease)(*group);
            }
            if !self.depth_view.is_null() {
                (self.api.wgpuTextureViewRelease)(self.depth_view);
            }
            if !self.depth.is_null() {
                (self.api.wgpuTextureRelease)(self.depth);
            }
            if !self.background_target.is_null(){(self.api.wgpuTextureRelease)(self.background_target);}
            if !self.diagnostic_target.is_null(){
                (self.api.wgpuTextureRelease)(self.diagnostic_target);
            }
            if !self.read_buffer.is_null() {
                (self.api.wgpuBufferRelease)(self.read_buffer);
            }
            if !self.texture_buffer.is_null() {
                (self.api.wgpuBufferRelease)(self.texture_buffer);
            }
            if !self.vertex_buffer.is_null() {
                (self.api.wgpuBufferRelease)(self.vertex_buffer);
            }
            if !self.pipeline.is_null() {
                (self.api.wgpuRenderPipelineRelease)(self.pipeline);
            }
            if !self.shader.is_null() {
                (self.api.wgpuShaderModuleRelease)(self.shader);
            }
            if !self.queue.is_null() {
                (self.api.wgpuQueueRelease)(self.queue);
            }
            if !self.surface.is_null() && !self.device.is_null() {
                (self.api.wgpuSurfaceUnconfigure)(self.surface);
            }
            if !self.device.is_null() {
                (self.api.wgpuDeviceRelease)(self.device);
            }
            if !self.adapter.is_null() {
                (self.api.wgpuAdapterRelease)(self.adapter);
            }
            if !self.surface.is_null() {
                (self.api.wgpuSurfaceRelease)(self.surface);
            }
            if !self.instance.is_null() {
                (self.api.wgpuInstanceRelease)(self.instance);
            }
        }
    }
}

#[cfg(test)] mod surface_latency_tests {
 use super::*;
 #[test] fn surface_latency_request_is_bounded_and_default_supplies_no_extension(){
  assert_eq!(requested_surface_latency(None).unwrap(),None);assert_eq!(requested_surface_latency(Some("default")).unwrap(),None);
  for n in 1..=3 {assert_eq!(requested_surface_latency(Some(&n.to_string())).unwrap(),Some(n));}
  for invalid in ["0","4","-1","256","unbounded",""] {assert!(requested_surface_latency(Some(invalid)).is_err());}
 }
 #[test] #[cfg(all(target_os="macos",target_pointer_width="64"))]
 fn surface_latency_extension_matches_installed_c_probe(){
  // Measured by iteration12/abi.c against the same installed headers.
  assert_eq!(std::mem::size_of::<WGPUSurfaceConfigurationExtras>(),24);
  assert_eq!(std::mem::align_of::<WGPUSurfaceConfigurationExtras>(),8);
  assert_eq!(std::mem::offset_of!(WGPUSurfaceConfigurationExtras,chain),0);
  assert_eq!(std::mem::offset_of!(WGPUSurfaceConfigurationExtras,desiredMaximumFrameLatency),16);
  assert_eq!(std::mem::size_of::<WGPUChainedStruct>(),16);
  assert_eq!(WGPUSType_SurfaceConfigurationExtras,196618);
 }
}

#[cfg(test)] mod offscreen_diagnostic_tests {
 use super::diagnostic_offscreen_requested as requested;
 #[test] fn default_preserves_surface_with_either_completion_policy(){
  for workstation in [false,true]{for overlap in [false,true]{
   assert!(!requested(workstation,None,overlap).unwrap());
   assert!(!requested(workstation,Some("0"),overlap).unwrap());
  }}
 }
 #[test] fn offscreen_requires_workstation_and_serial_completion(){
  assert!(requested(true,Some("1"),false).unwrap());
  assert!(requested(true,Some("1"),true).is_err());
  assert!(requested(false,Some("1"),false).is_err());
 }
 #[test] fn unsupported_values_fail_explicitly(){
  for value in ["", "2", "unbounded", "yes"]{assert!(requested(true,Some(value),false).is_err());}
 }
}

#[cfg(test)]mod retained_attribution_tests {
    use super::*;
    #[test]fn default_query_layout_and_opt_in_bounds_are_distinct(){
        assert_eq!(attribution_query_count(false),8);assert_eq!(attribution_query_count(true),14);
        assert_eq!(ATTRIBUTION_READBACK_BYTES,112);assert!(ATTRIBUTION_READBACK_BYTES<=256);
        assert_eq!(attribution_query_count(true)-1,13);
    }
    #[test]fn capture_enlarges_envelope_but_not_by_summing_overlaps(){
        let ticks=[0,0,0,0,0,0,100,200,80,170];
        assert_eq!(timestamp_interval_summary(&ticks,&[false,false,false,true,true]),Some(TimestampIntervalSummary{envelope_ticks:120,union_ticks:120,overlap_ticks:70,uncovered_ticks:0}));
        assert_eq!(timestamp_interval_summary(&ticks,&[false,false,false,true,false]),Some(TimestampIntervalSummary{envelope_ticks:100,union_ticks:100,overlap_ticks:0,uncovered_ticks:0}));
    }
    #[test]fn inside_spans_do_not_participate_in_frame_envelope(){
        let t=[0,0,0,0,0,0,100,200,110,130,140,190,0,0];
        assert_eq!(checked_inside_intervals(&t,true),Some([20,50]));
        assert_eq!(timestamp_interval_summary(&t[..8],&[false,false,false,true]).unwrap().envelope_ticks,100);
        assert_eq!(checked_inside_intervals(&t,false),None);
    }
    #[test]fn invalid_inside_boundaries_are_reported_instead_of_cost(){
        let t=[0,0,0,0,0,0,100,200,110,130,140,190,0,0];
        for (at,value) in [(8,99),(9,109),(10,129),(11,201)]{let mut bad=t;bad[at]=value;assert_eq!(checked_inside_intervals(&bad,true),None);}
    }
    #[test]fn query_pair_shape_must_match_active_mask(){assert!(timestamp_interval_summary(&[0;8],&[true;5]).is_none());}
}
