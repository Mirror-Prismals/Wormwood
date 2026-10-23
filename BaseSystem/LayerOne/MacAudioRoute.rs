//! CoreAudio queries run only in registered JACK hooks, never the audio callback.
#[cfg(target_os = "macos")]
mod native {
    use std::ffi::{c_char, c_void};
    #[repr(C)]
    struct Address {
        selector: u32,
        scope: u32,
        element: u32,
    }
    #[link(name = "CoreAudio", kind = "framework")]
    unsafe extern "C" {
        fn AudioObjectGetPropertyData(
            object: u32,
            address: *const Address,
            qualifier_size: u32,
            qualifier: *const c_void,
            size: *mut u32,
            data: *mut c_void,
        ) -> i32;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringGetCString(
            value: *const c_void,
            buffer: *mut c_char,
            size: isize,
            encoding: u32,
        ) -> bool;
        fn CFRelease(value: *const c_void);
    }
    unsafe fn property<T>(object: u32, selector: [u8; 4], value: &mut T) -> Result<(), String> {
        let address = Address {
            selector: u32::from_be_bytes(selector),
            scope: u32::from_be_bytes(*b"glob"),
            element: 0,
        };
        let mut size = std::mem::size_of::<T>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                object,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                value as *mut T as *mut c_void,
            )
        };
        if status != 0 || size != std::mem::size_of::<T>() as u32 {
            return Err(format!("CoreAudio property query failed: {status}"));
        }
        Ok(())
    }
    pub(super) fn default_uid() -> Result<String, String> {
        let mut device = 0u32;
        // Constants and signatures are from AudioHardware.h/AudioHardwareBase.h.
        unsafe {
            property(1, *b"dOut", &mut device)?;
        }
        if device == 0 {
            return Err("macOS has no default audio output".into());
        }
        let mut value: *const c_void = std::ptr::null();
        unsafe {
            property(device, *b"uid ", &mut value)?;
        }
        if value.is_null() {
            return Err("CoreAudio returned no output UID".into());
        }
        let mut bytes = [0u8; 4096];
        // The returned CFString property is owned by the caller.
        let converted = unsafe {
            CFStringGetCString(
                value,
                bytes.as_mut_ptr().cast(),
                bytes.len() as isize,
                0x08000100,
            )
        };
        unsafe {
            CFRelease(value);
        }
        if !converted {
            return Err("CoreAudio output UID conversion failed".into());
        }
        let end = bytes
            .iter()
            .position(|b| *b == 0)
            .ok_or("unterminated output UID")?;
        String::from_utf8(bytes[..end].to_vec()).map_err(|e| e.to_string())
    }
}
pub(super) fn default_uid() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    {
        native::default_uid()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("CoreAudio output queries require macOS".into())
    }
}
/// Explicit device selection stays pinned; default selection follows the OS.
pub(super) fn changed(
    follows_default: bool,
    selected: &str,
    current: Result<String, String>,
) -> bool {
    follows_default && current.as_deref().map_or(true, |uid| uid != selected)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_policy_handles_switch_disconnect_and_pinned_device() {
        assert!(!changed(true, "BT", Ok("BT".into())));
        assert!(changed(true, "BT", Ok("BuiltInSpeakerDevice".into())));
        assert!(changed(true, "BT", Err("no output".into())));
        assert!(!changed(false, "USB", Ok("BT".into())));
        assert!(!changed(false, "USB", Err("no output".into())));
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn native_default_output_has_a_uid() {
        assert!(!default_uid().unwrap().is_empty());
    }
}
