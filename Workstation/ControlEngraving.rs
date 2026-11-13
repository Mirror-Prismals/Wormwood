//! Immutable SSLE densities in authored control-local metres. The control BVH
//! packet owns the small placement descriptors; this packet owns only density.
//! Loading and validation run on the control thread, never the audio callback.
use super::{bubble_notes, decorative_controls};
use serde_json::Value;
use std::{collections::BTreeSet, fs::File, io::Read, path::Path};

pub(crate) const BASE: usize = (bubble_notes::BASE + bubble_notes::ROWS + 255) & !255;
pub(crate) const ROWS: usize = 262_144;
pub(crate) const HEADER: usize = 244;
const DIRECTORY: &str = "Art/AudioWorkstation/ControlEngraving";
const NATIVE_ASSETS: [&str; 5] = [
    "GemButton_Rewind",
    "GemButton_Stop",
    "GemButton_Record",
    "GemButton_Play",
    "GemFader_Ivory",
];

struct Mip {
    size: [usize; 2],
    offset: usize,
}
struct Pattern {
    id: String,
    offset: usize,
    texels: usize,
    origin: [f64; 2],
    extent: [f64; 2],
    unit: f64,
    mips: Vec<Mip>,
}
struct Asset {
    name: String,
    pattern: String,
    layers: [f64; 3],
}
pub(crate) struct Kit {
    density: Vec<[f32; 4]>,
    patterns: Vec<Pattern>,
    assets: Vec<Asset>,
    manifest_sha256: String,
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err(format!("{} exceeds its size bound", path.display()));
    }
    Ok(bytes)
}
fn integer(value: &Value) -> Result<usize, String> {
    value
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| "invalid control engraving integer".into())
}
fn number(value: &Value) -> Result<f64, String> {
    value
        .as_f64()
        .filter(|n| n.is_finite() && n.abs() <= 1.)
        .ok_or_else(|| "invalid control engraving metre value".into())
}
fn vector<const N: usize>(value: &Value) -> Result<[f64; N], String> {
    let values = value
        .as_array()
        .filter(|v| v.len() == N)
        .ok_or("invalid control engraving vector")?;
    let mut result = [0.; N];
    for (out, value) in result.iter_mut().zip(values) {
        *out = number(value)?;
    }
    Ok(result)
}
fn label(value: &Value) -> Result<&str, String> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or_else(|| "invalid control engraving label".into())
}
fn relative_path(value: &Value) -> Result<&str, String> {
    let name = label(value)?;
    if Path::new(name)
        .components()
        .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("control engraving path must be relative without traversal".into());
    }
    Ok(name)
}
fn array(value: &Value) -> Result<&[Value], String> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| "invalid control engraving array".into())
}
fn row_index(value: f32) -> Result<usize, String> {
    if !value.is_finite() || !(0.0..=16_777_216.).contains(&value) || value.fract() != 0. {
        return Err("invalid control engraving row address".into());
    }
    Ok(value as usize)
}

impl Pattern {
    fn read(value: &Value) -> Result<Self, String> {
        let size = array(&value["density_base_size_px"])?;
        if size.len() != 2 {
            return Err("invalid control engraving density dimensions".into());
        }
        let mut expected = [integer(&size[0])?, integer(&size[1])?];
        if expected[0] != 128 || !matches!(expected[1], 128 | 1024) {
            return Err("control density requires 128x128 or 128x1024 mips".into());
        }
        let levels = array(&value["density_mips"])?;
        if levels.is_empty() || levels.len() > 11 {
            return Err("invalid control density mip count".into());
        }
        let mut mips = Vec::new();
        let mut next = 0;
        for (i, mip) in levels.iter().enumerate() {
            let size = array(&mip["size_px"])?;
            if size.len() != 2
                || [integer(&size[0])?, integer(&size[1])?] != expected
                || integer(&mip["offset_texels"])? != next
            {
                return Err("control density mip chain has a gap or wrong size".into());
            }
            mips.push(Mip {
                size: expected,
                offset: next,
            });
            next += expected[0] * expected[1];
            if expected == [1, 1] && i + 1 != levels.len() {
                return Err("control density mip chain repeats its final level".into());
            }
            expected = expected.map(|v| (v / 2).max(1));
        }
        let texels = integer(&value["density_texels_padded"])?;
        let origin = vector(&value["origin_m"])?;
        let extent: [f64; 2] = vector(&value["extent_m"])?;
        let unit = number(&value["unit_m"])?;
        let pitch = number(&value["pitch_m"])?;
        if mips.last().is_none_or(|m| m.size != [1, 1])
            || texels != next.div_ceil(2) * 2
            || integer(&value["density_rows"])? != texels / 2
            || unit <= 0.
            || extent.iter().any(|v| *v <= 0.)
            || (pitch - unit * 0.0125).abs() > 1e-12
        {
            return Err("invalid control density domain or final mip".into());
        }
        let offset = integer(&value["density_offset_rows"])?;
        if offset > ROWS || texels / 2 > ROWS - offset {
            return Err("control density pattern exceeds immutable storage".into());
        }
        Ok(Self {
            id: label(&value["id"])?.into(),
            offset,
            texels,
            origin,
            extent,
            unit,
            mips,
        })
    }
}

