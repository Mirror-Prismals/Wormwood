//! Sight/sound data only. No World, Level, System, or Audicle references.
use serde_json::{Value, json};

/// A workshop material appearance includes the worker, tool and specimen.
/// Keep its combined skin bounded without dropping the worker or cut faces.
/// Composed scene and reflection bounds are shared in scene_limits.
pub(crate) fn visual_vertex_limit(shader: &str) -> usize {
    if shader == "material_surface" { 98304 } else { 24576 }
}

#[derive(Debug, Clone)]
pub(crate) struct Entity {
    pub id: u64,
    pub appearance: Appearance,
    /// Accepted world-space material skin. Native consumers share immutable
    /// numeric data; data() retains the JSON envelope for explicit callers.
    pub material_vertices: Option<std::sync::Arc<Vec<[f64;6]>>>,
    /// Immutable validated player UV/head/texture metadata, shared across poses.
    /// data() preserves the ordinary public JSON appearance envelope.
    pub shared_procedural:Option<std::sync::Arc<Value>>,
    pub water_packet:Option<std::sync::Arc<crate::water::presentation::Packet>>,
}

#[derive(Debug, Clone)]
pub(crate) enum Appearance {
    Visual {
        vertices: Vec<[f32; 6]>,
        shader: String,
        procedural: Value,
    },
    Sound {
        source: String,
    },
}

