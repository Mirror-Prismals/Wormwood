//! Shared native scene storage layout, separate from habitat selection.
pub(crate) const WOODLAND: &[u8] = include_bytes!("../Procedures/Assets/Woodland/materials.rgba");
pub(crate) const CLOUD_NOISE: &[u8] = include_bytes!("../Procedures/Assets/Clouds/noise.rgba");
// Preserve the entire 2048² player image. Native storage paths append the small
// woodland atlas after its reserved region; the existing player ABI is intact.
pub(crate) const PLAYER_BYTES: usize = 16_777_224;
pub(crate) const CLOUD_NOISE_OFFSET: usize = PLAYER_BYTES + WOODLAND.len();
pub(crate) const CLOUD_FRAME_OFFSET: usize = (CLOUD_NOISE_OFFSET + CLOUD_NOISE.len() + 15) & !15;
pub(crate) const FINISHES: &[u8; 5 * 128 * 128 * 4] = include_bytes!("../Procedures/Assets/Theatre/finishes.rgba");
pub(crate) const SCREEN_OFFSET: usize = CLOUD_FRAME_OFFSET + 12 * 16;
pub(crate) const SCREEN_WIDTH: usize = 512;
pub(crate) const SCREEN_HEIGHT: usize = 288;
pub(crate) const SCREEN_BYTES: usize = SCREEN_WIDTH * SCREEN_HEIGHT * 4;
pub(crate) const WALNUT: &[u8; 4_194_320] = include_bytes!("../Procedures/Assets/Theatre/table_material.rgba");
pub(crate) const ROCKS: &[u8; 2_097_152] = include_bytes!("../Procedures/Assets/Terrain/rock_materials.rgba");
pub(crate) const GRASS: &[u8; 699_056] = include_bytes!("../Procedures/Assets/Terrain/grass_material.rgba");
pub(crate) const DIRT: &[u8; 2_796_208] = include_bytes!("../Procedures/Assets/Terrain/dirt_material.rgba");
// Keep one material tail so existing GL and storage upload paths share its ABI.
pub(crate) const TABLE: &[u8; 18_175_376] = include_bytes!(concat!(env!("OUT_DIR"), "/scene_materials.rgba"));
pub(crate) const TABLE_OFFSET: usize = SCREEN_OFFSET + SCREEN_BYTES;
pub(crate) const SCENE_BYTES: usize = TABLE_OFFSET + TABLE.len() + FINISHES.len();
pub(crate) fn table_eye(view: &serde_json::Value) -> Result<[f32;4],String> {
    if view["table_eye"].is_null() {return Ok([0.,8.,-52.,1.]);}
    let eye=crate::audio::vector(&view["table_eye"])?;
    if eye.iter().any(|v|v.abs()>10000.) {return Err("table camera exceeds local material bounds".into());}
    Ok([eye[0] as f32,eye[1] as f32,eye[2] as f32,1.])
}

// First two vec4s of the appended limestone block are the per-view outdoor light.
pub(crate) const EXTERIOR_OFFSET: usize = 9_786_736;
pub(crate) const CARPET_OFFSET: usize = 13_981_072;
pub(crate) fn exterior_light(view:&serde_json::Value)->[f32;8] {
    let light=crate::base_system::layer_four::atmosphere::Lighting::from_view(view);
    [light.primary[0] as f32,light.primary[1] as f32,light.primary[2] as f32,
     light.brightness() as f32,light.tint[0] as f32,light.tint[1] as f32,light.tint[2] as f32,0.]
}

/// Convert the renderer's top-down P6 capture to a letterboxed monitor image.
pub(crate) fn monitor_image(ppm: &[u8]) -> Result<Vec<u8>, String> {
    let mut parts = ppm.splitn(4, |&b| b == b'\n');
    if parts.next() != Some(&b"P6"[..]) { return Err("invalid monitor capture".into()); }
    let dims = std::str::from_utf8(parts.next().unwrap_or_default()).map_err(|e| e.to_string())?;
    let (w,h) = dims.split_once(' ').ok_or("missing capture dimensions")?;
    let w: usize = w.parse().map_err(|_| "invalid capture width")?;
    let h: usize = h.parse().map_err(|_| "invalid capture height")?;
    if parts.next() != Some(&b"255"[..]) { return Err("invalid monitor capture depth".into()); }
    let pixels = parts.next().unwrap_or_default();
    if w == 0 || h == 0 || w.checked_mul(h).and_then(|n| n.checked_mul(3)) != Some(pixels.len()) {
        return Err("invalid monitor pixel count".into());
    }
    let scale = (SCREEN_WIDTH as f64 / w as f64).min(SCREEN_HEIGHT as f64 / h as f64);
    let width = ((w as f64 * scale).round() as usize).clamp(1, SCREEN_WIDTH);
    let height = ((h as f64 * scale).round() as usize).clamp(1, SCREEN_HEIGHT);
    let mut image = vec![0; SCREEN_BYTES];
    for y in 0..height {
        for x in 0..width {
            let src = 3 * ((y*h/height)*w + x*w/width);
            let dst = 4 * (((SCREEN_HEIGHT-height)/2+y)*SCREEN_WIDTH + (SCREEN_WIDTH-width)/2+x);
            image[dst..dst+3].copy_from_slice(&pixels[src..src+3]);
            image[dst+3] = 255;
        }
    }
    Ok(image)
}

