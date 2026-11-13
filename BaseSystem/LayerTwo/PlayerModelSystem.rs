//! Bounded GLB 2 skin loader. Adapted from Cardinal PlayerRigSystem's loader.
#[path = "RigMath.rs"]
pub(crate) mod math;
use crate::host::Host;
use math::*;
use serde_json::{Value, json};
pub(crate) const ASSET: &str = "Procedures/Assets/Player/prismal_player_welded_rigged.glb";
#[derive(Clone, Default)]
pub(crate) struct Joint {
    pub name: String,
    pub parent: Option<usize>,
    pub rest: V,
}
#[derive(Clone)]
pub(crate) struct Vertex {
    pub p: V,
    pub uv: [f64; 2],
    pub joints: [usize; 4],
    pub weights: [f64; 4],
}
#[derive(Default)]
pub(crate) struct Model {
    pub joints: Vec<Joint>,
    pub vertices: Vec<Vertex>,
    pub head_triangles: Vec<bool>,
}
pub(crate) fn validate(data: &Value) -> bool {
    data["joints"].as_array().is_some_and(|a| {
        !a.is_empty()
            && a.len() <= 128
            && a.iter().all(|j| {
                j["name"].is_string()
                    && vector(&j["rest"]).is_ok()
                    && (j["parent"].is_null()
                        || j["parent"].as_u64().is_some_and(|v| v < a.len() as u64))
            })
    }) && data["vertices"].as_array().is_some_and(|a| {
        !a.is_empty()
            && a.len() <= 24576
            && a.len() % 3 == 0
            && a.iter().all(|v| {
                v.as_array().is_some_and(|r| {
                    r.len() == 13 && r.iter().all(|v| v.as_f64().is_some_and(f64::is_finite))
                })
            })
    })
}
impl Model {
    pub fn data(&self) -> Value {
        json!({"joints":self.joints.iter().map(|j|json!({"name":j.name,"parent":j.parent,"rest":j.rest})).collect::<Vec<_>>(),"vertices":self.vertices.iter().map(|v|json!([v.p[0],v.p[1],v.p[2],v.uv[0],v.uv[1],v.joints[0],v.joints[1],v.joints[2],v.joints[3],v.weights[0],v.weights[1],v.weights[2],v.weights[3]])).collect::<Vec<_>>()})
    }
    pub fn from_data(data: &Value) -> Result<Self, String> {
        if !validate(data) {
            return Err("invalid rig data".into());
        }
        let joints = data["joints"]
            .as_array()
            .unwrap()
            .iter()
            .map(|j| {
                Ok(Joint {
                    name: j["name"].as_str().unwrap().into(),
                    parent: j["parent"].as_u64().map(|n| n as usize),
                    rest: vector(&j["rest"])?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        for (i, j) in joints.iter().enumerate() {
            let mut p = j.parent;
            for _ in 0..=joints.len() {
                match p {
                    Some(n) if n == i => return Err("cyclic skin".into()),
                    Some(n) => p = joints[n].parent,
                    None => break,
                }
            }
            if p.is_some() {
                return Err("cyclic skin".into());
            }
        }
        let head = joints
            .iter()
            .position(|j| j.name == "Head")
            .ok_or("Head joint missing")?;
        let mut vertices = Vec::new();
        for r in data["vertices"].as_array().unwrap() {
            let mut v = Vertex {
                p: [0.; 3],
                uv: [0.; 2],
                joints: [0; 4],
                weights: [0.; 4],
            };
            for i in 0..3 {
                v.p[i] = r[i].as_f64().unwrap();
            }
            for i in 0..2 {
                v.uv[i] = r[3 + i].as_f64().unwrap();
            }
            for i in 0..4 {
                v.joints[i] = r[5 + i]
                    .as_u64()
                    .filter(|n| *n < joints.len() as u64)
                    .ok_or("skin joint out of bounds")? as usize;
                v.weights[i] = r[9 + i]
                    .as_f64()
                    .filter(|n| *n >= 0.)
                    .ok_or("negative skin weight")?;
            }
            let sum: f64 = v.weights.iter().sum();
            if sum < 1e-8 {
                return Err("unweighted skin vertex".into());
            }
            for w in &mut v.weights {
                *w /= sum;
            }
            vertices.push(v);
        }
        let head_triangles = vertices
            .chunks_exact(3)
            .map(|tri| {
                tri.iter()
                    .filter(|v| {
                        v.joints[(0..4)
                            .max_by(|&a, &b| v.weights[a].total_cmp(&v.weights[b]))
                            .unwrap()]
                            == head
                    })
                    .count()
                    >= 2
            })
            .collect();
        Ok(Self {
            joints,
            vertices,
            head_triangles,
        })
    }
}
fn u32le(bytes: &[u8], at: usize) -> Result<usize, String> {
    let b = bytes
        .get(at..at.checked_add(4).ok_or("GLB overflow")?)
        .ok_or("truncated GLB")?;
    Ok(u32::from_le_bytes(b.try_into().unwrap()) as usize)
}
struct Glb<'a> {
    g: Value,
    bin: &'a [u8],
}
impl<'a> Glb<'a> {
    fn parse(b: &'a [u8]) -> Result<Self, String> {
        if b.len() > 32 * 1024 * 1024
            || u32le(b, 0)? != 0x46546c67
            || u32le(b, 4)? != 2
            || u32le(b, 8)? != b.len()
        {
            return Err("invalid GLB 2 header".into());
        }
        let mut at = 12;
        let mut g = None;
        let mut bin = None;
        while at < b.len() {
            let n = u32le(b, at)?;
            let kind = u32le(b, at + 4)?;
            at += 8;
            let end = at.checked_add(n).ok_or("GLB length overflow")?;
            let chunk = b.get(at..end).ok_or("truncated GLB chunk")?;
            if kind == 0x4e4f534a {
                g = Some(serde_json::from_slice(chunk).map_err(|e| e.to_string())?);
            } else if kind == 0x004e4942 {
                bin = Some(chunk);
            }
            at = end;
        }
        let g: Value = g.ok_or("missing GLB JSON")?;
        if g["extensionsRequired"]
            .as_array()
            .is_some_and(|v| !v.is_empty())
        {
            return Err("unsupported required GLB extension".into());
        }
        Ok(Self {
            g,
            bin: bin.ok_or("missing GLB BIN")?,
        })
    }
    fn accessor(&self, id: usize, width: usize) -> Result<Vec<Vec<f64>>, String> {
        let a = self.g["accessors"].get(id).ok_or("accessor missing")?;
        if !a["sparse"].is_null() {
            return Err("sparse accessors unsupported".into());
        }
        let ty = match a["type"].as_str() {
            Some("SCALAR") => 1,
            Some("VEC2") => 2,
            Some("VEC3") => 3,
            Some("VEC4") => 4,
            Some("MAT4") => 16,
            _ => 0,
        };
        if ty != width {
            return Err("accessor shape mismatch".into());
        }
        let kind = a["componentType"]
            .as_u64()
            .ok_or("accessor component missing")?;
        let size = match kind {
            5121 => 1,
            5123 => 2,
            5125 | 5126 => 4,
            _ => return Err("unsupported accessor component".into()),
        };
        let count = a["count"]
            .as_u64()
            .filter(|n| *n <= 32768)
            .ok_or("accessor budget exceeded")? as usize;
        let v = self.g["bufferViews"]
            .get(a["bufferView"].as_u64().ok_or("buffer view missing")? as usize)
            .ok_or("bad buffer view")?;
        if v["buffer"].as_u64() != Some(0) {
            return Err("external buffer unsupported".into());
        }
        let offset = v["byteOffset"].as_u64().unwrap_or(0) as usize;
        let length = v["byteLength"].as_u64().ok_or("view length missing")? as usize;
        let source = self
            .bin
            .get(offset..offset.checked_add(length).ok_or("view overflow")?)
            .ok_or("view outside BIN")?;
        let stride = v["byteStride"].as_u64().unwrap_or((width * size) as u64) as usize;
        if stride < width * size || stride > 252 && v["byteStride"].is_number() {
            return Err("invalid accessor stride".into());
        }
        let start = a["byteOffset"].as_u64().unwrap_or(0) as usize;
        let mut rows = Vec::with_capacity(count);
        for i in 0..count {
            let at = start
                .checked_add(i.checked_mul(stride).ok_or("accessor overflow")?)
                .ok_or("accessor overflow")?;
            let data = source
                .get(at..at.checked_add(width * size).ok_or("accessor overflow")?)
                .ok_or("accessor outside view")?;
            let mut row = Vec::with_capacity(width);
            for j in 0..width {
                let b = &data[j * size..(j + 1) * size];
                let n = match kind {
                    5121 => b[0] as f64,
                    5123 => u16::from_le_bytes(b.try_into().unwrap()) as f64,
                    5125 => u32::from_le_bytes(b.try_into().unwrap()) as f64,
                    _ => f32::from_le_bytes(b.try_into().unwrap()) as f64,
                };
                let n = if a["normalized"] == true && kind != 5126 {
                    n / if kind == 5121 {
                        255.
                    } else if kind == 5123 {
                        65535.
                    } else {
                        4294967295.
                    }
                } else {
                    n
                };
                if !n.is_finite() {
                    return Err("nonfinite accessor".into());
                }
                row.push(n);
            }
            rows.push(row);
        }
        Ok(rows)
    }
    fn load(&self) -> Result<Model, String> {
        let skin = &self.g["skins"][0];
        let nodes = skin["joints"]
            .as_array()
            .filter(|v| !v.is_empty() && v.len() <= 128)
            .ok_or("skin joints missing")?;
        let nodes = nodes
            .iter()
            .map(|n| n.as_u64().map(|n| n as usize).ok_or("bad skin node"))
            .collect::<Result<Vec<_>, _>>()?;
        let inv = self.accessor(
            skin["inverseBindMatrices"]
                .as_u64()
                .ok_or("inverse bind missing")? as usize,
            16,
        )?;
        if inv.len() != nodes.len() {
            return Err("bind count mismatch".into());
        }
        let all = self.g["nodes"].as_array().ok_or("nodes missing")?;
        let mut parents = vec![None; all.len()];
        for (i, n) in all.iter().enumerate() {
            if let Some(children) = n["children"].as_array() {
                for c in children {
                    let c = c
                        .as_u64()
                        .filter(|n| *n < all.len() as u64)
                        .ok_or("bad child")? as usize;
                    if parents[c].replace(i).is_some() {
                        return Err("multiply parented GLB node".into());
                    }
                }
            }
        }
        let mut joints = Vec::new();
        for (i, &n) in nodes.iter().enumerate() {
            let node = all.get(n).ok_or("joint node missing")?;
            let m = inverse(inv[i].clone().try_into().unwrap())?;
            let mut p = parents[n];
            let mut parent = None;
            for _ in 0..all.len() {
                match p {
                    Some(k) => {
                        if let Some(j) = nodes.iter().position(|n| *n == k) {
                            parent = Some(j);
                            break;
                        }
                        p = parents[k];
                    }
                    None => break,
                }
            }
            joints.push(Joint {
                name: node["name"].as_str().ok_or("named joints required")?.into(),
                parent,
                rest: [m[12], m[13], m[14]],
            });
        }
        let primitive = &self.g["meshes"][0]["primitives"][0];
        if primitive["mode"].as_u64().unwrap_or(4) != 4 {
            return Err("triangle primitive required".into());
        }
        let attr = &primitive["attributes"];
        let read = |key: &str, n| {
            self.accessor(
                attr[key].as_u64().ok_or_else(|| format!("missing {key}"))? as usize,
                n,
            )
        };
        let p = read("POSITION", 3)?;
        let uv = read("TEXCOORD_0", 2)?;
        let js = read("JOINTS_0", 4)?;
        let ws = read("WEIGHTS_0", 4)?;
        if [uv.len(), js.len(), ws.len()].iter().any(|n| *n != p.len()) {
            return Err("attribute count mismatch".into());
        }
        let indices = self.accessor(
            primitive["indices"].as_u64().ok_or("indices missing")? as usize,
            1,
        )?;
        if indices.is_empty() || indices.len() % 3 != 0 || indices.len() > 24576 {
            return Err("mesh triangle budget exceeded".into());
        }
        let mut vertices = Vec::new();
        for row in indices {
            let i = row[0] as usize;
            if i >= p.len() {
                return Err("mesh index out of bounds".into());
            }
            vertices.push(Vertex {
                p: p[i].clone().try_into().unwrap(),
                uv: uv[i].clone().try_into().unwrap(),
                joints: std::array::from_fn(|j| js[i][j] as usize),
                weights: ws[i].clone().try_into().unwrap(),
            });
        }
        Model::from_data(
            &Model {
                joints,
                vertices,
                head_triangles: Vec::new(),
            }
            .data(),
        )
    }
}
pub(crate) fn load(host: &mut Host, _: &Value) -> Result<Value, String> {
    let b =
        std::fs::read(host.root().join(ASSET)).map_err(|e| format!("Prismals player GLB: {e}"))?;
    Ok(Glb::parse(&b)?.load()?.data())
}
