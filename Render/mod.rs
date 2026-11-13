//! Native clients, private to GraphicsConnectionSystem.
#![allow(unsafe_op_in_unsafe_fn)]
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
mod window;
#[path = "WorkstationInputSafety.rs"]
mod input_safety;
#[path = "WorkstationDrawRate.rs"]
mod draw_rate;
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
type Dimensions = (u32, u32, f64, f64);

// A false draw from this gate precedes any backend surface acquisition. Keep
// that distinction explicit: benchmark recovery must not hide a device error.
fn draw_status(closed: bool, iconified: bool, drawable: Option<(u32, u32)>,
               logical: Option<(f64, f64)>, drawn: bool) -> Value {
    let (reason, recoverable) = if closed { ("window_closed", false) }
        else if iconified { ("iconified", true) }
        else if logical.is_none() { ("zero_logical_dimensions", true) }
        else if drawable.is_none() { ("zero_framebuffer", true) }
        else if drawn { ("presented", false) }
        else { ("redraw_not_requested", false) };
    json!({"reason":reason,"recoverable":recoverable,"attempted":drawn,
        "iconified":iconified,"drawable_dimensions":drawable,"logical_dimensions":logical})
}

/// Presentation is demand-driven only for the static workstation. Input polling
/// remains independent of whether a new optical frame needs GPU work.
struct Redraw {
    workstation: bool,
    paused_game: bool,
    dirty: bool,
    suspended: bool,
    dimensions: Option<Dimensions>,
}
impl Redraw {
    fn new(workstation: bool) -> Self {
        Self {
            workstation,
            paused_game: false,
            dirty: true,
            suspended: false,
            dimensions: None,
        }
    }
    fn invalidate(&mut self) {
        self.dirty = true;
    }
    // Keep input/audio ticking and retain all redraw debt while an ordinary
    // workstation window is unfocused. Finite diagnostics explicitly force draws.
    // This precedes backend draw/acquire; it cannot cancel a call already running.
    fn defer_unfocused(&mut self, focused: bool, force_draw: bool) -> bool {
        let deferred = (self.workstation || self.paused_game) && !focused && !force_draw;
        if deferred { self.invalidate(); }
        deferred
    }
    fn requested(&mut self, dimensions: Option<Dimensions>, exposed: bool, force: bool) -> bool {
        self.dirty |= exposed;
        let Some(dimensions) = dimensions else {
            self.suspended = true;
            return false;
        };
        self.dirty |= self.suspended || self.dimensions != Some(dimensions);
        self.suspended = false;
        self.dimensions = Some(dimensions);
        (!self.workstation && !self.paused_game) || self.dirty || force
    }
    fn presented(&mut self) {
        self.dirty = false;
    }
}

