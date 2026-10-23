//! UIKit CAMetalLayer surface. No GLFW window or desktop surface API on iOS.
use std::ffi::{CStr, c_char, c_void};
pub(super) type VulkanLoader = Option<
    unsafe extern "C" fn(super::vk::VkInstance, *const c_char) -> super::vk::PFN_vkVoidFunction,
>;
unsafe extern "C" {
    fn ww_metal_open() -> *mut c_void;
    fn ww_metal_layer(handle: *mut c_void) -> *mut c_void;
    fn ww_metal_dimensions(handle: *mut c_void, width: *mut u32, height: *mut u32);
    fn ww_metal_show(handle: *mut c_void);
    fn ww_metal_closed(handle: *mut c_void) -> bool;
    fn ww_metal_close(handle: *mut c_void);
}
pub(super) struct Window {
    pub handle: *mut c_void,
}
impl Window {
    pub unsafe fn new(opengl: bool, _: &CStr) -> Result<Self, String> {
        if opengl {
            return Err("OpenGL ES uses its EAGL surface client".into());
        }
        let handle = unsafe { ww_metal_open() };
        if handle.is_null() {
            return Err("iOS Metal surface unavailable".into());
        }
        Ok(Self { handle })
    }
    pub unsafe fn with_loader(opengl: bool, title: &CStr, _: VulkanLoader) -> Result<Self, String> {
        unsafe { Self::new(opengl, title) }
    }
    pub unsafe fn metal_layer(&self) -> *mut c_void {
        unsafe { ww_metal_layer(self.handle) }
    }
    pub unsafe fn dimensions(&self) -> Result<(u32, u32), String> {
        let (mut w, mut h) = (0, 0);
        unsafe { ww_metal_dimensions(self.handle, &mut w, &mut h) };
        if w == 0 || h == 0 || w > 8192 || h > 8192 {
            Err("invalid iOS drawable dimensions".into())
        } else {
            Ok((w, h))
        }
    }
    pub unsafe fn show(&self) {
        unsafe { ww_metal_show(self.handle) }
    }
    pub unsafe fn closed(&self) -> bool {
        unsafe { ww_metal_closed(self.handle) }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        unsafe { ww_metal_close(self.handle) }
    }
}
