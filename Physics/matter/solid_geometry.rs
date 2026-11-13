//! Material-space convex cells. Their shared faces are real interfaces: cutting
//! an interface exposes the existing interior instead of spawning a particle.
use super::*;
use crate::physics::cut_geometry::{Face, Plane, Poly, Vertex};

pub(super) const INTERIOR: u64 = 10_000;
pub(super) struct Specimen {
    pub cells: Vec<Poly>,
    pub kind: SolidKind,
    pub grain: V,
    pub bark: V,
    pub source: &'static str,
    pub volume: f64,
}

fn clip(poly: Poly, plane: Plane, tag: u64) -> Result<Option<Poly>, String> {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for v in &poly.vertices {
        let d = plane.distance(v.position);
        lo = lo.min(d);
        hi = hi.max(d);
    }
    if hi <= 1e-9 {
        return Ok(Some(poly));
    }
    if lo >= -1e-9 {
        return Ok(None);
    }
    poly.clip_negative(plane, tag)
        .map(Some)
        .ok_or_else(|| format!("solid material half-space failed closure/volume validation (tag {tag}, range {lo}..{hi})"))
}

pub(super) fn partition(
    shape: &Poly,
    seeds: &[[f64; 3]],
    metric: [f64; 3],
    offset: u64,
) -> Result<Vec<Poly>, String> {
    let mut cells = Vec::new();
    for (i, a) in seeds.iter().enumerate() {
        let mut cell = Some(shape.clone());
        // Weighted Voronoi distance stretches wood cells along its fibres.
        // Both sides use the same material plane and persistent face identity.
        let mut neighbours: Vec<_> = seeds.iter().enumerate().filter(|(j, _)| *j != i).collect();
        neighbours.sort_by(|(_, b), (_, c)| {
            let d = |p: &[f64; 3]| {
                (0..3)
                    .map(|k| metric[k] * (p[k] - a[k]).powi(2))
                    .sum::<f64>()
            };
            d(b).total_cmp(&d(c))
        });
        for (j, b) in neighbours {
            let normal = std::array::from_fn(|k| metric[k] * (b[k] - a[k]));
            let midpoint = std::array::from_fn(|k| (a[k] + b[k]) * 0.5);
            let tag = INTERIOR + offset + (i.min(j) * seeds.len() + i.max(j)) as u64;
            cell = match cell {
                Some(p) => clip(p, Plane::through(midpoint, normal), tag)?,
                None => break,
            };
        }
        if let Some(p) = cell {
            if !p.valid() {
                return Err("invalid closed solid cell".into());
            }
            cells.push(p);
        }
    }
    let actual: f64 = cells.iter().map(|p| p.measure(true).volume).sum();
    let expected = shape.measure(true).volume;
    if (actual - expected).abs() > expected * 1e-6 {
        return Err("solid partition failed volume conservation".into());
    }
    Ok(cells)
}

fn inside(shape: &Poly, p: [f64; 3]) -> bool {
    use crate::physics::cut_geometry as g;
    shape.faces.iter().all(|f| {
        let a = shape.vertices[f.nodes[0]].material;
        g::dot(shape.face_normal(f, true), g::sub(p, a)) < -1e-5
    })
}

