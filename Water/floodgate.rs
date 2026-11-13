//! Cardinal's four source planes and final recycling plane on a continuous
//! course. These are emitters/exits, not an invented physical sluice door.
use super::{hydrology::State as Contact, math::*, slides::Pose, soft_body::State as Water};
#[derive(Clone, Debug)]
pub(crate) struct Settings {
    pub enabled: bool,
    pub target: usize,
    pub spawn_per_step: usize,
    pub initial_speed: f64,
    pub course_start: Option<u64>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            target: 300,
            spawn_per_step: 12,
            initial_speed: 0.75,
            course_start: None,
        }
    }
}
#[derive(Default)]
pub(crate) struct State {
    pub settings: Settings,
    pub course: Vec<Pose>,
    pub spawned: u64,
    pub recycled: u64,
    pub returned: u64,
    pub next_slot: usize,
    pub revision: u64,
}
const GENERATORS: [f64; 4] = [0., 0.018, 0.036, 0.054];
impl State {
    pub fn active(&self) -> bool {
        self.settings.enabled && self.course.len() >= 2
    }
    pub fn pose(&self, t: f64) -> Option<Pose> {
        if self.course.len() < 2 {
            return None;
        }
        let lengths: Vec<f64> = self
            .course
            .windows(2)
            .map(|p| len(sub(p[1].position, p[0].position)))
            .collect();
        let total: f64 = lengths.iter().sum();
        if total <= 1e-9 {
            return None;
        }
        let mut distance = t.clamp(0., 1.) * total;
        for (i, &length) in lengths.iter().enumerate() {
            if distance <= length || i == lengths.len() - 1 {
                let a = self.course[i];
                let b = self.course[i + 1];
                let amount = (distance / length.max(1e-9)).clamp(0., 1.);
                let forward = safe_unit(mix(a.forward, b.forward, amount), a.forward);
                let up = safe_unit(mix(a.up, b.up, amount), a.up);
                let right = safe_unit(cross(up, forward), a.right);
                return Some(Pose {
                    position: mix(a.position, b.position, amount),
                    right,
                    up: unit(cross(forward, right)),
                    forward,
                    radius: a.radius * (1. - amount) + b.radius * amount,
                });
            }
            distance -= length;
        }
        None
    }
    fn emitter(&self, slot: usize, radius: f64) -> Option<(V, V, usize)> {
        let generator = slot % 4;
        let local = slot / 4;
        let across = (local % 5) as f64 - 2.;
        let vertical = ((local / 5) % 2) as f64;
        let depth = (local / 10) % 8;
        let pose = self.pose((GENERATORS[generator] + depth as f64 * 0.0055).min(0.97))?;
        // Keep the original 5x2x8 slot cadence, bounded by the real trough.
        let clearance = (pose.radius - radius - 0.005).max(0.);
        let x = (across * 0.88).clamp(-clearance, clearance);
        let y = (-((pose.radius - 2.).max(0.5)) + vertical * 0.88).clamp(-clearance, 0.);
        Some((
            add(pose.position, add(mul(pose.right, x), mul(pose.up, y))),
            mul(pose.forward, self.settings.initial_speed),
            generator,
        ))
    }
    fn safe_emitter(
        &mut self,
        water: &mut Water,
        contact: &Contact,
        ignore: Option<usize>,
    ) -> Option<(V, V, usize)> {
        for _ in 0..80 {
            let slot = self.next_slot;
            self.next_slot = self.next_slot.wrapping_add(1);
            let candidate = self.emitter(slot, water.settings.radius)?;
            if !contact.covered(candidate.0, water.settings.radius) {
                water.coverage_needed = true;
                if water.coverage_points.len() < 128 {
                    water.coverage_points.push(candidate.0);
                }
                continue;
            }
            if contact
                .sweep(candidate.0, candidate.0, water.settings.radius)
                .is_some()
            {
                continue;
            }
            if water.parcels.iter().enumerate().any(|(i, p)| {
                Some(i) != ignore
                    && len(sub(p.position, candidate.0)) < water.settings.rest_distance * 0.9
            }) {
                continue;
            }
            return Some(candidate);
        }
        None
    }
    pub fn return_owned(&mut self,water:&mut Water){
        let before=water.parcels.len();water.parcels.retain(|p|p.generator.is_none());
        let removed=before-water.parcels.len();water.removed+=removed as u64;self.returned+=removed as u64;
    }
    pub fn step(&mut self, water: &mut Water, contact: &Contact) -> Result<(), String> {
        if !self.active() {self.return_owned(water);return Ok(());}
        let exit = self
            .pose(0.995)
            .ok_or("continuous floodgate exit missing")?;
        let source = self.pose(0.).unwrap();
        for i in 0..water.parcels.len() {
            let p = &water.parcels[i];
            if p.generator.is_none() {
                continue;
            }
            let d = sub(p.position, exit.position);
            let half = (exit.radius - 0.5).max(0.5);
            let crossed = dot(d, exit.forward) >= 0.05
                && dot(d, exit.right).abs() <= half + 1.
                && dot(d, exit.up).abs() <= half + 1.;
            let escaped = dot(d, exit.up) < -16.
                || dot(sub(p.position, source.position), source.forward) < -8.
                || !finite(p.position);
            if crossed || escaped {
                if let Some((position, velocity, generator)) =
                    self.safe_emitter(water, contact, Some(i))
                {
                    let p = &mut water.parcels[i];
                    p.position = position;
                    p.velocity = velocity;
                    p.generator = Some(generator);
                    p.next_gate = 0;
                    p.quiet = 0.;
                    p.resting = false;
                    p.impact = 0.;
                    self.recycled += 1;
                }
            }
        }
        let owned = water
            .parcels
            .iter()
            .filter(|p| p.generator.is_some())
            .count();
        let needed = self
            .settings
            .target
            .saturating_sub(owned)
            .min(self.settings.spawn_per_step)
            .min(water.settings.capacity.saturating_sub(water.parcels.len()));
        for _ in 0..needed {
            if let Some((position, velocity, generator)) = self.safe_emitter(water, contact, None) {
                water.spawn(position, velocity, None, Some(generator))?;
                self.spawned += 1;
            } else {
                break;
            }
        }
        self.revision += 1;
        Ok(())
    }
}
