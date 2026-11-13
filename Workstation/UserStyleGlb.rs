//! A deliberately explicit static GLB profile. Unsupported data is rejected
//! with an authoring error instead of being silently omitted from the model.
use super::{
    MAX_MATERIALS, MAX_TRIANGLES, Material, RawMesh, Triangle, Vertex, boolean, number,
    read_bounded, vector,
};
use serde_json::Value;
use std::path::Path;

const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
const MAX_VERTICES: usize = MAX_TRIANGLES * 3;
const MAX_NODES: usize = 128;

pub(super) fn load(path: &Path, blender_axes: bool) -> Result<RawMesh, String> {
    let bytes = read_bounded(path, MAX_FILE_BYTES)?;
    decode(&bytes, blender_axes).map_err(|e| format!("{}: {e}", path.display()))
}

pub(super) fn decode(bytes: &[u8], blender_axes: bool) -> Result<RawMesh, String> {
    let glb = Glb::parse(bytes)?;
    glb.mesh(blender_axes)
}

fn integer(value: &Value, label: &str) -> Result<usize, String> {
    value
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| format!("invalid {label}"))
}
fn optional_integer(value: Option<&Value>, default: usize, label: &str) -> Result<usize, String> {
    value.map(|v| integer(v, label)).unwrap_or(Ok(default))
}
fn array<'a>(value: &'a Value, label: &str) -> Result<&'a [Value], String> {
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{label} must be an array"))
}
fn empty(value: Option<&Value>, label: &str) -> Result<(), String> {
    match value {
        None => Ok(()),
        Some(Value::Array(v)) if v.is_empty() => Ok(()),
        _ => Err(format!(
            "{label} is unsupported by the static panel profile"
        )),
    }
}
fn no_extensions(value: &Value, label: &str) -> Result<(), String> {
    match value.get("extensions") {
        None => Ok(()),
        Some(Value::Object(v)) if v.is_empty() => Ok(()),
        _ => Err(format!("{label} extensions are unsupported")),
    }
}
fn identity(value: Option<&Value>, expected: &[f64], label: &str) -> Result<(), String> {
    let Some(value) = value else {
        return Ok(());
    };
    let v = array(value, label)?;
    if v.len() == expected.len()
        && v.iter().zip(expected).all(|(v, expected)| {
            v.as_f64()
                .is_some_and(|v| v.is_finite() && (v - expected).abs() < 1e-8)
        })
    {
        Ok(())
    } else {
        Err(format!(
            "{label} must be identity; apply/bake object transforms before GLB export"
        ))
    }
}
fn u32le(bytes: &[u8], offset: usize) -> Result<usize, String> {
    let end = offset.checked_add(4).ok_or("GLB offset overflow")?;
    let data = bytes.get(offset..end).ok_or("truncated GLB word")?;
    Ok(u32::from_le_bytes(data.try_into().map_err(|_| "invalid GLB word")?) as usize)
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
    components: usize,
    component_type: usize,
    component_size: usize,
    normalized: bool,
}
impl Accessor<'_> {
    fn element(&self, index: usize, component: usize) -> &[u8] {
        let at = self.start + index * self.stride + component * self.component_size;
        &self.bytes[at..at + self.component_size]
    }
    fn index(&self, index: usize) -> usize {
        match self.component_type {
            5121 => self.element(index, 0)[0] as usize,
            5123 => u16::from_le_bytes(self.element(index, 0).try_into().unwrap()) as usize,
            5125 => u32::from_le_bytes(self.element(index, 0).try_into().unwrap()) as usize,
            _ => unreachable!("component type checked during accessor creation"),
        }
    }
    fn floats<const N: usize>(&self, index: usize, label: &str) -> Result<[f64; N], String> {
        if N != self.components {
            return Err("internal accessor shape mismatch".into());
        }
        let mut result = [0.; N];
        for (i, result) in result.iter_mut().enumerate() {
            *result = match self.component_type {
                5126 => f32::from_le_bytes(self.element(index, i).try_into().unwrap()) as f64,
                5121 if self.normalized => self.element(index, i)[0] as f64 / 255.,
                5123 if self.normalized => {
                    u16::from_le_bytes(self.element(index, i).try_into().unwrap()) as f64 / 65535.
                }
                _ => return Err(format!("unsupported {label} component type")),
            };
            if !result.is_finite() || result.abs() > 1e6 {
                return Err(format!("nonfinite or oversized {label}"));
            }
        }
        Ok(result)
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
        let mut at = 12usize;
        let mut document = None;
        let mut bin = None;
        while at < bytes.len() {
            let length = u32le(bytes, at)?;
            let kind = u32le(bytes, at + 4)?;
            at += 8;
            let end = at.checked_add(length).ok_or("GLB chunk length overflow")?;
            let chunk = bytes.get(at..end).ok_or("truncated GLB chunk")?;
            if length % 4 != 0 {
                return Err("GLB chunks must be aligned to four bytes".into());
            }
            match kind {
                0x4e4f534a if document.is_none() && bin.is_none() => {
                    document =
                        Some(serde_json::from_slice(chunk).map_err(|e| format!("GLB JSON: {e}"))?);
                }
                0x004e4942 if document.is_some() && bin.is_none() => bin = Some(chunk),
                _ => return Err("duplicate, reordered or unsupported GLB chunk".into()),
            }
            at = end;
        }
        let document: Value = document.ok_or("GLB requires a JSON chunk")?;
        let bin = bin.ok_or("GLB requires an embedded BIN chunk")?;
        if document["asset"]["version"] != "2.0" {
            return Err("GLB asset.version must be 2.0".into());
        }
        if document["asset"]
            .get("minVersion")
            .is_some_and(|v| v != "2.0")
        {
            return Err("GLB minVersion beyond 2.0 is unsupported".into());
        }
        for field in ["extensionsRequired", "extensionsUsed"] {
            if let Some(value) = document.get(field) {
                for extension in array(value, field)? {
                    if extension != "KHR_materials_unlit" {
                        return Err(format!(
                            "unsupported GLB extension {extension}; bake materials and geometry for the static profile"
                        ));
                    }
                }
            }
        }
        no_extensions(&document, "document")?;
        for field in [
            "animations",
            "skins",
            "cameras",
            "images",
            "textures",
            "samplers",
        ] {
            empty(document.get(field), field)?;
        }
        let buffers = array(&document["buffers"], "buffers")?;
        if buffers.len() != 1 || buffers[0].get("uri").is_some() {
            return Err(
                "panel GLB requires exactly one embedded buffer and no external/data URI".into(),
            );
        }
        no_extensions(&buffers[0], "buffer")?;
        let length = integer(&buffers[0]["byteLength"], "buffer length")?;
        if length == 0 || length > bin.len() || bin.len() - length > 3 {
            return Err("GLB BIN length disagrees with its embedded buffer".into());
        }
        Ok(Self {
            document,
            bin: &bin[..length],
        })
    }

    fn accessor(&self, id: &Value, shape: &str, limit: usize) -> Result<Accessor<'_>, String> {
        let id = integer(id, "accessor index")?;
        let accessors = array(&self.document["accessors"], "accessors")?;
        let accessor = accessors.get(id).ok_or("missing accessor")?;
        if accessor["type"] != shape || accessor.get("sparse").is_some() {
            return Err(format!("expected nonsparse {shape} accessor"));
        }
        no_extensions(accessor, "accessor")?;
        let component_type = integer(&accessor["componentType"], "accessor component type")?;
        let normalized = boolean(accessor.get("normalized"), false, "accessor normalized")?;
        let (components, component_size) = match (shape, component_type, normalized) {
            ("VEC3", 5126, false) => (3, 4),
            ("VEC2", 5126, false) => (2, 4),
            ("VEC2", 5121, true) => (2, 1),
            ("VEC2", 5123, true) => (2, 2),
            ("SCALAR", 5121, false) => (1, 1),
            ("SCALAR", 5123, false) => (1, 2),
            ("SCALAR", 5125, false) => (1, 4),
            _ => {
                return Err(format!(
                    "unsupported {shape} accessor encoding; use float positions/normals/UVs and unsigned indices"
                ));
            }
        };
        let count = integer(&accessor["count"], "accessor count")?;
        if count == 0 || count > limit {
            return Err(format!("accessor count exceeds 1..={limit}"));
        }
        let view_id = integer(&accessor["bufferView"], "buffer view index")?;
        let views = array(&self.document["bufferViews"], "bufferViews")?;
        let view = views.get(view_id).ok_or("missing buffer view")?;
        if integer(&view["buffer"], "buffer view buffer")? != 0 {
            return Err("only embedded buffer zero is supported".into());
        }
        no_extensions(view, "buffer view")?;
        let view_start = optional_integer(view.get("byteOffset"), 0, "buffer view offset")?;
        let view_length = integer(&view["byteLength"], "buffer view length")?;
        let view_end = view_start
            .checked_add(view_length)
            .ok_or("buffer view range overflow")?;
        if view_length == 0 || view_end > self.bin.len() {
            return Err("buffer view exceeds embedded BIN".into());
        }
        let offset = optional_integer(accessor.get("byteOffset"), 0, "accessor offset")?;
        let width = components * component_size;
        let stride = optional_integer(view.get("byteStride"), width, "buffer view stride")?;
        if stride < width
            || stride % component_size != 0
            || offset % component_size != 0
            || view_start % component_size != 0
            || (view.get("byteStride").is_some()
                && (!(4..=252).contains(&stride) || stride % 4 != 0))
        {
            return Err("invalid accessor stride/alignment".into());
        }
        let required = (count - 1)
            .checked_mul(stride)
            .and_then(|v| v.checked_add(width))
            .and_then(|v| v.checked_add(offset))
            .ok_or("accessor byte range overflow")?;
        if required > view_length {
            return Err("accessor exceeds its buffer view".into());
        }
        Ok(Accessor {
            bytes: self.bin,
            start: view_start + offset,
            stride,
            count,
            components,
            component_type,
            component_size,
            normalized,
        })
    }

    fn mesh(&self, blender_axes: bool) -> Result<RawMesh, String> {
        let doc = &self.document;
        let mut materials = self.materials()?;
        let authored_material_count = materials.len();
        let mut default_material = None;
        let mut triangles = Vec::new();
        let nodes = array(&doc["nodes"], "nodes")?;
        if nodes.is_empty() || nodes.len() > MAX_NODES {
            return Err(format!("panel requires 1..={MAX_NODES} nodes"));
        }
        let meshes = array(&doc["meshes"], "meshes")?;
        if meshes.is_empty() || meshes.len() > MAX_NODES {
            return Err("panel mesh count is out of bounds".into());
        }
        let scenes = array(&doc["scenes"], "scenes")?;
        if scenes.len() != 1 || optional_integer(doc.get("scene"), 0, "default scene")? != 0 {
            return Err("export exactly one active GLB scene".into());
        }
        no_extensions(&scenes[0], "scene")?;
        let roots = array(&scenes[0]["nodes"], "scene nodes")?;
        let mut seen = vec![false; nodes.len()];
        let mut used_meshes = vec![false; meshes.len()];
        let mut mesh_ids = Vec::new();
        for root in roots {
            self.visit_node(
                integer(root, "scene root")?,
                nodes,
                &mut seen,
                &mut mesh_ids,
            )?;
        }
        if seen.iter().any(|v| !v) {
            return Err("export only nodes belonging to the one panel scene".into());
        }
        for mesh_id in mesh_ids {
            let mesh = meshes.get(mesh_id).ok_or("node references missing mesh")?;
            used_meshes[mesh_id] = true;
            no_extensions(mesh, "mesh")?;
            empty(mesh.get("weights"), "mesh morph weights")?;
            let primitives = array(&mesh["primitives"], "mesh primitives")?;
            if primitives.is_empty() || primitives.len() > MAX_MATERIALS * 4 {
                return Err("mesh primitive count is out of bounds".into());
            }
            for primitive in primitives {
                no_extensions(primitive, "primitive")?;
                empty(primitive.get("targets"), "morph targets")?;
                if optional_integer(primitive.get("mode"), 4, "primitive mode")? != 4 {
                    return Err("panel primitives must be TRIANGLES".into());
                }
                let attributes = primitive["attributes"]
                    .as_object()
                    .ok_or("primitive requires attributes")?;
                if let Some(key) = attributes
                    .keys()
                    .find(|k| !["POSITION", "NORMAL", "TEXCOORD_0"].contains(&k.as_str()))
                {
                    return Err(format!(
                        "unsupported vertex attribute {key}; bake it into the supported material or WGSL"
                    ));
                }
                let position =
                    self.accessor(&primitive["attributes"]["POSITION"], "VEC3", MAX_VERTICES)?;
                let normal = attributes
                    .get("NORMAL")
                    .map(|v| self.accessor(v, "VEC3", MAX_VERTICES))
                    .transpose()?;
                let uv = attributes
                    .get("TEXCOORD_0")
                    .map(|v| self.accessor(v, "VEC2", MAX_VERTICES))
                    .transpose()?;
                if normal.as_ref().is_some_and(|a| a.count != position.count)
                    || uv.as_ref().is_some_and(|a| a.count != position.count)
                {
                    return Err("primitive attribute counts must agree".into());
                }
                let indices = primitive
                    .get("indices")
                    .map(|v| self.accessor(v, "SCALAR", MAX_TRIANGLES * 3))
                    .transpose()?;
                let count = indices.as_ref().map(|v| v.count).unwrap_or(position.count);
                if count % 3 != 0 || triangles.len() + count / 3 > MAX_TRIANGLES {
                    return Err(format!(
                        "triangle count is invalid or exceeds {MAX_TRIANGLES}"
                    ));
                }
                let material = match primitive.get("material") {
                    Some(v) => {
                        let index = integer(v, "material index")?;
                        if index >= authored_material_count {
                            return Err("primitive references missing material".into());
                        }
                        index
                    }
                    None => match default_material {
                        Some(index) => index,
                        None => {
                            if materials.len() >= MAX_MATERIALS {
                                return Err("model needs a default material in addition to its 64 authored materials; assign every primitive a material".into());
                            }
                            let index = materials.len();
                            materials.push(Material {
                                base: [1.; 3],
                                roughness: 1.,
                                emission: [0.; 3],
                                metallic: 1.,
                                unlit: false,
                                double_sided: false,
                            });
                            default_material = Some(index);
                            index
                        }
                    },
                };
                for at in (0..count).step_by(3) {
                    let mut vertices = [Vertex {
                        position: [0.; 3],
                        normal: [0.; 3],
                        uv: [0.; 2],
                    }; 3];
                    for (corner, vertex) in vertices.iter_mut().enumerate() {
                        let index = indices
                            .as_ref()
                            .map(|v| v.index(at + corner))
                            .unwrap_or(at + corner);
                        if index >= position.count {
                            return Err("triangle index exceeds vertex count".into());
                        }
                        let p = position.floats::<3>(index, "position")?;
                        if p.iter().any(|v| v.abs() > 1000.) {
                            return Err("panel positions must stay within ±1000 metres".into());
                        }
                        vertex.position = axes(p, blender_axes);
                        if let Some(normal) = &normal {
                            let n = axes(normal.floats::<3>(index, "normal")?, blender_axes);
                            let length = dot(n, n).sqrt();
                            if !(0.98..=1.02).contains(&length) {
                                return Err("authored normals must have unit length".into());
                            }
                            vertex.normal = n.map(|v| v / length);
                        }
                        if let Some(uv) = &uv {
                            vertex.uv = uv.floats::<2>(index, "UV")?;
                        }
                    }
                    let ab =
                        std::array::from_fn(|i| vertices[1].position[i] - vertices[0].position[i]);
                    let ac =
                        std::array::from_fn(|i| vertices[2].position[i] - vertices[0].position[i]);
                    let face = cross(ab, ac);
                    let length = dot(face, face).sqrt();
                    if !length.is_finite() || length < 1e-12 {
                        return Err("degenerate triangle; clean the mesh before export".into());
                    }
                    if normal.is_none() {
                        for vertex in &mut vertices {
                            vertex.normal = face.map(|v| v / length);
                        }
                    }
                    triangles.push(Triangle { vertices, material });
                }
            }
        }
        if used_meshes.iter().any(|v| !v) {
            return Err("export only meshes used by the active panel scene".into());
        }
        if triangles.is_empty() {
            return Err("panel scene contains no triangles".into());
        }
        Ok(RawMesh {
            triangles,
            materials,
        })
    }

    fn visit_node(
        &self,
        id: usize,
        nodes: &[Value],
        seen: &mut [bool],
        mesh_ids: &mut Vec<usize>,
    ) -> Result<(), String> {
        let node = nodes.get(id).ok_or("scene references missing node")?;
        if seen[id] {
            return Err("GLB node graph contains a cycle or multiple parents".into());
        }
        seen[id] = true;
        no_extensions(node, "node")?;
        if node.get("skin").is_some() || node.get("camera").is_some() {
            return Err("skin/camera nodes are unsupported".into());
        }
        empty(node.get("weights"), "node morph weights")?;
        identity(node.get("translation"), &[0., 0., 0.], "node translation")?;
        identity(node.get("scale"), &[1., 1., 1.], "node scale")?;
        identity(node.get("rotation"), &[0., 0., 0., 1.], "node rotation")?;
        identity(
            node.get("matrix"),
            &[
                1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
            ],
            "node matrix",
        )?;
        if node.get("matrix").is_some()
            && ["translation", "rotation", "scale"]
                .iter()
                .any(|k| node.get(*k).is_some())
        {
            return Err("GLB node cannot combine matrix and TRS transforms".into());
        }
        if let Some(mesh) = node.get("mesh") {
            mesh_ids.push(integer(mesh, "node mesh")?);
        }
        if let Some(children) = node.get("children") {
            for child in array(children, "node children")? {
                self.visit_node(integer(child, "child node")?, nodes, seen, mesh_ids)?;
            }
        }
        Ok(())
    }

    fn materials(&self) -> Result<Vec<Material>, String> {
        let values = match self.document.get("materials") {
            Some(value) => array(value, "materials")?,
            None => &[],
        };
        if values.len() > MAX_MATERIALS {
            return Err(format!("at most {MAX_MATERIALS} materials are supported"));
        }
        let mut materials = Vec::new();
        for value in values {
            let object = value.as_object().ok_or("material must be an object")?;
            for key in ["normalTexture", "occlusionTexture", "emissiveTexture"] {
                if object.contains_key(key) {
                    return Err(format!(
                        "{key} is unsupported; use procedural WGSL or scalar factors"
                    ));
                }
            }
            let alpha = value
                .get("alphaMode")
                .and_then(Value::as_str)
                .unwrap_or("OPAQUE");
            if alpha != "OPAQUE"
                || value.get("alphaMode").is_some_and(|v| !v.is_string())
                || value.get("alphaCutoff").is_some()
            {
                return Err(
                    "glTF materials must be OPAQUE; custom WGSL may supply a binary alpha cutout"
                        .into(),
                );
            }
            let mut unlit = false;
            if let Some(extensions) = value.get("extensions") {
                let extensions = extensions
                    .as_object()
                    .ok_or("material extensions must be an object")?;
                for (key, extension) in extensions {
                    if key != "KHR_materials_unlit"
                        || !extension.as_object().is_some_and(|v| v.is_empty())
                    {
                        return Err(format!("unsupported material extension {key}"));
                    }
                    unlit = true;
                }
            }
            let pbr = value.get("pbrMetallicRoughness");
            if let Some(pbr) = pbr {
                let pbr = pbr
                    .as_object()
                    .ok_or("pbrMetallicRoughness must be an object")?;
                for key in pbr.keys() {
                    if ![
                        "baseColorFactor",
                        "metallicFactor",
                        "roughnessFactor",
                        "extras",
                    ]
                    .contains(&key.as_str())
                    {
                        return Err(format!(
                            "unsupported PBR field {key}; use factors or procedural WGSL"
                        ));
                    }
                }
            }
            let factor = |key: &str, default: f64| -> Result<f64, String> {
                pbr.and_then(|v| v.get(key))
                    .map(|v| number(v, 0., 1., key))
                    .unwrap_or(Ok(default))
            };
            let color = pbr
                .and_then(|v| v.get("baseColorFactor"))
                .map(|v| vector::<4>(v, 0., 1., "baseColorFactor"))
                .transpose()?
                .unwrap_or([1.; 4]);
            if color[3] != 1. {
                return Err("baseColorFactor alpha must be 1 for the opaque profile".into());
            }
            materials.push(Material {
                base: [color[0], color[1], color[2]],
                roughness: factor("roughnessFactor", 1.)?,
                metallic: factor("metallicFactor", 1.)?,
                emission: value
                    .get("emissiveFactor")
                    .map(|v| vector::<3>(v, 0., 1., "emissiveFactor"))
                    .transpose()?
                    .unwrap_or([0.; 3]),
                unlit,
                double_sided: boolean(value.get("doubleSided"), false, "material doubleSided")?,
            });
        }
        Ok(materials)
    }
}

fn axes(v: [f64; 3], blender_axes: bool) -> [f64; 3] {
    if blender_axes { [v[0], -v[2], v[1]] } else { v }
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
