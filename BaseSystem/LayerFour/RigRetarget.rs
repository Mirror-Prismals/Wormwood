//! Cardinal PlayerRigSystem's coherent pelvis/torso retarget and length-preserving
//! skin transforms. Called only within PlayerRigSystem.Advance.
use crate::base_system::layer_two::player_model_system::{Model, math::*};
use serde_json::Value;
fn index(m: &Model, name: &str) -> Result<usize, String> {
    m.joints
        .iter()
        .position(|j| j.name == name)
        .ok_or_else(|| format!("missing retarget joint {name}"))
}
fn preserve(
    m: &Model,
    p: &mut [V],
    parent: &str,
    child: &str,
    preferred: Option<V>,
) -> Result<(), String> {
    let a = index(m, parent)?;
    let b = index(m, child)?;
    let rest = sub(m.joints[b].rest, m.joints[a].rest);
    let mut direction = preferred.unwrap_or_else(|| sub(p[b], p[a]));
    if len(direction) < 1e-5 {
        direction = rest;
    }
    p[b] = add(p[a], mul(unit(direction), len(rest)));
    Ok(())
}
#[derive(Clone, Copy)]
struct Transform {
    rest: V,
    target: V,
    axes: [V; 3],
}
impl Transform {
    fn apply(&self, p: V) -> V {
        let d = sub(p, self.rest);
        add(
            self.target,
            add(
                add(mul(self.axes[0], d[0]), mul(self.axes[1], d[1])),
                mul(self.axes[2], d[2]),
            ),
        )
    }
}
fn targets(model: &Model, pose: &Value) -> Result<Vec<V>, String> {
    let solved: Vec<V> =
        serde_json::from_value(pose["joints"].clone()).map_err(|e| e.to_string())?;
    let driven: Vec<bool> =
        serde_json::from_value(pose["driven"].clone()).map_err(|e| e.to_string())?;
    if solved.len() != model.joints.len() || driven.len() != solved.len() {
        return Err("pose joint count mismatch".into());
    }
    let root = index(model, "Root")?;
    let chest = index(model, "Ribs.001")?;
    let bind_pelvis = model.joints[root].rest;
    let bind_torso = len(sub(model.joints[chest].rest, bind_pelvis));
    let solved_torso = len(sub(solved[chest], solved[root]));
    if solved_torso < 1e-5 || bind_torso < 1e-5 {
        return Err("degenerate retarget torso".into());
    }
    let ratio = bind_torso / solved_torso;
    let mut targets: Vec<V> = model
        .joints
        .iter()
        .enumerate()
        .map(|(i, j)| {
            if driven[i] {
                add(bind_pelvis, mul(sub(solved[i], solved[root]), ratio))
            } else {
                j.rest
            }
        })
        .collect();
    let look = vector(&pose["look"])?;
    let pitch_weight = (look[1].clamp(-1., 1.).asin().abs() / 89f64.to_radians()).clamp(0., 1.);
    let torso_direction = if pose["torso_direction"].is_null() {
        [0., 1., 0.]
    } else {
        unit(vector(&pose["torso_direction"])?)
    };
    preserve(
        model,
        &mut targets,
        "Root",
        "Ribs.001",
        Some(torso_direction),
    )?;
    preserve(
        model,
        &mut targets,
        "Ribs.001",
        "Neck",
        Some(unit(mix([0., 1., 0.], look, 0.08 + 0.20 * pitch_weight))),
    )?;
    preserve(
        model,
        &mut targets,
        "Neck",
        "Head",
        Some(unit(mix([0., 1., 0.], look, 0.10 + 0.35 * pitch_weight))),
    )?;
    const CHAINS: [(&str, &str); 11] = [
        ("Root", "Ribs.001"),
        ("Ribs.001", "Neck"),
        ("Neck", "Head"),
        ("Thigh.L", "Shin.L"),
        ("Shin.L", "Foot.L"),
        ("Thigh.R", "Shin.R"),
        ("Shin.R", "Foot.R"),
        ("UpperArm.L", "ForeArm.L"),
        ("ForeArm.L", "Wrist.L"),
        ("UpperArm.R", "ForeArm.R"),
        ("ForeArm.R", "Wrist.R"),
    ];
    for (a, b) in &CHAINS[3..] {
        preserve(model, &mut targets, a, b, None)?;
    }
    // Final contact ankles are expressed in the displayed bind-space, after
    // torso retargeting. Re-solve the legs here so preserving bone lengths cannot
    // move a planted ankle away from the surface again.
    if let Some(feet) = pose["foot_placement"].as_array() {
        if feet.len() != 2 {
            return Err("two foot placements required".into());
        }
        for (side, suffix) in ["L", "R"].iter().enumerate() {
            let hip = index(model, &format!("Thigh.{suffix}"))?;
            let knee = index(model, &format!("Shin.{suffix}"))?;
            let ankle = index(model, &format!("Foot.{suffix}"))?;
            let a = len(sub(model.joints[knee].rest, model.joints[hip].rest));
            let b = len(sub(model.joints[ankle].rest, model.joints[knee].rest));
            if a < 1e-5 || b < 1e-5 {
                return Err("degenerate foot support leg".into());
            }
            let delta = sub(vector(&feet[side]["ankle"])?, targets[hip]);
            let wanted = len(delta);
            let axis = if wanted > 1e-8 {
                mul(delta, 1. / wanted)
            } else {
                [0., -1., 0.]
            };
            let distance = wanted.clamp((a - b).abs() + 1e-5, a + b - 1e-5);
            let along = (a * a - b * b + distance * distance) / (2. * distance);
            let radius = (a * a - along * along).max(0.).sqrt();
            let old_bend = sub(targets[knee], targets[hip]);
            let mut bend = sub(old_bend, mul(axis, dot(old_bend, axis)));
            if len(bend) < 1e-8 {
                let fallback = if axis[2].abs() < 0.8 {
                    [0., 0., 1.]
                } else {
                    [1., 0., 0.]
                };
                bend = sub(fallback, mul(axis, dot(fallback, axis)));
            }
            targets[knee] = add(targets[hip], add(mul(axis, along), mul(unit(bend), radius)));
            targets[ankle] = add(targets[hip], mul(axis, distance));
        }
    }
    Ok(targets)
}

