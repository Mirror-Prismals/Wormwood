//! Borrowed-device indexed plants and silhouette-matched sunlight depth.
#![allow(non_snake_case, non_upper_case_globals, unsafe_op_in_unsafe_fn)]
use super::super::wgpu::*;
use crate::vegetation::packet::Packet;
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
impl Api{unsafe fn load(resolve:&mut dyn FnMut(&CStr)->*const c_void)->Result<Self,String>{Ok(Self{$($name:{let name=CString::new(stringify!($name)).unwrap();let p=resolve(&name);if p.is_null(){return Err(format!("vegetation WebGPU entrypoint {} unavailable",name.to_string_lossy()));}std::mem::transmute::<*const c_void,unsafe extern "C" fn($($arg),*)$(->$ret)?>(p)},)+})}}
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

pub(crate) struct Client {
    api: Api,
    device: WGPUDevice,
    queue: WGPUQueue,
    shader: WGPUShaderModule,
    color: WGPURenderPipeline,
    shadow: WGPURenderPipeline,
    vertex: WGPUBuffer,
    index: WGPUBuffer,
    uniform: WGPUBuffer,
    impostors: WGPUBuffer,
    depth: WGPUTexture,
    depth_view: WGPUTextureView,
    color_group: WGPUBindGroup,
    shadow_group: WGPUBindGroup,
    count: u32,
}
impl Client {
    pub unsafe fn new(
        root: &Path,
        device: WGPUDevice,
        queue: WGPUQueue,
        materials: WGPUBuffer,
        format: WGPUTextureFormat,
        resolve: &mut dyn FnMut(&CStr) -> *const c_void,
    ) -> Result<Self, String> {
        let api = Api::load(resolve)?;
        let mut s = Self {
            api,
            device,
            queue,
            shader: ptr::null_mut(),
            color: ptr::null_mut(),
            shadow: ptr::null_mut(),
            vertex: ptr::null_mut(),
            index: ptr::null_mut(),
            uniform: ptr::null_mut(),
            impostors: ptr::null_mut(),
            depth: ptr::null_mut(),
            depth_view: ptr::null_mut(),
            color_group: ptr::null_mut(),
            shadow_group: ptr::null_mut(),
            count: 0,
        };
        let code = std::fs::read_to_string(root.join("Procedures/Shaders/vegetation.wgsl"))
            .map_err(|e| e.to_string())?;
        let source = WGPUShaderSourceWGSL {
            chain: WGPUChainedStruct {
                next: ptr::null(),
                sType: WGPUSType_ShaderSourceWGSL,
            },
            code: label(&code),
        };
        s.shader = (s.api.wgpuDeviceCreateShaderModule)(
            device,
            &WGPUShaderModuleDescriptor {
                nextInChain: &source.chain,
                label: label("Vegetation material"),
            },
        );
        if s.shader.is_null() {
            return Err("vegetation shader unavailable".into());
        }
        let attributes = [
            (WGPUVertexFormat_Float32x3, 0),
            (WGPUVertexFormat_Float32x3, 12),
            (WGPUVertexFormat_Float32x2, 24),
            (WGPUVertexFormat_Float32x3, 32),
            (WGPUVertexFormat_Float32x4, 44),
            (WGPUVertexFormat_Float32, 60),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (format, offset))| WGPUVertexAttribute {
            format,
            offset,
            shaderLocation: i as u32,
            ..Default::default()
        })
        .collect::<Vec<_>>();
        let vertex = WGPUVertexBufferLayout {
            arrayStride: 64,
            stepMode: WGPUVertexStepMode_Vertex,
            attributeCount: attributes.len(),
            attributes: attributes.as_ptr(),
        };
        let target = WGPUColorTargetState {
            format,
            writeMask: WGPUColorWriteMask_All,
            ..Default::default()
        };
        let depth = WGPUDepthStencilState {
            format: WGPUTextureFormat_Depth32Float,
            depthWriteEnabled: WGPUOptionalBool_True,
            depthCompare: WGPUCompareFunction_Less,
            stencilFront: WGPUStencilFaceState {
                compare: WGPUCompareFunction_Always,
                ..Default::default()
            },
            stencilBack: WGPUStencilFaceState {
                compare: WGPUCompareFunction_Always,
                ..Default::default()
            },
            ..Default::default()
        };
        for shadow in [false, true] {
            let fragment = WGPUFragmentState {
                module: s.shader,
                entryPoint: label(if shadow { "fs_shadow" } else { "fs_main" }),
                targetCount: if shadow { 0 } else { 1 },
                targets: if shadow { ptr::null() } else { &target },
                ..Default::default()
            };
            let desc = WGPURenderPipelineDescriptor {
                label: label("Vegetation indexed pass"),
                vertex: WGPUVertexState {
                    module: s.shader,
                    entryPoint: label(if shadow { "vs_shadow" } else { "vs_main" }),
                    bufferCount: 1,
                    buffers: &vertex,
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
            let p = (s.api.wgpuDeviceCreateRenderPipeline)(device, &desc);
            if p.is_null() {
                return Err("vegetation pipeline failed".into());
            }
            if shadow {
                s.shadow = p;
            } else {
                s.color = p;
            }
        }
        for (slot, size, usage) in [
            (&mut s.vertex, 98304 * 64, WGPUBufferUsage_Vertex),
            (&mut s.index, 98304 * 4, WGPUBufferUsage_Index),
            (&mut s.uniform, 192, WGPUBufferUsage_Uniform),
        ] {
            *slot = (s.api.wgpuDeviceCreateBuffer)(
                device,
                &WGPUBufferDescriptor {
                    size,
                    usage: usage | WGPUBufferUsage_CopyDst,
                    ..Default::default()
                },
            );
            if (*slot).is_null() {
                return Err("vegetation buffer allocation failed".into());
            }
        }
        s.depth = (s.api.wgpuDeviceCreateTexture)(
            device,
            &WGPUTextureDescriptor {
                usage: WGPUTextureUsage_RenderAttachment | WGPUTextureUsage_TextureBinding,
                dimension: WGPUTextureDimension_2D,
                size: WGPUExtent3D {
                    width: 2048,
                    height: 2048,
                    depthOrArrayLayers: 1,
                },
                format: WGPUTextureFormat_Depth32Float,
                mipLevelCount: 1,
                sampleCount: 1,
                ..Default::default()
            },
        );
        if s.depth.is_null() {
            return Err("vegetation sunlight texture failed".into());
        }
        s.depth_view = (s.api.wgpuTextureCreateView)(s.depth, ptr::null());
        if s.depth_view.is_null() {
            return Err("vegetation sunlight view failed".into());
        }
        let tree_bytes=&crate::vegetation::impostor::BYTES[8..];
        s.impostors=(s.api.wgpuDeviceCreateBuffer)(device,&WGPUBufferDescriptor{label:label("Tree silhouette normals"),size:tree_bytes.len() as u64,usage:WGPUBufferUsage_Storage|WGPUBufferUsage_CopyDst,..Default::default()});
        if s.impostors.is_null(){return Err("tree silhouette storage failed".into());}
        (s.api.wgpuQueueWriteBuffer)(queue,s.impostors,0,tree_bytes.as_ptr().cast(),tree_bytes.len());
        for shadow in [false, true] {
            let layout = (s.api.wgpuRenderPipelineGetBindGroupLayout)(
                if shadow { s.shadow } else { s.color },
                0,
            );
            let entries = [
                WGPUBindGroupEntry {
                    binding: 0,
                    buffer: s.uniform,
                    size: 192,
                    ..Default::default()
                },
                WGPUBindGroupEntry {
                    binding: 1,
                    textureView: s.depth_view,
                    ..Default::default()
                },
                WGPUBindGroupEntry {
                    binding: 2,
                    buffer: materials,
                    size: crate::scene_assets::SCENE_BYTES as u64,
                    ..Default::default()
                },
            ];
            let mut entries=entries.to_vec();
            entries.push(WGPUBindGroupEntry{binding:3,buffer:s.impostors,size:tree_bytes.len()as u64,..Default::default()});
            if shadow {entries.retain(|e|e.binding==0||e.binding==3);}
            let group = (s.api.wgpuDeviceCreateBindGroup)(
                device,
                &WGPUBindGroupDescriptor {
                    layout,
                    entryCount: entries.len(),
                    entries: entries.as_ptr(),
                    ..Default::default()
                },
            );
            (s.api.wgpuBindGroupLayoutRelease)(layout);
            if group.is_null() {
                return Err("vegetation bind group failed".into());
            }
            if shadow {
                s.shadow_group = group;
            } else {
                s.color_group = group;
            }
        }
        Ok(s)
    }
    pub unsafe fn upload(&mut self, packet: &Packet) -> Result<(), String> {
        let uniform = packet.scene.bytes()?;
        let vertices = packet.vertex_data();
        let indices = packet.index_data();
        for (buffer, bytes) in [
            (self.uniform, uniform.as_slice()),
            (self.vertex, vertices),
            (self.index, indices),
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
        self.count = packet.indices.len() as u32;
        Ok(())
    }
    pub unsafe fn draw(
        &self,
        encoder: WGPUCommandEncoder,
        output: WGPUTextureView,
        depth: WGPUTextureView,
    ) -> Result<(), String> {
        self.draw_timed(encoder,output,depth,None)
    }
    pub unsafe fn draw_timed(&self,encoder:WGPUCommandEncoder,output:WGPUTextureView,depth:WGPUTextureView,frame_end:Option<&WGPURenderPassTimestampWrites>)->Result<(),String>{
        for shadow in [true, false] {
            let timestamps=frame_end.map(|t|WGPURenderPassTimestampWrites{querySet:t.querySet,
                beginningOfPassWriteIndex:if shadow{t.beginningOfPassWriteIndex}else{u32::MAX},
                endOfPassWriteIndex:if shadow{u32::MAX}else{t.endOfPassWriteIndex}});
            let target = WGPURenderPassColorAttachment {
                view: output,
                depthSlice: u32::MAX,
                loadOp: WGPULoadOp_Load,
                storeOp: WGPUStoreOp_Store,
                ..Default::default()
            };
            let ds = WGPURenderPassDepthStencilAttachment {
                view: if shadow { self.depth_view } else { depth },
                depthLoadOp: if shadow {
                    WGPULoadOp_Clear
                } else {
                    WGPULoadOp_Load
                },
                depthStoreOp: WGPUStoreOp_Store,
                depthClearValue: 1.,
                ..Default::default()
            };
            let pass = (self.api.wgpuCommandEncoderBeginRenderPass)(
                encoder,
                &WGPURenderPassDescriptor {
                    colorAttachmentCount: if shadow { 0 } else { 1 },
                    colorAttachments: if shadow { ptr::null() } else { &target },
                    depthStencilAttachment: &ds,
                    timestampWrites:timestamps.as_ref().map_or(ptr::null(),|t|t),
                    ..Default::default()
                },
            );
            if pass.is_null() {
                return Err("vegetation render pass failed".into());
            }
            (self.api.wgpuRenderPassEncoderSetPipeline)(
                pass,
                if shadow { self.shadow } else { self.color },
            );
            (self.api.wgpuRenderPassEncoderSetBindGroup)(
                pass,
                0,
                if shadow {
                    self.shadow_group
                } else {
                    self.color_group
                },
                0,
                ptr::null(),
            );
            (self.api.wgpuRenderPassEncoderSetVertexBuffer)(pass, 0, self.vertex, 0, 98304 * 64);
            (self.api.wgpuRenderPassEncoderSetIndexBuffer)(
                pass,
                self.index,
                WGPUIndexFormat_Uint32,
                0,
                98304 * 4,
            );
            (self.api.wgpuRenderPassEncoderDrawIndexed)(pass, self.count, 1, 0, 0, 0);
            (self.api.wgpuRenderPassEncoderEnd)(pass);
            (self.api.wgpuRenderPassEncoderRelease)(pass);
        }
        Ok(())
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            for group in [self.color_group, self.shadow_group] {
                if !group.is_null() {
                    (self.api.wgpuBindGroupRelease)(group);
                }
            }
            for buffer in [self.vertex, self.index, self.uniform, self.impostors] {
                if !buffer.is_null() {
                    (self.api.wgpuBufferRelease)(buffer);
                }
            }
            if !self.depth_view.is_null() {
                (self.api.wgpuTextureViewRelease)(self.depth_view);
            }
            if !self.depth.is_null() {
                (self.api.wgpuTextureRelease)(self.depth);
            }
            for pipeline in [self.color, self.shadow] {
                if !pipeline.is_null() {
                    (self.api.wgpuRenderPipelineRelease)(pipeline);
                }
            }
            if !self.shader.is_null() {
                (self.api.wgpuShaderModuleRelease)(self.shader);
            }
        }
    }
}
