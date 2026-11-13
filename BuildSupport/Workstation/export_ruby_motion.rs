//! Finite moving-frame fixture export; parent owns compilation and GPU runs.
//! Usage: export_ruby_motion FRAME OUTPUT_DIRECTORY
//! Compare an original-frame bake reused with moved.reuse.plan.bin against a
//! new moved-frame bake with moved.fresh.plan.bin. The deterministic translation
//! moves panels, controls, labels, culling bounds and camera together, preserving
//! cache admission. Studio light 0 moves and changes intensity independently.
//! Existing floor-shadow samples stay unchanged in both moved comparisons.
#[path = "../../Workstation/RubyResponsePlan.rs"]
mod ruby_response_plan;

use std::{fs, io::Write, path::Path};

const FRAME_ROWS: usize = 49_408;
const MAX_BYTES: usize = 128 * 1024 * 1024;
const DELTA: [f32; 3] = [0.03125, -0.015625, 0.];
const FILES: [&str; 6] = ["original.frame.bin", "original.plan.bin", "moved.frame.bin",
    "moved.reuse.plan.bin", "moved.fresh.plan.bin", "report.json"];

fn index(value: f32, bound: usize) -> Result<usize, String> {
    if !value.is_finite() || value < 0. || value.fract() != 0. || value as usize >= bound {
        return Err("invalid motion-fixture row/count".into());
    }
    Ok(value as usize)
}

fn read_frame(path: &Path) -> Result<Vec<[f32; 4]>, String> {
    let size = fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size < (FRAME_ROWS * 16) as u64 || size > MAX_BYTES as u64 || size % 16 != 0 {
        return Err("motion fixture requires a bounded full RGBA32F optical packet".into());
    }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() as u64 != size { return Err("fixture changed during read".into()); }
    let rows: Vec<[f32; 4]> = bytes.chunks_exact(16).map(|b|
        std::array::from_fn(|i| f32::from_le_bytes(b[4*i..4*i+4].try_into().unwrap()))).collect();
    if !rows.iter().flatten().all(|v| v.is_finite()) { return Err("nonfinite optical fixture".into()); }
    if rows[7][0] != FRAME_ROWS as f32 || rows[7][1] != 57. || rows[245][0] > 0.5 {
        return Err("motion fixture requires current native gem-panel storage".into());
    }
    Ok(rows)
}

