//! Standalone finite fixture preparation; compile/run through the parent job.
//! Usage: export_ruby_cache SHADER FRAME OUTPUT_PREFIX
//! The frame is a captured full optical packet. Output contains the same source
//! generator and plan used by the application, with alpha-only acceptance counts.
#[path = "../../Workstation/RubyResponsePlan.rs"]
mod ruby_response_plan;
#[path = "../../Workstation/RubyResponseShader.rs"]
mod ruby_response_shader;

use std::{fs, io::Write, path::{Path, PathBuf}};

const FRAME_ROWS: usize = 49_408;
const MAX_FRAME_BYTES: u64 = 128 * 1024 * 1024;

fn read(path: &Path, maximum: u64) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err(format!("{} exceeds the bounded input size", path.display()));
    }
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325u64,
        |hash, byte| (hash ^ *byte as u64).wrapping_mul(0x100000001b3));
    format!("{hash:016x}")
}

fn quote(value: &str) -> String {
    let mut result = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c < ' ' => result.push_str(&format!("\\u{:04x}", c as u32)),
            c => result.push(c),
        }
    }
    result.push('"');
    result
}

fn write_new(path: &Path, contents: &[u8]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(contents).map_err(|e| format!("{}: {e}", path.display()))
}

fn replace_once(source: &mut String, old: &str, new: &str) -> Result<(), String> {
    if source.matches(old).count() != 1 {
        return Err(format!("ruby cache diagnostic marker changed: {old}"));
    }
    *source = source.replacen(old, new, 1);
    Ok(())
}

