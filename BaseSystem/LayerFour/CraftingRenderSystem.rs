//! Metric Blender meshes projected into the shared depth scene.
use crate::{
    audio::*,
    base_system::layer_five::crafting_system::{Kind, pose},
    host::Host,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
#[derive(Default)]
pub(crate) struct State {
    models: BTreeMap<String, Arc<Vec<[f32; 6]>>>,
}
pub(crate) fn frame(host: &Host, view: &Value) -> Result<Vec<[f32; 24]>, String> {
    let eye = vector(&view["position"])?;
    let mut vertices = Vec::new();
    for i in &host.crafting.items {
        let (p, up, right) = pose(i, host.crafting.held == Some(i.id), view)?;
        if length(sub(p, eye)) > 70. {
            continue;
        }
        let front = unit(cross(right, up));
        let key = match i.kind {
            Kind::Jar => "jar",
            Kind::Blueprint => "blueprint",
            Kind::Plank => "plank",
        };
        let Some(mesh) = host.crafting_render.models.get(key) else {
            continue;
        };
        for t in mesh.chunks_exact(3) {
            let points: [V; 3] = std::array::from_fn(|k| {
                add(
                    sub(p, eye),
                    add(
                        mul(right, t[k][0] as f64),
                        add(mul(up, t[k][1] as f64), mul(front, t[k][2] as f64)),
                    ),
                )
            });
            let normal = unit(cross(sub(points[1], points[0]), sub(points[2], points[0])));
            let shade = (0.45 + 0.55 * dot(normal, unit(mul(points[0], -1.))).abs()) as f32;
            for k in 0..3 {
                vertices.push([
                    points[k][0] as f32,
                    points[k][1] as f32,
                    points[k][2] as f32,
                    t[k][3] * shade,
                    t[k][4] * shade,
                    t[k][5] * shade,
                ]);
            }
        }
    }
    let mut local = view.clone();
    local["position"] = json!([0., 0., 0.]);
    local["two_sided"] = json!(true);
    Ok(super::appearance_system::projection::project(
        &vertices, &local,
    ))
}
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    match hook {
        "Load" => {
            let bytes = std::fs::read(
                host.root()
                    .join("Procedures/Assets/Crafting/crafting_mesh.json"),
            )
            .map_err(|e| format!("crafting models: {e}"))?;
            if bytes.len() > 4 * 1024 * 1024 {
                return Err("crafting model exceeds budget".into());
            }
            let data: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if data["schema_version"] != 1 {
                return Err("unsupported crafting model schema".into());
            }
            let mut models = BTreeMap::new();
            for key in ["jar", "blueprint", "plank"] {
                let v: Vec<[f32; 6]> =
                    serde_json::from_value(data["models"][key]["vertices"].clone())
                        .map_err(|e| e.to_string())?;
                if v.is_empty()
                    || v.len() % 3 != 0
                    || v.len() > 18000
                    || v.iter().flatten().any(|v| !v.is_finite())
                {
                    return Err("invalid crafting model".into());
                }
                models.insert(key.into(), Arc::new(v));
            }
            host.crafting_render.models = models;
            Ok(Value::Null)
        }
        "Frame" => Ok(json!({"triangles":frame(host,input)?})),
        _ => Err("unknown crafting render hook".into()),
    }
}
