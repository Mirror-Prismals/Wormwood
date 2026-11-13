//! Control-thread user panel packages. Geometry is immutable; Origami updates
//! only four instance poses. None of these operations belong in audio callbacks.
use super::{
    panel_layout::{self, Options, Rect, Viewport},
    panel_motion::Snapshot,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

#[path = "UserStyleGlb.rs"]
mod glb;
#[cfg(test)]
#[path = "UserStyleTests.rs"]
mod tests;

const ENGRAVING_END: usize = super::control_engraving::BASE + super::control_engraving::ROWS;
const PLUG_END: usize = super::edison_demo::PLUG_BASE + super::edison_demo::PLUG_ROWS;
pub(crate) const BASE: usize = ((if ENGRAVING_END > PLUG_END {
    ENGRAVING_END
} else {
    PLUG_END
}) + 255)
    & !255;
pub(crate) const ROWS: usize = 65536;
pub(crate) const HEADER: usize = 245;
const MAX_MANIFEST_BYTES: usize = 65536;
const MAX_SHADER_BYTES: usize = 65536;
const MAX_MODELS: usize = 16;
const MAX_TRIANGLES: usize = 6000;
const MAX_MATERIALS: usize = 64;
const PANEL_NAMES: [&str; 4] = ["top", "bottom", "left", "right"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Resize {
    Contain,
    Stretch,
    Snap,
}

#[derive(Clone, Copy)]
struct Vertex {
    position: [f64; 3],
    normal: [f64; 3],
    uv: [f64; 2],
}
#[derive(Clone)]
struct Triangle {
    vertices: [Vertex; 3],
    material: usize,
}
#[derive(Clone)]
struct Material {
    base: [f64; 3],
    roughness: f64,
    emission: [f64; 3],
    metallic: f64,
    unlit: bool,
    double_sided: bool,
}
struct RawMesh {
    triangles: Vec<Triangle>,
    materials: Vec<Material>,
}

struct Model {
    triangles: Vec<Triangle>,
    min: [f64; 3],
    max: [f64; 3],
    root: usize,
    end: usize,
    materials: usize,
    material_count: usize,
    resize: Resize,
    content: [f64; 4],
    content_z: f64,
    shader: bool,
}

/// The host-owned content rectangle transformed with the actual user model.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Surface {
    pub origin: [f64; 3],
    pub size: [f64; 2],
    pub scale: [f64; 3],
    /// Complete projected mesh bounds, in logical viewport pixels.
    pub visual_rect: Rect,
}

pub(crate) struct Kit {
    pub id: String,
    pub name: String,
    pub shader_source: Option<String>,
    pub animated: bool,
    pub background: bool,
    models: Vec<Model>,
    panels: [Option<usize>; 4],
    packet: Vec<[f32; 4]>,
    panel_shader: bool,
}

type CachedSelection = (Option<OsString>, Result<Option<Arc<Kit>>, String>);
#[derive(Default)]
struct Selection {
    explicit: Option<Arc<Kit>>,
    environment: Option<CachedSelection>,
}
fn selection() -> &'static Mutex<Selection> {
    static SELECTION: OnceLock<Mutex<Selection>> = OnceLock::new();
    SELECTION.get_or_init(|| Mutex::new(Selection::default()))
}

/// Prepare the complete package before publishing it. Failed imports preserve
/// the previous explicit selection. This never mutates process environment.
pub(crate) fn select_path(path: &Path) -> Result<(), String> {
    let kit = Arc::new(Kit::load(path)?);
    selection()
        .lock()
        .map_err(|_| "user style selection cache was poisoned")?
        .explicit = Some(kit);
    Ok(())
}

/// Selection is explicit and cached for the process. Changing WORMWOOD_STYLE
/// selects another package; editing a selected package requires a restart.
/// Errors are cached and returned, never silently replaced by built-in gems.
pub(crate) fn selected() -> Result<Option<Arc<Kit>>, String> {
    let choice = std::env::var_os("WORMWOOD_STYLE").filter(|v| !v.is_empty());
    let mut cache = selection()
        .lock()
        .map_err(|_| "user style selection cache was poisoned")?;
    if let Some(kit) = &cache.explicit {
        return Ok(Some(kit.clone()));
    }
    if let Some((old, value)) = cache.environment.as_ref() {
        if old == &choice {
            return value.clone();
        }
    }
    let value = choice
        .as_ref()
        .map(|path| Kit::load(Path::new(path)).map(Arc::new))
        .transpose();
    cache.environment = Some((choice, value.clone()));
    value
}

