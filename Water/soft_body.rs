//! Conserved continuous water parcels. Cardinal cohesion/viscosity/separation
//! remain; rest/wake replaces block conversion and cell-lattice constraints.
use super::{hydrology::State as Contact, math::*};
#[derive(Clone, Debug)]
pub(crate) struct Settings {
    pub gravity: f64,
    pub air_drag: f64,
    pub downhill: f64,
    pub neighbour_radius: f64,
    pub rest_distance: f64,
    pub cohesion: f64,
    pub viscosity: f64,
    pub radius: f64,
    pub max_speed: f64,
    pub settle_speed: f64,
    pub settle_seconds: f64,
    pub capacity: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            gravity: 18.,
            air_drag: 0.12,
            downhill: 9.,
            neighbour_radius: 1.55,
            rest_distance: 0.94,
            cohesion: 2.4,
            viscosity: 3.2,
            radius: 0.48,
            max_speed: 28.,
            settle_speed: 0.16,
            settle_seconds: 1.25,
            capacity: 1000,
        }
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Parcel {
    pub id: u64,
    pub position: V,
    pub velocity: V,
    pub quiet: f64,
    pub resting: bool,
    pub covered: bool,
    pub impact: f64,
    pub source: Option<u64>,
    pub generator: Option<usize>,
    pub next_gate: usize,
    pub contact_revision: u64,
    pub slide_revision: u64,
}
#[derive(Default)]
pub(crate) struct State {
    pub settings: Settings,
    pub parcels: Vec<Parcel>,
    pub revision: u64,
    pub time: f64,
    pub next_id: u64,
    pub emitted: u64,
    pub removed: u64,
    pub coverage_needed: bool,
    pub coverage_points: Vec<V>,
    pub contact_cache: Option<Box<super::hydrology::State>>,
}
impl State {
    pub fn spawn(
        &mut self,
        position: V,
        velocity: V,
        source: Option<u64>,
        generator: Option<usize>,
    ) -> Result<u64, String> {
        if !finite(position) || !finite(velocity) || self.parcels.len() >= self.settings.capacity {
            return Err("invalid or full water parcel pool".into());
        }
        self.next_id += 1;
        let id = self.next_id;
        self.parcels.push(Parcel {
            id,
            position,
            velocity,
            quiet: 0.,
            resting: false,
            covered: false,
            impact: 0.,
            source,
            generator,
            next_gate: 0,
            contact_revision: 0,
            slide_revision: 0,
        });
        self.emitted += 1;
        self.revision += 1;
        Ok(id)
    }
    pub fn disturb(&mut self, point: V, radius: f64, impulse: V) -> usize {
        let mut count = 0;
        for p in &mut self.parcels {
            let d = len(sub(p.position, point));
            if d <= radius {
                p.resting = false;
                p.quiet = 0.;
                p.velocity = add(
                    p.velocity,
                    mul(impulse, (1. - d / radius.max(1e-6)).max(0.)),
                );
                count += 1;
            }
        }
        if count > 0 {
            self.revision += 1;
        }
        count
    }
    pub fn sample(&self, point: V, radius: f64) -> (bool, V, f64) {
        let mut sum = [0.; 3];
        let mut weight = 0.;
        let mut impact = 0f64;
        for p in &self.parcels {
            let d = len(sub(p.position, point));
            if d < radius {
                let w = 1. - d / radius;
                sum = add(sum, mul(p.velocity, w));
                weight += w;
                impact = impact.max(p.impact);
            }
        }
        (
            weight > 0.,
            if weight > 0. {
                mul(sum, 1. / weight)
            } else {
                [0.; 3]
            },
            impact,
        )
    }
    pub fn fill_sources(
        &mut self,
        sources: &[super::bodies::Source],
        contact: &Contact,
    ) -> Result<(), String> {
        let mut budget = 12usize;
        for source in sources {
            let count = self
                .parcels
                .iter()
                .filter(|p| p.source == Some(source.id))
                .count();
            for _ in 0..source.target.saturating_sub(count).min(budget) {
                if self.parcels.len() >= self.settings.capacity {
                    return Ok(());
                }
                let mut found = None;
                for attempt in 0..64u64 {
                    let mut seed = self
                        .next_id
                        .wrapping_add(attempt)
                        .wrapping_add(source.id.wrapping_mul(7919))
                        .wrapping_add(self.revision.wrapping_mul(104729));
                    let mut local = [0.; 3];
                    for i in 0..3 {
                        seed ^= seed >> 12;
                        seed ^= seed << 25;
                        seed ^= seed >> 27;
                        seed = seed.wrapping_mul(2685821657736338717);
                        let u = (seed >> 11) as f64 / 9007199254740992.;
                        let r = self.settings.radius;
                        local[i] = source.min[i]
                            + r
                            + u * (source.max[i] - source.min[i] - 2. * r).max(0.);
                    }
                    let p = source.frame.world(local);
                    if !contact.covered(p, self.settings.radius) {
                        self.coverage_needed = true;
                        if self.coverage_points.len() < 128 {
                            self.coverage_points.push(p);
                        }
                        continue;
                    }
                    if contact.sweep(p, p, self.settings.radius).is_some() {
                        continue;
                    }
                    if self
                        .parcels
                        .iter()
                        .any(|q| len(sub(q.position, p)) < self.settings.rest_distance * 0.95)
                    {
                        continue;
                    }
                    found = Some(p);
                    break;
                }
                if let Some(p) = found {
                    self.spawn(
                        p,
                        source.frame.vector(source.velocity),
                        Some(source.id),
                        None,
                    )?;
                    budget = budget.saturating_sub(1);
                } else {
                    break;
                }
            }
            if budget == 0 {
                break;
            }
        }
        Ok(())
    }
    pub fn step(&mut self, dt: f64, contact: &Contact) -> Result<(), String> {
        if !dt.is_finite() || !(0.0..=0.1).contains(&dt) {
            return Err("invalid water timestep".into());
        }
        // Bounded fixed substeps consume up to the original 3/60s frame budget.
        // Excess time is reported by the caller, never replayed as an unbounded job.
        let total = dt.min(3. / 60.);
        let count = (total / (1. / 60.)).ceil().max(1.) as usize;
        for _ in 0..count {
            self.substep(total / count as f64, contact);
        }
        self.time += total;
        self.revision += 1;
        Ok(())
    }
    fn substep(&mut self, dt: f64, contact: &Contact) {
        let cfg = &self.settings;
        let count = self.parcels.len();
        for p in &mut self.parcels {
            if p.contact_revision != contact.revision || p.slide_revision != contact.slide_revision
            {
                p.resting = false;
                p.quiet = 0.;
            }
            p.contact_revision = contact.revision;
            p.slide_revision = contact.slide_revision;
            p.covered = contact.covered(p.position, cfg.radius);
            if !p.covered {
                self.coverage_needed = true;
                if self.coverage_points.len() < 128 {
                    self.coverage_points.push(p.position);
                }
                continue;
            }
            p.impact = (p.impact - dt * 0.85).max(0.);
            if !p.resting {
                p.velocity = mul(p.velocity, (-cfg.air_drag * dt).exp());
            }
        }
        // Symmetric pairs conserve internal linear momentum. Resting parcels
        // wake from a moving neighbour; their material never becomes a block.
        for i in 0..count {
            for j in i + 1..count {
                let (left, right) = self.parcels.split_at_mut(j);
                let a = &mut left[i];
                let b = &mut right[0];
                if !a.covered || !b.covered {
                    continue;
                }
                let delta = sub(b.position, a.position);
                let distance = len(delta);
                if distance >= cfg.neighbour_radius || distance <= 1e-9 {
                    continue;
                }
                if a.resting && b.resting {
                    continue;
                }
                let weight = 1. - distance / cfg.neighbour_radius;
                if (a.resting || b.resting) && len(sub(a.velocity, b.velocity)) > cfg.settle_speed {
                    a.resting = false;
                    b.resting = false;
                    a.quiet = 0.;
                    b.quiet = 0.;
                }
                let pull = mul(
                    delta,
                    (distance - cfg.rest_distance).max(0.) * cfg.cohesion * weight * dt / distance,
                );
                let viscosity = mul(
                    sub(b.velocity, a.velocity),
                    cfg.viscosity * weight * dt * 0.5,
                );
                let impulse = add(pull, viscosity);
                a.velocity = add(a.velocity, impulse);
                b.velocity = sub(b.velocity, impulse);
            }
        }
        let mut supported = vec![false; count];
        for (i, p) in self.parcels.iter_mut().enumerate() {
            if !p.covered || p.resting {
                supported[i] = p.resting;
                continue;
            }
            p.velocity = add(p.velocity, mul(unit(p.position), -cfg.gravity * dt));
            p.velocity = mul(
                p.velocity,
                (cfg.max_speed / len(p.velocity).max(1e-12)).min(1.),
            );
            let mut start = p.position;
            let mut remaining = mul(p.velocity, dt);
            for _ in 0..4 {
                let end = add(start, remaining);
                if !contact.covered(end, cfg.radius) {
                    self.coverage_needed = true;
                    if self.coverage_points.len() < 128 {
                        self.coverage_points.push(end);
                    }
                    p.velocity = [0.; 3];
                    break;
                }
                if let Some(hit) = contact.sweep(start, end, cfg.radius) {
                    let inward = dot(p.velocity, hit.normal).min(0.);
                    p.impact = p.impact.max((-inward / 8.).min(1.));
                    p.velocity = sub(p.velocity, mul(hit.normal, inward));
                    let up = unit(p.position);
                    supported[i] |= dot(hit.normal, up) > 0.15;
                    // Retained continuous downhill acceleration on supported slopes.
                    let gravity = mul(up, -cfg.downhill);
                    let tangent = sub(gravity, mul(hit.normal, dot(gravity, hit.normal)));
                    p.velocity = add(mul(p.velocity, 0.92), mul(tangent, dt));
                    start = hit.point;
                    remaining = mul(p.velocity, dt * (1. - hit.fraction));
                } else {
                    start = end;
                    break;
                }
            }
            p.position = start;
        }
        // Legacy three symmetric separation passes, each projected onto the
        // actual triangle boundaries so a correction cannot leak through a trough.
        for _ in 0..3 {
            for i in 0..count {
                for j in i + 1..count {
                    let (left, right) = self.parcels.split_at_mut(j);
                    let a = &mut left[i];
                    let b = &mut right[0];
                    if !a.covered || !b.covered || (a.resting && b.resting) {
                        continue;
                    }
                    let delta = sub(b.position, a.position);
                    let distance = len(delta);
                    if distance >= cfg.rest_distance || distance <= 1e-9 {
                        continue;
                    }
                    let correction = mul(
                        delta,
                        (cfg.rest_distance - distance) * 0.5 * 0.78 / distance,
                    );
                    for (p, step) in [(a, mul(correction, -1.)), (b, correction)] {
                        let end = add(p.position, step);
                        if !contact.covered(end, cfg.radius) {
                            continue;
                        }
                        let next = contact
                            .sweep(p.position, end, cfg.radius)
                            .map_or(end, |h| h.point);
                        if len(sub(next, p.position)) > 1e-6 {
                            p.resting = false;
                            p.quiet = 0.;
                        }
                        p.position = next;
                    }
                }
            }
        }
        for (i, p) in self.parcels.iter_mut().enumerate() {
            if !p.covered || p.resting {
                continue;
            }
            // A support probe also recognizes quiet contact when motion is tangent.
            let down = mul(unit(p.position), -0.01);
            supported[i] |= contact
                .sweep(p.position, add(p.position, down), cfg.radius)
                .is_some();
            if supported[i] && len(p.velocity) < cfg.settle_speed {
                p.quiet += dt;
            } else {
                p.quiet = 0.;
            }
            if p.quiet >= cfg.settle_seconds {
                p.resting = true;
                p.velocity = [0.; 3];
            }
        }
    }
}
