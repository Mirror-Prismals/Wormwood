//! Adaptive mesoscopic material workshop. Material, contact and presentation
//! are independent: OGC owns tool/material boundaries, the constitutive solver
//! owns internal strain/plasticity. Cohesive dirt has an implicit skin; solid
//! fracture exposes persistent polygonal faces and sand keeps discrete grains.
mod adapt;
mod coupling;
pub(crate) mod game;
mod game_materials;
mod growth;
mod layered;
mod material;
mod muscle;
mod worker;
mod thermal;
pub(crate) mod climate;
mod sand;
mod skin;
mod solid;
mod solid_geometry;
mod tools;
mod trials;
mod turf;
pub(crate) mod world_field;
use super::math::*;
use crate::host::Host;
use growth::Garden;
use material::{Bond, Kind, SolidKind};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use tools::Tool;
pub const CASES: &[&str] = &[
    "matter_worker_dig",
    "matter_worker_axe",
    "matter_snow",
    "matter_ice",
    "matter_ice_air",
    "matter_dirt",
    "matter_grass",
    "matter_seedlings",
    "matter_sand",
    "matter_pebbles",
    "matter_layers",
    "matter_wood",
    "matter_wood_chop",
    "matter_stone",
    "matter_rubble",
    "matter_stone_blunt",
];
pub(crate) fn is_case(name: &str) -> bool {
    CASES.contains(&name)
        || name.strip_suffix("_compare").is_some_and(|base| {
            CASES.contains(&base) && !matches!(base, "matter_seedlings" | "matter_layers" | "matter_worker_dig" | "matter_worker_axe" | "matter_snow" | "matter_ice" | "matter_ice_air")
        })
}
const MAX_ACTIVE: usize = 1000;
const OCT: [[u32; 3]; 8] = [
    [0, 2, 4],
    [2, 1, 4],
    [1, 3, 4],
    [3, 0, 4],
    [2, 0, 5],
    [1, 2, 5],
    [3, 1, 5],
    [0, 3, 5],
];
const AXES: [V; 6] = [
    [1., 0., 0.],
    [-1., 0., 0.],
    [0., 1., 0.],
    [0., -1., 0.],
    [0., 0., 1.],
    [0., 0., -1.],
];
#[derive(Clone)]
pub struct Particle {
    p: V,
    rest: V,
    v: V,
    spin: V,
    radius: f32,
    mass: f32,
    kind: Kind,
    anchored: bool,
    active: bool,
    level: u8,
    quiet: f32,
    moisture: f32,
    region: u32,
    activity: f32,
}
impl Particle {
    fn inv_mass(&self) -> f32 {
        if self.anchored { 0. } else { 1. / self.mass }
    }
}
#[derive(Clone)]
struct Family {
    parent: usize,
    children: [usize; 8],
}
#[derive(Clone)]
pub struct World {
    case: String,
    worker: Option<worker::Worker>,
    thermal: Option<thermal::Thermal>,
    particles: Vec<Particle>,
    bonds: Vec<Bond>,
    families: Vec<Family>,
    tool: Tool,
    garden: Garden,
    time: f32,
    frames: u64,
    fractures: u64,
    splits: u64,
    merges: u64,
    topology_dirty: bool,
    contact_order: Vec<usize>,
    contacts: u64,
    max_force: f32,
    skin: Vec<[f32; 6]>,
    skin_spacing: f32,
    initial_mass: f32,
    halted: Option<String>,
    step_ms: f64,
    total_step_ms: f64,
    worst_step_ms: f64,
    timings: [f64; 6], // adaptation, material, contact total, broad, narrow, skin
    solver_iterations: u64,
    solid: Option<solid::Solid>,
    turf: Option<turf::Turf>,
    // Packed upper triangle of active samples; contact_index maps stable IDs.
    // Avoid hashing/allocating every soil contact during every solver iteration.
    contact_lambda: Vec<f32>,
    contact_index: Vec<usize>,
    neighbour_keys: Vec<(usize,[i32;3])>,
    neighbour_cache: std::sync::Arc<Vec<(Vec<usize>,Vec<usize>)>>,
    material_timings: [f64; 4],
    shape_neighbours: Vec<Vec<(usize, usize)>>,
    sand: Option<sand::Sand>,
    comparison: Option<trials::Comparison>,
    peak_lifted: u64,
    peak_bucket: usize,
    layers: Option<Box<layered::Layered>>,
}

#[cfg(test)]
mod spatial_checks {
    use super::*;

    fn sparse_reference(w: &World, q: &[V]) -> Vec<(Vec<usize>, Vec<usize>)> {
        let mut bins = std::collections::BTreeMap::<[i32; 3], Vec<usize>>::new();
        for (i, p) in w.particles.iter().enumerate().filter(|(_,p)|p.active) {
            let _ = p;
            bins.entry(q[i].map(|v|(v/0.09).floor() as i32)).or_default().push(i);
        }
        bins.iter().map(|(k,members)| {
            let mut nearby = Vec::new();
            for z in -1..=1 { for y in -1..=1 { for x in -1..=1 {
                if let Some(other) = bins.get(&[k[0]+x,k[1]+y,k[2]+z]) { nearby.extend(other); }
            }}}
            (members.clone(),nearby)
        }).collect()
    }

