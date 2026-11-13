//! Evaluated Blender rigid-joint animation, shared by every scene backend.
use crate::audio::{cross, dot, mul, sub, unit, vector};
use serde_json::{Value, json};
use std::sync::OnceLock;
struct Model {
    colors: Vec<[f32; 3]>,
    frames: Vec<Vec<[f32; 3]>>,
}
fn model() -> &'static Model {
    static M: OnceLock<Model> = OnceLock::new();
    M.get_or_init(|| {
        let bytes = include_bytes!("../../Procedures/Assets/Wildlife/silver_spider_crab.bin");
        assert_eq!(&bytes[..4], b"SSC1");
        let mut i = 4;
        let mut word = || {
            let v = u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap());
            i += 4;
            v
        };
        let n = word() as usize;
        let count = word() as usize;
        let colors = (0..n)
            .map(|_| std::array::from_fn(|_| f32::from_bits(word())))
            .collect();
        let frames = (0..count)
            .map(|_| {
                (0..n)
                    .map(|_| std::array::from_fn(|_| f32::from_bits(word())))
                    .collect()
            })
            .collect();
        assert_eq!(i, bytes.len());
        Model { colors, frames }
    })
}
pub(crate) fn spec() -> Value {
    let m = model();
    let vertices: Vec<_> = m.frames[0]
        .iter()
        .zip(&m.colors)
        .map(|(p, c)| [p[0], p[1], p[2], c[0], c[1], c[2]])
        .collect();
    json!({"kind":"visual","shader":"material_surface","vertices":vertices})
}
pub(crate) fn animate(vertices: &mut [[f32; 6]], instance: &Value) -> Result<(), String> {
    let m = model();
    if vertices.len() != m.colors.len() {
        return Err("crab topology mismatch".into());
    }
    let pose = &instance["crab_pose"];
    let up = unit(vector(&pose["up"])?);
    let f = vector(&pose["forward"])?;
    let projected = sub(f, mul(up, dot(f, up)));
    let forward = if dot(projected, projected) > 1e-8 {
        unit(projected)
    } else {
        unit(cross(
            if up[1].abs() < 0.9 {
                [0., 1., 0.]
            } else {
                [1., 0., 0.]
            },
            up,
        ))
    };
    let right = unit(cross(up, forward));
    if dot(right, right) < 0.9 {
        return Err("invalid crab orientation".into());
    }
    let scale = pose["scale"].as_f64().unwrap_or(1.);
    let t = if pose["moving"] == true {
        pose["time"].as_f64().unwrap_or(0.) * 16.
    } else {
        0.
    };
    let frame = t.floor() as usize % m.frames.len();
    let next = (frame + 1) % m.frames.len();
    let a = t.fract();
    for (i, v) in vertices.iter_mut().enumerate() {
        let p: [f64; 3] = std::array::from_fn(|j| {
            (m.frames[frame][i][j] as f64 * (1. - a) + m.frames[next][i][j] as f64 * a) * scale
        });
        for j in 0..3 {
            v[j] = (right[j] * p[0] + up[j] * p[1] + forward[j] * p[2]) as f32;
        }
        // Faceted silver highlights remain visible in the scene's existing lighting.
        let c = m.colors[i];
        v[3..6].copy_from_slice(&c);
    }
    // Metallic facet bands are evaluated only for this creature, not the DAW.
    for tri in vertices.chunks_exact_mut(3) {
        let a = tri[0];
        let b = tri[1];
        let c = tri[2];
        let n = unit(cross(
            [
                (b[0] - a[0]) as f64,
                (b[1] - a[1]) as f64,
                (b[2] - a[2]) as f64,
            ],
            [
                (c[0] - a[0]) as f64,
                (c[1] - a[1]) as f64,
                (c[2] - a[2]) as f64,
            ],
        ));
        let band = dot(n, unit([0.3, 0.8, 0.5])).abs();
        let value = (0.22 + 0.48 * band + 0.3 * band.powi(24)) as f32;
        for v in tri {
            if v[3] < 0.8 || v[4] > 0.5 {
                for c in 3..6 {
                    v[c] *= value;
                }
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exported_mesh_and_animation_are_finite_and_bounded() {
        let m = model();
        assert!(m.colors.len() < 30000);
        assert_eq!(m.frames.len(), 16);
        for f in &m.frames {
            assert_eq!(f.len(), m.colors.len());
            assert!(f.iter().flatten().all(|v| v.is_finite() && v.abs() < 4.));
        }
        let mut vertices: Vec<[f32; 6]> =
            serde_json::from_value(spec()["vertices"].clone()).unwrap();
        animate(&mut vertices,&json!({"crab_pose":{"up":[0,1,0],"forward":[0,0,1],"time":0.3,"moving":true,"scale":1}})).unwrap();
    }
}
