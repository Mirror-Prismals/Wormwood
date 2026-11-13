use std::{env, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-changed=Render/WorkstationCcParameterPicker.m");
    println!("cargo:rerun-if-changed=Render/CcStbEasyFont.h");
    println!("cargo:rerun-if-changed=Audio/Plugins/native/AutomationParameters.h");
    let ios = env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    // Assemble the shared material tail at build time, preserving walnut bytes.
    let mut scene_materials=Vec::new();
    for path in ["Procedures/Assets/Theatre/table_material.rgba", "Procedures/Assets/Terrain/rock_materials.rgba", "Procedures/Assets/Terrain/grass_material.rgba", "Procedures/Assets/Terrain/dirt_material.rgba", "Procedures/Assets/Theatre/exterior_material.rgba", "Procedures/Assets/Theatre/carpet_material.rgba"] {
        println!("cargo:rerun-if-changed={path}");
        scene_materials.extend(std::fs::read(path).expect("scene material bake is required"));
    }
    assert_eq!(scene_materials.len(),18_175_376);
    std::fs::write(out.join("scene_materials.rgba"),scene_materials).expect("write shared material tail");

    if cfg!(target_os = "macos") && env::var_os("LIBCLANG_PATH").is_none() {
        // build.rs is single-threaded here, before loading libclang.
        unsafe {
            env::set_var(
                "LIBCLANG_PATH",
                "/Library/Developer/CommandLineTools/usr/lib",
            );
        }
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref()==Ok("macos") {
        println!("cargo:rerun-if-changed=Render/MaterialHUD.m");
        println!("cargo:rerun-if-changed=Render/PauseMenu.m");
        let object=out.join("material-hud.o");let archive=out.join("libmirror_material_hud.a");
        let pause_object=out.join("pause-menu.o");
        let display_object=out.join("display-cadence.o");
        let aggregate_object=out.join("mac-audio-aggregate.o");
        println!("cargo:rerun-if-changed=BaseSystem/LayerOne/MacAudioAggregate.m");
        println!("cargo:rustc-link-lib=framework=CoreAudio");
        let controls_object=out.join("workstation-controls.o");
        println!("cargo:rerun-if-changed=Render/WorkstationControls.m");
        println!("cargo:rerun-if-changed=Render/WorkstationPluginSurface.m");
        println!("cargo:rerun-if-changed=Render/DisplayCadence.m");
        println!("cargo:rustc-link-lib=framework=CoreGraphics");
        println!("cargo:rustc-link-lib=framework=CoreText");
        println!("cargo:rerun-if-changed=Workstation/ExportDialog.m");
        println!("cargo:rerun-if-changed=Workstation/AudioImportDialog.m");
        let export_object=out.join("workstation-export-dialog.o");
        let arch=if env::var("CARGO_CFG_TARGET_ARCH").as_deref()==Ok("aarch64"){"arm64"}else{"x86_64"};
        assert!(Command::new("xcrun").args(["clang","-arch",arch,"-fobjc-arc","-mmacosx-version-min=13.0","-c","Render/MaterialHUD.m","-o"]).arg(&object).status().expect("compile material HUD").success());
        assert!(Command::new("xcrun").args(["clang","-arch",arch,"-fobjc-arc","-mmacosx-version-min=13.0","-c","Render/PauseMenu.m","-o"]).arg(&pause_object).status().expect("compile pause menu").success());
        assert!(Command::new("xcrun").args(["clang","-arch",arch,"-fobjc-arc","-mmacosx-version-min=13.0","-c","Workstation/ExportDialog.m","-o"]).arg(&export_object).status().expect("compile workstation export dialog").success());
        assert!(Command::new("xcrun").args(["clang","-arch",arch,"-fobjc-arc","-mmacosx-version-min=13.0","-c","Render/DisplayCadence.m","-o"]).arg(&display_object).status().expect("compile display metadata reader").success());
        assert!(Command::new("xcrun").args(["clang","-arch",arch,"-fobjc-arc","-mmacosx-version-min=13.0","-c","Render/WorkstationControls.m","-o"]).arg(&controls_object).status().expect("compile workstation controls").success());
        assert!(Command::new("xcrun").args(["clang","-arch",arch,"-fobjc-arc","-mmacosx-version-min=13.0","-c","BaseSystem/LayerOne/MacAudioAggregate.m","-o"]).arg(&aggregate_object).status().expect("compile CoreAudio duplex setup").success());
        assert!(Command::new("xcrun").args(["ar","rcs"]).arg(&archive).arg(&object).arg(&pause_object).arg(&export_object).arg(&display_object).arg(&controls_object).arg(&aggregate_object).status().expect("archive native game UI").success());
        println!("cargo:rustc-link-search=native={}",out.display());println!("cargo:rustc-link-lib=static=mirror_material_hud");println!("cargo:rustc-link-lib=framework=Cocoa");
    }
// Native VST3/CLAP/AU/VST2 hosting is desktop-only; no SDK/compiler/link
// dependency from this subtree participates in an iOS build.
if env::var("CARGO_CFG_TARGET_OS").as_deref()==Ok("macos") {
    println!("cargo:rerun-if-changed=Audio/Plugins");
    let native=out.join("plugin-native");
    let arch=if env::var("CARGO_CFG_TARGET_ARCH").as_deref()==Ok("aarch64"){"arm64"}else{"x86_64"};
    assert!(Command::new("cmake").args(["-S","Audio/Plugins","-B"]).arg(&native)
        .args(["-G","Unix Makefiles","-DCMAKE_BUILD_TYPE=Release","-DCMAKE_OSX_DEPLOYMENT_TARGET=13.0"])
        .arg(format!("-DCMAKE_OSX_ARCHITECTURES={arch}")).status().expect("configure plugin host").success());
    assert!(Command::new("cmake").arg("--build").arg(&native).args(["--parallel","4"])
        .status().expect("compile plugin host").success());
    println!("cargo:rustc-link-search=native={}",native.display());
    println!("cargo:rustc-link-lib=static=wormwood_plugins");
    for framework in ["Cocoa","AudioToolbox","AudioUnit","CoreFoundation","QuartzCore"] {
        println!("cargo:rustc-link-lib=framework={framework}");
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
        let helper = if name == "vk" { "Render/Water/vulkan.rs" } else { "Render/Water/webgpu.rs" };
        println!("cargo:rerun-if-changed={helper}");
        let plants=if name=="vk"{"Render/Vegetation/vulkan.rs"}else{"Render/Vegetation/webgpu.rs"};
        println!("cargo:rerun-if-changed={plants}");
        let code = format!("{}\n{}\n{}", std::fs::read_to_string(source).unwrap(), std::fs::read_to_string(helper).unwrap(), std::fs::read_to_string(plants).unwrap());
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
                "^WGPU(NativeFeature|NativeSType|SurfaceSourceMetalLayer|SurfaceSourceWindowsHWND|SurfaceConfigurationExtras|ShaderSourceWGSL)$"
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
    // The workshop/contact client is independent of audio and graphics callbacks.
    if matches!(env::var("CARGO_CFG_TARGET_OS").as_deref(),Ok("macos"|"ios")) {
        let mut objects = Vec::new();
        for name in ["OffsetGeometricContact.cpp", "Bridge.cpp"] {
            let source = PathBuf::from("Physics/OGC").join(name);
            println!("cargo:rerun-if-changed={}", source.display());
            let object = out.join(format!("{name}.o"));
            let mut compiler=Command::new("xcrun");
            if ios {compiler.args(["--sdk","iphoneos","clang++","-target","arm64-apple-ios16.0"]);}else{compiler.arg("clang++");}
            let status = compiler
                .args([
                    "-std=c++17",
                    "-O2",
                    "-fPIC",
                    "-IPhysics/OGC",
                    "-c",
                ])
                .arg(format!("-I{prefix}/include"))
                .arg(&source)
                .arg("-o")
                .arg(&object)
                .status()
                .expect("C++ compiler required for OGC");
            assert!(status.success(), "OGC compilation failed");
            objects.push(object);
        }
        println!("cargo:rerun-if-changed=Physics/OGC/OffsetGeometricContact.h");
        let archive = out.join("libwormwood_ogc.a");
        let status = Command::new("xcrun")
            .args(["ar", "rcs"])
            .arg(&archive)
            .args(&objects)
            .status()
            .expect("archiver required");
        assert!(status.success());
        println!("cargo:rustc-link-search=native={}", out.display());
        println!("cargo:rustc-link-lib=static=wormwood_ogc");
        println!("cargo:rustc-link-lib=c++");
    }
    // Build the already-vendored embedded VM with the target platform toolchain.
    // No network access, package installation or external audio service is involved.
    if matches!(env::var("CARGO_CFG_TARGET_OS").as_deref(), Ok("macos" | "ios")) {
        println!("cargo:rerun-if-changed=Audio/native");
        let native = out.join("chuck-native");
        let mut configure = Command::new("cmake");
        configure.args(["-S", "Audio/native", "-B"]).arg(&native)
            .args(["-G", "Unix Makefiles", "-DCMAKE_BUILD_TYPE=Release"]);
        let arch = if env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("aarch64") { "arm64" } else { "x86_64" };
        configure.arg(format!("-DCMAKE_OSX_ARCHITECTURES={arch}"));
        if ios {
            configure.args(["-DCMAKE_SYSTEM_NAME=iOS", "-DCMAKE_OSX_SYSROOT=iphoneos", "-DCMAKE_OSX_DEPLOYMENT_TARGET=16.0"]);
        }
        assert!(configure.status().expect("CMake required for embedded ChucK").success(), "ChucK configure failed");
        assert!(Command::new("cmake").arg("--build").arg(&native)
            .args(["--target", "mirror_chuck", "--parallel", "2"])
            .status().expect("ChucK native build required").success(), "ChucK native build failed");
        println!("cargo:rustc-link-search=native={}", native.display());
        println!("cargo:rustc-link-lib=static=mirror_chuck");
    }
    for (directory, shader, output) in [
        ("Procedures/Shaders", "triangle.vert", "triangle.vert.spv"),
        ("Procedures/Shaders", "workstation.vert", "workstation.vert.spv"),
        ("Procedures/Shaders", "workstation.frag", "workstation.frag.spv"),
        ("Procedures/Shaders", "vegetation.vert", "vegetation.vert.spv"),
        ("Procedures/Shaders", "vegetation.frag", "vegetation.frag.spv"),
        ("Procedures/Shaders", "vegetation.shadow.vk.frag", "vegetation.shadow.spv"),
        ("Procedures/Shaders", "triangle.frag", "triangle.frag.spv"),
        ("Procedures/Water/Shaders", "water.vert", "water.vert.spv"),
        ("Procedures/Water/Shaders", "water.frag", "water.frag.spv"),
        ("Procedures/Water/Shaders", "composite.vert", "water-composite.vert.spv"),
        ("Procedures/Water/Shaders", "composite.frag", "water-composite.frag.spv"),
    ] {
        let source = format!("{directory}/{shader}");
        println!("cargo:rerun-if-changed={source}");
        let result = Command::new(
            env::var_os("WORMWOOD_GLSLANG").unwrap_or_else(|| "glslangValidator".into()),
        )
        .args(["-V", &source, "-o"])
        .arg(out.join(output))
        .output()
        .expect("glslangValidator is required");
        assert!(
            result.status.success(),
            "SPIR-V compilation failed: {}",
            String::from_utf8_lossy(&result.stdout)
        );
    }
}