    #[test]
    fn dense_bins_preserve_sparse_constraint_order_and_fallback() {
        let mut w = World::filled_box([-0.32,-0.2,-0.24],[0.32,0.3,0.24],Kind::Dirt,true);
        let mut q: Vec<_> = w.particles.iter().map(|p|p.p).collect();
        for (i,p) in w.particles.iter_mut().enumerate() {
            p.active = i%7 != 0;
            q[i][0] += ((i*17)%13) as f32*0.009;
        }
        assert_eq!(w.neighbour_groups(&q),sparse_reference(&w,&q));
        let cached=w.cached_neighbour_groups(&q);
        assert_eq!(*cached,sparse_reference(&w,&q));
        assert!(std::sync::Arc::ptr_eq(&cached,&w.cached_neighbour_groups(&q)));
        q[1] = [-100.,80.,-60.];
        assert_eq!(w.neighbour_groups(&q),sparse_reference(&w,&q));
        assert_eq!(*w.cached_neighbour_groups(&q),sparse_reference(&w,&q));
        for p in &mut w.particles {p.active=false;}
        assert!(w.neighbour_groups(&q).is_empty());
    }

    #[test]
    fn contact_storage_tracks_live_samples_not_refinement_history() {
        let mut w = World::filled_box([-0.32,0.,-0.24],[0.32,0.36,0.24],Kind::Dirt,true);
        for (i,p) in w.particles.iter_mut().enumerate() { p.active=i%11==0; }
        w.reset_contact_lambda();
        let n=w.active_count();
        assert_eq!(w.contact_lambda.len(),n*(n-1)/2);
        let slots:Vec<_>=w.particles.iter().enumerate().filter(|(_,p)|p.active).map(|(i,_)|w.contact_index[i]).collect();
        assert_eq!(slots,(0..n).collect::<Vec<_>>());
    }
}
impl World {
    fn empty(case: &str, compare: bool) -> Self {
        Self {
            case: case.into(),
            worker: None,
            thermal: None,
            particles: vec![],
            bonds: vec![],
            families: vec![],
            tool: Tool::new(case),
            garden: Garden::default(),
            time: 0.,
            frames: 0,
            fractures: 0,
            splits: 0,
            merges: 0,
            topology_dirty: true,
            contact_order: vec![],
            contacts: 0,
            max_force: 0.,
            skin: vec![],
            skin_spacing: 0.022,
            initial_mass: 0.,
            halted: None,
            step_ms: 0.,
            total_step_ms: 0.,
            worst_step_ms: 0.,
            timings: [0.; 6],
            solver_iterations: 0,
            solid: None,
            turf: None,
            contact_lambda: Vec::new(),
            contact_index: Vec::new(),
            neighbour_keys: Vec::new(),
            neighbour_cache: std::sync::Arc::new(Vec::new()),
            material_timings: [0.; 4],
            shape_neighbours: Vec::new(),
            sand: None,
            comparison: if compare {
                Some(trials::Comparison::new())
            } else {
                None
            },
            peak_lifted: 0,
            peak_bucket: 0,
            layers: None,
        }
    }

