//! Rust port of the borrowed-context indexed vegetation and sunlight passes.
#![allow(non_snake_case, unsafe_op_in_unsafe_fn)]
use crate::vegetation::packet::Packet;
use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::Path,
    ptr,
};
macro_rules! functions{($($name:ident:($($arg:ty),*)$(->$ret:ty)?;)+)=>{
struct Gl{$($name:unsafe extern "system" fn($($arg),*)$(->$ret)?,)+}
impl Gl{unsafe fn load(resolve:&mut dyn FnMut(&CStr)->*const c_void)->Result<Self,String>{Ok(Self{$($name:{let name=CString::new(concat!("gl",stringify!($name))).unwrap();let p=resolve(&name);if p.is_null(){return Err(format!("vegetation GL entrypoint {} unavailable",name.to_string_lossy()));}std::mem::transmute::<*const c_void,unsafe extern "system" fn($($arg),*)$(->$ret)?>(p)},)+})}}
}}
functions! {
 UniformMatrix4fv:(i32,i32,u8,*const f32);Uniform3fv:(i32,i32,*const f32);
 GetString:(u32)->*const u8;GetError:()->u32;GetIntegerv:(u32,*mut i32);
 CreateShader:(u32)->u32;ShaderSource:(u32,i32,*const *const c_char,*const i32);CompileShader:(u32);GetShaderiv:(u32,u32,*mut i32);GetShaderInfoLog:(u32,i32,*mut i32,*mut c_char);DeleteShader:(u32);
 CreateProgram:()->u32;AttachShader:(u32,u32);LinkProgram:(u32);GetProgramiv:(u32,u32,*mut i32);GetProgramInfoLog:(u32,i32,*mut i32,*mut c_char);DeleteProgram:(u32);UseProgram:(u32);
 GetUniformLocation:(u32,*const c_char)->i32;Uniform1i:(i32,i32);GetUniformBlockIndex:(u32,*const c_char)->u32;UniformBlockBinding:(u32,u32,u32);
 GenVertexArrays:(i32,*mut u32);BindVertexArray:(u32);DeleteVertexArrays:(i32,*const u32);GenBuffers:(i32,*mut u32);BindBuffer:(u32,u32);BindBufferBase:(u32,u32,u32);BufferData:(u32,isize,*const c_void,u32);DeleteBuffers:(i32,*const u32);EnableVertexAttribArray:(u32);VertexAttribPointer:(u32,i32,u32,u8,i32,*const c_void);
 GenSamplers:(i32,*mut u32);SamplerParameteri:(u32,u32,i32);BindSampler:(u32,u32);DeleteSamplers:(i32,*const u32);
 GenTextures:(i32,*mut u32);BindTexture:(u32,u32);ActiveTexture:(u32);TexImage2D:(u32,i32,i32,i32,i32,i32,u32,u32,*const c_void);TexParameteri:(u32,u32,i32);DeleteTextures:(i32,*const u32);
 GenFramebuffers:(i32,*mut u32);BindFramebuffer:(u32,u32);FramebufferTexture2D:(u32,u32,u32,u32,i32);CheckFramebufferStatus:(u32)->u32;DeleteFramebuffers:(i32,*const u32);ReadBuffer:(u32);DrawBuffers:(i32,*const u32);BlitFramebuffer:(i32,i32,i32,i32,i32,i32,i32,i32,u32,u32);
 Viewport:(i32,i32,i32,i32);ClearColor:(f32,f32,f32,f32);Clear:(u32);Enable:(u32);Disable:(u32);DepthFunc:(u32);DepthMask:(u8);ColorMask:(u8,u8,u8,u8);DrawElements:(u32,i32,u32,*const c_void);DrawArrays:(u32,i32,i32);
}

