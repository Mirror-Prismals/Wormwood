#![allow(non_snake_case)]
use super::window::Window;
use serde_json::{Value, json};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::Path,
    ptr,
};
macro_rules! functions {
    ($($name:ident : ($($arg:ty),*) $(-> $ret:ty)?;)+) => {
        struct Gl {$( $name:unsafe extern "system" fn($($arg),*) $(->$ret)?, )+}
        impl Gl {unsafe fn load(window:&Window)->Result<Self,String>{Ok(Self{$($name:std::mem::transmute::<*const c_void,unsafe extern "system" fn($($arg),*) $(->$ret)?>(window.gl_proc(CString::new(concat!("gl",stringify!($name))).unwrap().as_c_str())?),)+})}}
    }
}
functions! {
    ActiveTexture:(u32);Enable:(u32);BlendFunc:(u32,u32);
    TexSubImage2D:(u32,i32,i32,i32,i32,i32,u32,u32,*const c_void);
    GenTextures:(i32,*mut u32);DeleteTextures:(i32,*const u32);BindTexture:(u32,u32);TexParameteri:(u32,u32,i32);TexImage2D:(u32,i32,i32,i32,i32,i32,u32,u32,*const c_void);
    GetString:(u32)->*const u8; GetError:()->u32;
    CreateShader:(u32)->u32; ShaderSource:(u32,i32,*const *const c_char,*const i32);
    CompileShader:(u32);GetShaderiv:(u32,u32,*mut i32);GetShaderInfoLog:(u32,i32,*mut i32,*mut c_char);DeleteShader:(u32);
    CreateProgram:()->u32;AttachShader:(u32,u32);LinkProgram:(u32);GetProgramiv:(u32,u32,*mut i32);GetProgramInfoLog:(u32,i32,*mut i32,*mut c_char);DeleteProgram:(u32);UseProgram:(u32);
    GetUniformLocation:(u32,*const c_char)->i32;Uniform1i:(i32,i32);Uniform4fv:(i32,i32,*const f32);BindSampler:(u32,u32);
    GenVertexArrays:(i32,*mut u32);BindVertexArray:(u32);DeleteVertexArrays:(i32,*const u32);
    GenBuffers:(i32,*mut u32);DeleteBuffers:(i32,*const u32);BindBuffer:(u32,u32);BufferData:(u32,isize,*const c_void,u32);
    EnableVertexAttribArray:(u32);VertexAttribPointer:(u32,i32,u32,u8,i32,*const c_void);
    BindFramebuffer:(u32,u32);
    Viewport:(i32,i32,i32,i32);ClearColor:(f32,f32,f32,f32);Clear:(u32);DrawArrays:(u32,i32,i32);Finish:();
    PixelStorei:(u32,i32);ReadPixels:(i32,i32,i32,i32,u32,u32,*mut c_void);
}
// Cardinal-style delayed query retirement: never wait for an unavailable
// interactive sample, and never overwrite a query that the GPU still owns.
struct TimerApi {
    generate: unsafe extern "system" fn(i32, *mut u32),
    delete: unsafe extern "system" fn(i32, *const u32),
    begin: unsafe extern "system" fn(u32, u32),
    end: unsafe extern "system" fn(u32),
    available: unsafe extern "system" fn(u32, u32, *mut i32),
    result: unsafe extern "system" fn(u32, u32, *mut u64),
}
impl TimerApi {
    unsafe fn load(window: &Window, gl: &Gl) -> Option<Self> {
        // Resolve optional functions separately so older contexts remain usable.
        let get = std::mem::transmute::<*const c_void, unsafe extern "system" fn(u32, u32, *mut i32)>(
            window.gl_proc(c"glGetQueryiv").ok()?,
        );
        let mut bits = 0;
        get(0x88bf, 0x8864, &mut bits); // TIME_ELAPSED / QUERY_COUNTER_BITS
        if (gl.GetError)() != 0 || !(1..=64).contains(&bits) {
            return None;
        }
        Some(Self {
            generate: std::mem::transmute::<*const c_void, unsafe extern "system" fn(i32, *mut u32)>(
                window.gl_proc(c"glGenQueries").ok()?,
            ),
            delete: std::mem::transmute::<*const c_void, unsafe extern "system" fn(i32, *const u32)>(
                window.gl_proc(c"glDeleteQueries").ok()?,
            ),
            begin: std::mem::transmute::<*const c_void, unsafe extern "system" fn(u32, u32)>(
                window.gl_proc(c"glBeginQuery").ok()?,
            ),
            end: std::mem::transmute::<*const c_void, unsafe extern "system" fn(u32)>(
                window.gl_proc(c"glEndQuery").ok()?,
            ),
            available: std::mem::transmute::<
                *const c_void,
                unsafe extern "system" fn(u32, u32, *mut i32),
            >(window.gl_proc(c"glGetQueryObjectiv").ok()?),
            result: std::mem::transmute::<
                *const c_void,
                unsafe extern "system" fn(u32, u32, *mut u64),
            >(window.gl_proc(c"glGetQueryObjectui64v").ok()?),
        })
    }
}
struct GlTiming {
    api: Option<TimerApi>,
    queries: [u32; 8],
    pending: [u64; 8],
    active: Option<usize>,
    serial: u64,
    log: super::performance::GpuLog,
}
impl GlTiming {
    unsafe fn new(window: &Window, gl: &Gl, output: &Path, workstation: bool) -> Self {
        let mut api = TimerApi::load(window, gl);
        let mut queries = [0; 8];
        if let Some(functions) = &api {
            (functions.generate)(8, queries.as_mut_ptr());
            let error = (gl.GetError)();
            if queries.contains(&0) || error != 0 {
                (functions.delete)(8, queries.as_ptr());
                queries = [0; 8];
                api = None;
            }
        }
        let log = super::performance::GpuLog::new(
            output,
            "OpenGL",
            workstation,
            api.is_some(),
            "async_timestamp",
            "GL_TIME_ELAPSED over all drawing; excludes screenshot readback and swap.",
        );
        Self {
            api,
            queries,
            pending: [0; 8],
            active: None,
            serial: 0,
            log,
        }
    }
    unsafe fn poll(&mut self) {
        let Some(api) = &self.api else {
            return;
        };
        for slot in 0..self.pending.len() {
            if self.pending[slot] == 0 {
                continue;
            }
            let mut available = 0;
            (api.available)(self.queries[slot], 0x8867, &mut available);
            if available != 0 {
                let mut nanoseconds = 0;
                (api.result)(self.queries[slot], 0x8866, &mut nanoseconds);
                self.log
                    .record(self.pending[slot], nanoseconds as f64 / 1_000_000.);
                self.pending[slot] = 0;
            }
        }
    }
    unsafe fn begin(&mut self) {
        self.poll();
        self.serial += 1;
        self.log.submitted(self.serial);
        let Some(api) = &self.api else {
            return;
        };
        if let Some(slot) = self.pending.iter().position(|serial| *serial == 0) {
            self.active = Some(slot);
            (api.begin)(0x88bf, self.queries[slot]);
        } else {
            self.log.missing(self.serial);
        }
    }
    unsafe fn end(&mut self) {
        if let Some(slot) = self.active.take() {
            (self.api.as_ref().unwrap().end)(0x88bf);
            self.pending[slot] = self.serial;
        }
    }
}
impl Drop for GlTiming {
    fn drop(&mut self) {
        unsafe {
            self.end();
            self.poll();
            if let Some(api) = &self.api {
                (api.delete)(8, self.queries.as_ptr());
            }
        }
        self.log.finish();
    }
}
pub(super) struct Client {
    pub window: Window,
    gl: Gl,
    water: Option<super::water::opengl::Client>,
    water_targets: Option<super::water::opengl::SceneTargets>,
    water_frames: Vec<super::water::data::DrawFrame>,
    reflection_vertices: Vec<f32>,
    reflection_count: i32,
    reflection_vegetation: Option<super::vegetation::opengl::Client>,
    reflected_plants: Option<crate::vegetation::packet::Packet>,
    vegetation: Option<super::vegetation::opengl::Client>,
    plant_packet: Option<crate::vegetation::packet::Packet>,
    cloud_noise: u32,
    program: u32,
    world_program:u32,
    world_active:bool,
    vao: u32,
    vbo: u32,
    texture_id: u32,
    pub info: Value,
    capture: std::path::PathBuf,
    pub(super) captured: bool,
    pub(super) monitor_capture: Option<Vec<u8>>,
    pub(super) validate_triangle_center: bool,
    vertices: Vec<f32>,
    vertex_count: i32,
    pub(super) clear: [f32; 4],
    gpu_timing: Option<GlTiming>,
}
impl Client {
    pub unsafe fn new(
        root: &Path,
        data: &[f32; 24],
        output: &Path,
        workstation: bool,
    ) -> Result<Self, String> {
        let window = Window::new(true, c"Wormwood — OpenGL", workstation)?;
        let gl = Gl::load(&window)?;
        let version = (gl.GetString)(0x1f02);
        if version.is_null() {
            return Err("OpenGL version unavailable".into());
        }
        let renderer = (gl.GetString)(0x1f01);
        let info = json!({"api":"OpenGL","version":CStr::from_ptr(version.cast()).to_string_lossy(),"readback_verified":false,"renderer":if renderer.is_null(){"unknown".into()}else{CStr::from_ptr(renderer.cast()).to_string_lossy()}});
        let mut client = Self {
            window,
            gl,
            water: None,
            water_targets: None,
            water_frames: Vec::new(),
            reflection_vertices: Vec::new(),
            reflection_count: 0,
            reflection_vegetation: None,
            reflected_plants: None,
            vegetation: None,
            plant_packet: None,
            cloud_noise: 0,
            program: 0,
            world_program:0,world_active:false,
            vao: 0,
            vbo: 0,
            texture_id: 0,
            info,
            capture: output.join("triangle-OpenGL.ppm"),
            captured: false,
            monitor_capture: None,
            validate_triangle_center: true,
            vertices: Vec::new(),
            vertex_count: 0,
            clear: [0.035, 0.045, 0.065, 1.],
            gpu_timing: None,
        };
        let vertex = client.shader(
            0x8b31,
            &std::fs::read_to_string(root.join(if workstation {
                "Procedures/Shaders/workstation.gl.vert"
            } else {
                "Procedures/Shaders/triangle.gl.vert"
            }))
            .map_err(|e| e.to_string())?,
        )?;
        let fragment = match client.shader(
            0x8b30,
            &crate::workstation::style_shader::source(root, if workstation {
                "Procedures/Shaders/workstation.gl.frag"
            } else {
                "Procedures/Shaders/triangle.gl.frag"
            })?,
        ) {
            Ok(s) => s,
            Err(e) => {
                (client.gl.DeleteShader)(vertex);
                return Err(e);
            }
        };
        client.program = (client.gl.CreateProgram)();
        (client.gl.AttachShader)(client.program, vertex);
        (client.gl.AttachShader)(client.program, fragment);
        (client.gl.LinkProgram)(client.program);
        (client.gl.DeleteShader)(vertex);
        (client.gl.DeleteShader)(fragment);
        let mut success = 0;
        (client.gl.GetProgramiv)(client.program, 0x8b82, &mut success);
        if success == 0 {
            let mut log = vec![0u8; 4096];
            let mut length = 0;
            (client.gl.GetProgramInfoLog)(
                client.program,
                4096,
                &mut length,
                log.as_mut_ptr().cast(),
            );
            return Err(format!(
                "OpenGL link: {}",
                String::from_utf8_lossy(&log[..length.max(0) as usize])
            ));
        }
        (client.gl.GenVertexArrays)(1, &mut client.vao);
        (client.gl.BindVertexArray)(client.vao);
        (client.gl.GenBuffers)(1, &mut client.vbo);
        (client.gl.BindBuffer)(0x8892, client.vbo); // GL_ARRAY_BUFFER
        (client.gl.EnableVertexAttribArray)(0);
        (client.gl.VertexAttribPointer)(0, 4, 0x1406, 0, 32, ptr::null());
        (client.gl.EnableVertexAttribArray)(1);
        (client.gl.VertexAttribPointer)(1, 4, 0x1406, 0, 32, 16usize as *const c_void);
        client.set_batch(std::slice::from_ref(data))?;
        client.texture(&[1, 0, 0, 0, 1, 0, 0, 0, 255, 255, 255, 255])?;
        let noise = crate::scene_assets::gl_atlas();
        (client.gl.GenTextures)(1, &mut client.cloud_noise);
        (client.gl.ActiveTexture)(0x84c6);
        (client.gl.BindTexture)(0x0de1, client.cloud_noise);
        for (key, value) in [
            (0x2801, 0x2600),
            (0x2800, 0x2600),
            (0x2802, 0x812f),
            (0x2803, 0x812f),
        ] {
            (client.gl.TexParameteri)(0x0de1, key, value);
        }
        (client.gl.TexImage2D)(
            0x0de1,
            0,
            0x8058,
            528,
            (noise.len() / (528 * 4)) as i32,
            0,
            0x1908,
            0x1401,
            noise.as_ptr().cast(),
        );
        (client.gl.ActiveTexture)(0x84c0);
        if workstation {
            client.set_workstation(&super::workstation::initial(root)?)?;
        }
        client.wait()?;
        if super::performance::enabled() {
            client.gpu_timing = Some(GlTiming::new(
                &client.window,
                &client.gl,
                output,
                workstation,
            ));
        }
        Ok(client)
    }
    unsafe fn shader(&self, kind: u32, source: &str) -> Result<u32, String> {
        let source = CString::new(source).map_err(|e| e.to_string())?;
        let shader = (self.gl.CreateShader)(kind);
        let pointer = source.as_ptr();
        (self.gl.ShaderSource)(shader, 1, &pointer, ptr::null());
        (self.gl.CompileShader)(shader);
        let mut status = 0;
        (self.gl.GetShaderiv)(shader, 0x8b81, &mut status);
        if status == 0 {
            let mut log = vec![0u8; 4096];
            let mut length = 0;
            (self.gl.GetShaderInfoLog)(shader, 4096, &mut length, log.as_mut_ptr().cast());
            (self.gl.DeleteShader)(shader);
            return Err(format!(
                "OpenGL shader: {}",
                String::from_utf8_lossy(&log[..length.max(0) as usize])
            ));
        }
        Ok(shader)
    }
    pub unsafe fn set_world_backdrop(&mut self,root:&Path,enabled:bool)->Result<bool,String>{
        let changed=self.world_active!=enabled;
        if enabled && self.world_program==0 {
            let vertex=self.shader(0x8b31,&std::fs::read_to_string(root.join("Procedures/Shaders/triangle.gl.vert")).map_err(|e|e.to_string())?)?;
            let fragment=self.shader(0x8b30,&std::fs::read_to_string(root.join("Procedures/Shaders/triangle.gl.frag")).map_err(|e|e.to_string())?)?;
            let program=(self.gl.CreateProgram)();
            (self.gl.AttachShader)(program,vertex);(self.gl.AttachShader)(program,fragment);(self.gl.LinkProgram)(program);
            (self.gl.DeleteShader)(vertex);(self.gl.DeleteShader)(fragment);
            let mut success=0;(self.gl.GetProgramiv)(program,0x8b82,&mut success);
            if success==0{(self.gl.DeleteProgram)(program);return Err("live world camera program link failed".into());}
            self.world_program=program;
        }
        if self.world_active!=enabled {self.water_frames.clear();self.reflection_count=0;}
        self.world_active=enabled;Ok(changed)
    }
    fn scene_program(&self)->u32{if self.world_active{self.world_program}else{self.program}}
    pub unsafe fn set_water(
        &mut self,
        root: &Path,
        packets: &[crate::water::presentation::Packet],
        view: &Value,
        triangles: &[[f32; 24]],
    ) -> Result<(), String> {
        self.table_eye(view)?;
        let cloud_frame = crate::base_system::layer_four::cloud_system::read_frame(view)?;
        (self.gl.UseProgram)(self.scene_program());
        (self.gl.Uniform1i)(
            (self.gl.GetUniformLocation)(self.scene_program(), c"cloudNoiseAtlas".as_ptr()),
            6,
        );
        (self.gl.ActiveTexture)(0x84c6);
        (self.gl.BindTexture)(0x0de1, self.cloud_noise);
        (self.gl.BindSampler)(6, 0);
        (self.gl.ActiveTexture)(0x84c0);
        (self.gl.Uniform4fv)(
            (self.gl.GetUniformLocation)(self.scene_program(), c"cloudFrame".as_ptr()),
            12,
            cloud_frame.as_ptr().cast(),
        );

        self.water_frames.clear();
        self.reflection_vertices.clear();
        self.reflection_count = 0;
        if packets.is_empty() {
            return Ok(());
        }
        let (w, h) = self.window.dimensions()?;
        self.water_frames = super::scene::water_frames(packets, view, w, h, true)?;
        if self.water.is_none() {
            let window = &self.window;
            let mut resolve = |name: &CStr| window.gl_proc(name).unwrap_or(ptr::null());
            self.water = Some(super::water::opengl::Client::new(root, &mut resolve)?);
            self.water_targets = Some(super::water::opengl::SceneTargets::new(&mut resolve)?);
        }
        self.water_targets.as_mut().unwrap().resize(w, h)?;
        if let Some(frame) = self
            .water_frames
            .iter()
            .find(|f| f.optics.planar_reflection)
        {
            let reflected = super::scene::reflected_batch(triangles, frame)?;
            self.reflection_count = (reflected.len() * 3) as i32;
            for triangle in reflected {
                for i in 0..3 {
                    self.reflection_vertices
                        .extend_from_slice(&triangle[i * 4..i * 4 + 4]);
                    self.reflection_vertices
                        .extend_from_slice(&triangle[12 + i * 4..16 + i * 4]);
                }
            }
        }
        Ok(())
    }
    pub unsafe fn set_vegetation(
        &mut self,
        root: &Path,
        packet: Option<crate::vegetation::packet::Packet>,
    ) -> Result<(), String> {
        if packet.is_some() && self.vegetation.is_none() {
            let window = &self.window;
            self.vegetation = Some(super::vegetation::opengl::Client::new(root, &mut |name| {
                window.gl_proc(name).unwrap_or(ptr::null())
            })?);
        }
        self.reflected_plants = if let (Some(packet), Some(frame)) = (
            &packet,
            self.water_frames
                .iter()
                .find(|f| f.optics.planar_reflection),
        ) {
            Some(super::scene::reflected_plants(packet, frame)?)
        } else {
            None
        };
        if self.reflected_plants.is_some() && self.reflection_vegetation.is_none() {
            let window = &self.window;
            self.reflection_vegetation =
                Some(super::vegetation::opengl::Client::new(root, &mut |name| {
                    window.gl_proc(name).unwrap_or(ptr::null())
                })?);
        }
        self.plant_packet = packet;
        Ok(())
    }
    pub unsafe fn set_workstation(&mut self, rows: &[[f32; 4]]) -> Result<(), String> {
        let complete=crate::workstation::deferred_floor::complete(rows)?;
        let rows=complete.as_ref();
        if rows.is_empty() || rows.len() % 256 != 0 {
            return Err("unaligned workstation optical texture".into());
        }
        (self.gl.ActiveTexture)(0x84c0);
        (self.gl.BindTexture)(0x0de1, self.texture_id);
        (self.gl.TexParameteri)(0x0de1, 0x2801, 0x2600);
        (self.gl.TexParameteri)(0x0de1, 0x2800, 0x2600);
        if rows.len() == super::workstation::FRAME_ROWS {
            for (offset, range) in super::workstation::upload_ranges(rows) {
                (self.gl.TexSubImage2D)(0x0de1, 0, 0,
                    (offset / super::workstation::TEXTURE_WIDTH) as i32,
                    super::workstation::TEXTURE_WIDTH as i32,
                    (range.len() / super::workstation::TEXTURE_WIDTH) as i32,
                    0x1908, 0x1406, range.as_ptr().cast());
            }
            let width = super::workstation::TEXTURE_WIDTH;
            let remainder = rows.len() % width;
            if remainder != 0 {
                (self.gl.TexSubImage2D)(0x0de1, 0, 0, (rows.len()/width) as i32,
                    remainder as i32, 1, 0x1908, 0x1406,
                    rows.as_ptr().add(rows.len()-remainder).cast());
            }
        } else {
            (self.gl.TexImage2D)(
                0x0de1,
                0,
                0x8814,
                super::workstation::TEXTURE_WIDTH as i32,
                (rows.len() / super::workstation::TEXTURE_WIDTH) as i32,
                0,
                0x1908,
                0x1406,
                rows.as_ptr().cast(),
            );
        }
        (self.gl.UseProgram)(self.program);
        (self.gl.Uniform1i)(
            (self.gl.GetUniformLocation)(self.program, c"workstationData".as_ptr()),
            0,
        );
        self.wait()
    }
    pub unsafe fn table_eye(&mut self, view:&Value)->Result<(),String>{
        let eye=crate::scene_assets::table_eye(view)?;
        (self.gl.UseProgram)(self.scene_program());
        let light=crate::scene_assets::exterior_light(view);
        (self.gl.Uniform4fv)((self.gl.GetUniformLocation)(self.scene_program(),c"exteriorLight".as_ptr()),2,light.as_ptr());
        (self.gl.Uniform4fv)((self.gl.GetUniformLocation)(self.scene_program(),c"tableEye".as_ptr()),1,eye.as_ptr());Ok(())
    }
    pub unsafe fn monitor_image(&mut self, image: &[u8]) -> Result<(), String> {
        let mut atlas = crate::scene_assets::CLOUD_NOISE[8..].to_vec();
        atlas.extend_from_slice(crate::scene_assets::FINISHES);
        atlas.extend_from_slice(image);
        atlas.extend_from_slice(&crate::scene_assets::TABLE[16..]);
        atlas.resize(atlas.len().div_ceil(528*4)*528*4,0);
        (self.gl.ActiveTexture)(0x84c6);
        (self.gl.BindTexture)(0x0de1,self.cloud_noise);
        (self.gl.TexImage2D)(0x0de1,0,0x8058,528,(atlas.len()/(528*4)) as i32,0,0x1908,0x1401,atlas.as_ptr().cast());
        (self.gl.ActiveTexture)(0x84c0);
        Ok(())
    }
    pub unsafe fn texture(&mut self, bytes: &[u8]) -> Result<(), String> {
        let w = u32::from_le_bytes(bytes[..4].try_into().unwrap());
        let h = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
        if self.texture_id == 0 {
            (self.gl.GenTextures)(1, &mut self.texture_id);
        }
        (self.gl.BindTexture)(0x0de1, self.texture_id);
        (self.gl.TexParameteri)(0x0de1, 0x2801, 0x2601);
        (self.gl.TexParameteri)(0x0de1, 0x2800, 0x2601);
        (self.gl.TexParameteri)(0x0de1, 0x2802, 0x812f);
        (self.gl.TexParameteri)(0x0de1, 0x2803, 0x812f);
        (self.gl.TexImage2D)(
            0x0de1,
            0,
            0x8058,
            w as i32,
            h as i32,
            0,
            0x1908,
            0x1401,
            bytes.as_ptr().add(8).cast(),
        );
        self.wait()
    }
    pub unsafe fn set_batch(&mut self, triangles: &[[f32; 24]]) -> Result<(), String> {
        if triangles.len() > crate::scene_limits::MAX_SCENE_TRIANGLES {
            return Err(format!("mesh exceeds {} triangles",crate::scene_limits::MAX_SCENE_TRIANGLES));
        }
        // Preserve the 24-float triangle ABI at the System boundary, but stream
        // one interleaved position/material vertex buffer to the native API.
        self.vertices.clear();
        self.vertices.reserve(triangles.len() * 24);
        for triangle in triangles {
            for vertex in 0..3 {
                self.vertices
                    .extend_from_slice(&triangle[vertex * 4..vertex * 4 + 4]);
                self.vertices
                    .extend_from_slice(&triangle[12 + vertex * 4..16 + vertex * 4]);
            }
        }
        (self.gl.BindBuffer)(0x8892, self.vbo);
        (self.gl.BufferData)(
            0x8892,
            (self.vertices.len() * 4) as isize,
            self.vertices.as_ptr().cast(),
            0x88e0,
        );
        self.vertex_count = (triangles.len() * 3) as i32;
        self.check_error()
    }
    pub unsafe fn draw(&mut self) -> Result<(), String> {
        let scene_program=self.scene_program();
        let (w, h) = self.window.dimensions()?;
        if let Some(timing) = &mut self.gpu_timing {
            timing.begin();
        }
        (self.gl.Viewport)(0, 0, w as i32, h as i32);
        let scene_program = self.scene_program();
        if !self.water_frames.is_empty() {
            let targets = self.water_targets.as_mut().unwrap();
            targets.resize(w, h)?;
            if self.water_frames.iter().any(|f| f.optics.planar_reflection) {
                targets.begin(true, self.clear);
                (self.gl.UseProgram)(scene_program);
                (self.gl.Uniform1i)(
                    (self.gl.GetUniformLocation)(scene_program, c"cloudNoiseAtlas".as_ptr()),
                    6,
                );
                (self.gl.ActiveTexture)(0x84c6);
                (self.gl.BindTexture)(0x0de1, self.cloud_noise);
                (self.gl.BindSampler)(6, 0);
                (self.gl.ActiveTexture)(0x84c0);
                (self.gl.ActiveTexture)(0x84c0);
                (self.gl.BindTexture)(0x0de1, self.texture_id);
                (self.gl.BindVertexArray)(self.vao);
                (self.gl.BindBuffer)(0x8892, self.vbo);
                (self.gl.BufferData)(
                    0x8892,
                    (self.reflection_vertices.len() * 4) as isize,
                    self.reflection_vertices.as_ptr().cast(),
                    0x88e0,
                );
                (self.gl.Enable)(0x0b71);
                (self.gl.DrawArrays)(0x0004, 0, self.reflection_count);
                if let (Some(client), Some(packet)) =
                    (&mut self.reflection_vegetation, &self.reflected_plants)
                {
                    client.draw(packet, targets.reflection_framebuffer(), w, h)?;
                }
                (self.gl.BufferData)(
                    0x8892,
                    (self.vertices.len() * 4) as isize,
                    self.vertices.as_ptr().cast(),
                    0x88e0,
                );
            }
            targets.begin(false, self.clear);
        }

        (self.gl.ClearColor)(self.clear[0], self.clear[1], self.clear[2], 1.);
        (self.gl.Enable)(0x0b71);
        (self.gl.Enable)(0x0be2);
        (self.gl.BlendFunc)(0x0302, 0x0303);
        (self.gl.Clear)(0x4000 | 0x0100);
        (self.gl.UseProgram)(scene_program);
        (self.gl.Uniform1i)(
            (self.gl.GetUniformLocation)(scene_program, c"cloudNoiseAtlas".as_ptr()),
            6,
        );
        (self.gl.ActiveTexture)(0x84c6);
        (self.gl.BindTexture)(0x0de1, self.cloud_noise);
        (self.gl.BindSampler)(6, 0);
        (self.gl.ActiveTexture)(0x84c0);
        (self.gl.ActiveTexture)(0x84c0);
        (self.gl.BindTexture)(0x0de1, self.texture_id);
        (self.gl.BindVertexArray)(self.vao);
        (self.gl.DrawArrays)(0x0004, 0, self.vertex_count-if self.world_active{6}else{0});
        if let (Some(client), Some(packet)) = (&mut self.vegetation, &self.plant_packet) {
            client.draw(
                packet,
                self.water_targets
                    .as_ref()
                    .filter(|_| !self.water_frames.is_empty())
                    .map_or(0, |targets| targets.framebuffer()),
                w,
                h,
            )?;
        }
        if !self.water_frames.is_empty() {
            self.water.as_mut().unwrap().encode_batch(
                &self.water_frames,
                self.water_targets.as_ref().unwrap().attachments(
                    0,
                    self.water_frames.iter().any(|f| f.optics.planar_reflection),
                ),
            )?;
        }
        if self.world_active {
            (self.gl.BindFramebuffer)(0x8d40,0);
            (self.gl.Viewport)(0,0,w as i32,h as i32);
            (self.gl.Enable)(0x0b71);(self.gl.Enable)(0x0be2);
            (self.gl.BlendFunc)(0x0302,0x0303);
            (self.gl.Clear)(0x0100);
            (self.gl.UseProgram)(self.program);
            (self.gl.ActiveTexture)(0x84c0);(self.gl.BindTexture)(0x0de1,self.texture_id);
            (self.gl.BindVertexArray)(self.vao);
            (self.gl.DrawArrays)(0x0004,self.vertex_count-6,6);
        }
        if let Some(timing) = &mut self.gpu_timing {
            timing.end();
        }
        if !self.captured {
            let mut pixels = vec![0u8; w as usize * h as usize * 3];
            (self.gl.PixelStorei)(0x0d05, 1);
            (self.gl.ReadPixels)(
                0,
                0,
                w as i32,
                h as i32,
                0x1907,
                0x1401,
                pixels.as_mut_ptr().cast(),
            );
            let center = (h as usize / 2 * w as usize + w as usize / 2) * 3;
            if self.validate_triangle_center && pixels[center..center + 3].iter().all(|v| *v < 25) {
                return Err("OpenGL readback did not contain the triangle at its center".into());
            }
            let mut image = format!("P6\n{w} {h}\n255\n").into_bytes();
            for row in pixels.chunks_exact(w as usize * 3).rev() {
                image.extend_from_slice(row);
            }
            if let Some(monitor) = &mut self.monitor_capture {
                *monitor = crate::scene_assets::monitor_image(&image)?;
            } else {
                std::fs::write(&self.capture, image).map_err(|e| e.to_string())?;
            }
            self.captured = true;
            self.info["readback_verified"] = json!(self.validate_triangle_center);
        }
        // Swap/driver scheduling provides frame pacing. glFinish here forced
        // every interactive frame to synchronously drain the entire GPU queue.
        self.check_error()?;
        (self.window.swap)(self.window.handle);
        self.poll_performance();
        Ok(())
    }
    pub(super) unsafe fn poll_performance(&mut self) {
        if let Some(timing) = &mut self.gpu_timing {
            timing.poll();
        }
    }
    pub unsafe fn wait(&self) -> Result<(), String> {
        (self.gl.Finish)();
        self.check_error()
    }
    unsafe fn check_error(&self) -> Result<(), String> {
        let error = (self.gl.GetError)();
        if error == 0 {
            Ok(())
        } else {
            Err(format!("OpenGL error 0x{error:x}"))
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            if let Some(timing) = &mut self.gpu_timing {
                timing.end();
            }
            if self.cloud_noise != 0 {
                (self.gl.DeleteTextures)(1, &self.cloud_noise);
            }
            (self.gl.Finish)();
            self.gpu_timing.take();
            self.vegetation.take();
            self.reflection_vegetation.take();
            self.water.take();
            self.water_targets.take();
            if self.texture_id != 0 {
                (self.gl.DeleteTextures)(1, &self.texture_id);
            }
            if self.vbo != 0 {
                (self.gl.DeleteBuffers)(1, &self.vbo);
            }
            if self.vao != 0 {
                (self.gl.DeleteVertexArrays)(1, &self.vao);
            }
            if self.program != 0 {
                (self.gl.DeleteProgram)(self.program);
                if self.world_program!=0{(self.gl.DeleteProgram)(self.world_program);}
            }
        }
    }
}
