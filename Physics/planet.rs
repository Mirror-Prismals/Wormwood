//! Mirror's editable radial surface and bounded coarse material fragments.
use super::{Scene, State as MaterialState, math::*};
#[path="WaterSnapshot.rs"]
mod water_snapshot;
#[path="WaterCollisionWorker.rs"]
pub(crate) mod collision_worker;
use crate::host::Host;
use serde_json::{Value, json};
pub(crate) const RADIUS: f64 = 6_371_000.;
type D = [f64; 3];
/// Numeric transport for the registered CollisionSnapshot route. This keeps
/// the accepted geometry and ownership checks without a per-frame JSON mesh.
fn collision_input(host:&mut Host,position:D)->Result<collision_worker::Input,String>{
    if position.iter().any(|v|!v.is_finite()){return Err("invalid collision position".into());}
    let mut state=std::mem::take(&mut host.planet);
    let result=(||{
        state.load(host)?;
        let mut frame=state.frame();
        frame["underground"]=if host.system_enabled("CaveSystem"){host.invoke("CaveSystem.Snapshot",Value::Null)?}else{Value::Null};
        if frame["underground"].is_object(){frame["underground"]["material_domains"]=json!(state.edits);}
        let material=host.material_collision_vertices()?;
        let venue=host.theatre.venue.clone();let site=host.theatre.nightworld.clone();
        let stamp=[venue.as_ref().map_or(0,|v|std::sync::Arc::as_ptr(v)as usize as u64),venue.as_ref().map_or(0,|v|crate::audio::geometry::Geometry::revision(v.as_ref())),site.as_ref().map_or(0,|v|std::sync::Arc::as_ptr(v)as usize as u64),crate::terrain::nightworld::active(position)as u64,host.theatre.origin[0].to_bits(),host.theatre.origin[1].to_bits(),host.theatre.origin[2].to_bits()];
        let asset_changed=state.architecture_contact.as_ref().is_none_or(|c|c.stamp!=stamp);
        let refresh=asset_changed||state.architecture_contact.as_ref().is_none_or(|c|(0..3).any(|i|(position[i]-c.focus[i]).abs()>4.));
        if refresh {
            if asset_changed{state.architecture_epoch=state.architecture_epoch.checked_add(1).ok_or("architecture contact revision exhausted")?;}
            let mut triangles=Vec::new();
            if let Some(park)=venue.as_ref().and_then(|v|v.park.as_ref()) {triangles.extend(park.collision_vertices_radius(crate::audio::sub(position,host.theatre.origin),host.theatre.origin,40.));}
            if let Some(site)=&site{triangles.extend(site.collision_vertices_radius(position,40.));}
            state.architecture_contact=Some(ArchitectureContact{stamp,focus:position,triangles:std::sync::Arc::new(triangles),_venue:venue,_site:site});
        }
        let contact=state.architecture_contact.as_ref().unwrap();
        Ok(collision_worker::Input{position,frame,material,architecture:contact.triangles.clone(),architecture_revision:state.architecture_epoch,architecture_focus:if contact._venue.is_some()||contact._site.is_some(){Some(contact.focus)}else{None}})
    })();
    host.planet=state;result
}
pub(crate) fn collision_numeric(host:&mut Host,position:D)->Result<crate::water::types::Snapshot,String>{
    let input=collision_input(host,position)?;
    {let mut material=input.material;material.extend_from_slice(&input.architecture);host.planet.water_collision.capture_numeric_material(input.position,&input.frame,&Value::Null,material)}
}
pub(crate) fn collision_async(host:&mut Host,position:D)->Result<crate::water::types::Snapshot,String>{
    let input=collision_input(host,position)?;
    if host.water_collision_preparation.is_none(){host.water_collision_preparation=Some(collision_worker::Worker::start()?);}
    host.water_collision_preparation.as_mut().unwrap().advance(input)
}

