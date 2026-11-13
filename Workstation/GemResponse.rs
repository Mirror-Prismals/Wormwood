//! Immutable optical transport loaded once, outside the audio callbacks.
//! Missing/stale assets leave a zero descriptor and select canonical tracing.
use std::path::Path;

pub(crate) const BASE: usize = 3_500_544;
pub(crate) const RECORD_ROWS: usize = 16 + 128 * 128 * 60;
pub(crate) const ROWS: usize = 4 * RECORD_ROWS;

fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325u64,
        |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3));
    format!("{hash:016x}")
}

pub(crate) fn load(root: &Path, rows: &mut [[f32; 4]]) -> Result<(), String> {
    let directory = root.join("Art/AudioWorkstation/GemResponse");
    if !directory.join("response.json").is_file() { return Ok(()); }
    let read = |path: &Path| std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()));
    let document: serde_json::Value = serde_json::from_slice(&read(&directory.join("response.json"))?)
        .map_err(|e| e.to_string())?;
    if document["schema"] != 1 || document["base"] != BASE || document["rows"] != ROWS {
        return Err("unsupported optical response layout".into());
    }
    // Explicit source list; paths supplied by a manifest are never followed.
    for name in ["DecorativeControls/controls.bin", "DecorativeControls/controls.json",
                 "ControlEngraving/control_engraving.bin", "ControlEngraving/control_engraving.json"] {
        let actual = read(&root.join("Art/AudioWorkstation").join(name))?;
        if document["sources"][name].as_str() != Some(fingerprint(&actual).as_str()) {
            return Err(format!("optical response is stale for {name}"));
        }
    }
    let bytes = read(&directory.join("response.bin"))?;
    if bytes.len() != ROWS * 16 || document["fingerprint"].as_str() != Some(fingerprint(&bytes).as_str()) {
        return Err("optical response data failed integrity checks".into());
    }
    if rows.len() < BASE + ROWS { return Err("optical response storage is too small".into()); }
    // Validate before publishing any descriptor; failed loads cannot enable a
    // partially uploaded table. Float words are numeric, never packed bitcasts.
    if bytes.chunks_exact(4).any(|b| !f32::from_le_bytes(b.try_into().unwrap()).is_finite()) {
        return Err("nonfinite optical response".into());
    }
    for instance in 0..4 {
        let start = instance * RECORD_ROWS * 16;
        if f32::from_le_bytes(bytes[start..start+4].try_into().unwrap()) != 731.0 {
            return Err("missing optical response descriptor".into());
        }
    }
    for (row, words) in rows[BASE..BASE+ROWS].iter_mut().zip(bytes.chunks_exact(16)) {
        for (value, word) in row.iter_mut().zip(words.chunks_exact(4)) {
            *value = f32::from_le_bytes(word.try_into().unwrap());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn response_storage_and_shared_shader_agree() {
        let original = (super::super::user_style::BASE + super::super::user_style::ROWS).div_ceil(256)*256;
        assert_eq!(BASE, original);
        assert!((BASE+ROWS).div_ceil(512)*512*16 < 128*1024*1024);
        let source = include_str!("GemResponseKernel.glsl");
        assert!(source.contains(&format!("int base={BASE};")));
        assert!(source.contains(&format!("instance*{RECORD_ROWS}")));
    }
    #[test]
    fn absent_response_keeps_full_renderer() {
        let mut rows = [[0.; 4]; 1];
        load(Path::new("/nonexistent-wormwood-response"), &mut rows).unwrap();
        assert_eq!(rows, [[0.; 4]; 1]);
    }
}