impl Entity {
    pub(crate) fn shader(&self) -> Option<&str> {
        match &self.appearance { Appearance::Visual {shader,..}=>Some(shader), Appearance::Sound {..}=>None }
    }
    // The first supported visual format: position xyz and color rgb per vertex.
    // This is a small implemented format, not the final limit of BaseEntity.
    pub(crate) fn from_spec(id: u64, spec: &Value) -> Result<Self, String> {
        let appearance = match spec["kind"].as_str() {
            Some("visual") => {
                let rows = spec["vertices"]
                    .as_array()
                    .ok_or("vertices must be an array")?;
                let limit = visual_vertex_limit(spec["shader"].as_str().unwrap_or(""));
                if rows.is_empty() || rows.len() % 3 != 0 || rows.len() > limit {
                    return Err(format!("visual mesh requires 1 to {} triangles", limit / 3));
                }
                let mut vertices = Vec::new();
                for row in rows {
                    let row = row
                        .as_array()
                        .filter(|v| v.len() == 6)
                        .ok_or("vertex requires xyz and rgb")?;
                    let mut vertex = [0.0; 6];
                    for (i, value) in row.iter().enumerate() {
                        vertex[i] = value.as_f64().ok_or("vertex must be numeric")? as f32;
                        if !vertex[i].is_finite() {
                            return Err("vertex must be finite".into());
                        }
                    }
                    vertices.push(vertex);
                }
                let shader = spec["shader"]
                    .as_str()
                    .filter(|s| {
                        matches!(
                            *s,
                            "vertex_color"
                                | "workstation_panel"
                                | "planet_surface"
                                | "material_surface"
                                | "unlit_surface"
                                | "player_body"
                                | "color_emotion"
                                | "water_surface"
                        )
                    })
                    .ok_or("unsupported initial shader")?
                    .to_owned();
                let procedural = if shader == "water_surface" {
                    if !crate::water_render_data::valid_packet(&spec["procedural"]) {
                        return Err("invalid water appearance packet".into());
                    }
                    spec["procedural"].clone()
                } else if shader == "color_emotion" {
                    let p = &spec["procedural"];
                    if vertices.len() != 6
                        || !p.as_object().is_some_and(|o| o.len() == 4)
                        || !p["intensity"]
                            .as_f64()
                            .is_some_and(|x| x.is_finite() && (0.0..=1.).contains(&x))
                        || !p["time"]
                            .as_f64()
                            .is_some_and(|x| x.is_finite() && (0.0..=1000.).contains(&x))
                        || !p["charge"]
                            .as_f64()
                            .is_some_and(|x| x.is_finite() && (0.0..=1.).contains(&x))
                        || !matches!(p["mode"].as_str(), Some("pickup" | "dig"))
                    {
                        return Err("invalid color emotion appearance".into());
                    }
                    p.clone()
                } else if shader == "player_body" {
                    let p = &spec["procedural"];
                    if !p.as_object().is_some_and(|o| o.len() == 3)
                        || !p["uvs"].as_array().is_some_and(|a| {
                            a.len() == vertices.len()
                                && a.iter().all(|v| {
                                    v.as_array().is_some_and(|r| {
                                        r.len() == 2
                                            && r.iter().all(|n| {
                                                n.as_f64()
                                                    .is_some_and(|x| x.is_finite() && x.abs() < 1e6)
                                            })
                                    })
                                })
                        })
                        || !p["head_triangles"].as_array().is_some_and(|a| {
                            a.len() == vertices.len() / 3 && a.iter().all(Value::is_boolean)
                        })
                        || p["texture"] != "Procedures/Assets/Player/prismal_player.rgba"
                    {
                        return Err("invalid player appearance".into());
                    }
                    p.clone()
                } else if shader == "planet_surface" {
                    let data = if spec["procedural"].is_null() {
                        json!({"edits":[],"debris":[]})
                    } else {
                        spec["procedural"].clone()
                    };
                    if !data.as_object().is_some_and(|v| {
                        (2..=3).contains(&v.len())
                            && v.keys()
                                .all(|k| ["edits", "debris", "underground"].contains(&k.as_str()))
                    }) {
                        return Err("planet appearance allows only edits, debris and optional underground geometry".into());
                    }
                    crate::terrain::underground::Field::parse(&data["underground"])?;
                    let edits = &data["edits"];
                    if !crate::terrain::tool_cut::valid_edits(edits) {
                        return Err("invalid terrain cut appearance".into());
                    }
                    let debris = data["debris"]
                        .as_array()
                        .filter(|v| v.len() <= 300000 && v.len() % 3 == 0)
                        .ok_or("invalid fragment appearance budget")?;
                    if debris.iter().any(|v| {
                        !v.as_array().is_some_and(|v| {
                            v.len() == 6
                                && v.iter().all(|v| {
                                    v.as_f64().is_some_and(|v| v.is_finite() && v.abs() < 1e10)
                                })
                        })
                    }) {
                        return Err("invalid fragment appearance vertices".into());
                    }
                    let mut result = json!({"edits":edits,"debris":debris});
                    if !data["underground"].is_null() {
                        result["underground"] = data["underground"].clone();
                    }
                    result
                } else {
                    if !spec["procedural"].is_null() {
                        return Err(
                            "procedural appearance is only supported by the planet shader".into(),
                        );
                    }
                    Value::Null
                };
                Appearance::Visual {
                    vertices,
                    shader,
                    procedural,
                }
            }
            Some("sound") => Appearance::Sound {
                source: spec["source"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or("sound source required")?
                    .to_owned(),
            },
            _ => return Err("unknown appearance kind".into()),
        };
        let mut appearance=appearance;
        let shared_procedural=match &mut appearance {
            Appearance::Visual{shader,procedural,..} if shader=="player_body"=>Some(std::sync::Arc::new(std::mem::take(procedural))),
            _=>None,
        };
        Ok(Self { id, appearance, material_vertices:None,shared_procedural,water_packet:None })
    }

    pub(crate) fn visual_procedural(&self)->Option<&Value>{
        match &self.appearance{Appearance::Visual{procedural,..}=>Some(self.shared_procedural.as_deref().unwrap_or(procedural)),_=>None}
    }
    pub(crate) fn data(&self) -> Value {
        match &self.appearance {
            Appearance::Visual {
                vertices,
                shader,
                procedural,
            } => {
                let mut data=json!({"id":self.id,"kind":"visual","vertices":vertices,"shader":shader,"procedural":self.shared_procedural.as_deref().unwrap_or(procedural)});
                if let Some(water)=&self.water_packet {data["procedural"]=water.json();}
                if let Some(material)=&self.material_vertices {data["procedural"]["debris"]=json!(material.as_ref());}
                data
            }
            Appearance::Sound { source } => json!({"id":self.id,"kind":"sound","source":source}),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cave_snapshot_survives_planet_entity_appearance_validation() {
        let caves: Value = serde_json::from_str(include_str!("Procedures/caves.json")).unwrap();
        let mut spec = json!({"kind":"visual","shader":"planet_surface","vertices":[[0.,0.,0.,1.,1.,1.],[1.,0.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]],"procedural":{"edits":[],"debris":[],"underground":caves}});
        let entity = Entity::from_spec(1, &spec).unwrap();
        assert_eq!(
            entity.data()["procedural"]["underground"],
            spec["procedural"]["underground"]
        );
        spec["procedural"]["underground"] = json!({"schema_version":99,"enabled":true});
        assert!(Entity::from_spec(1, &spec).is_err());
    }
    #[test]
    fn procedural_entity_appearance_is_data_with_bounded_geometry_and_no_hidden_fields() {
        let mut spec = json!({"kind":"visual","shader":"planet_surface","vertices":[[0.,0.,0.,1.,1.,1.],[1.,0.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]],"procedural":{"edits":[{"normal":[0.,1.,0.],"radius":2.,"depth":0.8}],"debris":[]}});
        let entity = Entity::from_spec(1, &spec).unwrap();
        assert_eq!(entity.data()["procedural"], spec["procedural"]);
        spec["procedural"]["other_entity"] = json!(42);
        assert!(Entity::from_spec(1, &spec).is_err());
        spec["procedural"]
            .as_object_mut()
            .unwrap()
            .remove("other_entity");
        spec["procedural"]["edits"][0]["depth"] = json!(200.);
        assert!(Entity::from_spec(1, &spec).is_err());
    }
}
