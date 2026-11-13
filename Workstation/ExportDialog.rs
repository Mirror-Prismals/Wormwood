//! Main-thread bridge for the modeless macOS export settings/progress window.
//! The application polls requests and supplies worker status; this window never
//! renders audio, creates output files, or starts an export on its own.
use serde_json::Value;

#[cfg(target_os = "macos")]
mod native {
    use serde_json::Value;
    use std::ffi::{CString, c_char, c_int};

    const JSON_CAPACITY: usize = 65_536;
    const ERROR_CAPACITY: usize = 4096;

    unsafe extern "C" {
        fn ww_export_dialog_open(
            input: *const c_char,
            error: *mut c_char,
            capacity: usize,
        ) -> c_int;
        fn ww_export_dialog_poll(
            output: *mut c_char,
            output_capacity: usize,
            error: *mut c_char,
            error_capacity: usize,
        ) -> c_int;
        fn ww_export_dialog_update(
            input: *const c_char,
            error: *mut c_char,
            capacity: usize,
        ) -> c_int;
        fn ww_export_dialog_close(error: *mut c_char, capacity: usize) -> c_int;
    }

    fn input(value: &Value) -> Result<CString, String> {
        let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
        if bytes.len() > JSON_CAPACITY {
            return Err("export dialog JSON exceeds 65536 bytes".into());
        }
        CString::new(bytes).map_err(|_| "export dialog JSON contains a NUL byte".into())
    }

    fn result(code: c_int, error: &[u8]) -> Result<(), String> {
        if code == 0 {
            return Ok(());
        }
        let length = error.iter().position(|b| *b == 0).unwrap_or(error.len());
        let message = String::from_utf8_lossy(&error[..length]);
        Err(if message.is_empty() {
            "native export dialog failed".into()
        } else {
            message.into_owned()
        })
    }

    pub(super) fn open(defaults: &Value) -> Result<(), String> {
        let input = input(defaults)?;
        let mut error = [0u8; ERROR_CAPACITY];
        // Cocoa checks the calling thread before touching any UI state.
        let code = unsafe {
            ww_export_dialog_open(input.as_ptr(), error.as_mut_ptr().cast(), error.len())
        };
        result(code, &error)
    }

    pub(super) fn poll() -> Result<Value, String> {
        let mut output = vec![0u8; JSON_CAPACITY + 1];
        let mut error = [0u8; ERROR_CAPACITY];
        let code = unsafe {
            ww_export_dialog_poll(
                output.as_mut_ptr().cast(),
                output.len(),
                error.as_mut_ptr().cast(),
                error.len(),
            )
        };
        result(code, &error)?;
        let length = output
            .iter()
            .position(|b| *b == 0)
            .ok_or("native export dialog returned unterminated JSON")?;
        serde_json::from_slice(&output[..length]).map_err(|e| format!("export dialog request: {e}"))
    }

    pub(super) fn update(status: &Value) -> Result<(), String> {
        let input = input(status)?;
        let mut error = [0u8; ERROR_CAPACITY];
        let code = unsafe {
            ww_export_dialog_update(input.as_ptr(), error.as_mut_ptr().cast(), error.len())
        };
        result(code, &error)
    }

    pub(super) fn close() -> Result<(), String> {
        let mut error = [0u8; ERROR_CAPACITY];
        // Application teardown cancels the worker before invalidating all UI
        // requests. This differs from a user closing the window, which queues
        // cancellation only if a start has already reached the application.
        let code = unsafe { ww_export_dialog_close(error.as_mut_ptr().cast(), error.len()) };
        result(code, &error)
    }
}

pub(crate) fn open(defaults: &Value) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        native::open(defaults)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = defaults;
        Err("the native export dialog is available on macOS only".into())
    }
}

pub(crate) fn poll() -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        native::poll()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(Value::Null)
    }
}

pub(crate) fn update(status: &Value) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        native::update(status)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = status;
        Ok(())
    }
}

pub(crate) fn close() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        native::close()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}
