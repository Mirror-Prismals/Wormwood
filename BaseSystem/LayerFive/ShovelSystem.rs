//! Player-owned spade requests; contact physics owns the accepted rigid pose.
use crate::host::Host;
use serde_json::{Value, json};
type V = [f64; 3];
fn v(x: &Value) -> Result<V, String> {
    serde_json::from_value(x.clone()).map_err(|e| format!("tool vector: {e}"))
}
fn dot(a: V, b: V) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] + b[i])
}
fn mul(a: V, s: f64) -> V {
    a.map(|x| x * s)
}
fn unit(a: V) -> V {
    mul(a, 1. / dot(a, a).sqrt().max(1e-12))
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn finite_vector(x: &Value) -> bool {
    v(x).is_ok_and(|a| a.iter().all(|n| n.is_finite() && n.abs() < 1e9))
}
fn pair(x: &Value) -> Option<[f64; 2]> {
    let a: [f64; 2] = serde_json::from_value(x.clone()).ok()?;
    a.iter()
        .all(|n| n.is_finite() && n.abs() <= 1000.)
        .then_some(a)
}

#[cfg(test)]
mod guided_tests {
    use super::*;

    #[test]
    fn guided_dig_requires_fresh_tool_force_before_levering() {
        let mut dig = GuidedDig::new();
        let mut response = json!({"material":{"initialized":true},"force":[0.,0.,0.],"blocked":false,"contact":{"vertex_face_contacts":50}});
        assert_eq!(dig.observe(0.5, &response, true), None);
        assert_eq!(dig.phase, DigPhase::Lower); // soil-soil pairs are not tool contact
        response["force"] = json!([0., 2., 0.]);
        assert_eq!(dig.observe(0.1, &response, true), None);
        assert_eq!(dig.phase, DigPhase::Lever);
        assert_eq!(dig.observe(0.8, &response, true), None);
        assert_eq!(dig.phase, DigPhase::Lift);
        assert_eq!(dig.observe(0.6, &response, true), Some("empty_stroke"));
    }

    #[test]
    fn guided_dig_stops_on_missing_contact_stale_feedback_or_stall() {
        let empty = json!({"material":{"initialized":true},"force":[0.,0.,0.],"blocked":false,"contact":{}});
        assert_eq!(GuidedDig::new().observe(4., &empty, true), Some("no_soil_contact"));
        let stale = json!({"contact":{"feedback_stale":true}});
        assert_eq!(GuidedDig::new().observe(0.1, &stale, false), Some("contact_feedback_unavailable"));
        let blocked = json!({"blocked":true,"contact":{}});
        assert_eq!(GuidedDig::new().observe(0.75, &blocked, false), Some("blocked"));
    }

    #[test]
    fn completing_a_dig_requires_this_strokes_earth_to_lift() {
        let material=json!({"parcels":[
            {"id":0,"position":[0.,0.,0.],"mass":0.2,"detached":false},
            {"id":1,"position":[0.,0.,0.],"mass":2.,"detached":true}
        ]});
        let make=|| {
            let mut dig=GuidedDig::aimed(DigSite{position:[0.;3],up:[0.,1.,0.],heading:[0.,0.,-1.]},&material);
            dig.phase=DigPhase::Lift;
            dig
        };
        let mut dig=make();
        let mut response=json!({"pose":dig.goal(),"contact":{},"material":{"peak_lift":10.,"parcels":[
            {"id":0,"position":[0.,0.,0.],"mass":0.2,"detached":false},
            {"id":1,"position":[0.,1.,0.],"mass":2.,"detached":true}
        ]}});
        assert_eq!(dig.observe(0.6,&response,true),Some("empty_stroke"),"old debris or an old peak must not count");
        response["material"]["parcels"][0]["detached"]=json!(true);
        response["material"]["parcels"][0]["position"]=json!([0.,0.06,0.]);
        assert_eq!(make().observe(0.6,&response,true),Some("completed"));
        response["material"]["parcels"][0]["position"]=json!([0.,0.01,0.]);
        assert_eq!(make().observe(0.6,&response,true),Some("empty_stroke"),"separation alone is not lifting");
    }

    #[test]
    fn refined_child_is_compared_to_the_same_original_material_point() {
        let reference=json!([[-1.,0.,-1.],[1.,0.,-1.],[-1.,0.,1.],[1.,0.,1.],[-1.,1.,-1.],[1.,1.,-1.],[-1.,1.,1.],[1.,1.,1.]]);
        let material=json!({"parcels":[{"id":7,"position":[0.,0.5,0.],"mass":1.,"detached":false,
            "reference_vertices":reference,"vertices":reference,
            "faces":[[0,1,3,2],[4,6,7,5],[0,4,5,1],[2,3,7,6],[0,2,6,4],[1,5,7,3]]}]});
        let make=||{let mut dig=GuidedDig::aimed(DigSite{position:[0.;3],up:[0.,1.,0.],heading:[0.,0.,-1.]},&material);dig.phase=DigPhase::Lift;dig};
        let mut response=json!({"pose":make().goal(),"contact":{},"material":{"parcels":[
            {"id":8,"ancestors":[7],"position":[0.,0.25,0.],"reference_position":[0.,0.25,0.],"mass":0.2,"detached":true}
        ]}});
        assert_eq!(make().observe(0.6,&response,true),Some("empty_stroke"),"subdivision alone is not a lift");
        response["material"]["parcels"][0]["position"]=json!([0.,0.31,0.]);
        assert_eq!(make().observe(0.6,&response,true),Some("completed"),"lift is measured from this child's original material position");
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DigPhase { Position, Lower, Lever, Lift }

struct DigSite { position: V, up: V, heading: V }

struct GuidedDig {
    phase: DigPhase,
    elapsed: f64,
    stalled: f64,
    site: Option<DigSite>,
    // Track the same material through a stroke. Historical peak lift and
    // previously detached earth cannot make a new, empty stroke successful.
    material_start: Vec<(u64, Value)>,
    lifted_mass: f64,
    contact_seen: bool,
}
impl GuidedDig {
    fn new() -> Self { Self { phase: DigPhase::Lower, elapsed: 0., stalled: 0., site: None, material_start:Vec::new(),lifted_mass:0.,contact_seen:false } }
    fn aimed(site: DigSite, material: &Value) -> Self {
        let material_start=material["parcels"].as_array().into_iter().flatten()
            .filter(|p|p["detached"]==false)
            .filter_map(|p|Some((p["id"].as_u64()?,p.clone())))
            .collect();
        Self { phase: DigPhase::Position, site: Some(site), material_start, ..Self::new() }
    }
    fn goal(&self) -> Option<Value> {
        let site = self.site.as_ref()?;
        let pitch: f64 = if self.phase == DigPhase::Lever {
            -0.05 - 0.85 * (self.elapsed / 0.8).min(1.)
        } else if self.phase == DigPhase::Lift { -0.9 } else { -0.05 };
        let up = unit(add(mul(site.up, pitch.cos()), mul(site.heading, pitch.sin())));
        let right = unit(cross(site.up, site.heading));
        let forward = unit(cross(right, up));
        let height = match self.phase { DigPhase::Position => 0.06, DigPhase::Lower => -0.16, DigPhase::Lever => -0.12, DigPhase::Lift => 0.20 };
        let advance = match self.phase { DigPhase::Position=>0., DigPhase::Lower|DigPhase::Lever=>0.10, DigPhase::Lift=>0.04 };
        let position = add(add(site.position, mul(site.up, height)), mul(site.heading,advance));
        Some(json!({"position":position,"right":right,"up":up,"forward":forward}))
    }
    // Only fresh, non-zero force on the actual tool after the soil patch is
    // prepared counts as contact. Pair counts include soil-soil contacts.
    fn observe(&mut self, dt: f64, physical: &Value, moved: bool) -> Option<&'static str> {
        if physical["failure"].as_str().is_some_and(|s| !s.is_empty()) {
            return Some("physics_failed");
        }
        if physical["contact"]["feedback_stale"] == true {
            return Some("contact_feedback_unavailable");
        }
        if let Some(site)=&self.site {
            self.lifted_mass=physical["material"]["parcels"].as_array().into_iter().flatten()
                .filter(|p|p["detached"]==true)
                .filter_map(|p| {
                    let id=p["id"].as_u64()?;
                    let snapshot=&self.material_start.iter().find(|(i,_)|*i==id||p["ancestors"].as_array().is_some_and(|a|a.iter().any(|ancestor|ancestor.as_u64()==Some(*i))))?.1;
                    let before=if let Ok(reference)=v(&p["reference_position"]) {
                        crate::physics::material_point_at_snapshot(snapshot,reference)?
                    }else{v(&snapshot["position"]).ok()?};
                    let now=v(&p["position"]).ok()?;
                    (dot(sub(now,before),site.up)>=0.05).then(||p["mass"].as_f64()).flatten()
                }).sum();
        }
        if physical["blocked"] == true && !moved {
            self.stalled += dt;
        } else {
            self.stalled = 0.;
        }
        if self.stalled >= 0.75 {
            return Some("blocked");
        }
        self.elapsed += dt;
        match self.phase {
            DigPhase::Position => {
                let goal = self.goal().unwrap();
                let p = v(&physical["pose"]["position"]).unwrap_or([f64::INFINITY;3]);
                let delta = sub(p, v(&goal["position"]).unwrap());
                let aligned = v(&physical["pose"]["up"]).is_ok_and(|up|dot(up,v(&goal["up"]).unwrap())>0.99);
                if dot(delta,delta) < 0.025*0.025 && aligned {
                    self.phase = DigPhase::Lower; self.elapsed = 0.; self.stalled = 0.;
                } else if self.elapsed >= 2.5 { return Some("unreachable"); }
            }
            DigPhase::Lower => {
                let force = v(&physical["force"]).unwrap_or([0.; 3]);
                let effort = physical["tool_contact_effort"].as_f64().unwrap_or_else(||dot(force,force).sqrt());
                self.contact_seen |= physical["material"]["initialized"] == true && effort >= 1.;
                let at_ground = self.site.as_ref().is_none_or(|site| {
                    v(&physical["pose"]["position"]).is_ok_and(|p| {
                        let delta=sub(p,site.position);
                        let height=dot(delta,site.up);
                        let lateral=sub(delta,mul(site.up,height));
                        // The cutting edge must actually enter the material.
                        // First surface contact is not a completed insertion.
                        height < -0.10 && dot(lateral,lateral)<0.18*0.18
                    })
                });
                if physical["material"]["initialized"] == true && effort >= 1. && at_ground {
                    self.phase = DigPhase::Lever;
                    self.elapsed = 0.;
                } else if self.elapsed >= 4.0 {
                    return Some(if self.contact_seen {"insertion_blocked"} else {"no_soil_contact"});
                }
            }
            DigPhase::Lever => {
                let reached=self.goal().is_none_or(|goal| v(&physical["pose"]["up"]).is_ok_and(|up| dot(up,v(&goal["up"]).unwrap())>0.99));
                if self.elapsed >= 0.8 && reached {
                    self.phase = DigPhase::Lift;
                    self.elapsed = 0.;
                    self.stalled = 0.;
                } else if self.elapsed >= 2.5 { return Some("blocked"); }
            }
            DigPhase::Lift => {
                let reached = self.goal().is_none_or(|goal| v(&physical["pose"]["position"]).is_ok_and(|p| {
                    let delta=sub(p,v(&goal["position"]).unwrap()); dot(delta,delta)<0.03*0.03
                }));
                if self.elapsed >= 0.6 && reached {
                    return Some(if self.lifted_mass>=0.05 {"completed"} else {"empty_stroke"});
                }
                if self.elapsed >= 2. { return Some("blocked"); }
            }
        }
        None
    }
    fn name(&self) -> &'static str {
        match self.phase { DigPhase::Position => "positioning", DigPhase::Lower => "lowering", DigPhase::Lever => "levering", DigPhase::Lift => "lifting" }
    }
}
pub(crate) fn valid(x: &Value) -> bool {
    ["position", "forward", "up"]
        .iter()
        .all(|k| finite_vector(&x[*k]))
        && x["dt"]
            .as_f64()
            .is_some_and(|n| n.is_finite() && (0.0..=0.05).contains(&n))
        && x["action"].is_boolean()
        && x["reset"].is_boolean()
        && (x["aim"].is_null() || finite_vector(&x["aim"]))
        && (x["camera_position"].is_null() || finite_vector(&x["camera_position"]))
        && crate::base_system::layer_two::inverse_kinematics_system::valid_tool_reach(
            &x["tool_reach"],
        )
        && (x["tool_input"].is_null() || {
            let t = &x["tool_input"];
            ["held", "rotate", "drop"]
                .iter()
                .all(|k| t[*k].is_boolean())
                && pair(&t["delta"]).is_some()
                && (t["work"].is_null() || t["work"].is_boolean())
                && ["twist", "reach"].iter().all(|k| {
                    t[*k].is_null()
                        || t[*k]
                            .as_f64()
                            .is_some_and(|n| n.is_finite() && n.abs() <= 1000.)
                })
        })
}
pub(crate) fn valid_view(x: &Value) -> bool {
    x["instances"].as_array().is_some_and(|a| {
        (1..=2).contains(&a.len())
            && a.iter()
                .all(|r| r["entity"].as_u64().is_some_and(|n| n > 0) && finite_vector(&r["origin"]))
    }) && ["active", "equipped", "interaction_handled"]
        .iter()
        .all(|k| x[*k].is_boolean())
        && x["stage"].is_string()
}
pub(crate) fn valid_body_motion(x: &Value) -> bool {
    ["previous", "position", "forward"].iter().all(|k| finite_vector(&x[*k]))
        && crate::base_system::layer_two::inverse_kinematics_system::valid_tool_reach(&x["tool_reach"])
}
pub(crate) fn valid_body_constraint(x: &Value) -> bool {
    finite_vector(&x["position"]) && x["fraction"].as_f64().is_some_and(|x| (0.0..=1.).contains(&x))
}
#[derive(Default)]
pub(crate) struct State {
    event: Option<Value>,
    manipulation: Option<Value>,
    tool: Option<u64>,
    vertices: Vec<[f64; 6]>,
    origin: V,
    basis: [V; 3],
    grips: [V; 2],
    blade: Value,
    equipped: bool,
    physical_active: bool,
    held_was_down: bool,
    drop_was_down: bool,
    guided_dig: Option<GuidedDig>,
    guided_result: Option<&'static str>,
    work_was_down: bool,
    target_marker: Option<u64>,
    target: Value,
    target_age: f64,
    lateral: f64,
    elevation: f64,
    reach: f64,
    pitch: f64,
    yaw: f64,
    roll: f64,
    shoulder_height: f64,
    shoulder_half_width: f64,
    arm_min: [f64; 2],
    arm_max: [f64; 2],
    support_range: [f64; 2],
    torso_display_length: f64,
    bend_limit: f64,
    bend_response: f64,
    bend: f64,
    lowering: f64,
    bend_forward: V,
    full_request: Value,
}
impl State {
    fn constrain_body(&self, input: &Value) -> Result<Value, String> {
        let previous = v(&input["previous"])?;
        let wanted = v(&input["position"])?;
        if !self.equipped { return Ok(json!({"position":wanted,"fraction":1.})); }
        let forward = v(&input["forward"])?;
        let (neutral, _, maximum) = self.reach_frame(wanted, forward, &input["tool_reach"]);
        let shoulder = add(neutral[1], self.posture_offset(wanted));
        let hand = add(self.origin, self.offset(self.grips[1]));
        let travel = sub(wanted, previous);
        let from = sub(sub(shoulder, travel), hand);
        let radius = maximum[1] + 0.015;
        let distance = |t| { let d = add(from, mul(travel, t)); dot(d, d) };
        let fraction = if distance(1.) <= radius * radius || distance(1.) < distance(0.) {
            1.
        } else if distance(0.) > radius * radius {
            0.
        } else {
            let (mut low, mut high) = (0., 1.);
            for _ in 0..24 {
                let mid = (low + high) * 0.5;
                if distance(mid) <= radius * radius { low = mid; } else { high = mid; }
            }
            low
        };
        Ok(json!({"position":add(previous,mul(travel,fraction)),"fraction":fraction}))
    }
    #[cfg(test)]
    pub(crate) fn guided_active(&self) -> bool {
        self.guided_dig.is_some()
    }
    fn attributed(&self) -> Option<u64> {
        self.manipulation
            .as_ref()
            .or(self.event.as_ref())
            .and_then(|e| e["audicle"].as_u64())
    }
    fn begin_manipulation(&mut self, host: &mut Host) -> Result<(), String> {
        if self.manipulation.is_none() {
            let event = host.invoke("AudicleSystem.Create", Value::Null)?;
            host.invoke("AudicleSystem.Ready", event.clone())?;
            host.invoke("AudicleSystem.Go", event.clone())?;
            self.manipulation = Some(event);
        }
        host.active_audicle = self.attributed();
        Ok(())
    }
    fn finish_manipulation(&mut self, host: &mut Host, cancel: bool) -> Result<(), String> {
        if let Some(event) = self.manipulation.take() {
            host.active_audicle = event["audicle"].as_u64();
            host.invoke(
                if cancel {
                    "AudicleSystem.Cancel"
                } else {
                    "AudicleSystem.Finish"
                },
                event,
            )?;
        }
        host.active_audicle = self.attributed();
        Ok(())
    }
    fn close(&mut self, host: &mut Host, cancel: bool) -> Result<(), String> {
        let mut error = self.finish_manipulation(host, cancel).err();
        host.active_audicle = self.attributed();
        if let Err(e) = host.invoke("TerrainToolSystem.Release", Value::Null) {
            error = Some(e);
        }
        if let Some(id) = self.tool.take() {
            if let Err(e) = host.invoke("EntitySystem.Remove", json!({"entity":id})) {
                error = Some(e);
            }
        }
        if let Some(id) = self.target_marker.take() {
            if let Err(e) = host.invoke("EntitySystem.Remove", json!({"entity":id})) { error = Some(e); }
        }
        if let Some(event) = self.event.take() {
            host.active_audicle = event["audicle"].as_u64();
            if let Err(e) = host.invoke(
                if cancel {
                    "AudicleSystem.Cancel"
                } else {
                    "AudicleSystem.Finish"
                },
                event,
            ) {
                error = Some(e);
            }
        }
        *self = Self::default();
        if let Some(e) = error { Err(e) } else { Ok(()) }
    }
    fn prepare(&mut self, host: &mut Host, input: &Value) -> Result<(), String> {
        let bytes = std::fs::read(host.root().join("Procedures/Assets/shovel.json"))
            .map_err(|e| e.to_string())?;
        if bytes.len() > 2_000_000 {
            return Err("spade asset exceeds bound".into());
        }
        let asset: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        self.blade = asset["blade_collision"].clone();
        self.vertices =
            serde_json::from_value(asset["vertices"].clone()).map_err(|e| e.to_string())?;
        if self.vertices.is_empty()
            || self.vertices.len() % 3 != 0
            || self.vertices.iter().flatten().any(|n| !n.is_finite())
        {
            return Err("spade appearance must contain finite triangles".into());
        }
        self.grips = [
            v(&asset["grip_positions"]["left"])?,
            v(&asset["grip_positions"]["right"])?,
        ];
        // Use the same bind lengths, torso retarget, and display scale as the real rig.
        let model = host.invoke("PlayerModelSystem.Load", Value::Null)?;
        let joints = model["joints"]
            .as_array()
            .ok_or("missing player bind joints")?;
        let point = |name: &str| -> Result<V, String> {
            v(&joints
                .iter()
                .find(|j| j["name"] == name)
                .ok_or_else(|| format!("missing grip bind joint {name}"))?["rest"])
        };
        let config: Value = serde_json::from_slice(
            &std::fs::read(host.root().join("Procedures/player_body.json"))
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let number = |name: &str| -> Result<f64, String> {
            config[name]
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or_else(|| format!("missing grip body setting {name}"))
        };
        let scale = number("rig_scale")?;
        let torso = number("torso_length")?;
        if scale <= 0. || torso <= 0. {
            return Err("invalid grip rig proportions".into());
        }
        let root = point("Root")?;
        let chest = point("Ribs.001")?;
        let torso_ratio = dot(sub(chest, root), sub(chest, root)).sqrt() / torso;
        self.shoulder_height =
            number("rig_vertical_offset")? - 0.90 - number("standing_eye_offset")?
                + root[1] * scale
                + torso * torso_ratio * scale;
        self.shoulder_half_width = number("shoulder_half_width")? * torso_ratio * scale;
        self.torso_display_length = torso * torso_ratio * scale;
        self.bend_limit = number("tool_bend_limit_degrees")?.to_radians();
        self.bend_response = number("tool_bend_response")?;
        if !(0.0..=60f64.to_radians()).contains(&self.bend_limit)
            || !(1.0..=40.).contains(&self.bend_response)
        {
            return Err("invalid tool work posture settings".into());
        }
        for (i, side) in ["L", "R"].iter().enumerate() {
            let a = point(&format!("UpperArm.{side}"))?;
            let b = point(&format!("ForeArm.{side}"))?;
            let c = point(&format!("Wrist.{side}"))?;
            let upper = dot(sub(b, a), sub(b, a)).sqrt() * scale;
            let lower = dot(sub(c, b), sub(c, b)).sqrt() * scale;
            self.arm_min[i] = (upper - lower).abs() + 0.005;
            self.arm_max[i] = upper + lower - 0.015;
            if self.arm_max[i] <= self.arm_min[i] {
                return Err("invalid displayed arm reach".into());
            }
        }
        self.support_range =
            pair(&asset["support_grip_range"]).ok_or("spade support grip interval required")?;
        if self.support_range[0] < 0.35
            || self.support_range[1] > self.grips[1][1]
            || self.support_range[0] > self.support_range[1]
        {
            return Err("invalid spade support grip interval".into());
        }
        let p = v(&input["position"])?;
        let up = unit(p);
        let front = v(&input["forward"])?;
        let mut forward = sub(front, mul(up, dot(front, up)));
        if dot(forward, forward) < 1e-8 {
            forward = cross(
                up,
                if up[2].abs() < 0.8 {
                    [0., 0., 1.]
                } else {
                    [1., 0., 0.]
                },
            );
        }
        let forward = unit(forward);
        self.basis = [unit(cross(up, forward)), up, forward];
        let normal = unit(add(p, mul(forward, 0.65)));
        let height =
            host.invoke("PlanetSurfaceSystem.Height", json!({"position":normal}))?["height"]
                .as_f64()
                .ok_or("missing spade spawn height")?;
        self.origin = mul(normal, crate::terrain::RADIUS + height + 0.04);
        self.elevation = -1.50;
        self.reach = 0.60;
        self.lateral = 0.12;
        self.pitch = -0.5;
        self.bend_forward = forward;
        let event = host.invoke("AudicleSystem.Create", Value::Null)?;
        self.event = Some(event.clone());
        host.active_audicle = event["audicle"].as_u64();
        self.tool = host.invoke(
            "EntitySystem.Create",
            json!({"kind":"visual","shader":"material_surface","vertices":self.mesh()}),
        )?["entity"]
            .as_u64();
        if self.tool.is_none() {
            return Err("spade entity creation returned no identity".into());
        }
        host.invoke("AudicleSystem.Ready", event.clone())?;
        host.invoke("AudicleSystem.Go", event)?;
        // Prepare the same occupied ground at session start. Touching it with a
        // shovel no longer creates/reveals a fresh material fixture mid-stroke.
        let ground=host.invoke("TerrainToolSystem.Prepare",json!({"pose":self.pose(),"blade":self.blade,"patch_position":mul(unit(add(p,mul(forward,0.60))),crate::terrain::RADIUS),"dt":0.,"equipped":false,"release":false,"optional":true}))?;
        if ground["material"]["initialized"]==true { self.pitch=0.; }
        Ok(())
    }
    fn offset(&self, p: V) -> V {
        std::array::from_fn(|i| (0..3).map(|j| self.basis[j][i] * p[j]).sum())
    }
    fn mesh(&self) -> Vec<[f64; 6]> {
        self.vertices
            .iter()
            .map(|p| {
                let q = self.offset([p[0], p[1], p[2]]);
                [q[0], q[1], q[2], p[3], p[4], p[5]]
            })
            .collect()
    }
    fn pose(&self) -> Value {
        json!({"position":self.origin,"right":self.basis[0],"up":self.basis[1],"forward":self.basis[2]})
    }
    fn apply_pose(&mut self, pose: &Value) -> Result<(), String> {
        if !["position", "right", "up", "forward"]
            .iter()
            .all(|k| finite_vector(&pose[*k]))
            || !["right", "up", "forward"]
                .iter()
                .all(|k| v(&pose[*k]).is_ok_and(|a| (dot(a, a) - 1.).abs() < 1e-5))
            || dot(
                cross(v(&pose["right"])?, v(&pose["up"])?),
                v(&pose["forward"])?,
            ) < 0.99999
        {
            return Err("contact solver returned an invalid spade pose".into());
        }
        self.origin = v(&pose["position"])?;
        self.basis = [v(&pose["right"])?, v(&pose["up"])?, v(&pose["forward"])?];
        Ok(())
    }
    fn hit(&self, eye: V, ray: V) -> bool {
        let eye = sub(eye, self.origin);
        self.vertices.chunks_exact(3).any(|t| {
            let point = |p: &[f64; 6]| self.offset([p[0], p[1], p[2]]);
            let a = point(&t[0]);
            let e1 = sub(point(&t[1]), a);
            let e2 = sub(point(&t[2]), a);
            let h = cross(ray, e2);
            let det = dot(e1, h);
            if det.abs() < 1e-10 {
                return false;
            }
            let delta = sub(eye, a);
            let u = dot(delta, h) / det;
            if !(0.0..=1.).contains(&u) {
                return false;
            }
            let q = cross(delta, e1);
            let w = dot(ray, q) / det;
            let distance = dot(e2, q) / det;
            w >= 0. && u + w <= 1. && (0.0..=3.).contains(&distance)
        })
    }
    fn reach_frame(&self, eye: V, forward: V, observed: &Value) -> ([V; 2], [f64; 2], [f64; 2]) {
        // Prefer the previous displayed shoulder frame, translated with the player.
        // It also respects body yaw lag, rather than guessing that the torso follows the camera.
        if let (Ok(previous_eye), Some(shoulders)) =
            (v(&observed["eye"]), observed["shoulders"].as_array())
        {
            if shoulders.len() == 2 {
                if let (Ok(left), Ok(right), Some(min), Some(max)) = (
                    v(&shoulders[0]),
                    v(&shoulders[1]),
                    pair(&observed["arm_min"]),
                    pair(&observed["arm_max"]),
                ) {
                    if min
                        .iter()
                        .zip(max)
                        .all(|(a, b)| *a >= 0. && b > *a && b < 2.)
                    {
                        let old_posture = v(&observed["posture_offset"]).unwrap_or([0.; 3]);
                        let shift = sub(sub(eye, previous_eye), old_posture);
                        return (
                            [add(left, shift), add(right, shift)],
                            min,
                            max.map(|n| (n - 0.015).max(0.02)),
                        );
                    }
                }
            }
        }
        let up = unit(eye);
        let mut heading = sub(forward, mul(up, dot(forward, up)));
        if dot(heading, heading) < 1e-8 {
            heading = self.basis[2];
        }
        let right = unit(cross(unit(heading), up));
        let centre = add(eye, mul(up, self.shoulder_height));
        (
            [
                sub(centre, mul(right, self.shoulder_half_width)),
                add(centre, mul(right, self.shoulder_half_width)),
            ],
            self.arm_min,
            self.arm_max,
        )
    }
    fn posture_offset(&self, eye: V) -> V {
        sub(mul(
            add(
                mul(unit(eye), self.bend.cos() - 1.),
                mul(self.bend_forward, self.bend.sin()),
            ),
            self.torso_display_length,
        ),mul(unit(eye),self.lowering))
    }
    fn posture(&self) -> Value {
        json!({"bend_radians":self.bend,"forward":self.bend_forward,"lowering_metres":self.lowering})
    }
    fn update_posture(
        &mut self,
        eye: V,
        forward: V,
        neutral: [V; 2],
        maximum: [f64; 2],
        dt: f64,
    ) -> Result<(), String> {
        let up = unit(eye);
        // Lean from the shoulder midpoint toward both accepted grips, avoiding
        // a lateral bias from steering the whole torso toward just one hand.
        let offset = if self.equipped {
            let grip_midpoint = add(
                self.origin,
                mul(add(self.offset(self.grips[0]), self.offset(self.grips[1])), 0.5),
            );
            sub(grip_midpoint, mul(add(neutral[0], neutral[1]), 0.5))
        } else {
            forward
        };
        let horizontal = sub(offset, mul(up, dot(offset, up)));
        let fallback = unit(sub(forward, mul(up, dot(forward, up))));
        let wanted = if dot(horizontal, horizontal) > 1e-8 {
            unit(horizontal)
        } else {
            fallback
        };
        let response = 1. - (-self.bend_response * dt).exp();
        self.bend_forward = unit(add(
            mul(self.bend_forward, 1. - response),
            mul(wanted, response),
        ));
        let mut best_bend = 0.;
        let mut best_accepted_error = f64::INFINITY;
        let mut best_requested_error = f64::INFINITY;
        if self.equipped && self.full_request.is_object() {
            let pose = &self.full_request;
            let requested_origin = v(&pose["position"])?;
            let requested_axes = [v(&pose["right"])?, v(&pose["up"])?, v(&pose["forward"])?];
            let reach_error = |origin: V, axes: [V; 3], angle: f64| {
                let shift = sub(mul(
                    add(
                        mul(up, angle.cos() - 1.),
                        mul(self.bend_forward, angle.sin()),
                    ),
                    self.torso_display_length,
                ),mul(up,self.lowering));
                let mut grips = self.grips;
                let left = add(neutral[0], shift);
                let base = add(
                    add(origin, mul(axes[0], grips[0][0])),
                    mul(axes[2], grips[0][2]),
                );
                grips[0][1] = dot(sub(left, base), axes[1])
                    .clamp(self.support_range[0], self.support_range[1]);
                let mut error = 0f64;
                for i in 0..2 {
                    let hand = add(
                        origin,
                        add(
                            add(mul(axes[0], grips[i][0]), mul(axes[1], grips[i][1])),
                            mul(axes[2], grips[i][2]),
                        ),
                    );
                    let delta = sub(hand, add(neutral[i], shift));
                    error = error.max((dot(delta, delta).sqrt() - maximum[i]).max(0.));
                }
                error
            };
            for sample in 0..=24 {
                let angle = self.bend_limit * sample as f64 / 24.;
                let next_angle = self.bend + (angle - self.bend) * response;
                // Accepted grips are a hard priority, both at the target bend
                // and at this frame's smoothed bend. Only then minimize the
                // future request's reach error. Never stand up merely because
                // the requested spade has lifted while contact still holds it.
                let accepted_error = (reach_error(self.origin, self.basis, angle)
                    .max(reach_error(self.origin, self.basis, next_angle))
                    - 0.003)
                    .max(0.);
                let requested_error =
                    (reach_error(requested_origin, requested_axes, angle) - 0.003).max(0.);
                if accepted_error < best_accepted_error - 1e-9
                    || ((accepted_error - best_accepted_error).abs() <= 1e-9
                        && requested_error < best_requested_error - 1e-9)
                {
                    best_accepted_error = accepted_error;
                    best_requested_error = requested_error;
                    best_bend = angle;
                }
                if accepted_error == 0. && requested_error == 0. {
                    break;
                }
            }
        }
        self.bend += (best_bend - self.bend) * response;
        if (self.bend - best_bend).abs() < 1e-5 {
            self.bend = best_bend;
        }
        Ok(())
    }
    fn project_accepted_posture(
        &mut self,
        eye: V,
        neutral: [V; 2],
        minimum: [f64; 2],
        maximum: [f64; 2],
    ) {
        let up = unit(eye);
        let error = |angle: f64| {
            let shift = sub(mul(
                add(
                    mul(up, angle.cos() - 1.),
                    mul(self.bend_forward, angle.sin()),
                ),
                self.torso_display_length,
            ),mul(up,self.lowering));
            let shoulders = neutral.map(|p| add(p, shift));
            let mut grips = self.grips;
            let base = add(
                self.origin,
                add(
                    mul(self.basis[0], grips[0][0]),
                    mul(self.basis[2], grips[0][2]),
                ),
            );
            grips[0][1] = dot(sub(shoulders[0], base), self.basis[1])
                .clamp(self.support_range[0], self.support_range[1]);
            let mut worst = 0f64;
            for i in 0..2 {
                let hand = add(self.origin, self.offset(grips[i]));
                let delta = sub(hand, shoulders[i]);
                let distance = dot(delta, delta).sqrt();
                worst = worst.max((distance - maximum[i]).max(minimum[i] - distance).max(0.));
            }
            worst
        };
        let current = self.bend;
        if error(current) <= 1e-9 {
            return;
        }
        // Normal motion was already smoothed. Project only a constraint violation
        // introduced by the newly accepted physical pose; never move the tool.
        let mut feasible: Option<f64> = None;
        let mut best = current;
        let mut best_error = error(current);
        let step = self.bend_limit / 96.;
        for sample in 0..=96 {
            let angle = step * sample as f64;
            let e = error(angle);
            if e < best_error {
                best = angle;
                best_error = e;
            }
            if e <= 1e-9
                && feasible.is_none_or(|old| (angle - current).abs() < (old - current).abs())
            {
                feasible = Some(angle);
            }
        }
        if feasible.is_none() {
            // A feasible interval can be narrower than the angle samples.
            // Refine the best neighbourhood before treating reach as infeasible.
            let mut low = (best - step).max(0.);
            let mut high = (best + step).min(self.bend_limit);
            for _ in 0..32 {
                let a = low + (high - low) / 3.;
                let b = high - (high - low) / 3.;
                if error(a) < error(b) {
                    high = b;
                } else {
                    low = a;
                }
            }
            let angle = (low + high) * 0.5;
            let e = error(angle);
            if e < best_error {
                best = angle;
            }
            if e <= 1e-9 {
                feasible = Some(angle);
            }
        }
        if let Some(mut good) = feasible {
            // Refine toward the already smoothed bend, retaining the closest
            // feasible boundary instead of snapping to a sampled angle.
            let mut bad = current;
            for _ in 0..32 {
                let middle = (bad + good) * 0.5;
                if error(middle) <= 1e-9 {
                    good = middle;
                } else {
                    bad = middle;
                }
            }
            self.bend = good;
        } else {
            // Anatomically impossible poses remain visible to grip diagnostics;
            // the bounded best posture cannot stretch bones or move accepted soil.
            self.bend = best;
        }
    }
    fn slide_support(&mut self, pose: &Value, shoulder: V) -> Result<(), String> {
        let origin = v(&pose["position"])?;
        let axes = [v(&pose["right"])?, v(&pose["up"])?, v(&pose["forward"])?];
        let base = add(
            add(origin, mul(axes[0], self.grips[0][0])),
            mul(axes[2], self.grips[0][2]),
        );
        self.grips[0][1] =
            dot(sub(shoulder, base), axes[1]).clamp(self.support_range[0], self.support_range[1]);
        Ok(())
    }
    fn constrain_grasp(
        &mut self,
        pose: &mut Value,
        shoulders: [V; 2],
        minimum: [f64; 2],
        maximum: [f64; 2],
    ) -> Result<(), String> {
        let right = v(&pose["right"])?;
        let up = v(&pose["up"])?;
        let front = v(&pose["forward"])?;
        let mut origin = v(&pose["position"])?;
        // Alternate projection onto both arm-reach spheres. The support hand can slide
        // along the actual shaft; the upper hand remains on the D crossbar.
        for _ in 0..12 {
            pose["position"] = json!(origin);
            self.slide_support(pose, shoulders[0])?;
            for i in 0..2 {
                let grip = self.grips[i];
                let offset = add(
                    add(mul(right, grip[0]), mul(up, grip[1])),
                    mul(front, grip[2]),
                );
                let centre = sub(shoulders[i], offset);
                let delta = sub(origin, centre);
                let distance = dot(delta, delta).sqrt();
                if distance > maximum[i] {
                    origin = add(centre, mul(delta, maximum[i] / distance));
                } else if distance < minimum[i] {
                    origin = add(
                        centre,
                        mul(
                            if distance > 1e-8 { unit(delta) } else { front },
                            minimum[i],
                        ),
                    );
                }
            }
        }
        pose["position"] = json!(origin);
        Ok(())
    }
    fn requested_pose(&mut self, eye: V, forward: V, tool: &Value, dt: f64) -> Value {
        let delta = pair(&tool["delta"]).unwrap_or([0.; 2]);
        let rotating = tool["rotate"] == true;
        // Rotation may put the blade origin outside the ordinary translation range.
        // Zero input preserves it; subsequent translation approaches the range
        // continuously rather than snapping the origin back on gesture release.
        let translate = |value: f64, delta: f64, low: f64, high: f64| {
            (value + delta).clamp(value.min(low), value.max(high))
        };
        let previous_reach = self.reach;
        if rotating {
            self.yaw = (self.yaw + delta[0] * 0.006).clamp(-0.8, 0.8);
            self.pitch = (self.pitch + delta[1] * 0.006).clamp(-1.4, 0.8);
        } else if tool["held"] == true {
            self.lateral = translate(self.lateral, delta[0] * 0.0018, -0.30, 0.40);
            let lower = self.shoulder_height
                - self.grips[1][1]
                - self.arm_max.iter().copied().fold(0., f64::max)
                - self.torso_display_length;
            self.elevation = translate(self.elevation, -delta[1] * 0.0025, lower, -0.85);
        }
        self.roll = (self.roll + tool["twist"].as_f64().unwrap_or(0.) * 0.01).clamp(-1.2, 1.2);
        self.reach = translate(
            self.reach,
            tool["reach"].as_f64().unwrap_or(0.) * 0.002,
            0.35,
            0.95,
        );
        let radial = unit(eye);
        let mut heading = sub(forward, mul(radial, dot(forward, radial)));
        if dot(heading, heading) < 1e-8 {
            heading = self.basis[2];
        }
        let heading = unit(heading);
        let side = unit(cross(heading, radial));
        let facing = unit(add(mul(heading, self.yaw.cos()), mul(side, self.yaw.sin())));
        let right = unit(cross(radial, facing));
        let shaft = unit(add(
            mul(radial, self.pitch.cos()),
            mul(facing, self.pitch.sin()),
        ));
        let front = unit(cross(right, shaft));
        let wanted = [
            unit(add(
                mul(right, self.roll.cos()),
                mul(front, self.roll.sin()),
            )),
            shaft,
            unit(sub(
                mul(front, self.roll.cos()),
                mul(right, self.roll.sin()),
            )),
        ];
        let upper = self.grips[1];
        let offset = |basis: [V; 3]| {
            add(
                add(mul(basis[0], upper[0]), mul(basis[1], upper[1])),
                mul(basis[2], upper[2]),
            )
        };
        // The upper/D grip is the hand pivot, taken from the accepted physical
        // pose. Pinch/reach input can still translate that hand along the view.
        let pivot = add(
            add(self.origin, self.offset(upper)),
            mul(heading, self.reach - previous_reach),
        );
        let origin = if rotating {
            let origin = sub(pivot, offset(wanted));
            let relative = sub(origin, eye);
            self.reach = dot(relative, heading);
            self.lateral = dot(relative, side);
            self.elevation = dot(relative, radial);
            origin
        } else {
            add(
                add(add(eye, mul(heading, self.reach)), mul(side, self.lateral)),
                mul(radial, self.elevation),
            )
        };
        self.full_request =
            json!({"position":origin,"right":wanted[0],"up":wanted[1],"forward":wanted[2]});
        // Approach the requested grasp at bounded speed; pickup never teleports a blade through soil.
        let displacement = sub(origin, self.origin);
        let length = dot(displacement, displacement).sqrt();
        let origin = add(
            self.origin,
            mul(displacement, (1.2 * dt / length.max(1e-12)).min(1.)),
        );
        let amount = (dt * 4.).min(1.);
        let right = unit(add(mul(self.basis[0], 1. - amount), mul(wanted[0], amount)));
        let mut up = add(mul(self.basis[1], 1. - amount), mul(wanted[1], amount));
        up = unit(sub(up, mul(right, dot(up, right))));
        let front = unit(cross(right, up));
        // Apply the same pivot to the bounded orientation actually requested
        // this frame. Interpolating the blade origin independently would slide
        // the hand even though the complete target pose has the correct pivot.
        let origin = if rotating {
            sub(pivot, offset([right, up, front]))
        } else {
            origin
        };
        json!({"position":origin,"right":right,"up":up,"forward":front})
    }

    fn hold_accepted_pose(&mut self, eye: V, forward: V) {
        let radial = unit(eye);
        let mut heading = sub(forward, mul(radial, dot(forward, radial)));
        if dot(heading, heading) < 1e-8 { heading = self.basis[2]; }
        let heading = unit(heading);
        let side = unit(cross(heading, radial));
        let relative = sub(self.origin, eye);
        self.reach = dot(relative, heading);
        self.lateral = dot(relative, side);
        self.elevation = dot(relative, radial);
        let facing = unit(add(mul(heading, self.yaw.cos()), mul(side, self.yaw.sin())));
        self.pitch = dot(self.basis[1], facing).atan2(dot(self.basis[1], radial));
        let right = unit(cross(radial, facing));
        let front = unit(cross(right, self.basis[1]));
        self.roll = dot(self.basis[0], front).atan2(dot(self.basis[0], right));
    }
    fn update_target(&mut self, host: &mut Host, eye: V, camera: V, forward: V, dt: f64) -> Result<(),String> {
        self.target_age += dt;
        if !self.target.is_null() && self.target_age < 0.10 { return Ok(()); }
        self.target_age = 0.;
        self.target = host.invoke("TerrainToolSystem.Aim",json!({"position":camera,"forward":forward}))?;
        if self.target["hit"] != true { return Ok(()); }
        let p = v(&self.target["position"])?;
        let up = unit(p);
        let relative = sub(p,eye);
        let horizontal = sub(relative,mul(up,dot(relative,up)));
        let reach = dot(horizontal,horizontal).sqrt();
        self.target["reachable"] = json!((0.25..=1.05).contains(&reach) && dot(relative,relative)<2.6*2.6);
        let ready = self.target["physical_soil"] == true && self.target["reachable"] == true;
        let right = unit(cross(up,if up[2].abs()<0.8 {[0.,0.,1.]} else {[1.,0.,0.]}));
        let front = unit(cross(right,up));
        let mut vertices = Vec::new();
        for i in 0..12 {
            let at = |k: usize, r: f64| {
                let a=k as f64*std::f64::consts::TAU/12.;
                let q=add(add(mul(right,r*a.cos()),mul(front,r*a.sin())),mul(up,0.015));
                [q[0],q[1],q[2],if ready {0.25} else {0.9},if ready {0.9} else {0.35},0.15]
            };
            vertices.extend([at(i,0.035),at(i+1,0.035),at(i,0.05),at(i,0.05),at(i+1,0.035),at(i+1,0.05)]);
        }
        if let Some(id)=self.target_marker {
            host.invoke("EntitySystem.Update",json!({"entity":id,"kind":"visual","shader":"material_surface","vertices":vertices}))?;
        } else {
            self.target_marker=host.invoke("EntitySystem.Create",json!({"kind":"visual","shader":"material_surface","vertices":vertices}))?["entity"].as_u64();
        }
        Ok(())
    }
}
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let outer = host.active_audicle;
    let mut s = std::mem::take(&mut host.shovel);
    host.active_audicle = s.attributed().or(outer);
    let result = (|| match hook {
        "ConstrainMovement" => s.constrain_body(input),
        "Active" => Ok(json!(
            s.event.is_none()
                || s.equipped
                || s.physical_active
                || s.manipulation.is_some()
                || s.bend > 1e-5 || s.lowering > 1e-5
        )),
        "Abort" => { s.close(host, true)?; Ok(Value::Null) }
        "Close" => {
            s.close(host, false)?;
            Ok(Value::Null)
        }
        "Step" => {
            if input["reset"] == true && s.event.is_some() {
                s.close(host, true)?;
                host.active_audicle = outer;
            }
            if s.event.is_none() {
                s.prepare(host, input)?;
            }
            let eye = v(&input["position"])?;
            let forward = unit(v(&input["forward"])?);
            let camera = if input["camera_position"].is_null() {
                add(eye, s.posture_offset(eye))
            } else {
                v(&input["camera_position"])?
            };
            let ray = if input["aim"].is_null() {
                forward
            } else {
                unit(v(&input["aim"])?)
            };
            let raw = &input["tool_input"];
            let held = raw["held"] == true;
            let pressed = held && !s.held_was_down;
            let drop = raw["drop"] == true;
            let drop_pressed = drop && !s.drop_was_down;
            s.held_was_down = held;
            s.drop_was_down = drop;
            let tapped = input["touch"]["tap"] == true;
            let in_reach = dot(sub(eye, s.origin), sub(eye, s.origin)) < 9.;
            let target = in_reach && s.hit(camera, ray);
            let was_equipped = s.equipped;
            let dt = input["dt"].as_f64().unwrap();
            let mut picked_up = false;
            let mut released = false;
            if s.equipped && drop_pressed {
                s.equipped = false;
                released = true;
                s.begin_manipulation(host)?;
            } else if !s.equipped
                && in_reach
                && ((drop_pressed && input["touch"].is_null())
                    || ((pressed || tapped || input["action"] == true) && target))
            {
                s.equipped = true;
                picked_up = true;
                s.begin_manipulation(host)?;
            }
            let work = raw["work"] == true;
            let work_pressed = work && !s.work_was_down;
            s.work_was_down = work;
            if s.equipped && s.guided_dig.is_none() {
                if work_pressed || input["action"] == true { s.target_age = 1.; }
                s.update_target(host,eye,camera,ray,dt)?;
            }
            // The UI work button has a separate channel from camera gestures.
            // Releasing it cancels, and a completed attempt requires a new press.
            if !s.equipped || released || held || raw["rotate"] == true
                || (raw["work"].is_boolean() && !work)
            {
                if s.guided_dig.take().is_some() {
                    s.guided_result = Some("cancelled");
                    s.hold_accepted_pose(eye, forward);
                }
            } else if was_equipped && !picked_up && s.guided_dig.is_none()
                && (work_pressed || (raw["work"].is_null() && input["action"] == true))
            {
                if s.target["hit"] == true && s.target["physical_soil"] == true && s.target["reachable"] == true {
                    let position=v(&s.target["position"])?;
                    let up=unit(position);
                    let direction=sub(position,eye);
                    let heading=unit(sub(direction,mul(up,dot(direction,up))));
                    let material=host.invoke("TerrainToolSystem.Frame",Value::Null)?["material"].clone();
                    s.guided_dig = Some(GuidedDig::aimed(DigSite{position,up,heading},&material));
                    s.guided_result = None;
                    s.begin_manipulation(host)?;
                } else { s.guided_result = Some("no_reachable_soil"); }
            }
            let controlling = s.equipped && (held || raw["rotate"] == true || s.guided_dig.is_some());
            if controlling {
                s.begin_manipulation(host)?;
            }
            let mut motion = raw.clone();
            // The pickup click cannot also shove the newly grasped tool.
            if picked_up {
                motion = json!({"held":false,"rotate":false,"delta":[0.,0.]});
            }
            let (neutral, arm_min, arm_max) = s.reach_frame(eye, forward, &input["tool_reach"]);
            let mut desired = if s.equipped {
                s.requested_pose(eye, forward, &motion, dt)
            } else {
                s.pose()
            };
            if let Some(goal) = s.guided_dig.as_ref().and_then(GuidedDig::goal) {
                s.full_request=goal.clone();
                let goal_origin=v(&goal["position"])?;
                let delta=sub(goal_origin,s.origin);
                let amount=(0.6*dt/dot(delta,delta).sqrt().max(1e-12)).min(1.);
                let right=unit(add(mul(s.basis[0],1.-dt*4.),mul(v(&goal["right"])?,dt*4.)));
                let up=add(mul(s.basis[1],1.-dt*4.),mul(v(&goal["up"])?,dt*4.));
                let up=unit(sub(up,mul(right,dot(up,right))));
                desired=json!({"position":add(s.origin,mul(delta,amount)),"right":right,"up":up,"forward":unit(cross(right,up))});
            }
            let lower_target = if s.equipped {
                let pose=&s.full_request;
                let upper=add(v(&pose["position"])?,add(add(mul(v(&pose["right"])?,s.grips[1][0]),mul(v(&pose["up"])?,s.grips[1][1])),mul(v(&pose["forward"])?,s.grips[1][2])));
                let accepted=add(s.origin,s.offset(s.grips[1]));
                let height=dot(sub(upper,eye),unit(eye)).min(dot(sub(accepted,eye),unit(eye)));
                (s.shoulder_height-height-0.10).clamp(0.,0.25)
            } else {0.};
            s.lowering+=(lower_target-s.lowering)*(1.-(-10.*dt).exp());
            if (s.lowering-lower_target).abs()<1e-5 {s.lowering=lower_target;}
            s.update_posture(eye, forward, neutral, arm_max, dt)?;
            let offset = s.posture_offset(eye);
            let shoulders = neutral.map(|p| add(p, offset));
            if s.equipped {
                s.constrain_grasp(&mut desired, shoulders, arm_min, arm_max)?;
            }
            host.active_audicle = s.attributed().or(outer);
            let grasp = if s.equipped {
                json!({"points":s.grips,"shoulders":shoulders,"minimum":arm_min,"maximum":arm_max,"support_range":s.support_range})
            } else {
                Value::Null
            };
            let physical=host.invoke("TerrainToolSystem.Advance",json!({"pose":desired,"blade":s.blade,"control_point":s.grips[1],"grasp":grasp,"dt":input["dt"],"equipped":s.equipped,"release":released}))?;
            let previous_origin = s.origin;
            let previous_basis = s.basis;
            s.apply_pose(&physical["pose"])?;
            let moved = dot(sub(s.origin, previous_origin), sub(s.origin, previous_origin)) > 1e-10
                || (0..3).any(|i| dot(sub(s.basis[i], previous_basis[i]), sub(s.basis[i], previous_basis[i])) > 1e-8);
            if let Some(result) = s.guided_dig.as_mut().and_then(|dig| dig.observe(dt, &physical, moved)) {
                s.guided_dig = None;
                s.guided_result = Some(result);
                s.hold_accepted_pose(eye, forward);
            }
            if s.equipped {
                s.project_accepted_posture(eye, neutral, arm_min, arm_max);
                let final_shoulders = neutral.map(|p| add(p, s.posture_offset(eye)));
                s.slide_support(&physical["pose"], final_shoulders[0])?;
            }
            s.physical_active = physical["active"] == true;
            host.invoke("EntitySystem.Update",json!({"entity":s.tool.unwrap(),"kind":"visual","shader":"material_surface","vertices":s.mesh()}))?;
            let failed = physical["failure"].as_str().is_some_and(|e| !e.is_empty());
            if failed || (!s.equipped && !s.physical_active) {
                s.finish_manipulation(host, failed)?;
            } else if !controlling && !picked_up && !released && !s.physical_active {
                s.finish_manipulation(host, false)?;
            }
            let grips = if s.equipped {
                json!({"left":add(s.origin,s.offset(s.grips[0])),"right":add(s.origin,s.offset(s.grips[1])),"active":true})
            } else {
                Value::Null
            };
            let force = v(&physical["force"]).unwrap_or([0.; 3]);
            let contact_charge = (dot(force, force).sqrt() / 180.).clamp(0., 1.);
            let stage = if failed {
                "halted"
            } else if physical["blocked"] == true {
                "blocked"
            } else if s.guided_dig.is_some() {
                "guided_dig"
            } else if controlling {
                "manipulating"
            } else if s.equipped {
                "held"
            } else if s.physical_active {
                "settling"
            } else {
                "available"
            };
            let mut instances=vec![json!({"entity":s.tool.unwrap(),"origin":s.origin})];
            if s.equipped && s.target["hit"] == true {
                if let Some(marker)=s.target_marker { instances.push(json!({"entity":marker,"origin":s.target["position"]})); }
            }
            let work_feedback = if s.guided_dig.is_some() && physical["blocked"]==true {6}
            else if let Some(dig)=&s.guided_dig {
                match dig.phase { DigPhase::Position=>2,DigPhase::Lower=>3,DigPhase::Lever=>4,DigPhase::Lift=>5 }
            } else if work && s.guided_result == Some("blocked") {6}
            else if work && s.guided_result == Some("insertion_blocked") {6}
            else if work && s.guided_result == Some("no_soil_contact") {7}
            else if work && s.guided_result == Some("completed") {8}
            else if work && s.guided_result == Some("empty_stroke") {10}
            else if work {9}
            else if s.target["physical_soil"] == true && s.target["reachable"] == true {1} else {0};
            Ok(
                json!({"instances":instances,"work_feedback":work_feedback,"dig_target":s.target,"tool_pose":s.pose(),"tool_grip":grips,"local_grips":s.grips,"tool_posture":s.posture(),"posture_offset":s.posture_offset(eye),
                "active":s.equipped||s.physical_active||s.bend>1e-5||s.lowering>1e-5,"equipped":s.equipped,"interaction_handled":target||was_equipped||s.equipped||released,
                "stage":stage,"guided_phase":s.guided_dig.as_ref().map(GuidedDig::name),"guided_result":s.guided_result,"halted":failed,"charge":if s.equipped {contact_charge} else if target {0.2} else {0.},
                "color_mode":if s.equipped&&!picked_up {"dig"} else {"pickup"},"trigger":picked_up||released,
                "terrain_excavation":physical["material"]["changed"]==true,"physics":physical}),
            )
        }
        _ => Err("unknown shovel hook".into()),
    })();
    if result.is_err() && s.event.is_some() {
        let _ = s.close(host, true);
    }
    host.shovel = s;
    host.active_audicle = outer;
    result
}