pub(crate) struct Client {
    gl: Gl,
    woodland: u32,
    impostors: u32,
    program: u32,
    shadow: u32,
    vao: u32,
    vbo: u32,
    ebo: u32,
    depth: u32,
    fbo: u32,
    sampler: u32,
}
impl Client {
    pub unsafe fn new(
        root: &Path,
        resolve: &mut dyn FnMut(&CStr) -> *const c_void,
    ) -> Result<Self, String> {
        let gl = Gl::load(resolve)?;
        let mut s = Self {
            gl,
            woodland: 0,
            impostors: 0,
            program: 0,
            shadow: 0,
            vao: 0,
            vbo: 0,
            ebo: 0,
            depth: 0,
            fbo: 0,
            sampler: 0,
        };
        let version = (s.gl.GetString)(0x1f02);
        if version.is_null() {
            return Err("vegetation GL context missing".into());
        }
        let es = CStr::from_ptr(version.cast())
            .to_bytes()
            .starts_with(b"OpenGL ES");
        let suffix = if es { "ios" } else { "gl" };
        let vertex = std::fs::read_to_string(
            root.join(format!("Procedures/Shaders/vegetation.{suffix}.vert")),
        )
        .map_err(|e| e.to_string())?;
        let shadow_name = if es {
            "vegetation.shadow.ios.frag"
        } else {
            "vegetation.shadow.frag"
        };
        for (index, name) in [format!("vegetation.{suffix}.frag"), shadow_name.into()]
            .iter()
            .enumerate()
        {
            let vs = s.shader(0x8b31, &vertex)?;
            let fragment = std::fs::read_to_string(root.join("Procedures/Shaders").join(name))
                .map_err(|e| e.to_string())?;
            let fs = match s.shader(0x8b30, &fragment) {
                Ok(fs) => fs,
                Err(e) => {
                    (s.gl.DeleteShader)(vs);
                    return Err(e);
                }
            };
            let program = (s.gl.CreateProgram)();
            (s.gl.AttachShader)(program, vs);
            (s.gl.AttachShader)(program, fs);
            (s.gl.LinkProgram)(program);
            (s.gl.DeleteShader)(vs);
            (s.gl.DeleteShader)(fs);
            if index == 0 {
                s.program = program;
            } else {
                s.shadow = program;
            }
            let mut success = 0;
            (s.gl.GetProgramiv)(program, 0x8b82, &mut success);
            if success == 0 {
                let mut log = [0u8; 4096];
                let mut n = 0;
                (s.gl.GetProgramInfoLog)(program, 4096, &mut n, log.as_mut_ptr().cast());
                return Err(format!(
                    "vegetation link: {}",
                    String::from_utf8_lossy(&log[..n.max(0) as usize])
                ));
            }
        }
        (s.gl.GenVertexArrays)(1, &mut s.vao);
        (s.gl.BindVertexArray)(s.vao);
        (s.gl.GenBuffers)(1, &mut s.vbo);
        (s.gl.BindBuffer)(0x8892, s.vbo);
        (s.gl.GenBuffers)(1, &mut s.ebo);
        (s.gl.BindBuffer)(0x8893, s.ebo);
        for (i, (size, offset)) in [(3, 0), (3, 12), (2, 24), (3, 32), (4, 44), (1, 60)]
            .into_iter()
            .enumerate()
        {
            (s.gl.EnableVertexAttribArray)(i as u32);
            (s.gl.VertexAttribPointer)(i as u32, size, 0x1406, 0, 64, offset as *const c_void);
        }
        (s.gl.GenTextures)(1, &mut s.depth);
        (s.gl.BindTexture)(0x0de1, s.depth);
        (s.gl.TexImage2D)(
            0x0de1,
            0,
            0x8cac,
            2048,
            2048,
            0,
            0x1902,
            0x1406,
            ptr::null(),
        );
        for (key, value) in [
            (0x2801, 0x2600),
            (0x2800, 0x2600),
            (0x2802, 0x812f),
            (0x2803, 0x812f),
            (0x884c, 0),
        ] {
            (s.gl.TexParameteri)(0x0de1, key, value);
        }
        (s.gl.GenSamplers)(1, &mut s.sampler);
        for (key, value) in [
            (0x2801, 0x2600),
            (0x2800, 0x2600),
            (0x2802, 0x812f),
            (0x2803, 0x812f),
            (0x884c, 0),
        ] {
            (s.gl.SamplerParameteri)(s.sampler, key, value);
        }
        (s.gl.GenFramebuffers)(1, &mut s.fbo);
        (s.gl.BindFramebuffer)(0x8d40, s.fbo);
        (s.gl.FramebufferTexture2D)(0x8d40, 0x8d00, 0x0de1, s.depth, 0);
        let none = 0;
        (s.gl.DrawBuffers)(1, &none);
        (s.gl.ReadBuffer)(0);
        let complete = (s.gl.CheckFramebufferStatus)(0x8d40) == 0x8cd5;
        (s.gl.BindFramebuffer)(0x8d40, 0);
        if !complete {
            return Err("vegetation sunlight framebuffer incomplete".into());
        }
        s.check()?;
        let bytes = crate::terrain::woodland::BYTES;
        (s.gl.GenTextures)(1, &mut s.woodland);
        (s.gl.ActiveTexture)(0x84c5);
        (s.gl.BindTexture)(0x0de1, s.woodland);
        for (key, value) in [
            (0x2801, 0x2600),
            (0x2800, 0x2600),
            (0x2802, 0x812f),
            (0x2803, 0x812f),
        ] {
            (s.gl.TexParameteri)(0x0de1, key, value);
        }
        (s.gl.TexImage2D)(
            0x0de1,
            0,
            0x8058,
            24,
            (24 * crate::terrain::woodland::TILE_COUNT) as i32,
            0,
            0x1908,
            0x1401,
            bytes.as_ptr().add(8).cast(),
        );
        (s.gl.ActiveTexture)(0x84cf);
        let mut previous=0;(s.gl.GetIntegerv)(0x8069,&mut previous);
        (s.gl.GenTextures)(1,&mut s.impostors);(s.gl.BindTexture)(0x0de1,s.impostors);
        for (key,value) in [(0x2801,0x2600),(0x2800,0x2600),(0x2802,0x812f),(0x2803,0x812f)] {(s.gl.TexParameteri)(0x0de1,key,value);}
        (s.gl.TexImage2D)(0x0de1,0,0x8058,crate::vegetation::impostor::ATLAS_WIDTH as i32,crate::vegetation::impostor::ATLAS_HEIGHT as i32,0,0x1908,0x1401,crate::vegetation::impostor::BYTES.as_ptr().add(8).cast());
        (s.gl.BindTexture)(0x0de1,previous as u32);
        (s.gl.ActiveTexture)(0x84c0);
        s.check()?;
        Ok(s)
    }
    unsafe fn check(&self) -> Result<(), String> {
        let e = (self.gl.GetError)();
        if e == 0 {
            Ok(())
        } else {
            Err(format!("vegetation OpenGL error 0x{e:x}"))
        }
    }
    unsafe fn shader(&self, kind: u32, source: &str) -> Result<u32, String> {
        let source = CString::new(source).map_err(|e| e.to_string())?;
        let shader = (self.gl.CreateShader)(kind);
        let p = source.as_ptr();
        (self.gl.ShaderSource)(shader, 1, &p, ptr::null());
        (self.gl.CompileShader)(shader);
        let mut ok = 0;
        (self.gl.GetShaderiv)(shader, 0x8b81, &mut ok);
        if ok == 0 {
            let mut log = vec![0u8; 8192];
            let mut size = 0;
            (self.gl.GetShaderInfoLog)(shader, 8192, &mut size, log.as_mut_ptr().cast());
            (self.gl.DeleteShader)(shader);
            return Err(format!(
                "water GL shader: {}",
                String::from_utf8_lossy(&log[..size.max(0) as usize])
            ));
        }
        Ok(shader)
    }

