//! Native optical data shared by the explicit workstation pipelines.
//! Optical data and floor-light visibility are dynamic; Blender's LUT and baked
//! SSLE density mips are immutable and uploaded once per native session.
use crate::workstation::edison_demo;
use crate::workstation::bubble_notes;
use crate::workstation::control_engraving;
use crate::workstation::engraving::{DENSITY_BASE, DENSITY_ROWS};
use crate::workstation::input_monitor;
use crate::workstation::timeline_scene;
use std::path::Path;

pub(super) const STATIC_FRAME_ROWS: usize = 256
    + crate::workstation::floor_light::ROWS
    + crate::workstation::decorative_controls::ROWS
    + crate::workstation::engraving::ROWS;
pub(super) const FRAME_ROWS: usize = crate::workstation::timeline_controls::END;

pub(super) fn upload_ranges(rows: &[[f32; 4]]) -> Vec<(usize, &[[f32; 4]])> {
    if rows.len() == FRAME_ROWS {
        vec![(0, &rows[..STATIC_FRAME_ROWS]), (timeline_scene::WAVE_GPU_BASE, &rows[STATIC_FRAME_ROWS..crate::workstation::piano_scene::BASE]), (crate::workstation::piano_scene::GPU_BASE,&rows[crate::workstation::piano_scene::BASE..])]
    } else { vec![(0, rows)] }
}

pub(super) const TEXTURE_WIDTH: usize = 256;
pub(super) const LUT_EDGE: usize = 57;
pub(super) const STORAGE_BYTES: usize =
    (timeline_scene::WAVE_GPU_BASE + timeline_scene::WAVE_ROWS).div_ceil(TEXTURE_WIDTH) * TEXTURE_WIDTH * 16;

pub(super) fn initial(root: &Path) -> Result<Vec<[f32; 4]>, String> {
    let path = root.join("Art/AudioWorkstation/GemPanels/GemPanels_AgX.bin");
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let count = LUT_EDGE.pow(3);
    if bytes.len() != count * 16 {
        return Err("Blender AgX display LUT must contain 57³ RGBA32F entries".into());
    }
    if DENSITY_BASE != STATIC_FRAME_ROWS + count {
        return Err("SSLE density and Blender LUT offsets disagree".into());
    }
    let mut rows = vec![[0.; 4]; STORAGE_BYTES / 16];
    for (row, bytes) in rows[STATIC_FRAME_ROWS..STATIC_FRAME_ROWS + count]
        .iter_mut()
        .zip(bytes.chunks_exact(16))
    {
        for (value, word) in row.iter_mut().zip(bytes.chunks_exact(4)) {
            *value = f32::from_le_bytes(word.try_into().unwrap());
            if !value.is_finite() {
                return Err("nonfinite Blender display LUT entry".into());
            }
        }
    }
    let path = root.join("Art/AudioWorkstation/LaserEngraving/density.bin");
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() != DENSITY_ROWS * 16 {
        return Err("SSLE density must contain 95 packed 128-to-1 mip chains".into());
    }
    for (row, bytes) in rows[DENSITY_BASE..DENSITY_BASE + DENSITY_ROWS]
        .iter_mut()
        .zip(bytes.chunks_exact(16))
    {
        for (value, word) in row.iter_mut().zip(bytes.chunks_exact(4)) {
            *value = f32::from_le_bytes(word.try_into().unwrap());
            if !value.is_finite() || !(0.0..=65535.0).contains(value) || value.fract() != 0. {
                return Err("invalid packed SSLE density entry".into());
            }
        }
    }
    let monitor = input_monitor::Kit::load(root)?.read_geometry(root)?;
    rows[input_monitor::BASE..input_monitor::BASE + monitor.len()].copy_from_slice(&monitor);
    let edison = edison_demo::Kit::load(root)?.read_geometry(root)?;
    rows[edison_demo::BASE..edison_demo::BASE + edison.len()].copy_from_slice(&edison);
    let connected=edison_demo::Kit::load(root)?.read_connected(root)?;
    rows[edison_demo::PLUG_BASE..edison_demo::PLUG_BASE+connected.len()].copy_from_slice(&connected);
    let stock=crate::workstation::stock_eq_gems::Kit::load(root)?;
    rows[crate::workstation::stock_eq_gems::BASE..crate::workstation::stock_eq_gems::BASE+stock.geometry.len()].copy_from_slice(&stock.geometry);
    let bubbles=bubble_notes::Kit::load(root)?.read_geometry(root)?;
    rows[bubble_notes::BASE..bubble_notes::BASE+bubbles.len()].copy_from_slice(&bubbles);
    let density=control_engraving::Kit::load(root)?.read_density();
    rows[control_engraving::BASE..control_engraving::BASE+density.len()].copy_from_slice(&density);
    let timeline=timeline_scene::Kit::load(root)?.read_geometry(root)?;
    rows[timeline_scene::BASE..timeline_scene::BASE+timeline.len()].copy_from_slice(&timeline);
    let crosswalk=crate::workstation::track_io_scene::Kit::load(root)?.read_geometry(root)?;
    rows[crate::workstation::track_io_scene::BASE..crate::workstation::track_io_scene::BASE+crosswalk.len()].copy_from_slice(&crosswalk);
    use crate::workstation::fader_components::{CAP_BASE,CAP_EDGE};
    let pixels=include_bytes!("../Art/AudioWorkstation/GemFadersMeters/ivory_cap_linear.rgba32f");
    const _:()=assert!(include_bytes!("../Art/AudioWorkstation/GemFadersMeters/ivory_cap_linear.rgba32f").len()==CAP_EDGE*CAP_EDGE*16);
    for (row,rgba) in rows[CAP_BASE..CAP_BASE+CAP_EDGE*CAP_EDGE].iter_mut().zip(pixels.chunks_exact(16)){
        for (v,channel) in row.iter_mut().zip(rgba.chunks_exact(4)){*v=f32::from_le_bytes(channel.try_into().unwrap());}
    }
    if let Some(style) = crate::workstation::user_style::selected()? { style.write_geometry(&mut rows)?; }
    Ok(rows)
}