// GL samples both atlases by integer texel address on the existing noise unit.
pub(crate) fn gl_atlas() -> Vec<u8> {
    let mut pixels = CLOUD_NOISE[8..].to_vec();
    pixels.extend_from_slice(FINISHES);
    pixels.resize(pixels.len()+SCREEN_BYTES,0);
    pixels.extend_from_slice(&TABLE[16..]);
    pixels.resize(pixels.len().div_ceil(528 * 4) * 528 * 4, 0);
    pixels
}
pub(crate) fn scene_bytes(player: &[u8]) -> Result<Vec<u8>, String> {
    if player.len() > PLAYER_BYTES {
        return Err("player texture exceeds reserved region".into());
    }
    let mut out = vec![0u8; SCENE_BYTES];
    out[..player.len()].copy_from_slice(player);
    out[PLAYER_BYTES..CLOUD_NOISE_OFFSET].copy_from_slice(WOODLAND);
    out[CLOUD_NOISE_OFFSET..CLOUD_NOISE_OFFSET + CLOUD_NOISE.len()].copy_from_slice(CLOUD_NOISE);
    out[TABLE_OFFSET..TABLE_OFFSET+TABLE.len()].copy_from_slice(TABLE);
    out[TABLE_OFFSET+TABLE.len()..].copy_from_slice(FINISHES);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn monitor_image_preserves_orientation_and_centers_narrow_frames() {
        let mut ppm = b"P6\n2 2\n255\n".to_vec();
        ppm.extend_from_slice(&[255,0,0, 0,255,0, 0,0,255, 255,255,255]);
        let image = monitor_image(&ppm).unwrap();
        let pixel = |x,y| &image[(y*SCREEN_WIDTH+x)*4..(y*SCREEN_WIDTH+x)*4+3];
        assert_eq!(pixel(0usize,0usize), &[0,0,0]);
        assert_eq!(pixel(112,0), &[255,0,0]);
        assert_eq!(pixel(399,0), &[0,255,0]);
        assert_eq!(pixel(112,287), &[0,0,255]);
        assert_eq!(pixel(399,287), &[255,255,255]);
        assert!(monitor_image(&ppm[..ppm.len()-1]).is_err());
    }

    #[test]
    fn appended_assets_preserve_the_full_player_region_and_frame_alignment() {
        let player = [1, 0, 0, 0, 1, 0, 0, 0, 25, 50, 75, 255];
        let scene = scene_bytes(&player).unwrap();
        assert_eq!(&scene[..player.len()], &player);
        assert_eq!(&scene[PLAYER_BYTES..CLOUD_NOISE_OFFSET], WOODLAND);
        assert_eq!(
            &scene[CLOUD_NOISE_OFFSET..CLOUD_NOISE_OFFSET + CLOUD_NOISE.len()],
            CLOUD_NOISE
        );
        assert_eq!(CLOUD_FRAME_OFFSET % 16, 0);
        assert_eq!(SCREEN_OFFSET - CLOUD_FRAME_OFFSET, 192);
        assert!(scene_bytes(&vec![0; PLAYER_BYTES + 1]).is_err());
    }
}

