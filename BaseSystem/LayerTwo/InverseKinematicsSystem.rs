//! Cardinal's standing player control skeleton: cached FABRIK limbs, smoothed
//! joints, input-driven alternating steps, and a head/body yaw split.
//! World-up and foot-height operations are adapted to Mirror's spherical surface.
use crate::{
    base_system::layer_two::player_model_system::{Joint, Model, math::*},
    host::Host,
};
use serde_json::{Value, json};
use std::f64::consts::PI;
#[derive(Default)]
pub(crate) struct State {
    joints: Vec<Joint>,
    settings: Value,
    initialized: bool,
    previous: Option<V>,
    facing: V,
    feet: [V; 2],
    swing: Option<usize>,
    next: usize,
    start: V,
    target: V,
    progress: f64,
    duration: f64,
    steps: u64,
    phase: f64,
    legs: [Option<[V; 3]>; 2],
    arms: [Option<[V; 3]>; 2],
    settling: f64,
    bob_weight: f64,
    grip_blend: f64,
    grip_targets: Option<[V; 2]>,
}
fn safe(v: V, fallback: V) -> V {
    if len(v) > 0.0001 { unit(v) } else { fallback }
}
fn smooth(old: V, next: V, dt: f64, response: f64) -> V {
    mix(old, next, 1. - (-response * dt.clamp(0., 0.1)).exp())
}
fn number(p: &Value, k: &str) -> f64 {
    p[k].as_f64().unwrap()
}
// The physical tool supplies world-space grips. IK never changes its pose.
fn valid_tool_grip(v: &Value) -> bool {
    v.is_null()
        || (v.is_object()
            && (v["active"].is_null() || v["active"].is_boolean())
            && (v["active"] == false
                || ["left", "right"].iter().all(|key| {
                    vector(&v[*key]).is_ok_and(|p| p.iter().all(|x| x.abs() <= 1_000_000_000.))
                })))
}
fn bind_point(joints: &[Joint], name: &str) -> V {
    joints.iter().find(|j| j.name == name).unwrap().rest
}
fn bind_length(joints: &[Joint], a: &str, b: &str) -> f64 {
    len(sub(bind_point(joints, a), bind_point(joints, b)))
}
pub(crate) fn valid_tool_posture(v: &Value) -> bool {
    v.is_null()
        || (v.is_object()
            && v["bend_radians"]
                .as_f64()
                .is_some_and(|n| n.is_finite() && (0.0..=60f64.to_radians()).contains(&n))
            && vector(&v["forward"]).is_ok_and(|p| len(p) > 0.5 && len(p) < 1.5)
            && (v["lowering_metres"].is_null() || v["lowering_metres"].as_f64().is_some_and(|x|x.is_finite()&&(0.0..=0.25).contains(&x))))
}
pub(crate) fn valid_tool_reach(v: &Value) -> bool {
    v.is_null()
        || (v.is_object()
            && vector(&v["eye"]).is_ok()
            && vector(&v["posture_offset"]).is_ok()
            && v["shoulders"]
                .as_array()
                .is_some_and(|a| a.len() == 2 && a.iter().all(|p| vector(p).is_ok()))
            && v["arm_min"].as_array().is_some_and(|a| {
                a.len() == 2
                    && a.iter().all(|n| {
                        n.as_f64()
                            .is_some_and(|n| n.is_finite() && (0.0..=3.).contains(&n))
                    })
            })
            && v["arm_max"].as_array().is_some_and(|a| {
                a.len() == 2
                    && a.iter().all(|n| {
                        n.as_f64()
                            .is_some_and(|n| n.is_finite() && (0.0..=3.).contains(&n))
                    })
            })
            && (0..2)
                .all(|i| v["arm_max"][i].as_f64().unwrap() > v["arm_min"][i].as_f64().unwrap()))
}
pub(crate) fn valid(v: &Value) -> bool {
    ["position", "velocity", "forward"]
        .iter()
        .all(|k| vector(&v[*k]).is_ok())
        && v["move"]
            .as_array()
            .is_some_and(|m| m.len() == 6 && m.iter().all(Value::is_boolean))
        && v["eye_height"]
            .as_f64()
            .is_some_and(|n| (0.5..=3.).contains(&n))
        && v["dt"].as_f64().is_some_and(|n| (0. ..=0.1).contains(&n))
        && v["grounded"].is_boolean()
        && v["flying"].is_boolean()
        && v["reset"].is_boolean()
        && valid_tool_grip(&v["tool_grip"])
        && valid_tool_posture(&v["tool_posture"])
}
pub(crate) fn valid_tool_feedback(v: &Value) -> bool {
    v.is_object()
        && v["active"].is_boolean()
        && v["blend"].as_f64().is_some_and(|n| (0. ..=1.).contains(&n))
        && ["left", "right"].iter().all(|key| vector(&v[*key]).is_ok())
        && ["left_error", "right_error"].iter().all(|key| {
            v[*key].is_null() || v[*key].as_f64().is_some_and(|n| n.is_finite() && n >= 0.)
        })
        && ["left_reachable", "right_reachable"]
            .iter()
            .all(|key| v[*key].is_boolean())
}
pub(crate) fn valid_pose(v: &Value) -> bool {
    v["joints"]
        .as_array()
        .is_some_and(|a| !a.is_empty() && a.len() <= 128 && a.iter().all(|v| vector(v).is_ok()))
        && v["driven"].as_array().is_some_and(|a| {
            Some(a.len()) == v["joints"].as_array().map(Vec::len) && a.iter().all(Value::is_boolean)
        })
        && ["origin", "left", "up", "forward", "look", "view_offset"]
            .iter()
            .all(|k| vector(&v[*k]).is_ok())
        && v["scale"]
            .as_f64()
            .is_some_and(|n| (0.01..=2.).contains(&n))
        && valid_tool_feedback(&v["tool_grip"])
        && valid_tool_reach(&v["tool_reach"])
}
// Direct port of solveTwoBoneFabrik/applyPoleVector, retaining the previous bend.
fn fabrik(
    root: V,
    target: V,
    pole: V,
    a: f64,
    b: f64,
    iterations: usize,
    mid: &mut V,
    end: &mut V,
    up: V,
    forward: V,
) {
    let down = mul(up, -1.);
    let delta = sub(target, root);
    if len(delta) >= a + b - 0.001 {
        let d = safe(delta, down);
        *mid = add(root, mul(d, a));
        *end = add(root, mul(d, a + b));
        return;
    }
    *mid = add(root, mul(safe(sub(*mid, root), down), a));
    *end = add(*mid, mul(safe(sub(*end, *mid), down), b));
    for _ in 0..iterations {
        *end = target;
        *mid = add(
            *end,
            mul(safe(sub(*mid, *end), safe(sub(root, *end), up)), b),
        );
        *mid = add(
            root,
            mul(safe(sub(*mid, root), safe(sub(target, root), down)), a),
        );
        *end = add(
            *mid,
            mul(safe(sub(*end, *mid), safe(sub(target, *mid), down)), b),
        );
    }
    let axis = sub(*end, root);
    if len(axis) <= 0.0001 {
        return;
    }
    let dir = unit(axis);
    let on_axis = add(root, mul(dir, dot(sub(*mid, root), dir)));
    let radius = len(sub(*mid, on_axis));
    if radius <= 0.0001 {
        return;
    }
    let pole_axis = add(root, mul(dir, dot(sub(pole, root), dir)));
    let bend = safe(sub(pole, pole_axis), safe(sub(*mid, on_axis), forward));
    *mid = add(on_axis, mul(bend, radius));
}
fn limb(
    cache: &mut Option<[V; 3]>,
    root: V,
    target: V,
    pole: V,
    a: f64,
    b: f64,
    iterations: usize,
    dt: f64,
    response: f64,
    arm: bool,
    up: V,
    forward: V,
) -> [V; 3] {
    let mut mid = cache.map(|p| p[1]).unwrap_or_else(|| {
        add(
            add(root, mul(up, -a * if arm { 0.70 } else { 0.75 })),
            mul(
                safe(sub(pole, root), mul(forward, if arm { -1. } else { 1. })),
                a * if arm { 0.30 } else { 0.35 },
            ),
        )
    });
    let mut end = cache.map(|p| p[2]).unwrap_or(target);
    fabrik(
        root, target, pole, a, b, iterations, &mut mid, &mut end, up, forward,
    );
    let result = if let Some(old) = cache {
        [
            smooth(old[0], root, dt, if arm { 24. } else { response.max(24.) }),
            smooth(old[1], mid, dt, if arm { 18. } else { response }),
            smooth(old[2], end, dt, if arm { 18. } else { response }),
        ]
    } else {
        [root, mid, end]
    };
    *cache = Some(result);
    result
}
fn settings(host: &Host) -> Result<Value, String> {
    let path = host.root().join("Procedures/player_body.json");
    let p: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if p["schema_version"] != 1 {
        return Err("unsupported player body settings".into());
    }
    let ranges = [
        ("head_turn_limit", 20., 150.),
        ("body_turn_response", 1., 40.),
        ("forward_turn_response", 1., 50.),
        ("hip_drop", 0., 1.6),
        ("foot_drop", 0.25, 2.2),
        ("hip_half_width", 0.02, 0.6),
        ("foot_half_width", 0.02, 0.6),
        ("foot_forward", -0.6, 0.8),
        ("upper_leg", 0.1, 1.6),
        ("lower_leg", 0.1, 1.6),
        ("knee_forward", -1., 1.5),
        ("knee_drop", -0.5, 1.),
        ("torso_length", 0.15, 1.5),
        ("neck_length", 0.05, 0.6),
        ("head_length", 0.08, 0.5),
        ("eye_forward_offset", 0.02, 0.4),
        ("shoulder_half_width", 0.05, 0.9),
        ("upper_arm", 0.1, 1.2),
        ("lower_arm", 0.1, 1.2),
        ("iterations", 1., 24.),
        ("run_reference_speed", 0.25, 100.),
        ("slow_step_duration", 0.25, 1.5),
        ("run_step_duration", 0.06, 1.5),
        ("max_travel_per_step", 0.3, 1.),
        ("slow_stride", 0.02, 0.4),
        ("run_stride", 0.02, 1.1),
        ("slow_lift", 0.005, 0.15),
        ("run_lift", 0.005, 0.35),
        ("run_joint_response", 24., 120.),
        ("arm_swing", 0., 0.5),
        ("tool_grip_response", 1., 80.),
        ("tool_bend_limit_degrees", 0., 60.),
        ("tool_bend_response", 1., 40.),
        ("standing_eye_offset", 0., 1.4),
        ("hinge_pivot_height", 0.02, 0.5),
        ("hinge_eye_forward", 0.01, 0.35),
        ("camera_boom", 0.02, 0.6),
        ("rig_scale", 0.01, 2.),
        ("rig_vertical_offset", -2., 2.),
        ("bob_min_speed", 0., 10.),
        ("bob_full_speed", 0.1, 100.),
        ("bob_attack", 0.01, 2.),
        ("bob_release", 0.01, 2.),
        ("bob_vertical", 0., 0.25),
        ("bob_lateral", 0., 0.25),
    ];
    for (k, lo, hi) in ranges {
        if !p[k]
            .as_f64()
            .is_some_and(|v| v.is_finite() && v >= lo && v <= hi)
        {
            return Err(format!("invalid player body setting {k}"));
        }
    }
    if number(&p, "run_step_duration") > number(&p, "slow_step_duration")
        || number(&p, "run_stride") < number(&p, "slow_stride")
        || number(&p, "run_lift") < number(&p, "slow_lift")
    {
        return Err("invalid slow/run gait ranges".into());
    }
    Ok(p)
}
#[path="IKChains.rs"]
pub(crate) mod chains;
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    if hook == "SolveChains" {return chains::solve(input);}
    let mut s = std::mem::take(&mut host.ik);
    let result = (|| match hook {
        "Configure" => {
            let model = Model::from_data(input)?;
            let p = settings(host)?;
            s = State {
                joints: model.joints,
                settings: p,
                ..Default::default()
            };
            for name in [
                "Root",
                "Ribs.001",
                "Neck",
                "Head",
                "Thigh.L",
                "Shin.L",
                "Foot.L",
                "Thigh.R",
                "Shin.R",
                "Foot.R",
                "UpperArm.L",
                "ForeArm.L",
                "Wrist.L",
                "UpperArm.R",
                "ForeArm.R",
                "Wrist.R",
            ] {
                if !s.joints.iter().any(|j| j.name == name) {
                    return Err(format!("rig requires {name}"));
                }
            }
            for (a, b) in [
                ("Root", "Ribs.001"),
                ("UpperArm.L", "ForeArm.L"),
                ("ForeArm.L", "Wrist.L"),
                ("UpperArm.R", "ForeArm.R"),
                ("ForeArm.R", "Wrist.R"),
            ] {
                let length = bind_length(&s.joints, a, b);
                if !length.is_finite() || length < 1e-5 {
                    return Err(format!("degenerate grip retarget bone {a}/{b}"));
                }
            }
            Ok(Value::Null)
        }
        "Active" => Ok(json!(
            s.settling > 0. || s.swing.is_some() || s.grip_blend > 0.
        )),
        "Release" => {
            s = State::default();
            Ok(Value::Null)
        }
        "Solve" => {
            if s.joints.is_empty() {
                return Err("IK not configured".into());
            }
            let p = &s.settings;
            let n = |k| number(p, k);
            let dt = input["dt"].as_f64().unwrap().clamp(0., 0.1);
            let eye = vector(&input["position"])?;
            let up = unit(eye);
            let look = unit(vector(&input["forward"])?);
            let camera_forward = safe(sub(look, mul(up, dot(look, up))), s.facing);
            let camera_right = safe(cross(camera_forward, up), [1., 0., 0.]);
            // Mirror stores the neutral eye; Cardinal's solver stores the capsule centre.
            let sitting=input["sitting"]==true;
            let center = sub(eye, mul(up, if sitting{0.35}else{n("standing_eye_offset")}));
            let delta = s.previous.map(|v| sub(center, v)).unwrap_or([0.; 3]);
            let reset = input["reset"] == true
                || !s.initialized
                || len(sub(delta, mul(up, dot(delta, up)))) > 2.5
                || dot(delta, up).abs() > 1.5;
            if reset {
                s.initialized = false;
                s.facing = camera_forward;
                s.swing = None;
                s.next = 0;
                s.steps = 0;
                s.progress = 0.;
                s.phase = 0.;
                s.legs = [None; 2];
                s.arms = [None; 2];
                s.bob_weight = 0.;
                s.grip_blend = 0.;
                s.grip_targets = None;
            }
            s.facing = safe(sub(s.facing, mul(up, dot(s.facing, up))), camera_forward);
            let held: [bool; 4] = std::array::from_fn(|i| input["move"][i] == true);
            let movement_held = held.iter().any(|v| *v);
            let yaw = dot(up, cross(s.facing, camera_forward))
                .atan2(dot(s.facing, camera_forward).clamp(-1., 1.));
            let limit = n("head_turn_limit").to_radians();
            let turn = if held[0] {
                yaw
            } else {
                yaw - yaw.clamp(-limit, limit)
            };
            s.facing = unit(rotate(
                s.facing,
                up,
                turn * (1.
                    - (-dt
                        * if held[0] {
                            n("forward_turn_response")
                        } else {
                            n("body_turn_response")
                        })
                    .exp()),
            ));
            let forward = s.facing;
            let right = unit(cross(forward, up));
            let left = mul(right, -1.);
            let torso_display_ratio = bind_length(&s.joints,"Root","Ribs.001")/n("torso_length")*n("rig_scale");
            let lowering = input["tool_posture"]["lowering_metres"].as_f64().unwrap_or(0.);
            let hip_center = sub(center, mul(up, n("hip_drop")+lowering/torso_display_ratio));
            let foot_center = add(
                sub(center, mul(up, n("foot_drop"))),
                mul(forward, n("foot_forward")),
            );
            let hips = [
                sub(hip_center, mul(right, n("hip_half_width"))),
                add(hip_center, mul(right, n("hip_half_width"))),
            ];
            let mut neutral = [
                sub(foot_center, mul(right, n("foot_half_width"))),
                add(foot_center, mul(right, n("foot_half_width"))),
            ];
            let velocity = vector(&input["velocity"])?;
            let horizontal = sub(velocity, mul(up, dot(velocity, up)));
            let speed = len(horizontal);
            let measured = (speed / n("run_reference_speed")).clamp(0., 1.);
            let weight = if movement_held {
                measured.max(0.04)
            } else {
                measured
            };
            let mut commanded = [0.; 3];
            for (held, dir) in held.iter().zip([
                camera_forward,
                mul(camera_forward, -1.),
                mul(camera_right, -1.),
                camera_right,
            ]) {
                if *held {
                    commanded = add(commanded, dir);
                }
            }
            let direction = if speed > 0.03 {
                mul(horizontal, 1. / speed)
            } else {
                safe(commanded, forward)
            };
            let grounded = input["grounded"] == true && input["flying"] != true && !sitting;
            // Preserve Cardinal's foot-plane offset, substituting radial surface samples for fixed Y.
            let clearance =
                input["eye_height"].as_f64().unwrap() - n("standing_eye_offset") - n("foot_drop");
            let land = |host: &mut Host, v: V| -> Result<V, String> {
                let h = host.invoke("PlanetSurfaceSystem.Height", json!({"position":v}))?["height"]
                    .as_f64()
                    .ok_or("surface height missing")?;
                Ok(mul(unit(v), 6_371_000. + h + clearance))
            };
            if grounded {
                for v in &mut neutral {
                    *v = land(host, *v)?;
                }
            }
            if reset || !grounded {
                s.feet = neutral;
                s.swing = None;
                s.progress = 0.;
            }
            if grounded {
                for v in &mut s.feet {
                    *v = land(host, *v)?;
                }
            }
            if grounded && s.swing.is_none() && movement_held {
                let i = s.next;
                s.swing = Some(i);
                s.next = 1 - i;
                s.progress = 0.;
                let curve = weight.sqrt();
                s.duration = n("slow_step_duration")
                    + (n("run_step_duration") - n("slow_step_duration")) * curve;
                if speed > 0.05 {
                    s.duration = s.duration.min(n("max_travel_per_step") / speed).max(0.06);
                }
                let stride = n("slow_stride") + (n("run_stride") - n("slow_stride")) * curve;
                let advance = (stride + (speed * s.duration * 0.70).clamp(0., 0.45)).min(0.75);
                s.start = s.feet[i];
                s.target = land(host, add(neutral[i], mul(direction, advance)))?;
            }
            let mut arc = 0.;
            let mut side_arc = 0.;
            let mut foot_lift = [0.; 2];
            if let Some(i) = s.swing {
                s.progress = (s.progress + dt / s.duration.max(0.05)).min(1.);
                let t = s.progress;
                let eased = t * t * (3. - 2. * t);
                arc = (PI * t).sin();
                side_arc = if i == 0 { arc } else { -arc };
                let lift =
                    (n("slow_lift") + (n("run_lift") - n("slow_lift")) * weight.sqrt()) * arc;
                s.feet[i] = add(mix(s.start, s.target, eased), mul(up, lift));
                foot_lift[i] = lift;
                s.phase = (s.steps as f64 + t) * PI;
                if t >= 1. {
                    s.feet[i] = s.target;
                    s.swing = None;
                    s.progress = 0.;
                    s.steps += 1;
                    arc = 0.;
                    side_arc = 0.;
                    s.phase = s.steps as f64 * PI;
                }
            }
            if sitting {
                s.swing=None;
                for i in 0..2 {s.feet[i]=add(hips[i],add(mul(forward,0.72),mul(up,-0.12)));}
            }
            let response = 18. + (n("run_joint_response") - 18.) * weight;
            let mut legs = [[[0.; 3]; 3]; 2];
            for i in 0..2 {
                let pole = add(
                    add(hips[i], mul(forward, n("knee_forward"))),
                    add(
                        mul(up, if sitting{0.18}else{-n("knee_drop")}),
                        mul(
                            right,
                            n("hip_half_width") * 0.25 * if i == 0 { -1. } else { 1. },
                        ),
                    ),
                );
                legs[i] = limb(
                    &mut s.legs[i],
                    hips[i],
                    s.feet[i],
                    pole,
                    n("upper_leg"),
                    n("lower_leg"),
                    n("iterations") as usize,
                    dt,
                    response,
                    false,
                    up,
                    forward,
                );
            }
            let pelvis = mul(add(legs[0][0], legs[1][0]), 0.5);
            let bend = input["tool_posture"]["bend_radians"]
                .as_f64()
                .unwrap_or(0.)
                .clamp(0., n("tool_bend_limit_degrees").to_radians());
            let bend_forward = vector(&input["tool_posture"]["forward"]).unwrap_or(forward);
            let bend_forward = safe(sub(bend_forward, mul(up, dot(bend_forward, up))), forward);
            let torso_direction = unit(add(mul(up, bend.cos()), mul(bend_forward, bend.sin())));
            let control_posture_offset = sub(mul(sub(torso_direction, up), n("torso_length")),mul(up,lowering/torso_display_ratio));
            let posture_offset = mul(control_posture_offset, torso_display_ratio);
            let chest = add(pelvis, mul(torso_direction, n("torso_length")));
            // Original PlayerSpatialSystem neck hinge and horizontal camera boom.
            let pitch = dot(look, up).clamp(-1., 1.).asin();
            let head_up = safe(
                sub(mul(up, pitch.cos()), mul(camera_forward, pitch.sin())),
                up,
            );
            let neutral_eye = add(
                mul(up, n("hinge_pivot_height")),
                mul(forward, n("hinge_eye_forward")),
            );
            let aimed_eye = add(
                mul(head_up, n("hinge_pivot_height")),
                mul(look, n("hinge_eye_forward")),
            );
            let hinge = add(
                sub(aimed_eye, neutral_eye),
                mul(camera_forward, n("camera_boom")),
            );
            let head = add(
                sub(add(eye, hinge), mul(look, n("eye_forward_offset"))),
                control_posture_offset,
            );
            let pitch_weight = (pitch.abs() / 89f64.to_radians()).clamp(0., 1.);
            let head_direction = safe(mix(up, look, 0.10 + 0.35 * pitch_weight), up);
            let wanted_neck = sub(head, mul(head_direction, n("head_length")));
            let neck_delta = sub(wanted_neck, chest);
            let neck = if len(neck_delta) > n("neck_length") {
                add(chest, mul(safe(neck_delta, up), n("neck_length")))
            } else {
                wanted_neck
            };
            let grip_requested =
                input["tool_grip"].is_object() && input["tool_grip"]["active"] != false;
            if grip_requested {
                s.grip_targets = Some([
                    vector(&input["tool_grip"]["left"])?,
                    vector(&input["tool_grip"]["right"])?,
                ]);
            }
            let blend_target = if grip_requested { 1. } else { 0. };
            s.grip_blend +=
                (blend_target - s.grip_blend) * (1. - (-n("tool_grip_response") * dt).exp());
            // Equipped material tools already have a physical grasp. Blending
            // their wrists from locomotion creates visibly disconnected hands.
            if grip_requested && input["tool_grip"]["locked"] == true {
                s.grip_blend = 1.;
            }
            if (s.grip_blend - blend_target).abs() < 1e-4 {
                s.grip_blend = blend_target;
            }
            if s.grip_blend == 0. && !grip_requested {
                s.grip_targets = None;
            }
            // Invert PlayerRig's pelvis/torso retarget and presentation scale.
            // The body's camera offset cancels its view offset in world space.
            let rig_scale = n("rig_scale");
            let retarget_ratio = bind_length(&s.joints, "Root", "Ribs.001") / n("torso_length");
            let display_origin = sub(add(
                eye,
                mul(
                    up,
                    n("rig_vertical_offset") - 0.90 - n("standing_eye_offset"),
                ),
            ),mul(up,lowering));
            let bind_pelvis = bind_point(&s.joints, "Root");
            let mut pelvis_appearance = add(
                display_origin,
                mul(
                    add(
                        add(mul(left, bind_pelvis[0]), mul(up, bind_pelvis[1])),
                        mul(forward, bind_pelvis[2]),
                    ),
                    rig_scale,
                ),
            );
            if sitting {pelvis_appearance=sub(eye,mul(up,0.35));}
            let control_to_world = |point: V| {
                add(
                    pelvis_appearance,
                    mul(sub(point, pelvis), retarget_ratio * rig_scale),
                )
            };
            let swing = s.phase.sin() * n("arm_swing") * weight;
            let mut arms = [[[0.; 3]; 3]; 2];
            let mut hand_positions = [[0.; 3]; 2];
            let mut shoulders_world = [[0.; 3]; 2];
            let mut arm_min = [0.; 2];
            let mut arm_max = [0.; 2];
            let mut reachable = [false; 2];
            let mut grip_errors = [None; 2];
            for i in 0..2 {
                let sign = if i == 0 { -1. } else { 1. };
                let shoulder = add(chest, mul(right, n("shoulder_half_width") * sign));
                let neutral_target = add(
                    add(shoulder, mul(up, -(n("upper_arm") + n("lower_arm")) * 0.88)),
                    mul(forward, swing * sign),
                );
                let (upper_name, forearm_name, wrist_name) = if i == 0 {
                    ("UpperArm.L", "ForeArm.L", "Wrist.L")
                } else {
                    ("UpperArm.R", "ForeArm.R", "Wrist.R")
                };
                let bind_upper = bind_length(&s.joints, upper_name, forearm_name);
                let bind_lower = bind_length(&s.joints, forearm_name, wrist_name);
                shoulders_world[i] = control_to_world(shoulder);
                arm_min[i] = (bind_upper - bind_lower).abs() * rig_scale;
                arm_max[i] = (bind_upper + bind_lower) * rig_scale;
                let upper =
                    n("upper_arm") + (bind_upper / retarget_ratio - n("upper_arm")) * s.grip_blend;
                let lower =
                    n("lower_arm") + (bind_lower / retarget_ratio - n("lower_arm")) * s.grip_blend;
                let mut target = neutral_target;
                if let Some(grips) = s.grip_targets {
                    let wanted = add(
                        pelvis,
                        mul(
                            sub(grips[i], pelvis_appearance),
                            1. / (retarget_ratio * rig_scale),
                        ),
                    );
                    target = mix(neutral_target, wanted, s.grip_blend);
                    let distance = len(sub(grips[i], control_to_world(shoulder)));
                    reachable[i] = distance <= (bind_upper + bind_lower) * rig_scale + 0.002
                        && distance + 0.002 >= (bind_upper - bind_lower).abs() * rig_scale;
                    // Clamp to anatomical reach without teleporting the physical tool.
                    let delta = sub(target, shoulder);
                    let distance = len(delta).clamp(
                        (upper - lower).abs() + 1e-5,
                        (upper + lower - 1e-5).max((upper - lower).abs() + 1e-5),
                    );
                    target = add(shoulder, mul(safe(delta, forward), distance));
                }
                let pole = mix(
                    add(add(shoulder, mul(forward, -0.35)), mul(right, 0.08 * sign)),
                    add(
                        add(add(shoulder, mul(forward, 0.24)), mul(right, 0.34 * sign)),
                        mul(up, -0.20),
                    ),
                    s.grip_blend,
                );
                arms[i] = limb(
                    &mut s.arms[i],
                    shoulder,
                    target,
                    pole,
                    upper,
                    lower,
                    n("iterations") as usize,
                    dt,
                    18.,
                    true,
                    up,
                    forward,
                );
                if s.grip_targets.is_some() && s.grip_blend > 0. {
                    // Smoothing seeds the elbow and wrist, but cannot move the
                    // final grip chain off its actual shoulder/blended target.
                    let mut elbow = arms[i][1];
                    let mut wrist = arms[i][2];
                    fabrik(
                        shoulder,
                        target,
                        pole,
                        upper,
                        lower,
                        n("iterations") as usize,
                        &mut elbow,
                        &mut wrist,
                        up,
                        forward,
                    );
                    arms[i] = [shoulder, elbow, wrist];
                    s.arms[i] = Some(arms[i]);
                }
                // Match the length-preserving display retarget, including its scale.
                let elbow = add(
                    control_to_world(arms[i][0]),
                    mul(
                        safe(sub(arms[i][1], arms[i][0]), mul(up, -1.)),
                        bind_upper * rig_scale,
                    ),
                );
                hand_positions[i] = add(
                    elbow,
                    mul(
                        safe(sub(arms[i][2], arms[i][1]), forward),
                        bind_lower * rig_scale,
                    ),
                );
                if let Some(grips) = s.grip_targets {
                    grip_errors[i] = Some(len(sub(hand_positions[i], grips[i])));
                }
            }
            // Camera sway uses the same step phase, never an unrelated oscillator.
            let target_weight = if grounded && s.swing.is_some() {
                (0.20 + weight * 0.80).clamp(0., 1.)
            } else if grounded && speed > n("bob_min_speed") {
                (speed / n("bob_full_speed")).clamp(0., 1.)
            } else {
                0.
            };
            let blend = dt
                / if target_weight > s.bob_weight {
                    n("bob_attack")
                } else {
                    n("bob_release")
                };
            s.bob_weight += (target_weight - s.bob_weight) * blend.clamp(0., 1.);
            if s.bob_weight <= 1e-4 {
                s.bob_weight = 0.;
            }
            let view_offset = add(
                add(hinge, posture_offset),
                add(
                    mul(camera_right, n("bob_lateral") * side_arc * s.bob_weight),
                    mul(up, n("bob_vertical") * arc * s.bob_weight),
                ),
            );
            let local = |v: V| {
                let d = sub(v, center);
                [dot(d, left), dot(d, up), dot(d, forward)]
            };
            let mut joints: Vec<V> = s.joints.iter().map(|j| j.rest).collect();
            let mut driven = vec![false; joints.len()];
            let hand_directions:Vec<_>=["left_fingers","right_fingers"].iter().map(|key| {
                vector(&input["tool_grip"][*key]).ok().map(|v|[dot(v,left),dot(v,up),dot(v,forward)])
            }).collect();
            for (name, point) in [
                ("Root", pelvis),
                ("Ribs.001", chest),
                ("Neck", neck),
                ("Head", head),
                ("Thigh.L", legs[0][0]),
                ("Shin.L", legs[0][1]),
                ("Foot.L", legs[0][2]),
                ("Thigh.R", legs[1][0]),
                ("Shin.R", legs[1][1]),
                ("Foot.R", legs[1][2]),
                ("UpperArm.L", arms[0][0]),
                ("ForeArm.L", arms[0][1]),
                ("Wrist.L", arms[0][2]),
                ("UpperArm.R", arms[1][0]),
                ("ForeArm.R", arms[1][1]),
                ("Wrist.R", arms[1][2]),
            ] {
                let i = s.joints.iter().position(|j| j.name == name).unwrap();
                joints[i] = local(point);
                driven[i] = true;
            }
            if reset || movement_held || speed > 0.02 || turn.abs() > 0.002 || s.swing.is_some() {
                s.settling = 1.;
            } else {
                s.settling = (s.settling - dt).max(0.);
            }
            s.previous = Some(center);
            s.initialized = true;
            let origin = sub(sub(
                mul(
                    up,
                    n("rig_vertical_offset") - 0.90 - n("standing_eye_offset"),
                ),
                view_offset,
            ),mul(up,lowering));
            Ok(
                json!({"hand_directions":hand_directions,"joints":joints,"driven":driven,"origin":origin,"left":left,"up":up,"forward":forward,"look":[dot(look,left),dot(look,up),dot(look,forward)],"scale":n("rig_scale"),"view_offset":view_offset,"foot_lift":foot_lift,"torso_direction":[dot(torso_direction,left),dot(torso_direction,up),dot(torso_direction,forward)],"posture_offset":posture_offset,"tool_reach":{"eye":eye,"shoulders":shoulders_world,"arm_min":arm_min,"arm_max":arm_max,"posture_offset":posture_offset},"tool_grip":{"active":grip_requested,"blend":s.grip_blend,"left":hand_positions[0],"right":hand_positions[1],"left_error":grip_errors[0],"right_error":grip_errors[1],"left_reachable":reachable[0],"right_reachable":reachable[1]}}),
            )
        }
        _ => Err("unknown IK hook".into()),
    })();
    host.ik = s;
    result
}
