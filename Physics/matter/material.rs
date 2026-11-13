//! Constitutive parameters for mesoscopic matter, in SI units. These are
//! explicit artistic presets, not laboratory calibration or an atomic model.
use super::*;
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Dirt,
    Sand,
    FineSand,
    Snow,
    Water,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SolidKind {
    Wood,
    Stone,
    Ice,
}
impl SolidKind {
    pub fn density(self) -> f64 {
        match self {
            Self::Wood => 680.,
            Self::Stone => 2650.,
            Self::Ice => 917.,
        }
    }
}
#[derive(Clone, Copy)]
pub struct Law {
    pub density: f32,
    pub young: f32,
    pub tensile: f32,
    pub failure_strain: f32,
    pub plastic_yield: f32,
    pub creep: f32,
    pub friction: f32,
    pub cohesion: f32,
    pub color: V,
}
impl Kind {
    pub fn law(self) -> Law {
        match self {
            Self::Snow => Law {density:280.,young:18000.,tensile:1300.,failure_strain:0.008,plastic_yield:0.025,creep:0.,friction:0.55,cohesion:0.,color:[0.88,0.94,0.98]},
            Self::Dirt => Law {

                density: 1350.,
                young: 350_000.,
                tensile: 6500.,
                failure_strain: 0.08,
                plastic_yield: 0.025,
                creep: 1.2,
                friction: 0.78,
                cohesion: 1.,
                color: [0.39, 0.29, 0.18],
            },
            Self::Sand => Law {
                density: 1550.,
                young: 0.,
                tensile: 0.,
                failure_strain: 0.,
                plastic_yield: 0.,
                creep: 0.,
                friction: 0.74,
                cohesion: 0.,
                color: [0.75, 0.58, 0.32],
            },
            Self::Water => Law {density:1000.,young:0.,tensile:0.,failure_strain:0.,plastic_yield:0.,creep:0.,friction:0.015,cohesion:0.,color:[0.07,0.32,0.48]},
            Self::FineSand => Law {
                density: 1600.,
                young: 0.,
                tensile: 0.,
                failure_strain: 0.,
                plastic_yield: 0.,
                creep: 0.,
                friction: 0.65,
                cohesion: 0.,
                color: [0.78, 0.67, 0.45],
            },
        }
    }
}
#[derive(Clone)]
pub struct Bond {
    pub a: usize,
    pub b: usize,
    pub rest: f32,
    pub area: f32,
    pub damage: f32,
    pub lambda: f32,
    pub peak: f32,
    pub active: bool,
}
impl World {
    pub(super) fn reset_contact_lambda(&mut self) {
        // Refinement keeps inactive ancestors for later coarsening. Index only
        // live samples, so contact storage cannot grow quadratically with that
        // history. The live-sample budget bounds this packed multiplier table.
        self.contact_index.resize(self.particles.len(), usize::MAX);
        let mut n = 0;
        for (i, p) in self.particles.iter().enumerate() {
            self.contact_index[i] = if p.active { let slot = n; n += 1; slot } else { usize::MAX };
        }
        self.contact_lambda.resize(n * n.saturating_sub(1) / 2, 0.);
        self.contact_lambda.fill(0.);
    }
    pub fn refresh_shape_neighbours(&mut self) {
        self.shape_neighbours = vec![Vec::new(); self.particles.len()];
        for (id, b) in self.bonds.iter().enumerate().filter(|(_, b)| b.active) {
            self.shape_neighbours[b.a].push((b.b, id));
            self.shape_neighbours[b.b].push((b.a, id));
        }
    }
    fn solve_clump_shape(&self, q: &mut [V], dt: f32) {
        let mut correction = vec![[0.; 3]; q.len()];
        let mut weights = vec![0f32; q.len()];
        let mut ids = Vec::new();
        let mut rest = Vec::new();
        let mut current = Vec::new();
        for (i, adjacent) in self.shape_neighbours.iter().enumerate() {
            if adjacent.len() < 3 || !self.particles[i].active {
                continue;
            }
            ids.clear();
            ids.push(i);
            ids.extend(adjacent.iter().map(|v| v.0));
            if ids.iter().all(|&id|self.particles[id].anchored) { continue; }
            let center = self.particles[i].rest;
            rest.clear();
            rest.push(center);
            for &(other, bond) in adjacent {
                let delta = sub(self.particles[other].rest, center);
                // The shear-preserving cluster follows plastic bond shortening
                // instead of pulling compacted dirt back to its original size.
                rest.push(add(
                    center,
                    scale(delta, self.bonds[bond].rest / length(delta).max(1e-8)),
                ));
            }
            current.clear();
            current.extend(ids.iter().map(|&id| q[id]));
            let Some((origin, axes)) = super::super::rigid_fit::fit(&rest, &current) else {
                continue;
            };
            let m = self.particles[i].mass;
            let k = self.particles[i].kind.law().young * self.particles[i].radius;
            let stiffness = (k * dt * dt / (m + k * dt * dt)).min(0.45);
            for (&id, r) in ids.iter().zip(&rest) {
                if self.particles[id].anchored {
                    continue;
                }
                let goal = std::array::from_fn(|axis| {
                    (origin[axis] + (0..3).map(|j| axes[j][axis] * r[j] as f64).sum::<f64>()) as f32
                });
                // Preserve shear/volume of surviving local clumps. Removed
                // connections are absent from the next cluster construction.
                correction[id] = add(correction[id], scale(sub(goal, q[id]), stiffness));
                weights[id] += 1.;
            }
        }
        for i in 0..q.len() {
            if weights[i] > 0. {
                q[i] = add(q[i], scale(correction[i], 1. / weights[i]));
            }
        }
    }
    pub fn bond(&mut self, a: usize, b: usize, area: f32, ratio: f32, damage: f32) {
        let distance = length(sub(self.particles[a].p, self.particles[b].p));
        if distance < 1e-6 {
            return;
        }
        self.bonds.push(Bond {
            a,
            b,
            rest: distance * ratio,
            area,
            damage,
            lambda: 0.,
            peak: 0.,
            active: true,
        });
    }
    pub fn solve_material(&mut self, q: &mut [V], old: &[V], dt: f32) {
        let timer = std::time::Instant::now();
        let h2 = dt * dt;
        self.solve_clump_shape(q, dt);
        self.material_timings[0] += timer.elapsed().as_secs_f64() * 1000.;
        let timer = std::time::Instant::now();
        for bond in &mut self.bonds {
            if !bond.active {
                continue;
            }
            let a = &self.particles[bond.a];
            let b = &self.particles[bond.b];
            if !a.active || !b.active {
                continue;
            }
            let d = sub(q[bond.b], q[bond.a]);
            let len = length(d).max(1e-8);
            let axis = scale(d, 1. / len);
            let law = a.kind.law();
            let stiffness = law.young * bond.area / bond.rest.max(0.001);
            let alpha = 1. / (stiffness.max(1.) * h2);
            let denom = a.inv_mass() + b.inv_mass() + alpha;
            let dl = (-(len - bond.rest) - alpha * bond.lambda) / denom;
            bond.lambda += dl;
            q[bond.a] = sub(q[bond.a], scale(axis, dl * a.inv_mass()));
            q[bond.b] = add(q[bond.b], scale(axis, dl * b.inv_mass()));
            let tension = (-bond.lambda / h2).max(0.);
            let capacity = law.tensile * bond.area * (1. - 0.85 * bond.damage);
            let extension = ((len - bond.rest) / bond.rest).max(0.);
            // Use the latest coupled solve, not the worst transient from an
            // early iteration that the remaining iterations subsequently fix.
            bond.peak =
                (tension / capacity.max(0.01)).max(extension / law.failure_strain.max(0.001));
        }
        self.material_timings[1] += timer.elapsed().as_secs_f64() * 1000.;
        let timer = std::time::Instant::now();
        let groups = self.cached_neighbour_groups(q);
        self.material_timings[2] += timer.elapsed().as_secs_f64() * 1000.;
        let timer = std::time::Instant::now();
        // Neighbour contact includes Coulomb tangential friction. Sand has no
        // permanent bonds: load-bearing networks reform from current proximity.
        for (members, nearby) in groups.iter() {
            for &a in members {
                for &b in nearby {
                    if b <= a {
                        continue;
                    }
                    let pa = &self.particles[a];
                    let pb = &self.particles[b];
                    if pa.anchored && pb.anchored {
                        continue;
                    }
                    let d = sub(q[b], q[a]);
                    let rest = (pa.radius + pb.radius) * 0.90;
                    if dot(d, d) > (rest + 0.001).powi(2) {
                        continue;
                    }
                    let dist = length(d).max(1e-8);
                    let w = pa.inv_mass() + pb.inv_mass();
                    if w <= 0. {
                        continue;
                    }
                    let n = scale(d, 1. / dist);
                    let ai = self.contact_index[a];
                    let bi = self.contact_index[b];
                    let pair = bi * (bi - 1) / 2 + ai;
                    let previous_lambda = self.contact_lambda[pair];
                    let next_lambda = (previous_lambda + (rest - dist) / w).max(0.);
                    self.contact_lambda[pair] = next_lambda;
                    let correction = scale(n, next_lambda - previous_lambda);
                    q[a] = sub(q[a], scale(correction, pa.inv_mass()));
                    q[b] = add(q[b], scale(correction, pb.inv_mass()));
                    let delta = sub(sub(q[b], old[b]), sub(q[a], old[a]));
                    let tangent = sub(delta, scale(n, dot(delta, n)));
                    let friction = (pa.kind.law().friction * pb.kind.law().friction).sqrt();
                    let slip = length(tangent);
                    if slip > 1e-8 {
                        let normal_travel = next_lambda * w;
                        // A static cone holds the contact; sliding uses the
                        // lower dynamic coefficient. Keep the normal multiplier
                        // across iterations even after penetration is resolved.
                        let ratio = if slip <= friction * 1.25 * normal_travel {
                            1.
                        } else {
                            (friction * normal_travel / slip).min(1.)
                        };
                        let c = scale(tangent, ratio / w);
                        q[a] = add(q[a], scale(c, pa.inv_mass()));
                        q[b] = sub(q[b], scale(c, pb.inv_mass()));
                    }
                }
            }
        }
        self.material_timings[3] += timer.elapsed().as_secs_f64() * 1000.;
    }
    pub fn finish_material(&mut self, dt: f32) -> bool {
        let mut changed = false;
        for b in &mut self.bonds {
            if !b.active {
                continue;
            }
            let a = &self.particles[b.a];
            let c = &self.particles[b.b];
            let law = a.kind.law();
            let len = length(sub(a.p, c.p));
            let strain = (len - b.rest) / b.rest.max(1e-6);
            self.particles[b.a].activity = self.particles[b.a].activity.max(strain.abs());
            self.particles[b.b].activity = self.particles[b.b].activity.max(strain.abs());
            // Plastic rest-length evolution is permanent compaction, not
            // spring softness. Stone and wood never silently heal on unloading.
            if law.creep > 0. && strain < -law.plastic_yield && b.damage < 0.8 {
                let yield_len = len / (1. + strain.signum() * law.plastic_yield);
                b.rest += (yield_len - b.rest) * (law.creep * dt).min(0.2);
            }
            b.damage = (b.damage + dt * 12. * (b.peak - 1.).max(0.)).min(1.);
            if b.damage >= 1. {
                b.active = false;
                self.fractures += 1;
                changed = true;
            }
            b.peak = 0.;
            b.lambda = 0.;
        }
        changed
    }
}
