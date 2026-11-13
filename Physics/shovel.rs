//! Local, bounded shovel/contact experiment. The clod is pre-separated elastic soil,
//! not a terrain subtraction or a virgin-soil fracture implementation. Its nodes
//! are never pinned to the shovel: only XPBD material constraints and the retained
//! OGC adaptation support or move it. OGC provenance remains in Physics/OGC.
use super::{Scene, math::*};
use crate::host::Host;
use serde_json::{Value, json};
use std::collections::BTreeMap;

type D = [f64; 3];
const STEP: f32 = 1. / 240.;
const MAX_STEPS: usize = 8;
const ITERATIONS: usize = 3;
const STROKE_SECONDS: f32 = 6.2;
const LEFT_GRIP: V = [0., 0.40, -0.13];
const RIGHT_GRIP: V = [0., 1.0, -0.13];

fn d_dot(a: D, b: D) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn d_cross(a: D, b: D) -> D {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn d_unit(a: D) -> D {
    let n = d_dot(a, a).sqrt();
    a.map(|x| x / n)
}
fn d_point(value: &Value) -> Result<D, String> {
    let p: D = serde_json::from_value(value.clone())
        .map_err(|e| format!("three coordinates required: {e}"))?;
    if p.iter().any(|v| !v.is_finite()) {
        return Err("coordinates must be finite".into());
    }
    Ok(p)
}
fn vector(value: &Value) -> bool {
    d_point(value).is_ok()
}
fn direction(value: &Value) -> bool {
    d_point(value).is_ok_and(|p| p.iter().all(|x| x.abs() <= 1e6) && d_dot(p, p) > 0.1)
}

pub(crate) fn valid_configuration(value: &Value) -> bool {
    if !vector(&value["position"]) || !direction(&value["forward"]) || !direction(&value["up"]) {
        return false;
    }
    if !value["right"].is_null() && !direction(&value["right"]) {
        return false;
    }
    let Some(vertices) = value["blade_collision"]["vertices"].as_array() else {
        return false;
    };
    let Some(triangles) = value["blade_collision"]["triangles"].as_array() else {
        return false;
    };
    if !(3..=256).contains(&vertices.len()) || !(1..=512).contains(&triangles.len()) {
        return false;
    }
    if !vertices
        .iter()
        .all(|v| d_point(v).is_ok_and(|p| p.iter().all(|x| x.abs() <= 2.)))
    {
        return false;
    }
    if !triangles.iter().all(|f| {
        f.as_array().is_some_and(|f| {
            f.len() == 3
                && f.iter()
                    .all(|i| i.as_u64().is_some_and(|i| (i as usize) < vertices.len()))
        })
    }) {
        return false;
    }
    for grip in ["left", "right"] {
        if !value["grip_positions"][grip].is_null()
            && !d_point(&value["grip_positions"][grip])
                .is_ok_and(|p| p.iter().all(|v| v.abs() <= 2.))
        {
            return false;
        }
    }
    true
}
pub(crate) fn valid_step(value: &Value) -> bool {
    value["dt"]
        .as_f64()
        .is_some_and(|dt| dt.is_finite() && (0. ..=1. / 15.).contains(&dt))
        && value["action"].is_boolean()
        && (value["reset"].is_null() || value["reset"].is_boolean())
}

#[derive(Clone, Copy)]
struct Pose {
    position: V,
    pitch: f32,
}
impl Pose {
    fn point(self, p: V) -> V {
        add(self.position, rotate_x(p, [0.; 3], self.pitch))
    }
}
fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
fn pose(time: Option<f32>) -> Pose {
    // Lower into a prepared central slit, insert under the clod, lift, tip, and
    // return. These are target tool motions, never target material positions.
    let keys = [
        (0., [0., 0.50, -0.45], 0.),
        (0.7, [0., 0.125, -0.45], 0.),
        (1.9, [0., 0.125, 0.07], 0.),
        (3.3, [0., 0.64, 0.07], 0.),
        (4.3, [0., 0.64, 0.07], 1.25),
        (5.2, [0., 0.64, 0.07], 1.25),
        (STROKE_SECONDS, [0., 0.50, -0.45], 0.),
    ];
    let t = time.unwrap_or(0.).clamp(0., STROKE_SECONDS);
    for k in keys.windows(2) {
        if t <= k[1].0 {
            let u = ((t - k[0].0) / (k[1].0 - k[0].0)).clamp(0., 1.);
            let u = u * u * (3. - 2. * u);
            return Pose {
                position: std::array::from_fn(|i| mix(k[0].1[i], k[1].1[i], u)),
                pitch: mix(k[0].2, k[1].2, u),
            };
        }
    }
    Pose {
        position: keys[0].1,
        pitch: 0.,
    }
}
fn stage(time: Option<f32>) -> &'static str {
    match time {
        None => "ready",
        Some(t) if t < 0.7 => "lowering",
        Some(t) if t < 1.9 => "inserting",
        Some(t) if t < 3.3 => "lifting",
        Some(t) if t < 4.3 => "tipping",
        Some(t) if t < 5.2 => "dumping",
        Some(t) if t < STROKE_SECONDS => "returning",
        _ => "finished",
    }
}