pub(super) fn validate_frame(rows: &[[f32; 4]]) -> Result<(), String> {
    if rows.len() != FRAME_ROWS || !rows.iter().flatten().all(|x| x.is_finite()) {
        return Err(
            "workstation optical frame needs its finite optical and floor-light rows".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stock_gems_are_loaded_in_reserved_optical_storage_without_changing_edison() {
        let root=Path::new(env!("CARGO_MANIFEST_DIR"));let rows=initial(root).unwrap();
        let kit=crate::workstation::stock_eq_gems::Kit::load(root).unwrap();let base=crate::workstation::stock_eq_gems::BASE;
        assert_eq!(&rows[base..base+kit.geometry.len()],kit.geometry.as_slice());
        let edison=edison_demo::Kit::load(root).unwrap();let socket=edison.read_geometry(root).unwrap();let plug=edison.read_connected(root).unwrap();
        assert_eq!(&rows[edison_demo::BASE..edison_demo::BASE+socket.len()],socket.as_slice());
        assert_eq!(&rows[edison_demo::PLUG_BASE..edison_demo::PLUG_BASE+plug.len()],plug.as_slice());
    }
    #[test]
    #[ignore = "requires WORMWOOD_TEST_ASSET_ROOT pointing to a built app's EngineAssets"]
    fn workstation_packaged_assets_load_optical_geometry() {
        let root = std::env::var_os("WORMWOOD_TEST_ASSET_ROOT")
            .expect("packaged EngineAssets path is required");
        let root = Path::new(&root).canonicalize().unwrap();
        assert_ne!(root, Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize().unwrap());
        let rows = initial(&root).unwrap();
        assert_eq!(rows.len() * 16, STORAGE_BYTES);
        assert!(rows.iter().flatten().all(|v| v.is_finite()));
    }
}

// Explicit CPU-only fixture export for optical research. Never runs with the
// application or ordinary test suite; uses the real immutable asset loader.
#[cfg(test)]
mod optical_probe_export {
    #[test]
    #[ignore = "explicit offline tape artwork export"]
    fn export_tape_artwork_scene() {
        let directory=std::path::PathBuf::from(std::env::var("WORMWOOD_OPTICAL_PROBE_DIRECTORY").expect("explicit probe directory"));
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixture:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("BuildSupport/Workstation/BakedFixture.json")).unwrap()).unwrap();
        let frame:Vec<[f32;4]>=serde_json::from_value(fixture["rows"].clone()).unwrap();
        let mut all=super::initial(root).unwrap();
        // Tape asset baking needs the immutable models and authored camera/light
        // header, not an old dynamic timeline packet or its versioned offsets.
        all[..256].copy_from_slice(&frame[..256]);
        let bytes:Vec<u8>=all.iter().flatten().flat_map(|v|v.to_le_bytes()).collect();
        std::fs::write(directory.join("scene.bin"),bytes).unwrap();
    }
    #[test]
    #[ignore = "requires an explicit captured fixture/output directory"]
    fn export_captured_optical_fixture_for_probe() {
        let directory=std::path::PathBuf::from(std::env::var("WORMWOOD_OPTICAL_PROBE_DIRECTORY").expect("explicit probe directory"));
        let value:serde_json::Value=serde_json::from_slice(&std::fs::read(directory.join("captured.json")).unwrap()).unwrap();
        let frame:Vec<[f32;4]>=serde_json::from_value(value["rows"].clone()).unwrap();
        super::validate_frame(&frame).unwrap();
        let resolved=crate::workstation::deferred_floor::complete(&frame).unwrap();
        let mut all=super::initial(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        all[..frame.len()].copy_from_slice(&resolved);
        let bytes:Vec<u8>=all.iter().flatten().flat_map(|v|v.to_le_bytes()).collect();
        std::fs::write(directory.join("scene.bin"),bytes).unwrap();
        std::fs::write(directory.join("scene-info.json"),serde_json::to_vec_pretty(&serde_json::json!({
            "frame_rows":frame.len(),"storage_rows":all.len(),"storage_bytes":all.len()*16,
            "floor_request_resolved":crate::workstation::deferred_floor::requested(&frame),
            "scope":"Production initial asset loader plus captured dynamic packet; deferred floor resolved through the production CPU function; no graphics/audio/application started."
        })).unwrap()).unwrap();
    }
}

#[cfg(test)] mod bend_packet_tests {
 #[test] fn bend_rows_fit_remapped_gpu_storage(){
  let rows=vec![[0.;4];super::FRAME_ROWS];
  for(start,data)in super::upload_ranges(&rows){assert!((start+data.len())*16<=super::STORAGE_BYTES);}
  assert!(crate::workstation::bubble_notes::BEND_GPU_BASE+crate::workstation::bubble_notes::MAX_NOTES<=super::STORAGE_BYTES/16);
 }
}