impl Kit {
    /// Accept a package directory or its style.json. All subordinate assets
    /// must resolve to ordinary files inside that same canonical directory.
    pub(crate) fn load(path: &Path) -> Result<Self, String> {
        let requested = if path.is_dir() {
            path.join("style.json")
        } else {
            path.to_owned()
        };
        let parent = requested
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let root = parent
            .canonicalize()
            .map_err(|e| format!("{}: {e}", parent.display()))?;
        let manifest = requested
            .canonicalize()
            .map_err(|e| format!("{}: {e}", requested.display()))?;
        if !manifest.starts_with(&root) {
            return Err("style manifest resolves outside its package directory".into());
        }
        let bytes = read_bounded(&manifest, MAX_MANIFEST_BYTES)?;
        let doc: Value =
            serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", manifest.display()))?;
        allowed(
            &doc,
            &[
                "format", "version", "id", "name", "models", "panels", "shader",
            ],
            "style manifest",
        )?;
        if doc["format"] != "mirror.workstation.style" || doc["version"] != 1 {
            return Err("style requires format mirror.workstation.style and version 1".into());
        }
        let id = text(&doc["id"], "style id", 128)?.to_owned();
        if !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(
                "style id accepts only ASCII letters, digits, period, underscore and hyphen".into(),
            );
        }
        let name = text(&doc["name"], "style name", 256)?.to_owned();
        let (mut shader_source, mut animated, mut background, mut panel_shader) =
            (None, false, false, false);
        if let Some(shader) = doc.get("shader") {
            allowed(
                shader,
                &["file", "api", "animated", "panel", "background"],
                "style shader",
            )?;
            if shader["api"] != 1 {
                return Err("style shader api must be 1".into());
            }
            let path = package_file(&root, text(&shader["file"], "shader file", 1024)?)?;
            if path.extension().and_then(|v| v.to_str()) != Some("wgsl") {
                return Err("style shader must be a .wgsl file".into());
            }
            let source = String::from_utf8(read_bounded(&path, MAX_SHADER_BYTES)?)
                .map_err(|_| "style WGSL must be UTF-8")?;
            if source.trim().is_empty() {
                return Err("style WGSL is empty".into());
            }
            super::style_shader::validate(&source)?;
            shader_source = Some(source);
            animated = boolean(shader.get("animated"), false, "shader animated")?;
            background = boolean(shader.get("background"), false, "shader background")?;
            panel_shader = boolean(shader.get("panel"), true, "shader panel")?;
            if !panel_shader && !background {
                return Err("shader must enable panel or background".into());
            }
        }
        let declarations = doc["models"]
            .as_object()
            .ok_or("style models must be an object")?;
        if declarations.len() > MAX_MODELS {
            return Err(format!("style exceeds {MAX_MODELS} models"));
        }
        let mut names = BTreeMap::new();
        let mut models = Vec::new();
        let mut packet = Vec::new();
        let mut total_triangles = 0usize;
        for (key, declaration) in declarations {
            if key.is_empty() || key.len() > 128 {
                return Err("model id must be 1..128 bytes".into());
            }
            allowed(
                declaration,
                &[
                    "file",
                    "axes",
                    "resize",
                    "content_rect_m",
                    "content_z_m",
                    "shader",
                ],
                &format!("model {key}"),
            )?;
            let file = package_file(&root, text(&declaration["file"], "model file", 1024)?)?;
            if file.extension().and_then(|v| v.to_str()) != Some("glb") {
                return Err(format!("model {key}: expected a self-contained .glb file"));
            }
            let blender_axes = match declaration
                .get("axes")
                .and_then(Value::as_str)
                .unwrap_or("blender-gltf")
            {
                "blender-gltf" => true,
                "panel" => false,
                _ => return Err(format!("model {key}: axes must be blender-gltf or panel")),
            };
            // Invalid JSON types must not turn into defaults.
            for field in ["axes", "resize"] {
                if declaration.get(field).is_some_and(|v| !v.is_string()) {
                    return Err(format!("model {key}: {field} must be a string"));
                }
            }
            let resize = match declaration
                .get("resize")
                .and_then(Value::as_str)
                .unwrap_or("stretch")
            {
                "contain" => Resize::Contain,
                "stretch" => Resize::Stretch,
                "snap" => Resize::Snap,
                _ => {
                    return Err(format!(
                        "model {key}: resize must be contain, stretch or snap"
                    ));
                }
            };
            let content = vector::<4>(
                &declaration["content_rect_m"],
                -1000.,
                1000.,
                "content rectangle",
            )?;
            let content_z = declaration
                .get("content_z_m")
                .map(|v| number(v, -1000., 1000., "content Z"))
                .transpose()?
                .unwrap_or(0.);
            let shader = boolean(declaration.get("shader"), true, "model shader")?;
            let raw = glb::load(&file, blender_axes).map_err(|e| format!("model {key}: {e}"))?;
            total_triangles += raw.triangles.len();
            if total_triangles > MAX_TRIANGLES {
                return Err(format!(
                    "style exceeds {MAX_TRIANGLES} total model triangles"
                ));
            }
            let mut model = Model::pack(raw, &mut packet, resize, content, content_z, shader)
                .map_err(|e| format!("model {key}: {e}"))?;
            // CPU hits use the exact triangle list after BVH ordering.
            model.triangles.shrink_to_fit();
            names.insert(key.clone(), models.len());
            models.push(model);
        }
        let mappings = doc["panels"]
            .as_object()
            .ok_or("style panels must be an object")?;
        let mut panels = [None; 4];
        for (key, value) in mappings {
            let index = PANEL_NAMES
                .iter()
                .position(|name| *name == key.as_str())
                .ok_or_else(|| {
                    format!("unknown panel role {key}; expected top, bottom, left or right")
                })?;
            allowed(value, &["model", "builtin"], "panel mapping")?;
            match (value.get("model"), value.get("builtin")) {
                (Some(model), None) => {
                    let name = text(model, "panel model", 128)?;
                    panels[index] = Some(
                        *names
                            .get(name)
                            .ok_or_else(|| format!("unknown panel model {name}"))?,
                    );
                }
                (None, Some(builtin)) if builtin == "gem" => {}
                _ => {
                    return Err(format!(
                        "panel {key} must select exactly one model or builtin gem"
                    ));
                }
            }
        }
        if packet.len() > ROWS {
            return Err("user style exceeds geometry row budget".into());
        }
        Ok(Self {
            id,
            name,
            shader_source,
            animated,
            background,
            models,
            panels,
            packet,
            panel_shader,
        })
    }

    pub(crate) fn write_geometry(&self, rows: &mut [[f32; 4]]) -> Result<(), String> {
        let target = rows
            .get_mut(BASE..BASE + ROWS)
            .ok_or("user style geometry buffer is too short")?;
        target.fill([0.; 4]);
        target[..self.packet.len()].copy_from_slice(&self.packet);
        Ok(())
    }

    pub(crate) fn apply(
        &self,
        rows: &mut [[f32; 4]],
        view: Viewport,
        motion: &Snapshot,
        options: Options,
        time: f64,
    ) -> Result<(), String> {
        if rows.len() <= HEADER + 1 || !time.is_finite() || time < 0. || time > f32::MAX as f64 {
            return Err("invalid user style frame size or elapsed time".into());
        }
        rows[HEADER] = [
            1.,
            u8::from(self.animated) as f32,
            u8::from(self.background) as f32,
            if self.animated { time as f32 } else { 0. },
        ];
        rows[HEADER + 1] = [BASE as f32, self.packet.len() as f32, 0., 0.];
        for (index, mapping) in self.panels.iter().enumerate() {
            let Some(model_index) = mapping else {
                continue;
            };
            let model = &self.models[*model_index];
            let pose = self
                .pose(index, view, motion, options)?
                .ok_or("missing custom panel pose")?;
            let at = 32 + index * 40;
            rows[at..at + 40].fill([0.; 4]);
            rows[at] = [
                0.,
                model.root as f32,
                model.end as f32,
                model.materials as f32,
            ];
            rows[at + 1] = row(pose.translation, 1.);
            rows[at + 2] = row(
                pose.scale,
                u8::from(model.shader && self.panel_shader) as f64,
            );
            rows[at + 3] = row(model.min, 0.);
            rows[at + 4] = row(model.max, 0.);
            rows[at + 5] = model.content.map(|v| v as f32);
            rows[at + 6] = [
                model.triangles.len() as f32,
                model.material_count as f32,
                model.content_z as f32,
                0.,
            ];
            rows[5][3] =
                rows[5][3].min((pose.translation[2] + model.min[2] * pose.scale[2] - 0.02) as f32);
        }
        Ok(())
    }

    pub(crate) fn surface(
        &self,
        index: usize,
        view: Viewport,
        motion: &Snapshot,
        options: Options,
    ) -> Result<Option<Surface>, String> {
        let Some(pose) = self.pose(index, view, motion, options)? else {
            return Ok(None);
        };
        let model = &self.models[self.panels[index].ok_or("missing panel model")?];
        let content = model.content;
        Ok(Some(Surface {
            origin: [
                pose.translation[0] + (content[0] + content[2]) * 0.5 * pose.scale[0],
                pose.translation[1] + (content[1] + content[3]) * 0.5 * pose.scale[1],
                pose.translation[2] + model.content_z * pose.scale[2],
            ],
            size: [
                (content[2] - content[0]) * pose.scale[0],
                (content[3] - content[1]) * pose.scale[1],
            ],
            scale: pose.scale,
            visual_rect: pose.visual_rect,
        }))
    }

    /// Geometry-accurate orthographic coverage. Shader cutouts are deliberately
    /// not evaluated on the CPU; authors must keep functional content opaque.
    pub(crate) fn hit_panel_point(
        &self,
        index: usize,
        view: Viewport,
        motion: &Snapshot,
        options: Options,
        point: [f64; 2],
    ) -> Result<bool, String> {
        if point.iter().any(|v| !v.is_finite()) {
            return Err("invalid user panel hit point".into());
        }
        let Some(pose) = self.pose(index, view, motion, options)? else {
            return Ok(false);
        };
        let model = &self.models[self.panels[index].ok_or("missing panel model")?];
        let mpp = 1.3 / view.height;
        let local = [
            ((point[0] - view.width * 0.5) * mpp - pose.translation[0]) / pose.scale[0],
            ((view.height * 0.5 - point[1]) * mpp - pose.translation[1]) / pose.scale[1],
        ];
        if local[0] < model.min[0]
            || local[0] > model.max[0]
            || local[1] < model.min[1]
            || local[1] > model.max[1]
        {
            return Ok(false);
        }
        Ok(model.triangles.iter().any(|t| contains_xy(t, local)))
    }

    fn pose(
        &self,
        index: usize,
        view: Viewport,
        motion: &Snapshot,
        options: Options,
    ) -> Result<Option<Pose>, String> {
        let mapping = self.panels.get(index).ok_or("invalid user panel index")?;
        let Some(model_index) = mapping else {
            return Ok(None);
        };
        let model = &self.models[*model_index];
        // Validate the original snapshot before quantizing it.
        let mut rect = panel_layout::layout(view, motion, options)?[index];
        if model.resize == Resize::Snap {
            let mut target = *motion;
            target.extensions = target.open.map(|open| if open { 1. } else { 0. });
            target.left_width_scale = if target.left_width_scale >= 1.25 {
                1.5
            } else {
                1.
            };
            target.bottom_height_scale = if target.bottom_height_scale >= 1.25 {
                1.5
            } else {
                1.
            };
            target.overview_hide = target.overview_hide.map(|v| if v >= 0.5 { 1. } else { 0. });
            for i in 0..4 {
                if !target.open[i] {
                    target.opening_order[i] = 0;
                }
            }
            rect = panel_layout::layout(view, &target, options)?[index];
        }
        let mpp = 1.3 / view.height;
        let sx = rect.width * mpp / (model.max[0] - model.min[0]);
        let sy = rect.height * mpp / (model.max[1] - model.min[1]);
        let uniform = sx.min(sy);
        let scale = if model.resize == Resize::Contain {
            [uniform; 3]
        } else {
            [sx, sy, uniform]
        };
        let center = rect.center();
        let translation = [
            (center[0] - view.width * 0.5) * mpp - (model.min[0] + model.max[0]) * 0.5 * scale[0],
            (view.height * 0.5 - center[1]) * mpp - (model.min[1] + model.max[1]) * 0.5 * scale[1],
            -model.content_z * scale[2],
        ];
        let width = (model.max[0] - model.min[0]) * scale[0] / mpp;
        let height = (model.max[1] - model.min[1]) * scale[1] / mpp;
        Ok(Some(Pose {
            translation,
            scale,
            visual_rect: Rect {
                x: center[0] - width * 0.5,
                y: center[1] - height * 0.5,
                width,
                height,
            },
        }))
    }
}

