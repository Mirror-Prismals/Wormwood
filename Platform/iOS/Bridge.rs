//! App bootstrap C ABI. This validates the same Host/contracts; it does not claim an iOS cue.
use serde_json::json;
use std::{
    ffi::{CStr, c_char},
    path::Path,
};
/// # Safety
/// Both arguments must be valid NUL-terminated UTF-8 strings for this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wormwood_ios_bootstrap(
    resources: *const c_char,
    output: *const c_char,
) -> i32 {
    if resources.is_null() || output.is_null() {
        return -1;
    }
    let result = std::panic::catch_unwind(|| -> Result<(), String> {
        let resources = unsafe { CStr::from_ptr(resources) }
            .to_str()
            .map_err(|e| e.to_string())?;
        let output = unsafe { CStr::from_ptr(output) }
            .to_str()
            .map_err(|e| e.to_string())?;
        let output = Path::new(output);
        std::fs::create_dir_all(output).map_err(|e| e.to_string())?;
        let execution = (|| {
            let mut host = crate::host::Host::load_paths(Path::new(resources), output)?;
            host.validate_runtime()
        })();
        let source_fingerprint =
            std::fs::read(Path::new(resources).join("source-fingerprint.json"))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                .and_then(|value| value["fingerprint"].as_str().map(str::to_owned));
        let mut report = match &execution {
            Ok(validation) => {
                json!({"mode":"ios_host_bootstrap","runtime_validated":true,"validation":validation,"graphics_verified":false,"audio_verified":false})
            }
            Err(error) => {
                json!({"mode":"ios_host_bootstrap","runtime_validated":false,"error":error,"graphics_verified":false,"audio_verified":false})
            }
        };
        report["source_fingerprint"] = json!(source_fingerprint);
        std::fs::write(
            output.join("host-bootstrap.json"),
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        execution.map(|_| ())
    });
    match result {
        Ok(Ok(())) => 0,
        _ => -1,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bootstrap_uses_packaged_contracts_and_separate_writable_output() {
        let source = std::ffi::CString::new(env!("CARGO_MANIFEST_DIR")).unwrap();
        let output =
            std::env::temp_dir().join(format!("wormwood-ios-bridge-{}", std::process::id()));
        let destination = std::ffi::CString::new(output.to_str().unwrap()).unwrap();
        assert_eq!(
            unsafe { wormwood_ios_bootstrap(source.as_ptr(), destination.as_ptr()) },
            0
        );
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output.join("host-bootstrap.json")).unwrap())
                .unwrap();
        assert_eq!(report["runtime_validated"], true);
        assert_eq!(report["graphics_verified"], false);
        assert!(output.join("execution.jsonl").exists());
        std::fs::remove_dir_all(output).unwrap();
    }
}

