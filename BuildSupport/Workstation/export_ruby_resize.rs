//! Bounded native-gem resize fixtures for parent-owned GPU experiments.
//! Usage: export_ruby_resize FRAME OUTPUT_DIRECTORY [ASSET_ROOT]
//! Link the checkout's existing serde_json rlib when compiling standalone.
//!
//! This includes the actual GemMesh loader, PanelLayout constructors, floor
//! visibility builder and production response Plan. It does not stretch old
//! vertices or invent new extents while retaining old planes. Custom style
//! selection is unavailable in this native-gem-only harness; its small shim is
//! only needed to compile unrelated PanelLayout gesture helpers.
//!
//! The original packet stays byte-identical. Small, large and uniform cases
//! recut all active native panels at their original XY centres, update physical
//! absorption density and floor visibility, and keep other fixture furniture
//! and labels fixed. These isolate resize transport; they are not a complete
//! application layout animation. Report that limitation with GPU results.
#![allow(dead_code)]

#[path = "../../Workstation/GemMesh.rs"]
mod gem_mesh;
#[path = "../../Workstation/PanelMotion.rs"]
mod panel_motion;
#[path = "../../Workstation/PanelLayout.rs"]
mod panel_layout;
#[path = "../../Workstation/FloorLight.rs"]
mod floor_light;
#[path = "../../Workstation/RubyResponsePlan.rs"]
mod ruby_response_plan;

mod user_style {
    use super::{panel_layout::{Options, Rect, Viewport}, panel_motion::Snapshot};
    use std::sync::Arc;
    pub(crate) struct Kit;
    #[derive(Clone, Copy)]
    pub(crate) struct Surface {
        pub origin: [f64; 3], pub size: [f64; 2], pub scale: [f64; 3], pub visual_rect: Rect,
    }
    pub(crate) fn selected() -> Result<Option<Arc<Kit>>, String> { Ok(None) }
    impl Kit {
        pub(crate) fn surface(&self, _: usize, _: Viewport, _: &Snapshot, _: Options)
            -> Result<Option<Surface>, String> {
            Err("custom styles are unavailable in native resize fixtures".into())
        }
        pub(crate) fn hit_panel_point(&self, _: usize, _: Viewport, _: &Snapshot, _: Options, _: [f64; 2])
            -> Result<bool, String> {
            Err("custom style hit tests are unavailable in native resize fixtures".into())
        }
    }
}

use serde_json::{Value, json};
use std::{fs, io::Write, path::{Path, PathBuf}};

const FRAME_ROWS: usize = 49_408;
const MAX_BYTES: usize = 128 * 1024 * 1024;
const BOX_PADDING: f64 = 1e-6;
const PANEL_NAMES: [&str; 4] = ["GemPanel_Strip", "GemPanel_Broad", "GemPanel_Tall", "GemPanel_Compact"];
const CASES: [(&str, [f64; 2]); 3] = [
    ("small", [1.02, 0.99]), ("large", [1.35, 0.78]), ("uniform", [1.10, 1.10]),
];

fn fingerprint(bytes: impl IntoIterator<Item=u8>) -> String {
    let hash = bytes.into_iter().fold(0xcbf29ce484222325u64,
        |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3));
    format!("{hash:016x}")
}

fn row_fingerprint(rows: &[[f32; 4]]) -> String {
    fingerprint(rows.iter().flatten().flat_map(|v| v.to_le_bytes()))
}

fn read_frame(path: &Path) -> Result<Vec<[f32; 4]>, String> {
    let size = fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size < (FRAME_ROWS * 16) as u64 || size > MAX_BYTES as u64 || size % 16 != 0 {
        return Err("resize fixture requires a bounded full RGBA32F optical packet".into());
    }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() as u64 != size { return Err("fixture changed during read".into()); }
    let rows: Vec<[f32; 4]> = bytes.chunks_exact(16).map(|b|
        std::array::from_fn(|i| f32::from_le_bytes(b[4*i..4*i+4].try_into().unwrap()))).collect();
    if !rows.iter().flatten().all(|v| v.is_finite()) { return Err("nonfinite resize fixture".into()); }
    if rows[7][0] != FRAME_ROWS as f32 || rows[7][1] != 57.
        || rows[245][0] > 0.5 || rows[1][3] != 0. || rows[4][..3] != [0.,0.,-1.] {
        return Err("resize fixture requires current native orthographic gem panels".into());
    }
    Ok(rows)
}