pub(super) fn seeds(shape: &Poly, count: [usize; 3], salt: i32, fibres: bool) -> Vec<[f64; 3]> {
    let lo: [f64; 3] = std::array::from_fn(|k| {
        shape
            .vertices
            .iter()
            .map(|v| v.material[k])
            .fold(f64::INFINITY, f64::min)
    });
    let hi: [f64; 3] = std::array::from_fn(|k| {
        shape
            .vertices
            .iter()
            .map(|v| v.material[k])
            .fold(f64::NEG_INFINITY, f64::max)
    });
    let mut result = Vec::new();
    for y in 0..count[1] {
        for z in 0..count[2] {
            for x in 0..count[0] {
                let id = salt + (x + y * 113 + z * 37) as i32;
                let grid = [x, y, z];
                let p = std::array::from_fn(|k| {
                    if !fibres {
                        return lo[k]
                            + (hi[k] - lo[k])
                                * (grid[k] as f64
                                    + 0.5
                                    + 0.23 * noise(id + k as i32 * 971) as f64)
                                / count[k] as f64;
                    }
                    // Fibre bundles retain the same cross-section at each
                    // axial station. Independent 3D jitter made wood's weak
                    // interfaces interlock like an isotropic rock tessellation.
                    let key = if fibres {
                        salt + if k == 1 {
                            y as i32 * 113
                        } else {
                            (x + z * 37) as i32
                        }
                    } else {
                        id
                    };
                    let span = if fibres && k != 1 { 0.77 } else { 1. };
                    let middle = (lo[k] + hi[k]) * 0.5;
                    middle
                        + (hi[k] - lo[k])
                            * span
                            * ((grid[k] as f64 + 0.5 + 0.23 * noise(key + k as i32 * 971) as f64)
                                / count[k] as f64
                                - 0.5)
                });
                if inside(shape, p) {
                    result.push(p);
                }
            }
        }
    }
    result
}

fn rock(center: [f64; 3], half: [f64; 3], salt: i32) -> Result<Poly, String> {
    let mut poly = Poly::box_from(
        std::array::from_fn(|i| center[i] - half[i]),
        std::array::from_fn(|i| center[i] + half[i]),
    );
    // Clip off irregular corners; the broad faces stay planar as stone faces.
    for i in 0..8 {
        let normal = std::array::from_fn(|k| if i & (1 << k) == 0 { -1. } else { 1. });
        let reach = (half.iter().sum::<f64>()) * (0.72 + 0.08 * noise(salt + i) as f64);
        let n = crate::physics::cut_geometry::unit(normal);
        let p = std::array::from_fn(|k| center[k] + n[k] * reach / 3f64.sqrt());
        poly = clip(poly, Plane::through(p, n), 20 + i as u64)?
            .ok_or("stone clipping removed specimen")?;
    }
    Ok(poly)
}

fn tree_section() -> Result<(Poly, V, V), String> {
    use crate::vegetation::{self, geometry, profiles};
    let site = vegetation::Site {
        id: 9172,
        normal: [0., 1., 0.],
        origin: [0.; 3],
        profile: 0,
        moisture: 0.6,
        soil_depth: 0.8,
    };
    let mut state = vegetation::initial_state(site);
    state.age_years = 20.;
    let profile = profiles::PROFILES[site.profile];
    let mut tree = vegetation::Tree::generate(site, &state, profile);
    let branch = tree.branches[0].clone();
    let axis =
        crate::vegetation::math::unit(crate::vegetation::math::sub(branch.end, branch.start));
    let cut_length = 0.62;
    tree.branches.truncate(1);
    tree.branches[0].start = [0., 0.004, 0.];
    tree.branches[0].end =
        std::array::from_fn(|k| tree.branches[0].start[k] + axis[k] * cut_length);
    // Use the game's actual bark geometry builder, with wind disabled for the
    // detached sample. This is a cut trunk section, not an entire scaled tree.
    tree.frame = Some([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]);
    let mesh = geometry::build_seasonal(&tree, profile, geometry::Detail::Near, 0., 0.);
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut faces = Vec::new();
    for tri in mesh.indices.chunks_exact(3) {
        let mut nodes = Vec::new();
        for &index in tri {
            let p = mesh.vertices[index as usize].position;
            let id = vertices
                .iter()
                .position(|v| (0..3).all(|k| (v.material[k] - p[k]).abs() < 1e-8))
                .unwrap_or_else(|| {
                    vertices.push(Vertex {
                        position: p,
                        material: p,
                        velocity: [0.; 3],
                    });
                    vertices.len() - 1
                });
            nodes.push(id);
        }
        faces.push(Face { nodes, tag: 1 }); // bark
    }
    for (end, tag) in [(false, 2), (true, 3)] {
        let pivot = if end {
            tree.branches[0].end
        } else {
            tree.branches[0].start
        };
        let mut nodes: Vec<usize> = vertices
            .iter()
            .enumerate()
            .filter(|(_, v)| {
                crate::physics::cut_geometry::dot(
                    crate::physics::cut_geometry::sub(v.material, pivot),
                    axis,
                )
                .abs()
                    < 1e-6
            })
            .map(|(i, _)| i)
            .collect();
        let (u, v) = crate::vegetation::math::basis(axis);
        nodes.sort_by(|&a, &b| {
            let angle = |i: usize| {
                let d = crate::vegetation::math::sub(vertices[i].material, pivot);
                crate::vegetation::math::dot(d, v).atan2(crate::vegetation::math::dot(d, u))
            };
            angle(a).total_cmp(&angle(b))
        });
        if !end {
            nodes.reverse();
        }
        faces.push(Face { nodes, tag });
    }
    // Orient every face away from the material centroid (game bark triangles
    // are two-sided; the volume integrator needs a consistently wound shell).
    let center: [f64; 3] = std::array::from_fn(|k| {
        vertices.iter().map(|v| v.material[k]).sum::<f64>() / vertices.len() as f64
    });
    for face in &mut faces {
        use crate::physics::cut_geometry as g;
        let [a, b, c] = [0, 1, 2].map(|k| vertices[face.nodes[k]].material);
        if g::dot(g::cross(g::sub(b, a), g::sub(c, a)), g::sub(a, center)) < 0. {
            face.nodes.reverse();
        }
    }
    let poly = Poly { vertices, faces };
    if !poly.valid() {
        return Err("tree trunk section is not closed".into());
    }
    Ok((poly, axis.map(|x| x as f32), profile.bark_color))
}

