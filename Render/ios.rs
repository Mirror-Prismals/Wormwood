//! Explicit iOS API clients sharing the Entity/Audicle connection boundary.
#![allow(unsafe_op_in_unsafe_fn)]
#[path = "ios_gl.rs"]
mod gl;
mod vulkan;
mod webgpu;
#[path = "ios_window.rs"]
mod window;
#[allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_imports
)]
mod vk {
    include!(concat!(env!("OUT_DIR"), "/vk.rs"));
}
#[allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_imports
)]
mod wgpu {
    include!(concat!(env!("OUT_DIR"), "/wgpu.rs"));
}
use crate::base_entity::{Appearance, Entity};
use serde_json::{Value, json};
use std::path::Path;
enum Backend {
    OpenGL(gl::Session),
    Vulkan(vulkan::Client),
    WebGPU(webgpu::Client),
}
pub(super) struct Session(Backend);
impl Session {
    pub(super) fn prepare(
        root: &Path,
        output: &Path,
        backend: &str,
        entity: &Entity,
    ) -> Result<Self, String> {
        if backend == "OpenGL" {
            return Ok(Self(Backend::OpenGL(gl::Session::prepare(
                root, backend, entity,
            )?)));
        }
        let vertices = match &entity.appearance {
            Appearance::Visual { vertices, .. } if vertices.len() == 3 => vertices,
            _ => return Err("initial iOS renderer requires three vertices".into()),
        };
        let mut data = [0f32; 24];
        for (i, v) in vertices.iter().enumerate() {
            data[i * 4..i * 4 + 4].copy_from_slice(&[v[0], v[1], v[2], 1.]);
            data[12 + i * 4..16 + i * 4].copy_from_slice(&[v[3], v[4], v[5], 1.]);
        }
        unsafe {
            match backend {
                "Vulkan" => Ok(Self(Backend::Vulkan(vulkan::Client::new(
                    &data, root, output,
                )?))),
                "WebGPU" => Ok(Self(Backend::WebGPU(webgpu::Client::new(
                    root, &data, output,
                )?))),
                _ => Err("unknown explicit iOS API".into()),
            }
        }
    }
    pub(super) fn info(&self) -> Value {
        match &self.0 {
            Backend::OpenGL(c) => c.info(),
            Backend::Vulkan(c) => c.info.clone(),
            Backend::WebGPU(c) => c.info.clone(),
        }
    }
    pub(super) fn present(
        &mut self,
        frames: u32,
        mut record: impl FnMut(u32) -> Result<(), String>,
    ) -> Result<Value, String> {
        if let Backend::OpenGL(c) = &mut self.0 {
            return c.present(frames, record);
        }
        let backend = if matches!(&self.0, Backend::Vulkan(_)) {
            "Vulkan"
        } else {
            "WebGPU"
        };
        unsafe {
            match &self.0 {
                Backend::Vulkan(c) => c.window.show(),
                Backend::WebGPU(c) => c.window.show(),
                _ => {}
            }
        }
        for frame in 1..=frames {
            unsafe {
                match &mut self.0 {
                    Backend::Vulkan(c) => {
                        if c.window.closed() {
                            return Err("iOS Vulkan presentation interrupted".into());
                        }
                        c.draw()?;
                    }
                    Backend::WebGPU(c) => {
                        if c.window.closed() {
                            return Err("iOS WebGPU presentation interrupted".into());
                        }
                        c.draw()?;
                    }
                    _ => {}
                }
            }
            record(frame)?;
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        Ok(json!({"backend":backend,"frames":frames,"device":self.info()}))
    }
    pub(super) fn finish(&mut self) -> Result<(), String> {
        unsafe {
            match &mut self.0 {
                Backend::OpenGL(c) => c.finish(),
                Backend::Vulkan(c) => c.wait(),
                Backend::WebGPU(c) => c.wait(),
            }
        }
    }
}
