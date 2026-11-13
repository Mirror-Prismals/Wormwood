//! Four independent bird appearances, flights and finite ChucK-call Audicles.
use crate::{
    audio::{
        birds::{Action, Flock, SCRIPTS},
        vector,
    },
    host::Host,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Default)]
struct Live {
    event: Option<Value>,
    flight: Option<Value>,
    visual: Option<u64>,
    sound: Option<u64>,
    position: [f64; 3],
    forward: [f64; 3],
    flight_blend: f64,
    calling: f64,
}
#[derive(Default)]
pub(crate) struct State {
    flock: Flock,
    live: [Live; 4],
    calls: BTreeMap<u64, (usize, Value)>,
    time: f64,
    up: [f64; 3],
}
fn activate(host: &mut Host, event: &Value) {
    host.active_audicle = event["audicle"].as_u64();
}
fn begin(host: &mut Host) -> Result<Value, String> {
    let e = host.invoke("AudicleSystem.Create", Value::Null)?;
    Ok(e)
}
fn go(host: &mut Host, e: &Value) -> Result<(), String> {
    activate(host, e);
    host.invoke("AudicleSystem.Ready", e.clone())?;
    host.invoke("AudicleSystem.Go", e.clone())?;
    Ok(())
}
fn end(host: &mut Host, event: &mut Option<Value>, cancel: bool) -> Result<(), String> {
    if let Some(e) = event.clone() {
        activate(host, &e);
        host.invoke(
            if cancel {
                "AudicleSystem.Cancel"
            } else {
                "AudicleSystem.Finish"
            },
            e,
        )?;
        *event = None;
    }
    Ok(())
}

