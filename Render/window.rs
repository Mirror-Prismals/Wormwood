//! Minimal GLFW native window connection. Windows stay hidden until Go.
use libloading::Library;
use std::{
    ffi::{CStr, c_char, c_int, c_void},
    ptr,
};
type P = *mut c_void;
// Matches vkGetInstanceProcAddr as generated from the installed headers on
// macOS arm64 and Windows x64 (where __stdcall lowers to the C ABI).
pub(super) type VulkanLoader = Option<
    unsafe extern "C" fn(super::vk::VkInstance, *const c_char) -> super::vk::PFN_vkVoidFunction,
>;
pub(super) struct Window {
    pub handle: P,
    lib: Library,
    destroy: unsafe extern "C" fn(P),
    terminate: unsafe extern "C" fn(),
    poll: unsafe extern "C" fn(),
    should_close: unsafe extern "C" fn(P) -> c_int,
    show_window: unsafe extern "C" fn(P),
    captured: bool,
    cursor: [f64; 2],
    pub swap: unsafe extern "C" fn(P),
    framebuffer: unsafe extern "C" fn(P, *mut c_int, *mut c_int),
}
impl Window {
    pub unsafe fn new(opengl: bool, title: &CStr) -> Result<Self, String> {
        Self::with_loader(opengl, title, None)
    }
    pub unsafe fn with_loader(
        opengl: bool,
        title: &CStr,
        loader: VulkanLoader,
    ) -> Result<Self, String> {
        let path = std::env::var("WORMWOOD_GLFW_LIBRARY").unwrap_or_else(|_| {
            if cfg!(target_os = "macos") {
                "/opt/homebrew/lib/libglfw.dylib"
            } else if cfg!(target_os = "windows") {
                "glfw3.dll"
            } else {
                "libglfw.so.3"
            }
            .into()
        });
        let lib = Library::new(&path).map_err(|e| format!("GLFW {path}: {e}"))?;
        let init = *lib
            .get::<unsafe extern "C" fn() -> c_int>(b"glfwInit\0")
            .map_err(|e| e.to_string())?;
        let terminate = *lib
            .get::<unsafe extern "C" fn()>(b"glfwTerminate\0")
            .map_err(|e| e.to_string())?;
        let hint = *lib
            .get::<unsafe extern "C" fn(c_int, c_int)>(b"glfwWindowHint\0")
            .map_err(|e| e.to_string())?;
        let init_hint = *lib
            .get::<unsafe extern "C" fn(c_int, c_int)>(b"glfwInitHint\0")
            .map_err(|e| e.to_string())?;
        let create = *lib
            .get::<unsafe extern "C" fn(c_int, c_int, *const c_char, P, P) -> P>(
                b"glfwCreateWindow\0",
            )
            .map_err(|e| e.to_string())?;
        let destroy = *lib
            .get::<unsafe extern "C" fn(P)>(b"glfwDestroyWindow\0")
            .map_err(|e| e.to_string())?;
        let poll = *lib
            .get::<unsafe extern "C" fn()>(b"glfwPollEvents\0")
            .map_err(|e| e.to_string())?;
        let should_close = *lib
            .get::<unsafe extern "C" fn(P) -> c_int>(b"glfwWindowShouldClose\0")
            .map_err(|e| e.to_string())?;
        let show_window = *lib
            .get::<unsafe extern "C" fn(P)>(b"glfwShowWindow\0")
            .map_err(|e| e.to_string())?;
        let swap = *lib
            .get::<unsafe extern "C" fn(P)>(b"glfwSwapBuffers\0")
            .map_err(|e| e.to_string())?;
        let framebuffer = *lib
            .get::<unsafe extern "C" fn(P, *mut c_int, *mut c_int)>(b"glfwGetFramebufferSize\0")
            .map_err(|e| e.to_string())?;
        // GLFW snapshots init hints during glfwInit. Supply/reset the Vulkan
        // loader before that snapshot, not after creating a window.
        let init_loader = *lib
            .get::<unsafe extern "C" fn(VulkanLoader)>(b"glfwInitVulkanLoader\0")
            .map_err(|e| e.to_string())?;
        init_loader(loader);
        init_hint(0x00051002, 0); // GLFW_COCOA_MENUBAR
        if init() == 0 {
            return Err("GLFW initialization failed".into());
        }
        hint(0x00020004, 0); // GLFW_VISIBLE
        hint(0x00020003, 0); // GLFW_RESIZABLE
        if opengl {
            hint(0x00022002, 3);
            hint(0x00022003, 3);
            hint(0x00022006, 1);
            hint(0x00022008, 0x00032001);
        } else {
            hint(0x00022001, 0);
        } // GLFW_CLIENT_API = GLFW_NO_API
        let handle = create(640, 480, title.as_ptr(), ptr::null_mut(), ptr::null_mut());
        if handle.is_null() {
            terminate();
            return Err("GLFW window/context creation failed".into());
        }
        let result = Self {
            handle,
            lib,
            destroy,
            terminate,
            poll,
            should_close,
            show_window,
            captured: false,
            cursor: [0., 0.],
            swap,
            framebuffer,
        };
        if opengl {
            let current = result.symbol::<unsafe extern "C" fn(P)>(b"glfwMakeContextCurrent\0")?;
            current(handle);
            result.symbol::<unsafe extern "C" fn(c_int)>(b"glfwSwapInterval\0")?(1);
        }
        Ok(result)
    }
    pub unsafe fn symbol<T: Copy>(&self, name: &[u8]) -> Result<T, String> {
        self.lib
            .get::<T>(name)
            .map(|v| *v)
            .map_err(|e| e.to_string())
    }
    pub unsafe fn dimensions(&self) -> Result<(u32, u32), String> {
        let (mut w, mut h) = (0, 0);
        (self.framebuffer)(self.handle, &mut w, &mut h);
        if w <= 0 || h <= 0 || w > 8192 || h > 8192 {
            return Err("invalid framebuffer dimensions".into());
        }
        Ok((w as u32, h as u32))
    }
    pub unsafe fn show(&self) {
        (self.show_window)(self.handle);
        (self.poll)();
    }
    pub unsafe fn request_close(&self) -> Result<(), String> {
        self.symbol::<unsafe extern "C" fn(P, c_int)>(b"glfwSetWindowShouldClose\0")?(
            self.handle,
            1,
        );
        Ok(())
    }
    pub unsafe fn closed(&self) -> bool {
        (self.poll)();
        (self.should_close)(self.handle) != 0
    }
    pub unsafe fn flight_input(&mut self) -> Result<serde_json::Value, String> {
        let key = self.symbol::<unsafe extern "C" fn(P, c_int) -> c_int>(b"glfwGetKey\0")?;
        let mouse =
            self.symbol::<unsafe extern "C" fn(P, c_int) -> c_int>(b"glfwGetMouseButton\0")?;
        let focused = self
            .symbol::<unsafe extern "C" fn(P, c_int) -> c_int>(b"glfwGetWindowAttrib\0")?(
            self.handle,
            0x00020001,
        ) != 0;
        let capture = focused && mouse(self.handle, 1) == 1;
        let mut x = 0.;
        let mut y = 0.;
        self.symbol::<unsafe extern "C" fn(P, *mut f64, *mut f64)>(b"glfwGetCursorPos\0")?(
            self.handle,
            &mut x,
            &mut y,
        );
        let look = if capture && self.captured {
            [
                (x - self.cursor[0]).clamp(-1000., 1000.),
                (y - self.cursor[1]).clamp(-1000., 1000.),
            ]
        } else {
            [0., 0.]
        };
        if capture != self.captured {
            self.symbol::<unsafe extern "C" fn(P, c_int, c_int)>(b"glfwSetInputMode\0")?(
                self.handle,
                0x00033001,
                if capture { 0x00034003 } else { 0x00034001 },
            );
        }
        self.captured = capture;
        self.cursor = [x, y];
        let movement = [87, 83, 65, 68, 32, 340].map(|k| focused && key(self.handle, k) == 1);
        Ok(
            serde_json::json!({"move":movement,"look":look,"reset":focused&&key(self.handle,82)==1,"roll":if focused {f64::from(key(self.handle,81)==1)-f64::from(key(self.handle,69)==1)}else{0.},"boost":focused&&key(self.handle,341)==1}),
        )
    }
    pub unsafe fn keys(&self) -> Result<[bool; 3], String> {
        let get = self.symbol::<unsafe extern "C" fn(P, c_int) -> c_int>(b"glfwGetKey\0")?;
        Ok([
            get(self.handle, 32) == 1,
            get(self.handle, 256) == 1,
            get(self.handle, 81) == 1,
        ])
    }
    pub unsafe fn gl_proc(&self, name: &CStr) -> Result<*const c_void, String> {
        let get = self.symbol::<unsafe extern "C" fn(*const c_char) -> *const c_void>(
            b"glfwGetProcAddress\0",
        )?;
        let address = get(name.as_ptr());
        if address.is_null() {
            Err(format!(
                "missing OpenGL function {}",
                name.to_string_lossy()
            ))
        } else {
            Ok(address)
        }
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            (self.destroy)(self.handle);
            (self.terminate)();
        }
    }
}