impl Kit {
    pub(crate) fn load(root: &Path) -> Result<Self, String> {
        let directory = root.join(DIRECTORY);
        let manifest_bytes = read_bounded(&directory.join("control_engraving.json"), 262_144)?;
        let document: Value = serde_json::from_slice(&manifest_bytes).map_err(|e| e.to_string())?;
        let panel: Value = serde_json::from_slice(&read_bounded(
            &root.join("Art/AudioWorkstation/LaserEngraving/engraving.json"),
            262_144,
        )?)
        .map_err(|e| e.to_string())?;
        if integer(&document["schema"])? != 1
            || !document["style"].is_object()
            || document["style"] != panel["style"]
        {
            return Err("control engraving must use the existing panel SSLE style".into());
        }
        let pattern_values = array(&document["patterns"])?;
        let asset_values = array(&document["assets"])?;
        if !(5..=32).contains(&pattern_values.len()) || !(5..=32).contains(&asset_values.len()) {
            return Err("control engraving needs bounded transport and fader assets".into());
        }
        let patterns = pattern_values
            .iter()
            .map(Pattern::read)
            .collect::<Result<Vec<_>, _>>()?;
        let mut ids = BTreeSet::new();
        let mut ranges = Vec::new();
        for p in &patterns {
            if !ids.insert(p.id.as_str()) {
                return Err("duplicate control density pattern".into());
            }
            ranges.push((p.offset, p.offset + p.texels / 2));
        }
        ranges.sort_unstable();
        let mut used = 0;
        for (first, end) in ranges {
            if first != used {
                return Err("control density patterns overlap or leave a gap".into());
            }
            used = end;
        }
        if integer(&document["row_bytes"])? != 16
            || integer(&document["density_rows"])? != used
            || integer(&document["density_bytes"])? != used * 16
            || integer(&document["native_header_row"])? != HEADER
        {
            return Err("control density manifest disagrees with its packet layout".into());
        }
        let mut assets = Vec::new();
        let mut names = BTreeSet::new();
        for asset in asset_values {
            let name = label(&asset["name"])?;
            if !names.insert(name) {
                return Err("duplicate control engraving asset".into());
            }
            relative_path(&asset["glb"])?;
            relative_path(&asset["source_glb"])?;
            label(&asset["host_node"])?;
            label(&asset["host_material"])?;
            for node in array(&asset["removed_nodes"])? {
                label(node)?;
            }
            let pattern = label(&asset["pattern_id"])?;
            let p = patterns
                .iter()
                .find(|p| p.id == pattern)
                .ok_or("unknown control engraving pattern")?;
            let layers: [f64; 3] = vector(&asset["layer_z_m"])?;
            let surface = number(&asset["source_surface_z_m"])?;
            if layers[0] >= surface
                || (layers[1] - (layers[0] - 0.07 * p.unit)).abs() > 1e-10
                || (layers[2] - (layers[0] - 0.14 * p.unit)).abs() > 1e-10
            {
                return Err(
                    "control engraving layers must descend inside their source surface".into(),
                );
            }
            assets.push(Asset {
                name: name.into(),
                pattern: pattern.into(),
                layers,
            });
        }
        if NATIVE_ASSETS.iter().any(|name| !names.contains(name)) {
            return Err("control engraving is missing a native transport or fader asset".into());
        }
        let binary = relative_path(&document["density_binary"])?;
        // Keep the packet in its own authored directory, independent of cwd.
        if Path::new(binary).components().count() != 1 {
            return Err("invalid control density filename".into());
        }
        let bytes = read_bounded(&directory.join(binary), ROWS * 16)?;
        let hash = label(&document["density_sha256"])?;
        if bytes.len() != used * 16
            || hash.len() != 64
            || !hash.bytes().all(|b| b.is_ascii_hexdigit())
            || !hash.eq_ignore_ascii_case(&sha256(&bytes))
        {
            return Err("control density length or SHA256 disagrees with its manifest".into());
        }
        let density = decode_density(&bytes)?;
        for pattern in &patterns {
            let last = density[pattern.offset + pattern.texels / 2 - 1];
            if last[2] != 0. || last[3] != 0. {
                return Err("control density final padding texel must be zero".into());
            }
        }
        Ok(Self {
            density,
            patterns,
            assets,
            manifest_sha256: sha256(&manifest_bytes),
        })
    }

