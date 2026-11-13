//! Water resource/helper boundary, private to the native graphics connection.
//! The parent supplies its existing device/dispatch and completed opaque/reflected
//! attachments. This module never creates a window, device, surface, or swapchain.
#[path = "data.rs"]
pub(crate) mod data;
#[path = "opengl.rs"]
pub(crate) mod opengl;
#[path = "reflection.rs"]
pub(crate) mod reflection;
#[path = "vulkan.rs"]
pub(crate) mod vulkan;
#[path = "webgpu.rs"]
pub(crate) mod webgpu;
use data::{DrawFrame, ResourceSlot};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ColorFormat {
    Rgba8Unorm,
    Bgra8Unorm,
    Rgba16Float,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DepthFormat {
    Depth32Float,
    Depth24Stencil8,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct ResourceRequest {
    pub width: u32,
    pub height: u32,
    pub color_format: ColorFormat,
    pub depth_format: DepthFormat,
    pub planar_reflection: bool,
    pub vertex_capacity: usize,
    pub index_capacity: usize,
}
impl ResourceRequest {
    pub fn from_frame(
        frame: &DrawFrame,
        color_format: ColorFormat,
        depth_format: DepthFormat,
    ) -> Result<Self, String> {
        frame.validate()?;
        Ok(Self {
            width: frame.camera.width,
            height: frame.camera.height,
            color_format,
            depth_format,
            planar_reflection: frame.optics.planar_reflection,
            vertex_capacity: frame.vertices.len(),
            index_capacity: frame.indices.len(),
        })
    }
}
/// Native adapter handles only. This type is never serialized into a System envelope.
/// Attachment extents/origin/clip matrices must correspond to the same DrawFrame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BorrowedAttachments<T> {
    pub scene_color: T,
    pub scene_depth: T,
    pub planar_reflection: Option<T>,
    pub output: T,
}
impl<T> BorrowedAttachments<T> {
    /// Borrowed inputs only. WaterLayer is reported separately after allocation.
    pub fn logical_slots(&self) -> Vec<ResourceSlot> {
        let mut slots = vec![ResourceSlot::SceneColor, ResourceSlot::SceneDepth];
        if self.planar_reflection.is_some() {
            slots.push(ResourceSlot::PlanarReflection);
        }
        slots
    }
}
/// Explicitly packed std140/WGSL vec4 layout, 1296 bytes. Matrices are column-major.
/// resolution.w is scene-texture UV +Y relative to view-space +Y: +1 GL, -1 WebGPU/Vulkan.
pub(crate) fn uniform_bytes(frame: &DrawFrame, uv_y_sign: f32) -> Result<Vec<u8>, String> {
    frame.validate()?;
    let mut bytes = Vec::with_capacity(1296);
    let mut floats = |values: &[f32]| {
        for n in values {
            bytes.extend(n.to_ne_bytes());
        }
    };
    for column in &frame.camera.view {
        floats(column);
    }
    for column in &frame.camera.projection {
        floats(column);
    }
    let c = &frame.camera;
    let o = &frame.optics;
    floats(&[
        c.relative_position[0],
        c.relative_position[1],
        c.relative_position[2],
        o.time,
    ]);
    floats(&[c.width as f32, c.height as f32, o.murkiness, uv_y_sign]);
    floats(&[o.light[0], o.light[1], o.light[2], o.sky_color_intensity]);
    floats(&[
        o.sky_top[0],
        o.sky_top[1],
        o.sky_top[2],
        o.neutral_reflection_intensity,
    ]);
    floats(&[
        o.sky_bottom[0],
        o.sky_bottom[1],
        o.sky_bottom[2],
        if o.planar_reflection { 1. } else { 0. },
    ]);
    floats(&[
        o.surface_up[0],
        o.surface_up[1],
        o.surface_up[2],
        frame.ripples.len() as f32,
    ]);
    floats(&[
        o.tangent_u[0],
        o.tangent_u[1],
        o.tangent_u[2],
        if o.fullbright {1.}else{0.}, // tangent_u.w: explicit cheat inspection
    ]);
    floats(&[
        o.tangent_v[0],
        o.tangent_v[1],
        o.tangent_v[2],
        if o.cave_fog {1.}else{0.}, // tangent_v.w: shared deep-cave visibility fade
    ]);
    floats(&[
        o.micro_phase[0],
        o.micro_phase[1],
        o.micro_phase[2],
        if c.reversed_z { -1. } else { 1. },
    ]);
    for i in 0..data::MAX_RIPPLES {
        let value = frame
            .ripples
            .get(i)
            .map_or([0.; 4], |r| [r.uv[0], r.uv[1], r.age, r.strength]);
        floats(&value);
    }
    Ok(bytes)
}