struct Pose {
    translation: [f64; 3],
    scale: [f64; 3],
    visual_rect: Rect,
}
struct Node {
    min: [f64; 3],
    max: [f64; 3],
    escape: usize,
    first: usize,
    count: usize,
}

impl Model {
    fn pack(
        raw: RawMesh,
        packet: &mut Vec<[f32; 4]>,
        resize: Resize,
        content: [f64; 4],
        content_z: f64,
        shader: bool,
    ) -> Result<Self, String> {
        let RawMesh {
            mut triangles,
            materials,
        } = raw;
        if triangles.is_empty() || materials.is_empty() || materials.len() > MAX_MATERIALS {
            return Err("model requires triangles and at most 64 materials".into());
        }
        let (min, max) = bounds(&triangles);
        if (0..2).any(|a| max[a] - min[a] < 1e-6) {
            return Err("panel must have nonzero projected width and height".into());
        }
        if content[2] - content[0] < 1e-6
            || content[3] - content[1] < 1e-6
            || content[0] < min[0] - 1e-6
            || content[1] < min[1] - 1e-6
            || content[2] > max[0] + 1e-6
            || content[3] > max[1] + 1e-6
        {
            return Err(
                "content rectangle must have positive area inside the model's projected bounds"
                    .into(),
            );
        }
        if content_z < min[2] - 1e-5 || content_z > max[2] + 1e-5 {
            return Err(
                "content_z_m must lie within the model depth bounds (a zero-depth plane is valid)"
                    .into(),
            );
        }
        let mut nodes = Vec::new();
        build_bvh(&mut triangles, 0, &mut nodes);
        let needed = materials.len() * 2 + nodes.len() * 3 + triangles.len() * 8;
        if packet.len() + needed > ROWS {
            return Err("style exceeds its 65,536-row geometry budget".into());
        }
        let material_base = BASE + packet.len();
        for material in &materials {
            packet.push(row(material.base, material.roughness));
            packet.push(row(material.emission, material.metallic));
        }
        let root = BASE + packet.len();
        let end = root + nodes.len() * 3;
        for node in &nodes {
            // Pad even a single polygon's depth for a robust ray/AABB interval.
            packet.push(row(
                node.min.map(|v| v - 1e-6),
                (root + node.escape * 3) as f64,
            ));
            packet.push(row(
                node.max.map(|v| v + 1e-6),
                (end + node.first * 8) as f64,
            ));
            packet.push([node.count as f32, 0., 0., 0.]);
        }
        for triangle in &triangles {
            let material = &materials[triangle.material];
            let v = triangle.vertices;
            packet.push(row(v[0].position, triangle.material as f64));
            packet.push(row(v[1].position, u8::from(material.unlit) as f64));
            packet.push(row(v[2].position, u8::from(material.double_sided) as f64));
            for vertex in v {
                packet.push(row(vertex.normal, 0.));
            }
            packet.push([
                v[0].uv[0] as f32,
                v[0].uv[1] as f32,
                v[1].uv[0] as f32,
                v[1].uv[1] as f32,
            ]);
            packet.push([v[2].uv[0] as f32, v[2].uv[1] as f32, 0., 0.]);
        }
        Ok(Self {
            triangles,
            min,
            max,
            root,
            end,
            materials: material_base,
            material_count: materials.len(),
            resize,
            content,
            content_z,
            shader,
        })
    }
}

