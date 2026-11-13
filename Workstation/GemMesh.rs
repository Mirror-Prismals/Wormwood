//! Loads the authored blank gem panels on the control thread. This deliberately
//! accepts the static, single-mesh GLBs in this kit, rather than silently losing
//! transforms, skins, morph targets or material textures from arbitrary glTF.
use serde_json::Value;
use std::{fs::File, io::Read, path::Path};

const DIRECTORY: &str = "Art/AudioWorkstation/GemPanels";
const NAMES: [&str; 4] = [
    "GemPanel_Broad",
    "GemPanel_Tall",
    "GemPanel_Compact",
    "GemPanel_Strip",
];
const MAX_FILE_BYTES: usize = 1024 * 1024;
const MAX_VERTICES: usize = 4096;
const MAX_INDICES: usize = 12288;

#[derive(Clone, Copy)]
pub(crate) struct Vertex {
    pub position: [f64; 3],
    pub normal: [f64; 3],
}

#[derive(Clone)]
pub(crate) struct Material {
    /// Original RubyDaw tint metadata; volume absorption is separately retained.
    pub tint: [f64; 4],
    pub roughness: f64,
    pub ior: f64,
    pub transmission: f64,
    pub attenuation_color: [f64; 3],
    pub attenuation_distance_m: f64,
    pub thickness_m: f64,
}

#[derive(Clone)]
pub(crate) struct PanelMesh {
    pub name: String,
    /// Expanded triangle list, with the authored flat normals. Coordinates are
    /// metres: +X right, +Y up, +Z outward from the flat control table at Z=0.
    pub vertices: Vec<Vertex>,
    pub dimensions_m: [f64; 3],
    pub safe_rectangle_m: [f64; 2],
    pub material: Material,
}

/// `root` is the Wormwood root. Missing/unsupported assets are explicit errors;
/// no synthesized panel or visual fallback is supplied.
pub(crate) fn load(root: &Path) -> Result<Vec<PanelMesh>, String> {
    NAMES
        .iter()
        .map(|name| {
            let path = root.join(DIRECTORY).join(format!("{name}.glb"));
            let result = (|| {
                let file = File::open(&path).map_err(|e| e.to_string())?;
                let mut bytes = Vec::new();
                file.take((MAX_FILE_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                Glb::parse(&bytes)?.panel(name)
            })();
            result.map_err(|e: String| format!("{}: {e}", path.display()))
        })
        .collect()
}

fn integer(value: &Value, label: &str) -> Result<usize, String> {
    value
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| format!("invalid {label}"))
}

fn optional_integer(value: &Value, default: usize, label: &str) -> Result<usize, String> {
    if value.is_null() {
        Ok(default)
    } else {
        integer(value, label)
    }
}

fn scalar(value: &Value, min: f64, max: f64, label: &str) -> Result<f64, String> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && *v >= min && *v <= max)
        .ok_or_else(|| format!("invalid {label}"))
}

fn vector<const N: usize>(value: &Value, label: &str) -> Result<[f64; N], String> {
    let values = value
        .as_array()
        .filter(|a| a.len() == N)
        .ok_or_else(|| format!("invalid {label}"))?;
    let mut result = [0.; N];
    for (i, v) in values.iter().enumerate() {
        result[i] = scalar(v, -16., 16., label)?;
    }
    Ok(result)
}

fn identity(value: &Value, expected: &[f64], label: &str) -> Result<(), String> {
    if value.is_null() {
        return Ok(());
    }
    let values = value
        .as_array()
        .filter(|a| a.len() == expected.len())
        .ok_or_else(|| format!("invalid {label}"))?;
    if values.iter().zip(expected).all(|(v, expected)| {
        v.as_f64()
            .is_some_and(|n| n.is_finite() && (n - expected).abs() < 1e-8)
    }) {
        Ok(())
    } else {
        Err(format!(
            "nonidentity {label} is unsupported by the panel kit loader"
        ))
    }
}

fn u32le(bytes: &[u8], offset: usize) -> Result<usize, String> {
    let end = offset.checked_add(4).ok_or("GLB offset overflow")?;
    let bytes = bytes.get(offset..end).ok_or("truncated GLB")?;
    Ok(u32::from_le_bytes(bytes.try_into().map_err(|_| "invalid GLB word")?) as usize)
}

struct Glb<'a> {
    document: Value,
    bin: &'a [u8],
}

struct Accessor<'a> {
    bytes: &'a [u8],
    start: usize,
    stride: usize,
    count: usize,
    component: usize,
}

impl Accessor<'_> {
    // All row ranges have been checked once by Glb::accessor.
    fn row(&self, i: usize, width: usize) -> &[u8] {
        let offset = self.start + i * self.stride;
        &self.bytes[offset..offset + width]
    }
}