#[cfg(test)]
mod table_tests {
    use super::*;
    #[test]
    fn material_tail_preserves_monitor_and_legacy_finish_layout() {
        let bytes=scene_bytes(&[1,0,0,0,1,0,0,0,255,255,255,255]).unwrap();
        assert_eq!(&bytes[TABLE_OFFSET..TABLE_OFFSET+TABLE.len()],TABLE);
        assert_eq!(&bytes[bytes.len()-FINISHES.len()..],FINISHES);
        assert!(bytes[SCREEN_OFFSET..TABLE_OFFSET].iter().all(|v|*v==0));
        assert_eq!((TABLE.len()+FINISHES.len())%16,0);
        let atlas=gl_atlas();let base=CLOUD_NOISE.len()-8+FINISHES.len()+SCREEN_BYTES;
        assert_eq!(&atlas[base..base+TABLE.len()-16],&TABLE[16..]);
    }
    #[test]
    fn baked_normals_and_material_channels_have_physical_ranges() {
        let texels=349525;let normal_start=16+texels*4;let packed_start=normal_start+texels*4;
        let mut min_height=255u8;let mut max_height=0u8;
        for p in TABLE[normal_start..packed_start].chunks_exact(4) {
            let n=[p[0],p[1],p[2]].map(|x|x as f32/255.*2.-1.);
            let length=n.iter().map(|x|x*x).sum::<f32>().sqrt();
            assert!((length-1.).abs()<0.015 && n[2]>0.);
        }
        for p in TABLE[packed_start..packed_start+texels*4].chunks_exact(4) {
            assert!((75..=185).contains(&p[0]) && p[2]==0 && p[3]>200);
            min_height=min_height.min(p[1]);max_height=max_height.max(p[1]);
        }
        assert!(max_height>min_height+8);
    }
    #[test]
    fn scene_shader_material_validates_and_camera_rejects_invalid_positions() {
        let module=naga::front::wgsl::parse_str(include_str!("../Procedures/Shaders/triangle.wgsl")).unwrap();
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(),naga::valid::Capabilities::all()).validate(&module).unwrap();
        assert_eq!(table_eye(&serde_json::json!({"table_eye":[1,8,-49]})).unwrap(),[1.,8.,-49.,1.]);
        assert!(table_eye(&serde_json::json!({"table_eye":[1,8]})).is_err());
        assert!(table_eye(&serde_json::json!({"table_eye":[10001,8,0]})).is_err());
    }
}

#[cfg(test)]
mod rock_tests {
    use super::*;
    #[test]
    fn rock_maps_follow_walnut_and_keep_palette_normals_and_mips() {
        assert_eq!(&TABLE[..WALNUT.len()],WALNUT);
        assert_eq!(&TABLE[WALNUT.len()..WALNUT.len()+ROCKS.len()],ROCKS);
        let texels=87381;let material_bytes=texels*3*4;
        for kind in 0..2 {
            let start=kind*material_bytes;let normal=start+texels*4;
            let packed=normal+texels*4;
            let last=&ROCKS[start+(texels-1)*4..start+texels*4];
            if kind==0 {assert!(last[0]>last[1] && last[1]>last[2]);}
            else {assert!(last[2]>last[1] && last[1]>last[0]);}
            for pixel in ROCKS[normal..packed].chunks_exact(4) {
                let n=[pixel[0],pixel[1],pixel[2]].map(|v|v as f32/255.*2.-1.);
                assert!((n.iter().map(|x|x*x).sum::<f32>().sqrt()-1.).abs()<0.015);
                assert!(n[2]>0.);
            }
            assert!(ROCKS[packed..packed+texels*4].chunks_exact(4).all(|p|p[0]>=160 && p[2]==0 && p[3]>=120));
        }
        let atlas=gl_atlas();let base=CLOUD_NOISE.len()-8+FINISHES.len()+SCREEN_BYTES+WALNUT.len()-16;
        assert_eq!(&atlas[base..base+ROCKS.len()],ROCKS);
    }
}

#[cfg(test)]
mod grass_tests {
    use super::*;
    #[test]
    fn grass_tail_matches_both_upload_paths_and_normal_mips() {
        let start=WALNUT.len()+ROCKS.len();
        assert_eq!(&TABLE[start..start+GRASS.len()],GRASS);
        let atlas=gl_atlas();
        let gl_start=CLOUD_NOISE.len()-8+FINISHES.len()+SCREEN_BYTES+start-16;
        assert_eq!(&atlas[gl_start..gl_start+GRASS.len()],GRASS);
        let texels=87381;
        for p in GRASS[texels*4..texels*8].chunks_exact(4) {
            let n=[p[0],p[1],p[2]].map(|v|v as f32/255.*2.-1.);
            assert!((n.iter().map(|x|x*x).sum::<f32>().sqrt()-1.).abs()<0.015);
            assert!(n[2]>0.);
        }
        assert!(GRASS[..256*256*4].chunks_exact(4).any(|p|p[1]>p[0]+10));
        assert_eq!(GRASS.len(),texels*8+8);
    }
}

