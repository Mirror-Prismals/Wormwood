//! Sight/sound data only. No World, Level, System, or Audicle references.
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub(crate) struct Entity {
    pub id: u64,
    pub appearance: Appearance,
}

#[derive(Debug, Clone)]
pub(crate) enum Appearance {
    Visual {
        vertices: Vec<[f32; 6]>,
        shader: String,
    },
    Sound {
        source: String,
    },
}

impl Entity {
    // The first supported visual format: position xyz and color rgb per vertex.
    // This is a small implemented format, not the final limit of BaseEntity.
    pub(crate) fn from_spec(id: u64, spec: &Value) -> Result<Self, String> {
        let appearance = match spec["kind"].as_str() {
            Some("visual") => {
                let rows = spec["vertices"]
                    .as_array()
                    .ok_or("vertices must be an array")?;
                if rows.is_empty() || rows.len() % 3 != 0 || rows.len() > 4095 {
                    return Err("visual mesh requires 1 to 1365 triangles".into());
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
                    .filter(|s| matches!(*s, "vertex_color" | "water_volume" | "planet_surface"))
                    .ok_or("unsupported initial shader")?
                    .to_owned();
                Appearance::Visual { vertices, shader }
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
        Ok(Self { id, appearance })
    }

    pub(crate) fn data(&self) -> Value {
        match &self.appearance {
            Appearance::Visual { vertices, shader } => {
                json!({"id":self.id,"kind":"visual","vertices":vertices,"shader":shader})
            }
            Appearance::Sound { source } => json!({"id":self.id,"kind":"sound","source":source}),
        }
    }
}
