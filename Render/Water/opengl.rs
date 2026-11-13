//! Borrowed-context OpenGL 4.1 / ES3 water pass. Parent owns opaque and actual
//! reflected-scene draws, context lifetime, swapchain, and presentation.
#![allow(non_snake_case, unsafe_op_in_unsafe_fn)]
use super::{DepthFormat, data::DrawFrame};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    path::Path,
    ptr,
};
macro_rules! functions{($($name:ident:($($arg:ty),*)$(->$ret:ty)?;)+)=>{
struct Gl{$($name:unsafe extern "system" fn($($arg),*)$(->$ret)?,)+}
impl Gl{unsafe fn load(resolve:&mut dyn FnMut(&CStr)->*const c_void)->Result<Self,String>{Ok(Self{$($name:{let name=CString::new(concat!("gl",stringify!($name))).unwrap();let p=resolve(&name);if p.is_null(){return Err(format!("water GL entrypoint {} unavailable",name.to_string_lossy()));}std::mem::transmute::<*const c_void,unsafe extern "system" fn($($arg),*)$(->$ret)?>(p)},)+})}}
}}
functions! {
 GetString:(u32)->*const u8;GetError:()->u32;
 CreateShader:(u32)->u32;ShaderSource:(u32,i32,*const *const c_char,*const i32);CompileShader:(u32);GetShaderiv:(u32,u32,*mut i32);GetShaderInfoLog:(u32,i32,*mut i32,*mut c_char);DeleteShader:(u32);
 CreateProgram:()->u32;AttachShader:(u32,u32);LinkProgram:(u32);GetProgramiv:(u32,u32,*mut i32);GetProgramInfoLog:(u32,i32,*mut i32,*mut c_char);DeleteProgram:(u32);UseProgram:(u32);
 GetUniformLocation:(u32,*const c_char)->i32;Uniform1i:(i32,i32);GetUniformBlockIndex:(u32,*const c_char)->u32;UniformBlockBinding:(u32,u32,u32);
 GenVertexArrays:(i32,*mut u32);BindVertexArray:(u32);DeleteVertexArrays:(i32,*const u32);GenBuffers:(i32,*mut u32);BindBuffer:(u32,u32);BindBufferBase:(u32,u32,u32);BufferData:(u32,isize,*const c_void,u32);DeleteBuffers:(i32,*const u32);EnableVertexAttribArray:(u32);VertexAttribPointer:(u32,i32,u32,u8,i32,*const c_void);
 GenSamplers:(i32,*mut u32);SamplerParameteri:(u32,u32,i32);BindSampler:(u32,u32);DeleteSamplers:(i32,*const u32);
 GenTextures:(i32,*mut u32);BindTexture:(u32,u32);ActiveTexture:(u32);TexImage2D:(u32,i32,i32,i32,i32,i32,u32,u32,*const c_void);TexParameteri:(u32,u32,i32);DeleteTextures:(i32,*const u32);
 GenFramebuffers:(i32,*mut u32);BindFramebuffer:(u32,u32);FramebufferTexture2D:(u32,u32,u32,u32,i32);CheckFramebufferStatus:(u32)->u32;DeleteFramebuffers:(i32,*const u32);ReadBuffer:(u32);DrawBuffers:(i32,*const u32);BlitFramebuffer:(i32,i32,i32,i32,i32,i32,i32,i32,u32,u32);
 Viewport:(i32,i32,i32,i32);ClearColor:(f32,f32,f32,f32);Clear:(u32);Enable:(u32);Disable:(u32);DepthFunc:(u32);DepthMask:(u8);ColorMask:(u8,u8,u8,u8);DrawElements:(u32,i32,u32,*const c_void);DrawArrays:(u32,i32,i32);
}
#[derive(Clone, Copy)]
pub(crate) struct GlAttachments {
    pub scene_color: u32,
    pub scene_depth: u32,
    pub reflection: Option<u32>,
    pub output_framebuffer: u32,
    pub depth_format: DepthFormat,
}
pub(crate) struct Client {
    gl: Gl,
    program: u32,
    composite: u32,
    vao: u32,
    screen_vao: u32,
    vbo: u32,
    ebo: u32,
    ubo: u32,
    water_color: u32,
    scene_sampler: u32,
    depth_sampler: u32,
    water_depth: u32,
    water_fbo: u32,
    read_fbo: u32,
    extent: (u32, u32),
    depth_format: DepthFormat,
}
impl Client {
    pub unsafe fn new(
        root: &Path,
        resolve: &mut dyn FnMut(&CStr) -> *const c_void,
    ) -> Result<Self, String> {
        let gl = Gl::load(resolve)?;
        let version = (gl.GetString)(0x1f02);
        if version.is_null() {
            return Err("water GL context unavailable".into());
        }
        let es = CStr::from_ptr(version.cast())
            .to_bytes()
            .starts_with(b"OpenGL ES");
        let suffix = if es { "es" } else { "gl" };
        let mut s = Self {
            gl,
            program: 0,
            composite: 0,
            vao: 0,
            screen_vao: 0,
            vbo: 0,
            ebo: 0,
            ubo: 0,
            water_color: 0,
            scene_sampler: 0,
            depth_sampler: 0,
            water_depth: 0,
            water_fbo: 0,
            read_fbo: 0,
            extent: (0, 0),
            depth_format: DepthFormat::Depth32Float,
        };
        s.program = s.link(root, suffix, "water")?;
        s.composite = s.link(root, suffix, "composite")?;
        let block = (s.gl.GetUniformBlockIndex)(s.program, c"WaterUniforms".as_ptr());
        if block == u32::MAX {
            return Err("water GL uniform block missing".into());
        }
        (s.gl.UniformBlockBinding)(s.program, block, 0);
        (s.gl.GenVertexArrays)(1, &mut s.vao);
        (s.gl.GenVertexArrays)(1, &mut s.screen_vao);
        (s.gl.GenBuffers)(1, &mut s.vbo);
        (s.gl.GenBuffers)(1, &mut s.ebo);
        (s.gl.GenBuffers)(1, &mut s.ubo);
        (s.gl.BindVertexArray)(s.vao);
        (s.gl.BindBuffer)(0x8892, s.vbo);
        (s.gl.BindBuffer)(0x8893, s.ebo);
        for i in 0..6 {
            (s.gl.EnableVertexAttribArray)(i);
            (s.gl.VertexAttribPointer)(i, 4, 0x1406, 0, 96, (i as usize * 16) as *const c_void);
        }
        (s.gl.GenSamplers)(1, &mut s.scene_sampler);
        (s.gl.GenSamplers)(1, &mut s.depth_sampler);
        for (sampler, filter) in [(s.scene_sampler, 0x2601), (s.depth_sampler, 0x2600)] {
            for (parameter, value) in [
                (0x2801, filter),
                (0x2800, filter),
                (0x2802, 0x812f),
                (0x2803, 0x812f),
            ] {
                (s.gl.SamplerParameteri)(sampler, parameter, value);
            }
        }
        (s.gl.GenTextures)(1, &mut s.water_color);
        (s.gl.GenTextures)(1, &mut s.water_depth);
        (s.gl.GenFramebuffers)(1, &mut s.water_fbo);
        (s.gl.GenFramebuffers)(1, &mut s.read_fbo);
        for (program, names) in [
            (
                s.program,
                vec![c"scene_color", c"reflection_color", c"scene_depth"],
            ),
            (s.composite, vec![c"scene_color", c"water_layer"]),
        ] {
            (s.gl.UseProgram)(program);
            for (unit, name) in names.iter().enumerate() {
                let location = (s.gl.GetUniformLocation)(program, name.as_ptr());
                if location < 0 {
                    return Err("water GL texture binding missing".into());
                }
                (s.gl.Uniform1i)(location, unit as i32);
            }
        }
        s.check()?;
        Ok(s)
    }
    unsafe fn check(&self) -> Result<(), String> {
        let e = (self.gl.GetError)();
        if e == 0 {
            Ok(())
        } else {
            Err(format!("water OpenGL error 0x{e:x}"))
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
    unsafe fn link(&self, root: &Path, suffix: &str, name: &str) -> Result<u32, String> {
        let path = root.join("Procedures/Water/Shaders");
        let vertex = self.shader(
            0x8b31,
            &std::fs::read_to_string(path.join(format!("{name}.{suffix}.vert")))
                .map_err(|e| e.to_string())?,
        )?;
        let fragment = match self.shader(
            0x8b30,
            &std::fs::read_to_string(path.join(format!("{name}.{suffix}.frag")))
                .map_err(|e| e.to_string())?,
        ) {
            Ok(p) => p,
            Err(e) => {
                (self.gl.DeleteShader)(vertex);
                return Err(e);
            }
        };
        let p = (self.gl.CreateProgram)();
        (self.gl.AttachShader)(p, vertex);
        (self.gl.AttachShader)(p, fragment);
        (self.gl.LinkProgram)(p);
        (self.gl.DeleteShader)(vertex);
        (self.gl.DeleteShader)(fragment);
        let mut ok = 0;
        (self.gl.GetProgramiv)(p, 0x8b82, &mut ok);
        if ok == 0 {
            let mut log = vec![0u8; 8192];
            let mut size = 0;
            (self.gl.GetProgramInfoLog)(p, 8192, &mut size, log.as_mut_ptr().cast());
            (self.gl.DeleteProgram)(p);
            return Err(format!(
                "water GL link: {}",
                String::from_utf8_lossy(&log[..size.max(0) as usize])
            ));
        }
        Ok(p)
    }
    unsafe fn texture(
        &self,
        id: u32,
        width: u32,
        height: u32,
        internal: i32,
        format: u32,
        kind: u32,
    ) {
        (self.gl.BindTexture)(0x0de1, id);
        (self.gl.TexImage2D)(
            0x0de1,
            0,
            internal,
            width as i32,
            height as i32,
            0,
            format,
            kind,
            ptr::null(),
        );
        for (name, value) in [
            (0x2801, 0x2600),
            (0x2800, 0x2600),
            (0x2802, 0x812f),
            (0x2803, 0x812f),
        ] {
            (self.gl.TexParameteri)(0x0de1, name, value);
        }
    }
    /// Parent must call after its opaque/reflection draws. All sampled inputs are
    /// immutable here; a private depth copy orders overlapping water correctly.
    /// Output cannot alias sampled scene_color. Parent establishes its next pass's
    /// GL state explicitly; this is the last material/composite pass before present.
    pub unsafe fn encode_batch(
        &mut self,
        frames: &[DrawFrame],
        a: GlAttachments,
    ) -> Result<(), String> {
        let Some(first) = frames.first() else {
            return Ok(());
        };
        for frame in frames {
            frame.validate()?;
            if frame.camera.width != first.camera.width
                || frame.camera.height != first.camera.height
                || frame.camera.reversed_z != first.camera.reversed_z
            {
                return Err("water batch viewport/depth convention mismatch".into());
            }
            if frame.optics.planar_reflection && a.reflection.is_none() {
                return Err("water reflected-scene attachment missing".into());
            }
        }
        if a.scene_color == 0 || a.scene_depth == 0 {
            return Err("water opaque attachments missing".into());
        }
        let (width, height) = (first.camera.width, first.camera.height);
        let depth_attachment = if a.depth_format == DepthFormat::Depth24Stencil8 {
            0x821a
        } else {
            0x8d00
        };
        if self.extent != (width, height) || self.depth_format != a.depth_format {
            self.texture(self.water_color, width, height, 0x8058, 0x1908, 0x1401);
            match a.depth_format {
                DepthFormat::Depth32Float => {
                    self.texture(self.water_depth, width, height, 0x8cac, 0x1902, 0x1406)
                }
                DepthFormat::Depth24Stencil8 => {
                    self.texture(self.water_depth, width, height, 0x88f0, 0x84f9, 0x84fa)
                }
            }
            self.extent = (width, height);
            self.depth_format = a.depth_format;
        }
        (self.gl.BindFramebuffer)(0x8d40, self.read_fbo);
        (self.gl.DrawBuffers)(1, &0);
        for attachment in [0x8d00, 0x821a] {
            (self.gl.FramebufferTexture2D)(0x8ca8, attachment, 0x0de1, 0, 0);
        }
        (self.gl.FramebufferTexture2D)(0x8ca8, depth_attachment, 0x0de1, a.scene_depth, 0);
        (self.gl.ReadBuffer)(0);
        if (self.gl.CheckFramebufferStatus)(0x8ca8) != 0x8cd5 {
            return Err("water opaque-depth framebuffer incomplete".into());
        }
        (self.gl.BindFramebuffer)(0x8ca9, self.water_fbo);
        for attachment in [0x8d00, 0x821a] {
            (self.gl.FramebufferTexture2D)(0x8ca9, attachment, 0x0de1, 0, 0);
        }
        (self.gl.FramebufferTexture2D)(0x8ca9, 0x8ce0, 0x0de1, self.water_color, 0);
        (self.gl.FramebufferTexture2D)(0x8ca9, depth_attachment, 0x0de1, self.water_depth, 0);
        (self.gl.DrawBuffers)(1, &0x8ce0);
        if (self.gl.CheckFramebufferStatus)(0x8ca9) != 0x8cd5 {
            return Err("water private framebuffer incomplete".into());
        }
        (self.gl.BlitFramebuffer)(
            0,
            0,
            width as i32,
            height as i32,
            0,
            0,
            width as i32,
            height as i32,
            0x100,
            0x2600,
        );
        (self.gl.Viewport)(0, 0, width as i32, height as i32);
        (self.gl.Disable)(0x0c11);
        (self.gl.Disable)(0x0be2);
        (self.gl.Disable)(0x0b44);
        (self.gl.Enable)(0x0b71);
        (self.gl.DepthFunc)(if first.camera.reversed_z {
            0x0206
        } else {
            0x0203
        });
        (self.gl.DepthMask)(1);
        (self.gl.ColorMask)(1, 1, 1, 1);
        (self.gl.ClearColor)(0., 0., 0., 0.);
        (self.gl.Clear)(0x4000);
        (self.gl.UseProgram)(self.program);
        (self.gl.BindVertexArray)(self.vao);
        (self.gl.BindBufferBase)(0x8a11, 0, self.ubo);
        for (unit, texture) in [
            a.scene_color,
            a.reflection.unwrap_or(a.scene_color),
            a.scene_depth,
        ]
        .into_iter()
        .enumerate()
        {
            (self.gl.ActiveTexture)(0x84c0 + unit as u32);
            (self.gl.BindTexture)(0x0de1, texture);
            (self.gl.BindSampler)(
                unit as u32,
                if unit == 2 {
                    self.depth_sampler
                } else {
                    self.scene_sampler
                },
            );
        }
        for frame in frames {
            let vertices = frame.vertex_bytes_borrowed();
            let indices = frame.index_bytes_borrowed();
            let uniforms = super::uniform_bytes(frame, 1.)?;
            for (target, buffer, bytes) in [
                (0x8892, self.vbo, vertices),
                (0x8893, self.ebo, indices),
                (0x8a11, self.ubo, uniforms.as_slice()),
            ] {
                (self.gl.BindBuffer)(target, buffer);
                (self.gl.BufferData)(target, bytes.len() as isize, bytes.as_ptr().cast(), 0x88e0);
            }
            (self.gl.DrawElements)(0x0004, frame.indices.len() as i32, 0x1405, ptr::null());
        }
        (self.gl.BindFramebuffer)(0x8d40, a.output_framebuffer);
        (self.gl.Disable)(0x0b71);
        (self.gl.DepthMask)(0);
        (self.gl.UseProgram)(self.composite);
        (self.gl.BindVertexArray)(self.screen_vao);
        for (unit, texture) in [a.scene_color, self.water_color].into_iter().enumerate() {
            (self.gl.ActiveTexture)(0x84c0 + unit as u32);
            (self.gl.BindTexture)(0x0de1, texture);
            (self.gl.BindSampler)(unit as u32, self.scene_sampler);
        }
        (self.gl.DrawArrays)(0x0004, 0, 3);
        (self.gl.DepthMask)(1);
        self.check()
    }
    pub unsafe fn encode(&mut self, frame: &DrawFrame, a: GlAttachments) -> Result<(), String> {
        self.encode_batch(std::slice::from_ref(frame), a)
    }
    pub unsafe fn release(&mut self) {
        for sampler in [&mut self.scene_sampler, &mut self.depth_sampler] {
            if *sampler != 0 {
                (self.gl.DeleteSamplers)(1, sampler);
                *sampler = 0;
            }
        }
        for id in [&mut self.program, &mut self.composite] {
            if *id != 0 {
                (self.gl.DeleteProgram)(*id);
                *id = 0;
            }
        }
        for id in [&mut self.vao, &mut self.screen_vao] {
            if *id != 0 {
                (self.gl.DeleteVertexArrays)(1, id);
                *id = 0;
            }
        }
        for id in [&mut self.vbo, &mut self.ebo, &mut self.ubo] {
            if *id != 0 {
                (self.gl.DeleteBuffers)(1, id);
                *id = 0;
            }
        }
        for id in [&mut self.water_color, &mut self.water_depth] {
            if *id != 0 {
                (self.gl.DeleteTextures)(1, id);
                *id = 0;
            }
        }
        for id in [&mut self.water_fbo, &mut self.read_fbo] {
            if *id != 0 {
                (self.gl.DeleteFramebuffers)(1, id);
                *id = 0;
            }
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            self.release();
        }
    }
}

/// Scene attachments owned by the graphics connection alongside its opaque pass.
pub(crate) struct SceneTargets {gl:Gl,color:[u32;2],depth:[u32;2],fbo:[u32;2],extent:(u32,u32)}
impl SceneTargets {
    pub unsafe fn new(resolve:&mut dyn FnMut(&CStr)->*const c_void)->Result<Self,String>{
        let gl=Gl::load(resolve)?;let mut s=Self{gl,color:[0;2],depth:[0;2],fbo:[0;2],extent:(0,0)};
        (s.gl.GenTextures)(2,s.color.as_mut_ptr());(s.gl.GenTextures)(2,s.depth.as_mut_ptr());(s.gl.GenFramebuffers)(2,s.fbo.as_mut_ptr());Ok(s)
    }
    pub unsafe fn resize(&mut self,width:u32,height:u32)->Result<(),String>{
        if self.extent==(width,height){return Ok(());}
        for i in 0..2{
            for (texture,internal,format,kind) in [(self.color[i],0x8058,0x1908,0x1401),(self.depth[i],0x8cac,0x1902,0x1406)] {
                (self.gl.BindTexture)(0x0de1,texture);(self.gl.TexImage2D)(0x0de1,0,internal,width as i32,height as i32,0,format,kind,ptr::null());
                for (key,value) in [(0x2801,0x2600),(0x2800,0x2600),(0x2802,0x812f),(0x2803,0x812f),(0x884c,0)]{(self.gl.TexParameteri)(0x0de1,key,value);}
            }
            (self.gl.BindFramebuffer)(0x8d40,self.fbo[i]);(self.gl.FramebufferTexture2D)(0x8d40,0x8ce0,0x0de1,self.color[i],0);(self.gl.FramebufferTexture2D)(0x8d40,0x8d00,0x0de1,self.depth[i],0);(self.gl.DrawBuffers)(1,&0x8ce0);(self.gl.ReadBuffer)(0x8ce0);
            if (self.gl.CheckFramebufferStatus)(0x8d40)!=0x8cd5{return Err("water scene framebuffer incomplete".into());}
        }self.extent=(width,height);Ok(())
    }
    pub unsafe fn begin(&self,reflection:bool,clear:[f32;4]){
        (self.gl.BindFramebuffer)(0x8d40,self.fbo[reflection as usize]);(self.gl.Viewport)(0,0,self.extent.0 as i32,self.extent.1 as i32);(self.gl.ColorMask)(1,1,1,1);(self.gl.DepthMask)(1);(self.gl.ClearColor)(clear[0],clear[1],clear[2],clear[3]);(self.gl.Clear)(0x4000|0x0100);
    }
    pub fn reflection_framebuffer(&self)->u32{self.fbo[1]}
    pub fn framebuffer(&self)->u32{self.fbo[0]}
    pub fn attachments(&self,output:u32,reflection:bool)->GlAttachments{GlAttachments{scene_color:self.color[0],scene_depth:self.depth[0],reflection:reflection.then_some(self.color[1]),output_framebuffer:output,depth_format:DepthFormat::Depth32Float}}
}
impl Drop for SceneTargets{fn drop(&mut self){unsafe{(self.gl.DeleteFramebuffers)(2,self.fbo.as_ptr());(self.gl.DeleteTextures)(2,self.color.as_ptr());(self.gl.DeleteTextures)(2,self.depth.as_ptr());}}}
