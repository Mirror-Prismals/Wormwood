//! Bounded production timeline/cache fixture. Parent owns compilation and GPU runs.
//! Usage: export_ruby_timeline SHADER ORIGINAL_FRAME ASSET_ROOT OUTPUT_PREFIX
//! WORMWOOD_WORKSTATION_PIXEL_TRANSPORT=0/1 selects baseline/candidate.
//! Both use identical real TimelineScene geometry and the native benchmark's
//! four handles plus one tape clip. This does not start or measure audio.
#![allow(dead_code)]

#[path = "../../Workstation/RubyResponsePlan.rs"]
mod ruby_response_plan;
#[path = "../../Workstation/RubyResponseShader.rs"]
mod ruby_response_shader;
#[path = "../../Workstation/TimelineScene.rs"]
mod timeline_scene;

// Minimal layout/constant dependencies of the actual TimelineScene packer.
// Their source markers and the input packet ABI are checked before use.
mod piano_roll {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct Rect { pub left: f64, pub top: f64, pub width: f64, pub height: f64 }
}
mod panel_layout {
    #[derive(Clone, Copy, Debug)]
    pub(crate) struct Viewport { pub width: f64, pub height: f64, pub canvas_scale: f64 }
}
mod arrangement {
    use super::piano_roll::Rect;
    pub(crate) const RULER_HEIGHT: f64 = 32.;
    pub(crate) const LANE_HEIGHT: f64 = 64.;
    pub(crate) const BODY_OFFSET: f64 = 128.;
    pub(crate) const MONITOR_GUTTER_WIDTH: f64 = 57.;
    pub(crate) const MAX_VISIBLE_TRACKS: usize = 32;
    pub(crate) fn clip_body(canvas: Rect) -> Rect {
        Rect {
            left: canvas.left + BODY_OFFSET,
            top: canvas.top + RULER_HEIGHT,
            width: (canvas.width - BODY_OFFSET - MONITOR_GUTTER_WIDTH).max(0.),
            height: (canvas.height - RULER_HEIGHT).max(0.),
        }
    }
}
mod audio_spheres { pub(crate) const END: usize = 48_384; }
mod control_engraving {
    pub(crate) const BASE: usize = 2_648_576;
    pub(crate) const ROWS: usize = 262_144;
}
mod engraving {
    pub(crate) const BASE: usize = 45_312;
    pub(crate) const ROWS: usize = 4096;
}

use serde_json::{Value, json};
use std::{fs, io::Write, path::{Path, PathBuf}};

const FRAME_ROWS: usize = engraving::BASE + engraving::ROWS;
const LABEL_BASE: usize = audio_spheres::END + (32 * 4 + 128) * 3;
const MAX_BYTES: u64 = 128 * 1024 * 1024;