#[cfg(test)]
mod dirt_tests {
    use super::*;
    #[test]
    fn fine_loam_preserves_prior_materials_and_uploads_normal_mips() {
        let start=WALNUT.len()+ROCKS.len()+GRASS.len();
        assert_eq!(&TABLE[start..start+DIRT.len()],DIRT);
        let atlas=gl_atlas();
        let gl_start=CLOUD_NOISE.len()-8+FINISHES.len()+SCREEN_BYTES+start-16;
        assert_eq!(&atlas[gl_start..gl_start+DIRT.len()],DIRT);
        let texels=349525;
        for p in DIRT[texels*4..texels*8].chunks_exact(4) {
            let n=[p[0],p[1],p[2]].map(|v|v as f32/255.*2.-1.);
            assert!((n.iter().map(|x|x*x).sum::<f32>().sqrt()-1.).abs()<0.015);
            assert!(n[2]>0.);
        }
        assert_eq!(DIRT.len(),texels*8+8);
        let last=&DIRT[(texels-1)*4..texels*4];
        assert!(last[0]>last[1] && last[1]>last[2]);
        // The brown-loam selector includes dirt/root mat and excludes turf/sand.
        let selects=|c:[f32;3]|c[0]>c[1]*1.28 && c[2]<c[1]*0.85;
        assert!(selects([0.39,0.29,0.18]) && selects([0.29,0.21,0.11]));
        assert!(!selects([0.21,0.39,0.17]) && !selects([0.78,0.67,0.45]));
    }
}

#[cfg(test)]
mod exterior_tests {
 use super::*;
 #[test] fn exterior_mips_append_without_relocating_existing_materials(){
  assert_eq!(EXTERIOR_OFFSET,WALNUT.len()+ROCKS.len()+GRASS.len()+DIRT.len());
  let maps=&TABLE[EXTERIOR_OFFSET..CARPET_OFFSET];assert_eq!(maps.len(),4_194_336);
  let texels=349525;let mut lo=255u8;let mut hi=0u8;
  for p in maps[32+texels*4..32+texels*8].chunks_exact(4){
   let n=[p[0],p[1],p[2]].map(|v|v as f32/255.*2.-1.);
   assert!((n.iter().map(|v|v*v).sum::<f32>().sqrt()-1.).abs()<0.015);assert!(n[2]>0.);
  }
  for p in maps[32+texels*8..32+texels*12].chunks_exact(4){
   lo=lo.min(p[1]);hi=hi.max(p[1]);assert!(p[0]>=170 && p[2]==0 && p[3]>=200);
  }
  assert!(hi>lo+60,"joints need distinct height relief");
  let atlas=gl_atlas();let base=CLOUD_NOISE.len()-8+FINISHES.len()+SCREEN_BYTES+EXTERIOR_OFFSET-16;
  assert_eq!(&atlas[base..base+maps.len()],maps);
 }
 #[test] fn exterior_lighting_tracks_the_view_daylight(){
  let day=exterior_light(&serde_json::json!({"position":[0.,6371000.,0.],"day_night":{"fraction":0.5}}));
  let night=exterior_light(&serde_json::json!({"position":[0.,6371000.,0.],"day_night":{"fraction":0.}}));
  assert!(day[3]>night[3]*5.);assert!(day.iter().chain(night.iter()).all(|n|n.is_finite()));
 }
}

#[cfg(test)]
mod carpet_tests {
 use super::*;
 #[test] fn carpet_mips_preserve_previous_materials_and_both_upload_paths(){
  let carpet=include_bytes!("../Procedures/Assets/Theatre/carpet_material.rgba");
  assert_eq!(CARPET_OFFSET,EXTERIOR_OFFSET+4_194_336);
  assert_eq!(&TABLE[CARPET_OFFSET..],carpet);
  assert_eq!(carpet.len(),4_194_304);
  let texels=349525;
  for p in carpet[texels*4..texels*8].chunks_exact(4){
   let n=[p[0],p[1],p[2]].map(|v|v as f32/255.*2.-1.);
   assert!((n.iter().map(|v|v*v).sum::<f32>().sqrt()-1.).abs()<0.015 && n[2]>0.);
  }
  for p in carpet[..texels*4].chunks_exact(4){assert!(p[0]>p[1]*4 && p[0]>p[2]*4 && p[3]>=225);}
  for p in carpet[texels*8..texels*12].chunks_exact(4){assert!(p[0]>=219 && p[0]<=253 && p[2]==0 && p[3]>=225);}
  let atlas=gl_atlas();let start=CLOUD_NOISE.len()-8+FINISHES.len()+SCREEN_BYTES+CARPET_OFFSET-16;
  assert_eq!(&atlas[start..start+carpet.len()],carpet);
  let scene=scene_bytes(&[]).unwrap();assert_eq!(&scene[TABLE_OFFSET+CARPET_OFFSET..TABLE_OFFSET+TABLE.len()],carpet);
 }
}