fn main_impl() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 4 { return Err("Usage: export_ruby_cache SHADER FRAME OUTPUT_PREFIX".into()); }
    let shader_path = Path::new(&args[1]).canonicalize().map_err(|e| e.to_string())?;
    let frame_path = Path::new(&args[2]).canonicalize().map_err(|e| e.to_string())?;
    let prefix = PathBuf::from(&args[3]);
    let directory = prefix.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let shader_output = prefix.with_extension("wgsl");
    let plan_output = prefix.with_extension("plan.bin");
    let report_output = prefix.with_extension("json");
    for path in [&shader_output, &plan_output, &report_output] {
        if path.exists() { return Err(format!("Refusing to replace {}", path.display())); }
    }
    let source_bytes = read(&shader_path, 2 * 1024 * 1024)?;
    let original = std::str::from_utf8(&source_bytes).map_err(|e| e.to_string())?;
    let bytes = read(&frame_path, MAX_FRAME_BYTES)?;
    if bytes.len() % 16 != 0 || bytes.len() < FRAME_ROWS * 16 {
        return Err("Optical frame must contain complete float4 rows and the 49408-row dynamic prefix".into());
    }
    let mut frame = Vec::with_capacity(bytes.len() / 16);
    for words in bytes.chunks_exact(16) {
        let mut row = [0.; 4];
        for (value, word) in row.iter_mut().zip(words.chunks_exact(4)) {
            *value = f32::from_le_bytes(word.try_into().unwrap());
            if !value.is_finite() { return Err("Optical frame contains nonfinite words".into()); }
        }
        frame.push(row);
    }
    if frame[7][0] != FRAME_ROWS as f32 || frame[7][1] != 57. {
        return Err("Optical packet dynamic prefix or display LUT version changed".into());
    }
    let mut plan = ruby_response_plan::Plan::new(&frame);
    let update = plan.update(&frame[..FRAME_ROWS]);
    if update.ready_count == 0 || !update.dirty {
        return Err("Fixture contains no supported dirty ruby response entries".into());
    }
    let mut shader = ruby_response_shader::source(original)?;
    replace_once(&mut shader, "rubyUsed=true;finished=true;",
                 "rubyUsed=true;finished=true;rubyUsedSamples+=1u;")?;
    if shader.contains("// RUBY_PRIMARY_CACHED") {
        replace_once(&mut shader, "// RUBY_PRIMARY_CACHED", "rubyUsedSamples+=1u;")?;
    }
    replace_once(&mut shader, "fn workstationShade(screen:vec2<f32>)->vec4<f32> {",
                 "fn workstationShade(screen:vec2<f32>)->vec4<f32> {\n    rubyUsedSamples=0u;")?;
    replace_once(&mut shader, "return vec4<f32>(gemDisplay(result/samples),1.0);",
                 "return vec4<f32>(gemDisplay(result/samples),f32(rubyUsedSamples));")?;
    shader.insert_str(0, "var<private> rubyUsedSamples:u32=0u;\n");
    // Validate the instrumented artifact too: acceptance diagnostics must not
    // turn a valid production module into an unchecked probe-only variant.
    let module = naga::front::wgsl::parse_str(&shader)
        .map_err(|e| format!("instrumented shader: {}", e.emit_to_string(&shader)))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module).map_err(|e| format!("instrumented shader: {e:?}"))?;
    let mut plan_bytes = Vec::with_capacity(update.rows.len() * 16);
    for row in &update.rows { for value in row { plan_bytes.extend(value.to_le_bytes()); } }
    let mut entries = Vec::new();
    let mut panels = 0;
    let mut controls = 0;
    for slot in 0..update.entry_count as usize {
        let descriptor = &update.rows[1+slot*8..1+(slot+1)*8];
        if descriptor[0][3] < 0.5 { continue; }
        let target = descriptor[0][0] as usize;
        if target < 4 { panels += 1; } else { controls += 1; }
        let material = if target < 4 { 32+40*target } else {
            let instance = (target-4)/256;
            let pose = if instance >= 61 { frame[232][0] as usize+32+2*(instance-61) }
                else if instance >= 29 { frame[247][0] as usize+2*(instance-29) }
                else if instance >= 13 { frame[232][0] as usize+2*(instance-13) }
                else if instance >= 10 { 226+2*(instance-10) }
                else { 192+2*instance };
            let base = if frame[pose+1][3] > 0.5 { frame[pose+1][3] } else { frame[12][1] };
            base as usize + 40*((target-4)%256)
        };
        entries.push(format!(
            "    {{\"slot\":{slot},\"target\":{target},\"material_row\":{material},\"edge\":{},\"data_base_row\":{},\"state\":{},\"pose\":{:?},\"bounds\":{:?}}}",
            descriptor[0][1], descriptor[0][2], descriptor[0][3], descriptor[1], descriptor[2]));
    }
    let report = format!(
        "{{\n  \"schema\":1,\n  \"shader\":{},\n  \"frame\":{},\n  \"source_fingerprint\":{},\n  \"frame_fingerprint\":{},\n  \"shader_output\":{},\n  \"plan_output\":{},\n  \"frame_rows\":{},\n  \"dynamic_rows\":{},\n  \"plan_rows\":{},\n  \"cache_bytes\":{},\n  \"entry_count\":{},\n  \"ready_count\":{},\n  \"panel_entries\":{},\n  \"control_entries\":{},\n  \"eligible_control_components\":{},\n  \"admitted_control_components\":{},\n  \"dirty\":{},\n  \"alpha\":\"actual primary samples using a cached ruby response (0..2)\",\n  \"descriptors\":[\n{}\n  ]\n}}\n",
        quote(&shader_path.to_string_lossy()), quote(&frame_path.to_string_lossy()),
        quote(&fingerprint(&source_bytes)), quote(&fingerprint(&bytes)),
        quote(&shader_output.to_string_lossy()), quote(&plan_output.to_string_lossy()),
        frame.len(), FRAME_ROWS, ruby_response_plan::PLAN_ROWS, ruby_response_plan::CACHE_BYTES,
        update.entry_count, update.ready_count, panels, controls, update.rows[0][2], update.rows[0][3], update.dirty, entries.join(",\n"));
    write_new(&shader_output, shader.as_bytes())?;
    write_new(&plan_output, &plan_bytes)?;
    write_new(&report_output, report.as_bytes())?;
    println!("Exported {} ruby entries ({} panels, {} controls): {}",
             update.ready_count, panels, controls, report_output.display());
    Ok(())
}

fn main() {
    if let Err(error) = main_impl() { eprintln!("{error}"); std::process::exit(1); }
}