fn build_bvh(triangles: &mut [Triangle], first: usize, nodes: &mut Vec<Node>) {
    let (min, max) = bounds(triangles);
    let at = nodes.len();
    nodes.push(Node {
        min,
        max,
        escape: 0,
        first,
        count: triangles.len(),
    });
    if triangles.len() > 4 {
        let axis = (0..3)
            .max_by(|a, b| (max[*a] - min[*a]).total_cmp(&(max[*b] - min[*b])))
            .unwrap_or(0);
        triangles.sort_unstable_by(|a, b| {
            let sum = |t: &Triangle| t.vertices.iter().map(|v| v.position[axis]).sum::<f64>();
            sum(a).total_cmp(&sum(b))
        });
        let middle = triangles.len() / 2;
        let (left, right) = triangles.split_at_mut(middle);
        nodes[at].count = 0;
        build_bvh(left, first, nodes);
        build_bvh(right, first + middle, nodes);
    }
    nodes[at].escape = nodes.len();
}

fn bounds(triangles: &[Triangle]) -> ([f64; 3], [f64; 3]) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for t in triangles {
        for v in &t.vertices {
            for i in 0..3 {
                min[i] = min[i].min(v.position[i]);
                max[i] = max[i].max(v.position[i]);
            }
        }
    }
    (min, max)
}
fn contains_xy(triangle: &Triangle, p: [f64; 2]) -> bool {
    let a = triangle.vertices[0].position;
    let b = triangle.vertices[1].position;
    let c = triangle.vertices[2].position;
    let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    if area.abs() < 1e-14 {
        return false;
    }
    let u = ((p[0] - a[0]) * (c[1] - a[1]) - (p[1] - a[1]) * (c[0] - a[0])) / area;
    let v = ((b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])) / area;
    u >= -1e-8 && v >= -1e-8 && u + v <= 1. + 1e-8
}
fn row(v: [f64; 3], w: f64) -> [f32; 4] {
    [v[0] as f32, v[1] as f32, v[2] as f32, w as f32]
}
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    if !std::fs::metadata(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .is_file()
    {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let file = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    let mut bytes = Vec::new();
    file.take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() > limit {
        return Err(format!(
            "{} exceeds its {limit}-byte import limit",
            path.display()
        ));
    }
    Ok(bytes)
}
fn package_file(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path
        .components()
        .any(|v| !matches!(v, Component::Normal(_)))
    {
        return Err(format!(
            "package asset path must be relative without parent traversal: {relative}"
        ));
    }
    let resolved = root
        .join(path)
        .canonicalize()
        .map_err(|e| format!("asset {relative}: {e}"))?;
    if !resolved.starts_with(root) {
        return Err(format!(
            "asset {relative} resolves outside the style package"
        ));
    }
    Ok(resolved)
}
fn allowed(value: &Value, keys: &[&str], label: &str) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))?;
    if let Some(key) = object.keys().find(|key| !keys.contains(&key.as_str())) {
        return Err(format!("unsupported {label} field {key}"));
    }
    Ok(())
}
fn text<'a>(value: &'a Value, label: &str, limit: usize) -> Result<&'a str, String> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= limit && !s.contains('\0'))
        .ok_or_else(|| format!("{label} must contain 1..{limit} UTF-8 bytes"))
}
fn number(value: &Value, min: f64, max: f64, label: &str) -> Result<f64, String> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && *v >= min && *v <= max)
        .ok_or_else(|| format!("{label} must be finite within {min}..={max}"))
}
fn vector<const N: usize>(
    value: &Value,
    min: f64,
    max: f64,
    label: &str,
) -> Result<[f64; N], String> {
    let values = value
        .as_array()
        .filter(|v| v.len() == N)
        .ok_or_else(|| format!("{label} requires {N} numbers"))?;
    let mut result = [0.; N];
    for (out, value) in result.iter_mut().zip(values) {
        *out = number(value, min, max, label)?;
    }
    Ok(result)
}
fn boolean(value: Option<&Value>, default: bool, label: &str) -> Result<bool, String> {
    value
        .map(|v| {
            v.as_bool()
                .ok_or_else(|| format!("{label} must be boolean"))
        })
        .unwrap_or(Ok(default))
}
