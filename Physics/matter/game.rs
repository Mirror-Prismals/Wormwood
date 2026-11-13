//! Player interaction with the shared material world. Unstrained volumes are
//! implicit; disturbed volumes retain explicit matter, topology and contact.
//! Refinement samples the world field; it never instantiates workshop scenes.
use super::*;
#[path = "game_hand.rs"]
pub(super) mod hand;
use crate::terrain::{self, material_domain::Domain};
type D = [f64; 3];
fn d(v: &Value) -> Result<D, String> {
    serde_json::from_value(v.clone()).map_err(|_| "material game vector required".into())
}
fn addd(a: D, b: D) -> D {
    std::array::from_fn(|k| a[k] + b[k])
}
fn subd(a: D, b: D) -> D {
    std::array::from_fn(|k| a[k] - b[k])
}
fn muld(a: D, s: f64) -> D {
    a.map(|x| x * s)
}
fn dotd(a: D, b: D) -> f64 {
    (0..3).map(|k| a[k] * b[k]).sum()
}
fn lend(a: D) -> f64 {
    dotd(a, a).sqrt()
}
fn unitd(a: D) -> D {
    muld(a, 1. / lend(a).max(1e-9))
}
fn crossd(a: D, b: D) -> D {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
const TOOL_NAMES: [&str; 4] = ["Shovel", "Pickaxe", "Axe", "Bucket"];
const TOOL_CASES: [&str; 4] = ["matter_dirt", "matter_stone", "matter_wood", "matter_sand"];
const COLLISION_BLOCK_TRIANGLES: usize = 32;
#[derive(Clone, Copy)]
struct CollisionBounds {
    min: V,
    max: V,
}
impl CollisionBounds {
    fn from_triangles(triangles: &[[V; 3]]) -> Option<Self> {
        if triangles.is_empty() {
            return None;
        }
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for triangle in triangles {
            for p in triangle {
                for k in 0..3 {
                    if !p[k].is_finite() {
                        return None;
                    }
                    min[k] = min[k].min(p[k]);
                    max[k] = max[k].max(p[k]);
                }
            }
            let [a, b, c] = triangle.map(|p| p.map(f64::from));
            let ab = subd(b, a);
            let ac = subd(c, a);
            let bc = subd(c, b);
            let edge_squared = dotd(ab, ab).max(dotd(ac, ac)).max(dotd(bc, bc));
            let normal = crossd(ab, ac);
            let tolerance = 64. * f32::EPSILON as f64;
            // The existing closest-point routine can return NaN for collapsed
            // or poorly conditioned faces, even outside their geometric box.
            // Keep those faces on the exhaustive path instead of hiding that
            // existing outcome through spatial rejection.
            if dotd(normal, normal) <= edge_squared * edge_squared * tolerance * tolerance {
                return None;
            }
        }
        Some(Self { min, max })
    }
    fn excludes_sphere(&self, center: V, radius: f64) -> bool {
        if !radius.is_finite() || radius < 0. || center.iter().any(|v| !v.is_finite()) {
            return false;
        }
        // Queries and vertices share the same local f32 coordinates. Use f64
        // comparisons and an outward margin so boundary rounding cannot remove
        // a contact accepted by the existing closest-point calculation.
        (0..3).any(|k| {
            let scale = (center[k] as f64).abs()
                .max((self.min[k] as f64).abs())
                .max((self.max[k] as f64).abs())
                .max(1.);
            let padding = radius + scale * f32::EPSILON as f64 * 64.;
            (center[k] as f64) < self.min[k] as f64 - padding
                || (center[k] as f64) > self.max[k] as f64 + padding
        })
    }
}
#[derive(Clone)]
struct Frame {
    origin: D,
    axes: [D; 3],
}
impl Frame {
    fn at(origin: D) -> Self {
        let up = unitd(origin);
        let x = unitd(crossd([0., 0., -1.], up));
        Self {
            origin,
            axes: [x, up, crossd(x, up)],
        }
    }
    fn local(&self, p: D) -> V {
        let p = subd(p, self.origin);
        self.axes.map(|a| dotd(a, p) as f32)
    }
    fn vector(&self, p: D) -> V {
        self.axes.map(|a| dotd(a, p) as f32)
    }
    fn world(&self, p: V) -> D {
        std::array::from_fn(|k| {
            self.origin[k] + (0..3).map(|j| self.axes[j][k] * p[j] as f64).sum::<f64>()
        })
    }
}
struct Region {
    frame: Frame,
    world: World,
    domain: Option<Domain>,
    kind: &'static str,
    mesh: Vec<[f32; 6]>,
    tree: Option<u64>,
    collision: Vec<[V; 3]>,
    collision_bounds: Option<CollisionBounds>,
    collision_blocks: Vec<Option<CollisionBounds>>,
    sleeping: bool,
    bounds: (V,f64),
    // Inactive regions retain their native contact topology and neighbour cache.
    // The loaded region's context lives in Host for the registered OGC calls.
    contact: Option<crate::base_system::layer_one::ogc_connection_system::State>,
}
impl Clone for Region {
    fn clone(&self) -> Self {
        // A state checkpoint owns its matter, not a second native handle. The
        // restored checkpoint rebuilds contact lazily on first use.
        Self { frame:self.frame.clone(), world:self.world.clone(), domain:self.domain.clone(),
            kind:self.kind, mesh:self.mesh.clone(), tree:self.tree, collision:self.collision.clone(),
            collision_bounds:self.collision_bounds, collision_blocks:self.collision_blocks.clone(),
            sleeping:self.sleeping, bounds:self.bounds, contact:None }
    }
}
impl Region {
    fn refresh(&mut self) {
        let mut out = self.world.skin.clone();
        if let Some(s) = &self.world.solid {
            s.render(&mut out);
        }
        if let Some(s) = &self.world.turf {
            s.render(&mut out);
        }
        if let Some(s) = &self.world.layers {
            s.render(&mut out);
        }
        self.mesh = out;
        let lo:V=std::array::from_fn(|k|self.mesh.iter().map(|v|v[k]).fold(f32::INFINITY,f32::min));
        let hi:V=std::array::from_fn(|k|self.mesh.iter().map(|v|v[k]).fold(f32::NEG_INFINITY,f32::max));
        let center=scale(add(lo,hi),0.5);
        self.bounds=(center,length(sub(hi,lo)) as f64*0.5);
        self.replace_collision(self.world.collision());
    }
    fn replace_collision(&mut self, triangles: Vec<[V; 3]>) {
        // Every accepted skin replacement refreshes both levels together. The
        // blocks retain source order; no solver/contact ordering changes.
        self.collision_bounds = CollisionBounds::from_triangles(&triangles);
        self.collision_blocks.clear();
        self.collision_blocks.extend(triangles.chunks(COLLISION_BLOCK_TRIANGLES)
            .map(CollisionBounds::from_triangles));
        self.collision = triangles;
    }
    fn hit(&self, eye: D, dir: D, range: f64) -> Option<(D, D, f64)> {
        let o = self.frame.local(eye);
        let v = self.frame.vector(dir);
        let mut best = None;
        let mut near = range as f32;
        for &[a, b, c] in &self.collision {
            if let Some(h) = ray_triangle(o, v, a, b, c) {
                if h < near {
                    near = h;
                    let n = unit(cross(sub(b, a), sub(c, a)));
                    let n = if dot(n, v) > 0. { scale(n, -1.) } else { n };
                    let nw = std::array::from_fn(|k| {
                        (0..3).map(|j| self.frame.axes[j][k] * n[j] as f64).sum()
                    });
                    best = Some((self.frame.world(add(o, scale(v, h))), nw, h as f64));
                }
            }
        }
        best
    }
}
fn ray_triangle(o: V, d: V, a: V, b: V, c: V) -> Option<f32> {
    let e = sub(b, a);
    let f = sub(c, a);
    let p = cross(d, f);
    let det = dot(e, p);
    if det.abs() < 1e-8 {
        return None;
    }
    let inv = 1. / det;
    let s = sub(o, a);
    let u = dot(s, p) * inv;
    if !(0. ..=1.).contains(&u) {
        return None;
    }
    let q = cross(s, e);
    let v = dot(d, q) * inv;
    if v < 0. || u + v > 1. {
        return None;
    }
    let h = dot(f, q) * inv;
    (h > 0.002).then_some(h)
}
#[derive(Clone)]
struct Stroke {
    point: D,
    normal: D,
    time: f32,
    power: f32,
    pour: bool,
    start: hand::Pose,
}
#[derive(Clone)]
struct SuspendedTool {
    region: Option<usize>,
    tool: Option<Tool>,
}
#[derive(Default)]
pub(crate) struct Game {
    regions: Vec<Region>,
    active: Option<usize>,
    selected: usize,
    held: bool,
    charge: f32,
    stroke: Option<Stroke>,
    accumulator: f32,
    status: String,
    frames: u64,
    caption_clock: f32,
    last_pour: bool,
    standalone: Option<Tool>,
    last_eye: D,
    ground_eye: D,
    background_cursor: usize,
    loaded: Option<usize>,
    last_reach: Value,
    standalone_frame: Option<Frame>,
    suspended_tool: Option<SuspendedTool>,
}
impl Clone for Game {
    fn clone(&self) -> Self {
        Self { regions:self.regions.clone(), active:self.active, selected:self.selected,
            held:self.held, charge:self.charge, stroke:self.stroke.clone(), accumulator:self.accumulator,
            status:self.status.clone(), frames:self.frames, caption_clock:self.caption_clock,
            last_pour:self.last_pour, standalone:self.standalone.clone(), last_eye:self.last_eye,
            ground_eye:self.ground_eye, background_cursor:self.background_cursor, loaded:None, last_reach:self.last_reach.clone(), standalone_frame:self.standalone_frame.clone(), suspended_tool:self.suspended_tool.clone() }
    }
}
pub(crate) fn valid(input: &Value) -> bool {
    ["position", "aim", "up"]
        .iter()
        .all(|k| d(&input[*k]).is_ok_and(|v| v.iter().all(|n| n.is_finite())))
        && input["dt"]
            .as_f64()
            .is_some_and(|v| v.is_finite() && (0. ..=0.25).contains(&v))
        && crate::base_system::layer_two::inverse_kinematics_system::valid_tool_reach(&input["tool_reach"])
        && (input["tools_suspended"].is_null() || input["tools_suspended"].is_boolean())
}
impl Game {
    fn stash_contact(&mut self, host: &mut Host) {
        if let Some(i) = self.loaded.take() {
            self.regions[i].contact = Some(std::mem::take(&mut host.ogc));
        }
    }
    fn load_contact(&mut self, host: &mut Host, i: usize) -> Result<(), String> {
        if self.loaded == Some(i) { return Ok(()); }
        host.ogc.assert_owner("PhysicsSystem")?;
        self.stash_contact(host);
        if let Some(context) = self.regions[i].contact.take() {
            host.ogc = context;
        } else {
            self.regions[i].world.configure(host)?;
        }
        self.loaded = Some(i);
        Ok(())
    }
    pub(crate) fn craft_tree(&mut self, input:&Value)->Result<Value,String> {
        let position:D=serde_json::from_value(input["position"].clone()).map_err(|_|"craft position required")?;
        if position.iter().any(|v|!v.is_finite()){return Err("finite craft position required".into());}
        let requested=input["site_id"].as_u64();
        let candidate=self.regions.iter().enumerate().filter_map(|(i,r)|{
            let id=r.tree?;if requested.is_some_and(|v|v!=id){return None;}
            let center=r.frame.world(r.world.solid.as_ref()?.fallen_crown()?);
            let distance=lend(subd(center,position));
            (distance<=4.).then_some((i,id,center,distance))
        }).min_by(|a,b|a.3.total_cmp(&b.3));
        let Some((i,id,center,_))=candidate else{return Ok(Value::Null);};
        if input["consume"]==true {
            // Keep the transferred vegetation ID suppressed, and retire the entire
            // physical tree region once. No standing/generated tree is substituted.
            self.regions[i].mesh.clear();self.regions[i].collision.clear();
            self.regions[i].world.solid=None;self.regions[i].world.skin.clear();
            self.regions[i].world.topology_dirty=true;
            self.regions[i].tree=None;self.regions[i].sleeping=true;
            self.regions[i].collision_bounds=None;self.regions[i].collision_blocks.clear();
            if self.active==Some(i){self.stroke=None;self.held=false;}
        }
        Ok(json!({"site_id":id,"position":center}))
    }
    #[cfg(test)]
    pub(crate) fn diagnostics(&self) -> Value {
        json!(self.regions.iter().map(|p| json!({"kind":p.kind,"sleeping":p.sleeping,
            "vertices":p.mesh.len(),"collision_triangles":p.collision.len(),
            "support_triangles":p.world.tool.ground_faces.len(),"physics":p.world.report()})).collect::<Vec<_>>())
    }
    pub fn pause(&mut self) {
        self.held = false;
        self.charge = 0.;
        self.stroke = None;
        self.accumulator = 0.;
    }
    pub fn frame(&self) -> Value {
        let payload=self.tool_payload();
        json!({"vertices":self.vertices(),"domains":self.regions.len(),"status":self.status,"selected":self.selected,"active":self.stroke.is_some(),"frames":self.frames,
            "tool_payload":payload,"can_stow_tool":payload==0,"tools_suspended":self.suspended_tool.is_some()})
    }
    fn tool_payload(&self) -> usize {
        self.active.map_or(0, |i| self.regions[i].world.payload_count())
    }
    fn suspend_tool(&mut self) -> Result<(), String> {
        if self.suspended_tool.is_some() { return Ok(()); }
        // A shovel/bucket's support cannot disappear from under its load.
        // Phone pickup checks the same condition before taking either hand.
        if self.tool_payload() != 0 {
            return Err("empty the material tool before stowing it".into());
        }
        let saved = if let Some(i) = self.active {
            let region = &mut self.regions[i];
            let top=region.collision.iter().flatten().map(|p|p[1])
                .chain(region.world.positions().into_iter().map(|p|p[1]))
                .fold(0f32,f32::max)+40.;
            let world = &mut region.world;
            let saved = world.tool.clone();
            // The native material solvers retain a fixed contact topology.
            // Use their existing inactive-tool parking convention, above all
            // current material/support, without a grasp or moving carry goal.
            let parked=world.tool.model.iter().map(|p|add(*p,[0.,top,0.])).collect::<Vec<_>>();
            world.tool.initialize_pose(parked.clone());
            world.tool.manual=Some(parked);
            world.tool.grasp=None;
            // A positive floor avoids the actuator's 0/0 force-limit branch;
            // zero velocity plus the fixed current goal supplies no motion.
            world.tool.force_limit=Some(f32::EPSILON);
            Some(saved)
        } else {
            self.standalone.take()
        };
        self.suspended_tool=Some(SuspendedTool{region:self.active,tool:saved});
        self.pause();
        self.last_pour=false;
        Ok(())
    }
    fn resume_tool(&mut self, eye:D, direction:D, up:D, reach:&Value) -> Result<(),String> {
        let Some(saved)=self.suspended_tool.as_ref() else{return Ok(());};
        if saved.region!=self.active || saved.region.is_some_and(|i|i>=self.regions.len()) {
            return Err("suspended material tool ownership changed".into());
        }
        if let Some(mut tool)=saved.tool.clone() {
            let frame=saved.region.map_or_else(||Frame::at(eye),|i|self.regions[i].frame.clone());
            let pose=hand::carry(&tool,&frame,eye,direction,up,reach).vertices(&tool);
            // It was empty when stowed. Reattach at the current hand position,
            // never sweep its old pose across all the distance walked with a phone.
            tool.initialize_pose(pose.clone());
            tool.manual=Some(pose);
            tool.grasp=None;
            tool.force_limit=Some(450.);
            if let Some(i)=saved.region {self.regions[i].world.tool=tool;} else {
                self.standalone=Some(tool);
                self.standalone_frame=Some(frame);
            }
        }
        self.suspended_tool=None;
        self.pause();
        self.last_pour=false;
        Ok(())
    }
    fn advance_background(&mut self,host:&mut Host)->Result<(),String> {
        if !self.regions.is_empty() {
            let i=self.background_cursor%self.regions.len();
            self.background_cursor+=1;
            if self.active!=Some(i)&&!self.regions[i].sleeping {
                self.load_contact(host,i)?;
                let p=&mut self.regions[i];
                if let Err(e)=p.world.advance_frame(host){p.world.fail(e);p.sleeping=true;}
                p.sleeping|=!p.world.moving();
                p.refresh();
                if let Some(a)=self.active{self.load_contact(host,a)?;}
            }
        }
        Ok(())
    }
    fn advance_active_material(&mut self,host:&mut Host,dt:f32,altitude:f64)->Result<(),String> {
        let Some(i)=self.active else{return Ok(());};
        let p=&mut self.regions[i];
        let air=climate::now(altitude);
        if p.world.thermal.is_none()&&p.world.particles.iter().any(|p|p.kind==Kind::Water) {
            p.world.thermal=Some(thermal::Thermal::water(&p.world,air));
        }
        if let Some(t)=&mut p.world.thermal{t.air=air;}
        self.accumulator=(self.accumulator+dt).min(0.05);
        while self.accumulator>=1./60. {
            if let Err(e)=p.world.advance_frame(host) {
                p.world.fail(e.clone());self.status=format!("Material stopped: {e}");self.stroke=None;break;
            }
            self.accumulator-=1./60.;
        }
        p.refresh();
        Ok(())
    }
    pub fn vertices(&self) -> Vec<[f64;6]> {
        self.gather_vertices(|_|true)
    }
    pub fn visible_vertices(&self,eye:D,forward:D,up:D,aspect:f64)->Result<Vec<[f64;6]>,String> {
        if eye.iter().chain(&forward).chain(&up).any(|v|!v.is_finite()) || !(0.2..=8.).contains(&aspect) {
            return Err("invalid material appearance view".into());
        }
        let forward=unitd(forward);
        let right=unitd(crossd(forward,up));
        let up=crossd(right,forward);
        let vertical=35f64.to_radians().tan();
        let horizontal=vertical*aspect;
        Ok(self.gather_vertices(|p| {
            let center=subd(p.frame.world(p.bounds.0),eye);
            let radius=p.bounds.1;
            let z=dotd(center,forward);
            // Sphere/plane rejection is conservative, including material which
            // crosses the near plane. No simulation or collision is culled.
            z+radius>=0.05 && (cfg!(target_os="ios") || dotd(center,right).abs()<=z*horizontal+radius*(1.+horizontal*horizontal).sqrt())
                && dotd(center,up).abs()<=z*vertical+radius*(1.+vertical*vertical).sqrt()
        }))
    }
    fn gather_vertices(&self,visible:impl Fn(&Region)->bool) -> Vec<[f64;6]> {
        let mut mesh = Vec::<[f64; 6]>::new();
        for p in self.regions.iter().filter(|p| p.kind != "water" && visible(p)) {
            for v in &p.mesh {
                let w = p.frame.world([v[0], v[1], v[2]]);
                mesh.push([w[0], w[1], w[2], v[3] as f64, v[4] as f64, v[5] as f64]);
            }
        }
        mesh
    }
    pub fn height(&self, p: D, eye: f64) -> Option<f64> {
        let up = unitd(p);
        let start = addd(p, muld(up, 0.5));
        let mut height = None;
        for patch in self.regions.iter().filter(|p| p.kind != "water") {
            if let Some((hit, _, _)) = patch.hit(start, muld(up, -1.), eye + 1.) {
                let h = lend(hit) - terrain::RADIUS;
                if h <= lend(p) - terrain::RADIUS - eye + 0.6 {
                    height = Some(height.map_or(h, |v: f64| v.max(h)));
                }
            }
        }
        height
    }
    pub fn resolve_body(&self, input: &Value) -> Result<Value, String> {
        let old = d(&input["previous"])?;
        let requested = d(&input["position"])?;
        let eye = input["eye_height"].as_f64().unwrap_or(1.8);
        let up = unitd(requested);
        let delta = subd(requested, old);
        let steps = (lend(delta) / 0.10).ceil().clamp(1., 32.) as usize;
        let mut position = old;
        let mut normals = Vec::<D>::new();
        for step in 1..=steps {
            let target = addd(old, muld(delta, step as f64 / steps as f64));
            let mut accepted = target;
            for _ in 0..3 {
                for p in self.regions.iter().filter(|p| p.kind != "water") {
                    for offset in [-eye + 0.27, -eye * 0.5, -0.23] {
                        let center = p.frame.local(addd(accepted, muld(up, offset)));
                        if p.collision_bounds.as_ref().is_some_and(|b| b.excludes_sphere(center, 0.235)) {
                            continue;
                        }
                        for (block, triangles) in p.collision.chunks(COLLISION_BLOCK_TRIANGLES).enumerate() {
                            if p.collision_blocks.get(block).and_then(Option::as_ref)
                                .is_some_and(|b| b.excludes_sphere(center, 0.235)) {
                                continue;
                            }
                            for triangle in triangles {
                                let q = crate::water::hydrology::closest(
                                    center.map(f64::from),
                                    triangle.map(|v| v.map(f64::from)),
                                );
                                let diff = subd(center.map(f64::from), q);
                                let distance = lend(diff);
                                if distance >= 0.235 {
                                    continue;
                                }
                                let normal = if distance > 1e-7 {
                                    unitd(diff)
                                } else {
                                    unitd(crossd(
                                        subd(triangle[1].map(f64::from), triangle[0].map(f64::from)),
                                        subd(triangle[2].map(f64::from), triangle[0].map(f64::from)),
                                    ))
                                };
                                let world = std::array::from_fn(|k| {
                                    (0..3).map(|j| p.frame.axes[j][k] * normal[j]).sum()
                                });
                                accepted = addd(accepted, muld(world, 0.235 - distance));
                                if normals.len() < 12 {
                                    normals.push(world);
                                }
                            }
                        }
                    }
                }
            }
            // A caught physical tool must come free before the body can leave
            // it behind. The arm drive stays finite; this only bounds reach.
            if let Some(i) = self.active {
                let p = &self.regions[i];
                if let Ok(body)=d(&self.last_reach["eye"]) {
                    let pose=hand::Pose::fit(&p.world.tool);
                    for _ in 0..8 { for (side,grip) in p.world.tool.wrists().iter().enumerate() {
                        if let Ok(shoulder)=d(&self.last_reach["shoulders"][side]) {
                            let hand=p.frame.world(pose.point(*grip).map(|v|v as f32));
                            let shoulder_offset=subd(shoulder,body);
                            let delta=subd(addd(accepted,shoulder_offset),hand);
                            let previous=lend(subd(addd(old,shoulder_offset),hand));
                            let limit=self.last_reach["arm_max"][side].as_f64().unwrap_or(0.7)-0.015;
                            let limit=limit.max(previous);
                            if lend(delta)>limit {
                                let normal=unitd(delta);
                                accepted=subd(accepted,muld(normal,lend(delta)-limit));
                                if normals.len()<12 {normals.push(muld(normal,-1.));}
                            }
                        }
                    }}
                }
            }
            position = accepted;
        }
        Ok(json!({"position":position,"normals":normals}))
    }
    pub fn water_sample(&self, input: &Value) -> Result<Value, String> {
        let point = d(&input["position"])?;
        for patch in self.regions.iter().filter(|p| p.kind == "water") {
            let q = patch.frame.local(point);
            if let Some(p) = patch
                .world
                .particles
                .iter()
                .filter(|p| p.active)
                .find(|p| length(sub(p.p, q)) < p.radius * 1.5)
            {
                let surface = patch
                    .world
                    .particles
                    .iter()
                    .filter(|p| {
                        p.active && (p.p[0] - q[0]).abs() < 0.08 && (p.p[2] - q[2]).abs() < 0.08
                    })
                    .map(|p| p.p[1] + p.radius)
                    .fold(q[1], f32::max);
                let surface_position = patch.frame.world([q[0], surface, q[2]]);
                let height = lend(surface_position) - terrain::RADIUS;
                let velocity: D = std::array::from_fn(|k| {
                    (0..3).map(|j| patch.frame.axes[j][k] * p.v[j] as f64).sum()
                });
                return Ok(
                    json!({"occupied":true,"coverage_known":true,"surface_position":surface_position,"surface_height":height,"mean_height":height,"bed_height":height-0.2,"shore_distance":0.1,"shore_gradient":[0.,0.],"shore_direction":patch.frame.axes[0],"downstream_direction":velocity,"wave_amplitude":0.,"kind":"river","frame":{"origin":patch.frame.origin,"right":patch.frame.axes[0],"up":patch.frame.axes[1],"forward":patch.frame.axes[2]},"local_point":q,"uv":[q[0],q[2]],"material":true}),
                );
            }
        }
        Ok(json!({"occupied":false,"coverage_known":false}))
    }
    pub fn water_frame(&self) -> Value {
        let mut vertices = Vec::new();
        for p in self.regions.iter().filter(|p| p.kind == "water") {
            for v in &p.mesh {
                vertices.push(p.frame.world([v[0], v[1], v[2]]));
            }
        }
        let audio_sources: Vec<_> = self.regions.iter().filter(|p| p.kind == "water").filter_map(|patch| {
            let mut total = [0.; 3];
            let mut speed = 0f64;
            let mut count = 0;
            for p in patch.world.particles.iter().filter(|p| p.active && length(p.v) > 0.08) {
                let position = patch.frame.world(p.p);
                for k in 0..3 { total[k] += position[k]; }
                speed += f64::from(length(p.v));
                count += 1;
            }
            (count > 0).then(|| json!({"position":total.map(|v|v/count as f64),"speed":speed/count as f64}))
        }).collect();
        json!({"vertices":vertices,"audio_sources":audio_sources})
    }
    pub fn collision_frame(&self) -> Value {
        let vertices: Vec<_> = self.collision_vertices().into_iter().map(|w|[w[0],w[1],w[2],0.,0.,0.]).collect();
        json!({"debris":vertices})
    }
    pub fn collision_vertices(&self) -> Vec<D> {
        let mut vertices = Vec::new();
        for p in self.regions.iter().filter(|p| p.kind != "water") {
            for t in &p.collision {
                for &v in t {
                    let w = p.frame.world(v);
                    vertices.push(w);
                }
            }
        }
        vertices
    }
    fn aim(&self, eye: D, dir: D, terrain: &Value) -> Option<(D, D, Option<usize>)> {
        let mut result = None;
        let mut near = if self.selected < 2 {4.5} else {2.45};
        for (i, p) in self.regions.iter().enumerate() {
            if let Some((point, n, t)) = p.hit(eye, dir, near) {
                near = t;
                result = Some((point, n, Some(i)));
            }
        }
        let field = world_field::Field::shared_for_aim(terrain).ok()?;
        let distance = |p: D| field.distance(p);
        let mut previous = 0.;
        for k in 1..=(near / 0.025) as usize {
            let t = k as f64 * 0.025;
            let p = addd(eye, muld(dir, t));
            if distance(p) <= 0. {
                let mut lo = previous;
                let mut hi = t;
                for _ in 0..16 {
                    let m = (lo + hi) * 0.5;
                    if distance(addd(eye, muld(dir, m))) > 0. {
                        lo = m
                    } else {
                        hi = m
                    }
                }
                let p = addd(eye, muld(dir, hi));
                let n = unitd(std::array::from_fn(|axis| {
                    let mut a = p;
                    let mut b = p;
                    a[axis] += 0.005;
                    b[axis] -= 0.005;
                    distance(a) - distance(b)
                }));
                return Some((p, n, None));
            }
            previous = t;
        }
        result
    }
    pub fn step(&mut self, host: &mut Host, input: &Value) -> Result<Value, String> {
        let eye = d(&input["position"])?;
        let direction = unitd(d(&input["aim"])?);
        let up = unitd(d(&input["up"])?);
        let dt = input["dt"].as_f64().unwrap().min(0.05) as f32;
        let reach = &input["tool_reach"];
        self.last_reach=reach.clone();
        self.last_eye = eye;
        self.frames += 1;
        if let Some(i) = self.active { self.load_contact(host, i)?; }
        if input["tools_suspended"]==true {
            self.suspend_tool()?;
            self.advance_background(host)?;
            let altitude=self.active.map_or(lend(eye)-terrain::RADIUS,|i|lend(self.regions[i].frame.origin)-terrain::RADIUS);
            self.advance_active_material(host,dt,altitude)?;
            return Ok(json!({"vertices":[],"tool_grip":{"active":false},"caption_changed":false,
                "selected":self.selected,"active":false,"charge":0.,"status":self.status,
                "tools_suspended":true,"tool_payload":0,"can_stow_tool":true}));
        }
        self.resume_tool(eye,direction,up,reach)?;
        let requested = input["tools"]["slot"]
            .as_u64()
            .filter(|i| *i < 4)
            .map(|i| i as usize);
        if let Some(slot) = requested {
            if slot != self.selected
                && self.stroke.is_none()
                && !self
                    .active
                    .is_some_and(|i| self.regions[i].world.payload_count() > 0)
            {
                self.selected = slot;
                self.standalone = None;
                self.standalone_frame = None;
                if let Some(i) = self.active {
                    let p = &mut self.regions[i];
                    let mut t = Tool::held(TOOL_CASES[slot]);
                    t.ground = p.world.tool.ground.clone();
                    t.ground_faces = p.world.tool.ground_faces.clone();
                    let pose = hand::carry(&t, &p.frame, eye, direction, up, reach).vertices(&t);
                    t.p = pose.clone();
                    t.manual = Some(pose);
                    p.world.tool = t;
                    p.world.configure(host)?;
                }
            }
        }
        if requested.is_some_and(|s| s != self.selected)
            && self
                .active
                .is_some_and(|i| self.regions[i].world.payload_count() > 0)
        {
            self.status = "Empty the tool first — right click / Pour".into();
        }
        let held = input["tools"]["work"] == true;
        let pour = input["tools"]["pour"] == true;
        let mining = self.selected < 2;
        // Keep the historical contact-solver replay available to the workshop benchmark.
        #[cfg(test)]
        let mining = mining && input["solver_benchmark"] != true;
        if !mining && held && self.stroke.is_none() {
            self.charge = (self.charge + dt).min(1.2);
        }
        let release = !mining && self.held && !held;
        let poured = pour && !self.last_pour;
        self.last_pour = pour;
        let mut hit = self.aim(eye, direction, &input["terrain"]);
        if mining && (held && !self.held || poured) {
            self.status = if let Some((point, _, None)) = hit {
                host.invoke("PlanetSurfaceSystem.MineCell",json!({"point":point,"direction":direction,
                    "eye":eye,"slot":self.selected,"fill":poured}))?["status"].as_str().unwrap_or("").to_owned()
            } else {"Aim at reachable terrain".into()};
        }
        // Standing timber is queried from the real vegetation state.
        let wood = if self.selected == 2 {
            Some(host.invoke(
                "VegetationSystem.QueryWood",
                json!({"position":eye,"radius":5.,"time":0.}),
            )?)
        } else {
            None
        };
        let mut tree_hit = None;
        if let Some(wood) = &wood {
            for branch in wood["wood"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|b| b["branch_id"] == 0)
            {
                let a = addd(eye, d(&branch["start"])?);
                let b = addd(eye, d(&branch["end"])?);
                let axis = subd(b, a);
                let axislen = dotd(axis, axis);
                let radius = branch["radius"].as_f64().unwrap_or(0.1);
                for k in 1..99 {
                    let distance = k as f64 * 0.025;
                    if hit
                        .as_ref()
                        .is_some_and(|(p, _, _)| lend(subd(*p, eye)) < distance)
                    {
                        break;
                    }
                    let p = addd(eye, muld(direction, distance));
                    let t = (dotd(subd(p, a), axis) / axislen.max(1e-9)).clamp(0., 1.);
                    let radial = subd(p, addd(a, muld(axis, t)));
                    if lend(radial) < radius * (1. - 0.23 * t) {
                        hit = Some((p, unitd(radial), None));
                        tree_hit = Some(branch.clone());
                        break;
                    }
                }
            }
        }
        if self.selected == 3
            && release
            && self.stroke.is_none()
            && !self
                .active
                .is_some_and(|i| self.regions[i].world.payload_count() > 0)
            && !hit
                .as_ref()
                .is_some_and(|(_, _, i)| i.is_some_and(|i| self.regions[i].kind == "water"))
            && host.system_enabled("WaterSystem")
        {
            for k in 1..50 {
                let p = addd(eye, muld(direction, k as f64 * 0.05));
                if hit
                    .as_ref()
                    .is_some_and(|(h, _, _)| lend(subd(*h, eye)) < k as f64 * 0.05)
                {
                    break;
                }
                let water = host.invoke("WaterSystem.Sample", json!({"position":p}))?;
                if water["occupied"] == true
                    && water["surface_height"].as_f64().unwrap_or(0.)
                        - water["bed_height"].as_f64().unwrap_or(0.)
                        >= 0.04
                {
                    let surface = d(&water["surface_position"])?;
                    hit = Some((surface, up, None));
                    tree_hit = Some(
                        json!({"water":true,"depth":water["surface_height"].as_f64().unwrap_or(0.)-water["bed_height"].as_f64().unwrap_or(-1.)}),
                    );
                    break;
                }
            }
        }
        if self.stroke.is_none() && (release || poured && !mining) {
            // Reject unreachable work before instantiating any material. The
            // broad aim ray is selection only, never permission to throw tools.
            if !poured && hit.is_some_and(|(point,normal,_)| {
                let tool=Tool::held(TOOL_CASES[self.selected]);
                !hand::can_work(&tool,&Frame::at(eye),point,normal,self.selected,eye,direction,up,reach)
            }) {
                hit=None;
                self.status="Move closer — keep the target within your arms and tool reach".into();
            }
            if poured && self.active.is_some() {
                self.stroke = Some(Stroke {
                    point: eye,
                    normal: up,
                    time: 0.,
                    power: 450.,
                    pour: true,
                    start: hand::Pose::fit(&self.regions[self.active.unwrap()].world.tool),
                });
            } else if self
                .active
                .is_some_and(|i| self.regions[i].world.payload_count() > 0)
            {
                self.status = "Carrying material — right click / Pour to empty it".into();
            } else if let Some((point, normal, patch)) = hit {
                if lend(point) - terrain::RADIUS < terrain::BEDROCK_ELEVATION + 0.40 {
                    self.status = "Use the pickaxe for bedrock".into();
                } else {
                    let index = if let Some(i) = patch {
                        Some(i)
                    } else {
                        match self.activate(
                            host,
                            point,
                            normal,
                            tree_hit.as_ref(),
                            &input["terrain"],
                            reach,
                        ) {
                            Ok(v) => v,
                            Err(e) => {
                                self.status = format!("Could not refine material: {e}");
                                if let Some(i) = self.active {
                                    self.load_contact(host, i)?;
                                }
                                None
                            }
                        }
                    };
                    if let Some(i) = index {
                        if self.active != Some(i) {
                            let carried=self.active.map(|old|(self.regions[old].world.tool.clone(),self.regions[old].frame.clone()))
                                .or_else(||self.standalone.clone().zip(self.standalone_frame.clone()));
                            self.load_contact(host, i)?;
                            if let Some(old) = self.active {
                                let p = &mut self.regions[old];
                                p.world.tool.p = p
                                    .world
                                    .tool
                                    .model
                                    .iter()
                                    .map(|v| add(*v, [0., 40., 0.]))
                                    .collect();
                                p.world.tool.manual = Some(p.world.tool.p.clone());
                                p.world.tool.grasp = None;
                                p.sleeping = false;
                            }
                            let p = &mut self.regions[i];
                            let mut tool = Tool::held(TOOL_CASES[self.selected]);
                            if let Some((mut held,old_frame))=carried {
                                held.reframe(|v|p.frame.local(old_frame.world(v)),|v|p.frame.vector(subd(old_frame.world(v),old_frame.origin)));
                                tool=held;
                            }else{tool.p=hand::carry(&tool,&p.frame,eye,direction,up,reach).vertices(&tool);}
                            tool.manual=Some(tool.p.clone());
                            tool.ground = p.world.tool.ground.clone();
                            tool.ground_faces = p.world.tool.ground_faces.clone();
                            p.world.tool = tool;
                            p.world.configure(host)?;
                            self.ground_eye = eye;
                        }
                        self.active = Some(i);
                        self.stroke = Some(Stroke {
                            point,
                            normal,
                            time: 0.,
                            power: 45. + 755. * (self.charge / 1.2),
                            pour: false,
                            start: hand::Pose::fit(&self.regions[i].world.tool),
                        });
                    }
                }
            } else {
                if !self.status.starts_with("Move closer") {
                    self.status = "Aim at ground, rock or a tree within reach".into();
                }
            }
            self.charge = 0.;
        }
        self.held = held;
        // One background region per frame continues settling after leaving the
        // player's hand. Quiescent regions retain their accepted state and sleep.
        self.advance_background(host)?;
        let mut tool_vertices = Vec::new();
        let grips;
        if let Some(i) = self.active {
            let p = &mut self.regions[i];
            // Extend accepted support under a carried load as the player moves.
            if lend(subd(eye, self.ground_eye)) > 1.0 {
                let (mut ground, mut faces) = support_mesh(
                    &p.frame,
                    if p.kind == "water" {
                        None
                    } else {
                        p.domain.as_ref()
                    },
                    &input["terrain"],
                    p.tree.is_some(),
                );
                if lend(subd(eye, p.frame.origin)) > 3.0 {
                    let f = Frame::at(eye);
                    let (extra, extra_faces) = support_mesh(&f, None, &input["terrain"], false);
                    let offset = ground.len() as u32;
                    ground.extend(extra.into_iter().map(|v| p.frame.local(f.world(v))));
                    faces.extend(extra_faces.into_iter().map(|f| f.map(|i| i + offset)));
                }
                p.world.tool.ground = ground;
                p.world.tool.ground_faces = faces;
                p.world.configure(host)?;
                self.ground_eye = eye;
            }
            let pose = if let Some(stroke) = &mut self.stroke {
                stroke.time += dt;
                p.world.tool.force_limit = Some(stroke.power);
                p.world.tool.muscles.effort=if stroke.pour {0.4} else if self.selected==0 {
                    if (0.25..2.3).contains(&stroke.time) {1.}else{0.35}
                } else if (0.20..0.78).contains(&stroke.time) {1.}else{0.35};

                hand::work(
                    &p.world.tool,
                    &p.frame,
                    stroke,
                    self.selected,
                    eye,
                    direction,
                    up,
                    reach,
                ).vertices(&p.world.tool)
            } else {
                p.world.tool.force_limit = Some(450.);
                p.world.tool.muscles.effort=0.3;
                hand::carry(&p.world.tool, &p.frame, eye, direction, up, reach).vertices(&p.world.tool)
            };
            p.world.tool.grasp = hand::grasp
(&p.world.tool, &p.frame, reach);
            p.world.tool.manual = Some(pose);
            self.advance_active_material(host,dt,lend(eye)-terrain::RADIUS)?;
            let p=&mut self.regions[i];
            p.world.tool.render(&mut tool_vertices);
            for v in &mut tool_vertices {
                let w = p.frame.world([v[0], v[1], v[2]]);
                let rel = subd(w, eye);
                v[0] = rel[0] as f32;
                v[1] = rel[1] as f32;
                v[2] = rel[2] as f32;
            }
            grips=hand::feedback(&p.world.tool,&p.frame);
            if self.stroke.as_ref().is_some_and(|s| s.time > if s.pour {1.85} else if self.selected==0 {3.0} else if self.selected==3 {2.4} else {2.0}) {
                self.stroke = None;
                self.status = if p.world.payload_count() > 0 {
                    "Carrying material — right click / Pour to empty".into()
                } else {
                    format!("{} · ready for another stroke", p.kind)
                };
            }
        } else {
            let tool = self
                .standalone
                .get_or_insert_with(|| Tool::held(TOOL_CASES[self.selected]));
            let f = Frame::at(eye);
            let pose=hand::carry(tool, &f, eye, direction, up, reach);
            if let Some(old)=&self.standalone_frame {
                tool.reframe(|v|f.local(old.world(v)),|v|f.vector(subd(old.world(v),old.origin)));
            }else{tool.p=pose.vertices(tool);}
            tool.grasp=hand::grasp(tool,&f,reach);
            tool.manual=Some(pose.vertices(tool));
            tool.force_limit=Some(450.);
            tool.advance_held_air(dt)?;
            self.standalone_frame=Some(f.clone());
            grips=hand::feedback(tool,&f);
            tool.render(&mut tool_vertices);
            for v in &mut tool_vertices {
                let w = f.world([v[0], v[1], v[2]]);
                let rel = subd(w, eye);
                v[0] = rel[0] as f32;
                v[1] = rel[1] as f32;
                v[2] = rel[2] as f32;
            }
        }
        let mut vertices = Vec::<[f64; 6]>::new();
        for v in tool_vertices {
            vertices.push([
                eye[0] + v[0] as f64,
                eye[1] + v[1] as f64,
                eye[2] + v[2] as f64,
                v[3] as f64,
                v[4] as f64,
                v[5] as f64,
            ]);
        }
        // Small world-space aim mark, visible with every graphics backend.
        if let Some((point, normal, _)) = hit {
            let right = unitd(crossd(up, direction));
            let p = addd(point, muld(normal, 0.008));
            let a = addd(p, muld(right, 0.025));
            let b = addd(p, muld(right, -0.025));
            let c = addd(p, muld(up, 0.025));
            for v in [a, b, c] {
                vertices.push([v[0], v[1], v[2], 0.9, 0.85, 0.25]);
            }
        }
        self.caption_clock += dt;
        let caption_changed = self.caption_clock >= 0.2;
        if caption_changed {
            self.caption_clock = 0.;
        }
        let controls = if mining { "Primary: dig · Secondary: refill cut" } else if cfg!(target_os = "ios") {
            "Select tool · Hold Work, release · Pour"
        } else {
            "1 Shovel  2 Pickaxe  3 Axe  4 Bucket · Hold LMB, release · RMB empty"
        };
        let caption = format!(
            "{} · {} · {}",
            TOOL_NAMES[self.selected],
            if held && !mining {
                format!("Force {:0.0}%", 100. * self.charge / 1.2)
            } else if self.stroke.is_some() {
                "Working".into()
            } else {
                if self.status.is_empty() {
                    "Aim within reach".into()
                } else {
                    self.status.clone()
                }
            },
            controls
        );
        Ok(
            json!({"vertices":vertices,"tool_grip":grips,"caption":caption,"caption_changed":caption_changed,"selected":self.selected,"active":self.stroke.is_some(),"charge":self.charge/1.2,"status":self.status}),
        )
    }
    fn activate(
        &mut self,
        host: &mut Host,
        hit: D,
        normal: D,
        tree: Option<&Value>,
        terrain: &Value,
        reach: &Value,
    ) -> Result<Option<usize>, String> {
        let up = unitd(hit);
        let sample = terrain::sample(up);
        let field = world_field::Field::from_surface(terrain)?;
        let is_tree = tree.is_some_and(|t| t["site_id"].is_u64());
        let water = tree.is_some_and(|t| t["water"] == true);
        let substance = if water {
            world_field::Substance::Water
        } else {
            world_field::substance(hit)
        };
        if substance == world_field::Substance::Bedrock {
            self.status = "Use the pickaxe for bedrock".into();
            return Ok(None);
        }
        let kind = if is_tree { "wood" } else { substance.name() };
        let top = (match kind {
            // Refine the reach of a spade, not the entire geological column.
            // Deeper ground stays in the world field. Regional coupling and
            // boundary-release remain separate from this locality budget.
            "turf" => (sample.soil_depth.max(0.24) + 0.15).min(0.45),
            "stone" => 0.36,
            "dirt" => 0.36,
            "water" => tree
                .and_then(|v| v["depth"].as_f64())
                .unwrap_or(0.20)
                .clamp(0.04, 0.20),
            _ => 0.20,
        })
        .min((lend(hit) - terrain::RADIUS - terrain::BEDROCK_ELEVATION).max(0.01));
        let mut frame = Frame::at(addd(hit, muld(up, -top)));
        let min = match kind {
            "turf" => [-0.32, 0., -0.24],
            "stone" => [-0.30, 0., -0.30],
            _ => [-0.24, 0., -0.24],
        };
        let max = [-min[0], top + 0.006, -min[2]];
        let n = frame.vector(normal);
        let slope = if matches!(kind, "turf" | "dirt" | "fine sand" | "snow") {
            [
                -n[0] as f64 / (n[1] as f64).max(0.65),
                -n[2] as f64 / (n[1] as f64).max(0.65),
            ]
        } else {
            [0.; 2]
        };
        let mut transfer = None;
        let mut world = match kind {
            "turf" => {
                let mut w = World::empty("game_material", false);
                w.turf = None;
                w.layers = Some(Box::new(layered::Layered::game(
                    top as f32,
                    if top >= sample.soil_depth.max(0.24) + 0.15 - 1e-6 {
                        Some(make_stone(&field, &frame, min, [max[0], 0.15, max[2]])?)
                    } else { None },
                    |mut p| {
                        p[1] += slope[0] as f32 * p[0] + slope[1] as f32 * p[2];
                        field.distance(frame.world(p))
                    },
                )?));
                w
            }
            "stone" => {
                let mut w = World::empty("game_material", false);
                w.solid = Some(make_stone(&field, &frame, min, max)?);
                w
            }
            "wood" => {
                let id = tree.unwrap()["site_id"].as_u64().unwrap();
                let info = host.invoke(
                    "VegetationSystem.MaterialTree",
                    json!({"site_id":id,"position":hit,"operation":"prepare"}),
                )?;
                frame = Frame::at(d(&info["origin"])?);
                let mut w = World::empty("game_material", false);
                w.solid = Some(make_game_tree(&info, &frame, frame.local(hit)[1])?);
                transfer = Some(info);
                w
            }
            _ => World::filled_box(
                min.map(|v| v as f32),
                [max[0] as f32, top as f32, max[2] as f32],
                if water {
                    Kind::Water
                } else if kind == "snow" {
                    Kind::Snow
                } else if kind == "fine sand" {
                    Kind::FineSand
                } else {
                    Kind::Dirt
                },
                kind == "dirt",
            ),
        };
        if slope != [0.; 2] {
            world.shear(slope.map(|v| v as f32))?;
        }
        if matches!(kind, "dirt" | "fine sand" | "snow") {
            world.restrict_volume(|local| field.distance(frame.world(local)))?;
        }
        if water {
            let preview=host.invoke("TerrainSurfaceHydrologySystem.MaterialPreview",json!({"positions":world.particles.iter().map(|p|frame.world(p.p)).collect::<Vec<_>>()}))?;
            let occupied: Vec<bool> = serde_json::from_value(preview["occupied"].clone())
                .map_err(|_| "water occupancy transfer required")?;
            world.filter_water(&occupied)?;
        }
        let domain = (!is_tree).then(|| Domain {
            mined: false,
            tetra: None,
            slope,
            origin: frame.origin,
            axes: frame.axes,
            min,
            max,
        });
        if let Some(candidate) = &domain {
            if kind == "turf"
                && self
                    .regions
                    .iter()
                    .filter(|p| p.kind != "water")
                    .filter_map(|p| p.domain.as_ref())
                    .any(|d| domains_overlap(d, candidate))
            {
                self.status = "Aim inside the existing disturbed region or beside its edge".into();
                return Ok(None);
            }
        }
        let mut tool = Tool::held(TOOL_CASES[self.selected]);
        let forward = unitd(subd(hit, self.last_eye));
        let pose = hand::carry(&tool, &frame, self.last_eye, forward, up, reach).vertices(&tool);
        tool.p = pose.clone();
        tool.manual = Some(pose);
        tool.force_limit = Some(450.);
        let (ground, faces) = support_mesh(
            &frame,
            if water { None } else { domain.as_ref() },
            terrain,
            is_tree,
        );
        tool.ground = ground;
        tool.ground_faces = faces;
        world.tool = tool;
        if let Some(l) = &mut world.layers {
            l.refresh_skin(&world.tool);
        }
        // Accept the complete material/contact state before removing its static
        // representation. On configuration failure the original world remains.
        self.stash_contact(host);
        world.configure(host)?;
        if let Some(d) = &domain {
            let edit = json!({"domain":{"origin":d.origin,"axes":d.axes,"min":d.min,"max":d.max,"slope":d.slope}});
            if water {
                host.invoke("TerrainSurfaceHydrologySystem.MaterialDomain", edit)?;
            } else {
                host.invoke("PlanetSurfaceSystem.RefineMaterial", json!({"edit":edit}))?;
            }
        }
        let tree_id = transfer.as_ref().and_then(|v| v["site_id"].as_u64());
        if let Some(info) = transfer {
            host.invoke("VegetationSystem.MaterialTree",json!({"site_id":info["site_id"],"position":frame.origin,"operation":"commit","expected_revision":info["revision"]}))?;
        }
        let mut p = Region {
            frame,
            world,
            domain,
            kind,
            mesh: vec![],
            tree: tree_id,
            collision: vec![],
            collision_bounds: None,
            collision_blocks: vec![],
            sleeping: false,
            bounds:([0.;3],0.),
            contact: None,
        };
        p.refresh();
        self.regions.push(p);
        self.loaded = Some(self.regions.len() - 1);
        self.status = format!("{} — physical material", kind);
        Ok(Some(self.regions.len() - 1))
    }
}
fn make_stone(
    field: &world_field::Field,
    frame: &Frame,
    min: D,
    max: D,
) -> Result<solid::Solid, String> {
    use crate::physics::cut_geometry::{Plane, Poly};
    let mut shape = Poly::box_from(min, max);
    // Clip the occupied envelope BEFORE forming its material interfaces. Every
    // neighbouring cell then inherits exactly matching faces from one partition;
    // independently clipping already-bonded cells would tear their topology.
    for _ in 0..32 {
        let outside = shape
            .vertices
            .iter()
            .map(|v| {
                (
                    v.position,
                    field.distance(frame.world(v.position.map(|x| x as f32))),
                )
            })
            .max_by(|a, b| a.1.total_cmp(&b.1));
        let Some((local, _)) = outside.filter(|(_, d)| *d > 0.0005) else {
            break;
        };
        let (surface, n) = field.boundary(frame.world(local.map(|x| x as f32)));
        let plane = Plane::through(
            frame.local(surface).map(f64::from),
            frame.vector(n).map(f64::from),
        );
        shape = shape
            .clip_negative(plane, 1)
            .ok_or("No solid stone occupies this refinement envelope")?;
    }
    if shape
        .vertices
        .iter()
        .any(|v| field.distance(frame.world(v.position.map(|x| x as f32))) > 0.003)
    {
        return Err("Stone boundary needs a smaller refinement envelope".into());
    }
    let salt =
        ((frame.origin[0] * 10.) as i32).wrapping_mul(131) ^ ((frame.origin[2] * 10.) as i32);
    let seeds = solid_geometry::seeds(&shape, [4, 3, 4], salt.rem_euclid(100_000), false);
    if seeds.is_empty() {
        return Err("No stone occupies this volume".into());
    }
    let volume = shape.measure(true).volume;
    let cells = solid_geometry::partition(&shape, &seeds, [1.; 3], 0)?;
    let mut solid=solid::Solid::from_specimen(
        solid_geometry::Specimen {
            cells,
            kind: SolidKind::Stone,
            grain: [0., 1., 0.],
            bark: [0.; 3],
            source: "game geological stone volume",
            volume,
        },
        "game_stone",
    )?;
    solid.sample_stone_materials(|local| {
        let p=frame.world(local);
        terrain::cave_formations::mineral_material(p).unwrap_or(("stone",terrain::rock::base_color(p)))
    });
    Ok(solid)
}
fn make_game_tree(info: &Value, frame: &Frame, strike_height:f32) -> Result<solid::Solid, String> {
    use crate::physics::cut_geometry::{Face, Plane, Poly, Vertex};
    let branches = info["branches"]
        .as_array()
        .ok_or("tree branch transfer required")?;
    let first = &branches[0];
    let a = frame.local(d(&first["start"])?);
    let b = frame.local(d(&first["end"])?);
    let h = (b[1] - a[1]).min((strike_height+0.35).max(2.1)).max(0.3);
    let r = first["radius"].as_f64().unwrap() as f32;
    let mut shape = Poly::box_from(
        [-(r as f64), 0., -(r as f64)],
        [r as f64, h as f64, r as f64],
    );
    for k in 0..10 {
        let angle = k as f64 * std::f64::consts::TAU / 10.;
        let n = [
            angle.cos(),
            r as f64 * 0.23 / (b[1] - a[1]) as f64,
            angle.sin(),
        ];
        if let Some(c) = shape.clip_negative(
            Plane::through([angle.cos() * r as f64, 0., angle.sin() * r as f64], n),
            1,
        ) {
            shape = c;
        }
    }
    // Preserve full longitudinal fibres, with fine axial stations around the
    // player's actual cut. The previous uniform 35 cm sections usually put
    // every cross-grain interface outside the edge's 3 cm cutting zone.
    // These are intact bonded interfaces, not damage or a pre-cut notch.
    let cut=(strike_height as f64).clamp(0.13,h as f64-0.13);
    let stations=[0.025,cut-0.09,cut-0.025,cut+0.025,cut+0.09,h as f64-0.025];
    let section=solid_geometry::seeds(&shape,[3,1,3],111,true);
    let seeds:Vec<_>=stations.into_iter().flat_map(|y|section.iter().map(move |p|[p[0],y,p[2]])).collect();
    let volume = shape.measure(true).volume;
    let cells = solid_geometry::partition(&shape, &seeds, [1., 0.40, 1.], 0)?;
    let mut solid = solid::Solid::from_specimen(
        solid_geometry::Specimen {
            cells,
            kind: SolidKind::Wood,
            grain: [0., 1., 0.],
            bark: serde_json::from_value(info["bark"].clone()).unwrap_or([0.28, 0.20, 0.12]),
            source: "game tree with retained crown and anchored root fibres",
            volume,
        },
        "game_tree",
    )?;
    let mut proxies = Vec::new();
    for branch in branches {
        let mut a = frame.local(d(&branch["start"])?);
        let b = frame.local(d(&branch["end"])?);
        let mut r = branch["radius"].as_f64().unwrap() as f32;
        let r1 = branch["end_radius"].as_f64().unwrap() as f32;
        if branch["id"] == 0 {
            let t = (h - a[1]) / (b[1] - a[1]);
            if t >= 1. {
                continue;
            }
            a = add(a, scale(sub(b, a), t));
            r += t * (r1 - r);
        }
        let axis = unit(sub(b, a));
        let right = unit(cross(
            axis,
            if axis[1].abs() > 0.9 {
                [1., 0., 0.]
            } else {
                [0., 1., 0.]
            },
        ));
        let forward = cross(axis, right);
        let mut vertices = Vec::new();
        for (o, radius) in [(a, r), (b, r1)] {
            for k in 0..6 {
                let t = k as f32 * std::f32::consts::TAU / 6.;
                let p = add(
                    o,
                    add(
                        scale(right, radius * t.cos()),
                        scale(forward, radius * t.sin()),
                    ),
                )
                .map(f64::from);
                vertices.push(Vertex {
                    position: p,
                    material: p,
                    velocity: [0.; 3],
                });
            }
        }
        let mut faces = vec![
            Face {
                nodes: (0..6).rev().collect(),
                tag: 1,
            },
            Face {
                nodes: (6..12).collect(),
                tag: 1,
            },
        ];
        for k in 0..6 {
            faces.push(Face {
                nodes: vec![k, (k + 1) % 6, (k + 1) % 6 + 6, k + 6],
                tag: 1,
            });
        }
        let center = scale(add(a, b), 0.5).map(f64::from);
        for f in &mut faces {
            let [a, b, c] = [0, 1, 2].map(|i| vertices[f.nodes[i]].position);
            if dotd(crossd(subd(b, a), subd(c, a)), subd(a, center)) < 0. {
                f.nodes.reverse();
            }
        }
        proxies.push(Poly { vertices, faces });
    }
    let rows = info["vertices"].as_array().ok_or("tree skin required")?;
    let mut visual = Vec::new();
    for triangle in rows.chunks_exact(3) {
        let mut polygon: Vec<[f32; 6]> = triangle
            .iter()
            .map(|v| {
                let p = frame.local(d(&v["position"]).unwrap());
                let c: V = serde_json::from_value(v["color"].clone()).unwrap();
                [p[0], p[1], p[2], c[0], c[1], c[2]]
            })
            .collect();
        if triangle[0]["branch"] == 0 && triangle[0]["material"] == 0 {
            let mut result = Vec::new();
            let mut a = *polygon.last().unwrap();
            for &b in &polygon {
                let da = a[1] - h;
                let db = b[1] - h;
                if (da >= 0.) != (db >= 0.) {
                    let t = da / (da - db);
                    result.push(std::array::from_fn(|k| a[k] + t * (b[k] - a[k])));
                }
                if db >= 0. {
                    result.push(b);
                }
                a = b;
            }
            polygon = result;
        }
        for k in 1..polygon.len().saturating_sub(1) {
            visual.extend([polygon[0], polygon[k], polygon[k + 1]]);
        }
    }
    // Finite cross-grain support lets crown weight and axe contact load the
    // remaining hinge. No angle, cut count or timer commands the tree to fall.
    solid.prepare_felling(cut as f32 - 0.10, cut as f32 + 0.10);
    solid.attach_crown(proxies, visual);
    Ok(solid)
}
fn domains_overlap(a: &Domain, b: &Domain) -> bool {
    let basis = |d: &Domain| {
        [
            addd(d.axes[0], muld(d.axes[1], d.slope[0])),
            d.axes[1],
            addd(d.axes[2], muld(d.axes[1], d.slope[1])),
        ]
    };
    let aa = basis(a);
    let bb = basis(b);
    let center = |d: &Domain| d.world(std::array::from_fn(|k| (d.min[k] + d.max[k]) * 0.5));
    let delta = subd(center(a), center(b));
    let mut axes = Vec::new();
    for basis in [aa, bb] {
        for k in 0..3 {
            axes.push(crossd(basis[(k + 1) % 3], basis[(k + 2) % 3]));
        }
    }
    for x in aa {
        for y in bb {
            axes.push(crossd(x, y));
        }
    }
    for axis in axes {
        if lend(axis) < 1e-8 {
            continue;
        }
        let axis = unitd(axis);
        let radius = |d: &Domain, basis: [D; 3]| {
            (0..3)
                .map(|k| dotd(axis, basis[k]).abs() * (d.max[k] - d.min[k]) * 0.5)
                .sum::<f64>()
        };
        if dotd(delta, axis).abs() > radius(a, aa) + radius(b, bb) + 0.003 {
            return false;
        }
    }
    true
}
fn support_mesh(
    frame: &Frame,
    domain: Option<&Domain>,
    terrain: &Value,
    tree: bool,
) -> (Vec<V>, Vec<[u32; 3]>) {
    let edits = terrain["edits"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let field = terrain::underground::Field::parse(&terrain["underground"])
        .ok()
        .flatten();
    let underground =
        terrain::height(unitd(frame.origin)) - (lend(frame.origin) - terrain::RADIUS) > 3.;
    let mut triangles = Vec::new();
    let step = if tree { 1.5 } else { 0.25 };
    let half = if tree { 20 } else { 8 };
    for z in -half..half {
        for x in -half..half {
            let mut p = Vec::new();
            for [dx, dz] in [[0., 0.], [1., 0.], [1., 1.], [0., 1.]] {
                let q = [(x as f32 + dx) * step, 0., (z as f32 + dz) * step];
                let w = frame.world(q);
                let up = unitd(w);
                let surface = terrain::tool_cut::height(up, terrain::height(up), edits, false);
                let mut y = (terrain::RADIUS + surface - lend(frame.origin)) as f32;
                if underground {
                    if let Some(f) = &field {
                        let mut previous = 0.8;
                        let mut air = false;
                        for k in 0..120 {
                            let yy = 0.8 - k as f32 * 0.08;
                            let w = frame.world([q[0], yy, q[2]]);
                            let d = f.distance(w, terrain::height(unitd(w)));
                            if d > 0. {
                                air = true;
                            } else if air {
                                y = (previous + yy) * 0.5;
                                break;
                            }
                            previous = yy;
                        }
                    }
                }
                let w = frame.world([q[0], y, q[2]]);
                p.push([w[0], w[1], w[2], 0., 0., 0.]);
            }
            triangles.extend([[p[0], p[2], p[1]], [p[0], p[3], p[2]]]);
        }
    }
    let mut domains = terrain::material_domain::from_edits(edits);
    if let Some(d) = domain {
        domains.push(d.clone());
    }
    triangles = terrain::material_domain::clip(triangles, &domains);
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    let mut tri = |p: [V; 3]| {
        if length(cross(sub(p[1], p[0]), sub(p[2], p[0]))) < 1e-9 {
            return;
        }
        let n = vertices.len() as u32;
        vertices.extend(p);
        faces.push([n, n + 1, n + 2]);
    };
    for t in triangles {
        tri(t.map(|v| frame.local([v[0], v[1], v[2]])));
    }
    // The unactivated ground around and below the domain retains its contact
    // boundary. There is no invisible table running through its live contents.
    if let Some(d) = domain {
        let edit = json!({"domain":{"origin":d.origin,"axes":d.axes,"min":d.min,"max":d.max}});
        for t in terrain::material_domain::shell(&[edit], field.as_ref()) {
            tri(t.map(|p| frame.local([p[0], p[1], p[2]])));
        }
    }
    let mut unique = Vec::new();
    let mut index = std::collections::BTreeMap::<[u32; 3], u32>::new();
    let remap: Vec<_> = vertices
        .iter()
        .map(|p| {
            *index.entry(p.map(f32::to_bits)).or_insert_with(|| {
                let i = unique.len() as u32;
                unique.push(*p);
                i
            })
        })
        .collect();
    for face in &mut faces {
        *face = face.map(|i| remap[i as usize]);
    }
    (unique, faces)
}

pub(super) fn workshop_tree(info:&Value,height:f32)->Result<solid::Solid,String> {
    make_game_tree(info,&Frame{origin:[0.;3],axes:[[1.,0.,0.],[0.,1.,0.],[0.,0.,1.]]},height)
}

#[cfg(test)]
mod tool_suspension_checks {
    use super::*;

    fn fixture(loaded:bool)->Game {
        let x=if loaded{0.}else{0.8};
        let mut world=World::filled_box([x-0.01,-0.09,-0.01],[x+0.01,-0.07,0.01],Kind::Sand,false);
        world.tool=Tool::held("matter_sand");
        world.tool.initialize_pose(world.tool.model.clone());
        let frame=Frame::at([0.,terrain::RADIUS,0.]);
        world.tool.grasp=hand::grasp(&world.tool,&frame,&json!({
            "shoulders":[[-0.2,terrain::RADIUS+0.5,0.],[0.2,terrain::RADIUS+0.5,0.]],
            "arm_min":[0.,0.],"arm_max":[0.7,0.7]}));
        let mut region=Region {frame,world,domain:None,kind:"fine sand",mesh:vec![],tree:None,
            collision:vec![],collision_bounds:None,collision_blocks:vec![],sleeping:false,
            bounds:([0.;3],0.),contact:None};
        region.refresh();
        Game {regions:vec![region],active:Some(0),selected:3,held:true,charge:0.4,..Default::default()}
    }

    #[test]
    fn loaded_tool_cannot_be_stowed_or_lose_its_support() {
        let mut game=fixture(true);
        let count=game.tool_payload();
        assert!(count>0);
        assert_eq!(game.frame()["can_stow_tool"],false);
        let points=game.regions[0].world.tool.p.clone();
        let mass=game.regions[0].world.particles.iter().map(|p|p.mass).sum::<f32>();
        assert!(game.suspend_tool().is_err());
        assert!(game.suspended_tool.is_none());
        assert_eq!(game.regions[0].world.tool.p,points);
        assert!(game.regions[0].world.tool.grasp.is_some());
        assert_eq!(game.tool_payload(),count);
        assert_eq!(game.regions[0].world.particles.iter().map(|p|p.mass).sum::<f32>(),mass);
    }

    #[test]
    fn empty_tool_suspension_releases_hands_without_changing_material() {
        let mut game=fixture(false);
        assert_eq!(game.frame()["can_stow_tool"],true);
        let before=game.regions[0].world.particles.iter().map(|p|(p.p,p.v,p.mass,p.active)).collect::<Vec<_>>();
        game.suspend_tool().unwrap();
        assert!(!game.held);
        assert_eq!(game.charge,0.);
        assert!(game.stroke.is_none());
        assert!(!game.regions[0].sleeping);
        let tool=&mut game.regions[0].world.tool;
        assert!(tool.grasp.is_none());
        assert!(tool.p.iter().all(|p|p[1]>30.));
        let parked=tool.p.clone();
        assert_eq!(tool.goal(10.,1./240.),parked);
        game.suspend_tool().unwrap();
        assert_eq!(game.regions[0].world.tool.p,parked);
        assert_eq!(game.regions[0].world.particles.iter().map(|p|(p.p,p.v,p.mass,p.active)).collect::<Vec<_>>(),before);
        assert_eq!(game.frame()["tools_suspended"],true);
    }

    #[test]
    fn stowed_tool_reattaches_near_current_hands_after_player_travel() {
        let mut game=fixture(false);
        game.suspend_tool().unwrap();
        let saved_model=game.suspended_tool.as_ref().unwrap().tool.as_ref().unwrap().model.clone();
        let eye=[15.,terrain::RADIUS+1.8,0.];
        game.resume_tool(eye,[0.,0.,1.],[0.,1.,0.],&Value::Null).unwrap();
        assert!(game.suspended_tool.is_none());
        assert_eq!(game.selected,3);
        let region=&game.regions[0];
        assert_eq!(region.world.tool.model,saved_model);
        assert!(region.world.tool.p.iter().all(|p|lend(subd(region.frame.world(*p),eye))<2.));
        assert!(region.world.tool.grasp.is_none());
        assert_eq!(game.frame()["tools_suspended"],false);
        assert_eq!(game.regions[0].world.particles.len(),1);
    }

    #[test]
    fn stowed_tool_clears_structural_geometry_above_particle_contacts() {
        let mut game=fixture(false);
        // Structural domains have their own contact vertices, outside the
        // particle-only World::positions packet.
        game.regions[0].replace_collision(vec![[[-0.2,64.,-0.2],[0.2,64.,-0.2],[0.,64.,0.2]]]);
        game.suspend_tool().unwrap();
        assert!(game.regions[0].world.tool.p.iter().all(|p|p[1]>100.));
    }

    #[test]
    fn optional_tool_suspension_input_requires_boolean() {
        let mut input=json!({"position":[0.,terrain::RADIUS+1.8,0.],"aim":[0.,0.,1.],"up":[0.,1.,0.],"dt":0.01});
        assert!(valid(&input));
        input["tools_suspended"]=json!(true);assert!(valid(&input));
        input["tools_suspended"]=json!("true");assert!(!valid(&input));
    }
}

#[cfg(test)]
mod collision_bounds_checks {
    use super::*;

    fn fixture(count: usize) -> Game {
        let mut game = Game::default();
        for region in 0..count {
            let mut world = World::empty("game_material", false);
            // Two intersecting material surfaces: standing walls as well as
            // ground, with small triangles and original contiguous face order.
            for z in 0..16 {
                for x in 0..16 {
                    let a = [x as f32 * 0.06 - 0.48, 0., z as f32 * 0.06 - 0.48];
                    let b = [a[0] + 0.06, 0., a[2]];
                    let c = [a[0] + 0.06, 0., a[2] + 0.06];
                    let d = [a[0], 0., a[2] + 0.06];
                    for triangle in [[a, b, c], [a, c, d]] {
                        for p in triangle {
                            world.skin.push([p[0], p[1], p[2], 0.4, 0.3, 0.2]);
                        }
                        for p in triangle {
                            world.skin.push([0.40, (p[0] + 0.48) * 1.8, p[2], 0.4, 0.3, 0.2]);
                        }
                    }
                }
            }
            let mut patch = Region {
                frame: Frame::at([region as f64 * 3., terrain::RADIUS, 0.]),
                world,
                domain: None,
                kind: "dirt",
                mesh: vec![],
                tree: None,
                collision: vec![],
                collision_bounds: None,
                collision_blocks: vec![],
                sleeping: true,
                bounds: ([0.; 3], 0.),
                contact: None,
            };
            patch.refresh();
            game.regions.push(patch);
        }
        game
    }

    fn exhaustive(game: &Game) -> Game {
        let mut reference = game.clone();
        for patch in &mut reference.regions {
            patch.collision_bounds = None;
            patch.collision_blocks.clear();
        }
        reference
    }

    fn inputs(game: &Game, count: usize) -> Vec<Value> {
        (0..count).map(|i| {
            let patch = &game.regions[i % game.regions.len()];
            let local = [
                ((i * 37) % 101) as f32 / 100. * 1.6 - 0.8,
                0.65 + ((i * 19) % 31) as f32 * 0.05,
                ((i * 53) % 103) as f32 / 102. * 1.6 - 0.8,
            ];
            let position = patch.frame.world(local);
            let previous = patch.frame.world([local[0] - (i % 4) as f32 * 0.06, local[1], local[2]]);
            json!({"previous":previous,"position":position,"eye_height":1.45})
        }).collect()
    }

    #[test]
    fn material_body_bounds_preserve_exhaustive_resolution() {
        let game = fixture(16);
        let reference = exhaustive(&game);
        let mut contacts = 0;
        for input in inputs(&game, 128) {
            let expected = reference.resolve_body(&input).unwrap();
            contacts += usize::from(!expected["normals"].as_array().unwrap().is_empty());
            assert_eq!(game.resolve_body(&input).unwrap(), expected, "{input}");
        }
        assert!(contacts > 0, "fixture must exercise accepted contact responses");
    }

    #[test]
    fn material_collision_bounds_refresh_and_fail_open() {
        let mut game = fixture(1);
        let patch = &mut game.regions[0];
        assert!(!patch.collision_bounds.unwrap().excludes_sphere([0.; 3], 0.235));
        for v in &mut patch.world.skin {
            v[0] += 20.;
        }
        patch.refresh();
        assert!(patch.collision_bounds.unwrap().excludes_sphere([0.; 3], 0.235));
        assert!(!patch.collision_bounds.unwrap().excludes_sphere([20., 0., 0.], 0.235));
        assert!(patch.collision_blocks.iter().all(|b| b.unwrap().min[0] > 19.));
        let bounds = patch.collision_bounds.unwrap();
        assert!(!bounds.excludes_sphere([f32::NAN, 0., 0.], 0.235));
        assert!(!bounds.excludes_sphere([f32::INFINITY, 0., 0.], 0.235));
        assert!(!bounds.excludes_sphere([0.; 3], f64::NAN));
        patch.replace_collision(vec![[[f32::NAN, 0., 0.], [0.; 3], [1.; 3]]]);
        assert!(patch.collision_bounds.is_none());
        assert!(patch.collision_blocks[0].is_none());
        patch.replace_collision(vec![]);
        assert!(patch.collision_bounds.is_none());
        assert!(patch.collision_blocks.is_empty());
    }

    #[test]
    fn material_collision_bounds_keep_boundary_candidates() {
        let triangle = [[-1., 0., -1.], [1., 0., -1.], [0., 0., 1.]];
        let bounds = CollisionBounds::from_triangles(&[triangle]).unwrap();
        for y in [0., 0.23499998, 0.235, 0.23500001, -0.235] {
            for x in [-1.235, -1., 0., 1., 1.235] {
                let center = [x, y, 0.];
                let point = crate::water::hydrology::closest(center.map(f64::from), triangle.map(|p| p.map(f64::from)));
                if lend(subd(center.map(f64::from), point)) < 0.235 {
                    assert!(!bounds.excludes_sphere(center, 0.235));
                }
            }
        }
    }

    #[test]
    fn material_collision_bounds_preserve_nonfinite_degenerate_outcomes() {
        let collapsed = [[0., 0., 0.], [0., 0., 0.], [1., 0., 0.]];
        let closest = crate::water::hydrology::closest(
            [2., 0., 0.], collapsed.map(|p| p.map(f64::from)));
        assert!(closest.iter().any(|v| !v.is_finite()),
            "fixture must exercise nonfinite closest points outside the box");
        for triangle in [collapsed, [[0.; 3]; 3],
            [[0., 0., 0.], [1., 0., 0.], [0.5, 1e-8, 0.]]] {
            assert!(CollisionBounds::from_triangles(&[triangle]).is_none());
        }
        let mut game = fixture(1);
        let mut triangles = vec![[[10., 0., 0.], [11., 0., 0.], [10., 1., 0.]];
            COLLISION_BLOCK_TRIANGLES];
        triangles.push(collapsed);
        game.regions[0].replace_collision(triangles);
        assert!(game.regions[0].collision_bounds.is_none());
        assert!(game.regions[0].collision_blocks[0].is_some());
        assert!(game.regions[0].collision_blocks[1].is_none());
        let position = game.regions[0].frame.world([2., 1.45, 0.]);
        let input = json!({"previous":position,"position":position,"eye_height":1.45});
        let expected = exhaustive(&game).resolve_body(&input).unwrap();
        assert!(expected["position"].as_array().unwrap().iter().any(Value::is_null),
            "existing nonfinite coordinates serialize as null");
        assert_eq!(game.resolve_body(&input).unwrap(), expected);
    }

    #[test]
    #[ignore = "explicit CPU collision benchmark; run with --release --ignored --nocapture"]
    fn material_body_collision_benchmark() {
        use std::time::Instant;
        let game = fixture(16);
        let reference = exhaustive(&game);
        let inputs = inputs(&game, 256);
        let expected: Vec<_> = inputs.iter().map(|input| reference.resolve_body(input).unwrap()).collect();
        for (input, expected) in inputs.iter().zip(&expected) {
            assert_eq!(game.resolve_body(input).unwrap(), *expected);
        }
        let mut bounded_ms = Vec::new();
        let mut exhaustive_ms = Vec::new();
        // Alternate execution order to limit warmup/order bias. Timing reports
        // CPU queries only; all runs retain every original triangle and response.
        for round in 0..6 {
            for accelerated in [round % 2 == 0, round % 2 != 0] {
                let source = if accelerated { &game } else { &reference };
                let started = Instant::now();
                for input in &inputs {
                    std::hint::black_box(source.resolve_body(std::hint::black_box(input)).unwrap());
                }
                let ms = started.elapsed().as_secs_f64() * 1000.;
                if accelerated { bounded_ms.push(ms); } else { exhaustive_ms.push(ms); }
            }
        }
        let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
        eprintln!("material_body_collision {}", json!({
            "cpu_only":true,"exact_output_match":true,"regions":game.regions.len(),
            "triangles":game.regions.iter().map(|p|p.collision.len()).sum::<usize>(),
            "queries_per_round":inputs.len(),"rounds":bounded_ms.len(),
            "bounded_total_ms":bounded_ms,"exhaustive_total_ms":exhaustive_ms,
            "bounded_mean_ms_per_query":mean(&bounded_ms)/inputs.len() as f64,
            "exhaustive_mean_ms_per_query":mean(&exhaustive_ms)/inputs.len() as f64
        }));
    }
}

#[cfg(test)]impl Game{
 pub(crate) fn seed_crafting_tree(&mut self,position:D,fallen:bool){
  let frame=Frame::at(position);let mut world=World::empty("game_material",false);
  world.solid=Some(solid::Solid::crafting_fixture(fallen));
  let mut r=Region{frame,world,domain:None,kind:"wood",mesh:vec![],tree:Some(9191),collision:vec![],collision_bounds:None,collision_blocks:vec![],sleeping:true,bounds:([0.;3],5.),contact:None};r.refresh();self.regions.push(r);
 }
}
#[cfg(test)]mod crafting_material_checks{
 use super::*;
 #[test]fn crafting_consumes_exact_tree_once_and_rejects_distance(){
  let p=[0.,terrain::RADIUS+100.,0.];let mut game=Game::default();game.seed_crafting_tree(p,false);
  assert!(game.craft_tree(&json!({"position":p})).unwrap().is_null());
  game=Game::default();game.seed_crafting_tree(p,true);
  assert!(game.craft_tree(&json!({"position":addd(p,[10.,0.,0.])})).unwrap().is_null());
  assert!(game.craft_tree(&json!({"position":p,"site_id":123})).unwrap().is_null());
  assert_eq!(game.craft_tree(&json!({"position":p})).unwrap()["site_id"],9191);
  assert_eq!(game.craft_tree(&json!({"position":p,"site_id":9191,"consume":true})).unwrap()["site_id"],9191);
  assert!(game.craft_tree(&json!({"position":p,"consume":true})).unwrap().is_null());
  assert!(game.vertices().is_empty());assert!(game.collision_vertices().is_empty());
 }
}

