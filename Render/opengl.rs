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
    GetString:(u32)->*const u8; GetError:()->u32;
    CreateShader:(u32)->u32; ShaderSource:(u32,i32,*const *const c_char,*const i32);
    CompileShader:(u32);GetShaderiv:(u32,u32,*mut i32);GetShaderInfoLog:(u32,i32,*mut i32,*mut c_char);DeleteShader:(u32);
    CreateProgram:()->u32;AttachShader:(u32,u32);LinkProgram:(u32);GetProgramiv:(u32,u32,*mut i32);GetProgramInfoLog:(u32,i32,*mut i32,*mut c_char);DeleteProgram:(u32);UseProgram:(u32);
    GenVertexArrays:(i32,*mut u32);BindVertexArray:(u32);DeleteVertexArrays:(i32,*const u32);
    GetUniformLocation:(u32,*const c_char)->i32;Uniform4fv:(i32,i32,*const f32);
    Viewport:(i32,i32,i32,i32);ClearColor:(f32,f32,f32,f32);Clear:(u32);DrawArrays:(u32,i32,i32);Finish:();
    PixelStorei:(u32,i32);ReadPixels:(i32,i32,i32,i32,u32,u32,*mut c_void);
}
pub(super) struct Client {
    pub window: Window,
    gl: Gl,
    program: u32,
    vao: u32,
    pub info: Value,
    capture: std::path::PathBuf,
    captured: bool,
    count: i32,
    pub(super) clear: [f32; 4],
}
impl Client {
    pub unsafe fn new(root: &Path, data: &[f32; 24], output: &Path) -> Result<Self, String> {
        let window = Window::new(true, c"Wormwood — OpenGL")?;
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
            program: 0,
            vao: 0,
            info,
            capture: output.join("triangle-OpenGL.ppm"),
            captured: false,
            count: 3,
            clear: [0.035, 0.045, 0.065, 1.],
        };
        let vertex = client.shader(
            0x8b31,
            &std::fs::read_to_string(root.join("Procedures/Shaders/triangle.gl.vert"))
                .map_err(|e| e.to_string())?,
        )?;
        let fragment = match client.shader(
            0x8b30,
            &std::fs::read_to_string(root.join("Procedures/Shaders/triangle.gl.frag"))
                .map_err(|e| e.to_string())?,
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
        (client.gl.UseProgram)(client.program);
        (client.gl.Uniform4fv)(
            (client.gl.GetUniformLocation)(client.program, c"positions".as_ptr()),
            3,
            data.as_ptr(),
        );
        (client.gl.Uniform4fv)(
            (client.gl.GetUniformLocation)(client.program, c"colors".as_ptr()),
            3,
            data.as_ptr().add(12),
        );
        client.wait()?;
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
    pub unsafe fn set_batch(&mut self, triangles: &[[f32; 24]]) -> Result<(), String> {
        if triangles.len() > 128 {
            return Err("initial mesh batch exceeds 128 triangles".into());
        }
        let mut positions = [0f32; 1536];
        let mut colors = [0f32; 1536];
        for (i, t) in triangles.iter().enumerate() {
            positions[i * 12..i * 12 + 12].copy_from_slice(&t[..12]);
            colors[i * 12..i * 12 + 12].copy_from_slice(&t[12..]);
        }
        (self.gl.UseProgram)(self.program);
        (self.gl.Uniform4fv)(
            (self.gl.GetUniformLocation)(self.program, c"positions".as_ptr()),
            384,
            positions.as_ptr(),
        );
        (self.gl.Uniform4fv)(
            (self.gl.GetUniformLocation)(self.program, c"colors".as_ptr()),
            384,
            colors.as_ptr(),
        );
        self.count = (triangles.len() * 3) as i32;
        self.wait()
    }
    pub unsafe fn draw(&mut self) -> Result<(), String> {
        let (w, h) = self.window.dimensions()?;
        (self.gl.Viewport)(0, 0, w as i32, h as i32);
        (self.gl.ClearColor)(self.clear[0], self.clear[1], self.clear[2], 1.);
        (self.gl.Clear)(0x4000);
        (self.gl.UseProgram)(self.program);
        (self.gl.BindVertexArray)(self.vao);
        (self.gl.DrawArrays)(0x0004, 0, self.count);
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
            if pixels[center..center + 3].iter().all(|v| *v < 25) {
                return Err("OpenGL readback did not contain the triangle at its center".into());
            }
            let mut image = format!("P6\n{w} {h}\n255\n").into_bytes();
            for row in pixels.chunks_exact(w as usize * 3).rev() {
                image.extend_from_slice(row);
            }
            std::fs::write(&self.capture, image).map_err(|e| e.to_string())?;
            self.captured = true;
            self.info["readback_verified"] = json!(true);
        }
        self.wait()?;
        (self.window.swap)(self.window.handle);
        Ok(())
    }
    pub unsafe fn wait(&self) -> Result<(), String> {
        (self.gl.Finish)();
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
            (self.gl.Finish)();
            if self.vao != 0 {
                (self.gl.DeleteVertexArrays)(1, &self.vao);
            }
            if self.program != 0 {
                (self.gl.DeleteProgram)(self.program);
            }
        }
    }
}