#[derive(Clone)]
struct Fixture {
    scene: Scene,
    anchor: D,
    basis: [D; 3],
    blade: Vec<V>,
    tool_start: usize,
    soil_start: usize,
    soil_count: usize,
    grips: [V; 2],
}
impl Fixture {
    fn global(&self, p: V) -> D {
        std::array::from_fn(|i| {
            self.anchor[i] + (0..3).map(|j| self.basis[j][i] * p[j] as f64).sum::<f64>()
        })
    }
    fn axis(&self, p: V) -> D {
        std::array::from_fn(|i| (0..3).map(|j| self.basis[j][i] * p[j] as f64).sum())
    }
    fn spec(&self) -> Value {
        let mut spec = self.scene.spec();
        spec["contact_stiffness"] = json!(20000.);
        for s in spec["surfaces"].as_array_mut().unwrap() {
            s["friction"] = json!(0.65);
        }
        // The last contact surface is the rigidly driven blade, not a static
        // obstacle. Only the bounded native connection owns the actual context.
        spec["surfaces"].as_array_mut().unwrap().last_mut().unwrap()["motion"] = json!(1);
        spec
    }
    fn set_tool(&mut self, p: Pose) {
        for (i, local) in self.blade.iter().enumerate() {
            self.scene.p[self.tool_start + i] = p.point(*local);
        }
    }
    fn centroid(&self) -> V {
        let total = self.scene.p[self.soil_start..self.soil_start + self.soil_count]
            .iter()
            .fold([0.; 3], |a, p| add(a, *p));
        scale(total, 1. / self.soil_count as f32)
    }
    fn mesh(&self) -> Value {
        let vertices: Vec<[f64; 6]> = self
            .scene
            .faces()
            .into_iter()
            // The tool is drawn from its Blender appearance asset. The broad
            // floor is contact-only: the actual terrain draws the surrounding ground.
            .filter(|f| f.iter().all(|i| *i >= 4 && *i < self.tool_start))
            .flat_map(|f| {
                let [a, b, c] = f.map(|i| self.scene.p[i]);
                let light = 0.38
                    + 0.62 * dot(unit(cross(sub(b, a), sub(c, a))), unit([0.4, 0.8, 0.5])).abs();
                f.map(|i| {
                    let p = self.global(self.scene.p[i]);
                    let c = self.scene.colors[i].map(|v| (v * light) as f64);
                    [p[0], p[1], p[2], c[0], c[1], c[2]]
                })
            })
            .collect();
        json!({"kind":"visual","shader":"material_surface","vertices":vertices})
    }
    fn valid(&self) -> Result<(), String> {
        if self.scene.p.iter().flatten().any(|v| !v.is_finite()) {
            return Err("non-finite shovel material state".into());
        }
        if self
            .scene
            .velocity
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 40.)
        {
            return Err("soil velocity exceeded the experiment's 40 m/s safety bound".into());
        }
        let inverted = self
            .scene
            .tets
            .iter()
            .filter(|tet| {
                let p = tet.ids.map(|i| self.scene.p[i]);
                dot(sub(p[1], p[0]), cross(sub(p[2], p[0]), sub(p[3], p[0]))) / 6. * tet.rest <= 0.
            })
            .count();
        if inverted > 0 {
            return Err(format!(
                "shovel contact step rejected: {inverted} inverted soil tetrahedra; last valid state retained"
            ));
        }
        let intersections = self.scene.intersection_count();
        if intersections > 0 {
            return Err(format!(
                "shovel contact step rejected: {intersections} intersecting triangle pairs; last valid state retained"
            ));
        }
        Ok(())
    }
}
fn box_solid(scene: &mut Scene, lo: V, hi: V) {
    let start = scene.p.len();
    for p in [
        [lo[0], lo[1], lo[2]],
        [hi[0], lo[1], lo[2]],
        [hi[0], lo[1], hi[2]],
        [lo[0], lo[1], hi[2]],
        [lo[0], hi[1], lo[2]],
        [hi[0], hi[1], lo[2]],
        [hi[0], hi[1], hi[2]],
        [lo[0], hi[1], hi[2]],
    ] {
        scene.node(p, 0., [0.36, 0.32, 0.27]);
    }
    scene.surface(
        start,
        vec![
            [0, 1, 2],
            [0, 2, 3],
            [4, 6, 5],
            [4, 7, 6],
            [0, 5, 1],
            [0, 4, 5],
            [3, 2, 6],
            [3, 6, 7],
            [1, 5, 6],
            [1, 6, 2],
            [0, 3, 7],
            [0, 7, 4],
        ],
        true,
        false,
    );
}
fn fixture(input: &Value) -> Result<Fixture, String> {
    if !valid_configuration(input) {
        return Err("invalid shovel fixture configuration".into());
    }
    let anchor = d_point(&input["position"])?;
    let up = d_unit(d_point(&input["up"])?);
    let forward = d_point(&input["forward"])?;
    let parallel = d_dot(forward, up);
    let forward = std::array::from_fn(|i| forward[i] - parallel * up[i]);
    if d_dot(forward, forward) < 1e-8 {
        return Err("shovel forward direction must have a tangent component".into());
    }
    let forward = d_unit(forward);
    let right = d_unit(d_cross(up, forward));
    if !input["right"].is_null() && d_dot(d_unit(d_point(&input["right"])?), right) < 0.8 {
        return Err("shovel basis is reflected or inconsistent".into());
    }
    let blade: Vec<V> = serde_json::from_value(input["blade_collision"]["vertices"].clone())
        .map_err(|e| e.to_string())?;
    let triangles: Vec<[u32; 3]> =
        serde_json::from_value(input["blade_collision"]["triangles"].clone())
            .map_err(|e| e.to_string())?;
    if triangles.iter().any(|f| {
        let [a, b, c] = f.map(|i| blade[i as usize]);
        length(cross(sub(b, a), sub(c, a))) < 1e-8
    }) {
        return Err("degenerate shovel collision triangle".into());
    }
    let mut scene = Scene::new("shovel_preseparated_clod");
    scene.radius = 0.004;
    for p in [[-2., 0., -2.], [2., 0., -2.], [2., 0., 2.], [-2., 0., 2.]] {
        scene.node(p, 0., [0.25, 0.29, 0.20]);
    }
    scene.surface(0, vec![[0, 2, 1], [0, 3, 2]], true, false);
    box_solid(&mut scene, [-0.4, 0.012, -0.28], [0.4, 0.055, 0.50]);
    // Side ledges support a pre-separated clod over a clear central slit.
    // This isolates contact and lifting without pretending to cut intact soil.
    box_solid(&mut scene, [-0.24, 0.065, -0.10], [-0.147, 0.22, 0.34]);
    box_solid(&mut scene, [0.147, 0.065, -0.10], [0.24, 0.22, 0.34]);
    let soil_start = scene.p.len();
    let (nx, ny, nz) = (4, 2, 4);
    let soil_count = (nx + 1) * (ny + 1) * (nz + 1);
    let index = |x: usize, y: usize, z: usize| soil_start + x + (nx + 1) * (z + (nz + 1) * y);
    for y in 0..=ny {
        for z in 0..=nz {
            for x in 0..=nx {
                let shade = ((x * 17 + y * 37 + z * 11) % 13) as f32 / 130.;
                scene.node(
                    [
                        -0.17 + 0.34 * x as f32 / nx as f32,
                        0.236 + 0.12 * y as f32 / ny as f32,
                        -0.02 + 0.30 * z as f32 / nz as f32,
                    ],
                    3.6 / soil_count as f32,
                    [0.29 + shade, 0.17 + shade * 0.6, 0.075 + shade * 0.25],
                );
            }
        }
    }
    let mut boundary: BTreeMap<[usize; 3], ([usize; 3], usize)> = BTreeMap::new();
    for y in 0..ny {
        for z in 0..nz {
            for x in 0..nx {
                let a = index(x, y, z);
                let b = index(x + 1, y, z);
                let c = index(x, y, z + 1);
                let d = index(x + 1, y, z + 1);
                let u = (nx + 1) * (nz + 1);
                for ids in [
                    [a, b, d, d + u],
                    [a, d, c, d + u],
                    [a, c, c + u, d + u],
                    [a, c + u, a + u, d + u],
                    [a, a + u, b + u, d + u],
                    [a, b + u, b, d + u],
                ] {
                    scene.tetra(ids);
                    for opposite in 0..4 {
                        let mut f: [usize; 3] =
                            std::array::from_fn(|i| ids[(i + opposite + 1) % 4]);
                        let [p, q, r] = f.map(|i| scene.p[i]);
                        if dot(cross(sub(q, p), sub(r, p)), sub(scene.p[ids[opposite]], p)) > 0. {
                            f.swap(1, 2);
                        }
                        let mut key = f;
                        key.sort();
                        boundary
                            .entry(key)
                            .and_modify(|entry| entry.1 += 1)
                            .or_insert((f, 1));
                    }
                }
            }
        }
    }
    for edge in &mut scene.edges {
        edge.compliance = 3e-7;
    }
    let faces = boundary
        .into_values()
        .filter(|(_, count)| *count == 1)
        .map(|(f, _)| f.map(|i| (i - soil_start) as u32))
        .collect();
    scene.surface(soil_start, faces, false, false);
    let tool_start = scene.p.len();
    for p in &blade {
        scene.node(pose(None).point(*p), 0., [0.55, 0.58, 0.61]);
    }
    scene.surface(tool_start, triangles, true, false);
    scene.proxy="Pre-separated cohesive elastic clod on a prepared bedrock cradle. No fracture, plastic soil flow, excavation subtraction, adaptive remeshing or equal-fidelity claim to the OGC authors' solver. Kinematic tool; dynamic soil has no tool pins.".into();
    scene.finish()?;
    let grips = ["left", "right"].map(|name| {
        if input["grip_positions"][name].is_null() {
            if name == "left" {
                LEFT_GRIP
            } else {
                RIGHT_GRIP
            }
        } else {
            serde_json::from_value(input["grip_positions"][name].clone()).unwrap()
        }
    });
    let fixture = Fixture {
        scene,
        anchor,
        basis: [right, up, forward],
        blade,
        tool_start,
        soil_start,
        soil_count,
        grips,
    };
    fixture.valid()?;
    Ok(fixture)
}

