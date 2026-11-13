//! Native clients, private to GraphicsConnectionSystem.
#![allow(unsafe_op_in_unsafe_fn)]
#[path = "ios_gl.rs"]
mod opengl;
#[path = "Performance.rs"]
mod performance;
#[path = "Scene.rs"]
mod scene;
#[path = "Vegetation/mod.rs"]
mod vegetation;
mod vulkan;
#[path = "Water/mod.rs"]
mod water;
mod webgpu;
#[path = "ios_window.rs"]
mod window;
#[path = "Workstation.rs"]
mod workstation;

pub(super) fn validate_workstation_frame(rows: &[[f32; 4]]) -> Result<(), String> {
    workstation::validate_frame(rows)
}
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
    impl Api {
        pub(super) unsafe fn resolve(&self, name: &std::ffi::CStr) -> *const std::ffi::c_void {
            unsafe {
                self.__library
                    .get::<*const std::ffi::c_void>(name.to_bytes_with_nul())
                    .map(|p| *p)
                    .unwrap_or(std::ptr::null())
            }
        }
    }
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
            Appearance::Visual {
                vertices, shader, ..
            } if vertices.len() <= crate::base_entity::visual_vertex_limit(shader) => {
                let mut data = [0.0f32; 24];
                for (i, v) in vertices.iter().take(3).enumerate() {
                    data[i * 4..i * 4 + 4].copy_from_slice(&[v[0], v[1], v[2], 1.0]);
                    data[12 + i * 4..16 + i * 4].copy_from_slice(&[v[3], v[4], v[5], 1.0]);
                }
                data
            }
            _ => {
                return Err(
                    "renderer requires a supported visual mesh within its triangle budget".into(),
                );
            }
        };
        let workstation = matches!(&entity.appearance, Appearance::Visual { shader, .. } if shader == "workstation_panel");
        // SAFETY: native API calls run on this main thread. Each backend owns
        // its handles and keeps its API library alive through resource cleanup.
        unsafe {
            match backend {
                "OpenGL" => Ok(Self(Backend::OpenGL(opengl::Client::new(
                    root,
                    &data,
                    output,
                    workstation,
                )?))),
                "Vulkan" => Ok(Self(Backend::Vulkan(vulkan::Client::new(
                    &data,
                    root,
                    output,
                    workstation,
                )?))),
                "WebGPU" => Ok(Self(Backend::WebGPU(webgpu::Client::new(
                    root,
                    &data,
                    output,
                    workstation,
                    false,
                )?))),
                _ => Err("unknown explicit backend".into()),
            }
        }
    }
    pub(super) fn texture(&mut self, bytes: &[u8]) -> Result<(), String> {
        unsafe {
            match &mut self.0 {
                Backend::OpenGL(c) => c.texture(bytes),
                Backend::Vulkan(c) => c.texture(bytes),
                Backend::WebGPU(c) => c.texture(bytes),
            }
        }
    }
    pub(super) fn monitor_image(&mut self, image: &[u8]) -> Result<(), String> {
        if image.len() != crate::scene_assets::SCREEN_BYTES { return Err("invalid monitor image size".into()); }
        unsafe { match &mut self.0 {
            Backend::OpenGL(c) => c.monitor_image(image),
            Backend::Vulkan(c) => c.monitor_image(image),
            Backend::WebGPU(c) => c.monitor_image(image),
        }}
    }
    pub(super) fn input_mode(&mut self, pointer: bool) -> Result<(), String> {
        unsafe {
            match &mut self.0 {
                Backend::OpenGL(c) => c.window.input_mode(pointer),
                Backend::Vulkan(c) => c.window.input_mode(pointer),
                Backend::WebGPU(c) => c.window.input_mode(pointer),
            }
        }
    }
    fn workstation_overlay(&mut self,kind:u32,value:&Value,font:Option<&Path>)->Result<(),String>{
        let payload=std::ffi::CString::new(value.to_string()).map_err(|e|e.to_string())?;
        let font=std::ffi::CString::new(font.map(|p|p.to_string_lossy().into_owned()).unwrap_or_default()).map_err(|e|e.to_string())?;
        unsafe{
            let window=match &self.0{Backend::OpenGL(c)=>&c.window,Backend::Vulkan(c)=>&c.window,Backend::WebGPU(c)=>&c.window};
            let (width,height)=window.logical_dimensions()?;
            unsafe extern "C"{fn ww_ios_workstation_overlay_publish(kind:u32,json:*const std::ffi::c_char,font:*const std::ffi::c_char,width:f64,height:f64)->bool;}
            if !ww_ios_workstation_overlay_publish(kind,payload.as_ptr(),font.as_ptr(),width as f64,height as f64){return Err("Invalid iOS workstation overlay payload".into());}
        }
        Ok(())
    }
    pub(super) fn plugin_surface(&mut self,value:&Value,font:&Path)->Result<(),String>{self.workstation_overlay(0,value,Some(font))}
    pub(super) fn selection_range(&mut self,value:&Value)->Result<(),String>{self.workstation_overlay(1,value,None)}
    pub(super) fn cursor_label(&mut self,value:&Value,font:&Path)->Result<(),String>{self.workstation_overlay(2,value,Some(font))}
    pub(super) fn cc_parameter_picker(&mut self,value:&Value)->Result<(),String>{self.workstation_overlay(3,value,None)}
    pub(super) fn title(&mut self, name: &str, _show_hud: bool) -> Result<(), String> {
        window::caption(name)
    }

    pub(super) fn environment(&mut self, shader: &str) {
        let clear = match shader {
            "planet_surface" => [0.48, 0.62, 0.72, 1.],
            _ => [0.035, 0.045, 0.065, 1.],
        };
        // The material workshop shows an arbitrary moving scene, so its
        // center pixel need not contain the fixed diagnostic triangle.
        let diagnostic = shader != "planet_surface"
            && shader != "material_surface"
            && shader != "workstation_panel";
        match &mut self.0 {
            Backend::OpenGL(c) => {
                c.clear = clear;
                c.validate_triangle_center = diagnostic;
            }
            Backend::Vulkan(c) => {
                c.clear = clear;
                c.validate_triangle_center = diagnostic;
            }
            Backend::WebGPU(c) => {
                c.clear = clear;
                c.validate_triangle_center = diagnostic;
            }
        };
    }
    pub(super) fn table_eye(&mut self, view:&Value)->Result<(),String>{
        unsafe{match &mut self.0{Backend::OpenGL(c)=>c.table_eye(view),Backend::Vulkan(c)=>c.table_eye(view),Backend::WebGPU(c)=>c.table_eye(view)}}
    }
    pub(super) fn set_world_backdrop(&mut self,root:&Path,enabled:bool)->Result<bool,String>{
        unsafe {match &mut self.0 {
            Backend::OpenGL(c)=>c.set_world_backdrop(root,enabled),
            Backend::Vulkan(c)=>c.set_world_backdrop(root,enabled),
            Backend::WebGPU(c)=>c.set_world_backdrop(root,enabled),
        }}
    }
    pub(super) fn set_audio_overlay(&mut self,triangles:&[[f32;24]])->Result<bool,String>{unsafe{match &mut self.0{Backend::WebGPU(c)=>{c.set_audio_overlay(triangles)?;Ok(true)},_=>Ok(false)}}}
    pub(super) fn require_world_backdrop(&self)->Result<(),String>{match &self.0{Backend::WebGPU(c)=>c.require_world_backdrop(),_=>Err("retained world images require WebGPU".into())}}
    pub(super) fn set_workstation(&mut self, rows: &[[f32; 4]]) -> Result<(), String> {
        workstation::validate_frame(rows)?;
        unsafe {
            match &mut self.0 {
                Backend::OpenGL(c) => c.set_workstation(rows),
                Backend::Vulkan(c) => c.set_workstation(rows),
                Backend::WebGPU(c) => c.set_workstation(rows),
            }
        }
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
    pub(super) fn set_water(&mut self,root:&Path,packets:&Value,view:&Value,triangles:&[[f32;24]])->Result<(),String>{
        let packets=crate::water::presentation::parse_collection(packets)?;self.set_water_numeric(root,&packets,view,triangles)
    }
    pub(super) fn set_water_numeric(
        &mut self,
        root: &Path,
        packets: &[crate::water::presentation::Packet],
        view: &Value,
        triangles: &[[f32; 24]],
    ) -> Result<(), String> {
        unsafe {
            match &mut self.0 {
                Backend::OpenGL(c) => c.set_water(root, packets, view, triangles),
                Backend::Vulkan(c) => c.set_water(root, packets, view, triangles),
                Backend::WebGPU(c) => c.set_water(root, packets, view, triangles),
            }
        }
    }
    pub(super) fn set_vegetation(
        &mut self,
        root: &Path,
        packet: Option<crate::vegetation::packet::Packet>,
    ) -> Result<(), String> {
        unsafe {
            match &mut self.0 {
                Backend::OpenGL(c) => c.set_vegetation(root, packet),
                Backend::Vulkan(c) => c.set_vegetation(root, packet),
                Backend::WebGPU(c) => c.set_vegetation(root, packet),
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
        if !window::active() {
            return Ok(
                json!({"closed":window::stop_requested(),"keys":[false,false,false],"drawn":false,"suspended":!window::stop_requested(),"device":self.info()}),
            );
        }
        unsafe {
            let window = match &mut self.0 {
                Backend::OpenGL(c) => &mut c.window,
                Backend::Vulkan(c) => &mut c.window,
                Backend::WebGPU(c) => &mut c.window,
            };
            window.show();
            let closed = window.closed();
            let mut keys = window.keys()?;
            let flight = window.flight_input()?;
            keys[1] = flight["pause"] == true;
            let (w, h) = window.dimensions()?;
            let (logical_width, logical_height) = window.logical_dimensions()?;
            let aspect = w as f64 / h as f64;
            if !closed {
                match &mut self.0 {
                    Backend::OpenGL(c) => c.draw()?,
                    Backend::Vulkan(c) => c.draw()?,
                    Backend::WebGPU(c) => c.draw()?,
                }
            }
            Ok(
                json!({"closed":closed,"keys":keys,"drawn":!closed,"aspect":aspect,"width":w,"height":h,"logical_width":logical_width,"logical_height":logical_height,"device":self.info(),"flight":flight}),
            )
        }
    }
    pub(super) fn request_close(&self) -> Result<(), String> {
        unsafe {
            match &self.0 {
                Backend::OpenGL(c) => c.window.request_close(),
                Backend::Vulkan(c) => c.window.request_close(),
                Backend::WebGPU(c) => c.window.request_close(),
            }
        }
    }
    pub(super) fn capture_next(&mut self) {
        match &mut self.0 {
            Backend::OpenGL(c) => c.captured = false,
            Backend::Vulkan(c) => c.captured = false,
            Backend::WebGPU(c) => c.captured = false,
        };
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