    pub(crate) fn manifest_sha256(&self) -> &str {
        &self.manifest_sha256
    }
    pub(crate) fn used_rows(&self) -> usize {
        self.density.len()
    }
    pub(crate) fn read_density(self) -> Vec<[f32; 4]> {
        self.density
    }

    /// Only the original control palette gives material+2.w this meaning. The
    /// separate monitor palette uses that word for its opaque clear-coat weight.
    pub(crate) fn validate_controls(
        &self,
        rows: &[[f32; 4]],
        used: usize,
        materials: usize,
    ) -> Result<(), String> {
        let base = decorative_controls::BASE;
        if used > rows.len() || materials >= 256 || materials * 40 > used {
            return Err("invalid control engraving material table".into());
        }
        let get = |address: usize| -> Result<[f32; 4], String> {
            address
                .checked_sub(base)
                .filter(|i| *i < used)
                .and_then(|i| rows.get(i))
                .copied()
                .ok_or_else(|| "control engraving descriptor leaves the control packet".into())
        };
        let mut matched = BTreeSet::new();
        for material in 0..materials {
            let pointer = row_index(rows[material * 40 + 2][3])?;
            if pointer == 0 {
                continue;
            }
            if rows[material * 40][3] < 0.0001 || pointer < base + materials * 40 {
                return Err(
                    "control engraving descriptor requires an original dielectric material".into(),
                );
            }
            let domain = get(pointer)?;
            let layers = get(pointer + 1)?;
            let info = get(pointer + 2)?;
            let offset = row_index(info[0])?;
            let p = self
                .patterns
                .iter()
                .find(|p| p.offset == offset)
                .ok_or("unknown control density offset")?;
            if domain
                != [
                    p.origin[0] as f32,
                    p.origin[1] as f32,
                    p.extent[0] as f32,
                    p.extent[1] as f32,
                ]
                || layers[3] != p.unit as f32
                || row_index(info[1])? != p.mips.len()
                || row_index(info[2])? != pointer + 3
                || info[3] != 0.
            {
                return Err("control engraving descriptor disagrees with authored density".into());
            }
            let asset = self
                .assets
                .iter()
                .find(|a| {
                    NATIVE_ASSETS.contains(&a.name.as_str())
                        && a.pattern == p.id
                        && [layers[0], layers[1], layers[2]] == a.layers.map(|z| z as f32)
                })
                .ok_or("control engraving layer placement disagrees with authored asset")?;
            if !matched.insert(asset.name.as_str()) {
                return Err("duplicate native control engraving descriptor".into());
            }
            for (i, mip) in p.mips.iter().enumerate() {
                if get(pointer + 3 + i)?
                    != [
                        mip.size[0] as f32,
                        mip.size[1] as f32,
                        mip.offset as f32,
                        0.,
                    ]
                {
                    return Err(
                        "control engraving mip descriptor disagrees with authored density".into(),
                    );
                }
            }
        }
        if matched.len() != NATIVE_ASSETS.len() {
            return Err("missing native control engraving descriptors".into());
        }
        Ok(())
    }
}

fn decode_density(bytes: &[u8]) -> Result<Vec<[f32; 4]>, String> {
    if bytes.len() % 16 != 0 {
        return Err("unaligned control density packet".into());
    }
    let mut rows = Vec::with_capacity(bytes.len() / 16);
    for bytes in bytes.chunks_exact(16) {
        let row: [f32; 4] = std::array::from_fn(|i| {
            f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
        });
        if row.iter().enumerate().any(|(i, v)| {
            !v.is_finite()
                || *v < 0.
                || *v > if i % 2 == 0 { 65535. } else { 255. }
                || v.fract() != 0.
        }) {
            return Err("invalid packed control density texel".into());
        }
        rows.push(row);
    }
    Ok(rows)
}

