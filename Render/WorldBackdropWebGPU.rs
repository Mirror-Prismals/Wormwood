//! Native player-scene pipeline in the workstation surface. World passes keep
//! normal clip depth and materials; the optical UI is drawn after water.
use super::*;
pub(super) struct World {
    api: &'static Api,
    pub pipeline: WGPURenderPipeline,
    pub image:Option<super::world_image::Image>,
    overlay:WGPUBuffer,overlay_group:WGPUBindGroup,pub overlay_count:u32,
    shader: WGPUShaderModule,
    pub texture: WGPUBuffer,
    pub group: WGPUBindGroup,
}
impl Client {
    pub unsafe fn set_audio_overlay(&mut self,triangles:&[[f32;24]])->Result<(),String>{
        if triangles.len()>24576 || triangles.iter().flatten().any(|v|!v.is_finite()){return Err("invalid audio overlay".into());}
        self.wait()?;
        if let Some(world)=&mut self.world{world.overlay_count=(triangles.len()*3) as u32;if !triangles.is_empty(){let vertices=audio_vertices(triangles);(self.api.wgpuQueueWriteBuffer)(self.queue,world.overlay,0,vertices.as_ptr().cast(),std::mem::size_of_val(vertices.as_slice()));}}
        Ok(())
    }
    pub(crate) fn require_world_backdrop(&self)->Result<(),String>{if self.world_active&&self.world.is_some(){Ok(())}else{Err("retained game world is not prepared".into())}}
    pub(super) fn scene_texture(&self) -> WGPUBuffer {
        if self.world_active {
            self.world.as_ref().unwrap().texture
        } else {
            self.texture_buffer
        }
    }
    pub(super) fn scene_capacity(&self) -> usize {
        if self.world_active {
            crate::scene_assets::SCENE_BYTES
        } else {
            self.texture_capacity
        }
    }
    pub(super) fn scene_pipeline(&self) -> WGPURenderPipeline {
        if self.world_active {
            self.world.as_ref().unwrap().pipeline
        } else {
            self.pipeline
        }
    }
    pub unsafe fn set_world_backdrop(
        &mut self,
        root: &Path,
        enabled: bool,
    ) -> Result<bool, String> {
        let changed = self.world_active != enabled;
        if enabled && self.world.is_none() {
            self.world = Some(World::new(self, root)?);
        }
        if enabled{if let Some(image)=self.world.as_mut().and_then(|w|w.image.as_mut()){image.invalidate();}}
        if self.world_active != enabled {
            self.wait()?;
            // Bindings reference the scene's own asset buffer, never optical rows.
            self.vegetation.take();
            self.reflection_vegetation.take();
            if !self.reflection_group.is_null() {
                (self.api.wgpuBindGroupRelease)(self.reflection_group);
                self.reflection_group = ptr::null_mut();
            }
            if !self.reflection_buffer.is_null() {
                (self.api.wgpuBufferRelease)(self.reflection_buffer);
                self.reflection_buffer = ptr::null_mut();
            }
            self.water.take();
            self.water_frames.clear();
            self.release_scene_targets();
            self.reflection_count = 0;
            self.world_active = enabled;
        }
        Ok(changed)
    }
}
impl World {
    pub unsafe fn draw_audio(&self,pass:WGPURenderPassEncoder){if self.overlay_count>0{(self.api.wgpuRenderPassEncoderSetPipeline)(pass,self.pipeline);(self.api.wgpuRenderPassEncoderSetBindGroup)(pass,0,self.overlay_group,0,ptr::null());(self.api.wgpuRenderPassEncoderDraw)(pass,self.overlay_count,1,0,0);}}
    unsafe fn new(c: &Client, _root: &Path) -> Result<Self, String> {
        let api = c.api;
        let mut out = Self {
            api,
            pipeline: ptr::null_mut(),
            image:None,overlay:ptr::null_mut(),overlay_group:ptr::null_mut(),overlay_count:0,
            shader: ptr::null_mut(),
            texture: ptr::null_mut(),
            group: ptr::null_mut(),
        };
        let wgsl = WGPUShaderSourceWGSL {
            chain: WGPUChainedStruct {
                next: ptr::null(),
                sType: WGPUSType_ShaderSourceWGSL,
            },
            code: view(include_str!("../Procedures/Shaders/triangle.wgsl")),
        };
        out.shader = (api.wgpuDeviceCreateShaderModule)(
            c.device,
            &WGPUShaderModuleDescriptor {
                nextInChain: &wgsl.chain,
                label: view("Live world camera"),
            },
        );
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
            module: out.shader,
            entryPoint: view("fs_main"),
            targetCount: 1,
            targets: &target,
            ..Default::default()
        };
        let depth = WGPUDepthStencilState {
            format: WGPUTextureFormat_Depth32Float,
            depthWriteEnabled: WGPUOptionalBool_True,
            depthCompare: WGPUCompareFunction_Less,
            ..Default::default()
        };
        out.pipeline = (api.wgpuDeviceCreateRenderPipeline)(
            c.device,
            &WGPURenderPipelineDescriptor {
                vertex: WGPUVertexState {
                    module: out.shader,
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
                depthStencil: &depth,
                ..Default::default()
            },
        );
        let bytes =
            crate::scene_assets::scene_bytes(&[1, 0, 0, 0, 1, 0, 0, 0, 255, 255, 255, 255])?;
        out.texture = (api.wgpuDeviceCreateBuffer)(
            c.device,
            &WGPUBufferDescriptor {
                size: bytes.len() as u64,
                usage: WGPUBufferUsage_Storage | WGPUBufferUsage_CopyDst,
                ..Default::default()
            },
        );
        if out.shader.is_null() || out.pipeline.is_null() || out.texture.is_null() {
            return Err("live world camera resources failed".into());
        }
        (api.wgpuQueueWriteBuffer)(c.queue, out.texture, 0, bytes.as_ptr().cast(), bytes.len());
        let layout = (api.wgpuRenderPipelineGetBindGroupLayout)(out.pipeline, 0);
        let entries = [
            WGPUBindGroupEntry {
                binding: 0,
                buffer: c.vertex_buffer,
                size: crate::scene_limits::SCENE_BUFFER_BYTES,
                ..Default::default()
            },
            WGPUBindGroupEntry {
                binding: 1,
                buffer: out.texture,
                size: bytes.len() as u64,
                ..Default::default()
            },
        ];
        out.group = (api.wgpuDeviceCreateBindGroup)(
            c.device,
            &WGPUBindGroupDescriptor {
                layout,
                entryCount: 2,
                entries: entries.as_ptr(),
                ..Default::default()
            },
        );
        out.overlay=(api.wgpuDeviceCreateBuffer)(c.device,&WGPUBufferDescriptor{size:crate::scene_limits::SCENE_BUFFER_BYTES,usage:WGPUBufferUsage_Storage|WGPUBufferUsage_CopyDst,..Default::default()});
        let mut overlay_entries=entries;overlay_entries[0].buffer=out.overlay;
        out.overlay_group=(api.wgpuDeviceCreateBindGroup)(c.device,&WGPUBindGroupDescriptor{layout,entryCount:2,entries:overlay_entries.as_ptr(),..Default::default()});
        (api.wgpuBindGroupLayoutRelease)(layout);
        if out.group.is_null() {
            return Err("live world camera bindings failed".into());
        }
        check_errors()?;
        Ok(out)
    }
}
impl Drop for World {
    fn drop(&mut self) {
        unsafe {
            if !self.overlay_group.is_null(){(self.api.wgpuBindGroupRelease)(self.overlay_group);}
            if !self.overlay.is_null(){(self.api.wgpuBufferRelease)(self.overlay);}
            if !self.group.is_null() {
                (self.api.wgpuBindGroupRelease)(self.group);
            }
            if !self.pipeline.is_null() {
                (self.api.wgpuRenderPipelineRelease)(self.pipeline);
            }
            if !self.shader.is_null() {
                (self.api.wgpuShaderModuleRelease)(self.shader);
            }
            if !self.texture.is_null() {
                (self.api.wgpuBufferRelease)(self.texture);
            }
        }
    }
}

// The shared scene shader reads interleaved position/data vec4 pairs. Host
// triangle packets store three positions followed by three data values.
fn audio_vertices(triangles:&[[f32;24]])->Vec<f32>{
 let mut vertices=Vec::with_capacity(triangles.len()*24);
 for t in triangles{for i in 0..3{vertices.extend_from_slice(&t[i*4..i*4+4]);vertices.extend_from_slice(&t[12+i*4..16+i*4]);}}
 vertices
}
#[cfg(test)] mod audio_overlay_tests{
 #[test]fn ray_upload_matches_shared_scene_shader_vertex_abi(){
  let t=std::array::from_fn(|i|i as f32);let packed=super::audio_vertices(&[t]);
  assert_eq!(&packed[..8],&[0.,1.,2.,3.,12.,13.,14.,15.]);
  assert_eq!(&packed[16..],&[8.,9.,10.,11.,20.,21.,22.,23.]);
 }
}
