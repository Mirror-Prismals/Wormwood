//! Application-owned continuous ambient scripts; no work runs in the callback.
use crate::{
    audio::{V, add, mul},
    host::Host,
};
use serde_json::{Value, json};
const LAYERS: [(&str, usize); 5] = [
    ("wind", 2),
    ("flow", 8),
    ("ocean", 9),
    ("crickets", 10),
    ("depths", 11),
];
#[derive(Default)]
pub(crate) struct State {
    requests: [Option<u64>; 5],
    entities: [Option<u64>; 5],
    water: Value,
    next_water: f64,
    microphone_water: Value,
    next_microphone_water: f64,
}
impl State {
    pub fn microphone_sources(&mut self,host:&mut Host,position:V,up:V,time:f64)->Result<Vec<Value>,String> {
        if self.microphone_water.is_null() || time>=self.next_microphone_water {
            self.microphone_water=if host.system_enabled("TerrainSurfaceHydrologySystem") {
                host.invoke("TerrainSurfaceHydrologySystem.AudioSources",json!({"position":position}))?
            } else {json!({})};
            self.next_microphone_water=time+0.25;
        }
        Ok(sources(&self.entities,position,up,&self.microphone_water))
    }
    pub fn submerged(&self) -> bool {
        self.water["submerged"] == true
    }
    pub fn snapshot(&self) -> Value {
        json!({"requests":self.requests,"entities":self.entities,"water":self.water})
    }