#[derive(Clone)]
pub(super) struct FootShape {
    pub samples: [V; 5],
}
pub(super) fn foot_shapes(model: &Model) -> Result<[FootShape; 2], String> {
    let mut result = Vec::with_capacity(2);
    for suffix in ["L", "R"] {
        let foot = index(model, &format!("Foot.{suffix}"))?;
        let mut lower = [f64::INFINITY; 3];
        let mut upper = [f64::NEG_INFINITY; 3];
        for vertex in &model.vertices {
            let dominant = (0..4)
                .max_by(|&a, &b| vertex.weights[a].total_cmp(&vertex.weights[b]))
                .unwrap();
            let mut joint = Some(vertex.joints[dominant]);
            let mut belongs = false;
            while let Some(i) = joint {
                if i == foot {
                    belongs = true;
                    break;
                }
                joint = model.joints[i].parent;
            }
            if belongs {
                let offset = sub(vertex.p, model.joints[foot].rest);
                for i in 0..3 {
                    lower[i] = lower[i].min(offset[i]);
                    upper[i] = upper[i].max(offset[i]);
                }
            }
        }
        if lower.iter().chain(&upper).any(|x| !x.is_finite()) {
            return Err(format!("foot {suffix} has no sole vertices"));
        }
        // A conservative sole footprint measured from the authored mesh,
        // including toe descendants, instead of an assumed ankle clearance.
        result.push(FootShape {
            samples: [
                [lower[0], lower[1], lower[2]],
                [lower[0], lower[1], upper[2]],
                [upper[0], lower[1], lower[2]],
                [upper[0], lower[1], upper[2]],
                [
                    (lower[0] + upper[0]) * 0.5,
                    lower[1],
                    (lower[2] + upper[2]) * 0.5,
                ],
            ],
        });
    }
    result.try_into().map_err(|_| "foot shape count".into())
}
pub(super) fn ankle_targets(model: &Model, pose: &Value) -> Result<[V; 2], String> {
    let targets = targets(model, pose)?;
    Ok([
        targets[index(model, "Foot.L")?],
        targets[index(model, "Foot.R")?],
    ])
}
pub(super) fn skin(model: &Model, pose: &Value) -> Result<Vec<V>, String> {
    let targets = targets(model, pose)?;
    let driven: Vec<bool> =
        serde_json::from_value(pose["driven"].clone()).map_err(|e| e.to_string())?;
    let head = index(model, "Head")?;
    let look = vector(&pose["look"])?;
    const CHAINS: [(&str, &str); 11] = [
        ("Root", "Ribs.001"),
        ("Ribs.001", "Neck"),
        ("Neck", "Head"),
        ("Thigh.L", "Shin.L"),
        ("Shin.L", "Foot.L"),
        ("Thigh.R", "Shin.R"),
        ("Shin.R", "Foot.R"),
        ("UpperArm.L", "ForeArm.L"),
        ("ForeArm.L", "Wrist.L"),
        ("UpperArm.R", "ForeArm.R"),
        ("ForeArm.R", "Wrist.R"),
    ];
    let mut transforms = Vec::with_capacity(model.joints.len());
    for i in 0..model.joints.len() {
        // Unmapped spine, clavicle and toe bones inherit the nearest driven ancestor.
        let mut joint = Some(i);
        while let Some(j) = joint {
            if driven[j] {
                break;
            }
            joint = model.joints[j].parent;
        }
        let Some(j) = joint else {
            transforms.push(Transform {
                rest: [0.; 3],
                target: [0.; 3],
                axes: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            });
            continue;
        };
        let mut from = [0., 1., 0.];
        let mut to = from;
        if let Some((_, child)) = CHAINS
            .iter()
            .find(|(parent, _)| *parent == model.joints[j].name)
        {
            let child = index(model, child)?;
            from = sub(model.joints[child].rest, model.joints[j].rest);
            to = sub(targets[child], targets[j]);
        }
        if j == head {
            from = [0., 0., 1.];
            to = look;
        }
        if let Some(parent) = match model.joints[j].name.as_str() {
            "Wrist.L" => Some("ForeArm.L"),
            "Wrist.R" => Some("ForeArm.R"),
            _ => None,
        } {
            let parent = index(model, parent)?;
            from = sub(model.joints[j].rest, model.joints[parent].rest);
            to = sub(targets[j], targets[parent]);
            let side=if model.joints[j].name=="Wrist.L" {0}else{1};
            if let Ok(direction)=vector(&pose["hand_directions"][side]) {
                if len(direction)>0.5 {to=direction;}
            }
        }
        // Foot/toe geometry follows the sampled local grade, independently of
        // shin rotation. Airborne feet retain their ordinary level orientation.
        let side = match model.joints[j].name.as_str() {
            "Foot.L" => Some(0),
            "Foot.R" => Some(1),
            _ => None,
        };
        if let Some(side) = side {
            if pose["foot_placement"].is_array() {
                from = [0., 1., 0.];
                to = vector(&pose["foot_placement"][side]["up"])?;
            }
        }
        transforms.push(Transform {
            rest: model.joints[j].rest,
            target: targets[j],
            axes: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]].map(|v| between(v, from, to)),
        });
    }
    let origin = vector(&pose["origin"])?;
    let left = vector(&pose["left"])?;
    let up = vector(&pose["up"])?;
    let forward = vector(&pose["forward"])?;
    let scale = pose["scale"].as_f64().ok_or("rig scale missing")?;
    Ok(model
        .vertices
        .iter()
        .map(|v| {
            let dominant = (0..4)
                .max_by(|&a, &b| v.weights[a].total_cmp(&v.weights[b]))
                .unwrap();
            let p = if v.joints[dominant] == head {
                transforms[head].apply(v.p)
            } else {
                let mut p = [0.; 3];
                for i in 0..4 {
                    if v.weights[i] > 0. {
                        p = add(p, mul(transforms[v.joints[i]].apply(v.p), v.weights[i]));
                    }
                }
                p
            };
            add(
                origin,
                mul(
                    add(add(mul(left, p[0]), mul(up, p[1])), mul(forward, p[2])),
                    scale,
                ),
            )
        })
        .collect())
}
