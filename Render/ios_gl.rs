//! iOS surface client. OpenGL selects OpenGL ES 3 explicitly; other APIs never fall back.
use crate::base_entity::{Appearance, Entity};
use serde_json::{Value, json};
use std::{
    ffi::{CString, c_void},
    path::Path,
};
#[repr(C)]
struct Info {
    width: u32,
    height: u32,
    center: [u8; 4],
    corner: [u8; 4],
    renderer: [u8; 256],
    readback: bool,
}
unsafe extern "C" {
    fn ww_graphics_open(
        vertices: *const f32,
        vertex: *const i8,
        fragment: *const i8,
        info: *mut Info,
        error: *mut u8,
        capacity: usize,
    ) -> *mut c_void;
    fn ww_graphics_frame(
        handle: *mut c_void,
        info: *mut Info,
        error: *mut u8,
        capacity: usize,
    ) -> i32;
    fn ww_graphics_close(handle: *mut c_void, error: *mut u8, capacity: usize) -> i32;
}
pub(super) struct Session {
    handle: *mut c_void,
    info: Info,
}
fn message(error: &[u8]) -> String {
    String::from_utf8_lossy(error.split(|b| *b == 0).next().unwrap_or(error)).into_owned()
}
impl Session {
    pub(super) fn prepare(root: &Path, backend: &str, entity: &Entity) -> Result<Self, String> {
        if backend != "OpenGL" {
            return Err(format!(
                "iOS {backend} native client is not integrated; no fallback was selected"
            ));
        }
        let vertices = match &entity.appearance {
            Appearance::Visual { vertices, .. } if vertices.len() == 3 => vertices,
            _ => return Err("initial iOS renderer requires three visual vertices".into()),
        };
        let shader = |name: &str| -> Result<CString, String> {
            CString::new(
                std::fs::read(root.join("Procedures/Shaders").join(name))
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())
        };
        let vertex = shader("ios.vert")?;
        let fragment = shader("ios.frag")?;
        let mut s = Self {
            handle: std::ptr::null_mut(),
            info: Info {
                width: 0,
                height: 0,
                center: [0; 4],
                corner: [0; 4],
                renderer: [0; 256],
                readback: false,
            },
        };
        let mut error = [0; 1024];
        s.handle = unsafe {
            ww_graphics_open(
                vertices.as_ptr().cast(),
                vertex.as_ptr(),
                fragment.as_ptr(),
                &mut s.info,
                error.as_mut_ptr(),
                error.len(),
            )
        };
        if s.handle.is_null() {
            return Err(message(&error));
        }
        Ok(s)
    }
    pub(super) fn info(&self) -> Value {
        json!({"api":"OpenGL ES 3","renderer":message(&self.info.renderer),"width":self.info.width,"height":self.info.height,"readback_verified":self.info.readback,"center_rgba":self.info.center,"corner_rgba":self.info.corner})
    }
    pub(super) fn present(
        &mut self,
        frames: u32,
        mut record: impl FnMut(u32) -> Result<(), String>,
    ) -> Result<Value, String> {
        let mut error = [0; 1024];
        for frame in 1..=frames {
            if unsafe {
                ww_graphics_frame(self.handle, &mut self.info, error.as_mut_ptr(), error.len())
            } != 0
            {
                return Err(message(&error));
            }
            record(frame)?;
            // This finite diagnostic runs off the UIKit thread. Yielding here
            // does not enter the audio callback or claim display-clock synchronization.
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        Ok(json!({"backend":"OpenGL","frames":frames,"device":self.info()}))
    }
    pub(super) fn finish(&mut self) -> Result<(), String> {
        if !self.handle.is_null() {
            let mut error = [0; 1024];
            if unsafe { ww_graphics_close(self.handle, error.as_mut_ptr(), error.len()) } != 0 {
                return Err(message(&error));
            }
            self.handle = std::ptr::null_mut();
        }
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
