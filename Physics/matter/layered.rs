//! A single physical ground section. Turf, soil, root anchorage and rock
//! exchange contact forces in the same coupled iterations. Stage changes move
//! or exchange tools; they never delete cover, reset matter or pre-carve holes.
use super::*;

#[derive(Clone)]
struct Anchorage {
    turf: usize,
    soil: usize,
    rest: f32,
    lambda: f32,
    peak: f32,
    broken: bool,
}
#[derive(Clone)]
pub(super) struct Layered {
    turf: turf::Turf,
    soil: Box<World>,
    rock: Option<solid::Solid>,
    roots: Vec<Anchorage>,
    stage: usize,
    stage_start: f32,
    frames: u64,
    initial_mass: f64,
    exposed: Option<V>,
    finished: bool,
    soil_passes: usize,
    structure_skin: Vec<[f32; 6]>,
    profile_ms: [f64; 6],
}
impl Layered {
    pub fn new() -> Result<Self, String> {
        let mut turf = turf::Turf::new();
        turf.translate([0., 0.40, 0.]);
        let mut soil = Box::new(World::new("matter_layer_soil"));
        soil.contact_order = (0..soil.particles.len()).collect();
        soil.topology_dirty = false;
        let rock = solid::Solid::new("matter_layers")?;
        Self::assemble(turf, soil, Some(rock))
    }
    pub(super) fn game(
        depth: f32,
        rock: Option<solid::Solid>,
        occupied: impl Fn(V) -> f64,
    ) -> Result<Self, String> {
        let mut turf = turf::Turf::new();
        turf.translate([0., depth - 0.089, 0.]);
        // The lower boundary remains implicit ground. Only include stone when
        // the disturbed volume actually reaches the geological interface.
        let base = if rock.is_some() { 0.154 } else { 0.004 };
        // Keep approximately the original full-column parcel size instead of
        // increasing sample density merely because the activated layer shrank.
        let mut soil = Box::new(World::filled_box_spaced(
            [-0.32, base, -0.24],
            [0.32, depth - 0.085, 0.24],
            Kind::Dirt,
            true,
            0.080,
        ));
        soil.restrict_volume(occupied)?;
        Self::assemble(turf, soil, rock)
    }
    fn assemble(
        turf: turf::Turf,
        soil: Box<World>,
        rock: Option<solid::Solid>,
    ) -> Result<Self, String> {
        let mut roots = Vec::new();
        // A reduced set of roots penetrates the upper soil below the sod.
        // Nearest physical soil parcels receive the equal opposite reaction.
        for i in turf.lower_nodes().into_iter().step_by(6) {
            if let Some((j, p)) = soil.particles.iter().enumerate().min_by(|(_, a), (_, b)| {
                length(sub(turf.p[i], a.p)).total_cmp(&length(sub(turf.p[i], b.p)))
            }) {
                let rest = length(sub(turf.p[i], p.p));
                if rest < 0.075 {
                    roots.push(Anchorage {
                        turf: i,
                        soil: j,
                        rest: rest * 1.3 + 0.008,
                        lambda: 0.,
                        peak: 0.,
                        broken: false,
                    });
                }
            }
        }
        let initial_mass = turf.report()["mass_kg"].as_f64().unwrap_or(0.)
            + soil.initial_mass as f64
            + rock
                .as_ref()
                .map_or(0., |r| r.report()["mass_kg"].as_f64().unwrap_or(0.));
        Ok(Self {
            turf,
            soil,
            rock,
            roots,
            stage: 0,
            stage_start: 0.,
            frames: 0,
            initial_mass,
            exposed: None,
            finished: false,
            soil_passes: 0,
            structure_skin: vec![],
            profile_ms: [0.; 6],
        })
    }
    pub fn phase(&self) -> &str {
        if self.finished {
            return if self.exposed.is_some() {
                "Section complete · reset to replay"
            } else {
                "Cover remains · excavation incomplete · reset to replay"
            };
        }
        match self.stage {
            0 => "Cut and lift rooted turf",
            1 => "Excavate soil · deposit beside the hole",
            2 => "Shovel meets bedrock · resistance",
            _ => "Pickaxe on exposed bedrock",
        }
    }
    fn positions(&self, tool: &Tool) -> Vec<V> {
        let mut p = self.turf.p.clone();
        p.extend(self.soil.sample_positions());
        if let Some(rock) = &self.rock {
            p.extend(&rock.p);
        }
        p.extend(&tool.p);
        p.extend(&tool.ground);
        p
    }
    pub fn configure(&mut self, host: &mut Host, tool: &Tool) -> Result<(), String> {
        let a = self.turf.p.len();
        let b = a + self.soil.contact_order.len() * 6;
        let c = b + self.rock.as_ref().map_or(0, |r| r.p.len());
        let mut surfaces = vec![self.turf.contact_surface(0), self.soil.sample_surface(a)];
        if let Some(rock) = &self.rock {
            surfaces.push(rock.contact_surface(b));
        }
        surfaces.extend([
            coupling::tool_surface(tool, c),
            coupling::table_surface(tool, c + tool.p.len()),
        ]);
        host.invoke("OGCConnectionSystem.Configure",json!({"positions":self.positions(tool),"contact_radius":0.0012,"contact_stiffness":40000.,
            "surfaces":surfaces}))?;
        self.turf.dirty = false;
        if let Some(rock) = &mut self.rock {
            rock.dirty = false;
        }
        Ok(())
    }
    fn exposed_target(&self) -> Option<V> {
        self.rock.as_ref()?;
        // Require a clear patch, not merely one ray between two soil parcels.
        for z in -2..=2 {
            for x in -2..=2 {
                let target = [x as f32 * 0.055, 0.155, z as f32 * 0.055];
                let clear = [
                    [-0.018, -0.018],
                    [0.018, -0.018],
                    [0., 0.],
                    [0.018, 0.018],
                    [-0.018, 0.018],
                ]
                .iter()
                .all(|off| {
                    let q = add(target, [off[0], 0., off[1]]);
                    !self.turf.covers(q)
                        && !self.soil.particles.iter().any(|p| {
                            p.active
                                && p.p[1] + p.radius > q[1]
                                && (p.p[0] - q[0]).powi(2) + (p.p[2] - q[2]).powi(2)
                                    < (p.radius * 1.5).powi(2)
                        })
                });
                if clear {
                    return Some(target);
                }
            }
        }
        None
    }
    fn transition(&mut self, tool: &mut Tool, time: f32) -> bool {
        if tool.manual.is_some() || self.finished {
            return false;
        }
        let age = time - self.stage_start;
        if self.stage == 3 {
            if age >= 16. {
                self.finished = true;
            }
            return false;
        }
        if age < 15. || !tool.clear_of_sample() {
            return false;
        }
        self.exposed = self.exposed_target();
        match self.stage {
            0 => {
                self.stage = 1;
                tool.use_soil_stroke();
                tool.retarget([0., 0.12, 0.]);
            }
            1 => {
                self.soil_passes += 1;
                if let Some(target) = self.exposed {
                    self.stage = 2;
                    tool.retarget([target[0], -0.03, target[2]]);
                } else if self.soil_passes < 5 {
                    let offsets = [[-0.06, 0.], [0.06, 0.], [0., 0.055], [0., -0.055], [0., 0.]];
                    let p = offsets[self.soil_passes - 1];
                    tool.retarget([p[0], 0.12, p[1]]);
                } else {
                    self.finished = true;
                }
            }
            _ => {
                // Both tools are outside the specimen during the exchange.
                // The new pick then returns through its force-limited drive.
                let target = self.exposed.unwrap_or([0., 0.15, 0.]);
                let mut pick = Tool::new("matter_stone");
                pick.set_placement([0.72, 0.1, 0.], 0.);
                pick.retarget([target[0], -0.20, target[2]]);
                *tool = pick;
                self.stage = 3;
                self.stage_start = time;
                return true;
            }
        }
        self.stage_start = time;
        false
    }
    fn solve_anchorage(&mut self, turf: &mut [V], soil: &mut [V], dt: f32) {
        for r in &mut self.roots {
            if r.broken {
                continue;
            }
            let delta = sub(soil[r.soil], turf[r.turf]);
            let distance = length(delta).max(1e-8);
            let wa = self.turf.inverse_mass(r.turf);
            let wb = self.soil.particles[r.soil].inv_mass();
            if wa + wb == 0. {
                continue;
            }
            let alpha = 1. / (1400. * dt * dt);
            let next =
                (r.lambda + (-(distance - r.rest) - alpha * r.lambda) / (wa + wb + alpha)).min(0.);
            let dl = next - r.lambda;
            r.lambda = next;
            r.peak = r.peak.max(-next / (dt * dt));
            let correction = scale(delta, dl / distance);
            turf[r.turf] = sub(turf[r.turf], scale(correction, wa));
            soil[r.soil] = add(soil[r.soil], scale(correction, wb));
        }
    }
    pub fn substep(
        &mut self,
        host: &mut Host,
        tool: &mut Tool,
        time: f32,
        dt: f32,
    ) -> Result<(u64, f32, [f64; 4]), String> {
        if self.transition(tool, time) {
            self.configure(host, tool)?;
        }
        if let Some(rock) = &mut self.rock {
            rock.refresh_interfaces();
        }
        self.turf.refresh_contact();
        if self.turf.dirty || self.rock.as_ref().is_some_and(|r| r.dirty) {
            self.configure(host, tool)?;
        }
        let previous = self.positions(tool);
        let tool_old = tool.p.clone();
        let mut turf_step = self.turf.begin_step(dt);
        let mut soil_step = self.soil.sample_begin(dt);
        let mut rock_step = self.rock.as_mut().map(|r| r.begin_step(dt));
        for r in &mut self.roots {
            r.lambda = 0.;
            r.peak = 0.;
        }
        let goal = tool.goal(
            if self.finished {
                16.
            } else {
                time - self.stage_start
            },
            dt,
        );
        let mut timings = [0.; 4];
        let mut contacts = 0;
        let mut peak = 0f32;
        let a = self.turf.p.len();
        let b = a + self.soil.contact_order.len() * 6;
        let c = b + self.rock.as_ref().map_or(0, |r| r.p.len());
        let mut final_response = vec![[0.; 13]; tool.p.len()];
        for _ in 0..5 {
            let start = std::time::Instant::now();
            self.turf.refresh_contact();
            if self.turf.dirty {
                self.configure(host, tool)?;
            }
            let mut tq = self.turf.material_target(&mut turf_step, dt);
            self.profile_ms[0] += start.elapsed().as_secs_f64() * 1000.;
            let material_start = std::time::Instant::now();
            let mut sq = self.soil.sample_target(&mut soil_step, dt);
            self.profile_ms[1] += material_start.elapsed().as_secs_f64() * 1000.;
            let material_start = std::time::Instant::now();
            let rq = self
                .rock
                .as_mut()
                .zip(rock_step.as_mut())
                .map(|(r, step)| r.material_target(step, dt));
            self.profile_ms[2] += material_start.elapsed().as_secs_f64() * 1000.;
            // Retain root reactions across coupled iterations just like the
            // internal matrix reactions in each fixed inertial prediction.
            let before_t = tq.clone();
            let before_s = sq.clone();
            self.solve_anchorage(&mut tq, &mut sq, dt);
            for i in 0..tq.len() {
                turf_step.shift[i] = add(turf_step.shift[i], sub(tq[i], before_t[i]));
            }
            for i in 0..sq.len() {
                soil_step.shift[i] = add(soil_step.shift[i], sub(sq[i], before_s[i]));
            }
            timings[0] += start.elapsed().as_secs_f64() * 1000.;
            let start = std::time::Instant::now();
            let batch = host.contact_responses(&self.positions(tool), &previous)?;
            timings[1] += start.elapsed().as_secs_f64() * 1000.;
            timings[2] += batch.broad_ms;
            timings[3] += batch.narrow_ms;
            contacts = contacts.max(batch.contacts);
            let accepted_start = std::time::Instant::now();
            self.turf.accept_contact(&tq, &batch.responses[..a], dt);
            self.soil.sample_accept(&sq, &batch.responses[a..b], dt);
            if let (Some(rock), Some(rq)) = (&mut self.rock, &rq) {
                rock.accept_contact(rq, &batch.responses[b..c], dt)?;
            }
            self.profile_ms[3] += accepted_start.elapsed().as_secs_f64() * 1000.;
            final_response.copy_from_slice(&batch.responses[c..c + tool.p.len()]);
            peak = peak.max(
                final_response
                    .iter()
                    .map(|r| length([r[0], r[1], r[2]]))
                    .sum(),
            );
            let tool_start = std::time::Instant::now();
            tool.project(&goal, &final_response, dt)?;
            self.profile_ms[4] += tool_start.elapsed().as_secs_f64() * 1000.;
        }
        // Attribute each aggregated tool-vertex response to its nearest
        // physical boundary. Do not spend the same impact on turf and rock.
        let mut tr = vec![[0.; 13]; tool.p.len()];
        let mut rr = tr.clone();
        let mut load = [0f32; 3];
        for (i, r) in final_response.iter().enumerate() {
            let force = length([r[0], r[1], r[2]]);
            if force < 0.05 {
                continue;
            }
            let point = tool.p[i];
            let d = [
                self.turf.distance(point),
                self.soil.sample_distance(point),
                self.rock
                    .as_ref()
                    .map_or(f32::INFINITY, |r| r.distance(point)),
            ];
            let nearest = (0..3).min_by(|&a, &b| d[a].total_cmp(&d[b])).unwrap();
            if d[nearest] > 0.012 {
                continue;
            }
            load[nearest] += force;
            if nearest == 0 {
                tr[i] = *r;
            } else if nearest == 2 {
                rr[i] = *r;
            }
        }
        let all_load = final_response
            .iter()
            .map(|r| length([r[0], r[1], r[2]]))
            .sum::<f32>()
            .max(1e-8);
        let impact = tool.impact_energy(&tool_old, dt);
        self.turf.finish_step(
            &turf_step,
            tool,
            &tool_old,
            &tr,
            dt,
            impact * load[0] / all_load,
        )?;
        self.soil.sample_finish(&soil_step, dt)?;
        if let (Some(rock), Some(rock_step)) = (&mut self.rock, &rock_step) {
            rock.finish_step(
                rock_step,
                tool,
                &tool_old,
                &rr,
                dt,
                impact * load[2] / all_load,
            )?;
        }
        for r in &mut self.roots {
            if r.peak > 3.5 {
                r.broken = true;
                r.lambda = 0.;
            }
        }
        tool.record_contact(&tool_old, &final_response, dt);
        tool.finish(&tool_old, dt);
        Ok((contacts, peak, timings))
    }
    pub fn refresh_skin(&mut self, tool: &Tool) {
        let skin_start = std::time::Instant::now();
        self.frames += 1;
        let mut structural = Vec::new();
        self.render_structure(&mut structural);
        self.structure_skin = structural;
        let mut tool_skin = Vec::new();
        tool.render(&mut tool_skin);
        let budget = (if tool.manual.is_some() {
            18000usize
        } else {
            24576usize
        })
        .saturating_sub(self.structure_skin.len() + tool_skin.len() + 12);
        if self.frames % 4 == 0 || self.soil.skin.len() > budget {
            self.soil.frames = self.frames;
            self.soil.skin = skin::reconstruct_with_budget(&mut self.soil, budget);
        }
        self.profile_ms[5] += skin_start.elapsed().as_secs_f64() * 1000.;
    }
    pub(super) fn shear(&mut self, slope: [f32; 2]) -> Result<(), String> {
        self.turf.shear(slope);
        self.soil.shear(slope)?;
        if let Some(rock) = &mut self.rock {
            rock.shear(slope)?;
        }
        for r in &mut self.roots {
            r.rest = length(sub(self.turf.p[r.turf], self.soil.particles[r.soil].p)) * 1.3 + 0.008;
        }
        Ok(())
    }
    pub(super) fn collision(&self) -> Vec<[V; 3]> {
        let mut v = self.rock.as_ref().map_or_else(Vec::new, |r| r.collision());
        v.extend(self.turf.collision());
        v.extend(self.soil.collision());
        v
    }
    pub(super) fn payload_count(&self, tool: &Tool) -> usize {
        tool.carried_samples(
            self.soil
                .particles
                .iter()
                .filter(|p| p.active)
                .map(|p| (p.p, p.radius))
                .chain(self.turf.p.iter().copied().map(|p| (p, 0.002))),
        )
    }
    pub(super) fn moving(&self) -> bool {
        self.turf.moving() || self.soil.moving() || self.rock.as_ref().is_some_and(|r| r.moving())
    }
    pub fn report(&self) -> Value {
        let turf = self.turf.report();
        let rock = self.rock.as_ref().map_or(Value::Null, |r| r.report());
        let soil_mass = self
            .soil
            .particles
            .iter()
            .filter(|p| p.active)
            .map(|p| p.mass as f64)
            .sum::<f64>();
        json!({"phase":self.phase(),"stage":self.stage,"soil_passes":self.soil_passes,"complete":self.finished,
            "timing_ms_per_frame":{"turf":self.profile_ms[0]/self.frames.max(1) as f64,"soil":self.profile_ms[1]/self.frames.max(1) as f64,"stone":self.profile_ms[2]/self.frames.max(1) as f64,"accept_contact":self.profile_ms[3]/self.frames.max(1) as f64,"tool":self.profile_ms[4]/self.frames.max(1) as f64,"skin":self.profile_ms[5]/self.frames.max(1) as f64},
            "bedrock_exposed":self.exposed_target().is_some(),"bedrock_target":self.exposed,
            "mass_kg":soil_mass+turf["mass_kg"].as_f64().unwrap_or(0.)+rock["mass_kg"].as_f64().unwrap_or(0.),
            "initial_mass_kg":self.initial_mass,"soil_mass_kg":soil_mass,"soil_fractures":self.soil.fractures,
            "soil_samples":self.soil.active_count(),"anchoring_roots":self.roots.len(),
            "soil_solver_ms_per_frame":self.soil.material_timings.map(|v|v/self.frames.max(1) as f64),
            "torn_anchoring_roots":self.roots.iter().filter(|r|r.broken).count(),"turf":turf,"rock":rock,
            "soil_deposited_samples":self.soil.particles.iter().filter(|p|p.active&&p.p[0].abs()>0.36).count(),
            "scope":"bounded section; fixed soil resolution and prepared rock interfaces"})
    }
    pub fn render(&self, out: &mut Vec<[f32; 6]>) {
        out.extend_from_slice(&self.structure_skin);
        out.extend_from_slice(&self.soil.skin);
    }
    fn render_structure(&self, out: &mut Vec<[f32; 6]>) {
        if let Some(rock) = &self.rock {
            rock.render(out);
        }
        self.turf.render_with_leaves(out, 2);
        for r in &self.roots {
            let a = self.turf.p[r.turf];
            let b = self.soil.particles[r.soil].p;
            if r.broken {
                skin::rod(
                    out,
                    a,
                    add(a, [0., -r.rest * 0.4, 0.]),
                    0.0009,
                    [0.55, 0.43, 0.24],
                );
            } else {
                skin::rod(out, a, b, 0.001, [0.59, 0.47, 0.28]);
            }
        }
    }
}
impl World {
    pub(super) fn sample_positions(&self) -> Vec<V> {
        self.contact_order
            .iter()
            .flat_map(|&i| {
                let p = &self.particles[i];
                AXES.map(|a| add(p.p, scale(a, p.radius * 0.88)))
            })
            .collect()
    }
    pub(super) fn sample_surface(&self, start: usize) -> Value {
        let faces: Vec<_> = (0..self.contact_order.len())
            .flat_map(|i| OCT.map(|f| f.map(|j| j + i as u32 * 6)))
            .collect();
        let movable: Vec<_> = self
            .contact_order
            .iter()
            .flat_map(|&i| [!self.particles[i].anchored; 6])
            .collect();
        json!({"start":start,"count":self.contact_order.len()*6,"triangles":faces,"motion":0,"self_collision":false,"friction":0.72,"movable_vertices":movable})
    }
    fn sample_distance(&self, p: V) -> f32 {
        self.particles
            .iter()
            .filter(|p| p.active)
            .map(|a| (length(sub(a.p, p)) - a.radius * 0.88).abs())
            .fold(f32::INFINITY, f32::min)
    }
    pub(super) fn sample_begin(&mut self, dt: f32) -> coupling::Step {
        self.reset_contact_lambda();
        for b in &mut self.bonds {
            b.lambda = 0.;
            b.peak = 0.;
        }
        coupling::Step::new(
            &self.particles.iter().map(|p| p.p).collect::<Vec<_>>(),
            &self.particles.iter().map(|p| p.v).collect::<Vec<_>>(),
            &self
                .particles
                .iter()
                .map(|p| p.inv_mass())
                .collect::<Vec<_>>(),
            dt,
        )
    }
    pub(super) fn sample_target(&mut self, step: &mut coupling::Step, dt: f32) -> Vec<V> {
        let current: Vec<_> = self.particles.iter().map(|p| p.p).collect();
        let mut q = current.clone();
        if self.sand.is_none() {self.solve_material(&mut q, &step.old, dt);}
        step.target(&current, &mut q);
        q
    }
    pub(super) fn sample_accept(&mut self, q: &[V], response: &[[f32; 13]], dt: f32) {
        for (slot, &i) in self.contact_order.iter().enumerate() {
            let p = &mut self.particles[i];
            if p.anchored {
                continue;
            }
            p.p = add(
                p.p,
                translation(p.p, q[i], p.mass, &response[slot * 6..slot * 6 + 6], dt),
            );
        }
    }
    pub(super) fn sample_finish(&mut self, step: &coupling::Step, dt: f32) -> Result<(), String> {
        for (i, p) in self.particles.iter_mut().enumerate() {
            p.v = scale(sub(p.p, step.old[i]), (-1.2 * dt).exp() / dt);
            if p.p.iter().chain(&p.v).any(|x| !x.is_finite()) {
                return Err("non-finite layered soil".into());
            }
        }
        if self.finish_material(dt) {
            self.refresh_shape_neighbours();
        }
        Ok(())
    }
    pub(super) fn advance_layers(&mut self, host: &mut Host) -> Result<(), String> {
        let wall = std::time::Instant::now();
        let checkpoint = (self.layers.clone(), self.tool.clone(), self.time);
        for _ in 0..4 {
            self.prepare_worker(1./240.);
            match self
                .layers
                .as_mut()
                .unwrap()
                .substep(host, &mut self.tool, self.time, 1. / 240.)
            {
                Ok((contacts, force, costs)) => {
                    self.contacts = self.contacts.max(contacts);
                    self.max_force = self.max_force.max(force);
                    for (i, cost) in costs.into_iter().enumerate() {
                        self.timings[i + 1] += cost;
                    }
                    self.time += 1. / 240.;
                    self.solver_iterations += 5;
                }
                Err(error) => {
                    (self.layers, self.tool, self.time) = checkpoint;
                    self.layers.as_mut().unwrap().configure(host, &self.tool)?;
                    return Err(error);
                }
            }
        }
        self.layers.as_mut().unwrap().refresh_skin(&self.tool);
        self.frames += 1;
        self.step_ms = wall.elapsed().as_secs_f64() * 1000.;
        self.total_step_ms += self.step_ms;
        self.worst_step_ms = self.worst_step_ms.max(self.step_ms);
        Ok(())
    }
}
