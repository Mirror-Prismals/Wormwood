//! Render the existing volumetric sky at the selected resolution, then sample
//! it behind the full-resolution terrain. High uses the original scene pass.
use super::*;

pub(super) fn divisor() -> u32 {
    #[cfg(target_os = "macos")]
    unsafe {
        unsafe extern "C" {
            fn ww_cloud_divisor() -> u32;
        }
        return ww_cloud_divisor();
    }
    #[cfg(not(target_os = "macos"))]
    {
        1
    }
}

const UPSAMPLE: &str = r#"
@group(0) @binding(0) var sky: texture_2d<f32>;
struct Out { @builtin(position) p:vec4<f32>, @location(0) uv:vec2<f32> }
@vertex fn vs(@builtin(vertex_index) i:u32)->Out {
    let uv=vec2<f32>(f32((i<<1u)&2u),f32(i&2u));
    return Out(vec4<f32>(uv.x*2.-1.,1.-uv.y*2.,0.,1.),uv);
}
@fragment fn fs(i:Out)->@location(0) vec4<f32> {
    let size=vec2<i32>(textureDimensions(sky));
    let p=i.uv*vec2<f32>(size)-0.5;
    let base=vec2<i32>(floor(p));
    let lo=clamp(base,vec2<i32>(0),size-1);
    let hi=clamp(base+1,vec2<i32>(0),size-1);
    let f=fract(p);
    return mix(mix(textureLoad(sky,lo,0),textureLoad(sky,vec2<i32>(hi.x,lo.y),0),f.x),
               mix(textureLoad(sky,vec2<i32>(lo.x,hi.y),0),textureLoad(sky,hi,0),f.x),f.y);
}
"#;
pub(super) struct Sky {
    api: &'static Api,
    pub size: (u32, u32),
    textures: [WGPUTexture; 2],
    views: [WGPUTextureView; 2],
    pipeline: WGPURenderPipeline,
    group: WGPUBindGroup,
}
impl Sky {
    pub unsafe fn new(c: &Client, size: (u32, u32)) -> Result<Self, String> {
        let api = c.api;
        let mut s = Self {
            api,
            size,
            textures: [ptr::null_mut(); 2],
            views: [ptr::null_mut(); 2],
            pipeline: ptr::null_mut(),
            group: ptr::null_mut(),
        };
        for i in 0..2 {
            s.textures[i] = (api.wgpuDeviceCreateTexture)(
                c.device,
                &WGPUTextureDescriptor {
                    label: view("Cloud resolution target"),
                    dimension: WGPUTextureDimension_2D,
                    size: WGPUExtent3D {
                        width: size.0,
                        height: size.1,
                        depthOrArrayLayers: 1,
                    },
                    format: if i == 0 {
                        c.format
                    } else {
                        WGPUTextureFormat_Depth32Float
                    },
                    usage: WGPUTextureUsage_RenderAttachment
                        | if i == 0 {
                            WGPUTextureUsage_TextureBinding
                        } else {
                            0
                        },
                    mipLevelCount: 1,
                    sampleCount: 1,
                    ..Default::default()
                },
            );
            if s.textures[i].is_null() {
                return Err("cloud target allocation failed".into());
            }
            s.views[i] = (api.wgpuTextureCreateView)(s.textures[i], ptr::null());
            if s.views[i].is_null() {
                return Err("cloud target view failed".into());
            }
        }
        let wgsl = WGPUShaderSourceWGSL {
            chain: WGPUChainedStruct {
                next: ptr::null(),
                sType: WGPUSType_ShaderSourceWGSL,
            },
            code: view(UPSAMPLE),
        };
        let shader = (api.wgpuDeviceCreateShaderModule)(
            c.device,
            &WGPUShaderModuleDescriptor {
                nextInChain: &wgsl.chain,
                label: view("Cloud upsample"),
            },
        );
        if shader.is_null() {
            return Err("cloud upsample shader failed".into());
        }
        let target = WGPUColorTargetState {
            format: c.format,
            writeMask: WGPUColorWriteMask_All,
            ..Default::default()
        };
        let fs = WGPUFragmentState {
            module: shader,
            entryPoint: view("fs"),
            targetCount: 1,
            targets: &target,
            ..Default::default()
        };
        let depth = WGPUDepthStencilState {
            format: WGPUTextureFormat_Depth32Float,
            depthWriteEnabled: WGPUOptionalBool_False,
            depthCompare: WGPUCompareFunction_Always,
            ..Default::default()
        };
        s.pipeline = (api.wgpuDeviceCreateRenderPipeline)(
            c.device,
            &WGPURenderPipelineDescriptor {
                label: view("Upsample volumetric sky"),
                vertex: WGPUVertexState {
                    module: shader,
                    entryPoint: view("vs"),
                    ..Default::default()
                },
                primitive: WGPUPrimitiveState {
                    topology: WGPUPrimitiveTopology_TriangleList,
                    ..Default::default()
                },
                multisample: WGPUMultisampleState {
                    count: 1,
                    mask: u32::MAX,
                    ..Default::default()
                },
                fragment: &fs,
                depthStencil: &depth,
                ..Default::default()
            },
        );
        (api.wgpuShaderModuleRelease)(shader);
        if s.pipeline.is_null() {
            return Err("cloud upsample pipeline failed".into());
        }
        let layout = (api.wgpuRenderPipelineGetBindGroupLayout)(s.pipeline, 0);
        if layout.is_null() {
            return Err("cloud upsample layout failed".into());
        }
        let entry = WGPUBindGroupEntry {
            binding: 0,
            textureView: s.views[0],
            ..Default::default()
        };
        s.group = (api.wgpuDeviceCreateBindGroup)(
            c.device,
            &WGPUBindGroupDescriptor {
                layout,
                entryCount: 1,
                entries: &entry,
                ..Default::default()
            },
        );
        (api.wgpuBindGroupLayoutRelease)(layout);
        if s.group.is_null() {
            return Err("cloud upsample binding failed".into());
        }
        check_errors()?;
        Ok(s)
    }
    pub unsafe fn encode(&self, c: &Client, encoder: WGPUCommandEncoder) -> Result<(), String> {
        let color = WGPURenderPassColorAttachment {
            view: self.views[0],
            depthSlice: u32::MAX,
            loadOp: WGPULoadOp_Clear,
            storeOp: WGPUStoreOp_Store,
            ..Default::default()
        };
        let depth = WGPURenderPassDepthStencilAttachment {
            view: self.views[1],
            depthLoadOp: WGPULoadOp_Clear,
            depthStoreOp: WGPUStoreOp_Discard,
            depthClearValue: 1.,
            ..Default::default()
        };
        let pass = (self.api.wgpuCommandEncoderBeginRenderPass)(
            encoder,
            &WGPURenderPassDescriptor {
                label: view("Volumetric sky"),
                colorAttachmentCount: 1,
                colorAttachments: &color,
                depthStencilAttachment: &depth,
                ..Default::default()
            },
        );
        if pass.is_null() {
            return Err("cloud pass failed".into());
        }
        (self.api.wgpuRenderPassEncoderSetPipeline)(pass, c.pipeline);
        (self.api.wgpuRenderPassEncoderSetBindGroup)(pass, 0, c.bind_groups[0], 0, ptr::null());
        (self.api.wgpuRenderPassEncoderDraw)(pass, 6, 1, 0, 0);
        (self.api.wgpuRenderPassEncoderEnd)(pass);
        (self.api.wgpuRenderPassEncoderRelease)(pass);
        Ok(())
    }
    pub unsafe fn composite(&self, pass: WGPURenderPassEncoder) {
        (self.api.wgpuRenderPassEncoderSetPipeline)(pass, self.pipeline);
        (self.api.wgpuRenderPassEncoderSetBindGroup)(pass, 0, self.group, 0, ptr::null());
        (self.api.wgpuRenderPassEncoderDraw)(pass, 3, 1, 0, 0);
    }
}
impl Drop for Sky {
    fn drop(&mut self) {
        unsafe {
            if !self.group.is_null() {
                (self.api.wgpuBindGroupRelease)(self.group);
            }
            if !self.pipeline.is_null() {
                (self.api.wgpuRenderPipelineRelease)(self.pipeline);
            }
            for v in self.views {
                if !v.is_null() {
                    (self.api.wgpuTextureViewRelease)(v);
                }
            }
            for t in self.textures {
                if !t.is_null() {
                    (self.api.wgpuTextureRelease)(t);
                }
            }
        }
    }
}
