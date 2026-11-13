//! Borrowed-device WebGPU water submission. No adapter/device/surface/encoder
//! creation, queue submission, polling, or presentation belongs to this helper.
#![allow(non_snake_case, non_upper_case_globals, unsafe_op_in_unsafe_fn)]
use super::super::wgpu::*;
use super::{BorrowedAttachments, data::DrawFrame};
use std::{
    ffi::{CStr, CString, c_void},
    path::Path,
    ptr,
};
fn label(s: &str) -> WGPUStringView {
    WGPUStringView {
        data: s.as_ptr().cast(),
        length: s.len(),
    }
}
macro_rules! functions{($($name:ident:($($arg:ty),*)$(->$ret:ty)?;)+)=>{
struct Api{$($name:unsafe extern "C" fn($($arg),*)$(->$ret)?,)+}
impl Api{unsafe fn load(resolve:&mut dyn FnMut(&CStr)->*const c_void)->Result<Self,String>{Ok(Self{$($name:{let name=CString::new(stringify!($name)).unwrap();let p=resolve(&name);if p.is_null(){return Err(format!("water WebGPU entrypoint {} unavailable",name.to_string_lossy()));}std::mem::transmute::<*const c_void,unsafe extern "C" fn($($arg),*)$(->$ret)?>(p)},)+})}}
}}
functions! {
 wgpuDeviceCreateShaderModule:(WGPUDevice,*const WGPUShaderModuleDescriptor)->WGPUShaderModule;
 wgpuDeviceCreateRenderPipeline:(WGPUDevice,*const WGPURenderPipelineDescriptor)->WGPURenderPipeline;
 wgpuRenderPipelineGetBindGroupLayout:(WGPURenderPipeline,u32)->WGPUBindGroupLayout;
 wgpuDeviceCreateBindGroup:(WGPUDevice,*const WGPUBindGroupDescriptor)->WGPUBindGroup;
 wgpuDeviceCreateBuffer:(WGPUDevice,*const WGPUBufferDescriptor)->WGPUBuffer;
 wgpuQueueWriteBuffer:(WGPUQueue,WGPUBuffer,u64,*const c_void,usize);
 wgpuDeviceCreateTexture:(WGPUDevice,*const WGPUTextureDescriptor)->WGPUTexture;
 wgpuTextureCreateView:(WGPUTexture,*const WGPUTextureViewDescriptor)->WGPUTextureView;
 wgpuDeviceCreateSampler:(WGPUDevice,*const WGPUSamplerDescriptor)->WGPUSampler;
 wgpuCommandEncoderBeginRenderPass:(WGPUCommandEncoder,*const WGPURenderPassDescriptor)->WGPURenderPassEncoder;
 wgpuRenderPassEncoderSetPipeline:(WGPURenderPassEncoder,WGPURenderPipeline);
 wgpuRenderPassEncoderSetBindGroup:(WGPURenderPassEncoder,u32,WGPUBindGroup,usize,*const u32);
 wgpuRenderPassEncoderSetVertexBuffer:(WGPURenderPassEncoder,u32,WGPUBuffer,u64,u64);
 wgpuRenderPassEncoderSetIndexBuffer:(WGPURenderPassEncoder,WGPUBuffer,WGPUIndexFormat,u64,u64);
 wgpuRenderPassEncoderDrawIndexed:(WGPURenderPassEncoder,u32,u32,u32,i32,u32);
 wgpuRenderPassEncoderDraw:(WGPURenderPassEncoder,u32,u32,u32,u32);
 wgpuRenderPassEncoderEnd:(WGPURenderPassEncoder);
 wgpuShaderModuleRelease:(WGPUShaderModule);wgpuRenderPipelineRelease:(WGPURenderPipeline);wgpuBindGroupLayoutRelease:(WGPUBindGroupLayout);wgpuBindGroupRelease:(WGPUBindGroup);
 wgpuBufferRelease:(WGPUBuffer);wgpuTextureRelease:(WGPUTexture);wgpuTextureViewRelease:(WGPUTextureView);wgpuSamplerRelease:(WGPUSampler);wgpuRenderPassEncoderRelease:(WGPURenderPassEncoder);
}
#[derive(Default)]
struct Draw {
    vertex: WGPUBuffer,
    index: WGPUBuffer,
    uniform: WGPUBuffer,
    vertex_size: u64,
    index_size: u64,
    group: WGPUBindGroup,
}
pub(crate) struct Client {
    api: Api,
    device: WGPUDevice,
    queue: WGPUQueue,
    shader: WGPUShaderModule,
    composite_shader: WGPUShaderModule,
    pipeline: WGPURenderPipeline,
    composite: WGPURenderPipeline,
    layout: WGPUBindGroupLayout,
    composite_layout: WGPUBindGroupLayout,
    sampler: WGPUSampler,
    color: WGPUTexture,
    color_view: WGPUTextureView,
    depth: WGPUTexture,
    depth_view: WGPUTextureView,
    extent: (u32, u32),
    format: WGPUTextureFormat,
    reversed: bool,
    draws: Vec<Draw>,
    composite_group: WGPUBindGroup,
}
impl Client {
    pub unsafe fn new(
        root: &Path,
        device: WGPUDevice,
        queue: WGPUQueue,
        resolve: &mut dyn FnMut(&CStr) -> *const c_void,
    ) -> Result<Self, String> {
        if device.is_null() || queue.is_null() {
            return Err("water WebGPU requires borrowed device and queue".into());
        }
        let api = Api::load(resolve)?;
        let mut s = Self {
            api,
            device,
            queue,
            shader: ptr::null_mut(),
            composite_shader: ptr::null_mut(),
            pipeline: ptr::null_mut(),
            composite: ptr::null_mut(),
            layout: ptr::null_mut(),
            composite_layout: ptr::null_mut(),
            sampler: ptr::null_mut(),
            color: ptr::null_mut(),
            color_view: ptr::null_mut(),
            depth: ptr::null_mut(),
            depth_view: ptr::null_mut(),
            extent: (0, 0),
            format: WGPUTextureFormat_Undefined,
            reversed: false,
            draws: Vec::new(),
            composite_group: ptr::null_mut(),
        };
        s.shader = s.shader(root, "water.wgsl")?;
        s.composite_shader = s.shader(root, "composite.wgsl")?;
        let desc = WGPUSamplerDescriptor {
            label: label("Water scene sampler"),
            addressModeU: WGPUAddressMode_ClampToEdge,
            addressModeV: WGPUAddressMode_ClampToEdge,
            addressModeW: WGPUAddressMode_ClampToEdge,
            magFilter: WGPUFilterMode_Linear,
            minFilter: WGPUFilterMode_Linear,
            mipmapFilter: WGPUMipmapFilterMode_Nearest,
            lodMaxClamp: 32.,
            maxAnisotropy: 1,
            ..Default::default()
        };
        s.sampler = (s.api.wgpuDeviceCreateSampler)(s.device, &desc);
        if s.sampler.is_null() {
            return Err("water WebGPU sampler creation failed".into());
        }
        Ok(s)
    }
    unsafe fn shader(&self, root: &Path, name: &str) -> Result<WGPUShaderModule, String> {
        let source = std::fs::read_to_string(root.join("Procedures/Water/Shaders").join(name))
            .map_err(|e| e.to_string())?;
        let source = WGPUShaderSourceWGSL {
            chain: WGPUChainedStruct {
                next: ptr::null(),
                sType: WGPUSType_ShaderSourceWGSL,
            },
            code: label(&source),
        };
        let desc = WGPUShaderModuleDescriptor {
            nextInChain: &source.chain,
            label: label("Continuous water material"),
        };
        let shader = (self.api.wgpuDeviceCreateShaderModule)(self.device, &desc);
        if shader.is_null() {
            Err("water WebGPU shader creation failed".into())
        } else {
            Ok(shader)
        }
    }
    unsafe fn pipelines(
        &mut self,
        format: WGPUTextureFormat,
        reversed: bool,
    ) -> Result<(), String> {
        if self.format == format && self.reversed == reversed && !self.pipeline.is_null() {
            return Ok(());
        }
        for pipeline in [&mut self.pipeline, &mut self.composite] {
            if !pipeline.is_null() {
                (self.api.wgpuRenderPipelineRelease)(*pipeline);
                *pipeline = ptr::null_mut();
            }
        }
        for layout in [&mut self.layout, &mut self.composite_layout] {
            if !layout.is_null() {
                (self.api.wgpuBindGroupLayoutRelease)(*layout);
                *layout = ptr::null_mut();
            }
        }
        let attributes: Vec<_> = (0..6)
            .map(|i| WGPUVertexAttribute {
                format: WGPUVertexFormat_Float32x4,
                offset: i * 16,
                shaderLocation: i as u32,
            })
            .collect();
        let buffer = WGPUVertexBufferLayout {
            arrayStride: 96,
            stepMode: WGPUVertexStepMode_Vertex,
            attributeCount: 6,
            attributes: attributes.as_ptr(),
        };
        let target = WGPUColorTargetState {
            format: WGPUTextureFormat_RGBA8Unorm,
            writeMask: WGPUColorWriteMask_All,
            ..Default::default()
        };
        let fragment = WGPUFragmentState {
            module: self.shader,
            entryPoint: label("fs_main"),
            targetCount: 1,
            targets: &target,
            ..Default::default()
        };
        let depth = WGPUDepthStencilState {
            format: WGPUTextureFormat_Depth32Float,
            depthWriteEnabled: WGPUOptionalBool_True,
            depthCompare: if reversed {
                WGPUCompareFunction_GreaterEqual
            } else {
                WGPUCompareFunction_LessEqual
            },
            ..Default::default()
        };
        let mut desc = WGPURenderPipelineDescriptor {
            label: label("Continuous water surface"),
            vertex: WGPUVertexState {
                module: self.shader,
                entryPoint: label("vs_main"),
                bufferCount: 1,
                buffers: &buffer,
                ..Default::default()
            },
            primitive: WGPUPrimitiveState {
                topology: WGPUPrimitiveTopology_TriangleList,
                frontFace: WGPUFrontFace_CCW,
                cullMode: WGPUCullMode_None,
                ..Default::default()
            },
            depthStencil: &depth,
            multisample: WGPUMultisampleState {
                count: 1,
                mask: u32::MAX,
                ..Default::default()
            },
            fragment: &fragment,
            ..Default::default()
        };
        self.pipeline = (self.api.wgpuDeviceCreateRenderPipeline)(self.device, &desc);
        if self.pipeline.is_null() {
            return Err("water WebGPU surface pipeline creation failed".into());
        }
        let target = WGPUColorTargetState {
            format,
            writeMask: WGPUColorWriteMask_All,
            ..Default::default()
        };
        let fragment = WGPUFragmentState {
            module: self.composite_shader,
            entryPoint: label("fs_main"),
            targetCount: 1,
            targets: &target,
            ..Default::default()
        };
        desc.label = label("Water scene composite");
        desc.vertex = WGPUVertexState {
            module: self.composite_shader,
            entryPoint: label("vs_main"),
            ..Default::default()
        };
        desc.depthStencil = ptr::null();
        desc.fragment = &fragment;
        self.composite = (self.api.wgpuDeviceCreateRenderPipeline)(self.device, &desc);
        if self.composite.is_null() {
            return Err("water WebGPU composite pipeline creation failed".into());
        }
        self.layout = (self.api.wgpuRenderPipelineGetBindGroupLayout)(self.pipeline, 0);
        self.composite_layout = (self.api.wgpuRenderPipelineGetBindGroupLayout)(self.composite, 0);
        if self.layout.is_null() || self.composite_layout.is_null() {
            return Err("water WebGPU pipeline layouts missing".into());
        }
        self.format = format;
        self.reversed = reversed;
        Ok(())
    }
    unsafe fn texture(
        &self,
        width: u32,
        height: u32,
        format: WGPUTextureFormat,
    ) -> Result<(WGPUTexture, WGPUTextureView), String> {
        let desc = WGPUTextureDescriptor {
            label: label("Private water intermediate"),
            usage: WGPUTextureUsage_RenderAttachment | WGPUTextureUsage_TextureBinding,
            dimension: WGPUTextureDimension_2D,
            size: WGPUExtent3D {
                width,
                height,
                depthOrArrayLayers: 1,
            },
            format,
            mipLevelCount: 1,
            sampleCount: 1,
            ..Default::default()
        };
        let texture = (self.api.wgpuDeviceCreateTexture)(self.device, &desc);
        if texture.is_null() {
            return Err("water WebGPU intermediate allocation failed".into());
        }
        let view = (self.api.wgpuTextureCreateView)(texture, ptr::null());
        if view.is_null() {
            (self.api.wgpuTextureRelease)(texture);
            return Err("water WebGPU intermediate view failed".into());
        }
        Ok((texture, view))
    }
    unsafe fn buffer(&self, size: u64, usage: WGPUBufferUsage) -> Result<WGPUBuffer, String> {
        let desc = WGPUBufferDescriptor {
            label: label("Continuous water draw data"),
            size: size.max(4),
            usage: usage | WGPUBufferUsage_CopyDst,
            ..Default::default()
        };
        let p = (self.api.wgpuDeviceCreateBuffer)(self.device, &desc);
        if p.is_null() {
            Err("water WebGPU buffer allocation failed".into())
        } else {
            Ok(p)
        }
    }
    /// All borrowed color/depth views must be sampleable. Parent submits the
    /// completed encoder before the next encode_batch; queue writes are ordered
    /// after that submission. Each draw has distinct buffers within the batch.
    pub unsafe fn encode_batch(
        &mut self,
        encoder: WGPUCommandEncoder,
        frames: &[DrawFrame],
        a: BorrowedAttachments<WGPUTextureView>,
        color_format: WGPUTextureFormat,
        _depth_format: WGPUTextureFormat,
    ) -> Result<(), String> {
        self.encode_batch_timed(encoder,frames,a,color_format,_depth_format,None)
    }
    pub unsafe fn encode_batch_timed(&mut self,encoder:WGPUCommandEncoder,frames:&[DrawFrame],a:BorrowedAttachments<WGPUTextureView>,color_format:WGPUTextureFormat,_depth_format:WGPUTextureFormat,frame_end:Option<&WGPURenderPassTimestampWrites>)->Result<(),String>{
        let Some(first) = frames.first() else {
            return Ok(());
        };
        if encoder.is_null()
            || a.scene_color.is_null()
            || a.scene_depth.is_null()
            || a.output.is_null()
            || a.output == a.scene_color
        {
            return Err("water WebGPU borrowed attachments/encoder invalid".into());
        }
        for f in frames {
            f.validate()?;
            if f.camera.width != first.camera.width
                || f.camera.height != first.camera.height
                || f.camera.reversed_z != first.camera.reversed_z
            {
                return Err("water WebGPU batch camera convention mismatch".into());
            }
            if f.optics.planar_reflection && a.planar_reflection.is_none() {
                return Err("water WebGPU actual reflected-scene view missing".into());
            }
        }
        self.pipelines(color_format, first.camera.reversed_z)?;
        let (width, height) = (first.camera.width, first.camera.height);
        if self.extent != (width, height) {
            let (color, view) = self.texture(width, height, WGPUTextureFormat_RGBA8Unorm)?;
            let (depth, depth_view) =
                self.texture(width, height, WGPUTextureFormat_Depth32Float)?;
            for v in [&mut self.color_view, &mut self.depth_view] {
                if !v.is_null() {
                    (self.api.wgpuTextureViewRelease)(*v);
                }
            }
            for t in [&mut self.color, &mut self.depth] {
                if !t.is_null() {
                    (self.api.wgpuTextureRelease)(*t);
                }
            }
            self.color = color;
            self.color_view = view;
            self.depth = depth;
            self.depth_view = depth_view;
            self.extent = (width, height);
        }
        while self.draws.len() < frames.len() {
            self.draws.push(Draw::default());
        }
        for (i, frame) in frames.iter().enumerate() {
            let vertex = frame.vertex_bytes_borrowed();
            let index = frame.index_bytes_borrowed();
            let uniform = super::uniform_bytes(frame, -1.)?;
            if self.draws[i].vertex_size < vertex.len() as u64 {
                let p = self.buffer(vertex.len() as u64, WGPUBufferUsage_Vertex)?;
                if !self.draws[i].vertex.is_null() {
                    (self.api.wgpuBufferRelease)(self.draws[i].vertex);
                }
                self.draws[i].vertex = p;
                self.draws[i].vertex_size = vertex.len() as u64;
            }
            if self.draws[i].index_size < index.len() as u64 {
                let p = self.buffer(index.len() as u64, WGPUBufferUsage_Index)?;
                if !self.draws[i].index.is_null() {
                    (self.api.wgpuBufferRelease)(self.draws[i].index);
                }
                self.draws[i].index = p;
                self.draws[i].index_size = index.len() as u64;
            }
            if self.draws[i].uniform.is_null() {
                self.draws[i].uniform = self.buffer(1296, WGPUBufferUsage_Uniform)?;
            }
            let draw = &mut self.draws[i];
            for (buffer, bytes) in [
                (draw.vertex, vertex),
                (draw.index, index),
                (draw.uniform, uniform.as_slice()),
            ] {
                if !bytes.is_empty() {
                    (self.api.wgpuQueueWriteBuffer)(
                        self.queue,
                        buffer,
                        0,
                        bytes.as_ptr().cast(),
                        bytes.len(),
                    );
                }
            }
            if !draw.group.is_null() {
                (self.api.wgpuBindGroupRelease)(draw.group);
                draw.group = ptr::null_mut();
            }
            let entries = [
                WGPUBindGroupEntry {
                    binding: 0,
                    buffer: draw.uniform,
                    size: 1296,
                    ..Default::default()
                },
                WGPUBindGroupEntry {
                    binding: 1,
                    sampler: self.sampler,
                    ..Default::default()
                },
                WGPUBindGroupEntry {
                    binding: 2,
                    textureView: a.scene_color,
                    ..Default::default()
                },
                WGPUBindGroupEntry {
                    binding: 3,
                    textureView: a.planar_reflection.unwrap_or(a.scene_color),
                    ..Default::default()
                },
                WGPUBindGroupEntry {
                    binding: 4,
                    textureView: a.scene_depth,
                    ..Default::default()
                },
            ];
            let desc = WGPUBindGroupDescriptor {
                layout: self.layout,
                entryCount: entries.len(),
                entries: entries.as_ptr(),
                ..Default::default()
            };
            draw.group = (self.api.wgpuDeviceCreateBindGroup)(self.device, &desc);
            if draw.group.is_null() {
                return Err("water WebGPU material bindings failed".into());
            }
        }
        if !self.composite_group.is_null() {
            (self.api.wgpuBindGroupRelease)(self.composite_group);
            self.composite_group = ptr::null_mut();
        }
        let entries = [
            WGPUBindGroupEntry {
                binding: 0,
                sampler: self.sampler,
                ..Default::default()
            },
            WGPUBindGroupEntry {
                binding: 1,
                textureView: a.scene_color,
                ..Default::default()
            },
            WGPUBindGroupEntry {
                binding: 2,
                textureView: self.color_view,
                ..Default::default()
            },
        ];
        let desc = WGPUBindGroupDescriptor {
            layout: self.composite_layout,
            entryCount: entries.len(),
            entries: entries.as_ptr(),
            ..Default::default()
        };
        self.composite_group = (self.api.wgpuDeviceCreateBindGroup)(self.device, &desc);
        if self.composite_group.is_null() {
            return Err("water WebGPU composite bindings failed".into());
        }
        let color = WGPURenderPassColorAttachment {
            view: self.color_view,
            depthSlice: u32::MAX,
            loadOp: WGPULoadOp_Clear,
            storeOp: WGPUStoreOp_Store,
            clearValue: WGPUColor {
                r: 0.,
                g: 0.,
                b: 0.,
                a: 0.,
            },
            ..Default::default()
        };
        let depth = WGPURenderPassDepthStencilAttachment {
            view: self.depth_view,
            depthLoadOp: WGPULoadOp_Clear,
            depthStoreOp: WGPUStoreOp_Store,
            depthClearValue: if self.reversed { 0. } else { 1. },
            stencilReadOnly: 1,
            ..Default::default()
        };
        let begin_timestamps=frame_end.map(|t|WGPURenderPassTimestampWrites{querySet:t.querySet,beginningOfPassWriteIndex:t.beginningOfPassWriteIndex,endOfPassWriteIndex:u32::MAX});
        let desc = WGPURenderPassDescriptor {
            colorAttachmentCount: 1,
            colorAttachments: &color,
            depthStencilAttachment: &depth,
            timestampWrites:begin_timestamps.as_ref().map_or(ptr::null(),|t|t),
            ..Default::default()
        };
        let pass = (self.api.wgpuCommandEncoderBeginRenderPass)(encoder, &desc);
        if pass.is_null() {
            return Err("water WebGPU surface pass failed".into());
        }
        (self.api.wgpuRenderPassEncoderSetPipeline)(pass, self.pipeline);
        for (i, frame) in frames.iter().enumerate() {
            if frame.indices.is_empty() {
                continue;
            }
            let d = &self.draws[i];
            (self.api.wgpuRenderPassEncoderSetBindGroup)(pass, 0, d.group, 0, ptr::null());
            (self.api.wgpuRenderPassEncoderSetVertexBuffer)(pass, 0, d.vertex, 0, d.vertex_size);
            (self.api.wgpuRenderPassEncoderSetIndexBuffer)(
                pass,
                d.index,
                WGPUIndexFormat_Uint32,
                0,
                d.index_size,
            );
            (self.api.wgpuRenderPassEncoderDrawIndexed)(
                pass,
                frame.indices.len() as u32,
                1,
                0,
                0,
                0,
            );
        }
        (self.api.wgpuRenderPassEncoderEnd)(pass);
        (self.api.wgpuRenderPassEncoderRelease)(pass);
        let color = WGPURenderPassColorAttachment {
            view: a.output,
            depthSlice: u32::MAX,
            loadOp: WGPULoadOp_Clear,
            storeOp: WGPUStoreOp_Store,
            clearValue: WGPUColor {
                r: 0.,
                g: 0.,
                b: 0.,
                a: 1.,
            },
            ..Default::default()
        };
        let end_timestamps=frame_end.map(|t|WGPURenderPassTimestampWrites{querySet:t.querySet,beginningOfPassWriteIndex:u32::MAX,endOfPassWriteIndex:t.endOfPassWriteIndex});
        let desc = WGPURenderPassDescriptor {
            colorAttachmentCount: 1,
            colorAttachments: &color,
            timestampWrites:end_timestamps.as_ref().map_or(ptr::null(),|t|t),
            ..Default::default()
        };
        let pass = (self.api.wgpuCommandEncoderBeginRenderPass)(encoder, &desc);
        if pass.is_null() {
            return Err("water WebGPU composite pass failed".into());
        }
        (self.api.wgpuRenderPassEncoderSetPipeline)(pass, self.composite);
        (self.api.wgpuRenderPassEncoderSetBindGroup)(pass, 0, self.composite_group, 0, ptr::null());
        (self.api.wgpuRenderPassEncoderDraw)(pass, 3, 1, 0, 0);
        (self.api.wgpuRenderPassEncoderEnd)(pass);
        (self.api.wgpuRenderPassEncoderRelease)(pass);
        Ok(())
    }
    pub unsafe fn encode(
        &mut self,
        encoder: WGPUCommandEncoder,
        frame: &DrawFrame,
        a: BorrowedAttachments<WGPUTextureView>,
        color_format: WGPUTextureFormat,
        depth_format: WGPUTextureFormat,
    ) -> Result<(), String> {
        self.encode_batch(
            encoder,
            std::slice::from_ref(frame),
            a,
            color_format,
            depth_format,
        )
    }
    pub unsafe fn release(&mut self) {
        for d in self.draws.drain(..) {
            if !d.group.is_null() {
                (self.api.wgpuBindGroupRelease)(d.group);
            }
            for b in [d.vertex, d.index, d.uniform] {
                if !b.is_null() {
                    (self.api.wgpuBufferRelease)(b);
                }
            }
        }
        if !self.composite_group.is_null() {
            (self.api.wgpuBindGroupRelease)(self.composite_group);
            self.composite_group = ptr::null_mut();
        }
        for p in [&mut self.pipeline, &mut self.composite] {
            if !p.is_null() {
                (self.api.wgpuRenderPipelineRelease)(*p);
                *p = ptr::null_mut();
            }
        }
        for p in [&mut self.layout, &mut self.composite_layout] {
            if !p.is_null() {
                (self.api.wgpuBindGroupLayoutRelease)(*p);
                *p = ptr::null_mut();
            }
        }
        for p in [&mut self.shader, &mut self.composite_shader] {
            if !p.is_null() {
                (self.api.wgpuShaderModuleRelease)(*p);
                *p = ptr::null_mut();
            }
        }
        for p in [&mut self.color_view, &mut self.depth_view] {
            if !p.is_null() {
                (self.api.wgpuTextureViewRelease)(*p);
                *p = ptr::null_mut();
            }
        }
        for p in [&mut self.color, &mut self.depth] {
            if !p.is_null() {
                (self.api.wgpuTextureRelease)(*p);
                *p = ptr::null_mut();
            }
        }
        if !self.sampler.is_null() {
            (self.api.wgpuSamplerRelease)(self.sampler);
            self.sampler = ptr::null_mut();
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            self.release();
        }
    }
}