/// Run every explicitly selected native cue. The app calls this off the UIKit thread.
/// # Safety
/// Arguments must remain valid NUL-terminated UTF-8 strings for the call.
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wormwood_ios_run_cues(
    resources: *const c_char,
    output: *const c_char,
) -> i32 {
    if resources.is_null() || output.is_null() {
        return -1;
    }
    let result = std::panic::catch_unwind(|| -> Result<(), String> {
        let resources = Path::new(
            unsafe { CStr::from_ptr(resources) }
                .to_str()
                .map_err(|e| e.to_string())?,
        );
        let output = Path::new(
            unsafe { CStr::from_ptr(output) }
                .to_str()
                .map_err(|e| e.to_string())?,
        );
        let mut host = crate::host::Host::load_paths(resources, output)?;
        host.validate_runtime()?;
        let mut backends = Vec::new();
        for backend in ["OpenGL", "Vulkan", "WebGPU"] {
            let result = host.run_av(backend, 60);
            let report: serde_json::Value = serde_json::from_slice(
                &std::fs::read(output.join(format!("av-{backend}.json")))
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            backends.push(json!({"backend":backend,"passed":result.is_ok(),"report":report,"error":result.err()}));
        }
        let fingerprint: serde_json::Value = serde_json::from_slice(
            &std::fs::read(resources.join("source-fingerprint.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let passed = backends.iter().all(|v| v["passed"] == true);
        let report = json!({"mode":"ios_native_cues","execution_run_id":host.execution_run_id(),"source_fingerprint":fingerprint["fingerprint"],"passed":passed,"backends":backends});
        std::fs::write(
            output.join("ios-cues.json"),
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if passed {
            Ok(())
        } else {
            Err("one or more iOS native clients did not pass".into())
        }
    });
    match result {
        Ok(Ok(())) => 0,
        _ => -1,
    }
}

/// Native application entry; called off the UIKit thread, never from audio.
/// # Safety
/// All pointers must reference valid NUL-terminated UTF-8 for this call.
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wormwood_ios_run_game(
    resources: *const c_char,
    output: *const c_char,
    backend: *const c_char,
) -> i32 {
    if resources.is_null() || output.is_null() || backend.is_null() {
        return -1;
    }
    let result = std::panic::catch_unwind(|| -> Result<(), String> {
        let root = Path::new(
            unsafe { CStr::from_ptr(resources) }
                .to_str()
                .map_err(|e| e.to_string())?,
        );
        let output = Path::new(
            unsafe { CStr::from_ptr(output) }
                .to_str()
                .map_err(|e| e.to_string())?,
        );
        let backend = unsafe { CStr::from_ptr(backend) }
            .to_str()
            .map_err(|e| e.to_string())?;
        std::fs::create_dir_all(output).map_err(|e| e.to_string())?;
        let execution = (|| {
            if !["OpenGL", "Vulkan", "WebGPU"].contains(&backend) {
                return Err("unknown explicit iOS backend".into());
            }
            let project: serde_json::Value = serde_json::from_slice(
                &std::fs::read(root.join("Project.json")).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            let procedure = project["default_scene"]
                .as_str()
                .ok_or("missing default_scene")?;
            let mut host = crate::host::Host::load_paths(root, output)?;
            host.startup()?;
            host.run_interactive(procedure, backend, false)
        })();
        if let Err(error) = &execution {
            let _ = std::fs::write(output.join("session-error.txt"), error);
        }
        execution
    });
    match result {
        Ok(Ok(())) => 0,
        _ => -1,
    }
}

/// Run the isolated soil workshop on the native surface, off the UIKit thread.
/// # Safety
/// All pointers must reference valid NUL-terminated UTF-8 for this call.
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wormwood_ios_run_soil_workshop(
    resources: *const c_char,
    output: *const c_char,
    backend: *const c_char,
) -> i32 {
    unsafe {
        wormwood_ios_run_material_workshop(resources, output, backend, c"matter_dirt".as_ptr())
    }
}

/// Automatic material showcase selected by the native title screen.
/// # Safety
/// All four pointers must reference valid NUL-terminated UTF-8 for this call.
#[cfg(target_os = "ios")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn wormwood_ios_run_material_workshop(
    resources: *const c_char,
    output: *const c_char,
    backend: *const c_char,
    case: *const c_char,
) -> i32 {
    if resources.is_null() || output.is_null() || backend.is_null() || case.is_null() {
        return -1;
    }
    let result = std::panic::catch_unwind(|| -> Result<(), String> {
        let root = Path::new(
            unsafe { CStr::from_ptr(resources) }
                .to_str()
                .map_err(|e| e.to_string())?,
        );
        let output = Path::new(
            unsafe { CStr::from_ptr(output) }
                .to_str()
                .map_err(|e| e.to_string())?,
        );
        let backend = unsafe { CStr::from_ptr(backend) }
            .to_str()
            .map_err(|e| e.to_string())?;
        let case = unsafe { CStr::from_ptr(case) }
            .to_str()
            .map_err(|e| e.to_string())?;
        std::fs::create_dir_all(output).map_err(|e| e.to_string())?;
        let execution = (|| {
            if !["OpenGL", "Vulkan", "WebGPU"].contains(&backend) {
                return Err("unknown explicit iOS backend".into());
            }
            let mut host = crate::host::Host::load_paths(root, output)?;
            host.startup()?;
            if case == "workstation_panels" {
                return host.run_workstation(backend, None);
            }
            if !case.starts_with("matter_") || !crate::physics::is_case(case) {
                return Err("unknown material showcase".into());
            }
            host.run_workshop(case, backend, None)
        })();
        if let Err(error) = &execution {
            let _ = std::fs::write(output.join("session-error.txt"), error);
        }
        execution
    });
    match result {
        Ok(Ok(())) => 0,
        _ => -1,
    }
}