#[cfg(test)]
mod cave_mining_checks {
    use super::*;
    #[test]
    fn mineral_refinement_keeps_names_mass_and_color_after_movement() {
        let config:Value=serde_json::from_str(include_str!("../../Procedures/caves.json")).unwrap();
        let field=world_field::Field::from_surface(&json!({"underground":config,"edits":[]})).unwrap();
        for name in ["ruby","silver","blue_crystal","magenta_crystal"] {
            let instance=field.caves.as_ref().unwrap().formations.iter().find(|f|f.mineral==Some(name)).unwrap();
            // Find occupied inclusion material in the same world field used by the pickaxe.
            let mut center=None;
            for dx in [-0.04,0.,0.04] {for dy in [0.10,0.15,0.20] {
                let p=terrain::underground::world(addd(instance.origin,[dx,dy,0.]));
                if terrain::cave_formations::mineral_material(p).is_some_and(|(id,_)|id==name) && field.distance(p)< -0.035 {center=Some(p);}
            }}
            let center=center.expect("solid mineral interior");let frame=Frame::at(center);
            let mut material=make_stone(&field,&frame,[-0.025;3],[0.025;3]).unwrap();
            let report=material.report();assert!(report["mineral_mass_kg"][name].as_f64().unwrap_or(0.)>0.,"{name}: {report}");
            let mut before=Vec::new();material.render(&mut before);assert!(!before.is_empty());
            for p in &mut material.p {p[0]+=0.7;p[1]-=0.4;}
            let mut after=Vec::new();material.render(&mut after);
            assert_eq!(before.len(),after.len());
            for(a,b)in before.iter().zip(&after){
                let sa:f32=a[3..].iter().sum();let sb:f32=b[3..].iter().sum();
                for k in 3..6 {assert!((a[k]/sa.max(1e-8)-b[k]/sb.max(1e-8)).abs()<0.001,"mineral hue changed");}
            }
            assert_eq!(report["mineral_mass_kg"],material.report()["mineral_mass_kg"]);
        }
    }
}
