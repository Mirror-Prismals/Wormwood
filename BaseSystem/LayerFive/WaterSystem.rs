//! Water application lifetime, input edits and continuous animation Audicle.
use crate::{
    host::Host,
    water::{
        math::*,
        types::{number, vector},
    },
};
use serde_json::{Value, json};
#[derive(Clone, Copy)]
struct Ripple {
    position: V,
    age: f64,
    strength: f64,
}
#[derive(Default)]
pub(crate) struct State {
    event: Option<Value>,
    configured: bool,
    paused: bool,
    time: f64,
    course_start: Option<u64>,
    editor_dirty: bool,
    ripples: Vec<Ripple>,
    view: Value,
    gate_was_down:bool,
}
fn load(host: &Host, name: &str) -> Result<Value, String> {
    serde_json::from_slice(
        &std::fs::read(host.root().join("Procedures/Water").join(name))
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
fn load_editor(host: &Host) -> Result<Option<Value>, String> {
    let path = host.output_root().join("water-editor.json");
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("read water editor {}: {e}", path.display())),
    };
    if bytes.len() > 2_000_000 {
        return Err("water editor save exceeds bounded graph size".into());
    }
    let saved: Value = serde_json::from_slice(&bytes)
        .map_err(|e| format!("water editor save {}: {e}", path.display()))?;
    if saved["schema_version"] != 1 {
        return Err("unsupported water editor save version; file preserved".into());
    }
    Ok(Some(saved))
}
fn save_editor(host: &mut Host, state: &State) -> Result<(), String> {
    use std::io::Write;
    if !state.editor_dirty {
        return Ok(());
    }
    let slides = host.invoke("WaterSlideSystem.Snapshot", Value::Null)?;
    let bytes = serde_json::to_vec_pretty(
        &json!({"schema_version":1,"slides":slides,"course_start":state.course_start}),
    )
    .map_err(|e| e.to_string())?;
    let directory = host.output_root();
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let pending = directory.join(format!(
        "water-editor-{}-{stamp}.pending",
        std::process::id()
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| format!("write water editor {}: {e}", pending.display()))?;
    std::fs::rename(&pending, directory.join("water-editor.json"))
        .map_err(|e| format!("commit water editor (pending preserved): {e}"))
}
fn start(host: &mut Host, s: &mut State) -> Result<(), String> {
    let event = host.invoke("AudicleSystem.Create", Value::Null)?;
    s.event = Some(event.clone());
    host.active_audicle = event["audicle"].as_u64();
    host.invoke("AudicleSystem.Ready", event.clone())?;
    host.invoke("AudicleSystem.Go", event)?;
    Ok(())
}
fn close_components(host: &mut Host) -> Result<(), String> {
    let mut errors = Vec::new();
    for system in [
        "WaterRenderSystem",
        "FloodGateSystem",
        "SoftBodyWaterSystem",
        "TerrainSurfaceHydrologySystem",
        "WaterSlideSystem",
        "WaterFlowSystem",
        "OceanWaveSystem",
        "TideSystem",
    ] {
        if let Err(error) = host.invoke(&format!("{system}.Close"), Value::Null) {
            errors.push(format!("{system}: {error}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let outer = host.active_audicle;
    let mut s = std::mem::take(&mut host.water.application);
    let was_configured = s.configured;
    host.active_audicle = s
        .event
        .as_ref()
        .and_then(|e| e["audicle"].as_u64())
        .or(outer);
    let result = (|| {
        if hook != "Configure" {
            if let Some(event) = &s.event {
                host.invoke("AudicleSystem.Resume", event.clone())?;
            }
        }
        match hook {
            "Configure" => {
                if s.configured {
                    return Err("water already configured".into());
                }
                let mut settings = load(host, "settings.json")?;
                // Validate the authored document before indexing it to merge a
                // save, so malformed configuration reports an ordinary error.
                crate::water::config::Settings::parse(&settings)?;
                let mut bodies = load(host, "bodies.json")?;
                if let Some(park)=host.theatre.venue.as_ref().and_then(|v|v.park.as_ref()) {
                    bodies["pools"]=json!(park.pools.iter().map(|(min,max)|json!({"min":crate::audio::add(*min,host.theatre.origin),"max":crate::audio::add(*max,host.theatre.origin)})).collect::<Vec<_>>());
                    bodies["sheets"]=json!(park.water_sheets(host.theatre.origin));
                    let c=crate::terrain::nightworld::shaft();let r=crate::terrain::RADIUS;
                    bodies["pools"].as_array_mut().ok_or("pool array required")?.push(json!({"min":[c[0]-4.7,-(r-94.6),c[2]-4.7],"max":[c[0]+4.7,-(r-95.6),c[2]+4.7]}));
                    for source in park.sources(host.theatre.origin) {
                        bodies["sources"].as_array_mut().ok_or("water source array required")?.push(json!({"id":source.id,"frame":{"origin":source.frame.origin,"right":source.frame.right,"up":source.frame.up,"forward":source.frame.forward},"min":source.min,"max":source.max,"target":source.target,"velocity":source.velocity}));
                    }
                    host.theatre.flow.step(park,0.);
                }
                let mut slides = load(host, "slides.json")?;
                if let Some(saved) = load_editor(host)? {
                    slides = saved["slides"].clone();
                    settings["floodgate"]["course_start"] = saved["course_start"].clone();
                }
                let parsed = crate::water::config::Settings::parse(&settings)?;
                let mut validated_graph = crate::water::slides::State::default();
                validated_graph.settings = parsed.slides.clone();
                validated_graph.configure(&slides)?;
                s.course_start = parsed.floodgate.course_start;
                crate::water::bodies::State::parse(&bodies)?;
                let event = host.invoke("AudicleSystem.Create", Value::Null)?;
                s.event = Some(event.clone());
                host.active_audicle = event["audicle"].as_u64();
                for system in [
                    "TideSystem",
                    "OceanWaveSystem",
                    "WaterFlowSystem",
                    "SoftBodyWaterSystem",
                    "FloodGateSystem",
                ] {
                    host.invoke(&format!("{system}.Configure"), settings.clone())?;
                }
                host.invoke(
                    "WaterSlideSystem.Configure",
                    json!({"settings":settings,"slides":slides}),
                )?;
                host.invoke("TerrainSurfaceHydrologySystem.Configure", bodies)?;
                host.invoke("WaterRenderSystem.Prepare", settings)?;
                host.invoke("AudicleSystem.Ready", event.clone())?;
                host.invoke("AudicleSystem.Go", event)?;
                s.configured = true;
                s.view = json!({"instances":[],"active":true,"continuous_animation":true});
                Ok(s.view.clone())
            }
            "Step" => {
                if !s.configured {
                    return Err("water not configured".into());
                }
                if s.paused {
                    return Ok(s.view.clone());
                }
                let delta = number(input, "dt", 0., 0., 0.1)?;
                vector(&input["position"])?;
                vector(&input["forward"])?;
                if input["snapshot"].is_null() {
                    let snapshot = host.water_collision_snapshot(vector(&input["position"])?)?;
                    host.update_water_collision(snapshot)?;
                } else {
                    host.invoke("TerrainSurfaceHydrologySystem.Update",json!({"snapshot":input["snapshot"]}))?;
                }
                host.invoke(
                    "TideSystem.Step",
                    json!({"dt":delta,"world_age":input["world_age"]}),
                )?;
                host.invoke("OceanWaveSystem.Step", json!({"dt":delta}))?;
                host.invoke("WaterFlowSystem.Step", json!({"dt":delta}))?;
                if let Some(park)=host.theatre.venue.as_ref().and_then(|v|v.park.as_ref()) {
                    host.theatre.flow.step(park,delta);
                    host.water.hydrology.bodies.sheets=park.flowing_sheets(host.theatre.origin,Some(&host.theatre.flow));
                    host.water.hydrology.surface_cache=None;
                    // Jets return to the sump once landed; their bounded source
                    // allocation then feeds the next physically simulated parcel.
                    host.water.soft_body.parcels.retain(|p|!p.source.is_some_and(|id|(9000..9005).contains(&id)) || (!p.resting && p.position[1]>host.theatre.origin[1]+park.offset[1]+0.2));
                    let held=input["park_gate"]==true;
                    if held && !s.gate_was_down {
                        let position=sub(vector(&input["position"])?,host.theatre.origin);
                        if let Some((i,_))=park.rides.iter().enumerate().map(|(i,path)|(i,len(sub(position,path[0])))).filter(|(_,d)|*d<5.).min_by(|a,b|a.1.total_cmp(&b.1)) {
                            host.theatre.flow.gate_open[i]=!host.theatre.flow.gate_open[i];
                        }
                    }
                    s.gate_was_down=held;
                }
                let gates = host.invoke("FloodGateSystem.Step", Value::Null)?;
                let parcels = host.invoke("SoftBodyWaterSystem.Step", json!({"dt":delta}))?;
                s.time += delta.min(0.05);
                for r in &mut s.ripples {
                    r.age += delta;
                }
                s.ripples.retain(|r| r.age <= 4.5);
                let ripples: Vec<_> = s
                    .ripples
                    .iter()
                    .map(|r| json!({"position":r.position,"age":r.age,"strength":r.strength}))
                    .collect();
                let render=host.invoke("WaterRenderSystem.Update",json!({"position":input["position"],"forward":input["forward"],"view":input["view"],"time":s.time,"ripples":ripples}))?;
                s.view = json!({"instances":render["instances"],"active":true,"continuous_animation":true,"coverage_needed":parcels["coverage_needed"]==true||gates["coverage_needed"]==true,"coverage_points":{"parcels":parcels["coverage_points"],"gates":gates["coverage_points"]},"parcel_count":parcels["parcel_count"],"conserved":parcels["conserved"],"whitewater":parcels["whitewater"],"gates":gates,"consumed_dt":delta.min(0.05),"time":s.time});
                Ok(s.view.clone())
            }
            "Frame" => {
                let mut view = if s.view.is_object() {
                    s.view.clone()
                } else {
                    json!({"instances":[]})
                };
                view["configured"] = json!(s.configured);
                view["paused"] = json!(s.paused);
                Ok(view)
            }
            "Sample" => {
                let surface = host.invoke("TerrainSurfaceHydrologySystem.Sample", input.clone())?;
                let flow = host.invoke("WaterFlowSystem.Sample", input.clone())?;
                let particles = host.invoke("SoftBodyWaterSystem.Sample", input.clone())?;
                let (film,film_current)=host.theatre.venue.as_ref().and_then(|v|v.park.as_ref()).map_or((false,[0.;3]),|p|p.film_sample(&host.theatre.flow,sub(vector(&input["position"]).unwrap_or([0.;3]),host.theatre.origin)));
                let current=if film {json!(film_current)}else if particles["occupied"]==true {particles["velocity"].clone()}else{flow["velocity"].clone()};
                Ok(
                    json!({"occupied":film||surface["occupied"]==true||particles["occupied"]==true,"coverage_known":surface["coverage_known"],"depth":flow["depth"],"surface_position":surface["surface_position"],"current":current,"strength":if film||particles["occupied"]==true{json!(1.)}else{flow["strength"].clone()},"kind":flow["kind"],"particle_current":particles["velocity"],"whitewater":particles["whitewater"]}),
                )
            }
            "Disturb" => {
                let position = vector(&input["position"])?;
                let strength = number(input, "strength", 1., 0., 100.)?;
                if s.ripples.len() == 64 {
                    s.ripples.remove(0);
                }
                s.ripples.push(Ripple {
                    position,
                    age: 0.,
                    strength,
                });
                host.invoke("SoftBodyWaterSystem.Disturb", input.clone())
            }
            "Edit" => {
                if !s.configured {
                    return Err("water not configured".into());
                }
                if input["operation"] == "source" {
                    let start = if let Some(start) = input.get("start") {
                        start.clone()
                    } else {
                        // Reuse the editor's accepted ray rather than requiring
                        // the player to know an internal tie ID.
                        let hit = host.invoke("TerrainSurfaceHydrologySystem.Raycast",json!({"position":input["position"],"direction":input["direction"],"distance":8.}))?;
                        if hit.is_null() {
                            return Ok(json!({"source_selected":false}));
                        }
                        let selected = host.invoke("WaterSlideSystem.Edit",json!({"operation":"select","anchor":hit["position"],"max_distance":2.}))?;
                        selected["selected"].clone()
                    };
                    let selected = host.invoke(
                        "FloodGateSystem.Configure",
                        json!({"operation":"select_course","start":start}),
                    )?;
                    s.course_start = selected["course_start"].as_u64();
                    s.editor_dirty = true;
                    return Ok(selected);
                }
                let mut edit = input.clone();
                if input["operation"] == "place" && input["anchor"].is_null() {
                    let hit=host.invoke("TerrainSurfaceHydrologySystem.Raycast",json!({"position":input["position"],"direction":input["direction"],"distance":8.}))?;
                    if hit.is_null() {
                        return Ok(json!({"placed":false}));
                    }
                    edit["anchor"] = hit["position"].clone();
                    edit["normal"] = hit["normal"].clone();
                }
                let result = host.invoke("WaterSlideSystem.Edit", edit)?;
                s.editor_dirty = true;
                if input["operation"] == "clear" && input["all"] == true {
                    host.invoke(
                        "FloodGateSystem.Configure",
                        json!({"operation":"select_course","start":null}),
                    )?;
                    s.course_start = None;
                }
                host.invoke("TerrainSurfaceHydrologySystem.Update", json!({}))?;
                Ok(result)
            }
            "Pause" => {
                let paused = input["paused"]
                    .as_bool()
                    .ok_or("water pause boolean required")?;
                if paused && !s.paused {
                    if let Some(event) = s.event.clone() {
                        host.invoke("AudicleSystem.Finish", event)?;
                        s.event = None;
                    }
                } else if !paused && s.paused {
                    start(host, &mut s)?;
                }
                s.paused = paused;
                s.view["active"] = json!(!paused);
                s.view["continuous_animation"] = json!(!paused);
                Ok(s.view.clone())
            }
            "Close" => {
                host.water_collision_preparation=None;
                if s.configured {
                    save_editor(host, &s)?;
                }
                close_components(host)?;
                if let Some(event) = s.event.clone() {
                    host.invoke("AudicleSystem.Finish", event)?;
                    s.event = None;
                }
                s = Default::default();
                Ok(Value::Null)
            }
            _ => Err("unknown water application hook".into()),
        }
    })();
    let mut result = result;
    if result.is_err()
        && ((hook == "Configure" && !was_configured) || matches!(hook, "Step" | "Close"))
    {
        let error = result.as_ref().unwrap_err().to_string();
        let cleanup = close_components(host);
        if let Some(event) = s.event.clone() {
            match host.invoke(
                "AudicleSystem.Fail",
                json!({"audicle":event["audicle"],"reason":error}),
            ) {
                Ok(_) => s.event = None,
                Err(failure) => result = Err(format!("{error}; water failure tracking: {failure}")),
            }
        }
        if let Err(cleanup) = cleanup {
            result = Err(format!("{error}; water cleanup: {cleanup}"));
        } else if s.event.is_none() {
            s = Default::default();
        }
    }
    host.active_audicle = outer.filter(|id| host.audicles.live(*id));
    host.water.application = s;
    result
}
