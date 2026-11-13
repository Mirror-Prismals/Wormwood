//! Seed reserves, moisture-limited growth and elastic roots/shoots. Biological
//! time is deliberately accelerated; it is not a calibrated botanical model.
use super::*;
const LEAVES: usize = 3;
const NODES: usize = 1 + LEAVES * 3;
#[derive(Clone)]
struct Root {
    sample: usize,
    offset: V,
    origin_offset: V,
    node: V,
    previous: V,
    velocity: V,
    length: f32,
    broken: bool,
    load: f32,
}
#[derive(Clone)]
struct Plant {
    seed: V,
    age_hours: f32,
    reserve: f32,
    biomass: f32,
    height: f32,
    roots: Vec<Root>,
    shoot: [V; NODES],
    previous: [V; NODES],
    velocity: [V; NODES],
    azimuth: f32,
}
#[derive(Default, Clone)]
pub struct Garden {
    plants: Vec<Plant>,
}
impl Garden {
    pub fn seed(particles: &[Particle]) -> Self {
        let mut top: Vec<_> = particles
            .iter()
            .enumerate()
            .filter(|(_, p)| p.p[1] > 0.16 && p.p[0].abs() < 0.26 && p.p[2].abs() < 0.24)
            .collect();
        top.sort_by(|a, b| b.1.p[1].total_cmp(&a.1.p[1]));
        let mut plants: Vec<Plant> = Vec::new();
        for (i, p) in top {
            let seed = add(p.p, [0., p.radius * 0.9, 0.]);
            if plants.iter().any(|s| length(sub(s.seed, seed)) < 0.052) {
                continue;
            }
            let mut nearby: Vec<_> = particles
                .iter()
                .enumerate()
                .filter(|(_, q)| q.p[1] <= p.p[1] && length(sub(q.p, p.p)) < 0.14)
                .map(|(j, q)| (j, length(sub(q.p, p.p))))
                .collect();
            nearby.sort_by(|a, b| a.1.total_cmp(&b.1));
            let roots = nearby
                .into_iter()
                .take(4)
                .map(|(sample, _)| {
                    let target = add(
                        particles[sample].p,
                        [0., particles[sample].radius * 0.3, 0.],
                    );
                    Root {
                        sample,
                        offset: sub(target, particles[sample].p),
                        origin_offset: sub(seed, particles[sample].p),
                        node: seed,
                        previous: seed,
                        velocity: [0.; 3],
                        length: length(sub(target, seed)),
                        broken: false,
                        load: 0.,
                    }
                })
                .collect();
            let shoot = std::array::from_fn(|k| {
                add(
                    seed,
                    [
                        0.,
                        if k == 0 {
                            0.
                        } else {
                            0.003 * ((k - 1) % 3 + 1) as f32
                        },
                        0.,
                    ],
                )
            });
            plants.push(Plant {
                seed,
                age_hours: 0.,
                reserve: 0.006,
                biomass: 0.,
                height: 0.009,
                roots,
                shoot,
                previous: shoot,
                velocity: [[0.; 3]; NODES],
                azimuth: noise(i as i32 * 137) * std::f32::consts::PI,
            });
            let _ = i;
            if plants.len() == 32 {
                break;
            }
        }
        Self { plants }
    }
    pub fn advance(&mut self, particles: &mut [Particle], dt: f32, enabled: bool) {
        if !enabled {
            return;
        }
        for plant in &mut self.plants {
            let wet = plant
                .roots
                .iter()
                .filter(|r| !r.broken)
                .map(|r| particles[r.sample].moisture)
                .sum::<f32>()
                / plant.roots.len().max(1) as f32;
            plant.age_hours += dt * 12. * wet;
            if plant.age_hours > 5. && plant.reserve > 0. {
                let growth = (dt * 0.0015 * wet).min(plant.reserve);
                plant.reserve -= growth;
                plant.biomass += growth;
                plant.height = 0.009 + 0.20 * (plant.biomass / 0.006).sqrt();
                for root in &plant.roots {
                    if !root.broken {
                        particles[root.sample].moisture =
                            (particles[root.sample].moisture - growth * 0.6).max(0.);
                    }
                }
            }
            plant.previous = plant.shoot;
            for k in 0..NODES {
                plant.velocity[k][1] -= 1.5 * dt;
            }
            for r in &mut plant.roots {
                r.previous = r.node;
                r.node = add(r.node, scale(r.velocity, dt));
                r.load = 0.;
            }
        }
    }
    pub fn solve(&mut self, q: &mut [V], particles: &[Particle], dt: f32) {
        for plant in &mut self.plants {
            let mut root_mean = [0.; 3];
            let mut count = 0.;
            for r in &mut plant.roots {
                if r.broken || !particles[r.sample].active {
                    continue;
                }
                let grown = ((plant.age_hours - 5.) / 12.).clamp(0., 1.);
                let target = add(
                    q[r.sample],
                    add(scale(r.origin_offset, 1. - grown), scale(r.offset, grown)),
                );
                let delta = sub(r.node, target);
                let alpha = 2e-6 / (dt * dt);
                let root_weight = 1. / 0.003;
                let soil_weight = particles[r.sample].inv_mass();
                let correction = scale(delta, 1. / (root_weight + soil_weight + alpha));
                let force = length(correction) / (dt * dt);
                r.load = r.load.max(force);
                if force > 8. {
                    r.broken = true;
                    continue;
                }
                r.node = sub(r.node, scale(correction, root_weight));
                q[r.sample] = add(q[r.sample], scale(correction, soil_weight));
                let d = sub(plant.shoot[0], r.node);
                let l = length(d).max(1e-8);
                let rest = r.length * grown;
                if l > rest {
                    let c = scale(d, (l - rest) / (l * 2.));
                    plant.shoot[0] = sub(plant.shoot[0], c);
                    r.node = add(r.node, c);
                }
                root_mean = add(root_mean, r.node);
                count += 1.;
            }
            if count > 0. {
                plant.seed = scale(root_mean, 1. / count);
            }
            for leaf in 0..LEAVES {
                for segment in 0..3 {
                    let a = if segment == 0 { 0 } else { leaf * 3 + segment };
                    let b = leaf * 3 + segment + 1;
                    let delta = sub(plant.shoot[b], plant.shoot[a]);
                    let l = length(delta).max(1e-7);
                    let c = scale(
                        delta,
                        (l - plant.height * (1. - leaf as f32 * 0.08) / 3.) / l * 0.45,
                    );
                    plant.shoot[a] = add(plant.shoot[a], c);
                    plant.shoot[b] = sub(plant.shoot[b], c);
                }
            }
            // Phototropic preferred curvature is compliant; contact may bend
            // the shoot, transmitting its load through roots into actual soil.
            for k in 1..NODES {
                let leaf = (k - 1) / 3;
                let t = ((k - 1) % 3 + 1) as f32 / 3.;
                let angle = plant.azimuth + leaf as f32 * 2.39996;
                let desired = add(
                    plant.shoot[0],
                    [
                        angle.cos() * plant.height * 0.38 * t * t,
                        plant.height * (1. - leaf as f32 * 0.08) * t * (1. - 0.16 * t),
                        angle.sin() * plant.height * 0.38 * t * t,
                    ],
                );
                plant.shoot[k] = add(plant.shoot[k], scale(sub(desired, plant.shoot[k]), 0.08));
            }
        }
    }
    pub fn shoot_positions(&self) -> Vec<V> {
        self.plants.iter().flat_map(|p| p.shoot).collect()
    }
    pub fn predict(&mut self, dt: f32) {
        for p in &mut self.plants {
            for k in 0..NODES {
                p.shoot[k] = add(p.shoot[k], scale(p.velocity[k], dt));
            }
        }
    }
    pub fn restore_shoots(&mut self, positions: &[V]) {
        for (i, p) in self.plants.iter_mut().enumerate() {
            p.shoot
                .copy_from_slice(&positions[i * NODES..(i + 1) * NODES]);
        }
    }
    pub fn finish(&mut self, dt: f32) {
        for p in &mut self.plants {
            for k in 0..NODES {
                p.velocity[k] = scale(sub(p.shoot[k], p.previous[k]), (-5. * dt).exp() / dt);
            }
            for r in &mut p.roots {
                r.velocity = scale(sub(r.node, r.previous), (-8. * dt).exp() / dt);
            }
        }
    }
    pub fn contact_positions(&self) -> Vec<V> {
        self.plants
            .iter()
            .flat_map(|p| {
                p.shoot
                    .into_iter()
                    .flat_map(|v| [add(v, [-0.003, 0., 0.]), add(v, [0.003, 0., 0.])])
            })
            .collect()
    }
    pub fn contact_faces(&self, start: u32) -> Vec<[u32; 3]> {
        (0..self.plants.len())
            .flat_map(|i| {
                (0..LEAVES * 3).flat_map(move |k| {
                    let leaf = k / 3;
                    let segment = k % 3;
                    let a = start
                        + (i * NODES * 2
                            + if segment == 0 {
                                0
                            } else {
                                (leaf * 3 + segment) * 2
                            }) as u32;
                    let b = start + (i * NODES * 2 + (leaf * 3 + segment + 1) * 2) as u32;
                    [[a, a + 1, b], [a + 1, b + 1, b]]
                })
            })
            .collect()
    }
    pub fn contact(&mut self, goal: &[V], response: &[[f32; 13]], dt: f32) {
        for (i, p) in self.plants.iter_mut().enumerate() {
            for k in 0..NODES {
                let rs = &response[i * NODES * 2 + k * 2..i * NODES * 2 + k * 2 + 2];
                p.shoot[k] = add(
                    p.shoot[k],
                    translation(p.shoot[k], goal[i * NODES + k], 0.003, rs, dt),
                );
            }
        }
    }
    pub fn remap_split(&mut self, old: usize, children: [usize; 8], particles: &[Particle]) {
        for p in &mut self.plants {
            for r in &mut p.roots {
                if r.sample == old {
                    let old_tip = add(particles[old].p, r.offset);
                    let old_origin = add(particles[old].p, r.origin_offset);
                    r.sample = *children
                        .iter()
                        .min_by(|a, b| {
                            length(sub(particles[**a].p, r.node))
                                .total_cmp(&length(sub(particles[**b].p, r.node)))
                        })
                        .unwrap();
                    r.offset = sub(old_tip, particles[r.sample].p);
                    r.origin_offset = sub(old_origin, particles[r.sample].p);
                }
            }
        }
    }
    pub fn remap_merge(&mut self, parent: usize, children: [usize; 8], particles: &[Particle]) {
        for p in &mut self.plants {
            for r in &mut p.roots {
                if children.contains(&r.sample) {
                    let d = sub(particles[r.sample].p, particles[parent].p);
                    r.offset = add(r.offset, d);
                    r.origin_offset = add(r.origin_offset, d);
                    r.sample = parent;
                }
            }
        }
    }
    pub fn report(&self) -> Value {
        json!({"seeds":self.plants.len(),"leaves_per_plant":LEAVES,"germinated":self.plants.iter().filter(|p|p.biomass>0.).count(),"root_breaks":self.plants.iter().flat_map(|p|&p.roots).filter(|r|r.broken).count(),"plant_mass_kg":self.plants.iter().map(|p|p.reserve+p.biomass).sum::<f32>(),"biological_hours_per_second":12.})
    }
    pub fn render(&self, _particles: &[Particle], out: &mut Vec<[f32; 6]>) {
        for p in &self.plants {
            if p.biomass < 0.0001 {
                skin::rod(
                    out,
                    p.shoot[0],
                    add(p.shoot[0], [0.012, 0.003, 0.]),
                    0.003,
                    [0.57, 0.39, 0.15],
                );
                continue;
            }
            for leaf in 0..LEAVES {
                for k in 0..3 {
                    let a = p.shoot[if k == 0 { 0 } else { leaf * 3 + k }];
                    let b = p.shoot[leaf * 3 + k + 1];
                    let angle = p.azimuth + leaf as f32 * 2.39996;
                    let across = [-angle.sin(), 0., angle.cos()];
                    let width = 0.0055 * (1. - k as f32 / 3.);
                    let end_width = if k == 2 { 0.0002 } else { width * 0.75 };
                    let [al, ar] = [-1., 1.].map(|s| add(a, scale(across, width * s)));
                    let [bl, br] = [-1., 1.].map(|s| add(b, scale(across, end_width * s)));
                    skin::triangle(out, [al, ar, bl], [0.19, 0.43, 0.065]);
                    skin::triangle(out, [ar, br, bl], [0.29, 0.53, 0.085]);
                }
            }
            // Roots become visible naturally when their soil has separated.
            for r in &p.roots {
                skin::rod(
                    out,
                    p.shoot[0],
                    r.node,
                    0.0012,
                    if r.broken {
                        [0.38, 0.28, 0.15]
                    } else {
                        [0.67, 0.58, 0.34]
                    },
                );
            }
        }
    }
}