fn write_rows(path: &Path, rows: &[[f32; 4]]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    for chunk in rows.chunks(4096) {
        let bytes: Vec<u8> = chunk.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
        file.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn active_panels(rows: &[[f32; 4]]) -> Vec<usize> {
    (0..4).filter(|&panel| (panel as f32) < rows[0][1] && rows[32+40*panel][0] >= 0.5).collect()
}

fn dimensions(rows: &[[f32; 4]], panel: usize) -> Result<[f64; 2], String> {
    let extent = rows[35+40*panel];
    let dimensions = [2.*(f64::from(extent[0])-BOX_PADDING), 2.*(f64::from(extent[1])-BOX_PADDING)];
    if dimensions.iter().any(|v| !v.is_finite() || *v < 1e-5 || *v > 100.) {
        return Err("native panel dimensions outside resize fixture bounds".into());
    }
    Ok(dimensions)
}

fn validate_source(rows: &[[f32; 4]], panel: usize) -> Result<Value, String> {
    let base = 32+40*panel;
    let size = dimensions(rows, panel)?;
    let shape = panel_layout::optical_shape(size[0], size[1])?;
    let center = rows[base+2];
    let mut maximum_normal_error: f64 = 0.;
    let mut maximum_distance_error: f64 = 0.;
    for (index, plane) in shape.planes.iter().enumerate() {
        let actual = rows[base+6+index];
        for axis in 0..3 {
            maximum_normal_error = maximum_normal_error.max((plane[axis]-f64::from(actual[axis])).abs());
        }
        let world_distance = plane[3] + plane[0]*f64::from(center[0]) + plane[1]*f64::from(center[1]);
        maximum_distance_error = maximum_distance_error.max((world_distance-f64::from(actual[3])).abs());
    }
    let expected_center_z = -shape.dimensions_m[2]*0.5;
    let maximum_pose_error = (expected_center_z-f64::from(center[2])).abs();
    // Reconstructing dimensions from packed padded f32 bounds introduces small
    // roundoff. Fail before generating fixtures if this is not the native cut.
    if maximum_normal_error > 2e-5 || maximum_distance_error > 2e-5 || maximum_pose_error > 2e-5 {
        return Err(format!("panel {panel} does not match native optical_shape: normal={maximum_normal_error}, distance={maximum_distance_error}, pose={maximum_pose_error}"));
    }
    Ok(json!({"panel":panel,"dimensions_m":shape.dimensions_m,
        "reconstruction_normal_error":maximum_normal_error,
        "reconstruction_distance_error_m":maximum_distance_error,
        "reconstruction_center_z_error_m":maximum_pose_error}))
}

fn resize(original: &[[f32; 4]], meshes: &[gem_mesh::PanelMesh], factors: [f64; 2])
    -> Result<(Vec<[f32; 4]>, Vec<Value>), String> {
    let mut rows = original.to_vec();
    let mut details = Vec::new();
    let mut floor: f64 = 0.;
    for panel in active_panels(original) {
        let base = 32+40*panel;
        let old = dimensions(original, panel)?;
        let size = [old[0]*factors[0], old[1]*factors[1]];
        let old_unit = old[0].min(old[1])*0.5;
        let shape = panel_layout::optical_shape(size[0], size[1])?;
        // Exercise the actual mesh constructor too, using the validated GLB
        // solely as its authored material/reference; GPU geometry is planes.
        let authored = meshes.iter().find(|mesh| mesh.name == PANEL_NAMES[panel])
            .ok_or_else(|| format!("missing authored reference {}", PANEL_NAMES[panel]))?;
        let mesh = panel_layout::reshape(authored, size[0], size[1])?;
        if mesh.dimensions_m != shape.dimensions_m || mesh.vertices.is_empty() {
            return Err("native mesh/optical constructors disagree".into());
        }
        let vertex_outside = mesh.vertices.iter().any(|vertex| shape.planes.iter().any(|plane|
            (0..3).map(|axis| plane[axis]*vertex.position[axis]).sum::<f64>() > plane[3]+shape.local_unit_m*1e-4));
        if vertex_outside { return Err("reshaped native mesh left its optical halfspaces".into()); }
        floor = floor.min(-shape.dimensions_m[2]-0.02);
        for channel in 0..3 {
            rows[base+1][channel] = (f64::from(original[base+1][channel])*old_unit/shape.local_unit_m) as f32;
        }
        rows[base+2][2] = (-shape.dimensions_m[2]*0.5) as f32;
        for axis in 0..3 { rows[base+3][axis] = (shape.dimensions_m[axis]*0.5+BOX_PADDING) as f32; }
        let center = rows[base+2];
        let mut changed_normals = Vec::new();
        let mut changed_offsets = Vec::new();
        let mut maximum_normal_delta: f64 = 0.;
        let mut maximum_offset_delta: f64 = 0.;
        for (index, plane) in shape.planes.iter().enumerate() {
            let distance = plane[3]+plane[0]*f64::from(center[0])+plane[1]*f64::from(center[1]);
            let packed = [plane[0] as f32, plane[1] as f32, plane[2] as f32, distance as f32];
            let previous = original[base+6+index];
            let normal_delta = (0..3).map(|axis| (f64::from(packed[axis])-f64::from(previous[axis])).abs())
                .fold(0., f64::max);
            let offset_delta = (f64::from(packed[3])-f64::from(previous[3])).abs();
            if normal_delta > 1e-6 { changed_normals.push(index); }
            if offset_delta > 1e-7 { changed_offsets.push(index); }
            maximum_normal_delta = maximum_normal_delta.max(normal_delta);
            maximum_offset_delta = maximum_offset_delta.max(offset_delta);
            rows[base+6+index] = packed;
        }
        details.push(json!({"panel":panel,"asset":PANEL_NAMES[panel],"old_dimensions_m":old,
            "dimensions_m":shape.dimensions_m,"local_unit_m":shape.local_unit_m,
            "sigma_multiplier":old_unit/shape.local_unit_m,"center":rows[base+2],
            "mesh_vertices":mesh.vertices.len(),"changed_normal_planes":changed_normals,
            "changed_world_offset_planes":changed_offsets,"maximum_normal_delta":maximum_normal_delta,
            "maximum_world_offset_delta_m":maximum_offset_delta}));
    }
    rows[5][3] = floor as f32;
    let domain = floor_light::domain(&rows[..256]);
    rows[10] = [256.,floor_light::EDGE as f32,domain[0],domain[1]];
    rows[11] = [domain[2],domain[3],0.,0.];
    let field = floor_light::build(&rows[..256]);
    if field.len() != floor_light::ROWS { return Err("unexpected native floor field size".into()); }
    rows[256..256+floor_light::ROWS].copy_from_slice(&field);
    if !rows.iter().flatten().all(|value| value.is_finite()) { return Err("resize generated nonfinite packet".into()); }
    Ok((rows, details))
}

fn plan_summary(update: &ruby_response_plan::Update) -> Value {
    let mut dirty = Vec::new();
    let mut descriptors = Vec::new();
    for slot in 0..update.entry_count as usize {
        let at = 1+slot*8; let row = update.rows[at];
        if row[3] < 0.5 { continue; }
        if row[3] == 2. { dirty.push(row[0] as usize); }
        descriptors.push(json!({"slot":slot,"target":row[0],"state":row[3],
            "edge":row[1],"data_base":row[2],"pose":update.rows[at+1],"bounds":update.rows[at+2]}));
    }
    json!({"entry_count":update.entry_count,"ready_count":update.ready_count,
        "dirty":update.dirty,"dirty_targets":dirty,"descriptors":descriptors})
}

fn main_impl() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().collect();
    if !(3..=4).contains(&args.len()) { return Err("Usage: export_ruby_resize FRAME OUTPUT_DIRECTORY [ASSET_ROOT]".into()); }
    let input = Path::new(&args[1]); let output = Path::new(&args[2]);
    let root = args.get(3).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let mut names = vec!["original.frame.bin".to_owned(),"original.plan.bin".to_owned(),"report.json".to_owned()];
    for (name, _) in CASES {
        for suffix in ["frame.bin","plan.bin","transition.plan.bin"] { names.push(format!("{name}.{suffix}")); }
    }
    if names.iter().any(|name| output.join(name).exists()) { return Err("refusing to replace resize artifacts".into()); }
    let original = read_frame(input)?;
    let panels = active_panels(&original);
    if panels.is_empty() { return Err("fixture has no active native panels".into()); }
    let source_checks: Vec<_> = panels.iter().map(|&panel| validate_source(&original,panel)).collect::<Result<_,_>>()?;
    let meshes = gem_mesh::load(&root)?;
    let mut original_plan = ruby_response_plan::Plan::new(&original);
    let original_update = original_plan.update(&original[..FRAME_ROWS]);
    if !original_update.dirty || original_update.ready_count == 0 { return Err("fixture has no initial ruby bake".into()); }
    fs::create_dir_all(output).map_err(|e| e.to_string())?;
    write_rows(&output.join("original.frame.bin"),&original)?;
    write_rows(&output.join("original.plan.bin"),&original_update.rows)?;
    let mut cases = Vec::new();
    for (name, factors) in CASES {
        let (frame, details) = resize(&original,&meshes,factors)?;
        let mut transition = ruby_response_plan::Plan::new(&original);
        transition.update(&original[..FRAME_ROWS]); transition.submitted();
        let transition_update = transition.update(&frame[..FRAME_ROWS]);
        let mut fresh = ruby_response_plan::Plan::new(&frame);
        let fresh_update = fresh.update(&frame[..FRAME_ROWS]);
        let transition_summary = plan_summary(&transition_update);
        for panel in &panels {
            if !transition_summary["dirty_targets"].as_array().unwrap().iter().any(|value| value.as_u64() == Some(*panel as u64)) {
                return Err(format!("production plan failed to invalidate resized panel {panel} in {name}"));
            }
        }
        write_rows(&output.join(format!("{name}.frame.bin")),&frame)?;
        write_rows(&output.join(format!("{name}.plan.bin")),&fresh_update.rows)?;
        write_rows(&output.join(format!("{name}.transition.plan.bin")),&transition_update.rows)?;
        cases.push(json!({"name":name,"width_height_factors":factors,"frame_fingerprint":row_fingerprint(&frame),
            "panels":details,"fresh_plan":plan_summary(&fresh_update),"transition_plan":transition_summary,
            "floor_height_m":frame[5][3],"floor_visibility":"rebuilt with actual FloorLight::build"}));
    }
    let report = json!({"schema_version":1,"kind":"native-parametric-gem-resize-fixtures",
        "source_frame":input,"asset_root":root,"source_frame_rows":original.len(),"dynamic_rows":FRAME_ROWS,
        "source_frame_fingerprint":row_fingerprint(&original),"source_geometry_checks":source_checks,
        "constructor":"Workstation/PanelLayout.rs::optical_shape and reshape; actual GemMesh GLB loader",
        "constructor_source_fingerprint":fingerprint(include_bytes!("../../Workstation/PanelLayout.rs").iter().copied()),
        "original_plan":plan_summary(&original_update),"cases":cases,
        "limitations":["Fixed XY panel centres; this is a shape-transport experiment, not full Origami layout replay.",
            "Other furniture and labels remain at their original fixture positions and may no longer fit resized gems.",
            "Production plans correctly request fresh bakes; no stale cache acceptance is fabricated.",
            "GPU image equivalence, branch stability and speed remain unverified."],
        "gpu_verified":false});
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(output.join("report.json"))
        .map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(&report).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    file.write_all(b"\n").map_err(|e|e.to_string())?;
    println!("Exported native small, large and uniform resize fixtures to {}",output.display());
    Ok(())
}

fn main() {
    if let Err(error) = main_impl() { eprintln!("{error}"); std::process::exit(1); }
}