pub(super) fn specimen(case: &str) -> Result<Specimen, String> {
    if case == "matter_layers" {
        let shape = Poly::box_from([-0.32, 0.004, -0.24], [0.32, 0.15, 0.24]);
        let points = seeds(&shape, [5, 2, 4], 421, false);
        let cells = partition(&shape, &points, [1.; 3], 900_000)?;
        return Ok(Specimen {
            cells,
            kind: SolidKind::Stone,
            grain: [0., 1., 0.],
            bark: [0.; 3],
            source: "continuous bedrock slab beneath soil and turf",
            volume: shape.measure(true).volume,
        });
    }
    if matches!(case, "matter_wood" | "matter_wood_chop") {
        let (shape, grain, bark) = tree_section()?;
        let points = seeds(&shape, [5, 3, 5], 111, true);
        let cells = partition(&shape, &points, [1., 0.10, 1.], 0)?;
        return Ok(Specimen {
            cells,
            kind: SolidKind::Wood,
            grain,
            bark,
            source: "Vegetation::Tree broadleaf trunk section + Geometry::build_seasonal",
            volume: shape.measure(true).volume,
        });
    }
    let mut cells = Vec::new();
    let mut volume = 0.;
    let rubble = case == "matter_rubble";
    let count = if rubble { 7 } else { 1 };
    for i in 0..count {
        let center = if rubble {
            [
                -0.22 + (i % 3) as f64 * 0.20,
                0.076 + (i / 3) as f64 * 0.032,
                -0.16 + (i / 3) as f64 * 0.17,
            ]
        } else {
            [0., 0.181, 0.]
        };
        let half = if rubble {
            [0.079, 0.068, 0.074]
        } else {
            [0.23, 0.175, 0.21]
        };
        let shape = rock(center, half, 140 + i * 133)?;
        volume += shape.measure(true).volume;
        let points = seeds(
            &shape,
            if rubble { [2, 2, 2] } else { [4, 4, 4] },
            i * 337,
            false,
        );
        cells.extend(partition(
            &shape,
            &points,
            [1.; 3],
            (i as u64 + 1) * 100_000,
        )?);
    }
    Ok(Specimen {
        cells,
        kind: SolidKind::Stone,
        grain: [0., 1., 0.],
        bark: [0.; 3],
        source: "closed angular rock volumes with persistent material interfaces",
        volume,
    })
}
