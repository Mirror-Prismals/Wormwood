//! Indexed local geometry shared by native vegetation backends. Leaf materials
//! retain normals/UVs/thickness; RGB conversion is only a compatibility fallback.
use super::{Tree, math::*, profiles::Profile};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Detail {
    Near,
    Middle,
    Far,
}
#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub position: V,
    pub normal: V,
    pub uv: [f32; 2],
    pub color: [f32; 3],
    /// 0=bark, 1=leaf, 2=broadleaf spray, 3/4=needles; 6=whole-tree baked image in Forest.
    pub material: u32,
    pub transmission: f32,
    pub roughness: f32,
    pub occlusion: f32,
    pub branch_id: u32,
}
#[derive(Default, Clone)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}
impl Mesh {
    fn tri(&mut self, a: Vertex, b: Vertex, c: Vertex) {
        let i = self.vertices.len() as u32;
        self.vertices.extend([a, b, c]);
        self.indices.extend([i, i + 1, i + 2]);
    }
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }
}
fn vertex(
    position: V,
    normal: V,
    uv: [f32; 2],
    color: [f32; 3],
    material: u32,
    profile: Profile,
    occlusion: f32,
    branch_id: u32,
) -> Vertex {
    Vertex {
        position,
        normal,
        uv,
        color,
        material,
        transmission: if material == 0 {
            0.
        } else {
            profile.transmission
        },
        roughness: if material == 0 {
            profile.roughness
        } else {
            0.63
        },
        occlusion,
        branch_id,
    }
}

fn leaf_normal(
    normal: V,
    axis: V,
    across: V,
    length: f64,
    width: f64,
    curl: f64,
    u: f64,
    v: f64,
) -> V {
    // Differentiate in physical metres along each axis, including aspect ratio.
    unit(sub(
        normal,
        add(
            mul(axis, curl * (u - 0.5) * 4.),
            mul(across, curl * length / width * v * 0.7),
        ),
    ))
}