#[derive(Default)]
pub(crate) struct State {
    fixture: Option<Fixture>,
    configuration: Value,
    phase: Option<f32>,
    last_action: bool,
    accumulator: f32,
    frames: u64,
    substeps: u64,
    blocked_steps: u64,
    blocked: bool,
    contacts: Value,
    halt: Option<String>,
    discarded_seconds: f64,
    last_ms: f64,
    peak_centroid: f32,
    quiet_seconds: f32,
    maximum_soil_speed: f32,
}
impl State {
    pub(crate) fn active(&self) -> bool {
        self.fixture.is_some()
    }
    fn configure(&mut self, host: &mut Host, input: &Value) -> Result<(), String> {
        let fixture = fixture(input)?;
        host.invoke("OGCConnectionSystem.Configure", fixture.spec())?;
        let peak_centroid = fixture.centroid()[1];
        *self = Self {
            fixture: Some(fixture),
            configuration: input.clone(),
            peak_centroid,
            ..Default::default()
        };
        Ok(())
    }
    fn restore(&mut self, host: &mut Host, backup: Fixture) -> Result<(), String> {
        self.fixture = Some(backup);
        host.invoke(
            "OGCConnectionSystem.Configure",
            self.fixture.as_ref().unwrap().spec(),
        )?;
        Ok(())
    }
    fn candidate(&mut self, host: &mut Host, target: Pose) -> Result<(Value, f32), String> {
        let f = self
            .fixture
            .as_mut()
            .ok_or("shovel fixture is not configured")?;
        let previous = f.scene.p.clone();
        for i in f.soil_start..f.soil_start + f.soil_count {
            f.scene.velocity[i] = add(f.scene.velocity[i], scale(f.scene.gravity, STEP));
            f.scene.p[i] = add(f.scene.p[i], scale(f.scene.velocity[i], STEP));
        }
        for edge in &mut f.scene.edges {
            edge.lambda = 0.;
        }
        for tet in &mut f.scene.tets {
            tet.lambda = 0.;
        }
        let mut contact = Value::Null;
        for _ in 0..ITERATIONS {
            f.scene.constraints(STEP);
            f.set_tool(target);
            let requested = f.scene.p[f.tool_start..].to_vec();
            let mut projected=host.invoke("OGCConnectionSystem.Project",json!({"positions":f.scene.p,"previous":previous,"inverse_mass":f.scene.weights,"dt":STEP}))?;
            f.scene.p =
                serde_json::from_value(projected["positions"].take()).map_err(|e| e.to_string())?;
            if f.scene.p.len() != previous.len() {
                return Err("OGC changed shovel fixture node count".into());
            }
            let mut fraction = 1f32;
            for (k, wanted) in requested.iter().enumerate() {
                let i = f.tool_start + k;
                if length(sub(f.scene.p[i], *wanted)) > 1e-5 {
                    let delta = sub(*wanted, previous[i]);
                    let ratio =
                        dot(sub(f.scene.p[i], previous[i]), delta) / dot(delta, delta).max(1e-15);
                    fraction = fraction.min(ratio.clamp(0., 1.));
                }
            }
            contact = projected;
            if fraction < 0.9999 {
                return Ok((contact, fraction));
            }
        }
        for i in f.soil_start..f.soil_start + f.soil_count {
            f.scene.velocity[i] = scale(sub(f.scene.p[i], previous[i]), (-2. * STEP).exp() / STEP);
        }
        f.scene.time += STEP;
        f.valid()?;
        Ok((contact, 1.))
    }
    fn advance(&mut self, host: &mut Host) -> Result<(), String> {
        let backup = self
            .fixture
            .as_ref()
            .ok_or("shovel fixture is not configured")?
            .clone();
        let old_phase = self.phase;
        let requested = old_phase.map(|t| (t + STEP).min(STROKE_SECONDS));
        let result = self.candidate(host, pose(requested));
        let (mut contacts, fraction) = match result {
            Ok(result) => result,
            Err(error) => {
                self.restore(host, backup)?;
                return Err(error);
            }
        };
        let mut accepted_phase = requested;
        self.blocked = false;
        if fraction < 0.9999 {
            // A per-vertex conservative clip would distort the rigid tool. Roll
            // back and replay a shorter rigid stroke instead, keeping the solver
            // and the rendered tool on the same accepted transform.
            self.restore(host, backup.clone())?;
            accepted_phase = old_phase.map(|t| (t + STEP * fraction * 0.80).min(STROKE_SECONDS));
            let retry = self.candidate(host, pose(accepted_phase));
            match retry {
                Ok((result, fraction)) if fraction >= 0.9999 => contacts = result,
                Ok(_) => {
                    self.restore(host, backup.clone())?;
                    accepted_phase = old_phase;
                    match self.candidate(host, pose(old_phase)) {
                        Ok((result, fraction)) if fraction >= 0.9999 => contacts = result,
                        Ok(_) => {
                            self.restore(host, backup)?;
                            return Err("OGC could not retain the current rigid shovel pose; last valid state retained".into());
                        }
                        Err(error) => {
                            self.restore(host, backup)?;
                            return Err(error);
                        }
                    }
                }
                Err(error) => {
                    self.restore(host, backup)?;
                    return Err(error);
                }
            }
            self.blocked = true;
            self.blocked_steps += 1;
        }
        self.phase = accepted_phase;
        self.contacts = contacts;
        self.substeps += 1;
        let fixture = self.fixture.as_ref().unwrap();
        self.peak_centroid = self.peak_centroid.max(fixture.centroid()[1]);
        self.maximum_soil_speed = fixture.scene.velocity
            [fixture.soil_start..fixture.soil_start + fixture.soil_count]
            .iter()
            .map(|v| length(*v))
            .fold(0., f32::max);
        // Activity is also needed before and after the stroke: otherwise an
        // idle player would freeze a still-falling clod as soon as the tool
        // reached its final key. Sleeping never pins or zeros soil velocities.
        if self.phase.is_some_and(|t| t < STROKE_SECONDS) || self.maximum_soil_speed > 0.025 {
            self.quiet_seconds = 0.;
        } else {
            self.quiet_seconds += STEP;
        }
        Ok(())
    }
    pub(crate) fn report(&self) -> Value {
        let stroke_active = self.phase.is_some_and(|t| t < STROKE_SECONDS);
        let settling = self.active() && !stroke_active && self.quiet_seconds < 0.35;
        let tool=self.fixture.as_ref().map(|f| {
            let p=pose(self.phase);
            json!({"position":f.global(p.position),"right":f.axis([1.,0.,0.]),"up":f.axis(rotate_x([0.,1.,0.],[0.;3],p.pitch)),"forward":f.axis(rotate_x([0.,0.,1.],[0.;3],p.pitch)),"left_hand":f.global(p.point(f.grips[0])),"right_hand":f.global(p.point(f.grips[1]))})
        });
        json!({"configured":self.active(),"active":self.active()&&(stroke_active||settling)&&self.halt.is_none(),"stroke_active":stroke_active,"settling":settling&&self.halt.is_none(),"maximum_soil_speed":self.maximum_soil_speed,"quiet_seconds":self.quiet_seconds,"stage":stage(self.phase),"stroke_seconds":self.phase.unwrap_or(0.),"simulated_seconds":self.substeps as f64*STEP as f64,"frames":self.frames,"substeps":self.substeps,"fixed_step_seconds":STEP,"max_substeps_per_frame":MAX_STEPS,"iterations_per_substep":ITERATIONS,"discarded_seconds":self.discarded_seconds,"step_ms":self.last_ms,"tool":tool,"soil_centroid":self.fixture.as_ref().map(|f|f.global(f.centroid())),"peak_local_centroid_height":self.peak_centroid,"blocked":self.blocked,"blocked_substeps":self.blocked_steps,"contact":self.contacts,"halted":self.halt.is_some(),"failure":self.halt,"nodes":self.fixture.as_ref().map_or(0,|f|f.scene.p.len()),"soil_nodes":self.fixture.as_ref().map_or(0,|f|f.soil_count),"tetrahedra":self.fixture.as_ref().map_or(0,|f|f.scene.tets.len()),"soil_attached_to_tool":false,"material_model":"pre-separated cohesive elastic XPBD clod; no fracture or soil plasticity","terrain_excavation":false})
    }
}

pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let mut state = std::mem::take(&mut host.shovel_soil);
    let result = (|| {
        match hook {
            "Configure" => {
                state.configure(host, input)?;
            }
            "Step" => {
                if !valid_step(input) {
                    return Err("invalid shovel step".into());
                }
                if !state.active() {
                    return Err("shovel fixture is not configured".into());
                }
                if input["reset"] == true {
                    let configuration = state.configuration.clone();
                    state.configure(host, &configuration)?;
                }
                let action = input["action"] == true;
                if action
                    && !state.last_action
                    && state.halt.is_none()
                    && state.phase.is_none_or(|t| t >= STROKE_SECONDS)
                {
                    state.phase = Some(0.);
                }
                state.last_action = action;
                let start = std::time::Instant::now();
                if state.halt.is_none() {
                    let elapsed = input["dt"].as_f64().unwrap() as f32;
                    let pending = state.accumulator + elapsed;
                    state.accumulator = pending.min(STEP * MAX_STEPS as f32);
                    state.discarded_seconds += (pending - state.accumulator) as f64;
                    for _ in 0..MAX_STEPS {
                        if state.accumulator + 1e-7 < STEP {
                            break;
                        }
                        if let Err(error) = state.advance(host) {
                            state.halt = Some(error);
                            break;
                        }
                        state.accumulator = (state.accumulator - STEP).max(0.);
                    }
                }
                state.frames += 1;
                state.last_ms = start.elapsed().as_secs_f64() * 1000.;
            }
            "Mesh" => {
                return Ok(state
                    .fixture
                    .as_ref()
                    .ok_or("shovel fixture is not configured")?
                    .mesh());
            }
            "Report" => {}
            "Release" => {
                if state.active() {
                    host.invoke("OGCConnectionSystem.Release", Value::Null)?;
                }
                state = State::default();
                return Ok(Value::Null);
            }
            _ => return Err("unknown shovel soil hook".into()),
        }
        Ok(state.report())
    })();
    host.shovel_soil = state;
    result
}
