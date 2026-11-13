//! Tracked continuous water and trough appearances; native submission belongs
//! to the selected GraphicsConnectionSystem backend, using completed scene data.
use crate::{
    host::Host,
    water::{
        config,
        math::*,
        types::{number, vector},
    },
};
use serde_json::{Value, json};
#[derive(Default)]
pub(crate) struct State {
    entities: Vec<u64>,
    water_slots:Vec<bool>,
    instances: Value,
    settings: crate::water::optics::Settings,
}
impl State {
    pub(crate) fn planar_reflections(&self)->bool {self.settings.planar_reflection}
}
fn phases(origin: V) -> [f64; 3] {
    let p = crate::water::surface_mesh::chart().local(origin);
    let mut d = [1., 0.7];
    let mut out = [0.; 3];
    for (i, frequency) in [2.5, 4.5, 8.75].into_iter().enumerate() {
        if i > 0 {
            let a: f64 = if i == 1 { 1.5 } else { 2.2 };
            d = [
                d[0] * a.cos() - d[1] * a.sin(),
                d[0] * a.sin() + d[1] * a.cos(),
            ];
        }
        out[i] = ((p[0] * d[0] + p[2] * d[1]) * frequency).rem_euclid(std::f64::consts::TAU);
    }
    out
}
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let mut s = std::mem::take(&mut host.water.presentation);
    let result = (|| match hook {
        "Prepare" => {
            s.settings = config::Settings::parse(input)?.optics;
            Ok(json!({"instances":[]}))
        }
        "Frame" => {
            Ok(json!({"instances":if s.instances.is_array(){s.instances.clone()}else{json!([])}}))
        }
        "Update" => {
            let surface=host.water_surface_numeric(&json!({"position":input["position"],"forward":input["forward"],"view":input["view"]}))?;
            if host.verify_session {host.emit(json!({"event":"water_visibility","position":input["position"],"visibility":surface.mesh.visibility,"water_triangles":surface.mesh.indices.len()/3}))?;}
            if surface.mesh.truncated {return Err("continuous water mesh exceeds presentation budget".into());}
            let origin=surface.frame.origin;
            let indices=&surface.mesh.indices;
            let vertices=crate::water::presentation::mesh_rows(surface.frame,&surface.mesh);
            let time =
                number(input, "time", 0., 0., 1e12)?.rem_euclid(std::f64::consts::TAU * 1000.);
            let chart = crate::water::surface_mesh::chart();
            let uv_origin = chart.local(origin);
            let mut ripples = Vec::new();
            for r in input["ripples"]
                .as_array()
                .ok_or("water ripples array required")?
                .iter()
                .take(64)
            {
                let p = chart.local(vector(&r["position"])?);
                ripples.push(json!([
                    p[0] - uv_origin[0],
                    p[2] - uv_origin[2],
                    number(r, "age", 0., 0., 4.5)?,
                    number(r, "strength", 0., 0., 100.)?
                ]));
            }
            let optics = json!({"time":time,"murkiness":s.settings.murkiness,"sky_color_intensity":s.settings.sky_color_intensity,"neutral_reflection_intensity":s.settings.neutral_reflection_intensity,"sky_top":s.settings.sky_top,"sky_bottom":s.settings.sky_bottom,"light":s.settings.light,"planar_reflection":s.settings.planar_reflection&&surface.reflection_allowed,"micro_phase":phases(origin),"mean_height":surface.mean_height});
            let mut specs = Vec::new();
            for chunk in indices.chunks(24576) {
                let rows:Vec<_>=chunk.iter().map(|&i|vertices.get(i as usize).copied().ok_or("invalid water index".to_owned())).collect::<Result<_,_>>()?;
                if !rows.is_empty(){let packet=crate::water::presentation::Packet{frame:surface.frame,rows:std::sync::Arc::new(rows),indices:std::sync::Arc::new((0..chunk.len()as u32).collect()),optics:optics.clone(),ripples:json!(ripples)};
                    specs.push((Value::Null,Some(std::sync::Arc::new(packet)),origin));}
            }
            let trough = host.invoke("WaterSlideSystem.CollisionMesh", Value::Null)?;
            let points: Vec<V> = serde_json::from_value(trough["vertices"].clone())
                .map_err(|_| "trough vertices required")?;
            let normals: Vec<V> = serde_json::from_value(trough["normals"].clone())
                .map_err(|_| "trough normals required")?;
            let materials: Vec<u32> = serde_json::from_value(trough["materials"].clone())
                .map_err(|_| "trough materials required")?;
            let faces: Vec<[u32; 3]> = serde_json::from_value(trough["triangles"].clone())
                .map_err(|_| "trough triangles required")?;
            for chunk in faces.chunks(8192) {
                let mut rows = Vec::with_capacity(chunk.len() * 3);
                for face in chunk {
                    for &index in face {
                        let i = index as usize;
                        if i >= points.len() || i >= normals.len() || i >= materials.len() {
                            return Err("invalid trough appearance index".into());
                        }
                        let p = sub(points[i], origin);
                        let light = 0.35 + 0.65 * dot(normals[i], unit(s.settings.light)).abs();
                        let color = if materials[i] == 529 {
                            [0.65, 0.08, 0.12]
                        } else {
                            [0.08, 0.55, 0.2]
                        };
                        rows.push([
                            p[0],
                            p[1],
                            p[2],
                            color[0] * light,
                            color[1] * light,
                            color[2] * light,
                        ]);
                    }
                }
                if !rows.is_empty() {
                    specs.push((
                        json!({"kind":"visual","shader":"material_surface","vertices":rows}),
                        None,origin,
                    ));
                }
            }
            let mut instances = Vec::new();
            for (i, (mut spec, water, origin)) in specs.into_iter().enumerate() {
                let is_water=water.is_some();
                // Culling changes the number of water chunks ahead of the
                // trough slots. Recreate a slot when its representation changes
                // instead of sending a material Entity to the typed water route.
                if i<s.entities.len() && s.water_slots[i]!=is_water {
                    host.invoke("EntitySystem.Remove",json!({"entity":s.entities[i]}))?;
                    let id=if let Some(packet)=water {
                        host.publish_water_entity(None,packet)?
                    }else{host.invoke("EntitySystem.Create",spec)?["entity"].as_u64().ok_or("water Entity identity required")?};
                    s.entities[i]=id;s.water_slots[i]=is_water;
                    instances.push(json!({"entity":id,"origin":origin}));continue;
                }
                let id = if let Some(packet)=water {
                    let existing=s.entities.get(i).copied();let id=host.publish_water_entity(existing,packet)?;if existing.is_none(){s.entities.push(id);}id
                } else if let Some(&id) = s.entities.get(i) {
                    spec["entity"] = json!(id);
                    host.invoke("EntitySystem.Update", spec)?;
                    id
                } else {
                    let id = host.invoke("EntitySystem.Create", spec)?["entity"]
                        .as_u64()
                        .ok_or("water Entity identity required")?;
                    s.entities.push(id);
                    id
                };
                if s.water_slots.len()<=i{s.water_slots.push(is_water);}
                instances.push(json!({"entity":id,"origin":origin}));
            }
            while s.entities.len() > instances.len() {
                let id = *s.entities.last().unwrap();
                host.invoke("EntitySystem.Remove", json!({"entity":id}))?;
                s.entities.pop();
                s.water_slots.pop();
            }
            s.instances = json!(instances);
            Ok(
                json!({"instances":s.instances,"water_entities":s.entities.len(),"triangles":indices.len()/3}),
            )
        }
        "Close" => {
            while let Some(&id) = s.entities.last() {
                host.invoke("EntitySystem.Remove", json!({"entity":id}))?;
                s.entities.pop();
            }
            s = Default::default();
            Ok(Value::Null)
        }
        _ => Err("unknown water presentation hook".into()),
    })();
    host.water.presentation = s;
    result
}