pub fn build(tree: &Tree, profile: Profile, detail: Detail, time: f64) -> Mesh {
    build_seasonal(tree, profile, detail, time, 1.)
}
pub fn build_seasonal(
    tree: &Tree,
    profile: Profile,
    detail: Detail,
    time: f64,
    cover: f64,
) -> Mesh {
    let mut mesh = Mesh::default();
    let authored = super::species::asset(profile);
    let sides = match detail {
        Detail::Near => 7,
        Detail::Middle => 5,
        Detail::Far => 3,
    };
    for branch in &tree.branches {
        if detail == Detail::Far && branch.order > 1 {
            continue;
        }
        let tangent = unit(sub(branch.end, branch.start));
        let (e, n) = basis(tangent);
        let r = branch.radius;
        let end_r = r * if branch.order == 0 { 0.77 } else { 0.36 };
        for side in 0..sides {
            let angle = side as f64 * std::f64::consts::TAU / sides as f64;
            let angle2 = (side + 1) as f64 * std::f64::consts::TAU / sides as f64;
            let radial = |a: f64| add(mul(e, a.cos()), mul(n, a.sin()));
            let point = |end: bool, a: f64| {
                let p = add(
                    if end { branch.end } else { branch.start },
                    mul(radial(a), if end { end_r } else { r }),
                );
                tree.wind(p, time)
            };
            let bark = |end: bool, a: f64, u: f32| {
                vertex(
                    point(end, a),
                    radial(a),
                    [
                        u,
                        if end {
                            length(sub(branch.end, branch.start)) as f32
                        } else {
                            0.
                        },
                    ],
                    profile.bark_color,
                    0,
                    profile,
                    0.87,
                    branch.id,
                )
            };
            let a = bark(false, angle, side as f32 / sides as f32);
            let b = bark(false, angle2, (side + 1) as f32 / sides as f32);
            let c = bark(true, angle2, (side + 1) as f32 / sides as f32);
            let d = bark(true, angle, side as f32 / sides as f32);
            mesh.tri(a, b, c);
            mesh.tri(a, c, d);
        }
        if branch.order != 2 {
            continue;
        }
    }
    for branch in tree.branches.iter().filter(|b| b.order == 2) {
        let count = match detail {
            Detail::Near => 36,
            Detail::Middle => 12,
            Detail::Far => 6,
        };
        let seed = hash(tree.site.id ^ branch.id as u64 * 7919);
        let extent = tree.height * 0.12;
        for leaf in 0..count {
            // Shared prefix preserves cluster identity across detail levels.
            let s = hash(seed ^ (leaf as u64).wrapping_mul(0x9e3779b97f4a7c15));
            if random(s, 909) >= cover {
                continue;
            }
            let angle = random(s, 1) * std::f64::consts::TAU;
            let radius = random(s, 2).sqrt() * extent;
            let centre = add(
                add(branch.start, mul(sub(branch.end, branch.start), 0.25 + random(s, 10) * 0.75)),
                [
                    angle.cos() * radius,
                    (random(s, 3) - 0.40) * extent * 0.90,
                    angle.sin() * radius,
                ],
            );
            let flutter = (time * 3.8 + random(s, 4) * 6.283185307).sin() * 0.16;
            let normal = unit([
                angle.cos() * 0.55,
                (random(s, 5) - 0.35) * 1.6,
                angle.sin() * 0.55,
            ]);
            let normal = unit(add(normal, [flutter, 0., flutter * 0.4]));
            let (e, n) = basis(normal);
            let rotation = random(s, 6) * 6.283185307;
            let axis = add(mul(e, rotation.cos()), mul(n, rotation.sin()));
            let across = cross(normal, axis);
            let size = profile.leaf_length * (0.75 + random(s, 7) * 0.5);
            let (length, width, material) = match detail {
                // Resolve individual leaves nearby over the same leafy twig volume.
                // Sprays supply the many subpixel leaves without a full leaf mesh.
                Detail::Near if leaf < (if authored.is_some() {10} else {24}) => (size, profile.leaf_width, 1),
                Detail::Near | Detail::Middle => (extent * 2.0, extent * 1.6, 2),
                Detail::Far => (extent * 2.8, extent * 2.2, 2),
            };
            let material=if profile.evergreen() && material==2 { if profile.name=="eastern_white_pine" {3} else {4} } else {material};
            let hue = 0.83 + random(s, 8) as f32 * 0.30;
            let color = profile.leaf_color.map(|c| c * hue);
            let occlusion = (0.50 + (centre[1] / tree.height * 0.38)) as f32;
            let make = |u: f64, v: f64| {
                let bend = profile.leaf_curl * length * ((u - 0.5) * (u - 0.5) * 2. + v * v * 0.35);
                let p = add(
                    centre,
                    add(
                        add(mul(axis, (u - 0.5) * length), mul(across, v * width)),
                        mul(normal, bend),
                    ),
                );
                let p = tree.wind(p, time);
                let curved_normal =
                    leaf_normal(normal, axis, across, length, width, profile.leaf_curl, u, v);
                vertex(
                    p,
                    curved_normal,
                    [u as f32, (v + 0.5) as f32],
                    color,
                    material,
                    profile,
                    occlusion,
                    branch.id,
                )
            };
            if material == 1 && authored.is_some() {
                let outline=&authored.unwrap().outline;
                let middle=make(0.5,0.);
                for i in 0..outline.len() {
                    let a=outline[i];let b=outline[(i+1)%outline.len()];
                    mesh.tri(middle,make(a[0],a[1]),make(b[0],b[1]));
                }
            } else if material == 1 {
                let base = make(0., 0.);
                let tip = make(1., 0.);
                let mid = make(0.5, 0.);
                let l1 = make(0.28, -0.40);
                let l2 = make(0.67, -0.46);
                let r1 = make(0.28, 0.40);
                let r2 = make(0.67, 0.46);
                for [a, b, c] in [
                    [base, l1, mid],
                    [l1, l2, mid],
                    [l2, tip, mid],
                    [base, mid, r1],
                    [r1, mid, r2],
                    [r2, mid, tip],
                ] {
                    mesh.tri(a, b, c);
                }
            } else {
                let a = make(0., -0.5);
                let b = make(1., -0.5);
                let c = make(1., 0.5);
                let d = make(0., 0.5);
                mesh.tri(a, b, c);
                mesh.tri(a, c, d);
            }
        }
    }
    mesh
}

