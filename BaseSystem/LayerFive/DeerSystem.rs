//! Independent two-animal wildlife owner, ground queries and finite flee Audicles.
#[path = "DeerBehavior.rs"]
pub(crate) mod behavior;
#[path = "DeerPose.rs"]
mod pose;
use crate::{
    audio::{V, add, cross, dot, length, mul, sub, unit, vector},
    host::Host,
};
use behavior::{Animal, Ground};
use serde_json::{Value, json};
const CAPACITY: usize = 2;
struct Live {
    pose: pose::State,
    ik: Value,
    animal: Animal,
    event: Value,
    entity: Option<u64>,
    flee_event: Option<Value>,
    trees: Vec<(V, f64)>,
    next_trees: f64,
}
#[derive(Default)]
pub(crate) struct State {
    live: [Option<Live>; CAPACITY],
    time: f64,
    next_spawn: f64,
    spawn_serial: u64,
}
fn activate(host: &mut Host, event: &Value) {
    host.active_audicle = event["audicle"].as_u64();
}
fn go(host: &mut Host, event: &Value) -> Result<(), String> {
    activate(host, event);
    host.invoke("AudicleSystem.Ready", event.clone())?;
    host.invoke("AudicleSystem.Go", event.clone())?;
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
fn trees(host: &mut Host, position: V) -> Result<Vec<(V, f64)>, String> {
    if !host.system_enabled("VegetationSystem") {
        return Ok(vec![]);
    }
    let query = host.invoke(
        "VegetationSystem.QueryPlants",
        json!({"position":position,"radius":10.,"limit":32}),
    )?;
    query["plants"]
        .as_array()
        .ok_or("deer plant query required")?
        .iter()
        .filter(|p| p["alive"] == true && p["height"].as_f64().unwrap_or(0.) > 1.5)
        .map(|p| {
            Ok((
                vector(&p["position"])?,
                (p["height"].as_f64().unwrap_or(10.) * 0.025).clamp(0.15, 0.6) + 0.65,
            ))
        })
        .collect()
}
fn ground(
    host: &mut Host,
    previous: V,
    wanted: V,
    heading: V,
    eye: V,
    venue: Option<V>,
    trees: &[(V, f64)],
) -> Result<Option<Ground>, String> {
    let radial = unit(wanted);
    if let Some(origin) = venue {
        let delta = sub(wanted, origin);
        let planar = sub(delta, mul(radial, dot(delta, radial)));
        if length(planar) < 100. {
            return Ok(None);
        }
    }
    let height = |host: &mut Host, p: V| -> Result<f64, String> {
        host.invoke(
            "PlanetSurfaceSystem.Height",
            json!({"position":p,"camera_eye":eye,"eye_height":1.35}),
        )?["height"]
            .as_f64()
            .filter(|h| h.is_finite())
            .ok_or("finite deer ground required".into())
    };
    let h = height(host, wanted)?;
    let position = mul(radial, crate::terrain::RADIUS + h + 0.015);
    for (tree, radius) in trees {
        let delta = sub(position, *tree);
        let planar = sub(delta, mul(radial, dot(delta, radial)));
        if length(planar) < *radius {
            return Ok(None);
        }
    }
    let forward = unit(sub(heading, mul(radial, dot(heading, radial))));
    let right = unit(cross(radial, forward));
    if length(right) < 0.9 {
        return Ok(None);
    }
    let front = height(host, add(position, mul(forward, 0.55)))?;
    let back = height(host, add(position, mul(forward, -0.55)))?;
    let left = height(host, add(position, mul(right, -0.3)))?;
    let across = height(host, add(position, mul(right, 0.3)))?;
    if (front - back).abs() > 0.75
        || (across - left).abs() > 0.42
        || [front, back, left, across]
            .iter()
            .any(|x| (*x - h).abs() > 0.45)
    {
        return Ok(None);
    }
    let up = unit(sub(
        sub(radial, mul(forward, (front - back) / 1.1)),
        mul(right, (across - left) / 0.6),
    ));
    if host.system_enabled("WaterSystem") {
        let wet = host.invoke(
            "WaterSystem.Sample",
            json!({"position":add(position,mul(radial,0.12))}),
        )?;
        if wet["occupied"] == true || wet["coverage_known"] == false {
            return Ok(None);
        }
    }
    if host.system_enabled("PhysicsSystem") {
        let desired = add(position, mul(radial, 1.35));
        let resolved=host.invoke("PhysicsSystem.GameResolveBody",json!({"previous":add(previous,mul(unit(previous),1.35)),"position":desired,"eye_height":1.35}))?;
        if length(sub(vector(&resolved["position"])?, desired)) > 0.04 {
            return Ok(None);
        }
    }
    Ok(Some(Ground { position, up }))
}
impl State {
    fn remove(&mut self, host: &mut Host, slot: usize) -> Result<(), String> {
        let Some(b) = self.live[slot].as_mut() else {
            return Ok(());
        };
        end(host, &mut b.flee_event, true)?;
        activate(host, &b.event);
        if let Some(id) = b.entity {
            host.invoke("EntitySystem.Remove", json!({"entity":id}))?;
            b.entity = None;
        }
        let mut event = Some(b.event.clone());
        end(host, &mut event, true)?;
        self.live[slot] = None;
        Ok(())
    }
    fn spawn(
        &mut self,
        host: &mut Host,
        slot: usize,
        g: Ground,
        heading: V,
        nearby: Vec<(V, f64)>,
    ) -> Result<(), String> {
        let event = host.invoke("AudicleSystem.Create", Value::Null)?;
        self.live[slot] = Some(Live {
            pose: Default::default(),
            ik: Value::Null,
            animal: Animal::new(g, heading, slot as f64 * 0.37),
            event: event.clone(),
            entity: None,
            flee_event: None,
            trees: nearby,
            next_trees: self.time + 0.5,
        });
        activate(host, &event);
        let result = (|| {
            let id = host.invoke(
                "EntitySystem.Create",
                crate::base_system::layer_four::deer_rig::spec(),
            )?["entity"]
                .as_u64()
                .ok_or("deer Entity required")?;
            self.live[slot].as_mut().unwrap().entity = Some(id);
            go(host, &event)
        })();
        if result.is_err() {
            let _ = self.remove(host, slot);
        }
        result
    }
    fn step(&mut self, host: &mut Host, input: &Value) -> Result<Value, String> {
        let player = vector(&input["position"])?;
        let dt = input["dt"]
            .as_f64()
            .filter(|x| x.is_finite() && (0. ..=0.25).contains(x))
            .ok_or("bounded deer dt required")?;
        if length(player) < crate::terrain::RADIUS * 0.5 {
            return Err("world-space deer observer required".into());
        }
        self.time += dt;
        let up = unit(player);
        let reference = if up[1].abs() < 0.9 {
            [0., 1., 0.]
        } else {
            [1., 0., 0.]
        };
        let right = unit(cross(reference, up));
        let forward = unit(cross(up, right));
        let venue = if host.system_enabled("TheatreSystem") {
            let v = host.invoke("TheatreSystem.Status", Value::Null)?;
            if v["ready"] == true {
                Some(vector(&v["origin"])?)
            } else {
                None
            }
        } else {
            None
        };
        for slot in 0..CAPACITY {
            if self.live[slot]
                .as_ref()
                .is_some_and(|b| length(sub(b.animal.position, player)) > 180.)
            {
                self.remove(host, slot)?;
            }
        }
        if self.time >= self.next_spawn {
            self.next_spawn = self.time + 2.;
            for slot in 0..CAPACITY {
                if self.live[slot].is_some() {
                    continue;
                }
                for attempt in 0..16 {
                    let angle = (attempt as f64 + slot as f64 * 7. + self.spawn_serial as f64)
                        * 2.399963229728653;
                    let heading = add(mul(right, angle.cos()), mul(forward, angle.sin()));
                    let offset = mul(heading, 40. + (attempt % 4) as f64 * 10.);
                    let candidate = add(player, offset);
                    // Reject the building footprint before doing expensive terrain/tree queries.
                    if venue.is_some_and(|origin| {
                        let d = sub(candidate, origin);
                        length(sub(d, mul(up, dot(d, up)))) < 100.
                    }) {
                        continue;
                    }
                    let nearby = trees(host, candidate)?;
                    if let Some(g) =
                        ground(host, candidate, candidate, heading, player, venue, &nearby)?
                    {
                        if length(sub(g.position, player)) > 100.
                            || self
                                .live
                                .iter()
                                .flatten()
                                .any(|b| length(sub(b.animal.position, g.position)) < 12.)
                        {
                            continue;
                        }
                        self.spawn(host, slot, g, heading, nearby)?;
                        break;
                    }
                }
            }
            self.spawn_serial = self.spawn_serial.wrapping_add(1);
        }
        for live in self.live.iter_mut().flatten() {
            activate(host, &live.event);
            host.invoke("AudicleSystem.Resume", live.event.clone())?;
            if self.time >= live.next_trees {
                live.trees = trees(host, live.animal.position)?;
                live.next_trees = self.time + 0.5;
            }
            live.animal.advance(player, dt, |old, wanted, heading| {
                ground(host, old, wanted, heading, player, venue, &live.trees)
            })?;
            if host.system_enabled("InverseKinematicsSystem") {
                live.ik = live.pose.solve(host, &live.animal, player, dt)?;
            } else {
                live.ik = Value::Null;
                live.pose = Default::default();
            }
            if live.animal.fleeing && live.flee_event.is_none() {
                let e = host.invoke("AudicleSystem.Create", Value::Null)?;
                live.flee_event = Some(e.clone());
                go(host, &e)?;
            } else if !live.animal.fleeing {
                end(host, &mut live.flee_event, false)?;
            }
        }
        Ok(self.snapshot())
    }
    pub(crate) fn snapshot(&self) -> Value {
        let instances:Vec<_>=self.live.iter().flatten().filter_map(|b|b.entity.map(|id|json!({"entity":id,"origin":b.animal.position,"deer_pose":{"up":b.animal.up,"forward":b.animal.forward,"time":b.animal.time,"stride":b.animal.stride,"run":b.animal.run_blend,"alert":b.animal.alert_blend,"watch":b.animal.alert_blend,"look_yaw":b.animal.look_yaw,"look_pitch":b.animal.look_pitch,"ik":b.ik}}))).collect();
        let animals:Vec<_>=self.live.iter().flatten().map(|b|json!({"entity":b.entity,"position":b.animal.position,"fleeing":b.animal.fleeing,"speed":b.animal.speed,"stride":b.animal.stride,"look_yaw":b.animal.look_yaw,"look_pitch":b.animal.look_pitch})).collect();
        json!({"instances":instances,"animals":animals,"capacity":CAPACITY})
    }
    pub(crate) fn dispatch(
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
                let mut first = None;
                for slot in 0..CAPACITY {
                    if let Err(e) = self.remove(host, slot) {
                        first.get_or_insert(e);
                    }
                }
                if let Some(e) = first {
                    Err(e)
                } else {
                    *self = Self::default();
                    Ok(Value::Null)
                }
            }
            _ => Err("unknown deer hook".into()),
        };
        host.active_audicle = parent;
        result
    }
}
