#![allow(non_upper_case_globals)]
//! Native WebGPU client with a surface, pipeline, and verified readback.
use super::{wgpu::*, window::Window};
use serde_json::{Value, json};
use std::{
    ffi::{CStr, c_void},
    path::{Path, PathBuf},
    ptr,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
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
#[derive(Default)]
struct Wait {
    done: bool,
    status: u32,
    handle: *mut c_void,
    abandoned: bool,
}
unsafe extern "C" fn adapter_ready(
    status: WGPURequestAdapterStatus,
    handle: WGPUAdapter,
    _message: WGPUStringView,
    data: *mut c_void,
    _: *mut c_void,
) {
    let state = &mut *data.cast::<Wait>();
    if state.abandoned {
        if !handle.is_null() {
            if let Ok(api) = api() {
                (api.wgpuAdapterRelease)(handle);
            }
        }
        return;
    }
    state.status = status;
    state.handle = handle.cast();
    state.done = true;
}
unsafe extern "C" fn device_ready(
    status: WGPURequestDeviceStatus,
    handle: WGPUDevice,
    _message: WGPUStringView,
    data: *mut c_void,
    _: *mut c_void,
) {
    let state = &mut *data.cast::<Wait>();
    if state.abandoned {
        if !handle.is_null() {
            if let Ok(api) = api() {
                (api.wgpuDeviceRelease)(handle);
            }
        }
        return;
    }
    state.status = status;
    state.handle = handle.cast();
    state.done = true;
}
unsafe extern "C" fn mapped(
    status: WGPUMapAsyncStatus,
    _message: WGPUStringView,
    data: *mut c_void,
    _: *mut c_void,
) {
    let state = &mut *data.cast::<Wait>();
    state.status = status;
    state.done = true;
}
unsafe fn wait(
    api: &Api,
    instance: WGPUInstance,
    device: WGPUDevice,
    mut state: Box<Wait>,
) -> Result<*mut c_void, String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !state.done {
        if !device.is_null() {
            (api.wgpuDevicePoll)(device, 1, ptr::null());
        }
        (api.wgpuInstanceProcessEvents)(instance);
        if Instant::now() > deadline {
            // A pending callback may still hold userdata. Keep it valid on the
            // error path; API code remains mapped for the process lifetime.
            state.abandoned = true;
            Box::leak(state);
            return Err("WebGPU callback timed out; callback storage retained safely".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    if state.status != 1 {
        return Err(format!(
            "WebGPU asynchronous operation failed: {}",
            state.status
        ));
    }
    Ok(state.handle)
}
pub(super) struct Client {
    pub window: Window,
    api: &'static Api,
    instance: WGPUInstance,
    surface: WGPUSurface,
    adapter: WGPUAdapter,
    device: WGPUDevice,
    queue: WGPUQueue,
    shader: WGPUShaderModule,
    pipeline: WGPURenderPipeline,
    uniform: WGPUBuffer,
    bind_group: WGPUBindGroup,
    read_buffer: WGPUBuffer,
    width: u32,
    height: u32,
    stride: u32,
    format: WGPUTextureFormat,
    captured: bool,
    count: u32,
    pub(super) clear: [f32; 4],
    capture: PathBuf,
    // Keep Objective-C runtime and QuartzCore loaded while the surface lives.
    _native_libraries: Vec<libloading::Library>,
    pub info: Value,
}
impl Client {
    pub unsafe fn new(root: &Path, data: &[f32; 24], output: &Path) -> Result<Self, String> {
        if !cfg!(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "ios"
        )) {
            return Err("WebGPU native window integration requires macOS, Windows, or iOS".into());
        }
        check_errors()?;
        let window = Window::new(false, c"Wormwood — WebGPU")?;
        let (w, h) = window.dimensions()?;
        let api = api()?;
        let mut c = Self {
            window,
            api,
            instance: ptr::null_mut(),
            surface: ptr::null_mut(),
            adapter: ptr::null_mut(),
            device: ptr::null_mut(),
            queue: ptr::null_mut(),
            shader: ptr::null_mut(),
            pipeline: ptr::null_mut(),
            uniform: ptr::null_mut(),
            bind_group: ptr::null_mut(),
            read_buffer: ptr::null_mut(),
            width: w,
            height: h,
            stride: (w * 4).div_ceil(256) * 256,
            format: 0,
            captured: false,
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
        let mut state = Box::new(Wait::default());
        let callback = WGPURequestDeviceCallbackInfo {
            mode: WGPUCallbackMode_AllowProcessEvents,
            callback: Some(device_ready),
            userdata1: (&mut *state as *mut Wait).cast(),
            ..Default::default()
        };
        let device = WGPUDeviceDescriptor {
            label: view("Wormwood device"),
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
        let usages = caps.usages;
        (api.wgpuSurfaceCapabilitiesFreeMembers)(caps);
        c.format = format.ok_or("WebGPU surface has no supported 8-bit format")?;
        let usage = WGPUTextureUsage_RenderAttachment | WGPUTextureUsage_CopySrc;
        if usages & usage != usage {
            return Err("WebGPU surface does not support diagnostic readback".into());
        }
        let config = WGPUSurfaceConfiguration {
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
        let source = std::fs::read_to_string(root.join("Procedures/Shaders/triangle.wgsl"))
            .map_err(|e| e.to_string())?;
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
        let target = WGPUColorTargetState {
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
            ..Default::default()
        };
        c.pipeline = (api.wgpuDeviceCreateRenderPipeline)(c.device, &pipeline);
        if c.pipeline.is_null() {
            return Err("WebGPU pipeline creation failed".into());
        }
        let uniform = WGPUBufferDescriptor {
            label: view("Entity geometry and color"),
            size: 12288,
            usage: WGPUBufferUsage_Uniform | WGPUBufferUsage_CopyDst,
            ..Default::default()
        };
        c.uniform = (api.wgpuDeviceCreateBuffer)(c.device, &uniform);
        if c.uniform.is_null() {
            return Err("WebGPU uniform allocation failed".into());
        }
        let mut expanded = [0f32; 3072];
        expanded[..12].copy_from_slice(&data[..12]);
        expanded[1536..1548].copy_from_slice(&data[12..]);
        (api.wgpuQueueWriteBuffer)(c.queue, c.uniform, 0, expanded.as_ptr().cast(), 12288);
        let layout = (api.wgpuRenderPipelineGetBindGroupLayout)(c.pipeline, 0);
        if layout.is_null() {
            return Err("WebGPU bind-group layout failed".into());
        }
        let entry = WGPUBindGroupEntry {
            binding: 0,
            buffer: c.uniform,
            size: 12288,
            ..Default::default()
        };
        let group = WGPUBindGroupDescriptor {
            label: view("Entity appearance"),
            layout,
            entryCount: 1,
            entries: &entry,
            ..Default::default()
        };
        c.bind_group = (api.wgpuDeviceCreateBindGroup)(c.device, &group);
        (api.wgpuBindGroupLayoutRelease)(layout);
        if c.bind_group.is_null() {
            return Err("WebGPU bind group failed".into());
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
        c.info = json!({"api":"WebGPU","implementation":"wgpu-native","native_backend":info.backendType,"device":string(info.device),"description":string(info.description),"width":w,"height":h,"readback_verified":false});
        (api.wgpuAdapterInfoFreeMembers)(info);
        c.wait()?;
        Ok(c)
    }
    pub unsafe fn set_batch(&mut self, triangles: &[[f32; 24]]) -> Result<(), String> {
        if triangles.len() > 128 {
            return Err("initial mesh batch exceeds 128 triangles".into());
        }
        let mut data = [0f32; 3072];
        for (i, t) in triangles.iter().enumerate() {
            data[i * 12..i * 12 + 12].copy_from_slice(&t[..12]);
            data[1536 + i * 12..1548 + i * 12].copy_from_slice(&t[12..]);
        }
        (self.api.wgpuQueueWriteBuffer)(self.queue, self.uniform, 0, data.as_ptr().cast(), 12288);
        self.count = (triangles.len() * 3) as u32;
        Ok(())
    }
    pub unsafe fn draw(&mut self) -> Result<(), String> {
        let api = self.api;
        let mut surface = WGPUSurfaceTexture::default();
        (api.wgpuSurfaceGetCurrentTexture)(self.surface, &mut surface);
        if surface.texture.is_null() {
            return Err(format!("WebGPU surface acquire failed: {}", surface.status));
        }
        let mut frame = Frame {
            api,
            texture: surface.texture,
            view: ptr::null_mut(),
            encoder: ptr::null_mut(),
            pass: ptr::null_mut(),
            command: ptr::null_mut(),
        };
        frame.view = (api.wgpuTextureCreateView)(frame.texture, ptr::null());
        if frame.view.is_null() {
            return Err("WebGPU texture view failed".into());
        }
        frame.encoder = (api.wgpuDeviceCreateCommandEncoder)(self.device, ptr::null());
        if frame.encoder.is_null() {
            return Err("WebGPU command encoder failed".into());
        }
        let attachment = WGPURenderPassColorAttachment {
            view: frame.view,
            depthSlice: u32::MAX,
            loadOp: WGPULoadOp_Clear,
            storeOp: WGPUStoreOp_Store,
            clearValue: WGPUColor {
                r: self.clear[0] as f64,
                g: self.clear[1] as f64,
                b: self.clear[2] as f64,
                a: 1.0,
            },
            ..Default::default()
        };
        let pass = WGPURenderPassDescriptor {
            colorAttachmentCount: 1,
            colorAttachments: &attachment,
            ..Default::default()
        };
        frame.pass = (api.wgpuCommandEncoderBeginRenderPass)(frame.encoder, &pass);
        if frame.pass.is_null() {
            return Err("WebGPU render pass failed".into());
        }
        (api.wgpuRenderPassEncoderSetPipeline)(frame.pass, self.pipeline);
        (api.wgpuRenderPassEncoderSetBindGroup)(frame.pass, 0, self.bind_group, 0, ptr::null());
        (api.wgpuRenderPassEncoderDraw)(frame.pass, self.count, 1, 0, 0);
        (api.wgpuRenderPassEncoderEnd)(frame.pass);
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
        frame.command = (api.wgpuCommandEncoderFinish)(frame.encoder, ptr::null());
        if frame.command.is_null() {
            return Err("WebGPU command finish failed".into());
        }
        (api.wgpuQueueSubmit)(self.queue, 1, &frame.command);
        self.wait()?;
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
            if bytes[center..center + 3].iter().all(|v| *v < 25) {
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
            std::fs::write(&self.capture, ppm).map_err(|e| e.to_string())?;
            self.captured = true;
            self.info["readback_verified"] = json!(true);
        }
        if (api.wgpuSurfacePresent)(self.surface) != WGPUStatus_Success {
            return Err("WebGPU surface presentation failed".into());
        }
        check_errors()
    }
    pub unsafe fn wait(&self) -> Result<(), String> {
        if !self.device.is_null() {
            (self.api.wgpuDevicePoll)(self.device, 1, ptr::null());
            (self.api.wgpuInstanceProcessEvents)(self.instance);
        }
        check_errors()
    }
}
struct Frame {
    api: &'static Api,
    texture: WGPUTexture,
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
            if !self.texture.is_null() {
                (self.api.wgpuTextureRelease)(self.texture);
            }
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            if !self.device.is_null() {
                (self.api.wgpuDevicePoll)(self.device, 1, ptr::null());
            }
            if !self.bind_group.is_null() {
                (self.api.wgpuBindGroupRelease)(self.bind_group);
            }
            if !self.read_buffer.is_null() {
                (self.api.wgpuBufferRelease)(self.read_buffer);
            }
            if !self.uniform.is_null() {
                (self.api.wgpuBufferRelease)(self.uniform);
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
