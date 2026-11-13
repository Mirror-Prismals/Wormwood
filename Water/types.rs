//! Bounded continuous envelopes shared by water Systems; no native handles.
use super::math::*;
use serde_json::Value;
pub(crate) fn vector(v: &Value) -> Result<V, String> {
    let p: V =
        serde_json::from_value(v.clone()).map_err(|_| "water vector requires three numbers")?;
    if !finite(p) || p.iter().any(|n| n.abs() > 1e9) {
        return Err("invalid water vector".into());
    }
    Ok(p)
}
pub(crate) fn number(
    v: &Value,
    key: &str,
    fallback: f64,
    low: f64,
    high: f64,
) -> Result<f64, String> {
    let n = if v[key].is_null() {
        fallback
    } else {
        v[key]
            .as_f64()
            .ok_or_else(|| format!("invalid water {key}"))?
    };
    if !n.is_finite() || !(low..=high).contains(&n) {
        return Err(format!("water {key} outside bounds"));
    }
    Ok(n)
}
pub(crate) fn frame(v: &Value) -> Result<Frame, String> {
    let f = Frame {
        origin: vector(&v["origin"])?,
        right: vector(&v["right"])?,
        up: vector(&v["up"])?,
        forward: vector(&v["forward"])?,
    };
    if [f.right, f.up, f.forward]
        .iter()
        .any(|n| (len(*n) - 1.).abs() > 1e-5)
        || dot(f.right, f.up).abs() > 1e-5
        || dot(f.right, f.forward).abs() > 1e-5
        || dot(f.up, f.forward).abs() > 1e-5
        || dot(cross(f.right, f.up), f.forward) < 0.99999
    {
        return Err("invalid water frame".into());
    }
    Ok(f)
}
#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub origin: V,
    pub vertices: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
    pub revision: u64,
    /// Finite local metre-space query bounds, not terrain occupancy.
    pub min: V,
    pub max: V,
}
impl Snapshot {
    pub fn packet(&self) -> Value {
        serde_json::json!({"origin":self.origin,"vertices":self.vertices,"triangles":self.triangles,"revision":self.revision,
            "coverage":{"min":self.min,"max":self.max},"terrain_spacing_metres":3.,"material_surface":"accepted"})
    }
    pub fn parse(v: &Value) -> Result<Self, String> {
        let origin = vector(&v["origin"])?;
        let vertices: Vec<[f32; 3]> = serde_json::from_value(v["vertices"].clone())
            .map_err(|_| "invalid collision vertices")?;
        let triangles: Vec<[u32; 3]> = serde_json::from_value(v["triangles"].clone())
            .map_err(|_| "invalid collision triangles")?;
        let min = vector(&v["coverage"]["min"])?;
        let max = vector(&v["coverage"]["max"])?;
        if vertices.len() > 262144
            || triangles.len() > 262144
            || (0..3).any(|i| min[i] >= max[i])
            || vertices.iter().flatten().any(|n| !n.is_finite())
            || vertices.iter().any(|p| {
                (0..3).any(|i| (p[i] as f64) < min[i] - 0.01 || (p[i] as f64) > max[i] + 0.01)
            })
            || triangles
                .iter()
                .flatten()
                .any(|i| *i as usize >= vertices.len())
        {
            return Err("invalid bounded collision snapshot".into());
        }
        for t in &triangles {
            let [a, b, c] = t.map(|i| vertices[i as usize].map(f64::from));
            if len(cross(sub(b, a), sub(c, a))) < 1e-12 {
                return Err("degenerate collision triangle".into());
            }
        }
        Ok(Self {
            origin,
            vertices,
            triangles,
            revision: v["revision"]
                .as_u64()
                .ok_or("collision revision required")?,
            min,
            max,
        })
    }
    pub fn contains(&self, p: V, radius: f64) -> bool {
        let p = sub(p, self.origin);
        (0..3).all(|i| p[i] - radius >= self.min[i] && p[i] + radius <= self.max[i])
    }
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Triangle {
    pub p: [V; 3],
    pub min: V,
    pub max: V,
}
impl Triangle {
    pub fn new(p: [V; 3]) -> Self {
        Self {
            min: std::array::from_fn(|i| p.iter().map(|v| v[i]).fold(f64::INFINITY, f64::min)),
            max: std::array::from_fn(|i| p.iter().map(|v| v[i]).fold(f64::NEG_INFINITY, f64::max)),
            p,
        }
    }
}
