//! Procedural appearance geometry. This module is called by AppearanceSystem.View.
use serde_json::{Value, json};
type V = [f64; 3];
fn dot(a: V, b: V) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn unit(v: V) -> V {
    let l = dot(v, v).sqrt().max(1e-12);
    v.map(|x| x / l)
}
fn vector(view: &Value, key: &str) -> V {
    std::array::from_fn(|i| view[key][i].as_f64().unwrap())
}
pub(super) fn water(view: &Value) -> Vec<[f32; 24]> {
    let pos = vector(view, "position");
    let right = vector(view, "right");
    let up = vector(view, "up");
    let forward = vector(view, "forward");
    let mut triangles = Vec::new();
    // Finite rendering neighborhood of an unbounded homogeneous medium.
    for index in 0..12 {
        let seed = index as f64;
        let center: V = std::array::from_fn(|i| {
            let raw = if index == 0 {
                [0., 0., -4.][i]
            } else {
                ((seed * 17.173 + i as f64 * 7.113).sin() * 1413.713).rem_euclid(24.) - 12.
            };
            (raw - pos[i] + 12.).rem_euclid(24.) - 12.
        });
        let z = dot(center, forward);
        if z <= 0.05 {
            continue;
        }
        let fade = (1. - z / 24.).clamp(0.05, 1.);
        let mut t = [0f32; 24];
        for (j, (x, y)) in [(-0.025, -0.025), (0.025, -0.025), (0., 0.025)]
            .iter()
            .enumerate()
        {
            let v: V = std::array::from_fn(|i| center[i] + right[i] * x + up[i] * y);
            let d = dot(v, forward);
            let f = 1. / (70f64.to_radians() / 2.).tan();
            t[j * 4..j * 4 + 4].copy_from_slice(&[
                (dot(v, right) * f / (4. / 3.)) as f32,
                (dot(v, up) * f) as f32,
                (d - 0.05) as f32,
                d as f32,
            ]);
            t[12 + j * 4..16 + j * 4].copy_from_slice(&[
                (0.1 + 0.3 * fade) as f32,
                (0.35 + 0.35 * fade) as f32,
                (0.45 + 0.3 * fade) as f32,
                1.,
            ]);
        }
        triangles.push(t);
    }
    triangles
}
pub(super) fn planet(view: &Value) -> Vec<[f32; 24]> {
    let camera = vector(view, "position");
    let radius = 6_371_000.;
    let height = dot(camera, camera).sqrt();
    let up = unit(camera);
    let east = unit(cross(
        if up[1].abs() > 0.95 {
            [0., 0., -1.]
        } else {
            [0., 1., 0.]
        },
        up,
    ));
    let north = cross(up, east);
    let horizon = (radius / height.max(radius + 0.1)).acos();
    let extent = (horizon * 1.6 + 0.001).clamp(0.002, 1.45);
    let mut vertices = Vec::new();
    let sample = |x: usize, y: usize| {
        let a = (x as f64 / 8. * 2. - 1.) * extent;
        let b = (y as f64 / 8. * 2. - 1.) * extent;
        let theta = (a * a + b * b).sqrt();
        let tangent = if theta > 1e-12 {
            std::array::from_fn(|i| (east[i] * a + north[i] * b) / theta)
        } else {
            east
        };
        let normal: V = std::array::from_fn(|i| up[i] * theta.cos() + tangent[i] * theta.sin());
        let p = std::array::from_fn::<_, 3, _>(|i| (normal[i] * radius - camera[i]) as f32);
        (p, normal)
    };
    for y in 0..8 {
        for x in 0..8 {
            let corners = [
                sample(x, y),
                sample(x + 1, y),
                sample(x + 1, y + 1),
                sample(x, y + 1),
            ];
            let tint = if (x + y) % 2 == 0 { 1. } else { 0.87 };
            for k in [0, 1, 2, 0, 2, 3] {
                let (p, n) = corners[k];
                let light = (dot(n, unit([0.4, 0.8, 0.2])).max(0.) * 0.7 + 0.3) * tint;
                vertices.push([
                    p[0],
                    p[1],
                    p[2],
                    (0.27 * light) as f32,
                    (0.40 * light) as f32,
                    (0.22 * light) as f32,
                ]);
            }
        }
    }
    let mut local = view.clone();
    local["position"] = json!([0, 0, 0]);
    local["far"] = json!(height + radius * 2.);
    super::projection::project(&vertices, &local)
}