fn dot(a: D, b: D) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn unit(p: D) -> D {
    let d = dot(p, p).sqrt().max(1e-15);
    p.map(|v| v / d)
}
fn cross(a: D, b: D) -> D {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn point(v: &Value) -> Result<D, String> {
    serde_json::from_value(v.clone()).map_err(|e| e.to_string())
}
#[derive(Clone)]
struct ArchitectureContact {stamp:[u64;7],focus:D,triangles:std::sync::Arc<Vec<D>>,_venue:Option<std::sync::Arc<crate::theatre::Venue>>,_site:Option<std::sync::Arc<crate::theatre::nightworld::Site>>}
#[derive(Default, Clone)]
pub(crate) struct State {
    water_collision:water_snapshot::Cache,
    architecture_contact:Option<ArchitectureContact>,
    architecture_epoch:u64,
    loaded: bool,
    persistent: bool,
    edits: Vec<Value>,
    anchor: Option<D>,
    bodies: Vec<(usize, f32)>,
    material: MaterialState,
    active: bool,
    steps: u64,
    removed: f64,
}
impl State {
    pub(crate) fn contact_active(&self) -> bool {
        self.active
    }
    fn height(&self, n: D) -> f64 {
        crate::terrain::tool_cut::height(n, crate::terrain::height(n), &self.edits, true)
    }
    fn mine_cell(&mut self, input: &Value) -> Result<Value,String> {
        use crate::physics::matter::world_field::{substance, Substance};
        use crate::terrain::material_domain::Domain;
        use crate::terrain::material_domain::CELL_SIDE as SIDE;
        let hit = point(&input["point"])?;
        let dir = unit(point(&input["direction"])?);
        let eye = point(&input["eye"])?;
        let offset: D = std::array::from_fn(|i| hit[i] - eye[i]);
        if dot(offset,offset) > 4.51_f64.powi(2) || dot(dir,dir) < 0.99 {
            return Ok(json!({"changed":false,"status":"Aim at reachable terrain"}));
        }
        let fill = input["fill"] == true;
        // Select the empty side on refill, and the solid side on excavation.
        let probe: D = std::array::from_fn(|i| hit[i] + dir[i] * if fill {-0.008} else {0.008});
        let tile = crate::terrain::tetrahedral::at(probe).ok_or("terrain tile lookup failed")?;
        let origin = probe.map(|x| (x / SIDE).floor() * SIDE);
        let index = self.edits.iter().position(|e| e["mined"] == true &&
            if !e["tile"].is_null() { e["tile"] == tile["tile"] }
            else {Domain::parse(e).is_some_and(|d| if d.tetra.is_some() { d.distance(probe) <= 0.02 } else { d.origin == origin })});
        let reply = |changed, status| Ok(json!({"changed":changed,"status":status}));
        if fill {
            let Some(i) = index else {return reply(false,"Aim at the inside face of a cut");};
            let up = unit(eye);
            let domain = Domain::parse(&self.edits[i]).unwrap();
            if (0..=10).any(|k| domain.distance(std::array::from_fn(|j|eye[j]-up[j]*0.18*k as f64)) < 0.28) {
                return reply(false,"Step clear to refill");
            }
            self.edits.remove(i);
            return reply(true,"Ground restored");
        }

        let material = substance(probe);
        match (material,input["slot"].as_u64()) {
            (Substance::Stone | Substance::Bedrock,Some(0)) => return reply(false,"Use the pickaxe for rock"),
            (_,Some(1)) if !matches!(material, Substance::Stone | Substance::Bedrock) => return reply(false,"Use the shovel for soil"),
            _ => {}
        }
        if self.edits.len() >= 4096 {return reply(false,"Terrain edit storage full");}
        if index.is_some() {return reply(false,"Aim at another terrain piece");}
        self.edits.push(tile);
        reply(true,"Ground removed")
    }
    fn basis(&self) -> (D, D, D) {
        let n = self.anchor.unwrap_or([0., 1., 0.]);
        let east = unit(cross(
            if n[1].abs() > 0.95 {
                [0., 0., -1.]
            } else {
                [0., 1., 0.]
            },
            n,
        ));
        (east, n, cross(n, east))
    }
    fn global(&self, p: V) -> D {
        let (east, up, north) = self.basis();
        std::array::from_fn(|i| {
            up[i] * (RADIUS + p[1] as f64) + east[i] * p[0] as f64 + north[i] * p[2] as f64
        })
    }
    fn local(&self, p: D) -> V {
        let (e, u, n) = self.basis();
        let rel = std::array::from_fn(|i| p[i] - u[i] * RADIUS);
        [dot(rel, e) as f32, dot(rel, u) as f32, dot(rel, n) as f32]
    }
    fn disk(&self) -> Value {
        let particles:Vec<_>=self.bodies.iter().map(|(start,mass)|{let s=self.material.scene.as_ref().unwrap();json!({"positions":&s.p[*start..*start+4],"rest":&s.rest[*start..*start+4],"velocity":&s.velocity[*start..*start+4],"mass":mass})}).collect();
        json!({"version":1,"radius":RADIUS,"edits":self.edits,"anchor":self.anchor,"particles":particles,"removed_volume":self.removed,"debris_state":"suspended_on_reload"})
    }
    fn save(&self, host: &Host) -> Result<(), String> {
        if !self.persistent {
            return Ok(());
        }
        let dir = host.output_root().join("planet");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let tmp = dir.join(format!("surface-{}.tmp", std::process::id()));
        std::fs::write(&tmp, serde_json::to_vec_pretty(&self.disk()).unwrap())
            .map_err(|e| e.to_string())?;
        std::fs::rename(tmp, dir.join("surface.json")).map_err(|e| e.to_string())
    }
    fn load(&mut self, host: &mut Host) -> Result<(), String> {
        if self.loaded {
            return Ok(());
        }
        let config: Value = serde_json::from_slice(
            &std::fs::read(host.root().join("Procedures/terrain_session.json"))
                .map_err(|e| format!("terrain session: {e}"))?,
        )
        .map_err(|e| e.to_string())?;
        self.persistent = config["persist_excavation"] == true;
        let file = host.output_root().join("planet/surface.json");
        if !self.persistent {
            if file.exists() {
                let stamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| e.to_string())?
                    .as_nanos();
                std::fs::rename(
                    &file,
                    file.with_file_name(format!("archived-surface-{stamp}.json")),
                )
                .map_err(|e| format!("archive old digging: {e}"))?;
            }
            self.loaded = true;
            return Ok(());
        }
        if file.exists() {
            let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
            if bytes.len() > 2_000_000 {
                return Err("planet save exceeds size bound".into());
            }
            let v: Value =
                serde_json::from_slice(&bytes).map_err(|e| format!("invalid planet save: {e}"))?;
            if v["version"] != 1 || v["radius"] != RADIUS {
                return Err("unsupported planet save".into());
            }
            let edits = v["edits"]
                .as_array()
                .filter(|v| v.len() <= 4096)
                .ok_or("invalid edit count")?;
            if !crate::terrain::tool_cut::valid_edits(&v["edits"]) {
                return Err("invalid saved excavation".into());
            }
            self.edits = edits.clone();
            self.removed = v["removed_volume"]
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 0.)
                .ok_or("invalid material volume")?;
            if !v["anchor"].is_null() {
                let a = point(&v["anchor"])?;
                if a.iter().any(|v| !v.is_finite()) || (dot(a, a) - 1.).abs() > 1e-6 {
                    return Err("invalid debris frame".into());
                }
                self.anchor = Some(a);
            }
            let particles = v["particles"]
                .as_array()
                .filter(|v| v.len() <= 64)
                .ok_or("invalid debris count")?;
            if !particles.is_empty() && self.anchor.is_none() {
                return Err("debris has no local frame".into());
            }
            // A save is data, not permission to resume an older material trial.
            // Keep every fragment/velocity/rest pose, but do not collide it with
            // a different terrain generator or acquire the shovel's OGC context.
            self.rebuild(host, particles, false)?;
            self.active = false;
        }
        self.loaded = true;
        Ok(())
    }
    fn rebuild(
        &mut self,
        host: &mut Host,
        particles: &[Value],
        simulate: bool,
    ) -> Result<(), String> {
        if particles.is_empty() {
            return Ok(());
        }
        let mut scene = Scene::new("planet_debris");
        scene.radius = 0.012;
        scene.proxy="Coarse tetrahedral excavation fragments, local radial gravity; no granular soil, plastic compaction or fracture continuum.".into();
        let mut bodies = Vec::new();
        for b in particles {
            let mass = b["mass"]
                .as_f64()
                .filter(|m| m.is_finite() && *m > 0. && *m < 1e8)
                .ok_or("invalid fragment mass")? as f32;
            let points: Vec<V> =
                serde_json::from_value(b["positions"].clone()).map_err(|e| e.to_string())?;
            let rest: Vec<V> = serde_json::from_value(if b["rest"].is_null() {
                b["positions"].clone()
            } else {
                b["rest"].clone()
            })
            .map_err(|e| e.to_string())?;
            let velocities: Vec<V> =
                serde_json::from_value(b["velocity"].clone()).map_err(|e| e.to_string())?;
            if points.len() != 4
                || rest.len() != 4
                || rest
                    .iter()
                    .flatten()
                    .any(|v| !v.is_finite() || v.abs() > 1e6)
                || velocities.len() != 4
                || points
                    .iter()
                    .chain(&velocities)
                    .flatten()
                    .any(|v| !v.is_finite() || v.abs() > 1e6)
            {
                return Err("invalid fragment geometry".into());
            }
            let start = scene.p.len();
            for ((p, v), rest) in points.into_iter().zip(velocities).zip(rest) {
                scene.node(p, mass / 4., [0.47, 0.29, 0.13]);
                *scene.rest.last_mut().unwrap() = rest;
                *scene.velocity.last_mut().unwrap() = v;
            }
            for a in 0..4 {
                for b in a + 1..4 {
                    scene.edge(start + a, start + b, 1e-11);
                }
            }
            scene.tetra([start, start + 1, start + 2, start + 3]);
            scene.surface(
                start,
                vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
                false,
                false,
            );
            bodies.push((start, mass));
        }
        if simulate {
            // Half-metre contact mesh around the active material patch; reject distant
            // new excavation while these fragments are retained rather than lose mass.
            let start = scene.p.len();
            let n = 32usize;
            for z in 0..=n {
                for x in 0..=n {
                    let px = (x as f32 - 16.) * 0.5;
                    let pz = (z as f32 - 16.) * 0.5;
                    let normal = unit(self.global([px, 0., pz]));
                    let target = normal.map(|v| v * (RADIUS + self.height(normal)));
                    let local = self.local(target);
                    scene.node(local, 0., [0.23, 0.32, 0.15]);
                }
            }
            let mut faces = Vec::new();
            for z in 0..n {
                for x in 0..n {
                    let a = (z * (n + 1) + x) as u32;
                    faces.extend([
                        [a, a + 1, a + (n + 1) as u32],
                        [a + 1, a + (n + 2) as u32, a + (n + 1) as u32],
                    ]);
                }
            }
            scene.surface(start, faces, true, false);
        }
        scene.finish()?;
        if simulate && scene.intersection_count() > 0 {
            return Err("new debris placement intersects; excavation rejected".into());
        }
        if simulate {
            host.invoke("OGCConnectionSystem.Configure", scene.spec())?;
        }
        self.bodies = bodies;
        self.material = MaterialState {
            scene: Some(scene),
            ..Default::default()
        };
        self.steps = 0;
        self.active = simulate;
        Ok(())
    }
    fn excavate(&mut self, host: &mut Host, input: &Value) -> Result<Value, String> {
        let origin = point(&input["position"])?;
        let direction = unit(point(&input["forward"])?);
        // Trace the same displaced radial surface used by the renderer/collision.
        let distance = |t: f64| {
            let p = std::array::from_fn(|i| origin[i] + direction[i] * t);
            dot(p, p).sqrt() - RADIUS - self.height(unit(p))
        };
        let mut hit = None;
        let mut prev = distance(0.);
        for k in 1..=400 {
            let t = k as f64 * 0.25;
            let d = distance(t);
            if prev > 0. && d <= 0. {
                let (mut lo, mut hi) = (t - 0.25, t);
                for _ in 0..20 {
                    let m = (lo + hi) / 2.;
                    if distance(m) > 0. { lo = m } else { hi = m }
                }
                hit = Some((lo + hi) / 2.);
                break;
            }
            prev = d;
        }
        let Some(t) = hit else {
            return Ok(json!({"changed":false,"reason":"Aim at ground within 100 metres"}));
        };
        if self.edits.len() >= 128 || self.bodies.len() + 8 > 64 {
            return Ok(
                json!({"changed":false,"reason":"Initial material budget reached (64 retained fragments); no material was discarded"}),
            );
        }
        let normal = unit(std::array::from_fn(|i| origin[i] + direction[i] * t));
        if let Some(a) = self.anchor {
            let distance = (0..3)
                .map(|i| (a[i] - normal[i]).powi(2))
                .sum::<f64>()
                .sqrt()
                * RADIUS;
            if distance > 2. {
                return Ok(
                    json!({"changed":false,"reason":"This first physical debris patch supports excavation within 2m of its first cut"}),
                );
            }
        }
        let backup = self.clone();
        let mut particles = self.disk()["particles"].as_array().unwrap().clone();
        self.anchor = Some(self.anchor.unwrap_or(normal));
        let radius = 2.;
        let depth = 0.8;
        self.edits
            .push(json!({"normal":normal,"radius":radius,"depth":depth}));
        let volume = std::f64::consts::PI * radius * radius * depth / 2.;
        self.removed += volume;
        let center = self.local(normal.map(|v| v * (RADIUS + self.height(normal))));
        let size = (volume / 8. * 3. / 8.).cbrt() as f32;
        for i in 0..8 {
            let a = i as f32 / 8. * std::f32::consts::TAU;
            let mut c = add(
                center,
                [a.cos() * 4., 4. + (i % 2) as f32 * 2., a.sin() * 4.],
            );
            let spawn_normal = unit(self.global([c[0], 0., c[2]]));
            c[1] = self.local(spawn_normal.map(|v| v * (RADIUS + self.height(spawn_normal))))[1]
                + 4.
                + (i % 2) as f32 * 2.;
            let positions = [[1., 1., 1.], [-1., -1., 1.], [-1., 1., -1.], [1., -1., -1.]]
                .map(|v| add(c, scale(v, size)));
            particles.push(json!({"positions":positions,"velocity":vec![[a.cos()*0.3,1.,a.sin()*0.3];4],"mass":volume/8.*1600.}));
        }
        let placement = self.rebuild(host, &particles, true);
        let rejected = placement.is_err();
        let result = placement.and_then(|_| self.save(host));
        if let Err(error) = result {
            *self = backup;
            if self.active
                && let Some(scene) = &self.material.scene
            {
                host.invoke("OGCConnectionSystem.Configure", scene.spec())?;
            } else {
if host.invoke("OGCConnectionSystem.Ownership", Value::Null)?["owned_by_caller"] == true {
    host.invoke("OGCConnectionSystem.Release", Value::Null)?;
}
            }
            if rejected {
                return Ok(json!({"changed":false,"reason":error}));
            }
            return Err(error);
        }
        host.emit(json!({"event":"planet_excavated","edits":self.edits.len(),"removed_volume":volume,"removed_mass":volume*1600.,"fragments":8}))?;
        Ok(json!({"changed":true,"removed_volume":volume,"fragments":8}))
    }
    /// Read accepted fragment positions without advancing or initializing physics.
    pub(crate) fn acoustic_cells(&self) -> Vec<[[f64;3];4]> {
        self.material.scene.as_ref().map(|scene| self.bodies.iter().map(|(start,_)| {
            std::array::from_fn(|i| self.global(scene.p[start+i]))
        }).collect()).unwrap_or_default()
    }
    fn frame(&self) -> Value {
        let (east, up, north) = self.basis();
        let sun = unit([0.4, 0.8, 0.2]);
        let light_direction = [
            dot(east, sun) as f32,
            dot(up, sun) as f32,
            dot(north, sun) as f32,
        ];
        let vertices: Vec<[f64; 6]> = self
            .material
            .scene
            .as_ref()
            .map(|s| {
                self.bodies
                    .iter()
                    .flat_map(|(start, _)| {
                        [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]
                            .into_iter()
                            .flat_map(move |f| {
                                let [a, b, c] = f.map(|i| s.p[start + i]);
                                let normal = super::math::unit(super::math::cross(
                                    super::math::sub(b, a),
                                    super::math::sub(c, a),
                                ));
                                let light = (0.3
                                    + 0.7 * super::math::dot(normal, light_direction).abs())
                                    as f64;
                                f.map(|i| {
                                    let p = self.global(s.p[start + i]);
                                    [p[0], p[1], p[2], 0.47 * light, 0.29 * light, 0.13 * light]
                                })
                            })
                    })
                    .collect()
            })
            .unwrap_or_default();
        json!({"edits":self.edits,"debris":vertices,"active":self.active,"removed_volume":self.removed,"removed_mass":self.removed*1600.,"fragments":self.bodies.len(),"physics":self.material.report()})
    }
}
pub(crate) fn valid(v: &Value) -> bool {
    ["position", "forward"].iter().all(|k| {
        v[*k].as_array().is_some_and(|a| {
            a.len() == 3
                && a.iter()
                    .all(|v| v.as_f64().is_some_and(|x| x.is_finite() && x.abs() < 1e10))
        })
    })
}
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let mut state = std::mem::take(&mut host.planet);
    let result = (|| {
        // Cleanup cannot initialize gameplay, parse saves, or reacquire contact
        // resources after an unrelated graphics/preparation failure.
        if !state.loaded && matches!(hook, "Save" | "SuspendMaterial") {
            return Ok(Value::Null);
        }
        if !state.loaded {
            let mut loaded = State::default();
            loaded.load(host)?;
            state = loaded;
        }
        match hook {
"Reset" => {
    if host.invoke("OGCConnectionSystem.Ownership", Value::Null)?["owned_by_caller"] == true {
        host.invoke("OGCConnectionSystem.Release", Value::Null)?;
    }
                let persistent = state.persistent;
                let water_collision=std::mem::take(&mut state.water_collision);
                state = State {
                    water_collision,
                    loaded: true,
                    persistent,
                    ..Default::default()
                };
                state.save(host)?;
                Ok(Value::Null)
            }
            "ReleaseMaterials" => {state.edits.retain(|e|e["domain"].is_null() || e["mined"]==true);Ok(Value::Null)}
            "RefineMaterial" => {
                if state.edits.len() >= 4096 { return Err("material world storage limit reached".into()); }
                state.edits.push(input["edit"].clone());
                // This replaces static geometry with an occupied physical material
                // domain. It is not excavation and does not increment removed mass.
                Ok(Value::Null)
            }
            "MineCell" => state.mine_cell(input),
            "BladeCut" => {
                if state.edits.len() >= 128 {
                    return Err("terrain edit budget reached".into());
                }
                state.edits.push(input["edit"].clone());
                state.removed += input["volume"].as_f64().unwrap();
                state.save(host)?;
                Ok(Value::Null)
            }
            "CollisionSnapshot" => {let position=point(&input["position"])?;let mut frame=state.frame();frame["underground"]=if host.system_enabled("CaveSystem"){host.invoke("CaveSystem.Snapshot",Value::Null)?}else{Value::Null};if frame["underground"].is_object(){frame["underground"]["material_domains"]=json!(state.edits);}let soil=Value::Null;let material=host.invoke("PhysicsSystem.GameCollision",Value::Null)?;state.water_collision.capture_material(position,&frame,&soil,&material)},
            "Frame" => {
                let mut frame=state.frame();
                frame["underground"]=if host.system_enabled("CaveSystem") {
                    host.invoke("CaveSystem.Snapshot",Value::Null)?
                } else {Value::Null};
                if frame["underground"].is_object(){frame["underground"]["material_domains"]=json!(state.edits);}
                Ok(frame)
            },
            "SuspendMaterial" => {
                state.save(host)?;
                state.active = false;
if host.invoke("OGCConnectionSystem.Ownership", Value::Null)?["owned_by_caller"] == true {
    host.invoke("OGCConnectionSystem.Release", Value::Null)?;
}
                Ok(Value::Null)
            }
            "Height" => {
                let position=point(&input["position"])?;let eye=input["eye_height"].as_f64().unwrap_or(2.);
                let camera=if input["camera_eye"].is_null()||state.edits.iter().any(|e|crate::terrain::tool_cut::inside(unit(position),e)){None}else{Some(point(&input["camera_eye"])?) };
                let cave=if dot(position,position).sqrt()>RADIUS*0.5&&host.system_enabled("CaveSystem") {host.invoke("CaveSystem.Support",json!({"position":position,"eye_height":eye,"edits":state.edits}))?["height"].as_f64()}else{None};
                // Cave support replaces the rendered surface height. Do not
                // construct a second terrain interpolation just to discard it.
                let base=if cave.is_none(){base_query_height(&state,position,camera)}else{0.};
                let dynamic=host.invoke("PhysicsSystem.GameHeight",json!({"position":position,"eye_height":eye}))?["height"].as_f64();
                Ok(json!({"height":finish_query_height(&state,position,base,cave,dynamic),"active":state.active}))
            }
            "Excavate" => state.excavate(host, input),
            "Advance" => {
                if state.active {
                    let start = std::time::Instant::now();
                    for _ in 0..4 {
                        if let Err(e) = state.material.advance_safe(host, 1. / 240., 0) {
                            state.material.halt = Some(e);
                            state.active = false;
                            break;
                        }
                    }
                    state.steps += 1;
                    state.material.frames += 1;
                    let elapsed = start.elapsed().as_secs_f64() * 1000.;
                    state.material.sum_ms += elapsed;
                    state.material.max_ms = state.material.max_ms.max(elapsed);
                    if let Some(scene) = &state.material.scene {
                        let speed = scene
                            .velocity
                            .iter()
                            .zip(&scene.weights)
                            .filter(|(_, w)| **w > 0.)
                            .map(|(v, _)| dot(v.map(f64::from), v.map(f64::from)))
                            .fold(0f64, f64::max);
                        if state.steps > 120 && speed < 0.0025 {
                            state.active = false;
                        }
                        if state.steps > 600 {
                            state.material.halt=Some("Initial 10-second material budget reached; last valid state retained".into());
                            state.active = false;
                        }
                    }
                    host.emit(json!({"event":"planet_material_step","frame":state.steps,"halted":state.material.halt.is_some()}))?;
                }
                Ok(state.frame())
            }
            "Save" => {
                state.save(host)?;
if host.invoke("OGCConnectionSystem.Ownership", Value::Null)?["owned_by_caller"] == true {
    host.invoke("OGCConnectionSystem.Release", Value::Null)?;
}
                Ok(Value::Null)
            }
            _ => Err("unknown planet hook".into()),
        }
    })();
    host.planet = state;
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn excavation_profile_and_mass_are_bounded() {
        let mut s = State::default();
        s.edits
            .push(json!({"normal":[0.,1.,0.],"radius":2.,"depth":0.8}));
        assert_eq!(s.height([0., 1., 0.]), -0.8);
        let outside = unit([3. / RADIUS, 1., 0.]);
        assert_eq!(s.height(outside), crate::terrain::height(outside));
        let inside=unit([1. / RADIUS, 1., 0.]);
        let removed_depth=crate::terrain::height(inside)-s.height(inside);
        assert!((removed_depth-0.6).abs()<1e-6,
            "one metre from the centre, a two-metre-radius 0.8m cut removes 0.6m relative to uncut terrain");
    }
    #[test]
    fn planet_coordinates_retain_sub_metre_precision() {
        let s = State {
            anchor: Some([0., 1., 0.]),
            ..Default::default()
        };
        let p = [0.125, -0.5, 2.25];
        let q = s.local(s.global(p));
        for i in 0..3 {
            assert!((q[i] - p[i]).abs() < 1e-5);
        }
    }
}

