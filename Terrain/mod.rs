//! Shared, deterministic radial landscape sampler for Mirror.
//!
//! This is an authored Ridge and Valley landform approximation, not a surveyed
//! Pennsylvania reconstruction, tectonic simulation, or volumetric soil model.
//! Both surface presentation and contact queries must use this same height.
#[path="CaveFormations.rs"]
pub(crate) mod cave_formations;
#[path="OrganicCavePrototype.rs"]
pub(crate) mod organic_cave_prototype;
#[path="Rock.rs"]
pub(crate) mod rock;
#[path="Depths.rs"]
pub(crate) mod depths;
#[path="SurfaceNet.rs"]
pub(crate) mod surface_net;
#[path="SurfaceCover.rs"]
pub(crate) mod surface_cover;
#[path="Underground.rs"]
pub(crate) mod underground;
#[path="Drainage.rs"]
pub(crate) mod drainage;
#[path="Regions.rs"]
pub(crate) mod regions;
pub(crate) const RADIUS: f64 = 6_371_000.;
pub(crate) type Point = [f64; 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Material {
    Loam,
    Sandstone,
    Shale,
    Limestone,
}

#[derive(Clone, Copy)]
pub(crate) struct Sample {
    pub height: f64,
    pub color: [f64; 3],
    pub material: Material,
    /// A visual/generation descriptor, not a calibrated mechanical parameter.
    pub soil_depth: f64,
    pub moisture: f64,
}

fn dot(a: Point, b: Point) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn unit(p: Point) -> Point {
    let l = dot(p, p).sqrt().max(1e-15);
    p.map(|v| v / l)
}
fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn gaussian(x: f64) -> f64 {
    (-x * x).exp()
}
fn coordinates(n: Point) -> (f64, f64) {
    (n[0] * RADIUS, n[2] * RADIUS)
}

// The local reference region is centred at the north-pole spawn. Chord-based
// coordinates extend continuously over the sphere; that extension does not
// claim that Pennsylvania geology describes the rest of the planet.
fn structure(x: f64, z: f64) -> (f64, f64, f64, f64) {
    let across = 0.78 * x + 0.6257795 * z;
    let along = -0.6257795 * x + 0.78 * z;
    let fold = across + 620. * (along / 19_000.).sin() + 190. * (along / 5_200.).sin();
    let phase = (fold / 5_600.).rem_euclid(1.);
    // Paired resistant beds with a lower saddle in the intervening strata.
    let primary = gaussian((phase - 0.39) / 0.064);
    let secondary = gaussian((phase - 0.56) / 0.052);
    let drainage = gaussian((along - 2_650. - 240. * (across / 2_300.).sin()) / 420.);
    (primary, secondary, drainage, along)
}

fn raw_height(x: f64, z: f64) -> f64 {
    let (a, b, gap, along) = structure(x, z);
    let regional = 78. * (x / 21_000.).sin() + 64. * (z / 15_000.).sin();
    let ridge = (a * 312. + b * 220.) * (1. - 0.91 * gap) * (0.92 + 0.08 * (along / 3_400.).cos());
    let foothills = 9. * (x / 620.).sin() * (z / 910.).sin()
        + 3.2 * (x / 111. + z / 177.).sin() * (z / 227.).sin();
    let outcrop = (a + b).min(1.)
        * (1. - gap)
        * (3.2 * (along / 79.).sin() + 1.2 * (x / 29. + z / 43.).sin());
    let micro = 0.09 * (x / 1.3).sin() * (z / 1.7).sin() + 0.035 * (x / 0.41 + z / 0.53).sin();
    regional + ridge + foothills + outcrop + micro
}

/// Metres above the reference sphere. The spawn normal has exactly zero height.
pub(crate) fn base_height(normal: Point) -> f64 {
    let (x, z) = coordinates(unit(normal));
    regions::height(normal, raw_height(x, z) - raw_height(0., 0.))
}

pub(crate) fn height(normal:Point)->f64{if nightworld::active(normal){nightworld::height(normal)}else{drainage::height(normal,base_height(normal))}}

pub(crate) fn sample(normal: Point) -> Sample {
    if nightworld::active(normal) { return Sample {height:height(normal),color:nightworld::color(normal.map(|v|v*RADIUS)).map(|v|v as f64),material:Material::Loam,soil_depth:8.,moisture:0.02}; }
    match regions::region(normal) {
        regions::Region::Snow => return Sample {height:height(normal), color:[0.88,0.94,0.98],material:Material::Loam,soil_depth:2.,moisture:0.4},
        regions::Region::Jungle => return Sample {height:height(normal),color:[0.08,0.30,0.10],material:Material::Loam,soil_depth:3.,moisture:0.9},
        _ => {}
    }
    if regions::polar(normal) { return Sample { height: height(normal), color: [0.88,0.94,0.98], material: Material::Loam, soil_depth: 1.5, moisture: 0.05 }; }
    if regions::sandy(normal) { return Sample { height: height(normal), color: [0.72,0.57,0.34], material: Material::Loam, soil_depth: 5., moisture: 0.03 }; }
    if regions::region(normal) == regions::Region::Plains {
        let h=height(normal);
        let bare=regions::coordinates(normal).0 < -5_000. && base_height(normal)>180.;
        return Sample { height:h, color:if bare {[0.48,0.47,0.44]}else{[0.23,0.34,0.13]}, material:if bare {Material::Sandstone}else{Material::Loam}, soil_depth:if bare {0.1}else{1.2}, moisture:if bare {0.2}else{0.52} };
    }
    let (x, z) = coordinates(unit(normal));
    let (a, b, gap, _) = structure(x, z);
    let crest = (a + b).min(1.) * (1. - gap);
    let weathering = 0.5 + 0.5 * (x / 74. + z / 103.).sin();
    let moisture =
        (0.56 - 0.28 * crest + 0.24 * gap + 0.08 * (x / 270. - z / 390.).sin()).clamp(0., 1.);
    let soil_depth = (1.2 - crest * 1.05 + 0.35 * gap).clamp(0.08, 1.6);
    let material = if crest > 0.72 && weathering > 0.35 {
        Material::Sandstone
    } else if crest > 0.40 && b > a {
        Material::Shale
    } else if gap > 0.8 {
        Material::Limestone
    } else {
        Material::Loam
    };
    let variation = 0.035 * (x / 11. + z / 17.).sin() + 0.018 * (x / 3.1 - z / 4.7).cos();
    let color = match material {
        Material::Sandstone => [0.39 + variation, 0.37 + variation, 0.30 + variation],
        Material::Shale => [0.31 + variation, 0.33 + variation, 0.29 + variation],
        Material::Limestone => [0.33 + variation, 0.37 + variation, 0.28 + variation],
        Material::Loam => [
            0.23 + variation,
            0.31 + variation + moisture * 0.04,
            0.13 + variation,
        ],
    };
    Sample {
        height: height(normal),
        color,
        material,
        soil_depth,
        moisture,
    }
}

/// A metre-scale surface normal, shared with tools that need a terrain gradient.
pub(crate) fn surface_normal(normal: Point) -> Point {
    let n = unit(normal);
    let east = unit(cross(
        if n[1].abs() > 0.95 {
            [0., 0., -1.]
        } else {
            [0., 1., 0.]
        },
        n,
    ));
    let north = cross(n, east);
    let offset = |axis: Point, distance: f64| {
        height(unit(std::array::from_fn(|i| {
            n[i] + axis[i] * distance / RADIUS
        })))
    };
    let dx = (offset(east, 1.) - offset(east, -1.)) * 0.5;
    let dz = (offset(north, 1.) - offset(north, -1.)) * 0.5;
    unit(std::array::from_fn(|i| n[i] - east[i] * dx - north[i] * dz))
}

#[derive(Clone, Copy)]
pub(crate) enum Growth {
    Grass,
    Tree,
    Clod,
}
#[derive(Clone, Copy)]
pub(crate) struct Plant {
    pub normal: Point,
    pub size: f64,
    pub phase: f64,
    pub variation: f64,
}
fn hash(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58476d1ce4e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
fn random(seed: u64, stream: u64) -> f64 {
    (hash(seed.wrapping_add(stream.wrapping_mul(0x9e3779b97f4a7c15))) >> 11) as f64
        * (1. / ((1u64 << 53) as f64))
}

/// Bounded, world-fixed cube-face sites. Adjacent faces are considered near a
/// face boundary, and a site's owning face never changes with the camera.
pub(crate) fn growth(camera_normal: Point, kind: Growth) -> Vec<Plant> {
    if nightworld::active(camera_normal){return Vec::new();}
    let camera = unit(camera_normal);
    let (spacing, reach, limit, salt) = match kind {
        Growth::Grass => (1.2, 22., 240, 17u64),
        Growth::Tree => (23., 220., 20, 71u64),
        Growth::Clod => (4.8, 30., 24, 131u64),
    };
    let maximum = camera.iter().map(|x| x.abs()).fold(0., f64::max);
    let mut candidates = Vec::new();
    for axis in 0..3 {
        if camera[axis].abs() < maximum - reach * 4. / RADIUS {
            continue;
        }
        let sign = camera[axis].signum();
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        let centre = [
            camera[u] / camera[axis].abs() * RADIUS,
            camera[v] / camera[axis].abs() * RADIUS,
        ];
        let radius = (reach * 3.3 / spacing).ceil() as i64 + 1;
        let cell = [
            (centre[0] / spacing).floor() as i64,
            (centre[1] / spacing).floor() as i64,
        ];
        for cx in cell[0] - radius..=cell[0] + radius {
            for cz in cell[1] - radius..=cell[1] + radius {
                let seed = hash(
                    (cx as u64).wrapping_mul(0x632be59bd9b4e019)
                        ^ (cz as u64).wrapping_mul(0x9e3779b97f4a7c15)
                        ^ ((axis as u64 + if sign > 0. { 7 } else { 11 }) << 48)
                        ^ salt,
                );
                if random(seed, 1)
                    > match kind {
                        Growth::Grass => 0.82,
                        Growth::Tree => 0.64,
                        Growth::Clod => 0.30,
                    }
                {
                    continue;
                }
                let mut raw = [0.; 3];
                raw[axis] = sign;
                raw[u] = (cx as f64 + 0.15 + 0.7 * random(seed, 2)) * spacing / RADIUS;
                raw[v] = (cz as f64 + 0.15 + 0.7 * random(seed, 3)) * spacing / RADIUS;
                if raw[u].abs() >= 1. || raw[v].abs() >= 1. {
                    continue;
                }
                let normal = unit(raw);
                let distance2 = (0..3)
                    .map(|i| ((normal[i] - camera[i]) * RADIUS).powi(2))
                    .sum::<f64>();
                if distance2 > reach * reach {
                    continue;
                }
                if matches!(kind, Growth::Tree) && !regions::forest(normal) { continue; }
                if matches!(kind, Growth::Grass) && (regions::sandy(normal) || regions::polar(normal) || regions::region(normal)==regions::Region::Snow) { continue; }
                let s = sample(normal);
                if s.material != Material::Loam && !matches!(kind, Growth::Clod) {
                    continue;
                }
                let size = match kind {
                    Growth::Grass => 0.16 + random(seed, 4) * 0.31,
                    Growth::Tree => 7. + random(seed, 4) * 10.,
                    Growth::Clod => 0.07 + random(seed, 4) * 0.13,
                };
                candidates.push((
                    distance2,
                    Plant {
                        normal,
                        size,
                        phase: random(seed, 5) * std::f64::consts::TAU,
                        variation: random(seed, 6),
                    },
                ));
            }
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    candidates.truncate(limit);
    candidates.into_iter().map(|(_, p)| p).collect()
}

/// Deterministic two-metre world-fixed centre of the presentation rings.
pub(crate) fn mesh_anchor(camera_eye: Point) -> Point {
    let n = unit(camera_eye);
    let axis = (0..3)
        .max_by(|a, b| n[*a].abs().total_cmp(&n[*b].abs()))
        .unwrap();
    let mut raw = [0.; 3];
    raw[axis] = n[axis].signum();
    for i in 0..3 {
        if i != axis {
            let cell = (n[i] / n[axis].abs() * RADIUS / 2.).round();
            raw[i] = if cell == 0. { 0. } else { cell * 2. / RADIUS };
        }
    }
    unit(raw)
}

/// Intersect the same near-field triangles displayed by EnvironmentGeometry.
/// The supplied height includes saved excavation offsets. Returns None beyond
/// the dense eight-metre patch, where callers retain the analytic surface query.
pub(crate) fn rendered_height(
    camera_eye: Point,
    query_normal: Point,
    mut sample_height: impl FnMut(Point) -> f64,
) -> Option<f64> {
    let eye_up = unit(camera_eye);
    if dot(camera_eye, camera_eye).sqrt() - RADIUS - sample_height(eye_up) >= 100. {
        return None;
    }
    let centre = mesh_anchor(camera_eye);
    let east = unit(cross(
        if centre[1].abs() > 0.95 {
            [0., 0., -1.]
        } else {
            [0., 1., 0.]
        },
        centre,
    ));
    let north = cross(centre, east);
    let query = unit(query_normal);
    let x = dot(query, east);
    let y = dot(query, north);
    let radial = (x * x + y * y).sqrt().atan2(dot(query, centre)) * RADIUS;
    if radial >= 8. {
        return None;
    }
    let tau = std::f64::consts::TAU;
    let segment = (y.atan2(x).rem_euclid(tau) / tau * 64.).floor() as usize;
    let lower = (radial / 0.5).floor() * 0.5;
    let upper = lower + 0.5;
    let site = |radius: f64, index: usize| {
        let a = index as f64 / 64. * tau;
        let theta = radius / RADIUS;
        std::array::from_fn(|i| {
            centre[i] * theta.cos() + (east[i] * a.cos() + north[i] * a.sin()) * theta.sin()
        })
    };
    let normals = [
        site(lower, segment),
        site(upper, segment),
        site(upper, (segment + 1) % 64),
        site(lower, (segment + 1) % 64),
    ];
    // Gnomonic coordinates select the triangle by ray direction, independent
    // of its three vertex elevations. The final intersection uses its 3D plane.
    let plane_point = |n: Point| {
        [
            dot(n, east) / dot(n, centre) * RADIUS,
            dot(n, north) / dot(n, centre) * RADIUS,
        ]
    };
    let q = plane_point(query);
    let inside_first = {
        let [a, b, c] = [normals[0], normals[1], normals[2]].map(plane_point);
        let determinant = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
        let u = ((b[1] - c[1]) * (q[0] - c[0]) + (c[0] - b[0]) * (q[1] - c[1])) / determinant;
        let v = ((c[1] - a[1]) * (q[0] - c[0]) + (a[0] - c[0]) * (q[1] - c[1])) / determinant;
        lower == 0. || (u >= -1e-10 && v >= -1e-10 && u + v <= 1. + 1e-10)
    };
    let indices = if inside_first { [0, 1, 2] } else { [0, 2, 3] };
    let points: [Point; 3] = indices.map(|index| {
        let n = normals[index];
        let height = sample_height(n);
        std::array::from_fn(|i| n[i] * (RADIUS + height) - centre[i] * RADIUS)
    });
    let face = cross(
        std::array::from_fn(|i| points[1][i] - points[0][i]),
        std::array::from_fn(|i| points[2][i] - points[0][i]),
    );
    let reference: Point = std::array::from_fn(|i| (query[i] - centre[i]) * RADIUS);
    let denominator = dot(face, query);
    if denominator.abs() < 1e-16 {
        return None;
    }
    Some(dot(face, std::array::from_fn(|i| points[0][i] - reference[i])) / denominator)
}

#[path = "ToolCut.rs"]
pub(crate) mod tool_cut;
#[path = "MaterialGeometry.rs"]
pub(crate) mod material_geometry;

#[path = "Woodland.rs"]
pub(crate) mod woodland;

#[path="MaterialDomain.rs"]
pub(crate) mod material_domain;
pub(crate) const BEDROCK_ELEVATION:f64 = -96.;

#[path="Tetrahedral.rs"]
pub(crate) mod tetrahedral;

#[path="Nightworld.rs"]
pub(crate) mod nightworld;