impl<'a> Glb<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self, String> {
        if bytes.len() > MAX_FILE_BYTES
            || u32le(bytes, 0)? != 0x46546c67
            || u32le(bytes, 4)? != 2
            || u32le(bytes, 8)? != bytes.len()
        {
            return Err("invalid or oversized GLB 2 header".into());
        }
        let mut at: usize = 12;
        let mut document = None;
        let mut bin = None;
        while at < bytes.len() {
            let length = u32le(bytes, at)?;
            let kind = u32le(bytes, at + 4)?;
            at += 8;
            let end = at.checked_add(length).ok_or("GLB chunk overflow")?;
            let chunk = bytes.get(at..end).ok_or("truncated GLB chunk")?;
            if length % 4 != 0 {
                return Err("unaligned GLB chunk".into());
            }
            match kind {
                0x4e4f534a if document.is_none() && bin.is_none() => {
                    document = Some(serde_json::from_slice(chunk).map_err(|e| e.to_string())?);
                }
                0x004e4942 if document.is_some() && bin.is_none() => bin = Some(chunk),
                _ => return Err("duplicate, reordered or unsupported GLB chunk".into()),
            }
            at = end;
        }
        let document: Value = document.ok_or("missing GLB JSON")?;
        let bin = bin.ok_or("missing GLB BIN")?;
        if document["asset"]["version"] != "2.0" {
            return Err("GLB asset version must be 2.0".into());
        }
        if let Some(required) = document.get("extensionsRequired") {
            let required = required.as_array().ok_or("invalid required extensions")?;
            if required.iter().any(|e| {
                !matches!(
                    e.as_str(),
                    Some(
                        "KHR_materials_transmission" | "KHR_materials_ior" | "KHR_materials_volume"
                    )
                )
            }) {
                return Err("unsupported required GLB extension".into());
            }
        }
        let buffers = document["buffers"]
            .as_array()
            .filter(|a| a.len() == 1)
            .ok_or("panel GLB requires one embedded buffer")?;
        if !buffers[0]["uri"].is_null() {
            return Err("external buffers unsupported".into());
        }
        let length = integer(&buffers[0]["byteLength"], "buffer length")?;
        if length > bin.len() || bin.len() - length > 3 {
            return Err("embedded buffer length disagrees with BIN chunk".into());
        }
        Ok(Self {
            document,
            bin: &bin[..length],
        })
    }

    fn accessor(
        &self,
        id: &Value,
        shape: &str,
        width: usize,
        limit: usize,
    ) -> Result<Accessor<'_>, String> {
        let id = integer(id, "accessor index")?;
        let accessor = self.document["accessors"]
            .get(id)
            .ok_or("missing accessor")?;
        if accessor["type"] != shape
            || !accessor["sparse"].is_null()
            || (!accessor["normalized"].is_null() && accessor["normalized"] != false)
        {
            return Err("unsupported panel accessor shape, sparse data or normalization".into());
        }
        let component = integer(&accessor["componentType"], "component type")?;
        let size = match (shape, component) {
            ("VEC3", 5126) => 4,
            ("SCALAR", 5121) => 1,
            ("SCALAR", 5123) => 2,
            ("SCALAR", 5125) => 4,
            _ => return Err("unsupported panel accessor component type".into()),
        };
        let count = integer(&accessor["count"], "accessor count")?;
        if count == 0 || count > limit {
            return Err("panel accessor count outside budget".into());
        }
        let view_id = integer(&accessor["bufferView"], "buffer view index")?;
        let view = self.document["bufferViews"]
            .get(view_id)
            .ok_or("missing buffer view")?;
        if integer(&view["buffer"], "buffer index")? != 0 {
            return Err("nonembedded buffer view".into());
        }
        let offset = optional_integer(&view["byteOffset"], 0, "view offset")?;
        let length = integer(&view["byteLength"], "view length")?;
        let end = offset.checked_add(length).ok_or("buffer view overflow")?;
        let bytes = self.bin.get(offset..end).ok_or("buffer view outside BIN")?;
        let packed = size * width;
        let stride = optional_integer(&view["byteStride"], packed, "view stride")?;
        let start = optional_integer(&accessor["byteOffset"], 0, "accessor offset")?;
        if stride < packed
            || stride > 252
            || stride % size != 0
            || offset % size != 0
            || start % size != 0
            || (shape == "SCALAR" && !view["byteStride"].is_null())
        {
            return Err("invalid panel accessor alignment or stride".into());
        }
        let last = (count - 1)
            .checked_mul(stride)
            .and_then(|v| v.checked_add(start))
            .and_then(|v| v.checked_add(packed))
            .ok_or("accessor range overflow")?;
        if last > bytes.len() {
            return Err("accessor outside buffer view".into());
        }
        Ok(Accessor {
            bytes,
            start,
            stride,
            count,
            component,
        })
    }

    fn vectors(&self, id: &Value) -> Result<Vec<[f64; 3]>, String> {
        let accessor = self.accessor(id, "VEC3", 3, MAX_VERTICES)?;
        (0..accessor.count)
            .map(|i| {
                let row = accessor.row(i, 12);
                let mut v = [0.; 3];
                for (j, out) in v.iter_mut().enumerate() {
                    *out = f32::from_le_bytes(
                        row[j * 4..j * 4 + 4]
                            .try_into()
                            .map_err(|_| "invalid float component")?,
                    ) as f64;
                    if !out.is_finite() || out.abs() > 16. {
                        return Err("invalid panel vector".into());
                    }
                }
                // Undo Blender's glTF Y-up export. This rotation preserves winding.
                Ok([v[0], -v[2], v[1]])
            })
            .collect()
    }

    fn indices(&self, id: &Value) -> Result<Vec<usize>, String> {
        let accessor = self.accessor(id, "SCALAR", 1, MAX_INDICES)?;
        if accessor.count % 3 != 0 {
            return Err("panel indices must form triangles".into());
        }
        (0..accessor.count)
            .map(|i| {
                Ok(match accessor.component {
                    5121 => accessor.row(i, 1)[0] as usize,
                    5123 => u16::from_le_bytes(
                        accessor
                            .row(i, 2)
                            .try_into()
                            .map_err(|_| "invalid short index")?,
                    ) as usize,
                    _ => u32::from_le_bytes(
                        accessor
                            .row(i, 4)
                            .try_into()
                            .map_err(|_| "invalid integer index")?,
                    ) as usize,
                })
            })
            .collect()
    }

    fn panel(&self, expected_name: &str) -> Result<PanelMesh, String> {
        let g = &self.document;
        for key in ["skins", "animations"] {
            if !g[key].is_null() && g[key].as_array().is_none_or(|a| !a.is_empty()) {
                return Err(format!("panel kit does not support {key}"));
            }
        }
        let scene_id = optional_integer(&g["scene"], 0, "scene index")?;
        let scene = g["scenes"].get(scene_id).ok_or("missing panel scene")?;
        let roots = scene["nodes"]
            .as_array()
            .filter(|a| a.len() == 1)
            .ok_or("one panel scene node required")?;
        if integer(&roots[0], "scene node index")? != 0 {
            return Err("panel root must be node zero".into());
        }
        let nodes = g["nodes"]
            .as_array()
            .filter(|a| a.len() == 1)
            .ok_or("one static panel node required")?;
        let node = &nodes[0];
        if node["name"].as_str() != Some(expected_name)
            || !node["skin"].is_null()
            || !node["weights"].is_null()
            || (!node["children"].is_null()
                && node["children"].as_array().is_none_or(|a| !a.is_empty()))
        {
            return Err("unexpected panel node name, skin, morph weights or children".into());
        }
        identity(&node["translation"], &[0., 0., 0.], "panel translation")?;
        identity(&node["rotation"], &[0., 0., 0., 1.], "panel rotation")?;
        identity(&node["scale"], &[1., 1., 1.], "panel scale")?;
        identity(
            &node["matrix"],
            &[
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
            ],
            "panel matrix",
        )?;
        let mesh_id = integer(&node["mesh"], "mesh index")?;
        let mesh = g["meshes"].get(mesh_id).ok_or("missing panel mesh")?;
        if !mesh["weights"].is_null() {
            return Err("panel morph weights unsupported".into());
        }
        let primitives = mesh["primitives"]
            .as_array()
            .filter(|a| a.len() == 1)
            .ok_or("one panel primitive required")?;
        let primitive = &primitives[0];
        if optional_integer(&primitive["mode"], 4, "primitive mode")? != 4
            || !primitive["targets"].is_null()
            || !primitive["extensions"].is_null()
        {
            return Err("panel requires static uncompressed triangles".into());
        }
        let attrs = &primitive["attributes"];
        let positions = self.vectors(&attrs["POSITION"])?;
        let mut normals = self.vectors(&attrs["NORMAL"])?;
        if normals.len() != positions.len() {
            return Err("panel position/normal count mismatch".into());
        }
        for n in &mut normals {
            let length = n.iter().map(|c| c * c).sum::<f64>().sqrt();
            if !(0.98..=1.02).contains(&length) {
                return Err("invalid panel normal length".into());
            }
            for c in n {
                *c /= length;
            }
        }
        let indices = self.indices(&primitive["indices"])?;
        let mut vertices = Vec::with_capacity(indices.len());
        for index in indices {
            let position = *positions
                .get(index)
                .ok_or("panel index outside vertex array")?;
            vertices.push(Vertex {
                position,
                normal: normals[index],
            });
        }
        for triangle in vertices.chunks_exact(3) {
            let a = std::array::from_fn::<_, 3, _>(|i| {
                triangle[1].position[i] - triangle[0].position[i]
            });
            let b = std::array::from_fn::<_, 3, _>(|i| {
                triangle[2].position[i] - triangle[0].position[i]
            });
            let cross = [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ];
            let area = cross.iter().map(|c| c * c).sum::<f64>().sqrt();
            if area < 1e-12
                || triangle.iter().any(|v| {
                    cross.iter().zip(v.normal).map(|(a, b)| a * b).sum::<f64>() < 0.98 * area
                })
            {
                return Err("degenerate panel triangle or inconsistent flat normals".into());
            }
        }
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for v in &vertices {
            for i in 0..3 {
                min[i] = min[i].min(v.position[i]);
                max[i] = max[i].max(v.position[i]);
            }
        }
        let dimensions_m = std::array::from_fn(|i| max[i] - min[i]);
        let extras = &node["extras"];
        let width = scalar(&extras["nominal_width_m"], 0.01, 8., "nominal width")?;
        let height = scalar(&extras["nominal_height_m"], 0.01, 8., "nominal height")?;
        let safe_rectangle_m: [f64; 2] =
            vector(&extras["table_safe_rectangle_m"], "safe rectangle")?;
        if (width - dimensions_m[0]).abs() > 1e-4
            || (height - dimensions_m[1]).abs() > 1e-4
            || max[2].abs() > 1e-4
            || min[2] >= 0.
            || (min[0] + max[0]).abs() > 1e-4
            || (min[1] + max[1]).abs() > 1e-4
            || safe_rectangle_m[0] <= 0.
            || safe_rectangle_m[0] > width
            || safe_rectangle_m[1] <= 0.
            || safe_rectangle_m[1] > height
        {
            return Err("panel dimensions, origin or safe rectangle disagree with geometry".into());
        }
        let material_id = integer(&primitive["material"], "material index")?;
        let material = material(
            g["materials"]
                .get(material_id)
                .ok_or("missing panel material")?,
        )?;
        if (material.thickness_m - dimensions_m[2]).abs() > 1e-4 {
            return Err("panel material thickness disagrees with geometry".into());
        }
        Ok(PanelMesh {
            name: expected_name.into(),
            vertices,
            dimensions_m,
            safe_rectangle_m,
            material,
        })
    }
}

