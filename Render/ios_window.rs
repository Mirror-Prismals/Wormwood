//! UIKit surfaces; native UI calls marshal to the main queue.
use std::ffi::{c_char, c_void, CStr};
pub(super) type VulkanLoader = Option<
    unsafe extern "C" fn(super::vk::VkInstance, *const c_char) -> super::vk::PFN_vkVoidFunction,
>;
unsafe extern "C" {
    fn ww_metal_open(workstation: bool) -> *mut c_void;
    fn ww_metal_layer(handle: *mut c_void) -> *mut c_void;
    fn ww_logical_dimensions(handle: *mut c_void, w: *mut f64, h: *mut f64);
    fn ww_metal_dimensions(handle: *mut c_void, w: *mut u32, h: *mut u32);
    fn ww_metal_show(handle: *mut c_void);
    fn ww_metal_closed(handle: *mut c_void) -> bool;
    fn ww_metal_close(handle: *mut c_void);
    fn ww_ios_is_active() -> bool;
    fn ww_ios_request_close();
    fn ww_ios_stop_requested() -> bool;
    fn ww_ios_caption(text: *const c_char);
    fn ww_touch_ui(active: bool);
}
pub(super) struct Window {
    pub handle: *mut c_void,
}
impl Window {
    pub unsafe fn new(gl: bool, _: &CStr, workstation: bool) -> Result<Self, String> {
        if gl {
            return Err("OpenGL ES uses its EAGL surface client".into());
        }
        let handle = unsafe { ww_metal_open(workstation) };
        if handle.is_null() {
            Err("iOS Metal surface unavailable".into())
        } else {
            Ok(Self { handle })
        }
    }
    pub unsafe fn with_loader(
        gl: bool,
        title: &CStr,
        _: VulkanLoader,
        workstation: bool,
    ) -> Result<Self, String> {
        unsafe { Self::new(gl, title, workstation) }
    }
    pub unsafe fn metal_layer(&self) -> *mut c_void {
        unsafe { ww_metal_layer(self.handle) }
    }
    pub unsafe fn dimensions(&self) -> Result<(u32, u32), String> {
        self.available_dimensions()?
            .ok_or_else(|| "invalid iOS drawable dimensions".into())
    }
    pub unsafe fn available_dimensions(&self) -> Result<Option<(u32, u32)>, String> {
        let (mut w, mut h) = (0, 0);
        unsafe { ww_metal_dimensions(self.handle, &mut w, &mut h) };
        if w > 8192 || h > 8192 {
            Err("invalid iOS drawable dimensions".into())
        } else if w == 0 || h == 0 {
            // UIKit can transiently have no drawable during layout/backgrounding.
            Ok(None)
        } else {
            Ok(Some((w, h)))
        }
    }
    pub unsafe fn logical_dimensions(&self) -> Result<(f64, f64), String> {
        let (mut w, mut h) = (0., 0.);
        ww_logical_dimensions(self.handle, &mut w, &mut h);
        if w <= 0. || h <= 0. {
            return Err("invalid iOS logical dimensions".into());
        }
        Ok((w, h))
    }
    pub unsafe fn show(&self) {
        unsafe { ww_metal_show(self.handle) }
    }
    pub unsafe fn closed(&self) -> bool {
        unsafe { ww_metal_closed(self.handle) }
    }
    pub unsafe fn request_close(&self) -> Result<(), String> {
        unsafe { ww_ios_request_close() };
        Ok(())
    }
    pub unsafe fn input_mode(&mut self, _pointer: bool) -> Result<(), String> {
        unsafe {
            ww_touch_ui(_pointer);
        }
        Ok(())
    }
    pub unsafe fn flight_input(&mut self) -> Result<serde_json::Value, String> {
        Ok(crate::ios_touch::snapshot())
    }
    pub unsafe fn keys(&self) -> Result<[bool; 3], String> {
        Ok([false; 3])
    }
}
pub(super) fn active() -> bool {
    unsafe { ww_ios_is_active() }
}
pub(super) fn caption(text: &str) -> Result<(), String> {
    let s = std::ffi::CString::new(text).map_err(|e| e.to_string())?;
    unsafe { ww_ios_caption(s.as_ptr()) };
    Ok(())
}
impl Drop for Window {
    fn drop(&mut self) {
        unsafe { ww_metal_close(self.handle) }
    }
}

pub(super) fn stop_requested() -> bool {
    unsafe { ww_ios_stop_requested() }
}