impl State {
    fn despawn(&mut self, host: &mut Host, slot: usize) -> Result<(), String> {
        let mut first = None;
        let keys: Vec<_> = self
            .calls
            .iter()
            .filter(|(_, (s, _))| *s == slot)
            .map(|(k, _)| *k)
            .collect();
        for k in keys {
            let Some((_, event)) = self.calls.get(&k).cloned() else {
                continue;
            };
            activate(host, &event);
            if host.chuck.prepared_entity().is_some() {
                if let Err(error) = host.invoke(
                    "ChuckConnectionSystem.Remove",
                    json!({"request":k,"discard_events":true}),
                ) {
                    first.get_or_insert(error);
                    continue;
                }
            }
            let mut event = Some(event);
            match end(host, &mut event, true) {
                Ok(()) => {
                    self.calls.remove(&k);
                }
                Err(error) => {
                    first.get_or_insert(error);
                }
            }
        }
        let b = &mut self.live[slot];
        if let Err(error) = end(host, &mut b.flight, true) {
            first.get_or_insert(error);
        }
        if let Some(event) = &b.event {
            activate(host, event);
        }
        for id in [&mut b.visual, &mut b.sound] {
            if let Some(entity) = *id {
                match host.invoke("EntitySystem.Remove", json!({"entity":entity})) {
                    Ok(_) => *id = None,
                    Err(error) => {
                        first.get_or_insert(error);
                    }
                }
            }
        }
        if first.is_none() {
            if let Err(error) = end(host, &mut b.event, true) {
                first = Some(error);
            }
        }
        if let Some(error) = first {
            Err(error)
        } else {
            *b = Live::default();
            Ok(())
        }
    }
    fn spawn(&mut self, host: &mut Host, slot: usize, p: [f64; 3]) -> Result<(), String> {
        let e = begin(host)?;
        self.live[slot].event = Some(e.clone());
        let result = (|| {
            activate(host, &e);
            let visual = host.invoke(
                "EntitySystem.Create",
                crate::base_system::layer_four::cardinal_rig::spec(),
            )?["entity"]
                .as_u64()
                .ok_or("bird visual Entity missing")?;
            self.live[slot].visual = Some(visual);
            let sound = host.invoke(
                "EntitySystem.Create",
                json!({"kind":"sound","source":format!("chuck:birds/{}",SCRIPTS[slot])}),
            )?["entity"]
                .as_u64()
                .ok_or("bird sound Entity missing")?;
            self.live[slot].sound = Some(sound);
            self.live[slot].position = p;
            let reference = if self.up[1].abs() < 0.9 {
                [0., 1., 0.]
            } else {
                [1., 0., 0.]
            };
            self.live[slot].forward = crate::audio::unit(crate::audio::cross(reference, self.up));
            go(host, &e)
        })();
        if result.is_err() {
            let _ = self.despawn(host, slot);
        }
        result
    }
    fn poll(&mut self, host: &mut Host) -> Result<(), String> {
        let events = host.invoke(
            "ChuckConnectionSystem.Poll",
            json!({"requests":self.calls.keys().copied().collect::<Vec<_>>()}),
        )?;
        if events["telemetry"]["render_errors"].as_u64().unwrap_or(0) > 0 {
            return Err("embedded ChucK rendering failed".into());
        }
        for row in events["events"]
            .as_array()
            .ok_or("ChucK completion events required")?
        {
            let Some(id) = row["request"].as_u64() else {
                continue;
            };
            match row["stage"].as_str() {
                Some("completed") | Some("failed") | Some("cancelled") => {
                    if let Some((_, event)) = self.calls.get(&id).cloned() {
                        let mut event = Some(event);
                        end(host, &mut event, row["stage"] != "completed")?;
                        self.calls.remove(&id);
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn step(&mut self, host: &mut Host, input: &Value) -> Result<Value, String> {
        let p = vector(&input["position"])?;
        let up = vector(&input["up"])?;
        if (crate::audio::length(up) - 1.).abs() > 1e-4 {
            return Err("unit bird up required".into());
        }
        self.flock.set_up(up);
        self.up = up;
        let dt = input["dt"].as_f64().ok_or("bird dt required")?;
        if !dt.is_finite() || !(0.0..=0.25).contains(&dt) {
            return Err("bird dt outside bounded frame interval".into());
        }
        self.time = input["time"]
            .as_f64()
            .filter(|t| t.is_finite())
            .unwrap_or(self.time + dt);
        self.poll(host)?;
        // Refresh only occupied branches each frame. Bounded habitat discovery
        // stays on its separate two-second interval.
        let mut actions = Vec::new();
        if let Some(request) = self.flock.tracking_query(self.time) {
            let current = host.invoke("VegetationSystem.QueryPerches", request)?;
            actions.extend(self.flock.refresh(&current)?);
        }
        let query = if self.flock.needs_discovery(p, dt) {
            let mut request = self.flock.query(p, up);
            request["time"] = json!(self.time);
            Some(host.invoke("VegetationSystem.QueryPerches", request)?)
        } else {
            None
        };
        actions.extend(self.flock.step(p, dt, query.as_ref())?);
        for action in actions {
            match action {
                Action::Spawn(slot, p) => self.spawn(host, slot, p)?,
                Action::Despawn(slot) => self.despawn(host, slot)?,
                Action::Move(slot, p) => {
                    let b = &mut self.live[slot];
                    let delta = crate::audio::sub(p, b.position);
                    let tangent = crate::audio::sub(
                        delta,
                        crate::audio::mul(up, crate::audio::dot(delta, up)),
                    );
                    if b.flight.is_some() && crate::audio::length(tangent) > 1e-6 {
                        b.forward = crate::audio::unit(tangent);
                    }
                    b.position = p;
                }
                Action::FlightStarted(slot) => {
                    if let Some(e) = &self.live[slot].event {
                        activate(host, e);
                    }
                    let e = begin(host)?;
                    self.live[slot].flight = Some(e.clone());
                    go(host, &e)?;
                }
                Action::FlightFinished(slot) => end(host, &mut self.live[slot].flight, false)?,
                Action::Call(slot) => {
                    if let Some(e) = &self.live[slot].event {
                        activate(host, e);
                    }
                    let e = begin(host)?;
                    if let Err(err) = go(host, &e) {
                        let mut e = Some(e);
                        let _ = end(host, &mut e, true);
                        return Err(err);
                    }
                    activate(host, &e);
                    match host.invoke("ChuckConnectionSystem.Compile",json!({"script":format!("Procedures/chuck/birds/{}.ck",SCRIPTS[slot]),"channel":12+slot})){Ok(r)=>{let request=r["request"].as_u64().ok_or("ChucK request ID missing")?;self.calls.insert(request,(slot,e));},Err(err)=>{let mut e=Some(e);let _=end(host,&mut e,true);return Err(err)}}
                }
            }
        }
        for (slot, b) in self.live.iter_mut().enumerate() {
            let target = if b.flight.is_some() { 1. } else { 0. };
            b.flight_blend += (target - b.flight_blend).clamp(-dt / 0.18, dt / 0.18);
            let call_target = if self.calls.values().any(|(s, _)| *s == slot) {
                1.
            } else {
                0.
            };
            b.calling += (call_target - b.calling).clamp(-dt / 0.12, dt / 0.12);
        }
        Ok(self.snapshot())
    }
    pub fn snapshot(&self) -> Value {
        let mut sources = Vec::new();
        let mut instances = Vec::new();
        for (slot, b) in self.live.iter().enumerate() {
            if let Some(id) = b.sound {
                sources.push(json!({"id":id,"position":b.position,"channel":12+slot}));
            }
            if let Some(id) = b.visual {
                instances.push(json!({"entity":id,"origin":b.position,"bird_pose":{
                    "time":self.time + slot as f64 * 0.371,
                    "flight":b.flight_blend,"call":b.calling,
                    "up":self.up,"forward":b.forward
                }}));
            }
        }
        json!({"sources":sources,"instances":instances,"habitat_revision":self.flock.revision})
    }
    pub fn dispatch(
        &mut self,
        host: &mut Host,
        hook: &str,
        input: &Value,
    ) -> Result<Value, String> {
        let parent = host.active_audicle;
        let result = match hook {
            "Step" => self.step(host, input),
            "Snapshot" => Ok(self.snapshot()),
            "Close" => {
                let _ = self.flock.clear();
                let mut err = None;
                for i in 0..4 {
                    if let Err(e) = self.despawn(host, i) {
                        err.get_or_insert(e);
                    }
                }
                match err {
                    Some(e) => Err(e),
                    None => Ok(Value::Null),
                }
            }
            _ => Err("unknown bird hook".into()),
        };
        host.active_audicle = parent;
        result
    }
}
