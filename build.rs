use std::{env, path::PathBuf, process::Command};
fn main() {
    let ios = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    if cfg!(target_os = "macos") && env::var_os("LIBCLANG_PATH").is_none() {
        // build.rs is single-threaded here, before loading libclang.
        unsafe {
            env::set_var(
                "LIBCLANG_PATH",
                "/Library/Developer/CommandLineTools/usr/lib",
            );
        }
    }
    let prefix = env::var("WORMWOOD_NATIVE_PREFIX").unwrap_or_else(|_| "/opt/homebrew".into());
    println!("cargo:rerun-if-env-changed=WORMWOOD_NATIVE_PREFIX");
    println!("cargo:rerun-if-env-changed=LIBCLANG_PATH");
    println!("cargo:rerun-if-env-changed=WORMWOOD_WEBGPU_INCLUDE");
    println!("cargo:rerun-if-env-changed=WORMWOOD_GLSLANG");
    for (name, header, source) in [
        ("vk", "vulkan/vulkan.h", "Render/vulkan.rs"),
        ("wgpu", "wgpu.h", "Render/webgpu.rs"),
    ] {
        println!("cargo:rerun-if-changed={source}");
        let code = std::fs::read_to_string(source).unwrap();
        let function_prefix = if name == "vk" { "vk" } else { "wgpu" };
        let symbols: std::collections::BTreeSet<String> = code
            .split('.')
            .filter(|piece| piece.starts_with(function_prefix))
            .map(|piece| {
                piece
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect()
            })
            .collect();
        let pattern = format!("^({})$", symbols.into_iter().collect::<Vec<_>>().join("|"));
        let input = if name == "wgpu" {
            env::var_os("WORMWOOD_WEBGPU_INCLUDE")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(&prefix).join("include"))
                .join(header)
        } else {
            PathBuf::from(&prefix).join("include").join(header)
        };
        println!("cargo:rerun-if-changed={}", input.display());
        let mut builder = bindgen::Builder::default().header(input.to_str().unwrap());
        if cfg!(target_os = "macos") {
            let sdk = Command::new("xcrun")
                .args([
                    "--sdk",
                    if ios { "iphoneos" } else { "macosx" },
                    "--show-sdk-path",
                ])
                .output()
                .expect("macOS SDK required");
            assert!(sdk.status.success(), "cannot locate macOS SDK");
            builder = builder
                .clang_arg("-isysroot")
                .clang_arg(String::from_utf8(sdk.stdout).unwrap().trim());
        }
        if ios {
            builder = builder
                .clang_arg("-target")
                .clang_arg("arm64-apple-ios16.0")
                .clang_arg("-DVK_USE_PLATFORM_METAL_EXT");
        }
        builder
            .clang_arg(format!("-I{prefix}/include"))
            .allowlist_function(&pattern)
            .allowlist_var(if name == "vk" { "VK_.*" } else { "WGPU.*" })
            .allowlist_type(if name == "vk" {
                "^(Vk.*FlagBits.*|PFN_vkGetInstanceProcAddr)$"
            } else {
                "^WGPU(SurfaceSourceMetalLayer|SurfaceSourceWindowsHWND|ShaderSourceWGSL)$"
            })
            .derive_default(true)
            .layout_tests(false)
            .generate_comments(false)
            .prepend_enum_name(false)
            .dynamic_library_name("Api")
            .dynamic_link_require_all(true)
            .generate()
            .expect("native API headers must be available")
            .write_to_file(out.join(format!("{name}.rs")))
            .unwrap();
    }
    for shader in ["triangle.vert", "triangle.frag"] {
        let source = format!("Procedures/Shaders/{shader}");
        println!("cargo:rerun-if-changed={source}");
        let result = Command::new(
            env::var_os("WORMWOOD_GLSLANG").unwrap_or_else(|| "glslangValidator".into()),
        )
        .args(["-V", &source, "-o"])
        .arg(out.join(format!("{shader}.spv")))
        .output()
        .expect("glslangValidator is required");
        assert!(
            result.status.success(),
            "SPIR-V compilation failed: {}",
            String::from_utf8_lossy(&result.stdout)
        );
    }
}
