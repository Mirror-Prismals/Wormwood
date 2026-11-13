//! Bounded cohesive tetrahedral soil with breakable root fibres and native OGC contact.
//! The volume is filled before tool motion. No gesture, timer or swept polygon deletes soil.
use crate::{
    host::Host,
    physics::{Scene, math::*},
    terrain::{self, tool_cut},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
type D = [f64; 3];
fn d(v: &Value) -> Result<D, String> {
    serde_json::from_value(v.clone()).map_err(|e| e.to_string())
}
fn dd(a: D, b: D) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn addd(a: D, b: D) -> D {
    std::array::from_fn(|i| a[i] + b[i])
}
fn subd(a: D, b: D) -> D {
    std::array::from_fn(|i| a[i] - b[i])
}
fn dm(a: D, s: f64) -> D {
    a.map(|v| v * s)
}
fn du(a: D) -> D {
    dm(a, 1. / dd(a, a).sqrt().max(1e-12))
}
fn dc(a: D, b: D) -> D {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn point(pose: &Value, p: V) -> D {
    let a = d(&pose["position"]).unwrap();
    let axes = [
        d(&pose["right"]).unwrap(),
        d(&pose["up"]).unwrap(),
        d(&pose["forward"]).unwrap(),
    ];
    std::array::from_fn(|i| a[i] + (0..3).map(|k| axes[k][i] * p[k] as f64).sum::<f64>())
}
// Build two crossed, double-sided blades in the moving face's local frame.
// The base follows the three accepted soil nodes, including after separation.
fn grass_tuft(face: [V; 3], interior: V) -> Vec<V> {
    let center = scale(add(add(face[0], face[1]), face[2]), 1. / 3.);
    let tangent = unit(sub(face[1], face[0]));
    let mut normal = unit(cross(sub(face[1], face[0]), sub(face[2], face[0])));
    if dot(normal, sub(interior, center)) > 0. { normal = scale(normal, -1.); }
    let side = unit(cross(normal, tangent));
    let base = add(center, scale(normal, 0.006));
    let mut vertices = Vec::with_capacity(12);
    for axis in [tangent, side] {
        let left = sub(base, scale(axis, 0.012));
        let right = add(base, scale(axis, 0.012));
        let tip = add(add(base, scale(normal, 0.12)), scale(axis, 0.012));
        vertices.extend([left, right, tip, tip, right, left]);
    }
    vertices
}
#[cfg(test)]
mod grass_tests {
    use super::*;
    #[test]
    fn tuft_follows_its_soil_face_through_translation_and_rotation() {
        let face = [[0., 1., 0.], [1., 1., 0.], [0., 1., 1.]];
        let inside = [0., 0., 0.];
        let original = grass_tuft(face, inside);
        let transform = |p: V| [p[1] + 2., -p[0] + 3., p[2] - 4.];
        let moved = grass_tuft(face.map(transform), transform(inside));
        assert_eq!(original.len(), 12);
        for (a, b) in original.into_iter().zip(moved) {
            assert!(length(sub(transform(a), b)) < 1e-5);
        }
    }
}
pub(crate) fn valid_ray(v: &Value) -> bool {
    ["position", "forward"]
        .iter()
        .all(|k| d(&v[*k]).is_ok_and(|p| p.iter().all(|x| x.is_finite() && x.abs() < 1e9)))
}
pub(crate) fn valid_pose(v: &Value) -> bool {
    ["position", "right", "up", "forward"]
        .iter()
        .all(|k| d(&v[*k]).is_ok_and(|p| p.iter().all(|x| x.is_finite() && x.abs() < 1e9)))
        && ["right", "up", "forward"]
            .iter()
            .all(|k| d(&v[*k]).is_ok_and(|p| (dd(p, p) - 1.).abs() < 1e-5))
        && d(&v["right"])
            .ok()
            .zip(d(&v["up"]).ok())
            .is_some_and(|(a, b)| dd(a, b).abs() < 1e-5)
}
fn valid_blade(v: &Value) -> bool {
    v["vertices"].as_array().is_some_and(|p| {
        (4..=256).contains(&p.len())
            && p.iter()
                .all(|p| d(p).is_ok_and(|p| p.iter().all(|x| x.is_finite() && x.abs() < 2.)))
    }) && v["triangles"].as_array().is_some_and(|f| {
        !f.is_empty()
            && f.len() <= 512
            && f.iter().all(|f| {
                f.as_array().is_some_and(|f| {
                    f.len() == 3
                        && f.iter().all(|i| {
                            i.as_u64()
                                .is_some_and(|i| i < v["vertices"].as_array().unwrap().len() as u64)
                        })
                })
            })
    })
}
pub(crate) fn valid_cut(v: &Value) -> bool {
    valid_pose(&v["pose"]) && tool_cut::valid_edits(&json!([v["edit"]])) && valid_blade(&v["blade"])
}
pub(crate) fn valid_motion(v: &Value) -> bool {
    valid_pose(&v["pose"])
        && v["dt"]
            .as_f64()
            .is_some_and(|x| x.is_finite() && (0.0..=0.05).contains(&x))
        && (v["blade"].is_null() || valid_blade(&v["blade"]))
        && (v["equipped"].is_null() || v["equipped"].is_boolean())
        && (v["release"].is_null() || v["release"].is_boolean())
        && (v["optional"].is_null() || v["optional"].is_boolean())
        && (v["patch_position"].is_null() || d(&v["patch_position"]).is_ok_and(|p|p.iter().all(|x|x.is_finite()&&x.abs()<1e9)))
        && (v["grasp"].is_null() || {
            let g = &v["grasp"];
            ["points", "shoulders"].iter().all(|k| {
                g[*k].as_array().is_some_and(|a| {
                    a.len() == 2
                        && a.iter().all(|p| {
                            d(p).is_ok_and(|p| p.iter().all(|n| n.is_finite() && n.abs() < 1e9))
                        })
                })
            }) && ["minimum", "maximum"].iter().all(|k| {
                g[*k].as_array().is_some_and(|a| {
                    a.len() == 2
                        && a.iter().all(|n| {
                            n.as_f64()
                                .is_some_and(|n| n.is_finite() && (0.0..=3.).contains(&n))
                        })
                })
            }) && (0..2)
                .all(|i| g["maximum"][i].as_f64().unwrap() > g["minimum"][i].as_f64().unwrap())
                && g["support_range"].as_array().is_some_and(|a| {
                    a.len() == 2
                        && a.iter().all(|n| {
                            n.as_f64()
                                .is_some_and(|n| n.is_finite() && (0.0..=2.).contains(&n))
                        })
                        && a[0].as_f64().unwrap() <= a[1].as_f64().unwrap()
                })
        })
        && (v["control_point"].is_null()
            || d(&v["control_point"])
                .is_ok_and(|p| p.iter().all(|x| x.is_finite() && x.abs() <= 2.)))
}
#[derive(Clone)]
struct Cell {
    start: usize,
    faces: Vec<[u32; 3]>,
    volume: f64,
    anchored: bool,
    initial_height: f32,
}
#[derive(Clone)]
struct Bond {
    a: usize,
    b: usize,
    cells: [usize; 2],
    rest: V,
    lambda: V,
    compliance: f32,
    limit: f32,
    extension: f32,
    root: bool,
    broken: bool,
    peak_load: f32,
    interface: Option<usize>,
    area: f32,
}
#[derive(Clone)]
struct Interface {
    a: [usize; 3],
    b: [usize; 3],
    cells: [usize; 2],
    bonds: [usize; 3],
    gap: f32,
    normal_sign: f32,
}
#[derive(Clone)]
struct SkinFace {
    nodes: [u32; 3],
    // A skin seam tears with its supporting material interfaces.
    interfaces: Vec<usize>,
}
#[derive(Clone)]
struct Patch {
    scene: Scene,
    cells: Vec<Cell>,
    // Visible tufts are carried by the original top faces. Their geometry
    // is visual only; it is not added to the contact or water-collision mesh.
    grass_faces: Vec<([usize; 3], usize)>,
    bonds: Vec<Bond>,
    cohesive: Vec<Interface>,
    skin: Vec<SkinFace>,
    tool: Vec<V>,
    tool_start: usize,
    pose: Value,
    rigid_body: super::rigid_contact::Body,
    total_volume: f64,
    detached: BTreeSet<usize>,
    detached_volume: f64,
    peak_lift: f32,
}
#[derive(Default)]
pub(crate) struct State {
    soil_enabled: Option<bool>,
    cutting_selected: Option<bool>,
    volume: Option<super::soil_volume::State>,
    tool_controls: super::tool_motion::State,
    anchor: Option<D>,
    basis: [D; 3],
    patch: Option<Patch>,
    active: bool,
    equipped: bool,
    ever_released: bool,
    velocity: D,
    accumulator: f64,
    failure: Option<String>,
    blocked: bool,
    contacts: Value,
    peak_pairs: u64,
    force: D,
    contact_effort: f64,
    torque: D,
    accepted_steps: u64,
    rejected_steps: u64,
    requested_seconds: f64,
    budget_discarded_seconds: f64,
    advance_calls: u64,
    advance_ms: f64,
    native_ms: f64,
    minimum_rigid_bound: f64,
    near_zero_rigid_vertices: usize,
    grasp_feasible: bool,
}
impl State {
    #[cfg(test)]
    pub(crate) fn select_soil_experiment(&mut self, enabled: bool) {
        self.soil_enabled = Some(enabled);
    }

    fn local(&self, p: D) -> V {
        let q = subd(p, self.anchor.unwrap());
        std::array::from_fn(|i| dd(q, self.basis[i]) as f32)
    }
    fn global(&self, p: V) -> D {
        addd(
            self.anchor.unwrap(),
            std::array::from_fn(|i| (0..3).map(|j| self.basis[j][i] * p[j] as f64).sum()),
        )
    }
    fn world_vector(&self, p: V) -> D {
        std::array::from_fn(|i| (0..3).map(|j| self.basis[j][i] * p[j] as f64).sum())
    }
    fn spec(p: &Patch) -> Value {
        let mut v = p.scene.spec();
        v["contact_stiffness"] = json!(80000.);
        v["surfaces"][0]["element_ids"] = json!(
            (0..p.cells.len())
                .flat_map(|i| [i as u32; 4])
                .collect::<Vec<_>>()
        );
        v["surfaces"][0]["excluded_pairs"] = json!(contact_exclusions(p));
        for s in v["surfaces"].as_array_mut().unwrap() {
            s["friction"] = json!(0.65);
        }
        v["surfaces"].as_array_mut().unwrap().last_mut().unwrap()["motion"] = json!(0);
        v
    }
    fn initialize(&mut self, host: &mut Host, input: &Value) -> Result<(), String> {
        if !valid_blade(&input["blade"]) {
            return Err("first equipped advance requires a closed blade collision shell".into());
        }
        let rigid_body = super::rigid_contact::Body::from_value(&input["blade"]["rigid_body"])?;

        let tool: Vec<V> = serde_json::from_value(input["blade"]["vertices"].clone())
            .map_err(|e| e.to_string())?;
        let centre = tool
            .iter()
            .fold([0.; 3], |a, p| add(a, *p))
            .map(|x| x / tool.len() as f32);
        let n = if input["patch_position"].is_null() { du(point(&input["pose"], centre)) }
            else { du(d(&input["patch_position"])?) };
        let mut right = d(&input["pose"]["right"])?;
        right = du(subd(right, dm(n, dd(right, n))));
        self.anchor = Some(dm(n, terrain::RADIUS));
        self.basis = [right, n, du(dc(right, n))];
        let mut scene = Scene::new("rooted_soil_patch");
        scene.radius = 0.0015;
        scene.iterations = 3;
        scene.proxy="Coarse cohesive tetrahedral soil with interface failure, massless tension-limited rooted fibres, XPBD elasticity/volume and native OGC contact; not a calibrated continuum soil law. Material exists before interaction; no gesture-driven cutout.".into();
        let (nx, ny, nz) = (2usize, 2usize, 2usize);
        let (width, depth, thickness) = (0.52f32, 0.72f32, 0.24f32);
        let mut grid = Vec::new();
        for y in 0..=ny {
            for z in 0..=nz {
                for x in 0..=nx {
                    let px = (x as f32 / nx as f32 - 0.5) * width;
                    let pz = (z as f32 / nz as f32 - 0.5) * depth;
                    let normal = du(self.global([px, 0., pz]));
                    let h=host.invoke("PlanetSurfaceSystem.Height",json!({"position":normal}))?["height"].as_f64().unwrap()as f32;
                    // A conforming tetrahedral mesh, not occupancy voxels. Interior planes are
                    // perturbed deterministically; cohesive interfaces are duplicated for fracture.
                    let j = if x > 0 && x < nx {
                        0.012 * ((z * 7 + y * 3) as f32).sin()
                    } else {
                        0.
                    };
                    grid.push([
                        px + j,
                        h - thickness + (y as f32 / ny as f32) * thickness,
                        pz,
                    ]);
                }
            }
        }
        let id = |x, y, z| y * (nz + 1) * (nx + 1) + z * (nx + 1) + x;
        let mut cells = Vec::new();
        let mut interfaces: BTreeMap<[usize; 3], (usize, [usize; 3])> = BTreeMap::new();
        let mut bonds = Vec::new();
        let mut cohesive = Vec::new();
        let mut grass_faces = Vec::new();
        let mut material_vertices = Vec::new();
        for y in 0..ny {
            for z in 0..nz {
                for x in 0..nx {
                    let c = [
                        id(x, y, z),
                        id(x + 1, y, z),
                        id(x, y + 1, z),
                        id(x + 1, y + 1, z),
                        id(x, y, z + 1),
                        id(x + 1, y, z + 1),
                        id(x, y + 1, z + 1),
                        id(x + 1, y + 1, z + 1),
                    ];
                    for tet in [
                        [0, 1, 3, 7],
                        [0, 3, 2, 7],
                        [0, 2, 6, 7],
                        [0, 6, 4, 7],
                        [0, 4, 5, 7],
                        [0, 5, 1, 7],
                    ] {
                        let original = tet.map(|i| c[i]);
                        let raw = original.map(|i| grid[i]);
                        let center = raw.into_iter().fold([0.; 3], add).map(|v| v / 4.);
                        // Finite pore space prevents coincident closed contact faces at initialization.
                        // Its actual solid volume, rather than the enclosing box, determines mass.
                        let p = raw.map(|p| add(center, scale(sub(p, center), 0.95)));
                        let volume = (dot(sub(p[1], p[0]), cross(sub(p[2], p[0]), sub(p[3], p[0])))
                            / 6.)
                            .abs() as f64;
                        let start = scene.p.len();
                        for v in p {
                            scene.node(v, (volume * 1600. / 4.) as f32, [0.32, 0.20, 0.10]);
                        }
                        material_vertices.extend(original);
                        let mut anchored = false;
                        for (j, g) in original.iter().enumerate() {
                            if *g < ((nz + 1) * (nx + 1)) {
                                scene.weights[start + j] = 0.;
                                anchored = true;
                            }
                        }
                        scene.tetra([start, start + 1, start + 2, start + 3]);
                        let mut faces = Vec::new();
                        let cell_id = cells.len();
                        for omit in 0..4 {
                            let mut face = std::array::from_fn::<_, 3, _>(|i| (i + omit + 1) % 4);
                            let a = p[face[0]];
                            if dot(
                                cross(sub(p[face[1]], a), sub(p[face[2]], a)),
                                sub(p[omit], a),
                            ) > 0.
                            {
                                face.swap(1, 2);
                            }
                            faces.push(face.map(|i| i as u32));
                            if face.iter().all(|i| original[*i] >= ny * (nz + 1) * (nx + 1))
                            {
                                grass_faces.push((face.map(|i| start + i), start + omit));
                            }
                            let mut key = face.map(|i| original[i]);
                            key.sort();
                            let nodes =
                                key.map(|g| start + original.iter().position(|x| *x == g).unwrap());
                            if let Some((other, old)) = interfaces.remove(&key) {
                                let normal = unit(cross(
                                    sub(scene.p[old[1]], scene.p[old[0]]),
                                    sub(scene.p[old[2]], scene.p[old[0]]),
                                ));
                                let gap = dot(sub(scene.p[nodes[0]], scene.p[old[0]]), normal)
                                    .abs()
                                    * 0.8;
                                let first = bonds.len();
                                let interface_id = cohesive.len();
                                let middle = std::array::from_fn(|j| scale(add(scene.p[old[j]], scene.p[nodes[j]]), 0.5));
                                let mut frame = super::soil_material::face_frame(middle);
                                let normal_sign = if dot(frame[2], sub(scene.p[nodes[0]], scene.p[old[0]])) < 0. { -1. } else { 1. };
                                frame[1] = scale(frame[1], normal_sign);
                                frame[2] = scale(frame[2], normal_sign);
                                let area = length(cross(sub(middle[1],middle[0]),sub(middle[2],middle[0]))) / 6.;

                                cohesive.push(Interface {
                                    a: old,
                                    b: nodes,
                                    cells: [other, cell_id],
                                    bonds: [first, first + 1, first + 2],
                                    gap: gap.max(0.0001),
                                    normal_sign,
                                });
                                for j in 0..3 {
                                    bonds.push(Bond {
                                        a: old[j],
                                        b: nodes[j],
                                        cells: [other, cell_id],
                                        rest: super::soil_material::coordinates(sub(scene.p[nodes[j]], scene.p[old[j]]),frame),
                                        lambda: [0.; 3],
                                        compliance: 1e-8,
                                        limit: 75.,
                                        extension: 0.018,
                                        root: false,
                                        broken: false,
                                        peak_load: 0.,
                                        interface: Some(interface_id),
                                        area,
                                    });
                                }
                            } else {
                                interfaces.insert(key, (cell_id, nodes));
                            }
                        }

                        cells.push(Cell {
                            start,
                            faces,
                            volume,
                            anchored,
                            initial_height: center[1],
                        });
                    }
                }
            }
        }
        for e in &mut scene.edges {
            e.compliance = 2e-6;
        }
        // Root fibres span the lower anchored earth and the upper soil. Their tension
        // and extension determine failure; input charge is deliberately absent.
        for z in 0..nz {
            for x in 0..nx {
                let lower = (z * nx + x) * 6;
                let upper = ((nz * nx) + (z * nx + x)) * 6;
                for k in [0usize, 2, 4] {
                    let a = cells[lower + k].start;
                    let b = cells[upper + k].start + 3;
                    bonds.push(Bond {
                        a,
                        b,
                        cells: [lower + k, upper + k],
                        rest: sub(scene.p[b], scene.p[a]),
                        lambda: [0.; 3],
                        compliance: 7e-6,
                        limit: 75. + ((x * 7 + z * 3 + k) % 5) as f32 * 15.,
                        extension: 0.045,
                        root: true,
                        broken: false,
                        peak_load: 0.,
                        interface: None,
                        area: 0.,
                    });
                }
            }
        }
        let skin = sod_skin(&scene, &cohesive, &material_vertices, ny * (nz + 1) * (nx + 1));
        let mut soil_faces: Vec<_> = cells
            .iter()
            .flat_map(|c| c.faces.iter().map(move |f| f.map(|i| c.start as u32 + i)))
            .collect();
        soil_faces.extend(skin.iter().map(|f|f.nodes));
        scene.surface(0, soil_faces, false, true);
        let top: Vec<f64> = [
            [-width / 2., -depth / 2.],
            [width / 2., -depth / 2.],
            [width / 2., depth / 2.],
            [-width / 2., depth / 2.],
        ]
        .iter()
        .map(|p| terrain::height(du(self.global([p[0], 0., p[1]]))))
        .collect();
        let bottom = top.iter().copied().fold(f64::INFINITY, f64::min) - thickness as f64 - 0.015;
        let outline = json!([
            [-width / 2., -depth / 2.],
            [width / 2., -depth / 2.],
            [width / 2., depth / 2.],
            [-width / 2., depth / 2.]
        ]);
        let edit = json!({"normal":n,"radius":0.6,"depth":thickness,"blade":{"right":right,"forward":self.basis[2],"outline":outline,"bottom":bottom,"top":top}});
        // Replace only the static presentation/collider for this same occupied volume.
        // The original material remains above this foundation in scene.p at full mass.
        let surface = host.invoke("PlanetSurfaceSystem.Frame", Value::Null)?;
        let mut edits = surface["edits"].as_array().unwrap().clone();
        edits.push(edit.clone());
        let ground_start = scene.p.len();
        let mut tris = Vec::new();
        let size = 6usize;
        let mut pts = Vec::new();
        for z in 0..=size {
            for x in 0..=size {
                let normal = du(self.global([(x as f32 - 3.) * 0.5, 0., (z as f32 - 3.) * 0.5]));
                let h = tool_cut::height(normal, terrain::height(normal), &edits, false);
                let p = dm(normal, terrain::RADIUS + h);
                pts.push([p[0], p[1], p[2], 0.3, 0.2, 0.1]);
            }
        }
        for z in 0..size {
            for x in 0..size {
                let a = z * (size + 1) + x;
                tris.extend([
                    [pts[a], pts[a + size + 1], pts[a + 1]],
                    [pts[a + 1], pts[a + size + 1], pts[a + size + 2]],
                ]);
            }
        }
        let mut weld: BTreeMap<[i64; 3], u32> = BTreeMap::new();
        let mut faces = Vec::new();
        for t in tool_cut::mesh(tris, &edits) {
            let mut face = [0; 3];
            for j in 0..3 {
                let p = self.local([t[j][0], t[j][1], t[j][2]]);
                let key = p.map(|v| (v * 1e6).round() as i64);
                face[j] = *weld.entry(key).or_insert_with(|| {
                    let i = (scene.p.len() - ground_start) as u32;
                    scene.node(p, 0., [0.3, 0.2, 0.1]);
                    i
                });
            }
            faces.push(face);
        }
        scene.surface(ground_start, faces, true, false);
        let tool_start = scene.p.len();
        for p in &tool {
            scene.node(self.local(point(&input["pose"], *p)), 0., [0.4, 0.4, 0.4]);
        }
        scene.surface(
            tool_start,
            serde_json::from_value(input["blade"]["triangles"].clone())
                .map_err(|e| e.to_string())?,
            false,
            false,
        );
        scene.finish()?;
        let intersections = scene.intersection_count();
        if intersections > 0 {
            return Err(format!(
                "soil patch initialization intersects ({intersections}); move the spade above the surface before equipping"
            ));
        }
        let total_volume = cells.iter().map(|c| c.volume).sum();
        if grass_faces.is_empty() { return Err("soil patch has no top face for grass".into()); }
        let patch = Patch {
            scene,
            cells,
            grass_faces,
            bonds,
            cohesive,
            skin,
            tool,
            tool_start,
            pose: input["pose"].clone(),
            rigid_body,
            total_volume,
            detached: BTreeSet::new(),
            detached_volume: 0.,
            peak_lift: 0.,
        };
        host.invoke("OGCConnectionSystem.Configure", Self::spec(&patch))?;
        if let Err(e) = host.invoke("PlanetSurfaceSystem.RefineMaterial", json!({"edit":edit})) {
            host.invoke("OGCConnectionSystem.Release", Value::Null)?;
            return Err(e);
        }
        self.patch = Some(patch);
        self.active = true;
        self.equipped = true;
        self.failure = None;
        Ok(())
    }
    fn connected(p: &mut Patch) {
        let mut graph = vec![Vec::new(); p.cells.len()];
        for b in p.bonds.iter().filter(|b| !b.broken) {
            let [a, z] = b.cells;
            graph[a].push(z);
            graph[z].push(a);
        }
        let mut attached = BTreeSet::new();
        let mut todo = VecDeque::new();
        for (i, c) in p.cells.iter().enumerate() {
            if c.anchored {
                attached.insert(i);
                todo.push_back(i);
            }
        }
        while let Some(i) = todo.pop_front() {
            for j in &graph[i] {
                if attached.insert(*j) {
                    todo.push_back(*j);
                }
            }
        }
        p.detached = (0..p.cells.len())
            .filter(|i| !attached.contains(i))
            .collect();
        p.detached_volume = p.detached.iter().map(|i| p.cells[*i].volume).sum();
    }
    fn solid_tetrahedra(&self) -> Vec<[[f64;3];4]> {
        self.patch.as_ref().map(|patch|patch.cells.iter().map(|cell|std::array::from_fn(|i|self.global(patch.scene.p[cell.start+i]))).collect()).unwrap_or_default()
    }
    fn material(&self) -> Value {
        self.patch.as_ref().map_or(json!({"initialized":false}),|p|json!({"initialized":true,"changed":p.detached_volume>0.,"model":"cohesive_tetrahedra_and_tension_roots","solid_volume":p.total_volume,"mass":p.total_volume*1600.,"detached_volume":p.detached_volume,"detached_mass":p.detached_volume*1600.,"detached_elements":p.detached.len(),"elements":p.cells.len(),"cohesion_broken":p.bonds.iter().filter(|b|!b.root&&b.broken).count(),"roots_broken":p.bonds.iter().filter(|b|b.root&&b.broken).count(),"roots":p.bonds.iter().filter(|b|b.root).count(),"peak_root_load":p.bonds.iter().filter(|b|b.root).map(|b|b.peak_load).fold(0f32,f32::max),"peak_lift":p.peak_lift,"patch_center":self.anchor.map(|a|addd(a,dm(self.basis[1],terrain::height(du(a))))),"bounded_patch":true,"cohesion_law":"rotating_face_pressure_dependent_traction","skin_triangles":p.skin.iter().filter(|f|skin_intact(p,f)).count(),"skin_torn_triangles":p.skin.iter().filter(|f|!skin_intact(p,f)).count(),"parcels":p.cells.iter().enumerate().map(|(id,c)|json!({"id":id,"position":self.global(p.scene.p[c.start..c.start+4].iter().copied().fold([0.;3],add).map(|x|x/4.)),"mass":c.volume*1600.,"detached":p.detached.contains(&id)})).collect::<Vec<_>>()}))
    }
    fn response(&self, pose: &Value) -> Value {
        json!({"requested_pose":pose,"pose":self.patch.as_ref().map_or(pose.clone(),|p|p.pose.clone()),"active":self.active,"blocked":self.blocked,"failure":self.failure,"force":self.force,"tool_contact_effort":self.contact_effort,"torque":self.torque,"grasp_feasible":self.grasp_feasible,"contact":self.contacts,"material":self.material()})
    }
    fn advance(&mut self, host: &mut Host, input: &Value) -> Result<Value, String> {
        let clock = std::time::Instant::now();
        let equipped = input["equipped"].as_bool().unwrap_or(true);
        let release = input["release"] == true;
        if self.patch.is_none() {
            // Advance never creates material. The session must explicitly prepare
            // a world-owned material region before a work request reaches it.
            return self.tool_controls.advance(host, input);
        }
        self.equipped = equipped;
        if release {
            self.ever_released = true;
        }
        let dt = input["dt"].as_f64().unwrap();
        self.requested_seconds += dt;
        // Bound contact work per display frame, preventing an overloaded frame
        // from requesting still more substeps on the next frame. Report discarded
        // wall time explicitly; material time remains the actually simulated time.
        let pending = self.accumulator + dt;
        self.accumulator = pending.min(2. / 120.);
        self.budget_discarded_seconds += (pending - self.accumulator).max(0.);

        let steps = (self.accumulator * 120.).floor() as usize;
        self.accumulator -= steps as f64 / 120.;
        self.blocked = false;
        self.failure = None;
        self.contact_effort = 0.;
        for _ in 0..steps {
            let prior_velocity = self.velocity;
            let mut step_accepted = false;
            for retry in 0..5 {
                self.velocity = prior_velocity;
                let mut p = self.patch.as_ref().unwrap().clone();
                let backup = p.clone();
                let step = (1f32 / 120.) * 0.5f32.powi(retry);
                let mut desired = if equipped {
                    input["pose"].clone()
                } else {
                    p.pose.clone()
                };
                if !equipped {
                    self.velocity = addd(self.velocity, dm(self.basis[1], -9.81 * step as f64));
                    desired["position"] = json!(addd(
                        d(&p.pose["position"])?,
                        dm(self.velocity, step as f64)
                    ));
                }
                let from = d(&p.pose["position"])?;
                desired = super::rigid_motion::approach(
                    &p.pose,
                    &desired,
                    if input["control_point"].is_null() {
                        [0.; 3]
                    } else {
                        d(&input["control_point"])?
                    },
                    step as f64,
                )?;
                orthogonalize(&mut desired)?;
                let previous = p.scene.p.clone();
                for i in 0..p.tool_start {
                    if p.scene.weights[i] > 0. {
                        p.scene.velocity[i] =
                            add(p.scene.velocity[i], scale(p.scene.gravity, step));
                        p.scene.p[i] = add(p.scene.p[i], scale(p.scene.velocity[i], step));
                    }
                }
                let requested_pose = desired.clone();
                let target: Vec<_> = p
                    .tool
                    .iter()
                    .map(|v| self.local(point(&requested_pose, *v)))
                    .collect();
                // Tool vertices remain at the current rigid pose. Their mass is
                // integrated in six generalized freedoms, never node by node.
                for e in &mut p.scene.edges {
                    e.lambda = 0.;
                }
                for t in &mut p.scene.tets {
                    t.lambda = 0.;
                }
                for b in &mut p.bonds {
                    b.lambda = [0.; 3];
                }
                let mut contact = Value::Null;
                let mut candidate_effort = 0f64;
                let mut coupled_valid = true;
                let mut contact_anchor=backup.scene.p.clone();
                for _ in 0..3 {
                    for _ in 0..4 {
                        p.scene.constraints(step);
                        solve_bonds(&mut p, step);
                        propagate_faces(&mut p);
                        project_cohesive_compression(&mut p);
                    }
                    if refresh_skin(&mut p) {
                        // Only topology changes here. Initialize the reduced contact
                        // surface at the last accepted coordinates, then project the
                        // candidate. Rejected candidates restore the complete backup.
                        let mut spec=Self::spec(&p);
                        spec["positions"]=json!(contact_anchor);
                        host.invoke("OGCConnectionSystem.Configure",spec)?;
                    }
                    let mut projected=host.invoke("OGCConnectionSystem.Project",json!({"positions":p.scene.p,"previous":previous,"inverse_mass":p.scene.weights,"dt":step,"excluded_pairs":contact_exclusions(&p),"rigid_response_start":p.tool_start}))?;
                    p.scene.p = serde_json::from_value(projected["positions"].take())
                        .map_err(|e| e.to_string())?;
                    if projected["contact_overflows"].as_u64().unwrap_or(0) > 0 {
                        return Err("rooted soil OGC contact capacity exceeded".into());
                    }
                    let responses: Vec<[f32; 13]> =
                        serde_json::from_value(projected["rigid_responses"].take())
                            .map_err(|e| e.to_string())?;
                    candidate_effort = candidate_effort.max(responses.iter().map(|r| {
                        ((r[0] as f64).powi(2)+(r[1] as f64).powi(2)+(r[2] as f64).powi(2)).sqrt()
                    }).sum::<f64>());
                    self.native_ms += projected["broad_ms"].as_f64().unwrap_or(0.)
                        + projected["narrow_ms"].as_f64().unwrap_or(0.);
                    self.minimum_rigid_bound = responses
                        .iter()
                        .map(|r| r[12] as f64)
                        .fold(f64::INFINITY, f64::min);
                    self.near_zero_rigid_vertices =
                        responses.iter().filter(|r| r[12] < 1e-7).count();
                    let old_rigid = p.scene.p[p.tool_start..].to_vec();
                    let old_rigid_pose = p.pose.clone();
                    let grasp = if equipped && input["grasp"].is_object() {
                        let g = &input["grasp"];
                        let make = |i: usize| -> Result<super::rigid_contact::Grip, String> {
                            Ok(super::rigid_contact::Grip {
                                point: d(&g["points"][i])?,
                                shoulder: self.local(d(&g["shoulders"][i])?).map(|x| x as f64),
                                minimum: g["minimum"][i].as_f64().unwrap(),
                                maximum: g["maximum"][i].as_f64().unwrap(),
                                slide: if i == 0 {
                                    Some([
                                        g["support_range"][0].as_f64().unwrap(),
                                        g["support_range"][1].as_f64().unwrap(),
                                    ])
                                } else {
                                    None
                                },
                            })
                        };
                        Some(super::rigid_contact::Grasp {
                            grips: [make(0)?, make(1)?],
                        })
                    } else {
                        None
                    };
                    let rigid = super::rigid_contact::project(
                        &p.tool,
                        &p.scene.p[p.tool_start..],
                        &target,
                        &responses,
                        &p.rigid_body,
                        grasp.as_ref(),
                        step as f64,
                    )?;
                    self.blocked |= rigid.restricted;
                    self.grasp_feasible = rigid.grasp_feasible;
                    let rigid = rigid.positions;
                    let (origin, axes) = super::rigid_fit::fit(&p.tool, &rigid)
                        .ok_or("rigid accepted pose fit failed")?;
                    desired = json!({"position":self.global(origin.map(|x|x as f32)),"right":self.world_vector(axes[0].map(|x|x as f32)),"up":self.world_vector(axes[1].map(|x|x as f32)),"forward":self.world_vector(axes[2].map(|x|x as f32))});
                    orthogonalize(&mut desired)?;
                    for (i, v) in p.tool.iter().enumerate() {
                        p.scene.p[p.tool_start + i] = self.local(point(&desired, *v));
                    }
                    let certified = p.scene.p[p.tool_start..]
                        .iter()
                        .zip(&old_rigid)
                        .zip(&responses)
                        .all(|((p, old), r)| length(sub(*p, *old)) as f64 <= r[12] as f64 + 1e-10);
                    if !certified {
                        p.scene.p[p.tool_start..].copy_from_slice(&old_rigid);
                        desired = old_rigid_pose;
                        self.blocked = true;
                    }
                    p.pose = desired.clone();
                    if p.scene.intersection_count() > 0 {
                        coupled_valid = false;
                        contact = projected;
                        break;
                    }
                    host.invoke("OGCConnectionSystem.Synchronize",json!({"surface_start":p.tool_start,"positions":&p.scene.p[p.tool_start..],"previous":&previous[p.tool_start..]}))?;
                    contact_anchor=p.scene.p.clone();
                    contact = projected;
                }
                let exclusions: BTreeSet<_> = contact_exclusions(&p).into_iter().collect();
                let valid = coupled_valid
                    && p.scene.p.iter().flatten().all(|x| x.is_finite())
                    && p.scene.tets.iter().all(|t| {
                        let q = t.ids.map(|i| p.scene.p[i]);
                        dot(sub(q[1], q[0]), cross(sub(q[2], q[0]), sub(q[3], q[0]))) * t.rest > 0.
                    }); // Final positions already passed the full crossing check in the last coupled pass.
                if !valid {
                    #[cfg(test)]
                    if self.rejected_steps == 0 {
                        let inverted = p
                            .scene
                            .tets
                            .iter()
                            .filter(|t| {
                                let q = t.ids.map(|i| p.scene.p[i]);
                                dot(sub(q[1], q[0]), cross(sub(q[2], q[0]), sub(q[3], q[0])))
                                    * t.rest
                                    <= 0.
                            })
                            .count();
                        let soil_nodes = p.cells.len() * 4;
                        let soil_ground = super::intersections::count_filtered(&p.scene, |a, b| {
                            !((a[0] < soil_nodes && b[0] >= soil_nodes && b[0] < p.tool_start)
                                || (b[0] < soil_nodes && a[0] >= soil_nodes && a[0] < p.tool_start))
                        });
                        let soil_tool = super::intersections::count_filtered(&p.scene, |a, b| {
                            !((a[0] < soil_nodes && b[0] >= p.tool_start)
                                || (b[0] < soil_nodes && a[0] >= p.tool_start))
                        });
                        let mut pairs = Vec::new();
                        super::intersections::count_filtered_visit(
                            &p.scene,
                            |a, b| {
                                if a.iter().any(|i| *i >= soil_nodes)
                                    || b.iter().any(|i| *i >= soil_nodes)
                                {
                                    return false;
                                }
                                let mut pair = [a[0] / 4, b[0] / 4];
                                pair.sort();
                                pair[0] == pair[1] || exclusions.contains(&pair)
                            },
                            |a, b| {
                                if pairs.len() < 4 {
                                    pairs.push((a, b));
                                }
                            },
                        );
                        eprintln!("first rejected crossing node pairs: {pairs:?}");
                        eprintln!(
                            "soil first rejection: inverted={inverted}, soil_ground={soil_ground}, soil_tool={soil_tool}, pose={}, rigid_fit_candidate",
                            desired
                        );
                    }
                    host.invoke("OGCConnectionSystem.Configure", Self::spec(&backup))?;
                    self.rejected_steps += 1;
                    self.blocked = true;
                    self.velocity = [0.; 3];
                    // Roll back the entire candidate, including failure history and native state.
                    // A rejected motion cannot sever roots or create detached material.
                    continue;
                }
                for i in 0..p.tool_start {
                    if p.scene.weights[i] > 0. {
                        p.scene.velocity[i] =
                            scale(sub(p.scene.p[i], previous[i]), (-3. * step).exp() / step);
                    }
                }
                // Plastic edge rest-length relaxation models limited soil yielding while every
                // element keeps its original volume/mass constraint. It is a coarse model.
                for e in &mut p.scene.edges {
                    let l = length(sub(p.scene.p[e.a], p.scene.p[e.b]));
                    let strain = (l / e.rest.max(1e-6) - 1.).abs();
                    if strain > 0.22 {
                        e.rest += (l - e.rest) * 0.015;
                    }
                }
                Self::connected(&mut p);
                for i in &p.detached {
                    let c = &p.cells[*i];
                    let h = p.scene.p[c.start..c.start + 4]
                        .iter()
                        .map(|v| v[1])
                        .sum::<f32>()
                        / 4.;
                    p.peak_lift = p.peak_lift.max(h - c.initial_height);
                }
                p.scene.time += step;
                p.pose = desired;
                self.velocity = dm(subd(d(&p.pose["position"])?, from), 1. / step as f64);
                if !equipped && self.blocked {
                    self.velocity = dm(self.velocity, 0.8);
                }
                let forces: Vec<V> =
                    serde_json::from_value(contact["forces"].clone()).unwrap_or_default();
                let mut force = [0.; 3];
                let mut torque = [0.; 3];
                let mut effort = 0.;
                let centre = self.local(d(&p.pose["position"])?);
                for i in p.tool_start..p.scene.p.len() {
                    if let Some(f) = forces.get(i) {
                        force = add(force, *f);
                        effort += length(*f) as f64;
                        torque = add(torque, cross(sub(p.scene.p[i], centre), *f));
                    }
                }
                self.force = self.world_vector(force);
                self.contact_effort = self.contact_effort.max(effort.max(candidate_effort));
                self.torque = self.world_vector(torque);
                contact["forces"] = Value::Null;
                self.peak_pairs = self.peak_pairs.max(
                    contact["vertex_face_contacts"].as_u64().unwrap_or(0)
                        + contact["edge_edge_contacts"].as_u64().unwrap_or(0),
                );
                self.contacts = contact;
                self.accepted_steps += 1;
                self.patch = Some(p);
                step_accepted = true;
                self.accumulator += (1. / 120.) * (1. - 0.5f64.powi(retry));
                break;
            }
            if !step_accepted {
                self.accumulator += 1. / 120.;
                self.force = [0.; 3];
                self.torque = [0.; 3];
                self.contacts = json!({"feedback_stale":true,"reason":"all coupled rigid/material candidates rejected"});
            }
        }
        let p = self.patch.as_ref().unwrap();
        let speed = p.scene.velocity[..p.tool_start]
            .iter()
            .zip(&p.scene.weights)
            .filter(|(_, w)| **w > 0.)
            .map(|(v, _)| length(*v))
            .fold(0f32, f32::max);
        let requested_movement = equipped
            && dd(
                subd(d(&input["pose"]["position"])?, d(&p.pose["position"])?),
                subd(d(&input["pose"]["position"])?, d(&p.pose["position"])?),
            ) > 1e-8;
        self.active = speed > 0.015
            || requested_movement
            || (!equipped && dd(self.velocity, self.velocity) > 0.0001);
        self.advance_calls += 1;
        self.advance_ms += clock.elapsed().as_secs_f64() * 1000.;
        Ok(self.response(&input["pose"]))
    }
    fn mesh(&self, exposed_only: bool) -> Vec<[f64; 6]> {
        let Some(p) = &self.patch else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut internal = BTreeSet::new();
        if exposed_only {
            for face in &p.cohesive {
                if face.bonds.iter().any(|i| !p.bonds[*i].broken) {
                    for mut key in [face.a,face.b] { key.sort(); internal.insert(key); }
                }
            }
        }
        for (i, c) in p.cells.iter().enumerate() {
            for f in &c.faces {
                let mut key=f.map(|j|c.start+j as usize); key.sort();
                if internal.contains(&key) { continue; }
                let world = f.map(|j| self.global(p.scene.p[c.start + j as usize]));
                let normal = du(dc(subd(world[1], world[0]), subd(world[2], world[0])));
                let light = 0.32 + 0.68 * dd(normal, du([0.4, 0.8, 0.2])).abs();
                let top = exposed_only && p.grass_faces.iter().any(|(face,_)| {let mut top=*face;top.sort();top==key});
                let color = if top {
                    [0.20,0.36,0.09]
                } else if p.detached.contains(&i) {
                    [0.36, 0.23, 0.12]
                } else {
                    [0.28, 0.21, 0.12]
                };
                for v in world {
                    out.push([
                        v[0],
                        v[1],
                        v[2],
                        color[0] * light,
                        color[1] * light,
                        color[2] * light,
                    ]);
                }
            }
        }
        for face in p.skin.iter().filter(|f|skin_intact(p,f)) {
            for i in face.nodes {
                let w=self.global(p.scene.p[i as usize]);
                out.push([w[0],w[1],w[2],0.20,0.36,0.09]);
            }
        }
        if !exposed_only { return out; }
        for b in p.bonds.iter().filter(|b| b.root) {
            let a = p.scene.p[b.a];
            let z = p.scene.p[b.b];
            let end = if b.broken {
                add(a, scale(sub(z, a), 0.35))
            } else {
                z
            };
            let dir = unit(sub(end, a));
            let side = scale(
                unit(cross(
                    dir,
                    if dir[0].abs() < 0.8 {
                        [1., 0., 0.]
                    } else {
                        [0., 0., 1.]
                    },
                )),
                0.002,
            );
            let up = scale(unit(cross(dir, side)), 0.002);
            for j in 0..4 {
                let off = |j: usize| {
                    add(
                        scale(side, ((j as f32) * std::f32::consts::FRAC_PI_2).cos()),
                        scale(up, ((j as f32) * std::f32::consts::FRAC_PI_2).sin()),
                    )
                };
                let q = [
                    add(a, off(j)),
                    add(a, off(j + 1)),
                    add(end, off(j + 1)),
                    add(end, off(j)),
                ];
                for f in [[0, 1, 2], [0, 2, 3]] {
                    for i in f {
                        let w = self.global(q[i]);
                        out.push([w[0], w[1], w[2], 0.19, 0.12, 0.06]);
                    }
                }
            }
        }
        out
    }
    fn grass_mesh(&self) -> Vec<[f64; 6]> {
        let Some(p) = &self.patch else { return Vec::new(); };
        p.grass_faces.iter().flat_map(|(face,interior)|grass_tuft(face.map(|i|p.scene.p[i]),p.scene.p[*interior]))
            .map(|v| {
                let p = self.global(v);
                [p[0], p[1], p[2], 0.18, 0.42, 0.09]
            })
            .collect()
    }
    fn surface_height(&self, input: &Value) -> Value {
        let Some(p) = &self.patch else {
            return json!({"height":null});
        };
        let n = du(d(&input["position"]).unwrap());
        let ray = self.local(dm(n, terrain::RADIUS));
        let mut highest = None::<f32>;
        for f in &p.scene.surfaces[0].faces {
                let [a, b, z] = f.map(|i| p.scene.p[i as usize]);
                let den = (b[2] - z[2]) * (a[0] - z[0]) + (z[0] - b[0]) * (a[2] - z[2]);
                if den.abs() < 1e-12 {
                    continue;
                }
                let u = ((b[2] - z[2]) * (ray[0] - z[0]) + (z[0] - b[0]) * (ray[2] - z[2])) / den;
                let v = ((z[2] - a[2]) * (ray[0] - z[0]) + (a[0] - z[0]) * (ray[2] - z[2])) / den;
                if u >= -1e-5 && v >= -1e-5 && u + v <= 1.00001 {
                    let h = u * a[1] + v * b[1] + (1. - u - v) * z[1];
                    highest = Some(highest.map_or(h, |old| old.max(h)));
                }
        }
        json!({"height":highest.map(|h|dd(self.global([ray[0],h,ray[2]]),n)-terrain::RADIUS)})
    }
}
fn skin_intact(p: &Patch, face: &SkinFace) -> bool {
    face.interfaces
        .iter()
        .all(|i| p.cohesive[*i].bonds.iter().any(|b| !p.bonds[*b].broken))
}
fn refresh_skin(p: &mut Patch) -> bool {
    let mut faces: Vec<_> = p
        .cells
        .iter()
        .flat_map(|c| c.faces.iter().map(move |f| f.map(|i| c.start as u32 + i)))
        .collect();
    faces.extend(p.skin.iter().filter(|f| skin_intact(p, f)).map(|f| f.nodes));
    if faces == p.scene.surfaces[0].faces {
        return false;
    }
    p.scene.surfaces[0].faces = faces;
    true
}

/// Seal the numerical pore clearance at the rooted surface. These are contact
/// triangles on the existing massive soil nodes, not a cosmetic grass overlay.
/// Interface strips and the small vertex caps form a continuous sod skin until
/// their underlying cohesive connections tear.
fn sod_skin(
    scene: &Scene,
    interfaces: &[Interface],
    original: &[usize],
    top_start: usize,
) -> Vec<SkinFace> {
    let mut out = Vec::new();
    let mut caps: BTreeMap<usize, Vec<(usize, usize, usize)>> = BTreeMap::new();
    let mut emit = |mut nodes: [u32; 3], support: Vec<usize>| {
        let q = nodes.map(|i| scene.p[i as usize]);
        let normal = cross(sub(q[1], q[0]), sub(q[2], q[0]));
        if length(normal) < 1e-10 {
            return;
        }
        if normal[1] < 0. {
            nodes.swap(1, 2);
        }
        out.push(SkinFace {
            nodes,
            interfaces: support,
        });
    };
    for (id, f) in interfaces.iter().enumerate() {
        for (j, k) in [(0, 1), (1, 2), (2, 0)] {
            if original[f.a[j]] < top_start || original[f.a[k]] < top_start {
                continue;
            }
            let (a, b, c, d) = (f.a[j], f.a[k], f.b[k], f.b[j]);
            emit([a as u32, b as u32, c as u32], vec![id]);
            emit([a as u32, c as u32, d as u32], vec![id]);
            caps.entry(original[a]).or_default().push((a, d, id));
            caps.entry(original[b]).or_default().push((b, c, id));
        }
    }
    for edges in caps.values() {
        let mut graph: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
        for &(a, b, id) in edges {
            graph.entry(a).or_default().push((b, id));
            graph.entry(b).or_default().push((a, id));
        }
        // Boundary chains terminate at the surrounding static earth. Interior
        // cycles bound the holes between three or more material elements.
        if graph.values().any(|edges| edges.len() != 2) {
            continue;
        }
        let mut visited = BTreeSet::new();
        for &start in graph.keys() {
            if visited.contains(&start) {
                continue;
            }
            let mut nodes = vec![start];
            let mut support = Vec::new();
            let (mut previous, mut current) = (usize::MAX, start);
            loop {
                visited.insert(current);
                let &(next, id) = graph[&current]
                    .iter()
                    .find(|(n, _)| *n != previous)
                    .unwrap();
                support.push(id);
                if next == start {
                    break;
                }
                if visited.contains(&next) {
                    nodes.clear();
                    break;
                }
                nodes.push(next);
                previous = current;
                current = next;
            }
            support.sort();
            support.dedup();
            for i in 1..nodes.len().saturating_sub(1) {
                emit(
                    [nodes[0] as u32, nodes[i] as u32, nodes[i + 1] as u32],
                    support.clone(),
                );
            }
        }
    }
    out
}

fn projected_faces_overlap(a: [V; 3], b: [V; 3], normal: V) -> bool {
    for triangle in [a, b] {
        for i in 0..3 {
            let axis = cross(normal, sub(triangle[(i + 1) % 3], triangle[i]));
            let range = |tri: [V; 3]| {
                let d = tri.map(|p| dot(p, axis));
                (
                    d.into_iter().fold(f32::INFINITY, f32::min),
                    d.into_iter().fold(f32::NEG_INFINITY, f32::max),
                )
            };
            let (lo, hi) = range(a);
            let (other_lo, other_hi) = range(b);
            if hi < other_lo - 1e-7 || other_hi < lo - 1e-7 {
                return false;
            }
        }
    }
    true
}
fn project_cohesive_compression(p: &mut Patch) {
    // Intact cohesive interfaces resist compression separately from fracture.
    // This preserves disjoint finite contact surfaces before they become cracks.
    for f in &p.cohesive {
        let mut normal = unit(cross(
            sub(p.scene.p[f.a[1]], p.scene.p[f.a[0]]),
            sub(p.scene.p[f.a[2]], p.scene.p[f.a[0]]),
        ));
        let center = |id: usize| {
            p.scene.p[p.cells[id].start..p.cells[id].start + 4]
                .iter()
                .copied()
                .fold([0.; 3], add)
                .map(|x| x / 4.)
        };
        if dot(normal, sub(center(f.cells[1]), center(f.cells[0]))) < 0. {
            normal = scale(normal, -1.);
        }
        if f.bonds.iter().all(|i| p.bonds[*i].broken)
            && !projected_faces_overlap(
                f.a.map(|i| p.scene.p[i]),
                f.b.map(|i| p.scene.p[i]),
                normal,
            )
        {
            continue;
        }
        for j in 0..3 {
            let (a, b) = (f.a[j], f.b[j]);
            let gap = dot(sub(p.scene.p[b], p.scene.p[a]), normal);
            let w = p.scene.weights[a] + p.scene.weights[b];
            if gap < f.gap && w > 0. {
                let move_by = scale(normal, (f.gap - gap) / w);
                p.scene.p[a] = sub(p.scene.p[a], scale(move_by, p.scene.weights[a]));
                p.scene.p[b] = add(p.scene.p[b], scale(move_by, p.scene.weights[b]));
            }
        }
    }
}
fn contact_exclusions(_p: &Patch) -> Vec<[usize; 2]> {
    // Finite pore clearance makes every distinct tetrahedral surface collidable,
    // including intact cohesive neighbours. Cohesion acts before OGC projection;
    // no later material correction may invalidate its accepted contact state.
    Vec::new()
}
fn solve_bonds(p: &mut Patch, dt: f32) {
    let frames: Vec<_> = p.cohesive.iter().map(|f| {
        let face=std::array::from_fn(|j|scale(add(p.scene.p[f.a[j]],p.scene.p[f.b[j]]),0.5));
        let mut frame=super::soil_material::face_frame(face);
        frame[1]=scale(frame[1],f.normal_sign);
        frame[2]=scale(frame[2],f.normal_sign);
        frame
    }).collect();
    for b in &mut p.bonds {
        if b.broken {
            continue;
        }
        let delta = sub(p.scene.p[b.b], p.scene.p[b.a]);
        let difference = if b.root {
            let l = length(delta);
            let rest = length(b.rest);
            if l <= rest {
                // Roots only carry tension. A root can go slack between solver
                // iterations after contact moves its endpoints; retaining the
                // previous multiplier would apply stale tension if it tightens
                // again within this same substep.
                b.lambda = [0.; 3];
                continue;
            }
            scale(delta, (l - rest) / l.max(1e-8))
        } else {
            sub(delta, super::soil_material::transport(b.rest,frames[b.interface.unwrap()]))
        };
        let extension = length(difference);
        let alpha = b.compliance / (dt * dt);
        let denom = p.scene.weights[b.a] + p.scene.weights[b.b] + alpha;
        if denom <= 0. {
            continue;
        }
        let dl = scale(
            add(scale(difference, -1.), scale(b.lambda, -alpha)),
            1. / denom,
        );
        let next = add(b.lambda, dl);
        let load = length(next) / (dt * dt);
        b.peak_load = b.peak_load.max(load);
        let fails = if b.root {
            extension > b.extension || (load > b.limit && extension > 0.00005)
        } else {
            super::soil_material::Cohesion::LOAM.fails(difference,scale(next,1./(dt*dt)),frames[b.interface.unwrap()][2],b.area)
        };
        if fails {
            b.broken = true;
            continue;
        }
        b.lambda = next;
        p.scene.p[b.a] = sub(p.scene.p[b.a], scale(dl, p.scene.weights[b.a]));
        p.scene.p[b.b] = add(p.scene.p[b.b], scale(dl, p.scene.weights[b.b]));
    }
}
fn propagate_faces(p: &mut Patch) {
    let cracked: BTreeSet<_> = p
        .bonds
        .iter()
        .filter(|b| !b.root && b.broken)
        .map(|b| b.cells)
        .collect();
    for b in &mut p.bonds {
        if !b.root && cracked.contains(&b.cells) {
            b.broken = true;
        }
    }
}
fn orthogonalize(p: &mut Value) -> Result<(), String> {
    let r = du(d(&p["right"])?);
    let u = d(&p["up"])?;
    let u = du(subd(u, dm(r, dd(u, r))));
    p["right"] = json!(r);
    p["up"] = json!(u);
    p["forward"] = json!(du(dc(r, u)));
    Ok(())
}
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let mut s = std::mem::take(&mut host.terrain_tool);
    let result = (|| match hook {
        "Aim" if s.volume.is_some() => s.volume.as_ref().unwrap().aim(host,input),
        "Aim" => {
            let p = d(&input["position"])?;
            let ray = du(d(&input["forward"])?);
            let mut previous = 0.;
            for i in 0..=96 {
                let t = i as f64 * 3. / 96.;
                let q = addd(p, dm(ray, t));
                let static_h = host.invoke("PlanetSurfaceSystem.Height", json!({"position":q}))?["height"]
                    .as_f64()
                    .unwrap();
                let h = s.surface_height(&json!({"position":q}))["height"].as_f64().map_or(static_h, |h| h.max(static_h));
                if dd(q, q).sqrt() - terrain::RADIUS <= h {
                    let (mut lo, mut hi) = (previous, t);
                    for _ in 0..10 {
                        let mid = (lo + hi) * 0.5;
                        let q = addd(p, dm(ray, mid));
                        let static_h=host.invoke("PlanetSurfaceSystem.Height",json!({"position":q}))?["height"].as_f64().unwrap();
                        let h=s.surface_height(&json!({"position":q}))["height"].as_f64().map_or(static_h,|h|h.max(static_h));
                        if dd(q, q).sqrt() - terrain::RADIUS > h {
                            lo = mid
                        } else {
                            hi = mid
                        }
                    }
                    let q = addd(p, dm(ray, (lo + hi) * 0.5));
                    let material = s.surface_height(&json!({"position":q}))["height"].as_f64()
                        .is_some_and(|h| (dd(q,q).sqrt()-terrain::RADIUS-h).abs()<0.02);
                    return Ok(json!({"hit":true,"position":q,"normal":du(q),"physical_soil":material}));
                }
                previous = t;
            }
            Ok(json!({"hit":false}))
        }
        "Cut" => Ok(
            json!({"changed":false,"reason":"gesture-driven geometric cutting is retired; maneuver the spade through the physical rooted soil"}),
        ),
        "Advance" | "Prepare" => {
            if s.soil_enabled.is_none() || s.cutting_selected.is_none() {
                let config: Value = serde_json::from_slice(
                    &std::fs::read(host.root().join("Procedures/soil_patch.json"))
                        .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                if s.soil_enabled.is_none() {s.soil_enabled = Some(
                    config["enabled"]
                        .as_bool()
                        .ok_or("soil_patch.enabled must be boolean")?,
                );}
                s.cutting_selected=Some(config["material_model"]=="polyhedral_cutting");
            }
            if hook == "Prepare" {
                if s.soil_enabled != Some(true) {
                    if input["optional"] == true { return Ok(s.tool_controls.response(&input["pose"])); }
                    return Err("rooted soil preparation is disabled by its procedure".into());
                }
                if s.patch.is_none() {
                    s.tool_controls.release(host)?;
                    if s.cutting_selected==Some(true) {
                        if s.volume.is_none(){s.volume=Some(super::soil_volume::State::prepare(host,input)?);}
                        return Ok(s.volume.as_ref().unwrap().response(&input["pose"]));
                    }else{s.initialize(host, input)?;}
                }
                return Ok(s.response(&input["pose"]));
            }
            if s.soil_enabled == Some(false) {
                s.tool_controls.advance(host, input)
            } else if let Some(volume)=s.volume.as_mut() {
                volume.advance(host,input)
            } else {
                s.advance(host, input)
            }
        }
        "SurfaceHeight" if s.volume.is_some()=>Ok(json!({"height":s.volume.as_ref().unwrap().height(d(&input["position"])?) })),
        "SurfaceHeight" => Ok(s.surface_height(input)),

        "Frame" if s.volume.is_some()=>Ok(s.volume.as_ref().unwrap().frame_value()),

        "Frame" if s.soil_enabled == Some(false) || s.patch.is_none() => {
            Ok(s.tool_controls.frame())
        }
        "Frame" => Ok(
            json!({"tetrahedra":s.solid_tetrahedra(),"debris":s.mesh(false),"visual":s.mesh(true),"grass":s.grass_mesh(),"active":s.active,"removed_volume":s.patch.as_ref().map_or(0.,|p|p.detached_volume),"removed_mass":s.patch.as_ref().map_or(0.,|p|p.detached_volume*1600.),"failure":s.failure,"blocked":s.blocked,"material":s.material(),"contact":s.contacts,"peak_contact_pairs":s.peak_pairs,"accepted_steps":s.accepted_steps,"rejected_steps":s.rejected_steps,"timing":{"advance_calls":s.advance_calls,"advance_ms":s.advance_ms,"native_contact_ms":s.native_ms},"minimum_rigid_bound":s.minimum_rigid_bound,"near_zero_rigid_vertices":s.near_zero_rigid_vertices,"grasp_feasible":s.grasp_feasible,"requested_seconds":s.requested_seconds,"budget_discarded_seconds":s.budget_discarded_seconds,"maximum_substeps_per_frame":2,"simulated_seconds":s.patch.as_ref().map_or(0.,|p|p.scene.time),"rest_volume":s.patch.as_ref().map_or(0.,|p|p.scene.tets.iter().map(|t|t.rest.abs()as f64).sum::<f64>())}),
        ),
        "Release" => {
            s.tool_controls.release(host)?;
            if s.patch.is_some() || s.volume.is_some() {
                host.invoke("OGCConnectionSystem.Release", Value::Null)?;
            }
            s = State::default();
            Ok(Value::Null)
        }
        _ => Err("unknown terrain tool operation".into()),
    })();
    host.terrain_tool = s;
    result
}
