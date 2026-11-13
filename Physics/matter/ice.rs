//! Ice-specific thermal exchange and hydrostatic forces on accepted convex cells.
use super::super::thermal::{ICE_CP, LATENT, Thermal};
use super::*;
impl Solid {
    pub(in crate::physics::matter) fn exchange_heat(
        &mut self,
        t: &mut Thermal,
        dt: f32,
    ) -> Result<Vec<Particle>, String> {
        let mut drops = Vec::new();
        for i in 0..self.cells.len() {
            if self.cells[i].melted {
                continue;
            }
            let c = &self.cells[i];
            let mut air_area = 0.;
            let mut wet_area = 0.;
            for (j, f) in c.poly.faces.iter().enumerate() {
                if !self.exposed(i, j) {
                    continue;
                }
                for k in 1..f.nodes.len() - 1 {
                    let tri =
                        [f.nodes[0], f.nodes[k], f.nodes[k + 1]].map(|v| self.p[c.range.start + v]);
                    let area = length(cross(sub(tri[1], tri[0]), sub(tri[2], tri[0]))) * 0.5;
                    let wet = if t.bath {
                        tri.iter()
                            .map(|p| ((t.level() - p[1]) / 0.02 + 0.5).clamp(0., 1.))
                            .sum::<f32>()
                            / 3.
                    } else {
                        0.
                    };
                    wet_area += area * wet;
                    air_area += area * (1. - wet);
                }
            }
            let c = &mut self.cells[i];
            let temperature = super::super::thermal::temperature(c.enthalpy);
            let mass = c.body.mass as f32;
            let equilibrium = if t.air.temperature_c > 0. {
                LATENT + 4186. * t.air.temperature_c
            } else {
                ICE_CP * t.air.temperature_c
            };
            let qa =
                ((8. + 3. * t.air.wind_m_s) * air_area * (t.air.temperature_c - temperature) * dt)
                    .clamp(
                        mass * (equilibrium - c.enthalpy).min(0.),
                        mass * (equilibrium - c.enthalpy).max(0.),
                    );

            let qw = (240. * wet_area * (t.water_c() - temperature) * dt)
                .clamp(-c.body.mass as f32 * ICE_CP * 20., t.water_energy.max(0.));
            let mass = c.body.mass as f32;
            // Enthalpy stays at the melting plateau until the latent heat is paid.
            c.enthalpy += (qa + qw) / mass;
            t.air_heat += qa;
            t.water_energy -= qw;
            if c.enthalpy < LATENT {
                continue;
            }
            c.melted = true;
            self.dirty = true;
            t.melted += mass;
            let center = centroid(&self.p[c.range.clone()]);
            if t.bath && center[1] < t.level() + 0.15 {
                t.water_mass += mass;
                t.water_energy += mass * (c.enthalpy - LATENT).max(0.);
            } else {
                // One coarse liquid parcel carries this cell's complete mass
                // and velocity. Its subsequent shape follows the liquid law.
                let velocity = scale(
                    self.velocity[c.range.clone()]
                        .iter()
                        .copied()
                        .fold([0.; 3], add),
                    1. / c.range.len() as f32,
                );
                let radius = (mass / 1000.).cbrt() * 0.55;
                {
                    let p = center;
                    t.parcel_enthalpy.push(c.enthalpy);
                    drops.push(Particle {
                        p,
                        rest: p,
                        v: velocity,
                        spin: [0.; 3],
                        radius,
                        mass,
                        kind: Kind::Water,
                        anchored: false,
                        active: true,
                        level: 0,
                        quiet: 0.,
                        moisture: 1.,
                        region: 0,
                        activity: 0.,
                    });
                }
            }
            for k in c.range.clone() {
                self.weights[k] = 0.;
                self.velocity[k] = [0.; 3];
            }
            for j in &mut self.joints {
                if j.cells.contains(&i) {
                    j.broken = true;
                    j.contact_ready = false;
                    j.phase_removed = true;
                }
            }
        }
        Ok(drops)
    }
    pub(in crate::physics::matter) fn apply_buoyancy(
        &self,
        step: &mut coupling::Step,
        t: &mut Thermal,
        dt: f32,
    ) {
        if !t.bath {
            return;
        }
        let level = t.level();
        let mut displaced = 0.;
        for c in self.cells.iter().filter(|c| !c.melted && !c.anchored) {
            let mut poly = c.poly.clone();
            for (v, &p) in poly.vertices.iter_mut().zip(&self.p[c.range.clone()]) {
                v.position = p.map(f64::from);
                v.material = v.position;
            }
            let lo = self.p[c.range.clone()]
                .iter()
                .map(|p| p[1])
                .fold(f32::INFINITY, f32::min);
            let hi = self.p[c.range.clone()]
                .iter()
                .map(|p| p[1])
                .fold(f32::NEG_INFINITY, f32::max);
            let wet = if lo >= level {
                None
            } else if hi <= level {
                Some(poly)
            } else {
                poly.clip_negative(
                    crate::physics::cut_geometry::Plane::through(
                        [0., level as f64, 0.],
                        [0., 1., 0.],
                    ),
                    9_000_001,
                )
            };
            let Some(wet) = wet else { continue };
            let measure = wet.measure(true);
            displaced += measure.volume as f32;
            let force = (1000. * 9.81 * measure.volume) as f32;
            let center = centroid(&self.p[c.range.clone()]);
            let torque = cross(
                sub(measure.center.map(|v| v as f32), center),
                [0., force, 0.],
            );
            let (_, axes) =
                crate::physics::rigid_fit::fit(&c.model, &self.p[c.range.clone()]).unwrap();
            let local: [f64; 3] =
                std::array::from_fn(|j| dot(axes[j].map(|v| v as f32), torque) as f64);
            let inertia = c.body.inertia;
            let cofactor = [
                crate::physics::cut_geometry::cross(inertia[1], inertia[2]),
                crate::physics::cut_geometry::cross(inertia[2], inertia[0]),
                crate::physics::cut_geometry::cross(inertia[0], inertia[1]),
            ];
            let det = crate::physics::cut_geometry::dot(inertia[0], cofactor[0]).max(1e-12);
            let local_alpha: [f64; 3] = std::array::from_fn(|j| {
                crate::physics::cut_geometry::dot(cofactor[j], local) / det
            });
            let alpha: V = std::array::from_fn(|k| {
                (0..3).map(|j| axes[j][k] * local_alpha[j]).sum::<f64>() as f32
            });

            let fraction = (measure.volume / (c.body.mass / 917.)).clamp(0., 1.) as f32;
            for k in c.range.clone() {
                let drag = scale(self.velocity[k], -2.5 * fraction * dt * dt);
                step.predicted[k] = add(
                    step.predicted[k],
                    add(
                        [0., force / c.body.mass as f32 * dt * dt, 0.],
                        add(drag, scale(cross(alpha, sub(self.p[k], center)), dt * dt)),
                    ),
                );
            }
        }
        t.displaced = displaced;
    }
}
