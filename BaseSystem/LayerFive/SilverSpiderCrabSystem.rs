//! One bounded Depths predator, sharing the cave contact field with the player.
use crate::{
    audio::{V, add, cross, dot, length, mul, sub, unit, vector},
    host::Host,
};
use serde_json::{Value, json};
#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Hunting,
    Grabbed,
    Retreating,
    Passive,
}
#[derive(Debug)]
struct Brain {
    mode: Mode,
    presses: u32,
    keys: [bool; 4],
    retreat: f64,
}
impl Default for Brain {
    fn default() -> Self {
        Self {
            mode: Mode::Hunting,
            presses: 0,
            keys: [false; 4],
            retreat: 0.,
        }
    }
}
impl Brain {
    fn step(&mut self, distance: f64, dt: f64, keys: [bool; 4], allowed: bool) {
        match self.mode {
            Mode::Hunting | Mode::Passive
                if allowed && distance < if self.mode == Mode::Hunting { 2.6 } else { 2.8 } =>
            {
                self.mode = Mode::Grabbed;
                self.presses = 0;
                self.keys = keys;
            }
            Mode::Grabbed => {
                if keys.iter().zip(self.keys).any(|(a, b)| *a && !b) {
                    self.presses += 1;
                }
                if self.presses >= 24 {
                    self.mode = Mode::Retreating;
                    self.retreat = 5.;
                }
            }
            Mode::Retreating => {
                self.retreat -= dt;
                if self.retreat <= 0. {
                    self.mode = Mode::Passive;
                }
            }
            _ => {}
        }
        self.keys = keys;
    }
}
struct Animal {
    position: V,
    up: V,
    forward: V,
    brain: Brain,
    entity: u64,
    event: Value,
    attack: Option<Value>,
    time: f64,
}
#[derive(Default)]
pub(crate) struct State {
    animal: Option<Animal>,
    spawn_clock: f64,
}
fn sample(host: &mut Host, p: V, edits: &Value) -> Result<(f64, V), String> {
    let s = host.invoke("CaveSystem.Sample", json!({"position":p,"edits":edits}))?;
    Ok((
        s["signed_distance"]
            .as_f64()
            .ok_or("cave distance required")?,
        vector(&s["normal"])?,
    ))
}
fn begin(host: &mut Host) -> Result<Value, String> {
    let e = host.invoke("AudicleSystem.Create", Value::Null)?;
    host.active_audicle = e["audicle"].as_u64();
    host.invoke("AudicleSystem.Ready", e.clone())?;
    host.invoke("AudicleSystem.Go", e.clone())?;
    Ok(e)
}
impl State {
    fn close(&mut self, host: &mut Host) -> Result<Value, String> {
        if let Some(a) = self.animal.as_mut() {
            if let Some(e) = a.attack.take() {
                host.active_audicle = e["audicle"].as_u64();
                host.invoke("AudicleSystem.Cancel", e)?;
            }
            host.active_audicle = a.event["audicle"].as_u64();
            host.invoke("EntitySystem.Remove", json!({"entity":a.entity}))?;
            host.invoke("AudicleSystem.Cancel", a.event.clone())?;
        }
        self.animal = None;
        self.spawn_clock = 0.;
        Ok(Value::Null)
    }
    fn step(&mut self, host: &mut Host, input: &Value) -> Result<Value, String> {
        let player = vector(&input["position"])?;
        let dt = input["dt"]
            .as_f64()
            .filter(|v| v.is_finite() && (0. ..=0.25).contains(v))
            .ok_or("bounded crab dt required")?;
        let deep = crate::terrain::underground::local(player)[1] < -22.
            && !crate::terrain::nightworld::active(player);
        if !deep
            || self
                .animal
                .as_ref()
                .is_some_and(|a| length(sub(a.position, player)) > 100.)
        {
            self.close(host)?;
            return Ok(json!({"instances":[],"locked":false}));
        }
        let edits = host.invoke("PlanetSurfaceSystem.Frame", Value::Null)?["edits"].clone();
        self.spawn_clock -= dt;
        if self.animal.is_none() && self.spawn_clock <= 0. {
            self.spawn_clock = 2.;
            // Search a bounded ring in connected air, then descend to actual rock.
            'spawn: for i in 0..12 {
                let angle = i as f64 * 2.3999632297;
                let mut p = add(
                    player,
                    [
                        angle.cos() * (8. + (i % 3) as f64 * 4.),
                        0.,
                        angle.sin() * (8. + (i % 3) as f64 * 4.),
                    ],
                );
                if sample(host, p, &edits)?.0 < 1.2 {
                    continue;
                }
                for j in 1..=16 {
                    if sample(
                        host,
                        add(player, mul(sub(p, player), j as f64 / 16.)),
                        &edits,
                    )?
                    .0 < 1.
                    {
                        continue 'spawn;
                    }
                }
                for _ in 0..64 {
                    let (d, n) = sample(host, p, &edits)?;
                    if d < 1.3 && d > 0.6 {
                        let event = begin(host)?;
                        let entity = match host.invoke(
                            "EntitySystem.Create",
                            crate::base_system::layer_four::silver_spider_crab_rig::spec(),
                        ) {
                            Ok(v) => v["entity"].as_u64().ok_or("crab Entity required")?,
                            Err(e) => {
                                host.invoke("AudicleSystem.Cancel", event)?;
                                return Err(e);
                            }
                        };
                        self.animal = Some(Animal {
                            position: p,
                            up: n,
                            forward: unit(sub(player, p)),
                            brain: Brain::default(),
                            entity,
                            event,
                            attack: None,
                            time: 0.,
                        });
                        break 'spawn;
                    }
                    p = add(p, [0., -0.5, 0.]);
                    if d < 0.6 {
                        break;
                    }
                }
            }
        }
        let Some(a) = self.animal.as_mut() else {
            return Ok(json!({"instances":[],"locked":false}));
        };
        host.active_audicle = a.event["audicle"].as_u64();
        host.invoke("AudicleSystem.Resume", a.event.clone())?;
        a.time += dt;
        let keys = std::array::from_fn(|i| input["move"][i] == true);
        let old = a.brain.mode;
        let distance = length(sub(player, a.position));
        let mut visible = true;
        if distance < 2.8 && a.brain.mode != Mode::Grabbed {
            for i in 1..8 {
                if sample(
                    host,
                    add(player, mul(sub(a.position, player), i as f64 / 8.)),
                    &edits,
                )?
                .0 < 0.25
                {
                    visible = false;
                    break;
                }
            }
        }
        a.brain
            .step(distance, dt, keys, input["immune"] != true && visible);
        if a.brain.mode == Mode::Grabbed && old != Mode::Grabbed {
            a.attack = Some(begin(host)?);
        }
        if old == Mode::Grabbed && a.brain.mode != Mode::Grabbed {
            if let Some(e) = a.attack.take() {
                host.active_audicle = e["audicle"].as_u64();
                host.invoke("AudicleSystem.Finish", e)?;
            }
        }
        let locked = a.brain.mode == Mode::Grabbed;
        if !locked && a.brain.mode != Mode::Passive {
            let toward = unit(sub(player, a.position));
            let direction = if a.brain.mode == Mode::Retreating {
                mul(toward, -1.)
            } else {
                toward
            };
            let wet = if host.system_enabled("WaterSystem") {
                host.invoke("WaterSystem.Sample", json!({"position":a.position}))?["occupied"]
                    == true
            } else {
                false
            };
            let tangent = unit(sub(direction, mul(a.up, dot(direction, a.up))));
            let travel = if wet { direction } else { tangent };
            let speed = if a.brain.mode == Mode::Retreating {
                6.
            } else {
                4.5
            };
            let candidate = add(a.position, mul(travel, speed * dt));
            let (d, n) = sample(host, candidate, &edits)?;
            let corrected = if wet && d > 1.0 {
                candidate
            } else {
                add(candidate, mul(n, (1.0 - d).clamp(-0.45, 0.45)))
            };
            // Short swept segments prevent crossing thin rock; stop when blocked.
            let mut clear = true;
            for i in 1..=4 {
                if sample(
                    host,
                    add(a.position, mul(sub(corrected, a.position), i as f64 / 4.)),
                    &edits,
                )?
                .0 < 0.65
                {
                    clear = false;
                    break;
                }
            }
            if !clear && !wet && length(n) > 0.9 {
                a.up = n;
            }
            if clear {
                a.position = corrected;
                a.up = if wet {
                    unit(player)
                } else {
                    unit(add(mul(a.up, 0.65), mul(n, 0.35)))
                };
                let forward = unit(sub(travel, mul(a.up, dot(travel, a.up))));
                if length(forward) > 0.9 {
                    a.forward = forward;
                }
            }
        }
        let (origin, up, forward, scale) = if locked {
            let forward = vector(&input["forward"])?;
            let up = vector(&input["up"])?;
            (
                add(add(player, mul(forward, 1.65)), mul(up, -0.62)),
                up,
                mul(forward, -1.),
                0.52,
            )
        } else {
            (sub(a.position, mul(a.up, 1.0)), a.up, a.forward, 1.)
        };
        let caption = if locked {
            Some(format!(
                "SILVER SPIDER CRAB — Mash W A S D to escape  {}/24",
                a.brain.presses
            ))
        } else if old == Mode::Grabbed {
            Some("Escaped — the silver spider crab retreats".into())
        } else {
            None
        };
        Ok(
            json!({"instances":[{"entity":a.entity,"origin":origin,"silver_spider_crab":true,"crab_pose":{"up":up,"forward":forward,"time":a.time,"moving":a.brain.mode!=Mode::Passive,"scale":scale}}],"locked":locked,"caption":caption,"mode":format!("{:?}",a.brain.mode),"presses":a.brain.presses,"position":a.position}),
        )
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
            "Close" => self.close(host),
            _ => Err("unknown silver spider crab hook".into()),
        };
        host.active_audicle = parent;
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escape_requires_fresh_presses_then_retreats_and_only_rearms_up_close() {
        let mut b = Brain::default();
        b.step(2., 0.1, [false; 4], true);
        assert_eq!(b.mode, Mode::Grabbed);
        for _ in 0..80 {
            b.step(2., 0.1, [true; 4], true);
        }
        assert_eq!(b.presses, 1);
        for _ in 0..23 {
            b.step(2., 0.1, [false; 4], true);
            b.step(2., 0.1, [true, false, false, false], true);
        }
        assert_eq!(b.mode, Mode::Retreating);
        for _ in 0..60 {
            b.step(1., 0.1, [false; 4], true);
        }
        assert_eq!(b.mode, Mode::Grabbed);
    }
    #[test]
    fn passive_stays_calm_at_range_and_immunity_prevents_capture() {
        let mut b = Brain {
            mode: Mode::Passive,
            ..Default::default()
        };
        b.step(4., 0.1, [false; 4], true);
        assert_eq!(b.mode, Mode::Passive);
        b.step(1., 0.1, [false; 4], false);
        assert_eq!(b.mode, Mode::Passive);
        b.step(1., 0.1, [false; 4], true);
        assert_eq!(b.mode, Mode::Grabbed);
    }
}