    pub fn close(&mut self, host: &mut Host) -> Result<(), String> {
        let mut first = None;
        for i in 0..5 {
            if let Some(request) = self.requests[i] {
                if host.chuck.prepared_entity().is_some() {
                    if let Err(e) = host.invoke(
                        "ChuckConnectionSystem.Remove",
                        json!({"request":request,"discard_events":true}),
                    ) {
                        first.get_or_insert(e);
                        continue;
                    }
                }
                self.requests[i] = None;
            }
            if let Some(entity) = self.entities[i] {
                match host.invoke("EntitySystem.Remove", json!({"entity":entity})) {
                    Ok(_) => self.entities[i] = None,
                    Err(e) => {
                        first.get_or_insert(e);
                    }
                }
            }
        }
        if let Some(e) = first {
            Err(e)
        } else {
            *self = Self::default();
            Ok(())
        }
    }
    pub fn step(
        &mut self,
        host: &mut Host,
        position: V,
        up: V,
        time: f64,
    ) -> Result<Vec<Value>, String> {
        // Keep each infinite script alive once per session. Routing gates its
        // audibility without repeatedly compiling or resetting its sound.
        for (i, (name, channel)) in LAYERS.iter().enumerate() {
            if self.entities[i].is_none() {
                self.entities[i] = Some(
                    host.invoke(
                        "EntitySystem.Create",
                        json!({"kind":"sound","source":format!("chuck:ambient/{name}")}),
                    )?["entity"]
                        .as_u64()
                        .ok_or("ambient Entity missing")?,
                );
            }
            if self.requests[i].is_none() {
                self.requests[i]=Some(host.invoke("ChuckConnectionSystem.Compile",json!({"script":format!("Procedures/chuck/ambient/{name}.ck"),"channel":channel}))?["request"].as_u64().ok_or("ambient request missing")?);
            }
        }
        let events = host.invoke(
            "ChuckConnectionSystem.Poll",
            json!({"requests":self.requests.iter().flatten().collect::<Vec<_>>()}),
        )?;
        for event in events["events"]
            .as_array()
            .ok_or("ambient events missing")?
        {
            if matches!(
                event["stage"].as_str(),
                Some("failed" | "completed" | "cancelled")
            ) {
                return Err(format!("ambient script stopped: {event}"));
            }
        }
        if self.water.is_null() || time >= self.next_water {
            self.water = if host.system_enabled("TerrainSurfaceHydrologySystem") {
                host.invoke(
                    "TerrainSurfaceHydrologySystem.AudioSources",
                    json!({"position":position}),
                )?
            } else {
                json!({})
            };
            self.next_water = time + 0.25;
        }
        Ok(sources(&self.entities, position, up, &self.water))
    }
}
fn sources(entities: &[Option<u64>; 5], position: V, up: V, water: &Value) -> Vec<Value> {
    let submerged = water["submerged"] == true;
    let mut sources = Vec::new();
    for (i, (_, channel)) in LAYERS.iter().enumerate() {
        let (p, gain) = match i {
            0 => (
                add(position, mul(up, 2.)),
                if submerged { 0.08 } else { 1. },
            ),
            1 | 2 => {
                let row = &water[if i == 1 { "flow" } else { "ocean" }];
                let Ok(p) = crate::audio::vector(&row["position"]) else {
                    continue;
                };
                (
                    p,
                    row["strength"].as_f64().unwrap_or(0.) * if submerged { 0.15 } else { 1. },
                )
            }
            // The original script includes daytime insects as well as crickets.
            3 => (
                add(position, mul(up, -1.)),
                if submerged { 0.02 } else { 1. },
            ),
            _ => (position, if submerged { 1. } else { 0. }),
        };
        if gain > 0. {
            if let Some(id) = entities[i] {
                sources.push(json!({"id":id,"channel":channel,"position":p,"ambient_gain":gain,"outdoor_wind":i==0}));
            }
        }
    }
    sources
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "macos")]
    #[test]
    fn ambient_scripts_compile_and_produce_pcm_on_each_bus() {
        use crate::audio::{
            chuck::{Event, Session},
            dsp::Route,
        };
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let (mut vm, mut output) = Session::prepare(root, 48000).unwrap();
        let mut requests = Vec::new();
        for (name, channel) in LAYERS {
            requests.push(
                vm.compile(&format!("Procedures/chuck/ambient/{name}.ck"), channel)
                    .unwrap(),
            );
        }
        let mut compiled = std::collections::BTreeSet::new();
        let mut peaks = [0f32; 5];
        let mut left = [0f32; 512];
        let mut right = [0f32; 512];
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        let mut slot = 0;
        while std::time::Instant::now() < deadline
            && (compiled.len() < 5 || peaks.iter().any(|p| *p < 1e-7))
        {
            let mut routes = [Route::default(); 17];
            routes[LAYERS[slot].1] = Route {
                active: true,
                gain: 1.,
                alpha: 1.,
                ..Route::default()
            };
            output.set_routes(routes);
            output.render(&mut left, &mut right);
            for sample in left.iter().chain(right.iter()) {
                assert!(sample.is_finite());
                peaks[slot] = peaks[slot].max(sample.abs());
            }
            for event in vm.events() {
                match event {
                    Event::Compiled { request, .. } => {
                        compiled.insert(request);
                    }
                    Event::Failed { error, .. } => panic!("{error}"),
                    _ => {}
                }
            }
            slot = (slot + 1) % 5;
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(compiled.len(), 5);
        assert!(peaks.iter().all(|p| *p > 1e-7), "ambient peaks: {peaks:?}");
        for request in requests {
            vm.remove(request).unwrap();
        }
        vm.close().unwrap();
    }
    #[test]
    fn microphone_wind_anchor_is_independent_of_player() {
        let ids=[Some(1),Some(2),Some(3),Some(4),Some(5)];
        let capsule=[0.,6.55,24.];let up=[0.,1.,0.];
        let mic=sources(&ids,capsule,up,&json!({}));
        let player=sources(&ids,[100.,20.,0.],up,&json!({}));
        assert_eq!(mic.iter().find(|s|s["channel"]==2).unwrap()["position"],json!([0.,8.55,24.]));
        assert_ne!(mic,player);
        assert_eq!(mic,sources(&ids,capsule,up,&json!({})));
    }
    #[test]
    fn dry_land_has_no_water_audio_and_submersion_switches_depths() {
        let ids = [Some(1), Some(2), Some(3), Some(4), Some(5)];
        let dry = sources(&ids, [0., 10., 0.], [0., 1., 0.], &json!({}));
        assert_eq!(
            dry.iter()
                .map(|s| s["channel"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![2, 10]
        );
        let wet = sources(
            &ids,
            [0., 10., 0.],
            [0., 1., 0.],
            &json!({"submerged":true,"flow":{"position":[1.,10.,0.],"strength":0.5}}),
        );
        assert!(wet.iter().any(|s| s["channel"] == 11));
        assert!(!wet.iter().any(|s| s["channel"] == 9));
        assert_eq!(
            wet.iter().find(|s| s["channel"] == 8).unwrap()["ambient_gain"],
            0.075
        );
    }
}