pub(super) struct Session(Backend, Redraw);
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
        if workstation && backend!="WebGPU" && crate::workstation::cardinal_theme::selected(root)?.is_some(){
            return Err("Cardinal 2D themes currently require the WebGPU compositor; select WebGPU or use the default 3D gem ruby theme".into());
        }
        // SAFETY: native API calls run on this main thread. Each backend owns
        // its handles and keeps its API library alive through resource cleanup.
        unsafe {
            match backend {
                "OpenGL" => Ok(Self(
                    Backend::OpenGL(opengl::Client::new(root, &data, output, workstation)?),
                    Redraw::new(workstation),
                )),
                "Vulkan" => Ok(Self(
                    Backend::Vulkan(vulkan::Client::new(&data, root, output, workstation)?),
                    Redraw::new(workstation),
                )),
                "WebGPU" => Ok(Self(
                    Backend::WebGPU(webgpu::Client::new(root, &data, output, workstation, false)?),
                    Redraw::new(workstation),
                )),
                _ => Err("unknown explicit backend".into()),
            }
        }
    }
    pub(super) fn prewarm_workstation(root:&Path,output:&Path)->Result<Self,String>{
        let data=[0.;24];
        unsafe{
            let mut session=Self(Backend::WebGPU(webgpu::Client::new(root,&data,output,true,true)?),Redraw::new(true));
            session.set_world_backdrop(root,true)?;
            session.set_world_backdrop(root,false)?;
            Ok(session)
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
    #[cfg(target_os = "macos")]
    pub(super) fn pause_menu(
        &mut self,
        visible: bool,
        state: &serde_json::Value,
    ) -> Result<(), String> {
        if !self.1.workstation && self.1.paused_game != visible {
            self.1.paused_game = visible;
            self.1.invalidate();
        }
        unsafe {
            match &self.0 {
                Backend::OpenGL(c) => c.window.pause_menu(visible, state),
                Backend::Vulkan(c) => c.window.pause_menu(visible, state),
                Backend::WebGPU(c) => c.window.pause_menu(visible, state),
            }
        }
    }
    /// Pass-through baked artwork: independent of scene redraw and optical buffers.
    #[cfg(target_os="macos")]
    pub(super) fn cursor_label(&mut self,label:&Value,font:&Path)->Result<(),String>{
        let text=std::ffi::CString::new(label["text"].as_str().unwrap_or("")).map_err(|e|e.to_string())?;
        let font=std::ffi::CString::new(font.to_string_lossy().as_bytes()).map_err(|e|e.to_string())?;
        let payload=std::ffi::CString::new(label.to_string()).map_err(|e|e.to_string())?;
        let p=&label["position"];
        unsafe {
            let window=match &self.0{Backend::OpenGL(c)=>&c.window,Backend::Vulkan(c)=>&c.window,Backend::WebGPU(c)=>&c.window};
            let cocoa=window.symbol::<unsafe extern "C" fn(*mut std::ffi::c_void)->*mut std::ffi::c_void>(b"glfwGetCocoaWindow\0")?(window.handle);
            unsafe extern "C" {fn ww_workstation_cursor_label(window:*mut std::ffi::c_void,text:*const std::ffi::c_char,x:f64,y:f64,font:*const std::ffi::c_char,payload:*const std::ffi::c_char)->bool;}
            if !ww_workstation_cursor_label(cocoa,text.as_ptr(),p[0].as_f64().unwrap_or(0.),p[1].as_f64().unwrap_or(0.),font.as_ptr(),payload.as_ptr()){
                return Err("Could not load workstation drag artwork or bundled font".into());
            }
        }
        Ok(())
    }
    #[cfg(target_os="macos")]
    pub(super) fn selection_range(&mut self,range:&Value)->Result<(),String>{
        let payload=std::ffi::CString::new(range.to_string()).map_err(|e|e.to_string())?;
        unsafe{
            let window=match &self.0{Backend::OpenGL(c)=>&c.window,Backend::Vulkan(c)=>&c.window,Backend::WebGPU(c)=>&c.window};
            let cocoa=window.symbol::<unsafe extern "C" fn(*mut std::ffi::c_void)->*mut std::ffi::c_void>(b"glfwGetCocoaWindow\0")?(window.handle);
            unsafe extern "C"{fn ww_workstation_selection_range(window:*mut std::ffi::c_void,json:*const std::ffi::c_char)->bool;}
            if !ww_workstation_selection_range(cocoa,payload.as_ptr()){return Err("Invalid native range overlay".into());}
        }Ok(())
    }
    #[cfg(target_os="macos")]
    pub(super) fn plugin_surface(&mut self,surface:&Value,font:&Path)->Result<(),String>{
        let payload=std::ffi::CString::new(surface.to_string()).map_err(|e|e.to_string())?;
        let font=std::ffi::CString::new(font.to_string_lossy().as_bytes()).map_err(|e|e.to_string())?;
        unsafe{
            let window=match &self.0{Backend::OpenGL(c)=>&c.window,Backend::Vulkan(c)=>&c.window,Backend::WebGPU(c)=>&c.window};
            let cocoa=window.symbol::<unsafe extern "C" fn(*mut std::ffi::c_void)->*mut std::ffi::c_void>(b"glfwGetCocoaWindow\0")?(window.handle);
            unsafe extern "C"{fn ww_workstation_plugin_surface(window:*mut std::ffi::c_void,json:*const std::ffi::c_char,font:*const std::ffi::c_char)->bool;}
            if !ww_workstation_plugin_surface(cocoa,payload.as_ptr(),font.as_ptr()){return Err("Could not present Cardinal plugin labels".into());}
        }Ok(())
    }
    #[cfg(target_os="macos")]
    pub(super) fn cc_parameter_picker(&mut self,picker:&Value)->Result<(),String>{
        let payload=std::ffi::CString::new(picker.to_string()).map_err(|e|e.to_string())?;
        unsafe{
            let window=match &self.0{Backend::OpenGL(c)=>&c.window,Backend::Vulkan(c)=>&c.window,Backend::WebGPU(c)=>&c.window};
            let cocoa=window.symbol::<unsafe extern "C" fn(*mut std::ffi::c_void)->*mut std::ffi::c_void>(b"glfwGetCocoaWindow\0")?(window.handle);
            unsafe extern "C"{fn ww_workstation_cc_parameter_picker(window:*mut std::ffi::c_void,json:*const std::ffi::c_char)->bool;}
            if !ww_workstation_cc_parameter_picker(cocoa,payload.as_ptr()){return Err("Invalid native CC parameter picker".into());}
        }Ok(())
    }
    pub(super) fn title(&mut self, name: &str, show_hud: bool) -> Result<(), String> {
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
            #[cfg(target_os = "macos")]
            {
                unsafe extern "C" {
                    fn ww_material_hud(
                        window: *mut std::ffi::c_void,
                        text: *const std::ffi::c_char,
                        visible: bool,
                    );
                }
                let cocoa = window.symbol::<unsafe extern "C" fn(
                    *mut std::ffi::c_void,
                ) -> *mut std::ffi::c_void>(
                    b"glfwGetCocoaWindow\0"
                )?(window.handle);
                ww_material_hud(cocoa, title.as_ptr(), show_hud);
            }
        }
        Ok(())
    }
    pub(super) fn environment(&mut self, shader: &str) {
        let clear = match shader {
            "planet_surface" => [0.48, 0.62, 0.72, 1.],
            _ => [0.035, 0.045, 0.065, 1.],
        };
        let diagnostic = shader != "planet_surface" && shader != "workstation_panel";
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
    pub(super) fn require_world_backdrop(&self)->Result<(),String>{match &self.0{Backend::WebGPU(c)=>c.require_world_backdrop(),_=>Err("retained world images require the baked WebGPU workstation".into())}}
    pub(super) fn set_workstation(&mut self, rows: &[[f32; 4]]) -> Result<(), String> {
        workstation::validate_frame(rows)?;
        unsafe {
            match &mut self.0 {
                Backend::OpenGL(c) => c.set_workstation(rows),
                Backend::Vulkan(c) => c.set_workstation(rows),
                Backend::WebGPU(c) => c.set_workstation(rows),
            }
        }?;
        self.1.invalidate();
        Ok(())
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
    pub(super) fn pump(&mut self, force_draw: bool, capture_on_exit: bool) -> Result<Value, String> {
        unsafe {
            let started = force_draw.then(std::time::Instant::now);
            if let Backend::WebGPU(c)=&mut self.0{c.validation_prepare()?;}
            match &mut self.0 {
                Backend::OpenGL(c) => c.poll_performance(),
                Backend::WebGPU(c) => c.poll_performance()?,
                Backend::Vulkan(_) => {}
            }
            let backend_polled = started.map(|_| std::time::Instant::now());
            let window = match &mut self.0 {
                Backend::OpenGL(c) => &mut c.window,
                Backend::Vulkan(c) => &mut c.window,
                Backend::WebGPU(c) => &mut c.window,
            };
            window.show();
            let closed = window.closed();
            let events_polled = started.map(|_| std::time::Instant::now());
            let keys = window.keys()?;
            let flight = window.flight_input()?;
            // Return input to the controller before issuing another costly draw.
            // Escape still belongs to the editor/gesture first; this does not close it here.
            let input_preempts_draw=!capture_on_exit && input_safety::input_preempts_draw(self.1.workstation,flight["quit"]==true,window.escape_pressed());
            let input_observed=self.1.workstation.then(std::time::Instant::now);
            let exposed = window.take_refresh();
            let iconified = !closed && self.1.workstation && window.iconified()?;
            let mut drawable = None;
            let mut logical = None;
            let dimensions = if closed || iconified {
                None
            } else if self.1.workstation {
                drawable = window.available_dimensions()?;
                logical = window.available_logical_dimensions()?;
                drawable.zip(logical)
                    .map(|((w, h), (lw, lh))| (w, h, lw, lh))
            } else {
                let (w, h) = window.dimensions()?;
                let (lw, lh) = window.logical_dimensions()?;
                drawable = Some((w,h));
                logical = Some((lw,lh));
                Some((w, h, lw, lh))
            };
            let display_cadence=if self.1.workstation {window.display_cadence().info()}else{Value::Null};
            let window_ready = started.map(|_| std::time::Instant::now());
            if let (Some(observed),Backend::WebGPU(c))=(input_observed,&mut self.0){c.observe_workstation_input(observed);}
            let retained_refresh=matches!(&self.0,Backend::WebGPU(c) if c.needs_redraw());
            let focus_deferred = self.1.defer_unfocused(flight["focused"] == true, force_draw);
            let drawn = !closed && !input_preempts_draw && !focus_deferred
                && self.1.requested(dimensions, exposed, force_draw||retained_refresh);
            let eligibility_time=crate::workstation::frame_cadence::timeline_enabled().then(std::time::Instant::now);
            if drawn {
                match &mut self.0 {
                    Backend::OpenGL(c) => c.draw()?,
                    Backend::Vulkan(c) => c.draw()?,
                    Backend::WebGPU(c) => c.draw()?,
                }
                // Failed submissions retain invalidation for a later retry.
                self.1.presented();
            } else if self.1.workstation || self.1.paused_game {
                // Draw deferral does not stop the control side publishing GPU
                // writes. Complete their staging lifetime without presenting.
                if let Backend::WebGPU(c)=&mut self.0 {c.drain_deferred_uploads()?;}
            }
            match &self.0 {
                Backend::OpenGL(c)=>c.window.preview_rate(drawn,flight["focused"]==true),
                Backend::Vulkan(c)=>c.window.preview_rate(drawn,flight["focused"]==true),
                Backend::WebGPU(c)=>c.window.preview_rate(drawn,flight["focused"]==true),
            }
            let mut report = json!({"closed":closed,"keys":keys,"drawn":drawn,
                "suspended":!closed && dimensions.is_none(),"device":self.info(),"flight":flight,"input_preempts_draw":input_preempts_draw});
            if let Some(eligible)=eligibility_time{
                let stamp=crate::workstation::frame_cadence::timeline_stamp;
                report["pump_timeline"]=json!({"begin_ms":started.map(stamp),"backend_polled_ms":backend_polled.map(stamp),"events_polled_ms":events_polled.map(stamp),"window_ready_ms":window_ready.map(stamp),"eligibility_checked_ms":stamp(eligible),"after_draw_report_ms":stamp(std::time::Instant::now()),"drawn":drawn,"forced":force_draw,"retained_refresh":retained_refresh});
            }
            report["display_cadence"]=display_cadence;
            report["draw_status"] = draw_status(closed,iconified,drawable,logical,drawn);
            report["draw_status"]["focus_deferred"] = json!(focus_deferred);
            if focus_deferred && !closed && !iconified && dimensions.is_some() {
                report["draw_status"]["reason"] = json!("unfocused");
                report["draw_status"]["recoverable"] = json!(true);
            }
            report["draw_status"]["forced"] = json!(force_draw);
            report["draw_status"]["exposed"] = json!(exposed);
            report["draw_status"]["surface_acquire"] = match &self.0 {
                Backend::WebGPU(c) if drawn => c.info["surface_acquire"].clone(),
                _ => Value::Null,
            };
            report["dimensions_source"] = json!(if dimensions.is_some() { "current" }
                else if self.1.dimensions.is_some() { "previous" } else { "unavailable" });
            if let (Some(start),Some(polled),Some(events),Some(ready)) =
                (started,backend_polled,events_polled,window_ready) {
                let end=std::time::Instant::now();
                report["pump_timing_ms"] = json!({
                    "backend_poll":(polled-start).as_secs_f64()*1000.,
                    "window_events":(events-polled).as_secs_f64()*1000.,
                    "window_state":(ready-events).as_secs_f64()*1000.,
                    "draw_and_report":(end-ready).as_secs_f64()*1000.,
                    "total":(end-start).as_secs_f64()*1000.});
            }
            if let Some((width, height, logical_width, logical_height)) =
                dimensions.or(self.1.dimensions)
            {
                report["aspect"] = json!(width as f64 / height as f64);
                report["logical_width"] = json!(logical_width);
                report["logical_height"] = json!(logical_height);
                report["width"] = json!(width);
                report["height"] = json!(height);
            }
            Ok(report)
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
    fn monitor_capture(&mut self) -> &mut Option<Vec<u8>> {
        match &mut self.0 {
            Backend::OpenGL(c) => &mut c.monitor_capture,
            Backend::Vulkan(c) => &mut c.monitor_capture,
            Backend::WebGPU(c) => &mut c.monitor_capture,
        }
    }
    pub(super) fn capture_exit_frame(&mut self) -> Result<(Value, Vec<u8>), String> {
        self.capture_next();
        *self.monitor_capture() = Some(Vec::new());
        let report = self.pump(true, true);
        let image = self.monitor_capture().take().unwrap();
        let report = report?;
        if report["drawn"] != true || image.is_empty() { return Err("exit frame was not drawn".into()); }
        Ok((report, image))
    }
    pub(super) fn capture_next(&mut self) {
        self.1.invalidate();
        match &mut self.0 {
            Backend::OpenGL(c) => c.captured = false,
            Backend::Vulkan(c) => c.captured = false,
            Backend::WebGPU(c) => c.captured = false,
        };
    }
    pub(super) fn park(&mut self)->Result<(),String>{
        self.input_mode(true)?;
        unsafe { match &self.0 {
            Backend::OpenGL(c)=>c.window.hide(), Backend::Vulkan(c)=>c.window.hide(), Backend::WebGPU(c)=>c.window.hide(),
        }}
    }
    /// Render the resident DAW without polling input, showing a window or presenting.
    pub(super) fn hidden_scene(&mut self)->Result<(),String>{
        unsafe {match &mut self.0 {Backend::WebGPU(c)=>c.draw_hidden_scene(),_=>Err("hidden preload requires WebGPU".into())}}
    }
    pub(super) fn background_capture(&mut self)->Result<Vec<u8>,String>{
        self.capture_next();
        *self.monitor_capture()=Some(Vec::new());
        let result=unsafe {match &mut self.0 {
            Backend::WebGPU(c)=>c.draw_background(),
            _=>Err("resident display requires WebGPU".into()),
        }};
        let image=self.monitor_capture().take().unwrap_or_default();
        result?;
        if image.is_empty(){return Err("resident display readback is empty".into());}
        Ok(image)
    }
    pub(super) fn gpu_draw_count(&self)->u64 {match &self.0{Backend::WebGPU(c)=>c.gpu_draw_count(),_=>0}}
    pub(super) fn finish(&mut self) -> Result<(), String> {
        unsafe {
            match &mut self.0 {
                Backend::OpenGL(c) => c.wait(),
                Backend::Vulkan(c) => c.wait(),
                Backend::WebGPU(c) => c.snapshot_performance(),
            }
        }
    }
}

#[cfg(test)]
mod redraw_tests {
    use super::Redraw;
    const SIZE: Option<super::Dimensions> = Some((1280, 720, 1280., 720.));

    #[test]
    fn initial_inactive_startup_preserves_first_frame_until_activation() {
        let mut redraw=Redraw::new(true);
        for _ in 0..4 {
            assert!(redraw.defer_unfocused(false,false));
            assert!(redraw.dirty);assert_eq!(redraw.dimensions,None);
        }
        // Native activation arrives independently of rendering. No inactive
        // acquisition is needed to obtain the first focused frame.
        assert!(!redraw.defer_unfocused(true,false));
        assert!(redraw.requested(SIZE,false,false));
        redraw.presented();assert!(!redraw.requested(SIZE,false,false));
        assert!(redraw.defer_unfocused(false,false));
        assert!(!redraw.defer_unfocused(true,false));
        assert!(redraw.requested(SIZE,false,false));
    }
    #[test]
    fn pending_quit_preempts_startup_and_retained_refresh() {
        for focused in [false,true] {
            for retained_refresh in [false,true] {
                let mut redraw=Redraw::new(true);
                let mut input=super::input_safety::BufferedKeys::default();
                input.record(81,1,8,true);input.record(81,0,0,true);
                if !focused {input.blur();}
                let preempts=super::input_safety::input_preempts_draw(true,input.take_quit(),false);
                let deferred=redraw.defer_unfocused(focused,false);
                let drawn=!preempts && !deferred && redraw.requested(SIZE,false,retained_refresh);
                assert!(!drawn);assert!(redraw.dirty);assert_eq!(redraw.dimensions,None);
            }
        }
    }

    #[test]
    fn unfocused_workstation_retains_redraw_until_focus_returns() {
        let mut redraw = Redraw::new(true);
        assert!(redraw.requested(SIZE, false, false));
        redraw.presented();
        let resized = Some((960, 600, 960., 600.));
        let mut draws = 0;
        for _ in 0..100 {
            // Exposure, resize and pending retained refresh cannot bypass blur.
            if !redraw.defer_unfocused(false, false)
                && redraw.requested(resized, true, true) { draws += 1; }
        }
        assert_eq!(draws, 0);
        assert!(redraw.dirty);
        assert_eq!(redraw.dimensions, SIZE);
        assert!(!redraw.defer_unfocused(true, false));
        assert!(redraw.requested(resized, false, false));
        assert!(redraw.requested(resized, false, false)); // Unsubmitted debt survives.
        redraw.presented();
        assert!(!redraw.requested(resized, false, false));
    }
    #[test]
    fn focus_deferral_does_not_change_forced_diagnostics_or_game_draws() {
        let mut workstation = Redraw::new(true);
        assert!(!workstation.defer_unfocused(false, true));
        assert!(workstation.requested(SIZE, false, true));
        let mut game = Redraw::new(false);
        assert!(!game.defer_unfocused(false, false));
        assert!(game.requested(SIZE, false, false));
    }
    #[test]
    fn focus_return_after_minimize_preserves_pending_capture() {
        let mut redraw = Redraw::new(true);
        assert!(redraw.requested(SIZE, false, false)); redraw.presented();
        redraw.invalidate();
        assert!(!redraw.requested(None, true, false));
        assert!(redraw.defer_unfocused(false, false));
        assert!(!redraw.defer_unfocused(true, false));
        assert!(redraw.requested(SIZE, false, false));
        redraw.presented(); assert!(!redraw.requested(SIZE, false, false));
    }

    #[test]
    fn nondrawable_recovery_only_labels_observed_window_gates() {
        let drawable=Some((1280,720));let logical=Some((1280.,720.));
        for (closed,iconified,pixels,points,drawn,reason,recoverable) in [
            (true,false,None,None,false,"window_closed",false),
            (false,true,None,None,false,"iconified",true),
            (false,false,None,None,false,"zero_logical_dimensions",true),
            (false,false,None,logical,false,"zero_framebuffer",true),
            (false,false,drawable,None,false,"zero_logical_dimensions",true),
            (false,false,drawable,logical,false,"redraw_not_requested",false),
            (false,false,drawable,logical,true,"presented",false),
        ] {
            let status=super::draw_status(closed,iconified,pixels,points,drawn);
            assert_eq!(status["reason"],reason);
            assert_eq!(status["recoverable"],recoverable);
            assert_eq!(status["attempted"],drawn);
        }
    }

    #[test]
    fn workstation_idle_skips_gpu_work_but_diagnostics_can_force_it() {
        let mut redraw = Redraw::new(true);
        assert!(redraw.requested(SIZE, false, false));
        redraw.presented();
        assert!(!redraw.requested(SIZE, false, false));
        assert!(redraw.requested(SIZE, false, true));
        redraw.presented();
        assert!(!redraw.requested(SIZE, false, false));
        redraw.invalidate(); // Optical update or explicit screenshot.
        assert!(redraw.requested(SIZE, false, false));
        assert!(redraw.requested(SIZE, false, false)); // No successful draw yet.
    }

    #[test]
    fn workstation_expose_resize_and_restore_repaint_without_dropping_pending_capture() {
        let mut redraw = Redraw::new(true);
        redraw.requested(SIZE, false, false);
        redraw.presented();
        assert!(redraw.requested(SIZE, true, false));
        redraw.presented();
        assert!(redraw.requested(Some((1920, 1080, 1920., 1080.)), false, false));
        redraw.presented();
        redraw.invalidate();
        assert!(!redraw.requested(None, false, true));
        assert!(redraw.requested(SIZE, false, false));
        redraw.presented();
        assert!(!redraw.requested(SIZE, false, false));
    }

    #[test]
    fn workstation_gate_keeps_other_scenes_rendering_each_tick() {
        let mut redraw = Redraw::new(false);
        for _ in 0..3 {
            assert!(redraw.requested(SIZE, false, false));
            redraw.presented();
        }
    }
}

#[path="TheatreExteriorCheck.rs"]
mod theatre_exterior_check;
#[path="NightSkyCheck.rs"]
mod night_sky_check;
pub(super) fn check_theatre_exterior(root:&Path,output:&Path,backend:&str)->Result<(),String>{theatre_exterior_check::run(root,output,backend)}

#[path="FlashlightCheck.rs"]
mod flashlight_check;
pub(super) fn check_flashlight(root:&Path,output:&Path,backend:&str)->Result<(),String>{flashlight_check::run(root,output,backend)}

#[path="TreeCheck.rs"]
mod tree_check;
pub(super) fn check_trees(root:&Path,output:&Path,backend:&str)->Result<(),String>{tree_check::run(root,output,backend)}

#[path="NightworldCheck.rs"]
mod nightworld_check;

#[path="RegionalCheck.rs"]
mod regional_check;

#[path="MonasteryCheck.rs"]
mod monastery_check;
#[path="LaboratoryCheck.rs"]
mod laboratory_check;