fn read(path: &Path, maximum: u64) -> Result<Vec<u8>, String> {
    let metadata = fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err(format!("{} exceeds bounded input size", path.display()));
    }
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 != metadata.len() { return Err("input changed during read".into()); }
    Ok(bytes)
}
fn fingerprint(bytes: impl IntoIterator<Item = u8>) -> String {
    let hash = bytes.into_iter().fold(0xcbf29ce484222325u64,
        |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3));
    format!("{hash:016x}")
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    file.write_all(bytes).map_err(|e| e.to_string())
}
fn write_rows(path: &Path, rows: &[[f32; 4]]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    for chunk in rows.chunks(4096) {
        let bytes: Vec<_> = chunk.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
        file.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn json_bytes(value: &Value) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    bytes.push(b'\n'); Ok(bytes)
}
fn require(source: &str, marker: &str) -> Result<(), String> {
    if source.matches(marker).count() != 1 { return Err(format!("fixture source marker changed: {marker}")); }
    Ok(())
}
fn replace_once(source: &mut String, old: &str, new: &str) -> Result<(), String> {
    require(source, old)?;
    *source = source.replacen(old, new, 1); Ok(())
}
fn function(source: &str, name: &str) -> Result<(usize, usize), String> {
    let marker = format!("fn {name}("); require(source, &marker)?;
    let start = source.find(&marker).unwrap();
    let opening = start + source[start..].find('{').ok_or("missing function body")?;
    let mut depth = 1usize; let mut end = opening + 1;
    while depth > 0 && end < source.len() {
        match source.as_bytes()[end] { b'{' => depth += 1, b'}' => depth -= 1, _ => () }
        end += 1;
    }
    if depth != 0 { return Err("unclosed function".into()); }
    Ok((start, end))
}
fn verify_sources() -> Result<(), String> {
    let arrangement = include_str!("../../Workstation/Arrangement.rs");
    for marker in [
        "pub(crate) const RULER_HEIGHT: f64 = 32.;",
        "pub(crate) const LANE_HEIGHT: f64 = 64.;",
        "pub(crate) const BODY_OFFSET: f64 = 128.;",
        "pub(crate) const MONITOR_GUTTER_WIDTH: f64 = 57.;",
        "pub(crate) const MAX_VISIBLE_TRACKS: usize = 32;",
        "seconds_per_width: 16.,",
    ] { require(arrangement, marker)?; }
    let (start, end) = function(arrangement, "clip_body")?;
    let clip = &arrangement[start..end];
    for marker in [
        "left: canvas.left + BODY_OFFSET,",
        "top: canvas.top + RULER_HEIGHT,",
        "width: (canvas.width - BODY_OFFSET - MONITOR_GUTTER_WIDTH).max(0.),",
        "height: (canvas.height - RULER_HEIGHT).max(0.),",
    ] { require(clip, marker)?; }
    let benchmark = include_str!("../../BaseSystem/LayerFive/AudioWorkstationSystem.rs");
    let (start, end) = function(benchmark, "prepare_benchmark_audio")?;
    let benchmark = &benchmark[start..end];
    for marker in [
        r#""id":"benchmark-midi","name":"Benchmark MIDI","kind":"midi""#,
        r#""id":"benchmark-midi-clip","track":"benchmark-midi","kind":"midi","start_seconds":0."#,
        r#""bpm":120.,"length_quarters":512."#,
    ] { require(benchmark, marker)?; }
    Ok(())
}
fn read_frame(path: &Path) -> Result<Vec<[f32; 4]>, String> {
    let bytes = read(path, MAX_BYTES)?;
    if bytes.len() % 16 != 0 || bytes.len() < FRAME_ROWS * 16 {
        return Err("fixture needs a complete 49408-row dynamic prefix and immutable packet".into());
    }
    let rows: Vec<[f32; 4]> = bytes.chunks_exact(16).map(|b|
        std::array::from_fn(|i| f32::from_le_bytes(b[4*i..4*i+4].try_into().unwrap()))).collect();
    if !rows.iter().flatten().all(|v| v.is_finite()) { return Err("nonfinite optical packet".into()); }
    if rows[7][0] != FRAME_ROWS as f32 || rows[7][1] != 57.
        || rows[248][1] != audio_spheres::END as f32 || rows[248][2] != LABEL_BASE as f32
        || rows[0][2] != 2. || rows[1] != [0., 0., 3., 0.]
        || rows[2][..3] != [1., 0., 0.] || rows[3][..3] != [0., 1., 0.]
        || rows[4][..3] != [0., 0., -1.] || rows[245][0] != 0. {
        return Err("fixture requires the saved axial native-gem packet/layout ABI".into());
    }
    Ok(rows)
}
fn fixture(frame: &[[f32; 4]]) -> Result<(panel_layout::Viewport, Value), String> {
    let mpp = f64::from(frame[253][0]);
    if !(0.000001..0.1).contains(&mpp) { return Err("invalid captured timeline logical scale".into()); }
    // The saved empty arrangement contains the authentic clip rectangle.
    // Reconstruct its logical canvas to pass through the real append routine.
    let height = 1.3 / mpp;
    let width = 2. * f64::from(frame[2][3]) / mpp;
    if !(64.0..=16384.0).contains(&width) || !(64.0..=16384.0).contains(&height) {
        return Err("captured logical viewport outside diagnostic bounds".into());
    }
    let clip = frame[249].map(f64::from);
    let canvas = piano_roll::Rect {
        left: clip[0] / mpp + width * 0.5,
        top: height * 0.5 - clip[3] / mpp - arrangement::RULER_HEIGHT,
        width: (clip[2] - clip[0]) / mpp + arrangement::MONITOR_GUTTER_WIDTH,
        height: (clip[3] - clip[1]) / mpp + arrangement::RULER_HEIGHT,
    };
    let body = arrangement::clip_body(canvas);
    if body.width <= 0. || body.height < arrangement::LANE_HEIGHT {
        return Err("captured arrangement cannot show one complete benchmark track".into());
    }
    let view = panel_layout::Viewport { width, height, canvas_scale: 1. };
    let value = json!({
        "canvas":{"left":canvas.left,"top":canvas.top,"width":canvas.width,"height":canvas.height},
        "seconds_per_width":16.,"start_seconds":0.,"scroll_tracks":0,
        "selected_track":null,"playhead_seconds":0.,
        "tracks":[{"id":"benchmark-midi","name":"Benchmark MIDI","kind":"midi",
                   "gain":1.,"mute":false,"solo":false}],
        "clips":[{"id":"benchmark-midi-clip","track":"benchmark-midi","kind":"midi",
                  "start_seconds":0.,"duration_seconds":256.}]
    });
    Ok((view, value))
}
fn instrument(mut shader: String) -> Result<String, String> {
    let (start, end) = function(&shader, "rubyFastTrace")?;
    let mut fast = shader[start..end].to_owned();
    replace_once(&mut fast, "    if(responseAddress>=0){",
        "    if(responseAddress>=0){\n        if(hit.panel>=700000&&hit.panel<704096){rubyTimelineUsedSamples+=1u;}")?;
    shader.replace_range(start..end, &fast);
    replace_once(&mut shader, "rubyUsed=true;finished=true;",
        "rubyUsed=true;finished=true;if(hit.panel>=700000&&hit.panel<704096){rubyTimelineUsedSamples+=1u;}")?;
    replace_once(&mut shader, "fn workstationShade(screen:vec2<f32>)->vec4<f32> {",
        "fn workstationShade(screen:vec2<f32>)->vec4<f32> {\n    rubyTimelineUsedSamples=0u;")?;
    replace_once(&mut shader, "return vec4<f32>(gemDisplay(result/samples),1.0);",
        "return vec4<f32>(gemDisplay(result/samples),f32(rubyTimelineUsedSamples));")?;
    shader.insert_str(0, "var<private> rubyTimelineUsedSamples:u32=0u;\n");
    let module = naga::front::wgsl::parse_str(&shader)
        .map_err(|e| format!("instrumented shader: {}", e.emit_to_string(&shader)))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module).map_err(|e| format!("instrumented shader: {e:?}"))?;
    Ok(shader)
}
fn main_impl() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 5 { return Err("Usage: export_ruby_timeline SHADER ORIGINAL_FRAME ASSET_ROOT OUTPUT_PREFIX".into()); }
    verify_sources()?;
    let shader_path = Path::new(&args[1]).canonicalize().map_err(|e| e.to_string())?;
    let frame_path = Path::new(&args[2]).canonicalize().map_err(|e| e.to_string())?;
    let asset_root = Path::new(&args[3]).canonicalize().map_err(|e| e.to_string())?;
    let prefix = PathBuf::from(&args[4]);
    let outputs = ["wgsl","frame.bin","plan.bin","json","fixture.json"].map(|ext| prefix.with_extension(ext));
    for path in &outputs { if path.exists() { return Err(format!("Refusing to replace {}", path.display())); } }
    let source_bytes = read(&shader_path, 2 * 1024 * 1024)?;
    let source = std::str::from_utf8(&source_bytes).map_err(|e| e.to_string())?;
    let mut frame = read_frame(&frame_path)?;
    let original_fingerprint = fingerprint(frame.iter().flatten().flat_map(|v| v.to_le_bytes()));
    let (view, scene) = fixture(&frame)?;
    let kit = timeline_scene::Kit::load(&asset_root)?;
    let geometry = kit.read_geometry(&asset_root)?;
    let geometry_end = timeline_scene::BASE.checked_add(geometry.len()).ok_or("geometry size overflow")?;
    if geometry_end > frame.len() { return Err("captured full packet lacks timeline geometry capacity".into()); }
    frame[timeline_scene::BASE..geometry_end].copy_from_slice(&geometry);
    kit.append(&mut frame[..FRAME_ROWS], view, &scene)?;
    if frame[248] != [5., audio_spheres::END as f32, LABEL_BASE as f32, 4.] {
        return Err(format!("authentic timeline fixture did not produce five meshes/four handles: {:?}", frame[248]));
    }
    for instance in 0..5 {
        let tree = frame[audio_spheres::END + instance * 3 + 1];
        if tree[3] != if instance == 4 { 1. } else { 0. } {
            return Err("timeline fixture changed its handle/tape ordering".into());
        }
    }
    let pixel = std::env::var("WORMWOOD_WORKSTATION_PIXEL_TRANSPORT").as_deref() == Ok("1");
    let mut plan = ruby_response_plan::Plan::new(&frame);
    let update = plan.update(&frame[..FRAME_ROWS]);
    if !update.dirty || update.ready_count == 0 { return Err("fixture has no fresh production cache entries".into()); }
    let mut descriptors = Vec::new(); let mut timeline_targets = Vec::new();
    for slot in 0..update.entry_count as usize {
        let d = &update.rows[1+slot*8..1+(slot+1)*8];
        if d[0][3] < 0.5 { continue; }
        let target = d[0][0] as usize;
        if (700_000..704_096).contains(&target) { timeline_targets.push(target); }
        descriptors.push(json!({"slot":slot,"target":target,"state":d[0][3],"edge":d[0][1],
            "data_base_row":d[0][2],"allocated_rows":d[6][0],"pose":d[1],"bounds":d[2],
            "pixel_mode":d[4],"pixel_origin":d[5]}));
    }
    timeline_targets.sort_unstable();
    let expected: Vec<usize> = if pixel { vec![700000,700016,700032,700048] } else { Vec::new() };
    if timeline_targets != expected {
        return Err(format!("production timeline admission differs: pixel={pixel}, targets={timeline_targets:?}"));
    }
    for instance in 0..4 {
        let mapped = 16644 + instance * 16;
        let slot = update.rows[161+mapped/4][mapped%4];
        if pixel {
            if slot < 0. || update.rows[1+slot as usize*8][0] != (700000+instance*16) as f32 {
                return Err("timeline reserved map does not match descriptor".into());
            }
        } else if slot != -1. { return Err("baseline unexpectedly maps a timeline handle".into()); }
    }
    let shader = instrument(ruby_response_shader::source(source)?)?;
    let report = json!({
        "schema":1,"pixel_transport":pixel,
        "constructor":"production TimelineScene::Kit::load/read_geometry/append",
        "plan":"production Workstation/RubyResponsePlan.rs",
        "shader":"production Workstation/RubyResponseShader.rs",
        "source_shader":shader_path,"source_frame":frame_path,"asset_root":asset_root,
        "source_fingerprint":fingerprint(source_bytes),"original_frame_fingerprint":original_fingerprint,
        "frame_fingerprint":fingerprint(frame.iter().flatten().flat_map(|v| v.to_le_bytes())),
        "timeline_geometry_fingerprint":fingerprint(geometry.iter().flatten().flat_map(|v| v.to_le_bytes())),
        "frame_rows":frame.len(),"dynamic_rows":FRAME_ROWS,"plan_rows":ruby_response_plan::PLAN_ROWS,
        "cache_bytes":ruby_response_plan::CACHE_BYTES,"entry_count":update.entry_count,
        "ready_count":update.ready_count,"dirty":update.dirty,
        "timeline_meshes":frame[248][0],"timeline_handles":frame[248][3],
        "timeline_admitted_targets":timeline_targets,"descriptors":descriptors,
        "width":frame[8][0],"height":frame[8][1],
        "logical_viewport":[view.width,view.height],
        "alpha":"actual primary samples reusing timeline-handle transport, integer0..2; RGB unchanged",
        "canvas_reconstructed_from_saved_float32_clip":true,
        "limitations":["Static start-of-playback optical fixture; audio/device timing is not measured.",
            "The existing captured studio/furniture/floor field is retained.",
            "No document, installed application or original fixture is changed."],
        "outputs":{"shader":outputs[0],"frame":outputs[1],"plan":outputs[2],"fixture":outputs[4]},
        "gpu_verified":false
    });
    fs::create_dir_all(prefix.parent().unwrap_or(Path::new("."))).map_err(|e| e.to_string())?;
    write_new(&outputs[0], shader.as_bytes())?;
    write_rows(&outputs[1], &frame)?;
    write_rows(&outputs[2], &update.rows)?;
    write_new(&outputs[3], &json_bytes(&report)?)?;
    write_new(&outputs[4], &json_bytes(&scene)?)?;
    println!("Exported pixel={pixel}: {} production entries, {} timeline handles: {}",
        update.ready_count, expected.len(), outputs[3].display());
    Ok(())
}
fn main() {
    if let Err(error) = main_impl() { eprintln!("{error}"); std::process::exit(1); }
}