fn material(data: &Value) -> Result<Material, String> {
    let pbr = &data["pbrMetallicRoughness"];
    for texture in [
        &pbr["baseColorTexture"],
        &pbr["metallicRoughnessTexture"],
        &data["normalTexture"],
        &data["occlusionTexture"],
        &data["emissiveTexture"],
    ] {
        if !texture.is_null() {
            return Err("textured gem panel materials unsupported".into());
        }
    }
    if scalar(&pbr["metallicFactor"], 0., 1., "metallic factor")? != 0. {
        return Err("metallic gem panel materials unsupported".into());
    }
    let extensions = &data["extensions"];
    let transmission = &extensions["KHR_materials_transmission"];
    let volume = &extensions["KHR_materials_volume"];
    if !transmission["transmissionTexture"].is_null() || !volume["thicknessTexture"].is_null() {
        return Err("gem material transmission/thickness textures unsupported".into());
    }
    let tint_hex = data["extras"]["source_tint_rgba_hex"]
        .as_str()
        .filter(|s| s.len() == 8 && s.is_ascii())
        .ok_or("missing source ruby tint")?;
    let mut tint = [0.; 4];
    for (i, component) in tint.iter_mut().enumerate() {
        *component = u8::from_str_radix(&tint_hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| "invalid source tint")? as f64
            / 255.;
    }
    let attenuation_color: [f64; 3] = vector(&volume["attenuationColor"], "attenuation color")?;
    if attenuation_color.iter().any(|v| !(0.0..=1.0).contains(v)) {
        return Err("attenuation color outside unit range".into());
    }
    Ok(Material {
        tint,
        roughness: scalar(&pbr["roughnessFactor"], 0., 1., "roughness")?,
        ior: scalar(&extensions["KHR_materials_ior"]["ior"], 1., 3., "IOR")?,
        transmission: scalar(&transmission["transmissionFactor"], 0., 1., "transmission")?,
        attenuation_color,
        attenuation_distance_m: scalar(
            &volume["attenuationDistance"],
            1e-6,
            16.,
            "attenuation distance",
        )?,
        thickness_m: scalar(&volume["thicknessFactor"], 1e-6, 16., "thickness")?,
    })
}
