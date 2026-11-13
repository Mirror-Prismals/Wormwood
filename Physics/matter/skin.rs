//! A sparse implicit skin reconstructed from accepted material samples. The
//! sampling lattice is a meshing aid only; matter is not stored in grid cells.
use super::*;
use std::collections::{HashMap, HashSet};
const CUBE: [[i32; 3]; 8] = [
    [0, 0, 0],
    [1, 0, 0],
    [0, 0, 1],
    [1, 0, 1],
    [0, 1, 0],
    [1, 1, 0],
    [0, 1, 1],
    [1, 1, 1],
];
const TETS: [[usize; 4]; 6] = [
    [0, 1, 3, 7],
    [0, 3, 2, 7],
    [0, 2, 6, 7],
    [0, 6, 4, 7],
    [0, 4, 5, 7],
    [0, 5, 1, 7],
];
const EDGES: [(usize, usize); 6] = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
const BUDGET: usize = 16500;
pub fn reconstruct(w: &mut World) -> Vec<[f32; 6]> {
    reconstruct_with_budget(w, BUDGET)
}
pub(super) fn reconstruct_with_budget(w: &mut World, budget: usize) -> Vec<[f32; 6]> {
    if w.case == "matter_pebbles" {
        return grains(w);
    }
    // Reuse the last successful spacing instead of rebuilding several
    // over-budget surfaces every fourth frame. Recover detail when sparse.
    let floor = 0.022;
    let mut spacing = w.skin_spacing;
    if w.frames % 60 == 0 && w.skin.len() < budget / 4 {
        spacing = (spacing / 1.24).max(floor);
    }
    // Increase spacing for the whole reconstruction if needed, never discard
    // random triangles or omit distant detached material to fit the renderer.
    loop {
        if let Some(v) = build(w, spacing, budget) {
            w.skin_spacing = spacing;
            return v;
        }
        spacing *= 1.24;
    }
}
fn grains(w: &World) -> Vec<[f32; 6]> {
    let mut out = Vec::new();
    for (i, p) in w.particles.iter().enumerate().filter(|(_, p)| p.active) {
        // Discrete closed grain surfaces never merge into a liquid isosurface.
        let vertices = AXES.map(|a| add(p.p, scale(a, p.radius)));
        let color = p
            .kind
            .law()
            .color
            .map(|v| v * (0.90 + 0.10 * noise(i as i32 * 71)));
        for face in OCT {
            triangle(&mut out, face.map(|j| vertices[j as usize]), color);
        }
    }
    out
}
fn build(w: &World, h: f32, budget: usize) -> Option<Vec<[f32; 6]>> {
    let active: Vec<_> = w
        .particles
        .iter()
        .enumerate()
        .filter(|(_, p)| p.active)
        .collect();
    let mut components: Vec<_> = (0..w.particles.len()).collect();
    fn root(parent: &[usize], mut i: usize) -> usize {
        while parent[i] != i {
            i = parent[i];
        }
        i
    }
    for b in w.bonds.iter().filter(|b| b.active) {
        let a = root(&components, b.a);
        let c = root(&components, b.b);
        components[c] = a;
    }
    for i in 0..components.len() {
        components[i] = root(&components, i);
    }
    let bin = 0.12;
    let key = |p: V| p.map(|v| (v / bin).floor() as i32);
    let mut bins = HashMap::<[i32; 3], Vec<usize>>::new();
    let mut cells = HashSet::<[i32; 3]>::new();
    for &(i, p) in &active {
        bins.entry(key(p.p)).or_default().push(i);
        let lo = p.p.map(|v| ((v - p.radius * 1.65) / h).floor() as i32);
        let hi = p.p.map(|v| ((v + p.radius * 1.65) / h).ceil() as i32);
        for z in lo[2]..hi[2] {
            for y in lo[1]..hi[1] {
                for x in lo[0]..hi[0] {
                    cells.insert([x, y, z]);
                }
            }
        }
    }
    let sample = |p: V| -> (f32, V, V) {
        let k = key(p);
        let mut groups = Vec::<(usize, (f32, V, V))>::new();
        for z in -1..=1 {
            for y in -1..=1 {
                for x in -1..=1 {
                    if let Some(ids) = bins.get(&[k[0] + x, k[1] + y, k[2] + z]) {
                        for &i in ids {
                            let a = &w.particles[i];
                            let d = length(sub(p, a.p)) / (a.radius * 1.6);
                            if d >= 1. {
                                continue;
                            }
                            let weight = (1. - d * d).powi(3);
                            let group = if a.kind != Kind::Dirt {
                                usize::MAX
                            } else {
                                components[i]
                            };
                            let index =
                                if let Some(i) = groups.iter().position(|(id, _)| *id == group) {
                                    i
                                } else {
                                    groups.push((group, (0., [0.; 3], [0.; 3])));
                                    groups.len() - 1
                                };
                            let e = &mut groups[index].1;
                            e.0 += weight;
                            e.2 = add(
                                e.2,
                                scale(
                                    sub(p, a.p),
                                    -6. * (1. - d * d).powi(2) / (a.radius * 1.6).powi(2),
                                ),
                            );
                            let mut color = a.kind.law().color;
                            if a.kind == Kind::Dirt {
                                let wet = 1. - a.moisture * 0.15;
                                color = color.map(|v| v * wet);
                            }
                            e.1 = add(e.1, scale(color, weight));
                        }
                    }
                }
            }
        }
        groups
            .iter()
            .max_by(|(ai, a), (bi, b)| a.0.total_cmp(&b.0).then(ai.cmp(bi)))
            .map(|(_, (weight, color, gradient))| {
                (
                    *weight - 0.20,
                    scale(*color, 1. / weight.max(1e-8)),
                    scale(*gradient, -1.),
                )
            })
            .unwrap_or((-0.2, [0.2; 3], [0.; 3]))
    };
    let mut cache = HashMap::<[i32; 3], (f32, V, V)>::new();
    let mut out = Vec::new();
    let mut cells: Vec<_> = cells.into_iter().collect();
    cells.sort_unstable(); // stable triangle ordering despite hashed lookup
    for c in cells {
        let keys = CUBE.map(|v| std::array::from_fn(|a| c[a] + v[a]));
        let p = keys.map(|k| k.map(|v| v as f32 * h));
        let f = keys.map(|k| {
            *cache
                .entry(k)
                .or_insert_with(|| sample(k.map(|v| v as f32 * h)))
        });
        if f.iter().all(|v| v.0 >= 0.) || f.iter().all(|v| v.0 < 0.) {
            continue;
        }
        for tet in TETS {
            let mut polygon = Vec::<(V, V, V)>::new();
            let mut inside = [0.; 3];
            let mut count = 0.;
            for &i in &tet {
                if f[i].0 >= 0. {
                    inside = add(inside, p[i]);
                    count += 1.;
                }
            }
            if count == 0. || count == 4. {
                continue;
            }
            inside = scale(inside, 1. / count);
            for (a, b) in EDGES {
                let a = tet[a];
                let b = tet[b];
                if (f[a].0 >= 0.) == (f[b].0 >= 0.) {
                    continue;
                }
                let t = (f[a].0 / (f[a].0 - f[b].0)).clamp(0., 1.);
                polygon.push((
                    add(p[a], scale(sub(p[b], p[a]), t)),
                    add(scale(f[a].1, 1. - t), scale(f[b].1, t)),
                    unit(add(scale(f[a].2, 1. - t), scale(f[b].2, t))),
                ));
            }
            if polygon.len() < 3 {
                continue;
            }
            let center = scale(
                polygon.iter().fold([0.; 3], |v, (p, _, _)| add(v, *p)),
                1. / polygon.len() as f32,
            );
            let normal = unit(sub(center, inside));
            let u = unit(sub(polygon[0].0, center));
            let v = cross(normal, u);
            polygon.sort_by(|a, b| {
                let a = sub(a.0, center);
                let b = sub(b.0, center);
                dot(a, v)
                    .atan2(dot(a, u))
                    .total_cmp(&dot(b, v).atan2(dot(b, u)))
            });
            for i in 1..polygon.len() - 1 {
                for (p, color, normal) in [polygon[0], polygon[i], polygon[i + 1]] {
                    let light = 0.40 + 0.60 * dot(normal, unit([-0.4, 0.8, 0.55])).max(0.);
                    let grain = 0.96 + 0.04 * (p[0] * 411. + p[1] * 173. + p[2] * 293.).sin();
                    out.push([
                        p[0],
                        p[1],
                        p[2],
                        color[0] * light * grain,
                        color[1] * light * grain,
                        color[2] * light * grain,
                    ]);
                }
                if out.len() > budget {
                    return None;
                }
            }
        }
    }
    Some(out)
}
pub fn triangle(out: &mut Vec<[f32; 6]>, p: [V; 3], color: V) {
    let normal = unit(cross(sub(p[1], p[0]), sub(p[2], p[0])));
    if length(normal) < 0.5 {
        return;
    }
    let light = 0.46 + 0.54 * dot(normal, unit([-0.4, 0.8, 0.55])).abs();
    for p in p {
        let texture = 0.96 + 0.04 * (p[0] * 411. + p[1] * 173. + p[2] * 293.).sin();
        out.push([
            p[0],
            p[1],
            p[2],
            color[0] * light * texture,
            color[1] * light * texture,
            color[2] * light * texture,
        ]);
    }
}
pub fn rod(out: &mut Vec<[f32; 6]>, a: V, b: V, r: f32, color: V) {
    let axis = unit(sub(b, a));
    let ref_axis = if axis[0].abs() < 0.8 {
        [1., 0., 0.]
    } else {
        [0., 0., 1.]
    };
    let u = scale(unit(cross(axis, ref_axis)), r);
    let v = scale(unit(cross(axis, u)), r);
    for k in 0..6 {
        let offset = |k: usize| {
            let t = k as f32 * std::f32::consts::TAU / 6.;
            add(scale(u, t.cos()), scale(v, t.sin()))
        };
        let x = offset(k);
        let y = offset(k + 1);
        triangle(out, [add(a, x), add(b, x), add(b, y)], color);
        triangle(out, [add(a, x), add(b, y), add(a, y)], color);
    }
}
pub fn table(out: &mut Vec<[f32; 6]>) {
    triangle(
        out,
        [[-1., -0.002, -1.], [1., -0.002, 1.], [1., -0.002, -1.]],
        [0.15, 0.19, 0.21],
    );
    triangle(
        out,
        [[-1., -0.002, -1.], [-1., -0.002, 1.], [1., -0.002, 1.]],
        [0.15, 0.19, 0.21],
    );
}
