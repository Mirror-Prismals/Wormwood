//! Procedural fixtures of the published scenario families; default mesh scale is reduced.
use super::math::*;
use super::{Pin, Scene};
pub(crate) const CASES: &[&str] = &[
    "cloth_stack",
    "tight_knot",
    "cloth_twist",
    "cloth_on_body",
    "robot_tshirt",
    "yarn_stretch",
    "yarn_twist",
    "three_cloth_layers_sphere",
    "cloth_sphere",
    "volumetric_mat",
    "rooted_grass",
    "soil_spade_contact",
    "soil_spade_fracture",
];
fn cloth(scene: &mut Scene, n: usize, center: V, extent: f32, color: V) -> (usize, usize) {
    let start = scene.p.len();
    let step = 2. * extent / n as f32;
    for j in 0..=n {
        for i in 0..=n {
            scene.node(
                add(
                    center,
                    [-extent + i as f32 * step, 0., -extent + j as f32 * step],
                ),
                0.04,
                color,
            );
        }
    }
    let mut faces = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let a = (j * (n + 1) + i) as u32;
            faces.extend([
                [a, a + (n + 1) as u32, a + 1],
                [a + 1, a + (n + 1) as u32, a + (n + 2) as u32],
            ]);
        }
    }
    scene.surface(start, faces, false, true);
    for j in 0..=n {
        for i in 0..=n {
            let a = start + j * (n + 1) + i;
            if i < n {
                scene.edge(a, a + 1, 1e-7);
            }
            if j < n {
                scene.edge(a, a + n + 1, 1e-7);
            }
            if i < n && j < n {
                scene.edge(a, a + n + 2, 2e-7);
                scene.edge(a + 1, a + n + 1, 2e-7);
            }
            if i + 2 <= n {
                scene.edge(a, a + 2, 2e-4);
            }
            if j + 2 <= n {
                scene.edge(a, a + 2 * (n + 1), 2e-4);
            }
        }
    }
    (start, scene.p.len() - start)
}
fn floor(scene: &mut Scene) {
    let start = scene.p.len();
    for p in [[-5., 0., -5.], [5., 0., -5.], [5., 0., 5.], [-5., 0., 5.]] {
        scene.node(p, 0., [0.25, 0.3, 0.36]);
    }
    scene.surface(start, vec![[0, 2, 1], [0, 3, 2]], true, false);
}
fn cylinder(scene: &mut Scene, center: V, radius: f32, height: f32) {
    let start = scene.p.len();
    let n = 16;
    for y in [-height * 0.5, height * 0.5] {
        for i in 0..n {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            scene.node(
                add(center, [radius * a.cos(), y, radius * a.sin()]),
                0.,
                [0.35, 0.4, 0.48],
            );
        }
    }
    let mut faces = Vec::new();
    for i in 0..n {
        let j = (i + 1) % n;
        faces.extend([
            [i as u32, j as u32, (i + n) as u32],
            [j as u32, (j + n) as u32, (i + n) as u32],
        ]);
        if i > 0 && i < n - 1 {
            faces.extend([
                [0, (i + 1) as u32, i as u32],
                [n as u32, (i + n) as u32, (i + n + 1) as u32],
            ]);
        }
    }
    scene.surface(start, faces, true, false);
}
fn sphere(scene: &mut Scene, center: V, radius: f32) {
    let start = scene.p.len();
    let (rings, segments) = (8, 12);
    for j in 0..=rings {
        let phi = std::f32::consts::PI * j as f32 / rings as f32;
        for i in 0..segments {
            let t = std::f32::consts::TAU * i as f32 / segments as f32;
            scene.node(
                add(
                    center,
                    [
                        radius * phi.sin() * t.cos(),
                        radius * phi.cos(),
                        radius * phi.sin() * t.sin(),
                    ],
                ),
                0.,
                [0.35, 0.42, 0.55],
            );
        }
    }
    let mut faces = Vec::new();
    for j in 0..rings {
        for i in 0..segments {
            let a = (j * segments + i) as u32;
            let b = (j * segments + (i + 1) % segments) as u32;
            let c = a + segments as u32;
            let d = b + segments as u32;
            if j > 0 {
                faces.push([a, b, c]);
            }
            if j < rings - 1 {
                faces.push([b, d, c]);
            }
        }
    }
    scene.surface(start, faces, true, false);
}
fn strip(scene: &mut Scene, points: &[V], width: f32) {
    let start = scene.p.len();
    for p in points {
        for offset in [-width, width] {
            scene.node(add(*p, [0., offset, 0.]), 0.02, [0.86, 0.62, 0.24]);
        }
    }
    let mut faces = Vec::new();
    for i in 0..points.len() - 1 {
        let a = (i * 2) as u32;
        faces.extend([[a, a + 2, a + 1], [a + 1, a + 2, a + 3]]);
        for k in 0..2 {
            scene.edge(start + i * 2 + k, start + (i + 1) * 2 + k, 1e-7);
            if i + 2 < points.len() {
                scene.edge(start + i * 2 + k, start + (i + 2) * 2 + k, 1e-5);
            }
        }
        scene.edge(start + i * 2, start + i * 2 + 1, 1e-7);
        scene.edge(start + i * 2, start + (i + 1) * 2 + 1, 1e-6);
    }
    scene.surface(start, faces, false, true);
}
fn yarn(scene: &mut Scene, points: &[V], color: V) -> (usize, usize) {
    let start = scene.p.len();
    let n = points.len();
    for i in 0..n {
        let tangent = unit(sub(points[(i + 1).min(n - 1)], points[i.saturating_sub(1)]));
        let right = unit(cross(tangent, [0., 1., 0.]));
        let up = unit(cross(right, tangent));
        for k in 0..3 {
            let t = k as f32 / 3. * std::f32::consts::TAU;
            scene.node(
                add(
                    points[i],
                    add(scale(right, t.cos() * 0.012), scale(up, t.sin() * 0.012)),
                ),
                0.004,
                color,
            );
        }
    }
    let mut faces = Vec::new();
    for i in 0..n {
        for k in 0..3 {
            scene.edge(start + i * 3 + k, start + i * 3 + (k + 1) % 3, 1e-8);
            if i + 1 < n {
                let a = (i * 3 + k) as u32;
                let b = (i * 3 + (k + 1) % 3) as u32;
                faces.extend([[a, b, a + 3], [b, b + 3, a + 3]]);
                scene.edge(start + i * 3 + k, start + (i + 1) * 3 + k, 1e-8);
                scene.edge(start + i * 3 + k, start + (i + 1) * 3 + (k + 1) % 3, 1e-7);
            }
            if i + 2 < n {
                scene.edge(start + i * 3 + k, start + (i + 2) * 3 + k, 1e-5);
            }
        }
    }
    scene.surface(start, faces, false, true);
    (start, n * 3)
}
pub(crate) fn create(name: &str, scale: usize) -> Result<Scene, String> {
    if !CASES.contains(&name) {
        return Err("unknown author scenario".into());
    }
    let mut s = Scene::new(name);
    s.radius = if name.starts_with("yarn") {
        0.004
    } else {
        0.008
    };
    if name != "soil_spade_fracture" { floor(&mut s); }
    match name {
        "rooted_grass" => super::sod::fixture(&mut s),
        "soil_spade_contact" => super::soil_contact_baseline::fixture(&mut s),
        "soil_spade_fracture" => super::soil_workshop::fixture(&mut s),
        "cloth_stack" => {
            cylinder(&mut s, [0., 0.8, 0.], 0.65, 1.4);
            for layer in 0..50 {
                cloth(
                    &mut s,
                    3 * scale,
                    [0., 1.8 + layer as f32 * 0.04, 0.],
                    1.,
                    [0.3 + (layer % 5) as f32 * 0.1, 0.6, 0.8],
                );
            }
        }
        "cloth_sphere" | "three_cloth_layers_sphere" => {
            sphere(&mut s, [0., 1., 0.], 0.75);
            let layers = if name == "cloth_sphere" { 1 } else { 3 };
            for i in 0..layers {
                cloth(
                    &mut s,
                    8 * scale,
                    [0., 2.1 + i as f32 * 0.06, 0.],
                    1.1,
                    [0.85, 0.45 + i as f32 * 0.1, 0.25],
                );
            }
        }
        "cloth_twist" => {
            s.gravity = [0.; 3];
            let n = 8 * scale;
            let (start, _) = cloth(&mut s, n, [0., 1.5, 0.], 1., [0.22, 0.6, 0.9]);
            for j in 0..=n {
                for i in [0, n] {
                    let node = start + j * (n + 1) + i;
                    s.pin(
                        node,
                        Pin::Twist {
                            center: [s.p[node][0], 1.5, 0.],
                            sign: if i == 0 { -1. } else { 1. },
                            turns: 4.,
                        },
                    );
                }
            }
        }
        "tight_knot" => {
            s.gravity = [0.; 3];
            let n = 80 * scale;
            let mut points = Vec::new();
            for i in 0..=n {
                let t = 0.12 + (std::f32::consts::TAU - 0.24) * i as f32 / n as f32;
                let r = 0.7 + 0.25 * (3. * t).cos();
                points.push([
                    r * (2. * t).cos(),
                    1.5 + 0.25 * (3. * t).sin(),
                    r * (2. * t).sin(),
                ]);
            }
            let start = s.p.len();
            strip(&mut s, &points, 0.018);
            for (node, sign) in [
                (start, -1.),
                (start + 1, -1.),
                (s.p.len() - 2, 1.),
                (s.p.len() - 1, 1.),
            ] {
                s.pin(
                    node,
                    Pin::Pull {
                        axis: 2,
                        sign,
                        speed: 0.12,
                    },
                );
            }
        }
        "yarn_stretch" | "yarn_twist" => {
            s.gravity = [0., -1., 0.];
            let strands = 4 * scale;
            let n = 16 * scale;
            for axis in 0..2 {
                for row in 0..strands {
                    let fixed = -0.45 + row as f32 * 0.3 / scale as f32;
                    let points: Vec<_> = (0..=n)
                        .map(|i| {
                            let x = -0.6 + i as f32 * 1.2 / n as f32;
                            let weave = (std::f32::consts::PI
                                * ((x + 0.45) / (0.3 / scale as f32) + row as f32))
                                .cos()
                                * 0.045
                                * if axis == 0 { 1. } else { -1. };
                            if axis == 0 {
                                [x, 1.5 + weave, fixed]
                            } else {
                                [fixed, 1.5 + weave, x]
                            }
                        })
                        .collect();
                    let (start, count) = yarn(&mut s, &points, [0.4 + axis as f32 * 0.4, 0.7, 0.3]);
                    if axis == 0 {
                        for (base, sign) in [(start, -1.), (start + count - 3, 1.)] {
                            for k in 0..3 {
                                let node = base + k;
                                let pin = if name == "yarn_twist" {
                                    Pin::Twist {
                                        center: [s.p[node][0], 1.5, 0.],
                                        sign,
                                        turns: 2.5,
                                    }
                                } else {
                                    Pin::Pull {
                                        axis: 0,
                                        sign,
                                        speed: 0.025,
                                    }
                                };
                                s.pin(node, pin);
                            }
                        }
                    }
                }
            }
        }
        "cloth_on_body" => {
            sphere(&mut s, [0., 1.1, 0.], 0.5);
            let n = 2 * scale;
            let mut panels = Vec::new();
            for panel in 0..14 {
                let a = panel as f32 / 14. * std::f32::consts::TAU;
                let (start, count) = cloth(&mut s, n, [0.; 3], 0.22, [0.8, 0.35, 0.55]);
                for i in start..start + count {
                    let original = s.p[i];
                    let angle = a + original[0] / 0.22 * (std::f32::consts::TAU / 28. * 0.98);
                    let position = [
                        angle.cos() * 0.7,
                        1.3 + original[2] * 2.2,
                        angle.sin() * 0.7,
                    ];
                    s.p[i] = position;
                    s.rest[i] = position;
                }
                // Rest lengths follow the constructed cylindrical garment.
                for e in &mut s.edges {
                    if e.a >= start && e.b >= start {
                        e.rest = length(sub(s.p[e.b], s.p[e.a]));
                    }
                }
                for i in 0..=n {
                    s.pin(start + n * (n + 1) + i, Pin::Body { phase: a });
                }
                panels.push(start);
            }
            for panel in 0..14 {
                for row in 0..=n {
                    s.edge(
                        panels[panel] + row * (n + 1) + n,
                        panels[(panel + 1) % 14] + row * (n + 1),
                        1e-9,
                    );
                }
            }
            s.proxy="Procedural 14-panel stitched cylindrical garment with pinned motion around a spherical body proxy; original animated body and stitch assets are not reproduced.".into();
        }
        "robot_tshirt" => {
            cylinder(&mut s, [0., 0.6, 0.], 0.7, 1.);
            let n = 8 * scale;
            let (start, _) = cloth(&mut s, n, [0., 1.3, 0.], 0.7, [0.85, 0.7, 0.25]);
            for node in [start, start + n] {
                s.pin(node, Pin::Robot);
            }
            s.proxy="Kinematic two-gripper manipulation of a cloth proxy; original robot/T-shirt assets are not reproduced.".into();
        }
        "volumetric_mat" => {
            s.gravity = [0.; 3];
            s.substeps = 10;
            s.iterations = 8;
            let (nx, nz) = (24 * scale, 6 * scale);
            let start = s.p.len();
            let layer = (nx + 1) * (nz + 1);
            let total = 2 * layer;
            for y in 0..2 {
                for z in 0..=nz {
                    for x in 0..=nx {
                        s.node(
                            [
                                -0.8 + 1.6 * x as f32 / nx as f32,
                                1.4 + 0.15 * y as f32,
                                -0.8 + 1.6 * z as f32 / nz as f32,
                            ],
                            38.4 / total as f32,
                            [0.25 + 0.2 * y as f32, 0.65, 0.85],
                        );
                    }
                }
            }
            let mut faces = Vec::new();
            for z in 0..nz {
                for x in 0..nx {
                    let a = start + z * (nx + 1) + x;
                    let b = a + 1;
                    let c = a + nx + 1;
                    let d = c + 1;
                    let u = layer;
                    // Conforming Freudenthal split, with strain on every tet edge.
                    for ids in [
                        [a, b, d, d + u],
                        [a, d, c, d + u],
                        [a, c, c + u, d + u],
                        [a, c + u, a + u, d + u],
                        [a, a + u, b + u, d + u],
                        [a, b + u, b, d + u],
                    ] {
                        s.tetra(ids);
                    }
                    let (a, b, c, d) = (
                        (a - start) as u32,
                        (b - start) as u32,
                        (c - start) as u32,
                        (d - start) as u32,
                    );
                    let u = u as u32;
                    faces.extend([
                        [a, b, c],
                        [b, d, c],
                        [a + u, c + u, b + u],
                        [b + u, c + u, d + u],
                    ]);
                }
            }
            let mut boundary = Vec::new();
            boundary.extend(0..nx);
            boundary.extend((0..nz).map(|z| z * (nx + 1) + nx));
            boundary.extend((1..=nx).rev().map(|x| nz * (nx + 1) + x));
            boundary.extend((1..=nz).rev().map(|z| z * (nx + 1)));
            for k in 0..boundary.len() {
                let a = boundary[k] as u32;
                let b = boundary[(k + 1) % boundary.len()] as u32;
                let u = layer as u32;
                faces.extend([[a, a + u, b], [b, a + u, b + u]]);
            }
            s.surface(start, faces, false, true);
            for y in 0..2 {
                for z in 0..=nz {
                    for x in [0, nx] {
                        let node = start + y * layer + z * (nx + 1) + x;
                        s.pin(
                            node,
                            Pin::Twist {
                                center: [s.p[node][0], 1.475, 0.],
                                sign: if x == 0 { -1. } else { 1. },
                                turns: 1.,
                            },
                        );
                    }
                }
            }
        }
        _ => unreachable!(),
    }
    s.finish()?;
    Ok(s)
}
