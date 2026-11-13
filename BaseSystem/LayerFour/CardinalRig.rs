//! Blender-skinned cardinal clips, interpolated on the presentation thread.
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
        let bytes = include_bytes!("../../Procedures/Assets/Birds/cardinal.bin");
        assert_eq!(&bytes[..8], b"CARDINAL");
        let mut cursor = 8;
        let mut word = || {
            let w = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
            cursor += 4;
            w
        };
        let nv = word() as usize;
        let nt = word() as usize;
        let nf = word() as usize;
        assert!(nv <= 8192 && nt <= 6000 && nf == 88);
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
        return Err("cardinal topology mismatch".into());
    }
    let time = instance["bird_pose"]["time"]
        .as_f64()
        .filter(|x| x.is_finite())
        .ok_or("cardinal time required")?;
    let flight = instance["bird_pose"]["flight"]
        .as_f64()
        .filter(|x| (0. ..=1.).contains(x))
        .ok_or("cardinal flight blend required")?;
    let call = instance["bird_pose"]["call"]
        .as_f64()
        .filter(|x| (0. ..=1.).contains(x))
        .ok_or("cardinal call blend required")?;
    let up = unit(vector(&instance["bird_pose"]["up"])?);
    let heading = vector(&instance["bird_pose"]["forward"])?;
    let forward = unit(sub(heading, mul(up, dot(heading, up))));
    let right = unit(cross(up, forward));
    if length(right) < 0.9 || length(up) < 0.9 {
        return Err("cardinal frame must be orthogonal".into());
    }
    let mut posed = Vec::with_capacity(m.frames[0].len());
    for i in 0..m.frames[0].len() {
        let perch = sample(m, i, 0, 48, time, 2.);
        let flying = sample(m, i, 48, 16, time, 0.16);
        let calling = sample(m, i, 64, 24, time, 1.);
        let local: [f64; 3] = std::array::from_fn(|axis| {
            let resting = perch[axis] * (1. - call) + calling[axis] * call;
            resting * (1. - flight) + flying[axis] * flight
        });
        posed.push(add(
            add(mul(right, local[0]), mul(up, local[1])),
            mul(forward, local[2]),
        ));
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
    fn pose(time: f64, flight: f64, up: [f64; 3], forward: [f64; 3]) -> Value {
        json!({"bird_pose":{"time":time,"flight":flight,"call":0.,"up":up,"forward":forward}})
    }
    #[test]
    fn cardinal_clips_are_finite_bounded_and_move_wings() {
        let m = model();
        assert!(m.faces.len() <= 6000);
        for frame in &m.frames {
            for p in frame {
                assert!(p.iter().all(|x| x.is_finite() && x.abs() < 0.5));
            }
        }
        let difference: f32 = m.frames[48]
            .iter()
            .zip(&m.frames[52])
            .map(|(a, b)| (a[0] - b[0]).abs() + (a[1] - b[1]).abs())
            .sum();
        assert!(difference > 1., "flight must deform the mesh");
        let base: Vec<[f32; 6]> = serde_json::from_value(spec()["vertices"].clone()).unwrap();
        let mut a = base.clone();
        let mut b = base;
        animate(&mut a, &pose(0., 1., [0., 1., 0.], [0., 0., 1.])).unwrap();
        animate(&mut b, &pose(0.16, 1., [0., 1., 0.], [0., 0., 1.])).unwrap();
        assert_eq!(a, b, "flight loop must close");
        animate(&mut b, &pose(0.04, 0.5, [1., 0., 0.], [0., 0., 1.])).unwrap();
        assert!(b.iter().flatten().all(|v| v.is_finite()));
        assert!(animate(&mut b, &pose(0., 1., [0., 1., 0.], [0., 1., 0.])).is_err());
    }
}