// Small dependency-free SHA256 for bounded immutable authoring packets. This is
// content identity, not an authentication mechanism or a real-time operation.
pub(super) fn sha256(bytes: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let length = (bytes.len() as u64 * 8).to_be_bytes();
    let padded = (bytes.len() + 9).div_ceil(64) * 64;
    for offset in (0..padded).step_by(64) {
        let mut block = [0u8; 64];
        for (i, out) in block.iter_mut().enumerate() {
            let at = offset + i;
            *out = if at < bytes.len() {
                bytes[at]
            } else if at == bytes.len() {
                0x80
            } else if at >= padded - 8 {
                length[at - (padded - 8)]
            } else {
                0
            };
        }
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
        }
        for i in 16..64 {
            let a = w[i - 15];
            let b = w[i - 2];
            let s0 = a.rotate_right(7) ^ a.rotate_right(18) ^ (a >> 3);
            let s1 = b.rotate_right(17) ^ b.rotate_right(19) ^ (b >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add((e & f) ^ (!e & g))
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let t2 = s0.wrapping_add((a & b) ^ (a & c) ^ (b & c));
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (out, v) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *out = out.wrapping_add(v);
        }
    }
    state.iter().map(|v| format!("{v:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pattern_document(height: usize) -> Value {
        let mut size = [128, height];
        let mut texels: usize = 0;
        let mut mips = Vec::new();
        loop {
            mips.push(serde_json::json!({"size_px": size, "offset_texels": texels}));
            texels += size[0] * size[1];
            if size == [1, 1] {
                break;
            }
            size = size.map(|v| (v / 2).max(1));
        }
        let texels = texels.div_ceil(2) * 2;
        serde_json::json!({"id":"test", "density_offset_rows":0,
            "density_base_size_px":[128,height], "density_mips":mips,
            "density_texels_padded":texels, "density_rows":texels/2,
            "origin_m":[-0.012,-0.012], "extent_m":[0.024,0.024],
            "unit_m":0.02, "pitch_m":0.00025})
    }

    #[test]
    fn workstation_control_density_mips_are_complete_and_bounded() {
        for height in [128, 1024] {
            let good = pattern_document(height);
            assert!(Pattern::read(&good).is_ok());
            let mut gap = good.clone();
            gap["density_mips"][1]["offset_texels"] = serde_json::json!(1);
            assert!(Pattern::read(&gap).is_err());
            let mut truncated = good.clone();
            truncated["density_mips"].as_array_mut().unwrap().pop();
            assert!(Pattern::read(&truncated).is_err());
            let mut out_of_storage = good;
            out_of_storage["density_offset_rows"] = serde_json::json!(ROWS);
            assert!(Pattern::read(&out_of_storage).is_err());
        }
    }

    #[test]
    fn workstation_control_density_descriptors_reject_wrong_mips_and_opaque_hosts() {
        let mut kit = Kit {
            density: Vec::new(),
            patterns: Vec::new(),
            assets: Vec::new(),
            manifest_sha256: String::new(),
        };
        let mut rows = vec![[0.; 4]; 5 * 40 + 5 * 11];
        for (i, name) in NATIVE_ASSETS.iter().enumerate() {
            let mut pattern = Pattern::read(&pattern_document(128)).unwrap();
            pattern.id = format!("pattern{i}");
            pattern.offset = i * pattern.texels / 2;
            let layers = [-0.001, -0.0024, -0.0038];
            let at = 5 * 40 + i * 11;
            rows[i * 40][3] = 1.;
            rows[i * 40 + 2][3] = (decorative_controls::BASE + at) as f32;
            rows[at] = [-0.012, -0.012, 0.024, 0.024];
            rows[at + 1] = [layers[0] as f32, layers[1] as f32, layers[2] as f32, 0.02];
            rows[at + 2] = [
                pattern.offset as f32,
                8.,
                (decorative_controls::BASE + at + 3) as f32,
                0.,
            ];
            for (level, mip) in pattern.mips.iter().enumerate() {
                rows[at + 3 + level] = [
                    mip.size[0] as f32,
                    mip.size[1] as f32,
                    mip.offset as f32,
                    0.,
                ];
            }
            kit.assets.push(Asset {
                name: (*name).into(),
                pattern: pattern.id.clone(),
                layers,
            });
            kit.patterns.push(pattern);
        }
        assert!(kit.validate_controls(&rows, rows.len(), 5).is_ok());
        let mut bad = rows.clone();
        bad[200 + 3][2] = 42.;
        assert!(kit.validate_controls(&bad, bad.len(), 5).is_err());
        let mut bad = rows.clone();
        bad[0][3] = 0.;
        assert!(kit.validate_controls(&bad, bad.len(), 5).is_err());
        let mut bad = rows;
        bad[2][3] = (decorative_controls::BASE + bad.len()) as f32;
        assert!(kit.validate_controls(&bad, bad.len(), 5).is_err());
    }

    #[test]
    fn workstation_control_density_hash_and_packing() {
        assert_eq!(
            sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        let pack = |r: [f32; 4]| r.into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>();
        assert!(decode_density(&pack([65535., 255., 0., 0.])).is_ok());
        for row in [
            [0., 256., 0., 0.],
            [0.5, 0., 0., 0.],
            [f32::NAN, 0., 0., 0.],
        ] {
            assert!(decode_density(&pack(row)).is_err());
        }
        assert!(decode_density(&[0; 15]).is_err());
    }
}