    unsafe fn matrix(&self, program: u32, name: &CStr, matrix: &[[f32; 4]; 4]) {
        (self.gl.UniformMatrix4fv)(
            (self.gl.GetUniformLocation)(program, name.as_ptr()),
            1,
            0,
            matrix.as_ptr().cast(),
        );
    }
    pub unsafe fn draw(
        &mut self,
        packet: &Packet,
        target: u32,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        packet.scene.validate()?;
        (self.gl.BindVertexArray)(self.vao);
        (self.gl.BindBuffer)(0x8892, self.vbo);
        (self.gl.BufferData)(
            0x8892,
            (packet.vertices.len() * 64) as isize,
            packet.vertices.as_ptr().cast(),
            0x88e0,
        );
        (self.gl.BindBuffer)(0x8893, self.ebo);
        (self.gl.BufferData)(
            0x8893,
            (packet.indices.len() * 4) as isize,
            packet.indices.as_ptr().cast(),
            0x88e0,
        );
        for flag in [0x0be2, 0x0b44, 0x0c11, 0x8037, 0x8c89, 0x0b90] {
            (self.gl.Disable)(flag);
        }
        (self.gl.Enable)(0x0b71);
        (self.gl.DepthFunc)(0x0201);
        (self.gl.DepthMask)(1);
        (self.gl.BindFramebuffer)(0x8d40, self.fbo);
        (self.gl.Viewport)(0, 0, 2048, 2048);
        (self.gl.ColorMask)(0, 0, 0, 0);
        (self.gl.Clear)(0x0100);
        (self.gl.ActiveTexture)(0x84cf);
        let mut previous_tree=0;let mut previous_sampler=0;
        (self.gl.GetIntegerv)(0x8069,&mut previous_tree);(self.gl.GetIntegerv)(0x8919,&mut previous_sampler);
        (self.gl.BindTexture)(0x0de1,self.impostors);(self.gl.BindSampler)(15,0);
        (self.gl.UseProgram)(self.shadow);
        (self.gl.Uniform1i)((self.gl.GetUniformLocation)(self.shadow,c"treeAtlas".as_ptr()),15);
        self.matrix(
            self.shadow,
            c"viewProjection",
            &packet.scene.sun_view_projection,
        );
        (self.gl.ActiveTexture)(0x84c5);
        (self.gl.BindTexture)(0x0de1, self.woodland);
        (self.gl.BindSampler)(5, 0);
        (self.gl.DrawElements)(0x0004, packet.indices.len() as i32, 0x1405, ptr::null());
        (self.gl.BindFramebuffer)(0x8d40, target);
        (self.gl.Viewport)(0, 0, width as i32, height as i32);
        (self.gl.ColorMask)(1, 1, 1, 1);
        (self.gl.UseProgram)(self.program);
        (self.gl.Uniform1i)((self.gl.GetUniformLocation)(self.program,c"treeAtlas".as_ptr()),15);
        self.matrix(
            self.program,
            c"viewProjection",
            &packet.scene.view_projection,
        );
        self.matrix(
            self.program,
            c"sunViewProjection",
            &packet.scene.sun_view_projection,
        );
        for (name, value) in [
            (c"sunDirection", &packet.scene.sun_direction),
            (c"sunColor", &packet.scene.sun_color),
            (c"ambientColor", &packet.scene.ambient_color),
        ] {
            (self.gl.Uniform3fv)(
                (self.gl.GetUniformLocation)(self.program, name.as_ptr()),
                1,
                value.as_ptr(),
            );
        }
        (self.gl.Uniform1i)(
            (self.gl.GetUniformLocation)(self.program, c"shadowsEnabled".as_ptr()),
            packet.scene.options[0] as i32,
        );
        (self.gl.Uniform1i)(
            (self.gl.GetUniformLocation)(self.program, c"shadowDepth".as_ptr()),
            4,
        );
        (self.gl.ActiveTexture)(0x84c4);
        (self.gl.BindTexture)(0x0de1, self.depth);
        (self.gl.BindSampler)(4, self.sampler);
        (self.gl.Uniform1i)(
            (self.gl.GetUniformLocation)(self.program, c"woodlandAtlas".as_ptr()),
            5,
        );
        (self.gl.ActiveTexture)(0x84c5);
        (self.gl.BindTexture)(0x0de1, self.woodland);
        (self.gl.BindSampler)(5, 0);
        (self.gl.DrawElements)(0x0004, packet.indices.len() as i32, 0x1405, ptr::null());
        (self.gl.ActiveTexture)(0x84cf);(self.gl.BindTexture)(0x0de1,previous_tree as u32);(self.gl.BindSampler)(15,previous_sampler as u32);
        (self.gl.ActiveTexture)(0x84c0);
        (self.gl.Enable)(0x0be2);
        self.check()
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            if self.impostors!=0 {(self.gl.DeleteTextures)(1,&self.impostors);}
            if self.woodland != 0 {
                (self.gl.DeleteTextures)(1, &self.woodland);
            }
            for program in [self.program, self.shadow] {
                if program != 0 {
                    (self.gl.DeleteProgram)(program);
                }
            }
            (self.gl.DeleteVertexArrays)(1, &self.vao);
            (self.gl.DeleteBuffers)(1, &self.vbo);
            (self.gl.DeleteBuffers)(1, &self.ebo);
            (self.gl.DeleteTextures)(1, &self.depth);
            (self.gl.DeleteFramebuffers)(1, &self.fbo);
            (self.gl.DeleteSamplers)(1, &self.sampler);
        }
    }
}
