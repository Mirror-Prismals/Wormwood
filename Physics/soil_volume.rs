//! Playable adapter for material-preserving polyhedral cutting. The material
//! owns its topology; the tool supplies a physical edge and bounded actuation.
use super::{
    Scene,
    affine_soil::{self, Body},
    cut_geometry::*,
    math::V,
    soil_cutting::Material,
};
use crate::{
    host::Host,
    terrain::{self, tool_cut},
};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;
fn vector(v: &Value) -> Result<D, String> {
    serde_json::from_value(v.clone()).map_err(|e| e.to_string())
}
fn pose_point(pose: &Value, p: V) -> D {
    add(
        vector(&pose["position"]).unwrap(),
        ["right", "up", "forward"]
            .iter()
            .enumerate()
            .fold([0.; 3], |a, (i, k)| {
                add(a, mul(vector(&pose[*k]).unwrap(), p[i] as f64))
            }),
    )
}
fn fracture_tip(
    edge: [D; 2],
    accepted: [D; 2],
    wanted: [D; 2],
    inward: D,
    contact_force: D,
    equipped: bool,
    work: f64,
    effort: f64,
) -> Option<[D; 2]> {
    let travel = mul(
        add(sub(accepted[0], edge[0]), sub(accepted[1], edge[1])),
        0.5,
    );
    let requested = mul(add(sub(wanted[0], edge[0]), sub(wanted[1], edge[1])), 0.5);
    let tangent = unit(sub(accepted[1], accepted[0]));
    let direction = unit(sub(inward, mul(tangent, dot(inward, tangent))));
    // The small crack process zone follows the loaded sharp edge into the
    // material. Normalizing its accepted drift instead turns microscopic
    // side-slip into a sideways crack and stalls insertion at the surface.
    // A desired pose only selects whether the edge is pressing inward; it
    // supplies neither the swept distance nor the energy for this fracture.
    (equipped
        && work > 1e-9
        && effort > 2.
        && length(travel) > 1e-7
        && length(direction) > 0.9
        && dot(requested, direction) > 0.
        && dot(contact_force, direction) < 0.)
        .then(|| accepted.map(|p| add(p, mul(direction, 0.0015))))
}
pub(super) fn sample_snapshot(snapshot: &Value, material: D) -> Option<D> {
    let reference: Vec<D> = serde_json::from_value(snapshot["reference_vertices"].clone()).ok()?;
    let positions: Vec<D> = serde_json::from_value(snapshot["vertices"].clone()).ok()?;
    let faces: Vec<Vec<usize>> = serde_json::from_value(snapshot["faces"].clone()).ok()?;
    if positions.len() != reference.len() {
        return None;
    }
    let poly = Poly {
        vertices: reference
            .into_iter()
            .zip(positions)
            .map(|(material, position)| super::cut_geometry::Vertex {
                material,
                position,
                velocity: [0.; 3],
            })
            .collect(),
        faces: faces
            .into_iter()
            .map(|nodes| super::cut_geometry::Face { nodes, tag: 0 })
            .collect(),
    };
    let coordinates = poly.coordinates(material, true)?;
    Some(poly.sample(&coordinates, false))
}
#[derive(Clone)]
struct Frame {
    anchor: D,
    basis: [D; 3],
}
impl Frame {
    fn local(&self, p: D) -> D {
        let p = sub(p, self.anchor);
        self.basis.map(|a| dot(p, a))
    }
    fn global(&self, p: D) -> D {
        add(self.anchor, affine_soil::apply(self.basis, p))
    }
    fn world_vector(&self, p: D) -> D {
        affine_soil::apply(self.basis, p)
    }
}
#[derive(Clone)]
pub(super) struct State {
    frame: Frame,
    material: Material,
    clipped_contact_mesh: bool,
    cut_mesh_cache: RefCell<Option<(u64, Vec<(usize, D)>, Vec<[usize; 3]>)>>,
    ground: Vec<V>,
    ground_faces: Vec<[u32; 3]>,
    tool: Vec<V>,
    tool_faces: Vec<[u32; 3]>,
    tool_body: super::rigid_contact::Body,
    edge: [V; 2],
    edge_indices: Vec<usize>,
    kerf: f64,
    pose: Value,
    tool_velocity: D,
    active: bool,
    blocked: bool,
    failure: Option<String>,
    grasp_feasible: bool,
    force: D,
    torque: D,
    effort: f64,
    edge_effort: f64,
    contacts: Value,
    accepted: u64,
    rejected: u64,
    time: f64,
    requested: f64,
    discarded: f64,
    accumulator: f64,
    advance_calls: u64,
    advance_ms: f64,
    native_ms: f64,
    peak_pairs: u64,
    min_bound: f64,
    cut_capacity: bool,
}
impl State {
    pub fn prepare(host: &mut Host, input: &Value) -> Result<Self, String> {
        let config: Value = serde_json::from_slice(
            &std::fs::read(host.root().join("Procedures/soil_patch.json"))
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let mesh = config["internal_contact_mesh"]
            .as_str()
            .unwrap_or("clipped");
        if !matches!(mesh, "full_face" | "clipped") {
            return Err("soil_patch.internal_contact_mesh must be full_face or clipped".into());
        }
        // Refinement faces remain internal until their actual damaged area is
        // exposed. Promoting one small crack to a whole collision polygon
        // makes untouched material ahead of the edge obstruct the tool.
        // Keep full_face only as an explicitly selected diagnostic comparison.
        let clipped_contact_mesh = mesh == "clipped";
        #[cfg(test)]
        let clipped_contact_mesh = std::env::var_os("MIRROR_EXPERIMENTAL_CLIPPED_CONTACT")
            .is_some()
            || clipped_contact_mesh;
        let tool: Vec<V> = serde_json::from_value(input["blade"]["vertices"].clone())
            .map_err(|e| e.to_string())?;
        let tool_faces = serde_json::from_value(input["blade"]["triangles"].clone())
            .map_err(|e| e.to_string())?;
        let tool_body = super::rigid_contact::Body::from_value(&input["blade"]["rigid_body"])?;
        let mean = tool
            .iter()
            .fold([0.; 3], |a, p| add(a, p.map(|x| x as f64)))
            .map(|x| x / tool.len() as f64);
        let site = if input["patch_position"].is_null() {
            pose_point(&input["pose"], mean.map(|x| x as f32))
        } else {
            vector(&input["patch_position"])?
        };
        let normal = unit(site);
        let right = vector(&input["pose"]["right"])?;
        let right = unit(sub(right, mul(normal, dot(normal, right))));
        let height =
            host.invoke("PlanetSurfaceSystem.Height", json!({"position":normal}))?["height"]
                .as_f64()
                .unwrap();
        let frame = Frame {
            anchor: mul(normal, terrain::RADIUS + height),
            basis: [right, normal, unit(cross(right, normal))],
        };
        let material = Material::new([-0.26, -0.24, -0.36], [0.26, 0., 0.36]);
        let outline = [[-0.26, -0.36], [0.26, -0.36], [0.26, 0.36], [-0.26, 0.36]];
        let mut top = Vec::new();
        for p in outline {
            top.push(
                host.invoke(
                    "PlanetSurfaceSystem.Height",
                    json!({"position":unit(frame.global([p[0],0.,p[1]]))}),
                )?["height"]
                    .as_f64()
                    .unwrap(),
            );
        }
        let bottom = height - 0.255;
        let edit = json!({"normal":normal,"radius":0.6,"depth":0.24,"blade":{"right":right,"forward":frame.basis[2],"outline":outline,"bottom":bottom,"top":top}});
        let surface = host.invoke("PlanetSurfaceSystem.Frame", Value::Null)?;
        let mut edits = surface["edits"].as_array().unwrap().clone();
        edits.push(edit.clone());
        let mut tris = Vec::new();
        let mut points = Vec::new();
        let size = 6;
        for z in 0..=size {
            for x in 0..=size {
                let n = unit(frame.global([(x as f64 - 3.) * 0.5, 0., (z as f64 - 3.) * 0.5]));
                let h = tool_cut::height(n, terrain::height(n), &edits, false);
                let p = mul(n, terrain::RADIUS + h);
                points.push([p[0], p[1], p[2], 0.3, 0.2, 0.1]);
            }
        }
        for z in 0..size {
            for x in 0..size {
                let a = z * (size + 1) + x;
                tris.extend([
                    [points[a], points[a + size + 1], points[a + 1]],
                    [points[a + 1], points[a + size + 1], points[a + size + 2]],
                ]);
            }
        }
        let mut ground = Vec::new();
        let mut ground_faces = Vec::new();
        let mut weld = BTreeMap::new();
        for tri in tool_cut::mesh(tris, &edits) {
            let p = tri.map(|v| frame.local([v[0], v[1], v[2]]));
            let n = unit(cross(sub(p[1], p[0]), sub(p[2], p[0])));
            // The active region is joined to far-field earth by exact boundary
            // conditions. A duplicate static wall at that same interface would
            // falsely clamp every new cut vertex through a zero contact bound.
            if n[1].abs() < 0.05
                && p.iter()
                    .all(|v| v[0].abs() <= 0.26001 && v[2].abs() <= 0.36001)
            {
                continue;
            }
            let mut f = [0; 3];
            for i in 0..3 {
                let key = p[i].map(|x| (x * 1e6).round() as i64);
                f[i] = *weld.entry(key).or_insert_with(|| {
                    let id = ground.len() as u32;
                    ground.push(p[i].map(|x| x as f32));
                    id
                });
            }
            ground_faces.push(f);
        }
        let minimum = tool.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min);
        let edge_indices: Vec<_> = tool
            .iter()
            .enumerate()
            .filter(|(_, p)| (p[1] - minimum).abs() < 1e-6)
            .map(|(i, _)| i)
            .collect();
        if edge_indices.len() < 2 {
            return Err("physical spade requires a finite cutting edge".into());
        }
        let x0 = edge_indices
            .iter()
            .map(|i| tool[*i][0])
            .fold(f32::INFINITY, f32::min);
        let x1 = edge_indices
            .iter()
            .map(|i| tool[*i][0])
            .fold(f32::NEG_INFINITY, f32::max);
        let edge = [[x0, minimum, 0.], [x1, minimum, 0.]];
        let kerf = (edge_indices
            .iter()
            .map(|i| tool[*i][2])
            .fold(f32::NEG_INFINITY, f32::max)
            - edge_indices
                .iter()
                .map(|i| tool[*i][2])
                .fold(f32::INFINITY, f32::min)) as f64;
        let out = Self {
            frame,
            material,
            clipped_contact_mesh,
            cut_mesh_cache: RefCell::new(None),
            ground,
            ground_faces,
            tool,
            tool_faces,
            tool_body,
            edge,
            edge_indices,
            kerf,
            pose: input["pose"].clone(),
            tool_velocity: [0.; 3],
            active: false,
            blocked: false,
            failure: None,
            grasp_feasible: true,
            force: [0.; 3],
            torque: [0.; 3],
            effort: 0.,
            edge_effort: 0.,
            contacts: Value::Null,
            accepted: 0,
            rejected: 0,
            time: 0.,
            requested: 0.,
            discarded: 0.,
            accumulator: 0.,
            advance_calls: 0,
            advance_ms: 0.,
            native_ms: 0.,
            peak_pairs: 0,
            min_bound: 0.,
            cut_capacity: false,
        };
        host.invoke("OGCConnectionSystem.Configure", out.spec())?;
        if out.crossing_count() > 0 {
            host.invoke("OGCConnectionSystem.Release", Value::Null)?;
            return Err("cuttable soil preparation intersects the physical spade".into());
        }
        if let Err(e) = host.invoke("PlanetSurfaceSystem.RefineMaterial", json!({"edit":edit})) {
            host.invoke("OGCConnectionSystem.Release", Value::Null)?;
            return Err(e);
        }
        Ok(out)
    }
    fn layout(&self) -> (Vec<usize>, usize, usize) {
        let mut starts = Vec::new();
        let mut n = 0;
        for b in &self.material.bodies {
            starts.push(n);
            n += b.poly.vertices.len();
        }
        n += self.cut_samples().len();
        (starts, n, n + self.ground.len())
    }
    // Extra OGC vertices are attached to their body's reference coordinates;
    // they represent the damaged portion of internal faces, not new matter.
    fn cut_mesh_for(material: &Material) -> (Vec<(usize, D)>, Vec<[usize; 3]>) {
        let mut samples = Vec::new();
        let mut triangles = Vec::new();
        let mut vertices = BTreeMap::new();
        for (i, body) in material.bodies.iter().enumerate() {
            for face in &body.poly.faces {
                if face.tag < 100 {
                    continue;
                }
                let p = face
                    .nodes
                    .iter()
                    .take(3)
                    .map(|n| body.poly.vertices[*n].material)
                    .collect::<Vec<_>>();
                let outward = cross(sub(p[1], p[0]), sub(p[2], p[0]));
                for mut triangle in material.exposed_face_material(body, face) {
                    if dot(
                        cross(sub(triangle[1], triangle[0]), sub(triangle[2], triangle[0])),
                        outward,
                    ) < 0.
                    {
                        triangle.swap(1, 2);
                    }
                    // Weld the same material point within one body. Without
                    // this, adjacent triangles apply the same point contact
                    // repeatedly and make the OGC mesh unnecessarily large.
                    let keys = triangle.map(|p| (i, p.map(|x| (x * 1e9).round() as i64)));
                    if keys[0] == keys[1] || keys[1] == keys[2] || keys[2] == keys[0] {
                        continue;
                    }
                    let indices = std::array::from_fn(|j| {
                        *vertices.entry(keys[j]).or_insert_with(|| {
                            let index = samples.len();
                            samples.push((i, triangle[j]));
                            index
                        })
                    });
                    triangles.push(indices);
                }
            }
        }
        (samples, triangles)
    }
    fn cut_mesh(&self) -> (Vec<(usize, D)>, Vec<[usize; 3]>) {
        if !self.clipped_contact_mesh {
            return (Vec::new(), Vec::new());
        }
        let mut cache = self.cut_mesh_cache.borrow_mut();
        if cache
            .as_ref()
            .is_none_or(|(revision, _, _)| *revision != self.material.revision)
        {
            let (samples, faces) = Self::cut_mesh_for(&self.material);
            *cache = Some((self.material.revision, samples, faces));
        }
        let (_, samples, faces) = cache.as_ref().unwrap();
        (samples.clone(), faces.clone())
    }
    fn cut_samples(&self) -> Vec<(usize, D)> {
        self.cut_mesh().0
    }
    fn positions(&self) -> Vec<V> {
        self.positions_for(&self.cut_samples())
    }
    fn positions_for(&self, samples: &[(usize, D)]) -> Vec<V> {
        self.material
            .bodies
            .iter()
            .flat_map(|b| b.poly.vertices.iter().map(|v| v.position.map(|x| x as f32)))
            .chain(
                samples
                    .iter()
                    .map(|(i, p)| self.material.bodies[*i].point(*p).map(|x| x as f32)),
            )
            .chain(self.ground.iter().copied())
            .chain(self.tool.iter().map(|v| {
                self.frame
                    .local(pose_point(&self.pose, *v))
                    .map(|x| x as f32)
            }))
            .collect()
    }
    fn scene(&self) -> Scene {
        let (starts, soil_end, _) = self.layout();
        let mut s = Scene::new("cuttable_soil_volume");
        s.radius = 0.0015;
        for b in &self.material.bodies {
            for v in &b.poly.vertices {
                s.node(v.position.map(|x| x as f32), 0., [0.3, 0.2, 0.1]);
            }
        }
        for (i, p) in self.cut_samples() {
            s.node(
                self.material.bodies[i].point(p).map(|x| x as f32),
                0.,
                [0.3, 0.2, 0.1],
            );
        }
        let faces = self
            .material
            .bodies
            .iter()
            .enumerate()
            .flat_map(|(i, b)| {
                let start = starts[i];
                b.poly
                    .triangles()
                    .map(move |f| f.map(|j| (start + j) as u32))
            })
            .collect();
        s.surface(0, faces, false, true);
        for p in &self.ground {
            s.node(*p, 0., [0.3, 0.2, 0.1]);
        }
        s.surface(soil_end, self.ground_faces.clone(), true, false);
        let tool_start = s.p.len();
        for p in &self.tool {
            s.node(
                self.frame
                    .local(pose_point(&self.pose, *p))
                    .map(|x| x as f32),
                0.,
                [0.4; 3],
            );
        }
        s.surface(tool_start, self.tool_faces.clone(), false, false);
        s
    }
    fn spec(&self) -> Value {
        let mut v = self.scene().spec();
        v["contact_stiffness"] = json!(80000.);
        let (starts, soil_end, _) = self.layout();
        let mut active = vec![false; soil_end];
        let mut faces = Vec::<[u32; 3]>::new();
        for (i, body) in self.material.bodies.iter().enumerate() {
            for face in &body.poly.faces {
                if face.tag >= 100
                    && (self.clipped_contact_mesh
                        || self.material.exposed_face(body, face).is_empty())
                {
                    continue;
                }
                for k in 1..face.nodes.len() - 1 {
                    let triangle = [face.nodes[0], face.nodes[k], face.nodes[k + 1]]
                        .map(|n| (starts[i] + n) as u32);
                    for n in triangle {
                        active[n as usize] = true;
                    }
                    faces.push(triangle);
                }
            }
        }
        let physical_end: usize = self
            .material
            .bodies
            .iter()
            .map(|b| b.poly.vertices.len())
            .sum();
        let (samples, cut_faces) = self.cut_mesh();
        for cut_face in cut_faces {
            let triangle = cut_face.map(|n| (physical_end + n) as u32);
            for n in triangle {
                active[n as usize] = true;
            }
            faces.push(triangle);
        }
        v["surfaces"][0]["triangles"] = json!(faces);
        v["surfaces"][0]["active_vertices"] = json!(active);
        v["surfaces"][0]["element_ids"] = json!(
            self.material
                .bodies
                .iter()
                .enumerate()
                .flat_map(|(i, b)| vec![i as u32; b.poly.vertices.len()])
                .chain(samples.iter().map(|(i, _)| *i as u32))
                .collect::<Vec<_>>()
        );
        v["surfaces"][0]["excluded_pairs"] = json!(self.material.exclusions());
        for surface in v["surfaces"].as_array_mut().unwrap() {
            surface["friction"] = json!(0.65);
        }
        v
    }
    fn material_value(&self) -> Value {
        let detached_mass = self
            .material
            .bodies
            .iter()
            .filter(|b| self.material.detached.contains(&b.id))
            .map(|b| b.mass)
            .sum::<f64>();
        let parcels:Vec<_>=self.material.bodies.iter().map(|b|json!({"id":b.id,"ancestors":b.ancestors,"position":self.frame.global(b.center),"reference_position":b.reference_center,"mass":b.mass,"detached":self.material.detached.contains(&b.id),
            "reference_vertices":b.poly.vertices.iter().map(|v|v.material).collect::<Vec<_>>(),"vertices":b.poly.vertices.iter().map(|v|self.frame.global(v.position)).collect::<Vec<_>>(),"faces":b.poly.faces.iter().map(|f|f.nodes.clone()).collect::<Vec<_>>()})).collect();
        json!({"initialized":true,"changed":detached_mass>0.,"model":"material_preserving_polyhedral_cutting_with_affine_elasticity","solid_volume":self.material.rest_volume,"mass":self.material.mass,"detached_volume":detached_mass/1600.,"detached_mass":detached_mass,"detached_elements":self.material.detached.len(),"elements":self.material.bodies.len(),"cohesion_broken":self.material.stats.cohesion_broken,
            "roots":12,"roots_broken":self.material.stats.roots_broken,"peak_root_load":self.material.stats.peak_root_load,"peak_lift":self.material.stats.peak_lift,"patch_center":self.frame.global([0.;3]),"bounded_patch":true,
            "skin_triangles":self.material.bodies.iter().flat_map(|b|b.poly.faces.iter()).filter(|f|f.tag==2).map(|f|f.nodes.len().saturating_sub(2)).sum::<usize>(),"skin_torn_triangles":self.material.stats.cohesion_broken,"parcels":parcels,
            "cuts":self.material.stats.cuts,"refinements":self.material.stats.refinements,"cut_area":self.material.stats.cut_area,"fracture_work":self.material.stats.fracture_work,"contact_work_available":self.material.work_reservoir,"cut_capacity_reached":self.cut_capacity})
    }
    pub fn response(&self, requested: &Value) -> Value {
        json!({"requested_pose":requested,"pose":self.pose,"active":self.active,"blocked":self.blocked,"failure":self.failure,"grasp_feasible":self.grasp_feasible,"force":self.force,"torque":self.torque,"tool_contact_effort":self.effort,"contact":self.contacts,"material":self.material_value()})
    }
    fn grasp(&self, input: &Value) -> Result<Option<super::rigid_contact::Grasp>, String> {
        if input["equipped"] == false || !input["grasp"].is_object() {
            return Ok(None);
        }
        let g = &input["grasp"];
        let make = |i: usize| -> Result<super::rigid_contact::Grip, String> {
            Ok(super::rigid_contact::Grip {
                point: vector(&g["points"][i])?,
                shoulder: self.frame.local(vector(&g["shoulders"][i])?),
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
        Ok(Some(super::rigid_contact::Grasp {
            grips: [make(0)?, make(1)?],
        }))
    }
    fn crossing_count(&self) -> usize {
        let scene = self.scene();
        let (starts, soil_end, _) = self.layout();
        let element = |node: usize| starts.partition_point(|s| *s <= node).saturating_sub(1);
        let allowed = self.material.touch_matrix();
        super::intersections::count_filtered(&scene, |a, b| {
            if a[0] < soil_end && b[0] < soil_end {
                allowed[element(a[0])][element(b[0])]
            } else {
                false
            }
        })
    }
    fn edge_near_soil(&self, edge: [D; 2]) -> bool {
        // An accepted edge must be at the current material boundary. A held
        // button elsewhere cannot partition or fracture a remote region.
        (0..=8).any(|i| {
            let p = add(edge[0], mul(sub(edge[1], edge[0]), i as f64 / 8.));
            self.material.bodies.iter().any(|b| {
                b.poly.faces.iter().any(|f| {
                    let a = b.poly.vertices[f.nodes[0]].position;
                    let n = unit(cross(
                        sub(b.poly.vertices[f.nodes[1]].position, a),
                        sub(b.poly.vertices[f.nodes[2]].position, a),
                    ));
                    if dot(sub(p, a), n).abs() > 0.008 {
                        return false;
                    }
                    let projected = sub(p, mul(n, dot(sub(p, a), n)));
                    f.nodes.iter().enumerate().all(|(j, k)| {
                        let x = b.poly.vertices[*k].position;
                        let y = b.poly.vertices[f.nodes[(j + 1) % f.nodes.len()]].position;
                        dot(cross(sub(y, x), sub(projected, x)), n) >= -0.002 * length(sub(y, x))
                    })
                })
            })
        })
    }
    fn substep(&mut self, host: &mut Host, input: &Value, dt: f64) -> Result<bool, String> {
        let before = self.clone();
        match self.substep_candidate(host, input, dt, &before) {
            Ok(accepted) => Ok(accepted),
            Err(error) => {
                // A failed native operation must not leave newly fractured
                // material paired with an older native contact mesh.
                *self = before;
                self.rejected += 1;
                self.blocked = true;
                host.invoke("OGCConnectionSystem.Configure", self.spec())?;
                self.failure = Some(error);
                self.contacts = json!({"feedback_stale":true,"reason":"soil contact failed; previous material restored"});
                self.force = [0.; 3];
                self.torque = [0.; 3];
                Ok(false)
            }
        }
    }
    fn substep_candidate(
        &mut self,
        host: &mut Host,
        input: &Value,
        dt: f64,
        before: &Self,
    ) -> Result<bool, String> {
        let equipped = input["equipped"] != false;
        if equipped
            && input["grasp"].is_null()
            && self.material.bodies.iter().all(Body::fully_fixed)
            && ["position", "right", "up", "forward"]
                .iter()
                .all(|k| vector(&input["pose"][*k]).ok() == vector(&self.pose[*k]).ok())
            && self
                .tool
                .iter()
                .all(|p| self.frame.local(pose_point(&self.pose, *p))[1] > 0.02)
        {
            self.accepted += 1;
            self.time += dt;
            return Ok(true);
        }
        let mut wanted = if equipped {
            input["pose"].clone()
        } else {
            self.pose.clone()
        };
        if !equipped {
            self.tool_velocity = add(self.tool_velocity, mul(self.frame.basis[1], -9.81 * dt));
            wanted["position"] = json!(add(
                vector(&self.pose["position"])?,
                mul(self.tool_velocity, dt)
            ));
        }
        wanted = super::rigid_motion::approach(
            &self.pose,
            &wanted,
            if input["control_point"].is_null() {
                [0.; 3]
            } else {
                vector(&input["control_point"])?
            },
            dt,
        )?;
        let edge = self
            .edge
            .map(|p| self.frame.local(pose_point(&self.pose, p)));
        let target_edge = self.edge.map(|p| self.frame.local(pose_point(&wanted, p)));
        if equipped && self.edge_near_soil(edge) {
            let revision = self.material.revision;
            self.material
                .cut(edge, target_edge, self.edge_effort, self.kerf);
            if self.material.revision != revision {
                host.invoke("OGCConnectionSystem.Configure", self.spec())?;
            }
            self.cut_capacity = self.material.bodies.len() >= 128;
        }
        let prior = self.material.clone();
        let previous_positions = self.positions();
        let grasp = self.grasp(input)?;
        let target: Vec<_> = self
            .tool
            .iter()
            .map(|p| self.frame.local(pose_point(&wanted, *p)).map(|x| x as f32))
            .collect();
        let (starts, soil_end, tool_start) = self.layout();
        let physical_end: usize = self
            .material
            .bodies
            .iter()
            .map(|b| b.poly.vertices.len())
            .sum();
        let cut_samples = self.cut_samples();
        let mut last_responses = Vec::<[f32; 13]>::new();
        let mut last_contact = Value::Null;
        let mut contact_mesh_dirty = false;
        for _ in 0..3 {
            let positions = self.positions_for(&cut_samples);
            let mut response=host.invoke("OGCConnectionSystem.Project",json!({"positions":positions,"previous":previous_positions,"inverse_mass":vec![0.;positions.len()],"dt":dt,"excluded_pairs":self.material.exclusions(),"all_responses":true,"rigid_response_start":tool_start}))?;
            let all: Vec<[f32; 13]> = serde_json::from_value(response["all_responses"].take())
                .map_err(|e| e.to_string())?;
            let rigid: Vec<[f32; 13]> = serde_json::from_value(response["rigid_responses"].take())
                .map_err(|e| e.to_string())?;
            self.native_ms += response["broad_ms"].as_f64().unwrap_or(0.)
                + response["narrow_ms"].as_f64().unwrap_or(0.);
            self.peak_pairs = self.peak_pairs.max(
                response["vertex_face_contacts"].as_u64().unwrap_or(0)
                    + response["edge_edge_contacts"].as_u64().unwrap_or(0),
            );
            let mut systems: Vec<_> = self
                .material
                .bodies
                .iter()
                .zip(&prior.bodies)
                .map(|(b, p)| b.system(p, dt))
                .collect();
            let revision = self.material.revision;
            self.material.add_material_forces(&mut systems);
            if revision != self.material.revision {
                // Keep this substep's vertex layout stable. New damage is
                // exposed to OGC after the conservative old mesh is solved.
                contact_mesh_dirty = true;
            }
            for (i, b) in self.material.bodies.iter().enumerate() {
                for (j, v) in b.poly.vertices.iter().enumerate() {
                    let r = all[starts[i] + j];
                    let force = [r[0] as f64, r[1] as f64, r[2] as f64];
                    let h = std::array::from_fn(|c| {
                        std::array::from_fn(|row| r[3 + c * 3 + row] as f64)
                    });
                    let (a, rhs) = &mut systems[i];
                    Body::add_force(a, rhs, sub(v.material, b.reference_center), force, h);
                }
            }
            for (j, (i, material)) in cut_samples.iter().enumerate() {
                let r = all[physical_end + j];
                let force = [r[0] as f64, r[1] as f64, r[2] as f64];
                let h =
                    std::array::from_fn(|c| std::array::from_fn(|row| r[3 + c * 3 + row] as f64));
                let (a, rhs) = &mut systems[*i];
                Body::add_force(
                    a,
                    rhs,
                    sub(*material, self.material.bodies[*i].reference_center),
                    force,
                    h,
                );
            }
            #[cfg(test)]
            if self.material.bodies.len() > 1 {
                if let Ok(path) = std::env::var("MIRROR_CUT_SOLVE_CAPTURE") {
                    if !std::path::Path::new(&path).exists() {
                        std::fs::write(&path,serde_json::to_vec(&json!({"material":self.material.diagnostic(),"responses":all,"native":positions,"starts":starts,"time":self.time,"pose":self.pose,"systems":systems.iter().map(|(a,r)|json!({"matrix":a.iter().map(|r|r.to_vec()).collect::<Vec<_>>(),"rhs":r})).collect::<Vec<_>>()})).unwrap()).unwrap();
                    }
                }
            }
            let planes = self.material.contact_planes();
            let native = positions[..physical_end]
                .iter()
                .map(|p| p.map(|x| x as f64))
                .collect::<Vec<_>>();
            let extra = cut_samples
                .iter()
                .enumerate()
                .map(|(j, (body, material))| affine_soil::ContactSample {
                    body: *body,
                    material: *material,
                    native: positions[physical_end + j].map(|x| x as f64),
                    bound: all[physical_end + j][12] as f64,
                })
                .collect::<Vec<_>>();
            if !affine_soil::coupled_step_with_samples(
                &mut self.material.bodies,
                &systems,
                &all[..physical_end],
                &native,
                &planes,
                &extra,
            ) {
                #[cfg(test)]
                if let Ok(path) = std::env::var("MIRROR_CUT_SOLVE_FAILURE_CAPTURE") {
                    if !std::path::Path::new(&path).exists() {
                        let capture = json!({
                            "material": self.material.diagnostic(),
                            "responses": &all[..physical_end], "native": native,
                            "time": self.time, "dt": dt, "pose": self.pose,
                            "systems": systems.iter().map(|(a, rhs)| json!({
                                "matrix": a.iter().map(|r| r.to_vec()).collect::<Vec<_>>(),
                                "rhs": rhs,
                            })).collect::<Vec<_>>(),
                            "planes": planes.iter().map(|p| json!({
                                "a": p.a, "b": p.b, "normal": p.normal, "points": p.points,
                            })).collect::<Vec<_>>(),
                            "samples": extra.iter().map(|s| json!({
                                "body": s.body, "material": s.material,
                                "native": s.native, "bound": s.bound,
                            })).collect::<Vec<_>>(),
                        });
                        std::fs::write(&path, serde_json::to_vec(&capture).unwrap()).unwrap();
                        eprintln!("Captured first rejected coupled solve: {path}");
                    }
                    if std::env::var_os("MIRROR_CUT_STOP_ON_SOLVE_FAILURE").is_some() {
                        return Err("stopped after capturing rejected coupled solve".into());
                    }
                }
                *self = before.clone();
                self.rejected += 1;
                self.blocked = true;
                host.invoke("OGCConnectionSystem.Configure", self.spec())?;
                return Ok(false);
            }
            let old_rigid = &positions[tool_start..];
            let result = super::rigid_contact::project(
                &self.tool,
                old_rigid,
                &target,
                &rigid,
                &self.tool_body,
                grasp.as_ref(),
                dt,
            )?;
            self.blocked |= result.restricted;
            self.grasp_feasible = result.grasp_feasible;
            let (origin, axes) = super::rigid_fit::fit(&self.tool, &result.positions)
                .ok_or("cuttable-soil accepted tool pose fit failed")?;
            self.pose = json!({"position":self.frame.global(origin),"right":self.frame.world_vector(axes[0]),"up":self.frame.world_vector(axes[1]),"forward":self.frame.world_vector(axes[2])});
            self.min_bound = rigid
                .iter()
                .map(|r| r[12] as f64)
                .fold(f64::INFINITY, f64::min);
            let material_valid = self.material.valid();
            let crossings = self.crossing_count();
            if !material_valid || crossings > 0 {
                #[cfg(test)]
                if before.rejected == 0 {
                    eprintln!(
                        "cut-volume first rejection: valid={material_valid} crossings={crossings} pieces={} refinement={} cut_area={} pose={}",
                        self.material.bodies.len(),
                        self.material.stats.refinements,
                        self.material.stats.cut_area,
                        self.pose
                    );
                    let scene = self.scene();
                    let (starts, end, tool) = self.layout();
                    let element =
                        |node: usize| starts.partition_point(|s| *s <= node).saturating_sub(1);
                    let allowed = self.material.touch_matrix();
                    let mut pairs = Vec::new();
                    super::intersections::count_filtered_visit(
                        &scene,
                        |a, b| a[0] < end && b[0] < end && allowed[element(a[0])][element(b[0])],
                        |a, b| {
                            if pairs.len() < 4 {
                                pairs.push((
                                    a,
                                    b,
                                    if a[0] < end {
                                        element(a[0])
                                    } else if a[0] >= tool {
                                        9999
                                    } else {
                                        9998
                                    },
                                    if b[0] < end {
                                        element(b[0])
                                    } else if b[0] >= tool {
                                        9999
                                    } else {
                                        9998
                                    },
                                ));
                            }
                        },
                    );
                    eprintln!("cut-volume rejection triangle pairs {pairs:?}");
                    if let Ok(path) = std::env::var("MIRROR_CUT_REJECT_CAPTURE") {
                        std::fs::write(path,serde_json::to_vec(&json!({"material":self.material.diagnostic(),"scene":self.scene().spec(),"starts":starts,"pairs":pairs})).unwrap()).unwrap();
                        if std::env::var_os("MIRROR_CUT_STOP_ON_REJECTION").is_some() {
                            return Err(
                                "stopped after capturing first rejected cut candidate".into()
                            );
                        }
                    }
                }
                *self = before.clone();
                self.rejected += 1;
                self.blocked = true;
                host.invoke("OGCConnectionSystem.Configure", self.spec())?;
                return Ok(false);
            }
            let next = self.positions_for(&cut_samples);
            host.invoke("OGCConnectionSystem.Synchronize",json!({"surface_start":0,"positions":&next[..soil_end],"previous":&previous_positions[..soil_end]}))?;
            host.invoke("OGCConnectionSystem.Synchronize",json!({"surface_start":tool_start,"positions":&next[tool_start..],"previous":&previous_positions[tool_start..]}))?;
            last_responses = rigid;
            last_contact = response;
        }
        for (b, p) in self.material.bodies.iter_mut().zip(&prior.bodies) {
            b.finish(p, dt);
        }
        let next = self.positions_for(&cut_samples);
        if contact_mesh_dirty {
            host.invoke("OGCConnectionSystem.Configure", self.spec())?;
        }
        let mut force = [0.; 3];
        let mut torque = [0.; 3];
        let mut work = 0.;
        let mut effort = 0.;
        let mut edge_effort = 0.;
        let mut edge_force = [0.; 3];
        let origin = self.frame.local(vector(&self.pose["position"])?);
        for (i, r) in last_responses.iter().enumerate() {
            let f = [r[0] as f64, r[1] as f64, r[2] as f64];
            let p = next[tool_start + i].map(|x| x as f64);
            let old = previous_positions[tool_start + i].map(|x| x as f64);
            force = add(force, f);
            torque = add(torque, cross(sub(p, origin), f));
            effort += length(f);
            if self.edge_indices.contains(&i) {
                edge_effort += length(f);
                edge_force = add(edge_force, f);
                work += (-dot(f, sub(p, old))).max(0.);
            }
        }
        self.material.work_reservoir = (self.material.work_reservoir + work).min(2.);
        let accepted_edge = self
            .edge
            .map(|p| self.frame.local(pose_point(&self.pose, p)));
        // A sharp edge can initiate a crack just ahead of the visible tip.
        // Bound this uncalibrated process zone to OGC's 1.5 mm contact radius,
        // and enable it only when accepted edge motion did positive contact
        // work. A stationary or blocked request cannot open a crack.
        let blade_up = vector(&self.pose["up"])?;
        let inward = self.frame.basis.map(|axis| -dot(blade_up, axis));
        if let Some(fracture_edge) = fracture_tip(
            edge, accepted_edge, target_edge, inward, edge_force, equipped, work, edge_effort,
        ) {
            if self.material.fracture(edge, fracture_edge) {
                host.invoke("OGCConnectionSystem.Configure", self.spec())?;
            }
        }
        self.material.connectivity();
        self.force = self.frame.world_vector(force);
        self.torque = self.frame.world_vector(torque);
        self.effort = self.effort.max(effort);
        self.edge_effort = edge_effort;
        last_contact["forces"] = Value::Null;
        self.contacts = last_contact;
        self.accepted += 1;
        self.time += dt;
        self.tool_velocity = mul(
            sub(
                vector(&self.pose["position"])?,
                vector(&before.pose["position"])?,
            ),
            1. / dt,
        );
        Ok(true)
    }
    pub fn advance(&mut self, host: &mut Host, input: &Value) -> Result<Value, String> {
        if self.failure.is_some() {
            return Ok(self.response(&input["pose"]));
        }
        let started = std::time::Instant::now();
        let dt = input["dt"].as_f64().unwrap();
        self.requested += dt;
        let pending = self.accumulator + dt;
        self.accumulator = pending.min(2. / 120.);
        self.discarded += (pending - self.accumulator).max(0.);
        let steps = (self.accumulator * 120. + 1e-9).floor() as usize;
        self.accumulator -= steps as f64 / 120.;
        self.effort = 0.;
        self.blocked = false;
        for _ in 0..steps {
            let mut accepted = false;
            for retry in 0..5 {
                let subdt = (1. / 120.) * 0.5f64.powi(retry);
                if self.substep(host, input, subdt)? {
                    self.accumulator += 1. / 120. - subdt;
                    accepted = true;
                    break;
                }
                if self.failure.is_some() {
                    break;
                }
            }
            if !accepted {
                self.accumulator += 1. / 120.;
                if self.failure.is_none() {
                    self.contacts = json!({"feedback_stale":true,"reason":"all cut-volume contact candidates rejected"});
                }
                self.force = [0.; 3];
                self.torque = [0.; 3];
            }
            if self.failure.is_some() {
                break;
            }
        }
        self.active = self.material.bodies.iter().any(|b| {
            length(b.velocity) > 0.015 || b.deformation_velocity.iter().any(|v| length(*v) > 0.015)
        }) || length(sub(
            vector(&input["pose"]["position"])?,
            vector(&self.pose["position"])?,
        )) > 0.001
            || (input["equipped"] == false && length(self.tool_velocity) > 0.01);
        self.advance_calls += 1;
        self.advance_ms += started.elapsed().as_secs_f64() * 1000.;
        Ok(self.response(&input["pose"]))
    }
    fn triangles(&self, visual: bool) -> Vec<[f64; 6]> {
        let mut out = Vec::new();
        for b in &self.material.bodies {
            for face in &b.poly.faces {
                let color = if face.tag == 2 {
                    [0.20, 0.36, 0.09]
                } else {
                    [0.30, 0.21, 0.12]
                };
                // Only the damaged portions of internal faces are visible; a cut
                // elsewhere on this body must not hide the newly exposed surface.
                if visual && face.tag >= 100 {
                    for tri in self.material.exposed_face(b, face) {
                        for point in tri {
                            let p = self.frame.global(point);
                            out.push([p[0], p[1], p[2], color[0], color[1], color[2]]);
                        }
                    }
                    continue;
                }
                for k in 1..face.nodes.len() - 1 {
                    for i in [face.nodes[0], face.nodes[k], face.nodes[k + 1]] {
                        let p = self.frame.global(b.poly.vertices[i].position);
                        out.push([p[0], p[1], p[2], color[0], color[1], color[2]]);
                    }
                }
            }
        }
        out
    }
    fn grass(&self) -> Vec<[f64; 6]> {
        let mut out = Vec::new();
        for z in [-0.18, 0.18] {
            for x in [-0.13, 0.13] {
                for k in [-1., 1.] {
                    let material = [x + k * 0.035, 0., z];
                    let Some((p, f)) = self.material.sample_material(material) else {
                        continue;
                    };
                    let up = unit(cross(f[2], f[0]));
                    let base = add(p, mul(up, 0.004));
                    for axis in [unit(f[0]), unit(f[2])] {
                        let a = sub(base, mul(axis, 0.012));
                        let b = add(base, mul(axis, 0.012));
                        let c = add(add(base, mul(up, 0.12)), mul(axis, 0.012));
                        for point in [a, b, c, c, b, a] {
                            let p = self.frame.global(point);
                            out.push([p[0], p[1], p[2], 0.18, 0.42, 0.09]);
                        }
                    }
                }
            }
        }
        out
    }
    pub fn frame_value(&self) -> Value {
        let tetrahedra: Vec<_> = self
            .material
            .bodies
            .iter()
            .flat_map(|b| {
                let center = b.poly.center_vertices(false);
                b.poly.triangles().map(move |f| {
                    [
                        self.frame.global(center),
                        self.frame.global(b.poly.vertices[f[0]].position),
                        self.frame.global(b.poly.vertices[f[1]].position),
                        self.frame.global(b.poly.vertices[f[2]].position),
                    ]
                })
            })
            .collect();
        let material = self.material_value();
        json!({"tetrahedra":tetrahedra,"debris":self.triangles(false),"visual":self.triangles(true),"grass":self.grass(),"active":self.active,"removed_volume":material["detached_volume"],"removed_mass":material["detached_mass"],"failure":self.failure,"blocked":self.blocked,"material":material,"contact":self.contacts,"peak_contact_pairs":self.peak_pairs,"accepted_steps":self.accepted,"rejected_steps":self.rejected,
            "timing":{"advance_calls":self.advance_calls,"advance_ms":self.advance_ms,"native_contact_ms":self.native_ms},"minimum_rigid_bound":self.min_bound,"near_zero_rigid_vertices":0,"grasp_feasible":self.grasp_feasible,"requested_seconds":self.requested,"budget_discarded_seconds":self.discarded,"maximum_substeps_per_frame":2,"simulated_seconds":self.time,"rest_volume":self.material.rest_volume})
    }
    pub fn height(&self, position: D) -> Option<f64> {
        let n = unit(position);
        let p = self.frame.local(mul(n, terrain::RADIUS));
        let mut highest = None::<f64>;
        for b in &self.material.bodies {
            for f in b.poly.triangles() {
                let [a, c, z] = f.map(|i| b.poly.vertices[i].position);
                let den = (c[2] - z[2]) * (a[0] - z[0]) + (z[0] - c[0]) * (a[2] - z[2]);
                if den.abs() < 1e-14 {
                    continue;
                }
                let u = ((c[2] - z[2]) * (p[0] - z[0]) + (z[0] - c[0]) * (p[2] - z[2])) / den;
                let v = ((z[2] - a[2]) * (p[0] - z[0]) + (a[0] - z[0]) * (p[2] - z[2])) / den;
                if u >= -1e-7 && v >= -1e-7 && u + v <= 1.0000001 {
                    let y = u * a[1] + v * c[1] + (1. - u - v) * z[1];
                    highest = Some(highest.map_or(y, |h| h.max(y)));
                }
            }
        }
        highest.map(|y| dot(self.frame.global([p[0], y, p[2]]), n) - terrain::RADIUS)
    }
    pub fn aim(&self, host: &mut Host, input: &Value) -> Result<Value, String> {
        let p = vector(&input["position"])?;
        let ray = unit(vector(&input["forward"])?);
        let mut previous = 0.;
        let height = |host: &mut Host, q: D| -> Result<f64, String> {
            let h = host.invoke("PlanetSurfaceSystem.Height", json!({"position":q}))?["height"]
                .as_f64()
                .unwrap();
            Ok(self.height(q).map_or(h, |m| m.max(h)))
        };
        for i in 0..=96 {
            let t = i as f64 * 3. / 96.;
            let q = add(p, mul(ray, t));
            if length(q) - terrain::RADIUS <= height(host, q)? {
                let (mut lo, mut hi) = (previous, t);
                for _ in 0..12 {
                    let mid = (lo + hi) * 0.5;
                    let q = add(p, mul(ray, mid));
                    if length(q) - terrain::RADIUS > height(host, q)? {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                let q = add(p, mul(ray, (lo + hi) * 0.5));
                return Ok(
                    json!({"hit":true,"position":q,"normal":unit(q),"physical_soil":self.height(q).is_some_and(|h|(length(q)-terrain::RADIUS-h).abs()<0.02)}),
                );
            }
            previous = t;
        }
        Ok(json!({"hit":false}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contact_mesh_welds_material_vertices_and_preserves_exposed_area() {
        let mut material = Material::new([-0.26, -0.24, -0.36], [0.26, 0., 0.36]);
        let edge = [[-0.115, 0.001, 0.], [0.115, 0.001, 0.]];
        let end = [[-0.115, -0.04, 0.], [0.115, -0.04, 0.]];
        assert!(material.cut(edge, end, 20., 0.0012));
        assert!(State::cut_mesh_for(&material).1.is_empty());
        material.work_reservoir = 1.;
        assert!(material.fracture(edge, end));
        let area = |t: [D; 3]| length(cross(sub(t[1], t[0]), sub(t[2], t[0]))) * 0.5;
        let expected: f64 = material
            .bodies
            .iter()
            .flat_map(|body| {
                body.poly
                    .faces
                    .iter()
                    .flat_map(|face| material.exposed_face_material(body, face))
            })
            .map(area)
            .sum();
        let (samples, faces) = State::cut_mesh_for(&material);
        assert!(!faces.is_empty());
        assert!(
            samples.len() < faces.len() * 3,
            "shared vertices should be welded"
        );
        let actual: f64 = faces
            .iter()
            .map(|face| {
                let body = samples[face[0]].0;
                assert!(face.iter().all(|n| samples[*n].0 == body));
                area(face.map(|n| samples[n].1))
            })
            .sum();
        assert!(
            (actual - expected).abs() < 1e-9,
            "lost exposed area: {actual} versus {expected}"
        );
    }
    #[test]
    fn fracture_zone_requires_accepted_loaded_motion() {
        let edge = [[-0.1, 0., 0.], [0.1, 0., 0.]];
        let moved = edge.map(|p| add(p, [0., -0.001, 0.]));
        let inward = [0., -1., 0.];
        let force = [0., 100., 0.];
        let tip = |accepted, equipped, work, effort| {
            fracture_tip(edge, accepted, moved, inward, force, equipped, work, effort)
        };
        assert!(tip(edge, true, 1., 100.).is_none());
        assert!(tip(moved, true, 0., 100.).is_none());
        assert!(tip(moved, false, 1., 100.).is_none());
        assert!(tip(moved, true, 1., 0.).is_none());
        let tip = tip(moved, true, 1., 100.).unwrap();
        assert!((tip[0][1] - moved[0][1] + 0.0015).abs() < 1e-12);
    }
    #[test]
    fn fracture_zone_follows_loaded_blade_instead_of_lateral_drift() {
        let edge = [[-0.1, 0., 0.], [0.1, 0., 0.]];
        let slipped = edge.map(|p| add(p, [0.00001, 0., 0.]));
        for inward in [[0., -1., 0.], [0., -0.6, -0.8]] {
            let wanted = edge.map(|p| add(p, mul(inward, 0.006)));
            let force = mul(inward, -100.);
            let tip = fracture_tip(edge, slipped, wanted, inward, force, true, 0.001, 100.)
                .unwrap();
            for i in 0..2 {
                assert!(length(sub(sub(tip[i], slipped[i]), mul(inward, 0.0015))) < 1e-12);
            }
            let rising = edge.map(|p| sub(p, mul(inward, 0.006)));
            assert!(fracture_tip(edge, slipped, rising, inward, force, true, 0.001, 100.).is_none());
            assert!(fracture_tip(edge, slipped, wanted, inward, [100., 0., 0.], true, 0.001, 100.).is_none());
            assert!(fracture_tip(edge, slipped, wanted, inward, mul(force, -1.), true, 0.001, 100.).is_none());
        }
    }
}