fn write_rows(directory: &Path, name: &str, rows: &[[f32; 4]]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(directory.join(name))
        .map_err(|e| e.to_string())?;
    // Bounded chunks avoid an extra complete copy of the 56 MiB scene packet.
    for chunk in rows.chunks(4096) {
        let bytes: Vec<u8> = chunk.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
        file.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn fingerprint(rows: &[[f32; 4]]) -> String {
    let hash = rows.iter().flatten().flat_map(|v| v.to_le_bytes()).fold(0xcbf29ce484222325u64,
        |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3));
    format!("{hash:016x}")
}

fn translate(row: &mut [f32; 4]) {
    for axis in 0..3 { row[axis] += DELTA[axis]; }
}

fn pose_row(frame: &[[f32; 4]], instance: usize) -> Result<usize, String> {
    let at = if instance >= 61 { index(frame[232][0], FRAME_ROWS)? + 32 + 2*(instance-61) }
        else if instance >= 29 { index(frame[247][0], FRAME_ROWS)? + 2*(instance-29) }
        else if instance >= 13 { index(frame[232][0], FRAME_ROWS)? + 2*(instance-13) }
        else if instance >= 10 { 226 + 2*(instance-10) }
        else { 192 + 2*instance };
    if at + 1 >= FRAME_ROWS { return Err("control pose outside dynamic frame".into()); }
    Ok(at)
}

fn move_frame(frame: &mut [[f32; 4]]) -> Result<usize, String> {
    for panel in 0..4 {
        let base = 32 + 40*panel;
        if panel as f32 >= frame[0][1] || frame[base][0] < 0.5 { continue; }
        translate(&mut frame[base+2]);
        for plane in &mut frame[base+6..base+32] {
            plane[3] = (f64::from(plane[3]) + (0..3)
                .map(|axis| f64::from(plane[axis])*f64::from(DELTA[axis])).sum::<f64>()) as f32;
        }
    }
    let mut count = 0;
    for instance in 0..65 {
        let active = if instance >= 61 { ((instance-61) as f32) < frame[232][2] }
            else if instance >= 29 { ((instance-29) as f32) < frame[247][1] }
            else { (instance as f32) < frame[12][0] };
        if !active { continue; }
        let at = pose_row(frame, instance)?;
        if frame[at][3] <= 0. { continue; }
        translate(&mut frame[at]); count += 1;
    }
    // Camera translation keeps the exact same admitted/sorted component set;
    // the live studio/floor still see the changed component positions.
    translate(&mut frame[1]);
    for at in [13, 15, 238, 218, 220] { translate(&mut frame[at]); }
    frame[238][3] += DELTA[1]; frame[239][3] += DELTA[1];
    if frame[12][2] > 0. && frame[12][3] > frame[12][2] {
        let first = index(frame[12][2], FRAME_ROWS)?;
        let end = index(frame[12][3], FRAME_ROWS+1)?;
        if (end-first)%2 != 0 { return Err("invalid two-row instance BVH".into()); }
        for row in &mut frame[first..end] { translate(row); }
    }
    if frame[232][0] > 0. {
        let clip = index(frame[232][0], FRAME_ROWS)? + 40;
        if clip >= FRAME_ROWS { return Err("plugin clip outside dynamic frame".into()); }
        frame[clip][0] += DELTA[0]; frame[clip][2] += DELTA[0];
        frame[clip][1] += DELTA[1]; frame[clip][3] += DELTA[1];
    }
    if frame[212][3] > 0.5 {
        let glyph_base = index(frame[212][1], FRAME_ROWS)?;
        let glyphs = index(frame[212][2], 1025)?;
        if glyph_base + 2*glyphs > FRAME_ROWS { return Err("glyph positions outside dynamic frame".into()); }
        for glyph in 0..glyphs { translate(&mut frame[glyph_base+2*glyph]); }
        let line_base = index(frame[222][0], FRAME_ROWS)?;
        let lines = index(frame[222][1], 17)?;
        if line_base + 3*lines > FRAME_ROWS { return Err("line bounds outside dynamic frame".into()); }
        for line in 0..lines { translate(&mut frame[line_base+3*line]); }
    }
    if frame[9][0] < 1. { return Err("motion lighting check requires studio light 0".into()); }
    frame[16][0] += 0.25; frame[16][1] -= 0.125;
    for channel in 0..3 { frame[20][channel] *= 1.5; }
    Ok(count)
}

fn dirty_targets(update: &ruby_response_plan::Update) -> Vec<usize> {
    (0..update.entry_count as usize).filter_map(|slot| {
        let row = update.rows[1+8*slot];
        (row[3] == 2.).then_some(row[0] as usize)
    }).collect()
}

fn main_impl() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 { return Err("Usage: export_ruby_motion FRAME OUTPUT_DIRECTORY".into()); }
    let input = Path::new(&args[1]); let output = Path::new(&args[2]);
    if FILES.iter().any(|name| output.join(name).exists()) {
        return Err("refusing to replace motion-check artifacts".into());
    }
    let original = read_frame(input)?;
    let mut reused_plan = ruby_response_plan::Plan::new(&original);
    let initial = reused_plan.update(&original[..FRAME_ROWS]);
    if !initial.dirty || initial.ready_count == 0 { return Err("fixture has no initial ruby bake".into()); }
    reused_plan.submitted();
    let mut moved = original.clone();
    let translated_controls = move_frame(&mut moved)?;
    let reused = reused_plan.update(&moved[..FRAME_ROWS]);
    let mut fresh_plan = ruby_response_plan::Plan::new(&moved);
    let fresh = fresh_plan.update(&moved[..FRAME_ROWS]);
    if reused.dirty || !fresh.dirty || reused.ready_count != initial.ready_count {
        return Err(format!("translation unexpectedly changed cache admission/invalidation: initial={}, reused={}, dirty={:?}",
            initial.ready_count, reused.ready_count, dirty_targets(&reused)));
    }
    let mut fresh_ready = fresh.rows.clone();
    for slot in 0..fresh.entry_count as usize {
        let descriptor = &mut fresh_ready[1+slot*8];
        if descriptor[3] == 2. { descriptor[3] = 1.; }
    }
    if reused.rows != fresh_ready { return Err("reused/fresh moved descriptors differ beyond dirty state".into()); }
    // Key invalidation evidence only; no GPU claim follows from these checks.
    reused_plan.submitted();
    let mut changed = moved[..FRAME_ROWS].to_vec();
    changed[38][3] += 0.001;
    let shape_targets = dirty_targets(&reused_plan.update(&changed));
    fresh_plan.submitted();
    let mut changed_material = moved[..FRAME_ROWS].to_vec();
    changed_material[32][2] += 0.0001;
    let material_targets = dirty_targets(&fresh_plan.update(&changed_material));
    if !shape_targets.contains(&0) || !material_targets.contains(&0) {
        return Err("moved fixture failed shape/material invalidation check".into());
    }
    fs::create_dir_all(output).map_err(|e| e.to_string())?;
    write_rows(output, FILES[0], &original)?; write_rows(output, FILES[1], &initial.rows)?;
    write_rows(output, FILES[2], &moved)?; write_rows(output, FILES[3], &reused.rows)?;
    write_rows(output, FILES[4], &fresh.rows)?;
    let report = format!(concat!("{{\n  \"schema_version\":1,\n  \"frame_rows\":{},\n  \"dynamic_rows\":{},",
        "\n  \"translation_metres\":{:?},\n  \"translated_controls\":{},\n  \"camera_translated_with_scene\":true,",
        "\n  \"light0_translation_metres\":[0.25,-0.125,0.0],\n  \"light0_intensity_multiplier\":1.5,",
        "\n  \"floor_shadow_field\":\"unchanged fixture data in both moved comparisons\",",
        "\n  \"original_fingerprint\":\"{}\",\n  \"moved_fingerprint\":\"{}\",",
        "\n  \"initial_entries\":{},\n  \"reused_entries\":{},\n  \"reused_dirty\":{},\n  \"fresh_dirty\":{},",
        "\n  \"descriptor_equal_except_state\":true,\n  \"shape_dirty_targets\":{:?},\n  \"material_dirty_targets\":{:?},",
        "\n  \"gpu_pixels_verified\":false\n}}\n"),
        original.len(), FRAME_ROWS, DELTA, translated_controls, fingerprint(&original), fingerprint(&moved),
        initial.ready_count, reused.ready_count, reused.dirty, fresh.dirty, shape_targets, material_targets);
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(output.join(FILES[5]))
        .map_err(|e| e.to_string())?;
    file.write_all(report.as_bytes()).map_err(|e| e.to_string())?;
    println!("Exported {} reusable moved ruby entries: {}", reused.ready_count, output.display());
    Ok(())
}

fn main() {
    if let Err(error) = main_impl() { eprintln!("{error}"); std::process::exit(1); }
}
