//! Layer-two acoustic computation. No external host or application event ownership.
use crate::audio::{
    acoustic::{self, Listener, Source},
    geometry::Snapshot,
    vector,
};
use serde_json::{Value, json};
#[derive(Default)]
pub(crate) struct State {
    pub engine: acoustic::State,
    pub(crate) wind_cover: super::terrain_audio::WindCover,
    pub microphone: acoustic::State,
    pub microphone_terrain: Option<Box<State>>,
    pub terrain_geometry: Option<std::sync::Arc<crate::terrain::material_geometry::Surface>>,
    pub(crate) terrain_worker: Option<super::terrain_audio::Worker>,
    pub(crate) terrain_rig: Vec<Option<crate::audio::geometry::Hit>>,
}
pub(crate) fn listener(v: &Value) -> Result<Listener, String> {
    Ok(Listener {
        position: vector(&v["position"])?,
        forward: vector(&v["forward"])?,
        up: vector(&v["up"])?,
    })
}
pub(crate) fn response_value(r: &acoustic::Response) -> Value {
    json!({"id":r.id,"channel":r.channel,"position":r.source,"gain":r.gain,"pre_falloff":r.pre_falloff,"occluded":r.occluded,"direction":r.direction,"pan":r.pan,"hf_alpha":r.hf_alpha,"echo_delay":r.echo_delay,"echo_gain":r.echo_gain,"escape":r.escape})
}
impl State {
    pub fn dispatch(&mut self, hook: &str, input: &Value) -> Result<Value, String> {
        match hook {
            "Advance" | "AdvanceMicrophone" => {
                let g = Snapshot::parse(&input["geometry"])?;
                let l = listener(&input["listener"])?;
                let rows = input["sources"]
                    .as_array()
                    .ok_or("audio sources required")?;
                if rows.len() > 17 {
                    return Err("acoustic source budget exceeded".into());
                }
                let mut sources = Vec::with_capacity(rows.len());
                for row in rows {
                    sources.push(Source {
                        id: row["id"].as_u64().ok_or("source id required")?,
                        channel: row["channel"]
                            .as_u64()
                            .filter(|n| *n < 17)
                            .ok_or("source channel required")?
                            as usize,
                        position: vector(&row["position"])?,
                    });
                }
                let engine = if hook == "AdvanceMicrophone" {
                    &mut self.microphone
                } else {
                    &mut self.engine
                };
                engine.advance(
                    &g,
                    l,
                    &sources,
                    input["time"].as_f64().ok_or("audio time required")?,
                    input["metres_per_unit"].as_f64().unwrap_or(1.),
                )?;
                if hook == "AdvanceMicrophone" {
                    Ok(snapshot(&self.microphone))
                } else {
                    Ok(self.snapshot())
                }
            }
            "Snapshot" => Ok(self.snapshot()),
            "Invalidate" => {
                // Join off-callback computation before clearing accepted output.
                self.terrain_worker = None;
                self.terrain_rig.clear();
                self.engine.invalidate();
                self.microphone.invalidate();
                self.microphone_terrain = None;
                self.terrain_geometry = None;
                self.wind_cover = Default::default();
                Ok(Value::Null)
            }
            _ => Err("unknown ray audio hook".into()),
        }
    }
    pub fn snapshot(&self) -> Value {
        snapshot(&self.engine)
    }
}
fn snapshot(engine: &acoustic::State) -> Value {
    json!({"version":engine.version,"batch_active":engine.batch_active(),"escape":engine.escape,"sources":engine.responses.values().map(response_value).collect::<Vec<_>>(),"segments":engine.debug.iter().map(|s|json!({"from":s.from,"to":s.to,"source":s.source,"kind":s.kind})).collect::<Vec<_>>()})
}
