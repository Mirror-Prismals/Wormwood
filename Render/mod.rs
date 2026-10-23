//! Native clients, private to GraphicsConnectionSystem.
#![allow(unsafe_op_in_unsafe_fn)]
mod opengl;
mod vulkan;
mod webgpu;
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
    OpenGL(opengl::Client),
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
        let data = match &entity.appearance {
            Appearance::Visual { vertices, .. } if vertices.len() <= 384 => {
                let mut data = [0.0f32; 24];
                for (i, v) in vertices.iter().take(3).enumerate() {
                    data[i * 4..i * 4 + 4].copy_from_slice(&[v[0], v[1], v[2], 1.0]);
                    data[12 + i * 4..16 + i * 4].copy_from_slice(&[v[3], v[4], v[5], 1.0]);
                }
                data
            }
            _ => return Err("initial renderer supports at most twelve triangles".into()),
        };
        // SAFETY: native API calls run on this main thread. Each backend owns
        // its handles and keeps its API library alive through resource cleanup.
        unsafe {
            match backend {
                "OpenGL" => Ok(Self(Backend::OpenGL(opengl::Client::new(
                    root, &data, output,
                )?))),
                "Vulkan" => Ok(Self(Backend::Vulkan(vulkan::Client::new(
                    &data, root, output,
                )?))),
                "WebGPU" => Ok(Self(Backend::WebGPU(webgpu::Client::new(
                    root, &data, output,
                )?))),
                _ => Err("unknown explicit backend".into()),
            }
        }
    }
    pub(super) fn title(&mut self, name: &str) -> Result<(), String> {
        let title = std::ffi::CString::new(name).map_err(|e| e.to_string())?;
        unsafe {
            let window = match &self.0 {
                Backend::OpenGL(c) => &c.window,
                Backend::Vulkan(c) => &c.window,
                Backend::WebGPU(c) => &c.window,
            };
            window.symbol::<unsafe extern "C" fn(*mut std::ffi::c_void, *const std::ffi::c_char)>(
                b"glfwSetWindowTitle\0",
            )?(window.handle, title.as_ptr());
        }
        Ok(())
    }
    pub(super) fn environment(&mut self, shader: &str) {
        let clear = match shader {
            "water_volume" => [0.02, 0.16, 0.22, 1.],
            "planet_surface" => [0.24, 0.43, 0.65, 1.],
            _ => [0.035, 0.045, 0.065, 1.],
        };
        match &mut self.0 {
            Backend::OpenGL(c) => c.clear = clear,
            Backend::Vulkan(c) => c.clear = clear,
            Backend::WebGPU(c) => c.clear = clear,
        };
    }
    pub(super) fn set_batch(&mut self, triangles: &[[f32; 24]]) -> Result<(), String> {
        unsafe {
            match &mut self.0 {
                Backend::OpenGL(c) => c.set_batch(triangles),
                Backend::Vulkan(c) => c.set_batch(triangles),
                Backend::WebGPU(c) => c.set_batch(triangles),
            }
        }
    }
    pub(super) fn info(&self) -> Value {
        match &self.0 {
            Backend::OpenGL(c) => c.info.clone(),
            Backend::Vulkan(c) => c.info.clone(),
            Backend::WebGPU(c) => c.info.clone(),
        }
    }
    pub(super) fn present(
        &mut self,
        limit: Option<u32>,
        frame_period_us: u64,
        close_after: Option<u32>,
        mut record: impl FnMut(u64) -> Result<(), String>,
    ) -> Result<Value, String> {
        let mut count = 0u64;
        let mut window_closed = false;
        let mut close_requested = false;
        let backend = match &mut self.0 {
            Backend::OpenGL(_) => "OpenGL",
            Backend::Vulkan(_) => "Vulkan",
            Backend::WebGPU(_) => "WebGPU",
        };
        unsafe {
            match &mut self.0 {
                Backend::OpenGL(c) => c.window.show(),
                Backend::Vulkan(c) => c.window.show(),
                Backend::WebGPU(c) => c.window.show(),
            }
            while limit.is_none_or(|n| count < u64::from(n)) {
                let close = match &mut self.0 {
                    Backend::OpenGL(c) => c.window.closed(),
                    Backend::Vulkan(c) => c.window.closed(),
                    Backend::WebGPU(c) => c.window.closed(),
                };
                if close {
                    window_closed = true;
                    break;
                }
                match &mut self.0 {
                    Backend::OpenGL(c) => c.draw()?,
                    Backend::Vulkan(c) => c.draw()?,
                    Backend::WebGPU(c) => c.draw()?,
                }
                count += 1;
                record(count)?;
                if !close_requested && close_after.is_some_and(|n| count >= u64::from(n)) {
                    match &self.0 {
                        Backend::OpenGL(c) => c.window.request_close()?,
                        Backend::Vulkan(c) => c.window.request_close()?,
                        Backend::WebGPU(c) => c.window.request_close()?,
                    };
                    close_requested = true;
                }
                // Explicit finite-diagnostic pacing. This waits only on the
                // presentation thread, never in the real-time audio callback.
                if frame_period_us != 0 {
                    std::thread::sleep(std::time::Duration::from_micros(frame_period_us));
                }
            }
        }
        Ok(
            json!({"backend":backend,"frames":count,"frame_period_us":frame_period_us,"close_source":if window_closed{Some(if close_requested{"diagnostic_request"}else{"window_event"})}else{None},"end_reason":if window_closed{"window_closed"}else{"frame_limit"},"device":self.info()}),
        )
    }
    pub(super) fn pump(&mut self) -> Result<Value, String> {
        unsafe {
            let window = match &mut self.0 {
                Backend::OpenGL(c) => &mut c.window,
                Backend::Vulkan(c) => &mut c.window,
                Backend::WebGPU(c) => &mut c.window,
            };
            window.show();
            let closed = window.closed();
            let keys = window.keys()?;
            let flight = window.flight_input()?;
            if !closed {
                match &mut self.0 {
                    Backend::OpenGL(c) => c.draw()?,
                    Backend::Vulkan(c) => c.draw()?,
                    Backend::WebGPU(c) => c.draw()?,
                }
            }
            Ok(
                json!({"closed":closed,"keys":keys,"drawn":!closed,"device":self.info(),"flight":flight}),
            )
        }
    }
    pub(super) fn finish(&mut self) -> Result<(), String> {
        unsafe {
            match &mut self.0 {
                Backend::OpenGL(c) => c.wait(),
                Backend::Vulkan(c) => c.wait(),
                Backend::WebGPU(c) => c.wait(),
            }
        }
    }
}
