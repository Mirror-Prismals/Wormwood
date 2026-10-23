//! Inventory only: presence is not successful loading or device/API support.
use serde_json::{Value, json};
use std::{path::Path, process::Command};

fn package_version(name: &str) -> Value {
    match Command::new("pkg-config")
        .args(["--modversion", name])
        .output()
    {
        Ok(result) if result.status.success() => json!({
            "package": name, "version": String::from_utf8_lossy(&result.stdout).trim()
        }),
        _ => json!({"package": name, "version": null}),
    }
}

fn existing(paths: &[&str]) -> Vec<String> {
    paths
        .iter()
        .filter(|p| Path::new(p).exists())
        .map(|p| p.to_string())
        .collect()
}

pub(crate) fn probe() -> Result<Value, String> {
    Ok(json!({
        "mode": "inventory_only",
        "os": std::env::consts::OS,
        "architecture": std::env::consts::ARCH,
        "backends": [
            {"name": "WebGPU", "device_test": "not_implemented",
             "package": package_version("wgpu-native"),
             "candidate_files": existing(&["/opt/homebrew/lib/libwgpu_native.dylib", "/usr/local/lib/libwgpu_native.dylib"])},
            {"name": "Vulkan", "device_test": "not_implemented",
             "package": package_version("vulkan"),
             "candidate_files": existing(&["/opt/homebrew/lib/libvulkan.dylib", "/usr/local/lib/libvulkan.dylib"])},
            {"name": "OpenGL", "device_test": "not_implemented",
             "candidate_files": existing(&["/System/Library/Frameworks/OpenGL.framework"])}
        ],
        "jack": {"server_connection": "not_attempted", "package": package_version("jack"),
                 "candidate_files": existing(&["/opt/homebrew/lib/libjack.dylib", "/usr/local/lib/libjack.dylib"])},
        "chuck": {"vm": "not_initialized", "priority": "after_jack"},
        "limitations": ["No native libraries loaded", "No window or graphics device created", "No triangle rendered", "No audio produced", "Candidate paths are currently macOS-oriented"]
    }))
}