/// Shared analytic silhouette for individual leaves and perforated leaf sprays.
/// The fragment shaders use the equivalent equations, including shadow passes.
pub fn coverage(uv: [f64; 2], spray: bool) -> f64 {
    let ellipse = |x: f64, y: f64, rx: f64, ry: f64| {
        1. - ((uv[0] - x) / rx).powi(2) - ((uv[1] - y) / ry).powi(2)
    };
    if !spray {
        return ellipse(0.5, 0.5, 0.50, 0.48);
    }
    let cell = [(uv[0] * 20.).floor(), (uv[1] * 16.).floor()];
    let fract = |v: f64| v - v.floor();
    let seed = fract((cell[0] * 127.1 + cell[1] * 311.7).sin() * 43758.5453);
    let q = [fract(uv[0] * 20.) - 0.5 - (seed - 0.5) * 0.65,
        fract(uv[1] * 16.) - 0.5 - (fract(seed * 17.) - 0.5) * 0.65];
    let tilt = (seed - 0.5) * 1.4;
    let x = q[0] * tilt.cos() - q[1] * tilt.sin();
    let y = q[0] * tilt.sin() + q[1] * tilt.cos();
    (1. - (x / 0.48).powi(2) - (y / 0.42).powi(2))
        .min(ellipse(0.5, 0.5, 0.50, 0.49) + (seed - 0.5) * 0.45)
}

