//! Ground recipes own their countdowns; all material authorization is physical.
use crate::{audio::*, host::Host};
use serde_json::{Value, json};
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Kind {
    Jar,
    Blueprint,
    Plank,
}
#[derive(Clone)]
pub(crate) struct Item {
    pub id: u64,
    pub kind: Kind,
    pub position: V,
    pub up: V,
    pub right: V,
    pub job: Option<Job>,
}
#[derive(Clone)]
pub(crate) struct Job {
    tree: u64,
    event: Value,
    pub elapsed: f64,
    pub rate: f64,
}
#[derive(Default)]
pub(crate) struct State {
    pub items: Vec<Item>,
    pub held: Option<u64>,
    previous: [bool; 2],
    owned_f: bool,
    owned_select: bool,
    next_id: u64,
}
pub(crate) fn rate(distance: f64) -> f64 {
    100f64.powf(-((distance - 10.) / 10.).clamp(0., 1.))
}
impl State {
    pub fn carrying(&self) -> bool {
        self.held.is_some()
    }
    pub fn owns_interact(&self) -> bool {
        self.owned_f
    }
    fn push(&mut self, kind: Kind, position: V, up: V, right: V) {
        self.next_id += 1;
        self.items.push(Item {
            id: self.next_id,
            kind,
            position,
            up,
            right,
            job: None,
        });
    }
    fn status(&self) -> Value {
        json!({"held":self.held,"items":self.items.iter().map(|i|json!({"id":i.id,"kind":format!("{:?}",i.kind),"position":i.position,"remaining_seconds":i.job.as_ref().map(|j|(60.-j.elapsed).max(0.)),"rate":i.job.as_ref().map(|j|j.rate)})).collect::<Vec<_>>()})
    }
}
pub(crate) fn pose(item: &Item, held: bool, view: &Value) -> Result<(V, V, V), String> {
    if !held {
        return Ok((item.position, item.up, item.right));
    }
    let eye = vector(&view["position"])?;
    let f = unit(vector(&view["forward"])?);
    let up = unit(vector(&view["up"])?);
    let right = unit(cross(f, up));
    Ok((add(eye, add(mul(f, 0.85), mul(up, -0.32))), up, right))
}
fn trace(host: &mut Host, eye: V, direction: V, distance: f64) -> Result<Value, String> {
    let snapshot = host.invoke(
        "TerrainGeometrySystem.Capture",
        json!({"position":eye,"time":0.}),
    )?;
    host.invoke(
        "TerrainGeometrySystem.Trace",
        json!({"snapshot":snapshot,"position":eye,"direction":direction,"distance":distance}),
    )
}
fn caption(i: &Item) -> String {
    match i.kind {
        Kind::Jar => "Blueprint jar · F open".into(),
        Kind::Plank => "Timber plank · F pick up / place".into(),
        Kind::Blueprint => {
            if let Some(j) = &i.job {
                format!(
                    "Planks · {:0.1}s remaining · {:0.2}× speed",
                    (60. - j.elapsed).max(0.),
                    j.rate
                )
            } else {
                "Planks · needs 1 felled tree within 4 blocks · Shift + left click · 60 seconds"
                    .into()
            }
        }
    }
}
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let mut s = std::mem::take(&mut host.crafting);
    let result = (|| match hook {
        "Configure" => {
            host.invoke("CraftingRenderSystem.Load", Value::Null)?;
            s = State::default();
            let data: Value = serde_json::from_slice(
                &std::fs::read(host.root().join("Procedures/caves.json"))
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            if let Some(field) = crate::terrain::underground::Field::parse(&data)? {
                // Fixed authored tunnel sites, projected onto actual cave support. No random
                // session spawning or jar creation from player movement.
                for local in [[20., -8., -13.], [29., -15., -17.], [-51., -44., 16.]] {
                    let p = crate::terrain::underground::world(local);
                    if let Some(h) = field.support(p, 0.6, &|n| crate::terrain::height(n)) {
                        let up = unit(p);
                        let position = mul(up, crate::terrain::RADIUS + h + 0.012);
                        if field.distance(add(position, mul(up, 0.3)), crate::terrain::height(up))
                            > 0.24
                        {
                            let right = unit(cross(up, [0., 0., 1.]));
                            s.push(Kind::Jar, position, up, right);
                        }
                    }
                }
            }
            Ok(s.status())
        }
        "Status" => Ok(s.status()),
        "Close" => {
            for i in &s.items {
                if let Some(j) = &i.job {
                    host.invoke("AudicleSystem.Cancel", j.event.clone())?;
                }
            }
            s = State::default();
            Ok(Value::Null)
        }
        "Grip" => {
            let Some(i) = s.items.iter().find(|i| Some(i.id) == s.held) else {
                return Ok(json!({"active":false}));
            };
            let (p, up, right) = pose(i, true, input)?;
            let p = add(p, mul(up, 0.035));
            Ok(
                json!({"active":true,"locked":true,"left":add(p,mul(right,-0.15)),"right":add(p,mul(right,0.15))}),
            )
        }
        "Frame" => {
            let eye = vector(&input["view"]["position"])?;
            let dt = input["dt"]
                .as_f64()
                .filter(|v| v.is_finite())
                .unwrap_or(0.)
                .clamp(0., 0.25);
            let mut complete = Vec::new();
            for i in &mut s.items {
                if let Some(job) = &mut i.job {
                    host.invoke("AudicleSystem.Resume", job.event.clone())?;
                    let tree = host.invoke(
                        "PhysicsSystem.GameCraftTree",
                        json!({"position":i.position,"site_id":job.tree}),
                    )?;
                    job.rate = if tree.is_null() {
                        0.
                    } else {
                        rate(length(sub(eye, i.position)))
                    };
                    job.elapsed = (job.elapsed + dt * job.rate).min(60.);
                    if job.elapsed >= 60. {
                        let accepted = host.invoke(
                            "PhysicsSystem.GameCraftTree",
                            json!({"position":i.position,"site_id":job.tree,"consume":true}),
                        )?;
                        if !accepted.is_null() {
                            host.invoke("AudicleSystem.Finish", job.event.clone())?;
                            complete.push(i.clone());
                        }
                    }
                }
            }
            for i in complete {
                s.items.retain(|v| v.id != i.id);
                let front = unit(cross(i.right, i.up));
                for k in 0..6 {
                    let p = add(
                        i.position,
                        add(
                            mul(i.right, (k % 3) as f64 * 0.28 - 0.28),
                            mul(front, (k / 3) as f64 * 0.13),
                        ),
                    );
                    // Six independent objects; stack in two layers on the supported sheet.
                    let p = add(p, mul(i.up, (k / 3) as f64 * 0.095));
                    s.push(Kind::Plank, p, i.up, i.right);
                }
            }
            Ok(s.status())
        }
        "Input" => {
            let w = &input["window"];
            let view = &input["view"];
            let f = w["flight"]["phone"]["interact"] == true;
            let click = w["flight"]["select"] == true;
            let keys = [f, click];
            let edges: [bool; 2] = std::array::from_fn(|k| keys[k] && !s.previous[k]);
            s.previous = keys;
            if !f {
                s.owned_f = false;
            }
            let previous_owned_select = s.owned_select;
            if !click {
                s.owned_select = false;
            }
            let mut consumed = false;
            let mut text = None;
            let was_held = s.carrying();
            if input["blocked"] != true
                && w["flight"]["focused"] != false
                && view["position"].is_array()
            {
                let eye = vector(&view["position"])?;
                let ray = unit(vector(&view["forward"])?);
                let target = s
                    .items
                    .iter()
                    .enumerate()
                    .filter(|(_, i)| Some(i.id) != s.held)
                    .filter_map(|(k, i)| {
                        let center = add(
                            i.position,
                            mul(i.up, if i.kind == Kind::Jar { 0.28 } else { 0.05 }),
                        );
                        let along = dot(sub(center, eye), ray);
                        let radius = match i.kind {
                            Kind::Jar => 0.29,
                            Kind::Blueprint => 0.4,
                            Kind::Plank => 0.6,
                        };
                        ((0.05..=3.25).contains(&along)
                            && length(sub(sub(center, eye), mul(ray, along))) <= radius)
                            .then_some((k, along))
                    })
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                let target = if let Some((k, d)) = target {
                    let hit = trace(host, eye, ray, d)?;
                    if hit["distance"].as_f64().is_none_or(|v| v + 0.08 >= d) {
                        Some(k)
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(k) = target {
                    text = Some(caption(&s.items[k]));
                }
                if s.held.is_some() {
                    text = Some("F place on the ground".into());
                }
                if edges[0] {
                    if let Some(id) = s.held {
                        s.owned_f = true;
                        let hit = trace(host, eye, ray, 3.25)?;
                        if let (Ok(p), Ok(up)) = (vector(&hit["position"]), vector(&hit["normal"]))
                        {
                            if dot(unit(up), unit(p)) > 0.65 {
                                let i = s.items.iter_mut().find(|i| i.id == id).unwrap();
                                i.position = add(p, mul(up, 0.012));
                                i.up = unit(up);
                                let side = cross(ray, up);
                                i.right = if length(side) > 0.01 {
                                    unit(side)
                                } else {
                                    unit(vector(&view["right"])?)
                                };
                                s.held = None;
                            }
                        }
                    } else if let Some(k) = target {
                        s.owned_f = true;
                        if s.items[k].job.is_none()
                            && host.invoke("PhysicsSystem.GameFrame", Value::Null)?["can_stow_tool"]
                                == true
                        {
                            if s.items[k].kind == Kind::Jar {
                                s.items[k].kind = Kind::Blueprint;
                            }
                            s.held = Some(s.items[k].id);
                        }
                    }
                }
                if edges[1]
                    && w["flight"]["workstation"]["editor"]["shift"] == true
                    && s.held.is_none()
                {
                    if let Some(k) = target {
                        if s.items[k].kind == Kind::Blueprint {
                            consumed = true;
                            s.owned_select = true;
                            if s.items[k].job.is_none() {
                                let tree = host.invoke(
                                    "PhysicsSystem.GameCraftTree",
                                    json!({"position":s.items[k].position}),
                                )?;
                                if let Some(id) = tree["site_id"].as_u64() {
                                    if s.items
                                        .iter()
                                        .all(|i| i.job.as_ref().is_none_or(|j| j.tree != id))
                                    {
                                        let event =
                                            host.invoke("AudicleSystem.Create", Value::Null)?;
                                        host.invoke("AudicleSystem.Ready", event.clone())?;
                                        host.invoke("AudicleSystem.Go", event.clone())?;
                                        s.items[k].job = Some(Job {
                                            tree: id,
                                            event,
                                            elapsed: 0.,
                                            rate: 1.,
                                        });
                                        text = Some(
                                            "Crafting planks · 60 seconds · stay within 10 blocks"
                                                .into(),
                                        );
                                    } else {
                                        text = Some(
                                            "That tree is already reserved by another blueprint"
                                                .into(),
                                        );
                                    }
                                } else {
                                    text = Some(
                                        "Needs a physically felled tree within 4 blocks".into(),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            Ok(
                json!({"consumed_interact":s.owned_f,"consumed_select":consumed||s.owned_select||previous_owned_select,"held":s.carrying(),"changed":was_held!=s.carrying(),"caption":text}),
            )
        }
        _ => Err("unknown crafting hook".into()),
    })();
    host.crafting = s;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proximity_clock() {
        assert_eq!(rate(0.), 1.);
        assert_eq!(rate(10.), 1.);
        assert!((rate(15.) - 0.1).abs() < 1e-12);
        assert!((rate(20.) - 0.01).abs() < 1e-12);
        assert_eq!(rate(100.), rate(20.));
    }
    #[test]
    fn sixty_seconds_of_local_work() {
        for (d, seconds) in [(0., 60.), (10., 60.), (15., 600.), (20., 6000.)] {
            assert!((seconds * rate(d) - 60.).abs() < 1e-9);
        }
    }
}
