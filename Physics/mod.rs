//! Bounded CPU XPBD material integration with a separately contracted OGC contact client.
mod intersections;
pub(crate) mod math;
pub(crate) mod planet;
pub(crate) mod shovel;
pub(crate) mod terrain_tool;
mod tool_motion;
mod rigid_fit;
mod rigid_contact;
mod rigid_motion;
mod soil_material;
mod cut_geometry;
mod affine_soil;
mod soil_cutting;
mod soil_volume;
pub(crate) fn material_point_at_snapshot(snapshot:&serde_json::Value,point:[f64;3])->Option<[f64;3]> {
    soil_volume::sample_snapshot(snapshot,point)
}
mod scenes;
mod soil_workshop;
mod soil_contact_baseline;
pub(crate) mod matter;
#[path="Sod.rs"]
mod sod;
use crate::host::Host;
use math::*;
use serde_json::{Value, json};

#[derive(Clone)]
struct Edge {
    a: usize,
    b: usize,
    rest: f32,
    compliance: f32,
    lambda: f32,
}
#[derive(Clone)]
struct Tet {
    ids: [usize; 4],
    rest: f32,
    lambda: f32,
}
#[derive(Clone)]
pub(crate) struct Surface {
    start: usize,
    count: usize,
    faces: Vec<[u32; 3]>,
    fixed: bool,
    self_collision: bool,
}
#[derive(Clone)]
pub(crate) enum Pin {
    Twist { center: V, sign: f32, turns: f32 },
    Pull { axis: usize, sign: f32, speed: f32 },
    Body { phase: f32 },
    Robot,
}
#[derive(Clone)]
pub(crate) struct Scene {
    pub(crate) name: String,
    pub(crate) p: Vec<V>,
    pub(crate) velocity: Vec<V>,
    rest: Vec<V>,
    weights: Vec<f32>,
    colors: Vec<V>,
    edges: Vec<Edge>,
    tets: Vec<Tet>,
    pub(crate) surfaces: Vec<Surface>,
    pins: Vec<(usize, Pin)>,
    pin_angles: Vec<f32>,
    pub(crate) gravity: V,
    pub(crate) radius: f32,
    pub(crate) time: f32,
    pub(crate) proxy: String,
    substeps: usize,
    iterations: usize,
    sod: Option<sod::Material>,
    soil_probe: Option<soil_workshop::Probe>,
    contact_probe: Option<soil_contact_baseline::Probe>,
}
impl Scene {
    pub(crate) fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            p: vec![],
            velocity: vec![],
            rest: vec![],
            weights: vec![],
            colors: vec![],
            edges: vec![],
            tets: vec![],
            surfaces: vec![],
            pins: vec![],
            pin_angles: vec![],
            gravity: [0., -9.81, 0.],
            radius: 0.008,
            time: 0.,
            proxy: String::new(),
            substeps: 4,
            iterations: 4,
            sod: None,
            soil_probe: None,
            contact_probe: None,
        }
    }
    pub(crate) fn node(&mut self, p: V, mass: f32, color: V) {
        self.p.push(p);
        self.rest.push(p);
        self.velocity.push([0.; 3]);
        self.weights.push(if mass > 0. { 1. / mass } else { 0. });
        self.colors.push(color);
    }
    pub(crate) fn surface(
        &mut self,
        start: usize,
        faces: Vec<[u32; 3]>,
        fixed: bool,
        self_collision: bool,
    ) {
        self.surfaces.push(Surface {
            start,
            count: self.p.len() - start,
            faces,
            fixed,
            self_collision,
        });
    }
    pub(crate) fn edge(&mut self, a: usize, b: usize, compliance: f32) {
        self.edges.push(Edge {
            a,
            b,
            rest: length(sub(self.rest[b], self.rest[a])),
            compliance,
            lambda: 0.,
        });
    }
    pub(crate) fn tetra(&mut self, ids: [usize; 4]) {
        let p = ids.map(|i| self.rest[i]);
        let rest = dot(sub(p[1], p[0]), cross(sub(p[2], p[0]), sub(p[3], p[0]))) / 6.;
        if rest.abs() > 1e-9 {
            self.tets.push(Tet {
                ids,
                rest,
                lambda: 0.,
            });
            // Volume alone permits cell shearing/inversion. Every tetrahedral
            // edge also carries elastic strain, shared edges only once.
            for a in 0..4 {
                for b in a + 1..4 {
                    if !self.edges.iter().any(|e| {
                        (e.a == ids[a] && e.b == ids[b]) || (e.a == ids[b] && e.b == ids[a])
                    }) {
                        self.edge(ids[a], ids[b], 1e-8);
                    }
                }
            }
        }
    }
    pub(crate) fn pin(&mut self, node: usize, pin: Pin) {
        self.weights[node] = 0.;
        self.pins.push((node, pin));
        self.pin_angles.push(0.);
    }
    pub(crate) fn finish(&self) -> Result<(), String> {
        if self.p.len() > 8192 || self.faces().len() > 8192 {
            return Err(
                "scene exceeds the initial 8192-node/triangle budget; lower mesh scale".into(),
            );
        }
        let mut covered = vec![false; self.p.len()];
        for s in &self.surfaces {
            for i in s.start..s.start + s.count {
                if covered[i] {
                    return Err("overlapping contact surface ranges".into());
                }
                covered[i] = true;
            }
        }
        if covered.iter().any(|v| !*v) {
            return Err("node has no contact surface".into());
        }
        if self.p.iter().flatten().any(|v| !v.is_finite()) {
            return Err("non-finite initial scene".into());
        }
        Ok(())
    }
    pub(crate) fn faces(&self) -> Vec<[usize; 3]> {
        self.surfaces
            .iter()
            .flat_map(|s| s.faces.iter().map(|f| f.map(|i| s.start + i as usize)))
            .collect()
    }
    pub(crate) fn spec(&self) -> Value {
        json!({"positions":self.p,"contact_radius":self.radius,"contact_stiffness":5000.,"surfaces":self.surfaces.iter().map(|s|json!({"start":s.start,"count":s.count,"triangles":s.faces,"motion":if s.fixed{2}else{0},"self_collision":s.self_collision,"friction":0.3})).collect::<Vec<_>>()})
    }
    pub(crate) fn mesh(&self) -> Value {
        let mut vertices: Vec<[f32; 6]> = self
            .faces()
            .iter()
            .flat_map(|face| {
                let [a, b, c] = face.map(|i| self.p[i]);
                let n = unit(cross(sub(b, a), sub(c, a)));
                let light = 0.3 + 0.7 * dot(n, unit([0.4, 0.8, 0.5])).abs();
                face.map(|i| {
                    let p = self.p[i];
                    let c = self.colors[i].map(|v| v * light);
                    [p[0], p[1], p[2], c[0], c[1], c[2]]
                })
            })
            .collect();
        if let Some(probe) = &self.soil_probe {
            probe.decorate(self, &mut vertices);
        }
        json!({"kind":"visual","shader":"material_surface","vertices":vertices})
    }
    fn drivers(&mut self) {
        for (node, pin) in &self.pins {
            if self.name == "yarn_twist" && self.time >= 8. {
                self.weights[*node] = 250.;
                continue;
            }
            let rest = self.rest[*node];
            let t = self.time;
            self.p[*node] = match pin {
                Pin::Twist {
                    center,
                    sign,
                    turns,
                } => rotate_x(
                    rest,
                    *center,
                    sign * turns * std::f32::consts::TAU * (t / 8.).min(1.),
                ),
                Pin::Pull { axis, sign, speed } => {
                    let mut p = rest;
                    p[*axis] += sign * speed * t.min(10.);
                    p
                }
                Pin::Body { phase } => add(rest, [0.08 * (t + phase).sin(), 0., 0.04 * t.sin()]),
                Pin::Robot => add(
                    rest,
                    [
                        0.15 * (t * 0.5).sin(),
                        0.12 * (t * 0.8).sin().max(0.),
                        0.1 * (t * 0.5).sin(),
                    ],
                ),
            };
        }
    }
    fn constraints(&mut self, dt: f32) {
        for e in &mut self.edges {
            let delta = sub(self.p[e.b], self.p[e.a]);
            let l = length(delta);
            if l < 1e-9 {
                continue;
            }
            let alpha = e.compliance / (dt * dt);
            let denom = self.weights[e.a] + self.weights[e.b] + alpha;
            if denom <= 0. {
                continue;
            }
            let lambda = (-(l - e.rest) - alpha * e.lambda) / denom;
            e.lambda += lambda;
            let correction = scale(delta, lambda / l);
            self.p[e.a] = sub(self.p[e.a], scale(correction, self.weights[e.a]));
            self.p[e.b] = add(self.p[e.b], scale(correction, self.weights[e.b]));
        }
        for tet in &mut self.tets {
            let p = tet.ids.map(|i| self.p[i]);
            let volume = dot(sub(p[1], p[0]), cross(sub(p[2], p[0]), sub(p[3], p[0]))) / 6.;
            let g1 = scale(cross(sub(p[2], p[0]), sub(p[3], p[0])), 1. / 6.);
            let g2 = scale(cross(sub(p[3], p[0]), sub(p[1], p[0])), 1. / 6.);
            let g3 = scale(cross(sub(p[1], p[0]), sub(p[2], p[0])), 1. / 6.);
            let g0 = scale(add(add(g1, g2), g3), -1.);
            let grads = [g0, g1, g2, g3];
            let alpha = 1e-8 / (dt * dt);
            let denom = alpha
                + tet
                    .ids
                    .iter()
                    .zip(grads)
                    .map(|(i, g)| self.weights[*i] * dot(g, g))
                    .sum::<f32>();
            if denom > 0. {
                let lambda = (-(volume - tet.rest) - alpha * tet.lambda) / denom;
                tet.lambda += lambda;
                for (i, g) in tet.ids.iter().zip(grads) {
                    self.p[*i] = add(self.p[*i], scale(g, lambda * self.weights[*i]));
                }
            }
        }
    }
    fn integrate(&mut self, host: &mut Host, dt: f32) -> Result<Value, String> {
        let previous = self.p.clone();
        if let Some(mut probe) = self.contact_probe.take() {
            probe.drive(self, dt);
            self.contact_probe = Some(probe);
        }
        if let Some(mut probe) = self.soil_probe.take() {
            probe.drive(self, dt);
            self.soil_probe = Some(probe);
        }
        for i in 0..self.p.len() {
            if self.weights[i] > 0. {
                self.velocity[i] = add(self.velocity[i], scale(self.gravity, dt));
                self.p[i] = add(self.p[i], scale(self.velocity[i], dt));
            }
        }
        self.time += dt;
        if let Some(mut material)=self.sod.take() {
            material.drive(self);
            material.begin_step();
            self.sod=Some(material);
        }
        self.drivers();
        for e in &mut self.edges {
            e.lambda = 0.;
        }
        for tet in &mut self.tets {
            tet.lambda = 0.;
        }
        let mut contacts = Value::Null;
        for _ in 0..self.iterations {
            self.constraints(dt);
            if let Some(mut probe) = self.soil_probe.take() {
                probe.solve(self, dt);
                self.soil_probe = Some(probe);
            }
            if let Some(mut material)=self.sod.take() {
                material.solve(self,dt);
                self.sod=Some(material);
            }
            self.drivers();
            let projected = host.invoke(
                "OGCConnectionSystem.Project",
                json!({"positions":self.p,"previous":previous,"inverse_mass":self.weights,"dt":dt}),
            )?;
            self.p = serde_json::from_value(projected["positions"].clone())
                .map_err(|e| e.to_string())?;
            if let Some(mut material)=self.sod.take() {
                material.observe_contact(self, projected["vertex_face_contacts"].as_u64().unwrap_or(0)+projected["edge_edge_contacts"].as_u64().unwrap_or(0));
                self.sod=Some(material);
            }
            contacts = projected;
            contacts["positions"] = Value::Null;
            if let Some(mut probe) = self.contact_probe.take() {
                probe.observe(&contacts);
                self.contact_probe = Some(probe);
            }
            if let Some(mut probe) = self.soil_probe.take() {
                probe.observe(&contacts);
                self.soil_probe = Some(probe);
            }
        }
        if let Some(probe) = &mut self.soil_probe {
            probe.end_step(dt);
        }
        for (index, (node, pin)) in self.pins.iter().enumerate() {
            if let Pin::Twist { center, .. } = pin {
                if self.weights[*node] > 0. {
                    continue;
                }
                let a = sub(previous[*node], *center);
                let b = sub(self.p[*node], *center);
                let delta = (b[2].atan2(b[1]) - a[2].atan2(a[1]) + std::f32::consts::PI)
                    .rem_euclid(std::f32::consts::TAU)
                    - std::f32::consts::PI;
                self.pin_angles[index] += delta;
            }
        }
        for i in 0..self.p.len() {
            self.velocity[i] = scale(sub(self.p[i], previous[i]), (-0.8 * dt).exp() / dt);
        }
        Ok(contacts)
    }
    fn twist_progress(&self) -> Value {
        let mut actual = 0.;
        let mut requested = 0.;
        let mut error = 0f32;
        let mut count = 0.;
        for (i, (node, pin)) in self.pins.iter().enumerate() {
            if let Pin::Twist {
                center,
                sign,
                turns,
            } = pin
            {
                let rest = sub(self.rest[*node], *center);
                if rest[1] * rest[1] + rest[2] * rest[2] < 1e-8 {
                    continue;
                }
                actual += 2. * self.pin_angles[i] * sign / std::f32::consts::TAU;
                requested += 2. * turns * (self.time / 8.).min(1.);
                count += 1.;
                if self.weights[*node] == 0. {
                    error = error.max(length(sub(
                        self.p[*node],
                        rotate_x(
                            self.rest[*node],
                            *center,
                            sign * turns * std::f32::consts::TAU * (self.time / 8.).min(1.),
                        ),
                    )));
                }
            }
        }
        if count == 0. {
            Value::Null
        } else {
            json!({"commanded_relative_turns":requested/count,"measured_relative_turns":actual/count,"maximum_handle_target_error":error,"released":self.name=="yarn_twist"&&self.time>=8.})
        }
    }
    pub(crate) fn intersection_count(&self) -> usize {
        intersections::count(self)
    }
}
#[derive(Default, Clone)]
pub(crate) struct State {
    matter: Option<matter::World>,
    game:matter::game::Game,
    pub(crate) scene: Option<Scene>,
    frames: u64,
    contacts: Value,
    halt: Option<String>,
    sum_ms: f64,
    max_ms: f64,
    peak_pairs: u64,
    verification_retries: u64,
    last_rejection: Value,
}
pub(crate) fn cases() -> &'static [&'static str] {
    scenes::CASES
}
impl State {
    pub(crate) fn game_collision_vertices(&self) -> Vec<[f64;3]> { self.game.collision_vertices() }
    pub(crate) fn game_vertices(&self,eye:[f64;3],forward:[f64;3],up:[f64;3],aspect:f64) -> Result<Vec<[f64;6]>,String> {self.game.visible_vertices(eye,forward,up,aspect)}
}
#[cfg(test)]
impl State {
    pub(crate) fn mining_diagnostics(&self) -> Value { self.game.diagnostics() }
    pub(crate) fn seed_crafting_tree(&mut self,p:[f64;3],fallen:bool){self.game.seed_crafting_tree(p,fallen);}
}
pub(crate) fn is_case(name: &str) -> bool {
    cases().contains(&name) || matter::is_case(name)
}
pub(crate) fn valid_request(v: &Value) -> bool {
    v["frames"].as_u64().is_none_or(|n| (1..=3600).contains(&n))
        && v["case"].as_str().is_some_and(is_case)
        && v["scale"].as_u64().is_none_or(|n| (1..=3).contains(&n))
}
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let mut state = std::mem::take(&mut host.physics);
    let result = (|| {
        match hook {
            "GameResolveBody" => return state.game.resolve_body(input),
            "GameWaterSample" => return state.game.water_sample(input),
            "GameWater" => return Ok(state.game.water_frame()),
            "GameCollision" => return Ok(state.game.collision_frame()),
            "GameStep" => return state.game.step(host,input),
            "GameFrame" => return Ok(state.game.frame()),
            "GameCraftTree" => return state.game.craft_tree(input),
            "GameHeight" => {let p=serde_json::from_value(input["position"].clone()).map_err(|_|"game height position")?;return Ok(json!({"height":state.game.height(p,input["eye_height"].as_f64().unwrap_or(2.))}));},
            "GamePause" => {state.game.pause();return Ok(Value::Null);},
            "GameClose" => {if host.invoke("OGCConnectionSystem.Ownership",Value::Null)?["owned_by_caller"]==true{host.invoke("OGCConnectionSystem.Release",Value::Null)?;}host.invoke("VegetationSystem.MaterialTree",json!({"operation":"release"}))?;if host.system_enabled("TerrainSurfaceHydrologySystem"){host.invoke("TerrainSurfaceHydrologySystem.MaterialDomain",json!({"clear":true}))?;}host.invoke("PlanetSurfaceSystem.ReleaseMaterials",Value::Null)?;state.game=Default::default();return Ok(Value::Null);},
            "Configure" => {
                let name=input["case"].as_str().unwrap();
                if matter::is_case(name) {
                    let mut world=matter::World::new(name);
                    world.load_worker(host)?;
                    world.configure(host)?;
                    state=State {matter:Some(world),..Default::default()};
                    host.emit(json!({"event":"physics_configured","case":name}))?;
                    return Ok(state.report());
                }
                let scene = scenes::create(
                    input["case"].as_str().unwrap(),
                    input["scale"].as_u64().unwrap_or(1) as usize,
                )?;
                let initial = scene.intersection_count();
                if initial > 0 {
                    return Err(format!(
                        "scenario initial state intersects ({initial} triangle pairs); cannot start an OGC reference run"
                    ));
                }
                host.invoke("OGCConnectionSystem.Configure", scene.spec())?;
                state = State {
                    scene: Some(scene),
                    ..Default::default()
                };
                host.emit(json!({"event":"physics_configured","case":input["case"]}))?;
            }
            "Advance" => {
                if let Some(world)=&mut state.matter {
                    if let Err(error)=world.advance(host) {world.fail(error);}
                    return Ok(state.report());
                }
                if state.halt.is_some() {
                    return Ok(state.report());
                }
                let start = std::time::Instant::now();
                let substeps = state
                    .scene
                    .as_ref()
                    .ok_or("physics is not configured")?
                    .substeps;
                for _ in 0..substeps {
                    if let Err(error) = state.advance_safe(host, 1. / (60 * substeps) as f32, 0) {
                        state.halt = Some(error);
                        break;
                    }
                }
                state.frames += 1;
                let scene = state.scene.as_ref().ok_or("physics scene missing")?;
                let intersections = scene.intersection_count();
                if intersections > 0 {
                    state.halt = Some(format!(
                        "intersection monitor detected {intersections} triangle pairs"
                    ));
                }
                if scene.p.iter().flatten().any(|v| !v.is_finite()) {
                    state.halt = Some("non-finite material state".into());
                }
                let elapsed = start.elapsed().as_secs_f64() * 1000.;
                state.sum_ms += elapsed;
                state.max_ms = state.max_ms.max(elapsed);
                host.emit(json!({"event":"physics_step","frame":state.frames,"simulated_seconds":scene.time,"step_ms":elapsed,"intersections":intersections,"halted":state.halt.is_some()}))?;
            }
            "Release" => {
                host.invoke("OGCConnectionSystem.Release", Value::Null)?;
                state = State::default();
                return Ok(Value::Null);
            }
            "Report" => {}
            "Mesh" => return Ok(if let Some(world)=&state.matter {world.mesh()} else {state.scene.as_ref().ok_or("physics not configured")?.mesh()}),
            _ => return Err("unknown physics hook".into()),
        }
        Ok(state.report())
    })();
    host.physics = state;
    result
}
impl State {
    fn advance_safe(&mut self, host: &mut Host, dt: f32, depth: u32) -> Result<(), String> {
        let backup = self
            .scene
            .as_ref()
            .ok_or("physics is not configured")?
            .clone();
        let integration = self.scene.as_mut().unwrap().integrate(host, dt);
        if let Err(error) = integration {
            self.scene = Some(backup);
            host.invoke(
                "OGCConnectionSystem.Configure",
                self.scene.as_ref().unwrap().spec(),
            )?;
            return Err(error);
        }
        let contacts = integration.unwrap();
        let candidate = self.scene.as_ref().unwrap();
        let non_finite = candidate.p.iter().flatten().any(|v| !v.is_finite());
        let crossings = candidate.intersection_count();
        let inverted = candidate
            .tets
            .iter()
            .filter(|tet| {
                let p = tet.ids.map(|i| candidate.p[i]);
                let v = dot(sub(p[1], p[0]), cross(sub(p[2], p[0]), sub(p[3], p[0]))) / 6.;
                v * tet.rest <= 0.
            })
            .count();
        let invalid = non_finite || crossings > 0 || inverted > 0;
        if invalid {
            self.last_rejection = json!({"candidate_time":candidate.time,"dt":dt,"intersection_pairs":crossings,"inverted_tetrahedra":inverted,"non_finite":non_finite});
            self.scene = Some(backup);
            host.invoke(
                "OGCConnectionSystem.Configure",
                self.scene.as_ref().unwrap().spec(),
            )?;
            self.verification_retries += 1;
            if depth >= 5 {
                return Err(format!(
                    "minimum step rejected: {crossings} intersection pairs, {inverted} inverted tetrahedra; last valid state retained"
                ));
            }
            self.advance_safe(host, dt * 0.5, depth + 1)?;
            self.advance_safe(host, dt * 0.5, depth + 1)?;
        } else {
            self.peak_pairs = self.peak_pairs.max(
                contacts["vertex_face_contacts"].as_u64().unwrap_or(0)
                    + contacts["edge_edge_contacts"].as_u64().unwrap_or(0),
            );
            self.contacts = contacts;
        }
        Ok(())
    }
    pub(crate) fn report(&self) -> Value {
        if let Some(world)=&self.matter {return world.report();}
        let scene = self.scene.as_ref();
        json!({"case":scene.map(|s|s.name.as_str()),"frames":self.frames,"nodes":scene.map_or(0,|s|s.p.len()),"tetrahedra":scene.map_or(0,|s|s.tets.len()),"twist":scene.map(Scene::twist_progress),"sod":scene.and_then(|s|s.sod.as_ref().map(|m|m.report(s))),"soil_probe":scene.and_then(|s|s.soil_probe.as_ref().map(|m|m.report(s)).or_else(||s.contact_probe.as_ref().map(|m|m.report(s)))),"substeps_per_frame":scene.map_or(0,|s|s.substeps),"iterations_per_substep":scene.map_or(0,|s|s.iterations),"triangles":scene.map_or(0,|s|s.faces().len()),"simulated_seconds":scene.map_or(0.,|s|s.time),"step_ms_average":self.sum_ms/self.frames.max(1)as f64,"step_ms_max":self.max_ms,"peak_contact_pairs":self.peak_pairs,"verification_retries":self.verification_retries,"last_rejected_step":self.last_rejection,"build_profile":if cfg!(debug_assertions){"debug"}else{"release"},"continuous_collision_detection":false,"verified_intersections":scene.map_or(0,Scene::intersection_count),"simulated_seconds_per_wall_second":scene.map_or(0.,|s|s.time as f64)/(self.sum_ms/1000.).max(1e-9),"contact":self.contacts,"halted":self.halt.is_some(),"failure":self.halt,"differences":scene.map(|s|format!("CPU XPBD distance/bending/volume constraints plus existing OGC adaptation; reduced mesh scale; procedural geometry. {}",s.proxy))})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requested_cases_have_bounded_geometry() {
        for case in cases() {
            let scene = scenes::create(case, 1).unwrap();
            assert!(scene.p.len() > 4);
            assert!(!scene.faces().is_empty());
            scene.finish().unwrap();
        }
    }
    #[test]
    fn edge_constraint_reduces_strain() {
        let mut s = Scene::new("test");
        s.node([0., 0., 0.], 1., [1.; 3]);
        s.node([1., 0., 0.], 1., [1.; 3]);
        s.edge(0, 1, 0.);
        s.p[1][0] = 2.;
        s.constraints(1. / 240.);
        assert!((length(sub(s.p[0], s.p[1])) - 1.).abs() < 1e-5);
    }

}