    pub fn new(case: &str) -> Self {
        let compare = case.ends_with("_compare");
        let case = case.strip_suffix("_compare").unwrap_or(case);
        let mut w = Self::empty(case, compare);
        if compare {
            w.tool.force_limit = Some(trials::LIMITS[0]);
        }
        if matches!(case,"matter_worker_dig"|"matter_worker_axe"|"matter_snow"|"matter_ice"|"matter_ice_air") {
            if let Err(e)=w.initialize_showcase(case) { w.halted=Some(e); }
            return w;
        }
        if case == "matter_layers" {

            w.tool = Tool::new("matter_grass");
            w.tool.set_placement([0., 0.40, 0.], 0.);
            w.tool.dump_to_side = true;
            match layered::Layered::new() {
                Ok(mut layers) => {
                    layers.refresh_skin(&w.tool);
                    w.layers = Some(Box::new(layers));
                }
                Err(error) => w.halted = Some(error),
            }
            return w;
        }
        if case == "matter_grass" {
            w.turf = Some(turf::Turf::new());
            return w;
        }
        if matches!(
            case,
            "matter_wood"
                | "matter_wood_chop"
                | "matter_stone"
                | "matter_rubble"
                | "matter_stone_blunt"
        ) {
            match solid::Solid::new(case) {
                Ok(solid) => {
                    w.tool.set_crosscut_height(solid.crosscut_height());
                    if case == "matter_wood" {
                        let (offset, yaw) = solid.splitting_target();
                        w.tool.set_placement(offset, yaw);
                    }
                    w.solid = Some(solid);
                }
                Err(error) => w.halted = Some(error),
            }
            return w;
        }
        let kind = match case {
            "matter_pebbles" => Kind::Sand,
            "matter_sand" => Kind::FineSand,
            _ => Kind::Dirt,
        };
        let spacing = if case == "matter_layer_soil" {
            0.055
        } else if kind == Kind::Sand {
            0.047
        } else if kind == Kind::FineSand {
            // Each sand parcel registers an eight-triangle OGC boundary.
            // Keep the complete patch below the 8192-triangle surface limit.
            0.033
        } else {
            0.073
        };
        // Staggered close-packed material samples inside smooth analytic shapes.
        // There are no hexahedral clods or pre-cut slabs in the visible surface.
        let width: i32 = if kind == Kind::FineSand { 10 } else { 5 };
        for y in 0..9 {
            for z in -width..=width {
                for x in -width..=width {
                    let yy = (if case == "matter_layer_soil" {
                        0.184
                    } else if kind == Kind::Sand {
                        0.030
                    } else if kind == Kind::FineSand {
                        0.015
                    } else {
                        0.035
                    }) + y as f32 * spacing * 0.82;
                    let xx = (x as f32
                        + if (y + z).rem_euclid(2) == 0 {
                            0.25
                        } else {
                            -0.25
                        })
                        * spacing;
                    let zz = (z as f32 + if y % 2 == 0 { 0.25 } else { -0.25 }) * spacing * 0.9;
                    let jitter = [
                        noise(x * 31 + y * 19 + z * 71) * 0.003,
                        noise(x * 17 - y * 23 + z * 13) * 0.002,
                        noise(x * 11 + y * 43 - z * 29) * 0.003,
                    ];
                    let p = add([xx, yy, zz], jitter);
                    let inside = match case {
                        "matter_layer_soil" => xx.abs() < 0.295 && zz.abs() < 0.211 && yy < 0.37,
                        "matter_pebbles" => {
                            yy < 0.25 * (1. - (xx * xx + zz * zz).sqrt() / 0.34).max(0.) + 0.06
                        }
                        "matter_sand" => {
                            (xx / 0.31).powi(2) + (zz / 0.29).powi(2) < 1.
                                && yy
                                    < 0.12 * (1. - (xx * xx + zz * zz).sqrt() / 0.33).max(0.)
                                        + 0.035
                        }
                        _ => {
                            (xx / 0.34).powi(2) + (zz / 0.30).powi(2) < 1.
                                && yy < 0.20 + 0.035 * (xx * 7.).cos() * (zz * 9.).cos()
                        }
                    };
                    if !inside {
                        continue;
                    }
                    let radius = spacing * 0.53;
                    let anchored = if case == "matter_layer_soil" {
                        xx.abs() > 0.245 || zz.abs() > 0.17
                    } else {
                        kind == Kind::Dirt && yy < 0.08
                    };
                    w.particles.push(Particle {
                        p,
                        rest: p,
                        v: [0.; 3],
                        spin: [0.; 3],
                        radius,
                        mass: spacing.powi(3) * 0.74 * kind.law().density,
                        kind,
                        anchored,
                        active: true,
                        level: 0,
                        quiet: 0.,
                        moisture: 0.65,
                        activity: 0.,
                        region: 0,
                    });
                }
            }
        }
        if kind.law().cohesion > 0. {
            for a in 0..w.particles.len() {
                for b in a + 1..w.particles.len() {
                    let d = length(sub(w.particles[a].p, w.particles[b].p));
                    if d < spacing * 1.48 && w.particles[a].region == w.particles[b].region {
                        let area = spacing.powi(2) * 0.24;
                        w.bond(a, b, area, 1., 0.);
                    }
                }
            }
        }
        w.initial_mass = w.particles.iter().map(|p| p.mass).sum();
        if kind == Kind::FineSand {
            w.sand = Some(sand::Sand::new(w.particles.len()));
        }
        w.refresh_shape_neighbours();
        if case == "matter_seedlings" {
            w.garden = Garden::seed(&w.particles);
        }
        w.skin = skin::reconstruct(&mut w);
        w
    }
    pub fn title(&self) -> &str {
        match self.case.as_str() {
            "matter_worker_dig" => "Worker · dig a ground section",
            "matter_worker_axe" => "Worker · chop a standing tree",
            "matter_snow" => "Worker · shovel and compact snow",
            "matter_ice" => "Ice · fracture, float and melt in water",
            "matter_ice_air" => "Ice · melting in air",

            "matter_dirt" => "Loam · spade",
            "matter_grass" => "Rooted turf · cut, peel and lift",
            "matter_seedlings" => "Seedlings · accelerated germination",
            "matter_sand" => "Fine sand · bucket",
            "matter_pebbles" => "Pebbles · bucket",
            "matter_layers" => "Ground section · turf, soil and bedrock",
            "matter_wood" => "Broadleaf trunk · split along grain",
            "matter_wood_chop" => "Broadleaf trunk · chop across grain",
            "matter_stone" => "Solid rock · pickaxe",
            "matter_stone_blunt" => "Solid rock · broad bucket contact",
            _ => "Loose stone · shovel",
        }
    }
    pub fn phase(&self) -> &str {
        if self.comparison.as_ref().is_some_and(|c| c.complete) {
            return "Force comparison complete · reset to replay";
        }
        if let Some(worker)=&self.worker { return worker.phase(); }
        if self.thermal.is_some() { return "Thermal exchange · accelerated time"; }
        if let Some(layers) = &self.layers {
            return layers.phase();
        }
        if self.case == "matter_seedlings" && self.time < 8. {
            "Germination · accelerated growth"
        } else {
            self.tool.phase(self.stroke_time())
        }
    }
    fn stroke_time(&self) -> f32 {
        (self.time
            - if self.case == "matter_seedlings" {
                8.
            } else {
                0.
            })
        .max(0.)
    }
    fn active_count(&self) -> usize {
        self.particles.iter().filter(|p| p.active).count()
    }
    fn cached_neighbour_groups(&mut self, q: &[V]) -> std::sync::Arc<Vec<(Vec<usize>, Vec<usize>)>> {
        let keys:Vec<_>=self.particles.iter().enumerate().filter(|(_,p)|p.active)
            .map(|(i,_)|(i,q[i].map(|v|(v/0.09).floor() as i32))).collect();
        if keys != self.neighbour_keys {
            self.neighbour_cache=std::sync::Arc::new(self.neighbour_groups(q));
            self.neighbour_keys=keys;
        }
        self.neighbour_cache.clone()
    }
    pub fn neighbour_groups(&self, q: &[V]) -> Vec<(Vec<usize>, Vec<usize>)> {
        let cell = 0.09;
        let key = |p: V| p.map(|v| (v / cell).floor() as i32);
        let entries: Vec<_> = self.particles.iter().enumerate().filter(|(_, p)| p.active)
            .map(|(i, _)| (i, key(q[i]))).collect();
        if entries.is_empty() { return Vec::new(); }
        let lo: [i32; 3] = std::array::from_fn(|k| entries.iter().map(|e| e.1[k]).min().unwrap());
        let hi: [i32; 3] = std::array::from_fn(|k| entries.iter().map(|e| e.1[k]).max().unwrap());
        let size: [usize; 3] = std::array::from_fn(|k| (hi[k] as i64 - lo[k] as i64 + 1) as usize);
        if let Some(count) = size[0].checked_mul(size[1]).and_then(|n| n.checked_mul(size[2])).filter(|n| *n <= 65_536) {
            // Compact material occupies a small bounded range of bins. A
            // counting sort avoids 27 hash lookups per bin, twenty times per
            // frame. Preserve the sparse path's lexicographic and particle
            // order so this changes lookup cost, not the constraint solve.
            let index = |k: [i32; 3]| ((k[0] as i64-lo[0] as i64) as usize * size[1]
                + (k[1] as i64-lo[1] as i64) as usize) * size[2] + (k[2] as i64-lo[2] as i64) as usize;
            let mut offsets = vec![0usize; count+1];
            for &(_, k) in &entries { offsets[index(k)+1] += 1; }
            for i in 0..count { offsets[i+1] += offsets[i]; }
            let mut cursor = offsets[..count].to_vec();
            let mut members = vec![0; entries.len()];
            for &(i, k) in &entries {
                let bin = index(k); members[cursor[bin]] = i; cursor[bin] += 1;
            }
            let mut groups = Vec::new();
            for bin in 0..count {
                if offsets[bin] == offsets[bin+1] { continue; }
                let k = [lo[0] as i64 + (bin/(size[1]*size[2])) as i64,
                    lo[1] as i64 + ((bin/size[2])%size[1]) as i64,
                    lo[2] as i64 + (bin%size[2]) as i64];
                let mut nearby = Vec::new();
                for z in -1..=1 { for y in -1..=1 { for x in -1..=1 {
                    let n = [k[0]+x, k[1]+y, k[2]+z];
                    if (0..3).any(|a| n[a] < lo[a] as i64 || n[a] > hi[a] as i64) { continue; }
                    let i = index(n.map(|v|v as i32));
                    nearby.extend_from_slice(&members[offsets[i]..offsets[i+1]]);
                }}}
                groups.push((members[offsets[bin]..offsets[bin+1]].to_vec(), nearby));
            }
            return groups;
        }
        // Widely scattered debris must not allocate a box spanning the world.
        let mut bins = HashMap::<[i32; 3], Vec<usize>>::new();
        for (i, k) in entries {
            bins.entry(k).or_default().push(i);
        }
        let mut groups = Vec::with_capacity(bins.len());
        let mut keys: Vec<_> = bins.keys().copied().collect();
        keys.sort_unstable();
        for k in keys {
            let mut nearby = Vec::new();
            for z in -1..=1 {
                for y in -1..=1 {
                    for x in -1..=1 {
                        if let Some(other) = bins.get(&[k[0] + x, k[1] + y, k[2] + z]) {
                            nearby.extend_from_slice(other);
                        }
                    }
                }
            }
            // Stream pairs during the solve instead of allocating the complete
            // pair list. Neighbour bins are looked up once per occupied bin.
            groups.push((bins[&k].clone(), nearby));
        }
        groups
    }
    fn positions(&self) -> Vec<V> {
        let mut out = Vec::with_capacity(self.contact_order.len() * 6 + self.tool.p.len() + 4);
        for &i in &self.contact_order {
            let p = &self.particles[i];
            for a in AXES {
                out.push(add(p.p, scale(a, p.radius * 0.88)));
            }
        }
        out.extend(self.garden.contact_positions());
        out.extend(&self.tool.p);
        out.extend(&self.tool.ground);
        out
    }
    pub fn configure(&mut self, host: &mut Host) -> Result<(), String> {
        if self.thermal.is_some() && self.solid.is_some() {return self.configure_ice(host);}
        if let Some(error) = &self.halted {
            return Err(error.clone());
        }
        if let Some(solid) = &mut self.solid {
            return solid.configure(host, &self.tool);
        }
        if let Some(turf) = &mut self.turf {
            return turf.configure(host, &self.tool);
        }
        if let Some(layers) = &mut self.layers {
            return layers.configure(host, &self.tool);
        }
        self.contact_order = self
            .particles
            .iter()
            .enumerate()
            .filter(|(_, p)| p.active)
            .map(|(i, _)| i)
            .collect();
        let samples = self.contact_order.len() * 6;
        let n = samples + self.garden.contact_positions().len();
        let m = self.tool.p.len();
        let mut movable: Vec<_> = self.contact_order.iter().flat_map(|&i| [!self.particles[i].anchored; 6]).collect();
        movable.resize(n, true);
        let mut faces: Vec<_> = (0..self.contact_order.len())
            .flat_map(|i| OCT.map(|f| f.map(|v| v + i as u32 * 6)))
            .collect();
        faces.extend(self.garden.contact_faces(samples as u32));
        // Material self contact is the internal pressure/friction solve above.
        // OGC resolves the closed particle skins against the actual tool shell.
        let spec = json!({"positions":self.positions(),"contact_radius":0.002,"contact_stiffness":30000.,"surfaces":[
            {"start":0,"count":n,"triangles":faces,"motion":0,"self_collision":false,"friction":0.65,"movable_vertices":movable},
            {"start":n,"count":m,"triangles":self.tool.faces,"motion":0,"self_collision":false,"friction":0.55},
            coupling::table_surface(&self.tool,n+m)]});
        host.invoke("OGCConnectionSystem.Configure", spec)?;
        self.topology_dirty = false;
        Ok(())
    }
    fn advance_frame(&mut self, host: &mut Host) -> Result<(), String> {
        if self.halted.is_some() {
            return Ok(());
        }
        if self.thermal.is_some() && self.solid.is_some() {
            let checkpoint=self.clone();
            if let Err(e)=self.advance_thermal(host).and_then(|_|self.advance_ice(host)) {
                *self=checkpoint;self.configure(host)?;return Err(e);
            }
            return Ok(());
        }
        if self.thermal.is_some() {self.advance_thermal(host)?;}

        if self.layers.is_some() {
            return self.advance_layers(host);
        }
        if self.solid.is_some() {
            return self.advance_solid(host);
        }
        if self.turf.is_some() {
            return self.advance_turf(host);
        }
        let wall = std::time::Instant::now();
        // Topology changes occur only between contact batches.
        if self.tool.manual.is_none() && self.frames % 12 == 0 {
            self.adapt();
        }
        if self.topology_dirty {
            self.configure(host)?;
            self.refresh_shape_neighbours();
        }
        self.timings[0] += wall.elapsed().as_secs_f64() * 1000.;
        let checkpoint = (
            self.particles.clone(),
            self.bonds.clone(),
            self.tool.clone(),
            self.garden.clone(),
            self.time,
            self.fractures,
            self.sand.clone(),
        );
        let dt = 1. / 240.;
        for _ in 0..4 {
            if let Err(error) = self.substep(host, dt) {
                (
                    self.particles,
                    self.bonds,
                    self.tool,
                    self.garden,
                    self.time,
                    self.fractures,
                    self.sand,
                ) = checkpoint;
                self.configure(host)?;
                return Err(error);
            }
            self.time += dt;
        }
        self.frames += 1;
        self.peak_bucket = self.peak_bucket.max(
            self.tool.contained_grains(
                self.particles
                    .iter()
                    .filter(|p| p.active)
                    .map(|p| (p.p, p.radius)),
            ),
        );
        let supported = self.supported_samples();
        self.peak_lifted = self.peak_lifted.max(
            self.particles
                .iter()
                .enumerate()
                .filter(|(i, p)| p.active && !supported[*i] && p.p[1] - p.rest[1] > 0.05)
                .count() as u64,
        );
        // Reconstruct from accepted matter; cache between visual updates.
        if self.frames % 4 == 0 {
            let skin_start = std::time::Instant::now();
            self.skin = skin::reconstruct_with_budget(
                self,
                if self.tool.manual.is_some() {
                    6000
                } else {
                    16500
                },
            );
            self.timings[5] += skin_start.elapsed().as_secs_f64() * 1000.;
        }
        self.step_ms = wall.elapsed().as_secs_f64() * 1000.;
        self.total_step_ms += self.step_ms;
        self.worst_step_ms = self.worst_step_ms.max(self.step_ms);
        Ok(())
    }
    fn advance_solid(&mut self, host: &mut Host) -> Result<(), String> {
        let wall = std::time::Instant::now();
        let checkpoint = (self.solid.clone(), self.tool.clone(), self.time);
        for _ in 0..4 {
            self.prepare_worker(1./240.);
            match self
                .solid
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
                    self.solver_iterations += 5;
                    self.time += 1. / 240.;
                }
                Err(error) => {
                    (self.solid, self.tool, self.time) = checkpoint;
                    self.solid.as_mut().unwrap().configure(host, &self.tool)?;
                    return Err(error);
                }
            }
        }
        self.frames += 1;
        self.peak_lifted = self
            .peak_lifted
            .max(self.solid.as_ref().unwrap().lifted_count() as u64);
        self.step_ms = wall.elapsed().as_secs_f64() * 1000.;
        self.total_step_ms += self.step_ms;
        self.worst_step_ms = self.worst_step_ms.max(self.step_ms);
        Ok(())
    }
    fn advance_turf(&mut self, host: &mut Host) -> Result<(), String> {
        let wall = std::time::Instant::now();
        let checkpoint = (self.turf.clone(), self.tool.clone(), self.time);
        for _ in 0..4 {
            self.prepare_worker(1./240.);
            match self
                .turf
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
                    self.solver_iterations += 5;
                    self.time += 1. / 240.;
                }
                Err(error) => {
                    (self.turf, self.tool, self.time) = checkpoint;
                    self.turf.as_mut().unwrap().configure(host, &self.tool)?;
                    return Err(error);
                }
            }
        }
        self.frames += 1;
        if self.comparison.is_some() {
            self.peak_lifted = self.peak_lifted.max(
                self.turf.as_ref().unwrap().report()["lifted_elements"]
                    .as_u64()
                    .unwrap_or(0),
            );
        }
        self.step_ms = wall.elapsed().as_secs_f64() * 1000.;
        self.total_step_ms += self.step_ms;
        self.worst_step_ms = self.worst_step_ms.max(self.step_ms);
        Ok(())
    }
    fn substep(&mut self, host: &mut Host, dt: f32) -> Result<(), String> {
        self.prepare_worker(dt);
        let old: Vec<_> = self.particles.iter().map(|p| p.p).collect();
        let previous = self.positions();
        let tool_old = self.tool.p.clone();
        let mut predicted = old.clone();
        for (i, p) in self
            .particles
            .iter_mut()
            .enumerate()
            .filter(|(_, p)| p.active && !p.anchored)
        {
            predicted[i] = add(p.p, add(scale(p.v, dt), [0., -9.81 * dt * dt, 0.]));
        }
        if let Some(sand) = &mut self.sand {
            predicted = sand.predict(&self.particles, dt)?;
        }
        let tool_goal = self.tool.goal(self.stroke_time(), dt);
        self.reset_contact_lambda();
        let mut material_shift = vec![[0.; 3]; self.particles.len()];
        for b in &mut self.bonds {
            b.lambda = 0.;
            b.peak = 0.;
        }
        self.garden
            .advance(&mut self.particles, dt, self.case == "matter_seedlings");
        let shoot_initial = self.garden.shoot_positions();
        self.garden.predict(dt);
        let shoot_predicted = self.garden.shoot_positions();
        self.garden.restore_shoots(&shoot_initial);
        let mut shoot_shift = vec![[0.; 3]; shoot_initial.len()];
        for _ in 0..5 {
            self.solver_iterations += 1;
            let mut q: Vec<_> = self.particles.iter().map(|p| p.p).collect();
            let material_start = std::time::Instant::now();
            if self.sand.is_none() {
                self.solve_material(&mut q, &old, dt);
            }
            let shoot_old = self.garden.shoot_positions();
            self.garden.solve(&mut q, &self.particles, dt);
            let mut shoot_goal = self.garden.shoot_positions();
            for i in 0..shoot_goal.len() {
                shoot_shift[i] = add(shoot_shift[i], sub(shoot_goal[i], shoot_old[i]));
                shoot_goal[i] = add(shoot_predicted[i], shoot_shift[i]);
            }
            self.garden.restore_shoots(&shoot_old);
            // The inertial target is fixed for the entire substep. Replacing
            // it by the last iterate reapplied contact acceleration five times
            // and made quiet grains continually rearrange themselves.
            for i in 0..q.len() {
                material_shift[i] = add(material_shift[i], sub(q[i], self.particles[i].p));
                q[i] = add(predicted[i], material_shift[i]);
            }
            self.timings[1] += material_start.elapsed().as_secs_f64() * 1000.;
            let current = self.positions();
            let contact_start = std::time::Instant::now();
            let batch = host.contact_responses(&current, &previous)?;
            self.timings[2] += contact_start.elapsed().as_secs_f64() * 1000.;
            self.timings[3] += batch.broad_ms;
            self.timings[4] += batch.narrow_ms;
            let responses = batch.responses;
            self.contacts = batch.contacts;
            for (slot, &i) in self.contact_order.iter().enumerate() {
                let p = &mut self.particles[i];
                if p.anchored {
                    continue;
                }
                let response = &responses[slot * 6..slot * 6 + 6];
                let delta = translation(p.p, q[i], p.mass, response, dt);
                p.p = add(p.p, delta);
                let force = response
                    .iter()
                    .map(|r| length([r[0], r[1], r[2]]))
                    .sum::<f32>();
                self.max_force = self.max_force.max(force);
            }
            let sample_end = self.contact_order.len() * 6;
            let n = sample_end + self.garden.contact_positions().len();
            self.garden
                .contact(&shoot_goal, &responses[sample_end..n], dt);
            let response = &responses[n..n + self.tool.p.len()];
            self.tool.project(&tool_goal, response, dt)?;
            if self.solver_iterations % 5 == 0 {
                self.tool.record_contact(&tool_old, response, dt);
            }
        }
        for (i, p) in self
            .particles
            .iter_mut()
            .enumerate()
            .filter(|(_, p)| p.active)
        {
            p.v = scale(sub(p.p, old[i]), (-1.2 * dt).exp() / dt);
            p.quiet = if length(p.v) < 0.045 {
                p.quiet + dt
            } else {
                0.
            };
            p.activity *= (-3. * dt).exp();
            if !p.p.iter().chain(&p.v).all(|v| v.is_finite()) {
                return Err("non-finite material state".into());
            }
        }
        self.tool.finish(&tool_old, dt);
        if let Some(sand) = &mut self.sand {
            sand.finish(&mut self.particles, &old, dt)?;
        }
        self.garden.finish(dt);
        if self.finish_material(dt) {
            self.refresh_shape_neighbours();
        }
        Ok(())
    }
    pub fn fail(&mut self, error: String) {
        self.halted = Some(error);
    }
    fn supported_samples(&self) -> Vec<bool> {
        let mut supported: Vec<_> = self
            .particles
            .iter()
            .map(|p| p.active && p.anchored)
            .collect();
        loop {
            let mut change = false;
            for b in self.bonds.iter().filter(|b| b.active) {
                if supported[b.a] != supported[b.b] {
                    supported[b.a] = true;
                    supported[b.b] = true;
                    change = true;
                }
            }
            if !change {
                break;
            }
        }
        supported
    }
    pub fn report(&self) -> Value {
        let supported = self.supported_samples();
        let mass: f32 = self
            .particles
            .iter()
            .filter(|p| p.active)
            .map(|p| p.mass)
            .sum();
        let kinetic = self
            .particles
            .iter()
            .filter(|p| p.active)
            .map(|p| 0.5 * p.mass * dot(p.v, p.v))
            .sum::<f32>();
        let max_speed = self
            .particles
            .iter()
            .filter(|p| p.active)
            .map(|p| length(p.v))
            .fold(0f32, f32::max);
        let lifted = self
            .particles
            .iter()
            .enumerate()
            .filter(|(i, p)| p.active && !supported[*i] && p.p[1] - p.rest[1] > 0.05)
            .count();
        let mut report = json!({"case":self.case,"title":self.title(),"phase":self.phase(),"frames":self.frames,"simulated_seconds":self.time,
          "active_samples":self.active_count(),"refinements":self.splits,"coarsenings":self.merges,"fractures":self.fractures,"lifted_samples":lifted,
          "mass_kg":mass,"initial_mass_kg":self.initial_mass,"peak_contact_pairs":self.contacts,
          "material_kinetic_energy_j":kinetic,"maximum_sample_speed_m_s":max_speed,
          "grains_inside_bucket":self.tool.contained_grains(self.particles.iter().filter(|p|p.active).map(|p|(p.p,p.radius))),
          "step_ms_last":self.step_ms,"step_ms_average":self.total_step_ms/self.frames.max(1) as f64,"step_ms_worst":self.worst_step_ms,
          "substeps_per_frame":4,"maximum_iterations_per_substep":5,"iterations_per_substep_average":self.solver_iterations as f64 / (4*self.frames.max(1)) as f64,
          "timing_ms_per_step":{"adapt":self.timings[0]/self.frames.max(1) as f64,"material":self.timings[1]/self.frames.max(1) as f64,"contact":self.timings[2]/self.frames.max(1) as f64,"contact_broad":self.timings[3]/self.frames.max(1) as f64,"contact_narrow":self.timings[4]/self.frames.max(1) as f64,"skin":self.timings[5]/self.frames.max(1) as f64},
          "peak_contact_force_n":self.max_force,"plants":self.garden.report(),"halted":self.halted.is_some(),"failure":self.halted,
          "model":"adaptive meshfree elastic/plastic bonds, granular friction, OGC tool contact, implicit visual skin",
          "calibrated":false,"continuous_collision_detection":false});
        if let Some(solid) = &self.solid {
            let state = solid.report();
            report["model"] = state["model"].clone();
            report["active_samples"] = state["cells"].clone();
            report["fractures"] = state["broken_interfaces"].clone();
            report["lifted_samples"] = state["lifted_cells"].clone();
            report["mass_kg"] = state["mass_kg"].clone();
            report["initial_mass_kg"] = state["mass_kg"].clone();
            report["maximum_sample_speed_m_s"] = state["maximum_vertex_speed_m_s"].clone();
            report["material_kinetic_energy_j"] = state["kinetic_energy_j"].clone();
            report["solid"] = state;
        }
        if let Some(turf) = &self.turf {
            let state = turf.report();
            report["model"] = state["model"].clone();
            report["active_samples"] = state["elements"].clone();
            report["fractures"] = state["severed_roots"].clone();
            report["lifted_samples"] = state["lifted_elements"].clone();
            report["mass_kg"] = state["mass_kg"].clone();
            report["initial_mass_kg"] = state["mass_kg"].clone();
            report["maximum_sample_speed_m_s"] = state["maximum_node_speed_m_s"].clone();
            report["material_kinetic_energy_j"] = state["kinetic_energy_j"].clone();
            report["turf"] = state;
        }
        if let Some(sand) = &self.sand {
            report["sand"] = sand.report();
            if self.case=="matter_snow" {
                report["sand"]["model"]=json!("snow: APIC elastoplastic compression, tensile yield, permanent densification and hardening; melt becomes liquid");
            } else if self.particles.iter().all(|p|p.kind==Kind::Water) {
                report["sand"]["model"]=json!("water: APIC weakly compressible pressure and viscosity with OGC contact");
            }

            report["model"] = report["sand"]["model"].clone();
        }
        if let Some(layers) = &self.layers {
            let state = layers.report();
            report["mass_kg"] = state["mass_kg"].clone();
            report["initial_mass_kg"] = state["initial_mass_kg"].clone();
            report["model"] =
                json!("coupled turf, cohesive soil and brittle bedrock, shared OGC contact");
            report["layers"] = state;
        }
        report["tool"] = self.tool.report(self.stroke_time());
        report["muscles"]=self.tool.muscles.report();
        if let Some(w)=&self.worker { report["worker"]=w.report(&self.tool); }
        if let Some(t)=&self.thermal {
            report["thermal"]=t.report(self);
            if self.solid.is_some() {
                report["initial_mass_kg"]=json!(self.initial_mass);
                report["mass_kg"]=report["thermal"]["accounted_specimen_mass_kg"].clone();
                report["model"]=json!("brittle ice, hydrostatic buoyancy, enthalpy phase change and OGC-contact meltwater");
            }
        }

        if let Some(c) = &self.comparison {
            report["comparison"] = json!({"trial":c.trial+1,"label":trials::LABELS[c.trial],"complete":c.complete,
                "drive_limit_n":trials::LIMITS[c.trial],"seconds_per_trial":trials::DURATION,"fresh_specimens":true,"results":c.outcomes});
            report["simulated_seconds"] = json!(self.time + c.elapsed);
        }
        report["peak_lifted_samples"] = json!(self.peak_lifted);
        report["peak_bucket_samples"] = json!(self.peak_bucket);
        report
    }
    pub fn mesh(&self) -> Value {
        let mut v = self.skin.clone();
        if let Some(solid) = &self.solid {
            solid.render(&mut v);
        }
        if let Some(turf) = &self.turf {
            turf.render(&mut v);
        }
        if let Some(layers) = &self.layers {
            layers.render(&mut v);
        }
        self.tool.render(&mut v);
        if let Some(w)=&self.worker { w.render(&self.tool,&mut v); }
        if let Some(t)=&self.thermal { t.render(&mut v); }
        self.garden.render(&self.particles, &mut v);
        skin::table(&mut v);
        json!({"kind":"visual","shader":"material_surface","vertices":v})
    }
}
fn noise(i: i32) -> f32 {
    let mut x = (i as u32).wrapping_mul(747796405).wrapping_add(2891336453);
    x = ((x >> ((x >> 28) + 4)) ^ x).wrapping_mul(277803737);
    (((x >> 22) ^ x) as f32 / u32::MAX as f32) * 2. - 1.
}
fn translation(current: V, goal: V, mass: f32, rs: &[[f32; 13]], dt: f32) -> V {
    let inertia = mass / (dt * dt);
    let mut matrix = [[0f32; 3]; 3];
    for (i, row) in matrix.iter_mut().enumerate() {
        row[i] = inertia;
    }
    let mut rhs = scale(sub(goal, current), inertia);
    let mut bound = f32::INFINITY;
    for r in rs {
        for i in 0..3 {
            rhs[i] += r[i];
            for j in 0..3 {
                matrix[i][j] += r[3 + j * 3 + i];
            }
        }
        bound = bound.min(r[12].max(0.));
    }
    let mut a = matrix;
    let mut b = rhs;
    for k in 0..3 {
        let d = a[k][k].max(1e-8);
        for j in k..3 {
            a[k][j] /= d;
        }
        b[k] /= d;
        for i in 0..3 {
            if i == k {
                continue;
            }
            let f = a[i][k];
            for j in k..3 {
                a[i][j] -= f * a[k][j];
            }
            b[i] -= f * b[k];
        }
    }
    let limit = bound.min(0.008);
    scale(b, (limit / length(b).max(1e-12)).min(1.))
}