#[cfg(test)]
mod mining_checks {
    use super::*;
    #[test]
    fn mining_selects_existing_piece_independent_of_aim_and_refills_it() {
        use crate::terrain::{BEDROCK_ELEVATION, material_domain::Domain, tetrahedral};
        let p = [0.32,RADIUS+BEDROCK_ELEVATION-0.3,0.32];
        let tile = tetrahedral::at(p).unwrap();
        let d = Domain::parse(&tile).unwrap();
        let v = d.tetra.unwrap().map(|p|d.world(p));
        let center: D = std::array::from_fn(|k|v.iter().map(|p|p[k]).sum::<f64>()/4.);
        let mut a=State::default(); let mut b=State::default();
        for (state,dir) in [(&mut a,[0.,-1.,0.]),(&mut b,[1.,0.,0.])] {
            let hit: D=std::array::from_fn(|k|center[k]-dir[k]*0.008);
            let eye: D=std::array::from_fn(|k|hit[k]-dir[k]*4.);
            let mut input=json!({"point":hit,"eye":eye,"direction":dir,"slot":1,"fill":false});
            assert_eq!(state.mine_cell(&input).unwrap()["changed"],true);
            assert_eq!(state.edits[0]["tile"],tile["tile"]);
            assert_eq!(state.mine_cell(&input).unwrap()["changed"],false);
            assert_eq!(state.edits.len(),1);
            input["fill"]=json!(true); input["point"]=json!(center);
            assert_eq!(state.mine_cell(&input).unwrap()["changed"],true);
            assert!(state.edits.is_empty());
        }
    }
    #[test]
    fn mine_bedrock_and_restore_its_collision() {
        use crate::terrain::{BEDROCK_ELEVATION,material_domain};
        let hit = [0.32,RADIUS+BEDROCK_ELEVATION-0.2,0.32];
        let mut input = json!({"point":hit,"eye":[1.52,hit[1]+2.,0.32],
            "direction":[-0.5145,-0.8575,0.],"slot":0,"fill":false});
        let mut state = State::default();
        assert_eq!(state.mine_cell(&input).unwrap()["changed"],false);
        input["slot"] = json!(1);
        assert_eq!(state.mine_cell(&input).unwrap()["changed"],true);
        let domains = material_domain::from_edits(&state.edits);
        let vertices = domains[0].tetra.unwrap().map(|p|domains[0].world(p));
        let center = std::array::from_fn(|k|vertices.iter().map(|p|p[k]).sum::<f64>()/4.);
        assert!(material_domain::carve_bedrock(center,-10.,&domains)>0.);
        input["fill"] = json!(true);
        input["point"] = json!(center);
        assert_eq!(state.mine_cell(&input).unwrap()["changed"],true);
        assert!(material_domain::carve_bedrock(center,-10.,&[])<0.);
    }
}

