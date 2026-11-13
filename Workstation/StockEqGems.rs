//! Original stock EQ's five gem surfaces, using the approved panel cut and BSDF.
//! One composite control instance per insert keeps the existing renderer ABI.
use super::{panel_layout, stock_eq};
use serde_json::Value;
use std::{path::Path, sync::Arc};

// The connected Edison reserves 262144 rows but currently uses 71049. Keep
// every authored address and the total buffer size unchanged; validate overlap.
pub(crate) const BASE: usize = super::edison_demo::PLUG_BASE + 131072;
const CAPACITY: usize = 131072;
const PARTS: usize = 5;
const VARIANTS: usize = 16;
#[derive(Clone)]
struct Triangle {
    v: [[f64; 3]; 3],
    n: [[f64; 3]; 3],
    material: usize,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Variant {
    pub base: usize,
    pub root: usize,
    pub end: usize,
    pub min: [f64; 3],
    pub max: [f64; 3],
}
pub(crate) struct Kit {
    pub geometry: Vec<[f32; 4]>,
    pub variants: [Variant; VARIANTS],
}
fn row(v: [f64; 3], w: f64) -> [f32; 4] {
    [v[0] as f32, v[1] as f32, v[2] as f32, w as f32]
}
fn number(v: &Value) -> Result<f64, String> {
    v.as_f64()
        .filter(|x| x.is_finite())
        .ok_or("invalid stock gem material number".into())
}
fn vec3(v: &Value) -> Result<[f64; 3], String> {
    Ok([number(&v[0])?, number(&v[1])?, number(&v[2])?])
}
fn bounds(tris: &[Triangle]) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for t in tris {
        for v in t.v {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
    }
    (lo, hi)
}
#[derive(Clone)]
struct Node {
    lo: [f64; 3],
    hi: [f64; 3],
    escape: usize,
    first: usize,
    count: usize,
    min_material: usize,
    max_material: usize,
}
fn node(tris: &mut [Triangle], nodes: &mut Vec<Node>, ordered: &mut Vec<Triangle>) {
    let (lo, hi) = bounds(tris);
    let at = nodes.len();
    let first = ordered.len();
    let count = if tris.len() <= 4 { tris.len() } else { 0 };
    nodes.push(Node {
        lo,
        hi,
        escape: 0,
        first,
        count,
        min_material: tris.iter().map(|t| t.material).min().unwrap(),
        max_material: tris.iter().map(|t| t.material).max().unwrap(),
    });
    if count > 0 {
        ordered.extend_from_slice(tris);
    } else {
        let axis = (0..3)
            .max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b])))
            .unwrap();
        tris.sort_by(|a, b| {
            a.v.iter()
                .map(|v| v[axis])
                .sum::<f64>()
                .total_cmp(&b.v.iter().map(|v| v[axis]).sum::<f64>())
        });
        let middle = tris.len() / 2;
        let (a, b) = tris.split_at_mut(middle);
        node(a, nodes, ordered);
        node(b, nodes, ordered);
    }
    nodes[at].escape = nodes.len();
}
fn packet(tris: &mut [Triangle], rows: &mut Vec<[f32; 4]>) -> Variant {
    let base = BASE + rows.len() - PARTS * 40;
    let root = BASE + rows.len();
    let mut nodes = Vec::new();
    let mut ordered = Vec::new();
    node(tris, &mut nodes, &mut ordered);
    let end = root + nodes.len() * 3;
    let (min, max) = bounds(tris);
    for n in nodes {
        rows.push(row(n.lo, (root + n.escape * 3) as f64));
        rows.push(row(n.hi, (end + n.first * 6) as f64));
        rows.push([
            n.count as f32,
            n.min_material as f32,
            n.max_material as f32,
            0.,
        ]);
    }
    for t in ordered {
        for i in 0..3 {
            rows.push(row(t.v[i], if i == 0 { t.material as f64 } else { 0. }));
        }
        for n in t.n {
            rows.push(row(n, 0.));
        }
    }
    Variant {
        base,
        root,
        end,
        min,
        max,
    }
}
impl Kit {
    pub(crate) fn load(root: &Path) -> Result<Arc<Self>, String> {
        // Identical generated geometry can be shared by panel writer and renderer.
        static CACHE: std::sync::Mutex<Option<(std::path::PathBuf, Arc<Kit>)>> =
            std::sync::Mutex::new(None);
        let mut cache = CACHE.lock().map_err(|_| "stock gem cache poisoned")?;
        if let Some((path, kit)) = &*cache {
            if path == root {
                return Ok(kit.clone());
            }
        }
        let connected: Value = serde_json::from_slice(
            &std::fs::read(root.join("Art/AudioWorkstation/EdisonHardware/connected.json"))
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        if connected["base"].as_u64() != Some(super::edison_demo::PLUG_BASE as u64)
            || connected["rows"].as_u64().is_none_or(|n| n > 131072)
        {
            return Err("stock gems overlap connected Edison packet".into());
        }
        let runtime: Value = serde_json::from_slice(
            &std::fs::read(root.join("Art/AudioWorkstation/GemPanels/GemPanels_runtime.json"))
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let material = runtime["materials"]
            .as_array()
            .and_then(|a| a.iter().find(|m| m["asset"] == "GemPanel_Broad"))
            .ok_or("missing approved bottom gem material")?;
        let kit = Arc::new(Self::build(material)?);
        *cache = Some((root.into(), kit.clone()));
        Ok(kit)
    }
    fn build(material: &Value) -> Result<Self, String> {
        let controls = stock_eq::Controls::new([0., 0., 230., 132.], 1.);
        let rects = [
            controls.rect,
            controls.slopes[0],
            controls.slopes[1],
            controls.bypass,
            controls.remove,
        ];
        let mut shapes = Vec::new();
        for r in rects {
            shapes.push(panel_layout::shape(r[2], r[3])?);
        }
        let sigma = vec3(&material["sigma_per_source_unit"])?;
        let base_color = vec3(&material["base_color_linear_rgba"])?;
        let mut rows = Vec::new();
        let empty = Variant {
            base: 0,
            root: 0,
            end: 0,
            min: [0.; 3],
            max: [0.; 3],
        };
        let mut variants = [empty; VARIANTS];
        for flags in 0..VARIANTS {
            let selected = flags & 1 != 0;
            let bypassed = flags & 2 != 0;
            let mut triangles = Vec::new();
            for part in 0..PARTS {
                let r = rects[part];
                let optical = panel_layout::optical_shape(r[2], r[3])?;
                let shape = &shapes[part];
                let pressed = (part == 3 && flags & 4 != 0) || (part == 4 && flags & 8 != 0);
                let depth = if pressed { 0.75 } else { 1. };
                let shift = [
                    r[0] + r[2] * 0.5 - 115.,
                    66. - r[1] - r[3] * 0.5,
                    if part == 0 {
                        0.
                    } else {
                        optical.dimensions_m[2] * depth + 0.5
                    },
                ];
                let at = rows.len();
                rows.resize(at + 40, [0.; 4]);
                rows[at] = [
                    1.,
                    number(&material["ior"])? as f32,
                    number(&material["roughness"])? as f32,
                    number(&material["transmission"])? as f32,
                ];
                // Default sigma is byte-equivalent to the approved bottom gem model at
                // this size. Original selected/B state changes tint, never the base BSDF.
                let mut absorption = sigma;
                if part == 3 && bypassed {
                    let tint = [244. / 255., 241. / 255., 232. / 255.];
                    absorption = tint.map(|v: f64| -v.ln() * 1.7 + 0.03);
                }
                if selected {
                    let blue = [0.18, 0.52, 0.95];
                    for k in 0..3 {
                        let tint = (-absorption[k]).exp() * 0.55 + blue[k] * 0.45;
                        absorption[k] = -tint.clamp(0.015, 0.98).ln();
                    }
                }
                rows[at + 1] = row(absorption.map(|v| v / optical.local_unit_m), 0.);
                rows[at + 2] = [0., 0., 26., 0.];
                rows[at + 4] = row(base_color, 0.);
                for (i, p) in optical.planes.iter().enumerate() {
                    let n = [p[0], p[1], p[2] / depth];
                    let length = n.iter().map(|v| v * v).sum::<f64>().sqrt();
                    rows[at + 6 + i] = row(
                        n.map(|v| v / length),
                        (p[3] + n.iter().zip(shift).map(|(a, b)| a * b).sum::<f64>()) / length,
                    );
                }
                for (offset, key) in [(32, "ggx_glass_E"), (36, "ggx_glass_inv_E")] {
                    for i in 0..16 {
                        rows[at + offset + i / 4][i % 4] =
                            number(&material["ggx_multiscatter"][key][i])? as f32;
                    }
                }
                for vs in shape.vertices.chunks_exact(3) {
                    let mut t = Triangle {
                        v: [[0.; 3]; 3],
                        n: [[0.; 3]; 3],
                        material: part,
                    };
                    for i in 0..3 {
                        t.v[i] = [
                            vs[i].position[0] + shift[0],
                            vs[i].position[1] + shift[1],
                            vs[i].position[2] * depth + shift[2],
                        ];
                        let n = [vs[i].normal[0], vs[i].normal[1], vs[i].normal[2] / depth];
                        let len = n.iter().map(|v| v * v).sum::<f64>().sqrt();
                        t.n[i] = n.map(|v| v / len);
                    }
                    triangles.push(t);
                }
            }
            variants[flags] = packet(&mut triangles, &mut rows);
        }
        if rows.len() > CAPACITY || rows.iter().flatten().any(|v| !v.is_finite()) {
            return Err("stock gem geometry exceeds reserved packet".into());
        }
        Ok(Self {
            geometry: rows,
            variants,
        })
    }
    pub(crate) fn variant(
        &self,
        selected: bool,
        bypassed: bool,
        bypass_pressed: bool,
        remove_pressed: bool,
    ) -> Variant {
        self.variants[usize::from(selected)
            | usize::from(bypassed) << 1
            | usize::from(bypass_pressed) << 2
            | usize::from(remove_pressed) << 3]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn kit() -> Arc<Kit> {
        Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap()
    }
    fn face(kit: &Kit, variant: usize, p: [f64; 2]) -> (usize, f64) {
        let v = kit.variants[variant];
        let stop = kit
            .variants
            .get(variant + 1)
            .map(|v| v.base)
            .unwrap_or(BASE + kit.geometry.len());
        let mut hit = (usize::MAX, f64::NEG_INFINITY);
        for at in (v.end..stop).step_by(6) {
            let a = kit.geometry[at - BASE];
            let b = kit.geometry[at + 1 - BASE];
            let c = kit.geometry[at + 2 - BASE];
            let ax = a[0] as f64;
            let ay = a[1] as f64;
            let bx = b[0] as f64;
            let by = b[1] as f64;
            let cx = c[0] as f64;
            let cy = c[1] as f64;
            let det = (by - cy) * (ax - cx) + (cx - bx) * (ay - cy);
            if det.abs() < 1e-12 {
                continue;
            }
            let u = ((by - cy) * (p[0] - cx) + (cx - bx) * (p[1] - cy)) / det;
            let w = ((cy - ay) * (p[0] - cx) + (ax - cx) * (p[1] - cy)) / det;
            let t = 1. - u - w;
            if u >= -1e-9 && w >= -1e-9 && t >= -1e-9 {
                let z = u * a[2] as f64 + w * b[2] as f64 + t * c[2] as f64;
                if z > hit.1 {
                    hit = (a[3] as usize, z);
                }
            }
        }
        hit
    }
    #[test]
    fn stock_gems_preserve_original_five_surfaces_and_physical_press() {
        let kit = kit();
        assert!(kit.geometry.len() < CAPACITY);
        assert!(
            BASE + kit.geometry.len()
                <= super::super::edison_demo::PLUG_BASE + super::super::edison_demo::PLUG_ROWS
        );
        for (i, p) in [
            [0., 0.],
            [-53.5, -51.],
            [53.5, -51.],
            [-17., 88.],
            [17., 88.],
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(face(&kit, 0, *p).0, i);
        }
        let b = face(&kit, 0, [-17., 88.]).1;
        let pressed = face(&kit, 4, [-17., 88.]).1;
        assert!((b - 16.6).abs() < 1e-4);
        assert!((pressed - 12.575).abs() < 1e-4);
        assert_eq!(face(&kit, 0, [0., 0.]), face(&kit, 4, [0., 0.]));
        assert_eq!(face(&kit, 0, [17., 88.]), face(&kit, 4, [17., 88.]));
        assert!(face(&kit, 8, [17., 88.]).1 < face(&kit, 0, [17., 88.]).1);
        eprintln!(
            "stock gem packet: {} immutable rows, 16 variants, 5 parts each",
            kit.geometry.len()
        );
    }
    #[test]
    fn stock_gems_preserve_approved_default_optics_and_closed_planes() {
        let kit = kit();
        let runtime: Value = serde_json::from_str(include_str!(
            "../Art/AudioWorkstation/GemPanels/GemPanels_runtime.json"
        ))
        .unwrap();
        let m = runtime["materials"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["asset"] == "GemPanel_Broad")
            .unwrap();
        let a = kit.variants[0].base - BASE;
        assert_eq!(
            kit.geometry[a],
            [
                1.,
                m["ior"].as_f64().unwrap() as f32,
                m["roughness"].as_f64().unwrap() as f32,
                m["transmission"].as_f64().unwrap() as f32
            ]
        );
        for i in 0..3 {
            assert_eq!(
                kit.geometry[a + 1][i],
                (m["sigma_per_source_unit"][i].as_f64().unwrap() / 66.) as f32
            );
        }
        for variant in 0..VARIANTS {
            let v = kit.variants[variant];
            let stop = kit
                .variants
                .get(variant + 1)
                .map(|v| v.base)
                .unwrap_or(BASE + kit.geometry.len());
            assert!(v.root < v.end);
            assert_eq!(v.root, v.base + PARTS * 40);
            for at in (v.end..stop).step_by(6) {
                let material = kit.geometry[at - BASE][3] as usize;
                assert!(material < PARTS);
                let base = v.base + 40 * material - BASE;
                assert_eq!(kit.geometry[base], kit.geometry[a]);
                assert_eq!(kit.geometry[base + 2][2], 26.);
                for vertex in &kit.geometry[at - BASE..at + 3 - BASE] {
                    for axis in 0..3 {
                        assert!(
                            vertex[axis] as f64 >= v.min[axis] - 1e-4
                                && vertex[axis] as f64 <= v.max[axis] + 1e-4
                        );
                    }
                    for plane in &kit.geometry[base + 6..base + 32] {
                        let distance = (0..3)
                            .map(|i| plane[i] as f64 * vertex[i] as f64)
                            .sum::<f64>()
                            - plane[3] as f64;
                        assert!(
                            distance < 0.001,
                            "variant {variant}, part {material}, outside {distance}"
                        );
                    }
                }
            }
        }
    }
}
