//! Blender-skinned deer clips, interpolated on the presentation thread.
//! Topology and materials are shared; no asset I/O or rig work reaches audio.
use crate::audio::{add, cross, dot, length, mul, sub, unit, vector};
use serde_json::{Value, json};
use std::sync::OnceLock;
struct Model {
    faces: Vec<([usize; 3], [f32; 3])>,
    frames: Vec<Vec<[f32; 3]>>,
}
fn model() -> &'static Model {
    static MODEL: OnceLock<Model> = OnceLock::new();
    MODEL.get_or_init(|| {
        let bytes = include_bytes!("../../Procedures/Assets/Wildlife/deer.bin");
        assert_eq!(&bytes[..8], b"DEERSKIN");
        let mut cursor = 8;
        let mut word = || {
            let w = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
            cursor += 4;
            w
        };
        let nv = word() as usize;
        let nt = word() as usize;
        let nf = word() as usize;
        assert!(nv <= 8192 && nt <= 6000 && nf == 96);
        let mut faces = Vec::with_capacity(nt);
        for _ in 0..nt {
            let indices = [word() as usize, word() as usize, word() as usize];
            assert!(indices.iter().all(|i| *i < nv));
            let color = [
                f32::from_bits(word()),
                f32::from_bits(word()),
                f32::from_bits(word()),
            ];
            faces.push((indices, color));
        }
        let frames = (0..nf)
            .map(|_| {
                (0..nv)
                    .map(|_| {
                        [
                            f32::from_bits(word()),
                            f32::from_bits(word()),
                            f32::from_bits(word()),
                        ]
                    })
                    .collect()
            })
            .collect();
        assert_eq!(cursor, bytes.len());
        Model { faces, frames }
    })
}
#[derive(Clone, Debug, serde::Deserialize)]
pub(crate) struct PoseFrame {
    pub legs: [[[f64; 3]; 3]; 4],
    pub foot_up: [[f64; 3]; 4],
    pub neck: [f64; 3],
    pub head: [f64; 3],
    pub head_forward: [f64; 3],
}
#[derive(serde::Deserialize)]
struct SkinRig {
    schema_version: u32,
    rigid_head_vertices: Vec<usize>,
    bones: Vec<String>,
    weights: Vec<[f64; 14]>,
    hoof_weights: Vec<f64>,
    lengths: [[f64; 2]; 4],
    frames: Vec<PoseFrame>,
}
fn rig() -> &'static SkinRig {
    static RIG: OnceLock<SkinRig> = OnceLock::new();
    RIG.get_or_init(|| {
        let r: SkinRig = serde_json::from_str(include_str!(
            "../../Procedures/Assets/Wildlife/deer_rig.json"
        ))
        .expect("exported deer rig");
        assert_eq!(r.schema_version, 1);
        assert_eq!(r.bones.len(), 14);
        assert_eq!(r.frames.len(), 96);
        assert_eq!(r.weights.len(), model().frames[0].len());
        assert_eq!(r.hoof_weights.len(), r.weights.len());
        r
    })
}
fn mix_vector(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    std::array::from_fn(|i| a[i] * (1. - t) + b[i] * t)
}
fn mix_frame(a: &PoseFrame, b: &PoseFrame, t: f64) -> PoseFrame {
    PoseFrame {
        legs: std::array::from_fn(|i| {
            std::array::from_fn(|j| mix_vector(a.legs[i][j], b.legs[i][j], t))
        }),
        foot_up: std::array::from_fn(|i| unit(mix_vector(a.foot_up[i], b.foot_up[i], t))),
        neck: mix_vector(a.neck, b.neck, t),
        head: mix_vector(a.head, b.head, t),
        head_forward: unit(mix_vector(a.head_forward, b.head_forward, t)),
    }
}
fn pose_sample(start: usize, count: usize, time: f64, duration: f64) -> PoseFrame {
    let f = time.rem_euclid(duration) / duration * count as f64;
    let i = f.floor() as usize;
    mix_frame(
        &rig().frames[start + i],
        &rig().frames[start + (i + 1) % count],
        f.fract(),
    )
}
pub(crate) fn authored_pose(time: f64, stride: f64, run: f64, alert: f64) -> PoseFrame {
    let resting = mix_frame(
        &pose_sample(0, 48, time, 2.),
        &pose_sample(48, 24, time, 1.),
        alert,
    );
    mix_frame(&resting, &pose_sample(72, 24, stride * 0.4, 0.4), run)
}
pub(crate) fn leg_lengths() -> [[f64; 2]; 4] {
    rig().lengths
}
fn retarget(points: &mut [[f64; 3]], frame: &PoseFrame, pose: &Value) -> Result<(), String> {
    use crate::base_system::layer_two::player_model_system::math::{between, rotate};
    let mut shifted = frame.clone();
    if !pose["ik"]["body_offset"].is_null() {
        let offset = vector(&pose["ik"]["body_offset"])?;
        if length(offset) > 0.35 {
            return Err("deer body offset exceeds support bound".into());
        }
        for p in points.iter_mut() {
            *p = add(*p, offset);
        }
        for chain in &mut shifted.legs {
            for p in chain {
                *p = add(*p, offset);
            }
        }
        shifted.neck = add(shifted.neck, offset);
        shifted.head = add(shifted.head, offset);
    }
    let frame = &shifted;
    let mut chains = frame.legs;
    let mut foot_up = frame.foot_up;
    if !pose["ik"].is_null() {
        let rows = pose["ik"]["chains"]
            .as_array()
            .filter(|a| a.len() == 4)
            .ok_or("four solved deer limbs required")?;
        for i in 0..4 {
            chains[i] = serde_json::from_value(rows[i]["joints"].clone())
                .map_err(|_| "deer IK joints required")?;
            foot_up[i] = vector(&pose["ik"]["foot_up"][i])?;
            if chains[i]
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 8.)
                || !(0.9..=1.1).contains(&length(foot_up[i]))
            {
                return Err("invalid deer IK frame".into());
            }
        }
    }
    let number = |name: &str, low: f64, high: f64| -> Result<f64, String> {
        if pose[name].is_null() {
            return Ok(0.);
        }
        pose[name]
            .as_f64()
            .filter(|v| v.is_finite() && (low..=high).contains(v))
            .ok_or_else(|| format!("invalid deer {name}"))
    };
    let watch = number("watch", 0., 1.)?;
    let yaw = number("look_yaw", -2.45, 2.45)?;
    let pitch = number("look_pitch", -0.6, 0.6)?;
    let base_yaw = frame.head_forward[0].atan2(frame.head_forward[2]);
    let base_pitch = frame.head_forward[1].clamp(-1., 1.).asin();
    let yaw = base_yaw * (1. - watch) + yaw * watch;
    let pitch = base_pitch * (1. - watch) + pitch * watch;
    let neck_yaw = ((yaw - base_yaw) * 0.38).clamp(-0.85, 0.85);
    let neck_pitch = ((pitch - base_pitch) * 0.35).clamp(-0.22, 0.22);
    let y = [0., 1., 0.];
    let side = rotate([1., 0., 0.], y, neck_yaw);
    type Rotation = [[f64; 3]; 3];
    let axes = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    let apply = |r: Rotation, v: [f64; 3]| -> [f64; 3] {
        std::array::from_fn(|i| r[0][i] * v[0] + r[1][i] * v[1] + r[2][i] * v[2])
    };
    let rotation = |from, to| -> Rotation { axes.map(|v| between(v, from, to)) };
    let neck_rotation = axes.map(|v| rotate(rotate(v, y, neck_yaw), side, -neck_pitch));
    let neck_vector = |v| apply(neck_rotation, v);
    let neck_point = |p| add(frame.neck, neck_vector(sub(p, frame.neck)));
    let pivot = neck_point(frame.head);
    let old_direction = neck_vector(frame.head_forward);
    let direction = [
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        yaw.cos() * pitch.cos(),
    ];
    let head_rotation = rotation(old_direction, direction);
    let head_point = |p| add(pivot, apply(head_rotation, sub(neck_point(p), pivot)));
    let upper = [4, 6, 8, 10];
    let lower = [5, 7, 9, 11];
    let top_rotation: [Rotation; 4] = std::array::from_fn(|i| {
        rotation(
            sub(frame.legs[i][1], frame.legs[i][0]),
            sub(chains[i][1], chains[i][0]),
        )
    });
    let low_rotation: [Rotation; 4] = std::array::from_fn(|i| {
        rotation(
            sub(frame.legs[i][2], frame.legs[i][1]),
            sub(chains[i][2], chains[i][1]),
        )
    });
    let foot_rotation: [Rotation; 4] =
        std::array::from_fn(|i| rotation(apply(low_rotation[i], frame.foot_up[i]), foot_up[i]));
    for (i, p) in points.iter_mut().enumerate() {
        let original = *p;
        let weights = &rig().weights[i];
        let mut next = original;
        for leg in 0..4 {
            if weights[upper[leg]] == 0. && weights[lower[leg]] == 0. {
                continue;
            }
            let old = frame.legs[leg];
            let solved = chains[leg];
            if weights[upper[leg]] > 0. {
                let top = add(solved[0], apply(top_rotation[leg], sub(original, old[0])));
                next = add(next, mul(sub(top, original), weights[upper[leg]]));
            }
            if weights[lower[leg]] > 0. {
                let low = add(solved[1], apply(low_rotation[leg], sub(original, old[1])));
                let level = add(solved[2], apply(foot_rotation[leg], sub(low, solved[2])));
                let low = mix_vector(low, level, rig().hoof_weights[i]);
                next = add(next, mul(sub(low, original), weights[lower[leg]]));
            }
        }
        if weights[1] > 0. {
            next = add(next, mul(sub(neck_point(original), original), weights[1]));
        }
        let head_weight = weights[2] + weights[12] + weights[13];
        if head_weight > 0. {
            next = add(next, mul(sub(head_point(original), original), head_weight));
        }
        *p = next;
    }
    Ok(())
}
pub(crate) fn spec() -> Value {
    let m = model();
    let vertices: Vec<[f32; 6]> = m
        .faces
        .iter()
        .flat_map(|(ids, c)| {
            ids.iter().map(|i| {
                let p = m.frames[0][*i];
                [p[0], p[1], p[2], c[0], c[1], c[2]]
            })
        })
        .collect();
    json!({"kind":"visual","shader":"material_surface","vertices":vertices})
}
fn sample(
    m: &Model,
    vertex: usize,
    start: usize,
    count: usize,
    seconds: f64,
    duration: f64,
) -> [f64; 3] {
    let frame = seconds.rem_euclid(duration) / duration * count as f64;
    let a = frame.floor() as usize;
    let t = frame.fract();
    std::array::from_fn(|i| {
        m.frames[start + a][vertex][i] as f64 * (1. - t)
            + m.frames[start + (a + 1) % count][vertex][i] as f64 * t
    })
}
pub(crate) fn animate(vertices: &mut [[f32; 6]], instance: &Value) -> Result<(), String> {
    let m = model();
    if vertices.len() != m.faces.len() * 3 {
        return Err("deer topology mismatch".into());
    }
    let time = instance["deer_pose"]["time"]
        .as_f64()
        .filter(|x| x.is_finite())
        .ok_or("deer time required")?;
    let run = instance["deer_pose"]["run"]
        .as_f64()
        .filter(|x| (0. ..=1.).contains(x))
        .ok_or("deer run blend required")?;
    let alert = instance["deer_pose"]["alert"]
        .as_f64()
        .filter(|x| (0. ..=1.).contains(x))
        .ok_or("deer alert blend required")?;
    let stride = instance["deer_pose"]["stride"]
        .as_f64()
        .filter(|x| x.is_finite())
        .ok_or("deer stride required")?;
    let up = unit(vector(&instance["deer_pose"]["up"])?);
    let heading = vector(&instance["deer_pose"]["forward"])?;
    let forward = unit(sub(heading, mul(up, dot(heading, up))));
    let right = unit(cross(up, forward));
    if length(right) < 0.9 || length(up) < 0.9 {
        return Err("deer frame must be orthogonal".into());
    }
    let mut posed = Vec::with_capacity(m.frames[0].len());
    for i in 0..m.frames[0].len() {
        let idle = sample(m, i, 0, 48, time, 2.);
        let attentive = sample(m, i, 48, 24, time, 1.);
        let running = sample(m, i, 72, 24, stride * 0.4, 0.4);
        let local: [f64; 3] = std::array::from_fn(|axis| {
            let resting = idle[axis] * (1. - alert) + attentive[axis] * alert;
            resting * (1. - run) + running[axis] * run
        });
        posed.push(local);
    }
    let frame = authored_pose(time, stride, run, alert);
    if !instance["deer_pose"]["ik"].is_null() || !instance["deer_pose"]["watch"].is_null() {
        retarget(&mut posed, &frame, &instance["deer_pose"])?;
    }
    for p in &mut posed {
        *p = add(add(mul(right, p[0]), mul(up, p[1])), mul(forward, p[2]));
    }
    for (triangle, (ids, _)) in vertices.chunks_exact_mut(3).zip(&m.faces) {
        for (v, id) in triangle.iter_mut().zip(ids) {
            for axis in 0..3 {
                v[axis] = posed[*id][axis] as f32;
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deer_skin_has_grounded_finite_looping_running_frames() {
        let m = model();
        assert_eq!(m.faces.len(), 3686);
        for f in &m.frames {
            assert!(f.iter().flatten().all(|x| x.is_finite() && x.abs() < 4.));
            assert!(f.iter().map(|v| v[1]).fold(f32::INFINITY, f32::min) >= -1e-5);
        }
        let diff: f32 = m.frames[72]
            .iter()
            .zip(&m.frames[78])
            .map(|(a, b)| (a[1] - b[1]).abs() + (a[2] - b[2]).abs())
            .sum();
        assert!(diff > 5.);
        let mut a: Vec<[f32; 6]> = serde_json::from_value(spec()["vertices"].clone()).unwrap();
        let mut b = a.clone();
        let pose = |stride| json!({"deer_pose":{"time":0.,"run":1.,"alert":1.,"stride":stride,"up":[0.,1.,0.],"forward":[0.,0.,1.]}});
        animate(&mut a, &pose(0.)).unwrap();
        animate(&mut b, &pose(1.)).unwrap();
        assert_eq!(a, b);
    }
    #[test]
    fn deer_ik_skin_aims_head_without_stretching_antlers() {
        assert!(rig().rigid_head_vertices.len() > 20);
        assert!(
            rig()
                .rigid_head_vertices
                .iter()
                .all(|&i| rig().weights[i][2] > 0.999),
            "the entire muzzle/face/antlers must follow the head bone"
        );
        let frame = authored_pose(0., 0.2, 1., 1.);
        let lengths = leg_lengths();
        let chains:Vec<_>=(0..4).map(|i|json!({"root":add(frame.legs[i][0],[0.,-0.18,0.]),"mid":frame.legs[i][1],"end":frame.legs[i][2],"target":[frame.legs[i][2][0],0.02+(i%2)as f64*0.05,frame.legs[i][2][2]],"pole":add(frame.legs[i][1],[0.,0.,if i<2{0.8}else{-0.8}]),"lengths":lengths[i]})).collect();
        let mut ik = crate::base_system::layer_two::inverse_kinematics_system::chains::solve(
            &json!({"chains":chains}),
        )
        .unwrap();
        ik["body_offset"] = json!([0., -0.18, 0.]);
        ik["foot_up"] = serde_json::to_value([[0., 1., 0.]; 4]).unwrap();
        let mut points: Vec<[f64; 3]> = (0..model().frames[0].len())
            .map(|i| sample(model(), i, 72, 24, 0.08, 0.4))
            .collect();
        let original = points.clone();
        retarget(
            &mut points,
            &frame,
            &json!({"ik":ik,"watch":1.,"look_yaw":2.3,"look_pitch":0.05}),
        )
        .unwrap();
        assert!(
            points
                .iter()
                .flatten()
                .all(|v| v.is_finite() && v.abs() < 4.)
        );
        let head: Vec<_> = rig()
            .weights
            .iter()
            .enumerate()
            .filter(|(_, w)| w[2] > 0.999)
            .map(|(i, _)| i)
            .collect();
        let a = head[0];
        let b = *head
            .iter()
            .max_by(|&&a, &&b| {
                length(sub(original[a], original[head[0]]))
                    .total_cmp(&length(sub(original[b], original[head[0]])))
            })
            .unwrap();
        assert!(
            (length(sub(points[a], points[b])) - length(sub(original[a], original[b]))).abs()
                < 1e-6,
            "head and antlers remain rigid"
        );
        assert!(
            head.iter()
                .map(|&i| length(sub(points[i], original[i])))
                .sum::<f64>()
                / head.len() as f64
                > 0.25,
            "visible head skin must track the gaze"
        );
        for (leg, lower) in [5, 7, 9, 11].into_iter().enumerate() {
            let sole = points
                .iter()
                .enumerate()
                .filter(|(i, _)| rig().weights[*i][lower] > 0.999 && rig().hoof_weights[*i] > 0.999)
                .map(|(_, p)| p[1])
                .fold(f64::INFINITY, f64::min);
            let contact = 0.02 + (leg % 2) as f64 * 0.05;
            assert!(
                (sole - contact).abs() < 0.025,
                "solved hoof skin must meet its support: leg {leg} sole {sole} target {contact}"
            );
        }
    }
}