fn base_query_height(state:&State,position:D,camera:Option<D>)->f64{
    let normal=unit(position);let in_cut=state.edits.iter().any(|e|crate::terrain::tool_cut::inside(normal,e));
    if in_cut{state.height(normal)}else{camera.and_then(|camera|crate::terrain::rendered_height(camera,normal,|n|state.height(n))).unwrap_or_else(||state.height(normal))}
}
fn finish_query_height(state:&State,position:D,base:f64,cave:Option<f64>,dynamic:Option<f64>)->f64{
    let height=cave.unwrap_or(base);let height=dynamic.map_or(height,|h|h.max(height));let floor=crate::terrain::BEDROCK_ELEVATION;
    if height<floor&&!state.edits.iter().any(|e|e["mined"]==true&&crate::terrain::material_domain::Domain::parse(e).is_some_and(|d|d.distance(position)<4.)){floor}else{height}
}
pub(crate) fn heights_numeric(host:&mut Host,positions:&[D],camera:D)->Result<Vec<f64>,String>{
    if positions.len()>32||positions.iter().flatten().chain(camera.iter()).any(|n|!n.is_finite()){return Err("invalid bounded surface height batch".into());}
    let mut state=std::mem::take(&mut host.planet);let result=(||{state.load(host)?;
        let world:Vec<_>=positions.iter().enumerate().filter(|(_,p)|dot(**p,**p).sqrt()>RADIUS*0.5).map(|(i,&p)|(i,p)).collect();
        let mut caves=vec![None;positions.len()];if host.system_enabled("CaveSystem")&&!world.is_empty(){let points:Vec<_>=world.iter().map(|(_,p)|*p).collect();let values=host.cave_supports_numeric(&points,2.,&state.edits)?;for ((i,_),height)in world.iter().zip(values){caves[*i]=height;}}
        positions.iter().enumerate().map(|(i,&p)|{let base=if caves[i].is_none(){base_query_height(&state,p,Some(camera))}else{0.};let dynamic=host.invoke("PhysicsSystem.GameHeight",json!({"position":p,"eye_height":2.}))?["height"].as_f64();Ok(finish_query_height(&state,p,base,caves[i],dynamic))}).collect()
    })();host.planet=state;result
}