/// Lighting fallback for the existing RGB path: diffuse, two-sided transmission
/// and procedural bark relief. Native material path additionally uses sun shadows.
pub fn shade(v: &Vertex, sun: V, view: V, visibility: f64) -> [f32; 3] {
    let mut n = unit(v.normal);
    let sun = unit(sun);
    let view = unit(view);
    let mut base = v.color.map(f64::from);
    if v.material == 0 {
        let ridge = (v.uv[0] as f64 * 180. + (v.uv[1] as f64 * 5.).sin() * 0.7).sin();
        let (e, _) = basis(n);
        n = unit(add(n, mul(e, ridge * 0.28)));
        base = base.map(|c| c * (0.86 + 0.14 * ridge));
    }
    if dot(n, view) < 0. && v.material != 0 {
        n = mul(n, -1.);
    }
    let lambert = dot(n, sun).max(0.);
    let back = dot(mul(n, -1.), sun).max(0.);
    let wrap = ((dot(n, sun) + 0.35) / 1.35).max(0.);
    let transmission = if v.material == 0 {
        0.
    } else {
        v.transmission as f64 * back * (0.45 + 0.55 * dot(mul(view, -1.), sun).max(0.).powi(3))
    };
    let light = 0.19 * v.occlusion as f64
        + visibility.clamp(0., 1.) * (0.66 * lambert + 0.15 * wrap + transmission);
    std::array::from_fn(|i| (base[i] * light) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vegetation::{Site, initial_state, profiles::PROFILES};
    fn tree() -> Tree {
        let site = Site {
            id: 42,
            normal: [0., 1., 0.],
            origin: [0., crate::terrain::RADIUS, 0.],
            profile: 0,
            moisture: 0.6,
            soil_depth: 1.,
        };
        Tree::generate(site, &initial_state(site), PROFILES[0])
    }
    #[test]
    fn winter_and_partial_leaf_drop_preserve_the_same_branch_mesh() {
        let tree = tree();
        let profile = PROFILES[0];
        for detail in [Detail::Near, Detail::Middle, Detail::Far] {
            let summer = build_seasonal(&tree, profile, detail, 0., 1.);
            let autumn = build_seasonal(&tree, profile, detail, 0., 0.5);
            let winter = build_seasonal(&tree, profile, detail, 0., 0.);
            assert!(summer.vertices.iter().any(|v| v.material > 0));
            assert!(winter.vertices.iter().all(|v| v.material == 0));
            let wood = |mesh: &Mesh| {
                mesh.vertices
                    .iter()
                    .filter(|v| v.material == 0)
                    .map(|v| (v.position, v.branch_id))
                    .collect::<Vec<_>>()
            };
            assert_eq!(wood(&summer), wood(&winter));
            assert_eq!(wood(&summer), wood(&autumn));
            assert!(
                autumn.vertices.len() >= winter.vertices.len()
                    && autumn.vertices.len() <= summer.vertices.len()
            );
            assert!(autumn.vertices.iter().filter(|v| v.material > 0).all(|v| {
                summer
                    .vertices
                    .iter()
                    .any(|s| s.material == v.material && s.position == v.position)
            }));
        }
    }
    #[test]
    fn curved_leaves_and_open_sprays() {
        let t = tree();
        let mesh = build(&t, PROFILES[0], Detail::Near, 0.);
        assert!(
            mesh.vertices
                .iter()
                .any(|v| v.material == 1 && v.transmission > 0.)
        );
        assert!(mesh.vertices.iter().all(|v| {
            v.position
                .iter()
                .chain(v.normal.iter())
                .all(|x| x.is_finite())
        }));
        assert!(coverage([0.0, 0.0], true) < 0.);
        assert!(coverage([0.525, 0.53125], true) > 0.);
    }
    #[test]
    fn leafy_crowns_keep_vertical_coverage_at_each_detail() {
        let t = tree();
        for detail in [Detail::Near, Detail::Middle, Detail::Far] {
            let mesh = build(&t, PROFILES[0], detail, 0.);
            // Ground-level views must see crown area from both horizontal axes.
            // A large horizontal fan has almost no area in these projections.
            for axis in [0, 2] {
                let area: f64 = mesh.vertices.chunks_exact(3)
                    .filter(|v| v[0].material > 0)
                    .map(|v| {
                        let a = sub(v[1].position, v[0].position);
                        let b = sub(v[2].position, v[0].position);
                        (a[axis] * b[1] - a[1] * b[axis]).abs() * 0.5
                    }).sum();
                assert!(area > t.crown_radius * t.crown_radius * 2., "{detail:?}: {area}");
            }
        }
    }
    #[test]
    fn wind_keeps_roots_fixed() {
        let t = tree();
        assert_eq!(t.wind([0.; 3], 2.), [0.; 3]);
        assert_ne!(
            t.wind([0., t.height, 0.], 0.),
            t.wind([0., t.height, 0.], 1.)
        );
    }

    #[test]
    fn wind_normals_follow_the_deformed_surface_above_and_below_bending_limits() {
        let t = tree();
        let normal = unit([0.3, 1., -0.4]);
        let (a, b) = basis(normal);
        let epsilon = 1e-5;
        for height in [-1., t.height * 0.4, t.height + 1.] {
            let p = [0.2, height, -0.1];
            let derivative = |axis: V| {
                unit(sub(
                    t.wind(add(p, mul(axis, epsilon)), 1.7),
                    t.wind(sub(p, mul(axis, epsilon)), 1.7),
                ))
            };
            let n = t.wind_normal(p, normal, 1.7);
            assert!(dot(n, derivative(a)).abs() < 1e-8);
            assert!(dot(n, derivative(b)).abs() < 1e-8);
        }
    }
    #[test]
    fn detail_reduces_geometry() {
        let t = tree();
        assert!(
            build(&t, PROFILES[0], Detail::Near, 0.).triangles()
                > build(&t, PROFILES[0], Detail::Middle, 0.).triangles()
        );
        assert!(
            build(&t, PROFILES[0], Detail::Middle, 0.).triangles()
                > build(&t, PROFILES[0], Detail::Far, 0.).triangles()
        );
    }

    #[test]
    fn narrow_curved_leaf_normals_match_surface_tangents() {
        let normal = unit([0.2, 1., -0.3]);
        let (axis, across) = basis(normal);
        for width in [0.025, 0.085, 0.3] {
            let length = 0.15;
            let curl = 0.21;
            let surface = |u: f64, v: f64| {
                add(
                    add(mul(axis, (u - 0.5) * length), mul(across, v * width)),
                    mul(
                        normal,
                        curl * length * (2. * (u - 0.5).powi(2) + 0.35 * v * v),
                    ),
                )
            };
            for (u, v) in [(0.28, -0.40), (0.67, 0.46), (0.5, 0.)] {
                let epsilon = 1e-5;
                let du = unit(sub(surface(u + epsilon, v), surface(u - epsilon, v)));
                let dv = unit(sub(surface(u, v + epsilon), surface(u, v - epsilon)));
                let n = leaf_normal(normal, axis, across, length, width, curl, u, v);
                assert!(dot(n, du).abs() < 1e-9);
                assert!(dot(n, dv).abs() < 1e-9);
                assert!((crate::vegetation::math::length(n) - 1.).abs() < 1e-12);
            }
        }
    }
}
