//! Enthalpy accounting in SI units. Mechanics keep real time; workshop heating
//! explicitly accelerates thermal time so melting is visible in a short visit.
use super::*;
pub(super) const LATENT: f32 = 334_000.;
pub(super) const ICE_CP: f32 = 2106.;
pub(super) const WATER_CP: f32 = 4186.;
#[derive(Clone)]
pub(super) struct Thermal {
    pub air: climate::Air,
    pub bath: bool,
    pub water_mass: f32,
    pub water_energy: f32,
    pub displaced: f32,
    pub scale: f32,
    pub elapsed: f32,
    pub melted: f32,
    initial_mass: f32,
    initial_bath_mass: f32,
    pub(super) parcel_enthalpy: Vec<f32>,
    pub air_heat: f32,
}
impl Thermal {
    pub fn snow(w: &World) -> Self {
        Self {
            air: climate::Air {
                temperature_c: -6.,
                wind_m_s: 1.,
                cloud: 0.4,
            },
            bath: false,
            water_mass: 0.,
            water_energy: 0.,
            displaced: 0.,
            scale: 180.,
            elapsed: 0.,
            melted: 0.,
            initial_mass: w.initial_mass,
            initial_bath_mass: 0.,
            parcel_enthalpy: vec![-6. * ICE_CP; w.particles.len()],
            air_heat: 0.,
        }
    }
    pub fn water(w: &World, air: climate::Air) -> Self {
        let mut t = Self::snow(w);
        t.air = air;
        t.scale = 1.;
        t.parcel_enthalpy = vec![LATENT + WATER_CP * air.temperature_c.max(1.); w.particles.len()];
        t
    }
    pub fn water_c(&self) -> f32 {
        if self.water_mass > 0. {
            self.water_energy / (WATER_CP * self.water_mass)
        } else {
            0.
        }
    }
    pub fn level(&self) -> f32 {
        (self.water_mass / 1000. + self.displaced) / (1.4 * 1.1)
    }
    pub fn report(&self, w: &World) -> Value {
        let min = self
            .parcel_enthalpy
            .iter()
            .map(|&h| temperature(h))
            .fold(f32::INFINITY, f32::min);
        let frozen = w
            .solid
            .as_ref()
            .map_or(0., |s| s.report()["mass_kg"].as_f64().unwrap_or(0.));
        let parcels = w
            .particles
            .iter()
            .filter(|p| p.active)
            .map(|p| p.mass as f64)
            .sum::<f64>();
        json!({"accounted_specimen_mass_kg":frozen+parcels+(self.water_mass-self.initial_bath_mass) as f64,"air_c":self.air.temperature_c,"water_c":self.water_c(),"thermal_time_scale":self.scale,"thermal_seconds":self.elapsed,
            "melted_kg":self.melted,"initial_frozen_mass_kg":self.initial_mass,"bath_mass_kg":self.water_mass,
            "air_heat_j":self.air_heat,"minimum_parcel_c":if min.is_finite(){Some(min)}else{None},
            "minimum_snow_volume_ratio":w.sand.as_ref().map_or(1.,|s|s.minimum_plastic_volume()),
            "model":"finite bath energy, air convection, latent heat; hydrostatic bath; cellwise ice melting"})
    }
    pub fn render(&self, out: &mut Vec<[f32; 6]>) {
        if self.bath {
            let h = self.level();
            // Opaque water under the floes; its height tracks transferred mass.
            skin::triangle(
                out,
                [[-0.70, h, -0.55], [0.70, h, 0.55], [0.70, h, -0.55]],
                [0.08, 0.32, 0.43],
            );
            skin::triangle(
                out,
                [[-0.70, h, -0.55], [-0.70, h, 0.55], [0.70, h, 0.55]],
                [0.08, 0.32, 0.43],
            );
            for (a, b) in [
                ([-0.75, 0., -0.60], [-0.70, 0.42, 0.60]),
                ([0.70, 0., -0.60], [0.75, 0.42, 0.60]),
                ([-0.70, 0., -0.60], [0.70, 0.42, -0.55]),
            ] {
                worker::box_mesh(out, a, b, [0.30, 0.34, 0.37]);
            }
        }
    }
}
pub(super) fn temperature(h: f32) -> f32 {
    if h < 0. {
        h / ICE_CP
    } else if h <= LATENT {
        0.
    } else {
        (h - LATENT) / WATER_CP
    }
}
impl World {
    pub(super) fn initialize_ice(&mut self, case: &str) -> Result<(), String> {
        let bath = case == "matter_ice";
        let y = if bath { 0.25 } else { 0.018 };
        let poly =
            crate::physics::cut_geometry::Poly::box_from([-0.25, y, -0.19], [0.25, y + 0.13, 0.19]);
        let seeds = solid_geometry::seeds(&poly, [4, 2, 3], 918, false);
        let volume = poly.measure(true).volume;
        let cells = solid_geometry::partition(&poly, &seeds, [1.; 3], 3_000_000)?;
        self.solid = Some(solid::Solid::from_specimen(
            solid_geometry::Specimen {
                cells,
                kind: SolidKind::Ice,
                grain: [0., 1., 0.],
                bark: [0.; 3],
                source: "freshwater ice; brittle interfaces with latent-heat melt transfer",
                volume,
            },
            case,
        )?);
        self.tool = Tool::new("matter_stone");
        self.tool.set_placement([0., y as f32 - 0.22, 0.], 0.);
        self.tool.force_limit = Some(250.);
        let mass = volume as f32 * 917.;
        self.initial_mass = mass;
        self.sand = Some(sand::Sand::new(0));
        self.thermal = Some(Thermal {
            air: climate::Air {
                temperature_c: 10.,
                wind_m_s: 1.,
                cloud: 0.3,
            },
            bath,
            water_mass: if bath { 1000. * 1.4 * 1.1 * 0.34 } else { 0. },
            water_energy: if bath {
                1000. * 1.4 * 1.1 * 0.34 * WATER_CP * 12.
            } else {
                0.
            },
            displaced: 0.,
            scale: 240.,
            elapsed: 0.,
            melted: 0.,
            initial_mass: mass,
            initial_bath_mass: if bath { 1000. * 1.4 * 1.1 * 0.34 } else { 0. },
            parcel_enthalpy: Vec::new(),
            air_heat: 0.,
        });
        if bath {
            // Collision walls match the finite reservoir footprint.
            self.tool.ground = vec![
                [-0.70, 0., -0.55],
                [0.70, 0., -0.55],
                [0.70, 0., 0.55],
                [-0.70, 0., 0.55],
                [-0.70, 0.60, -0.55],
                [0.70, 0.60, -0.55],
                [0.70, 0.60, 0.55],
                [-0.70, 0.60, 0.55],
            ];
            self.tool.ground_faces = vec![
                [0, 2, 1],
                [0, 3, 2],
                [0, 1, 5],
                [0, 5, 4],
                [1, 2, 6],
                [1, 6, 5],
                [2, 3, 7],
                [2, 7, 6],
                [3, 0, 4],
                [3, 4, 7],
            ];
        }
        Ok(())
    }
    pub(super) fn advance_thermal(&mut self, host: &mut Host) -> Result<(), String> {
        let Some(mut thermal) = self.thermal.take() else {
            return Ok(());
        };
        let dt = thermal.scale / 60.;
        thermal.elapsed += dt;
        if self.case == "matter_snow" && self.time > 20. {
            thermal.air.temperature_c = 8.;
        }
        if thermal.bath {
            // The supported reservoir also exchanges heat with ambient air.
            // Freezing is not implemented: liquid cooling stops at zero.
            let target = WATER_CP * thermal.water_mass * thermal.air.temperature_c.max(0.);
            let q = ((8. + 3. * thermal.air.wind_m_s)
                * 1.4
                * 1.1
                * (thermal.air.temperature_c - thermal.water_c())
                * dt)
                .clamp(
                    (target - thermal.water_energy).min(0.),
                    (target - thermal.water_energy).max(0.),
                );
            thermal.water_energy += q;
            thermal.air_heat += q;
            // Airborne meltwater joins the reservoir on entering its volume.
            // Remove its explicit parcel and APIC state together, preserving
            // mass and sensible heat in the hydrostatic representation.
            for i in (0..self.particles.len()).rev() {
                let p = &self.particles[i];
                if p.kind == Kind::Water
                    && p.p[0].abs() < 0.70
                    && p.p[2].abs() < 0.55
                    && p.p[1] <= thermal.level()
                {
                    thermal.water_mass += p.mass;
                    thermal.water_energy += p.mass * (thermal.parcel_enthalpy[i] - LATENT).max(0.);
                    self.particles.remove(i);
                    thermal.parcel_enthalpy.remove(i);
                    self.sand.as_mut().unwrap().remove_point(i);
                    self.topology_dirty = true;
                }
            }
        }
        if let Some(solid) = &mut self.solid {
            let drops = solid.exchange_heat(&mut thermal, dt)?;
            if !drops.is_empty() {
                self.particles.extend(drops);
                self.sand.as_mut().unwrap().resize(self.particles.len());
                self.topology_dirty = true;
            }
        }
        {
            // Porous snow parcels exchange heat with the air in their pores.
            // Each parcel retains its mass when its constitutive phase changes.
            for (i, p) in self.particles.iter_mut().enumerate() {
                if !p.active || !matches!(p.kind, Kind::Snow | Kind::Water) {
                    continue;
                }
                let h = &mut thermal.parcel_enthalpy[i];
                let old = *h;
                let area = 4. * std::f32::consts::PI * p.radius * p.radius;
                let conductance = (3. + 2. * thermal.air.wind_m_s) * area;
                let q = conductance * (thermal.air.temperature_c - temperature(*h)) * dt;
                // Bound exchange by the equilibrium enthalpy to avoid an
                // explicit heat step overshooting either side of 0 C.
                let equilibrium = if p.kind == Kind::Water {
                    LATENT + WATER_CP * thermal.air.temperature_c.max(0.)
                } else if thermal.air.temperature_c > 0. {
                    LATENT + WATER_CP * thermal.air.temperature_c
                } else {
                    ICE_CP * thermal.air.temperature_c
                };
                let dh = (q / p.mass).clamp((equilibrium - *h).min(0.), (equilibrium - *h).max(0.));
                *h += dh;
                thermal.air_heat += p.mass * (*h - old);
                if p.kind == Kind::Snow && *h >= LATENT {
                    p.kind = Kind::Water;
                    p.anchored = false;
                    p.radius = (p.mass / 1000.).cbrt() * 0.55;
                    if let Some(s) = &mut self.sand {
                        s.reset_point(i);
                    }
                    thermal.melted += p.mass;
                }
            }
        }
        self.thermal = Some(thermal);
        if self.topology_dirty && self.solid.is_some() {
            self.contact_order = (0..self.particles.len()).collect();
            self.configure_ice(host)?;
        }
        Ok(())
    }
    fn ice_positions(&self) -> Vec<V> {
        let mut p = self.solid.as_ref().unwrap().p.clone();
        p.extend(self.sample_positions());
        p.extend(&self.tool.p);
        p.extend(&self.tool.ground);
        p
    }
    pub(super) fn configure_ice(&mut self, host: &mut Host) -> Result<(), String> {
        let s = self.solid.as_ref().unwrap();
        let a = s.p.len();
        let b = a + self.particles.len() * 6;
        let solid_surface = s.contact_surface(0);
        let mut surfaces = Vec::new();
        if solid_surface["triangles"]
            .as_array()
            .is_some_and(|a| !a.is_empty())
        {
            surfaces.push(solid_surface);
        }
        if !self.particles.is_empty() {
            surfaces.push(self.sample_surface(a));
        }
        surfaces.extend([
            coupling::tool_surface(&self.tool, b),
            coupling::table_surface(&self.tool, b + self.tool.p.len()),
        ]);
        host.invoke("OGCConnectionSystem.Configure",json!({"positions":self.ice_positions(),"contact_radius":0.0012,"contact_stiffness":40000.,"surfaces":surfaces}))?;
        self.solid.as_mut().unwrap().dirty = false;
        self.topology_dirty = false;
        Ok(())
    }
    pub(super) fn advance_ice(&mut self, host: &mut Host) -> Result<(), String> {
        let wall = std::time::Instant::now();
        let dt = 1. / 240.;
        for _ in 0..4 {
            self.solid.as_mut().unwrap().refresh_interfaces();
            if self.solid.as_ref().unwrap().dirty || self.topology_dirty {
                self.configure_ice(host)?;
            }
            let previous = self.ice_positions();
            let old = self.tool.p.clone();
            let mut solid_step = self.solid.as_mut().unwrap().begin_step(dt);
            self.solid.as_ref().unwrap().apply_buoyancy(
                &mut solid_step,
                self.thermal.as_mut().unwrap(),
                dt,
            );
            let mut liquid_step = self.sample_begin(dt);
            if !self.particles.is_empty() {
                liquid_step.predicted = self.sand.as_mut().unwrap().predict(&self.particles, dt)?;
            }
            let goal = self.tool.goal(self.time, dt);
            let a = self.solid.as_ref().unwrap().p.len();
            let b = a + self.particles.len() * 6;
            let mut response = vec![[0.; 13]; self.tool.p.len()];
            for _ in 0..5 {
                let q = self
                    .solid
                    .as_mut()
                    .unwrap()
                    .material_target(&mut solid_step, dt);
                let liquid = self.sample_target(&mut liquid_step, dt);
                let batch = host.contact_responses(&self.ice_positions(), &previous)?;
                self.contacts = self.contacts.max(batch.contacts);
                self.solid
                    .as_mut()
                    .unwrap()
                    .accept_contact(&q, &batch.responses[..a], dt)?;
                self.sample_accept(&liquid, &batch.responses[a..b], dt);
                response.copy_from_slice(&batch.responses[b..b + self.tool.p.len()]);
                self.tool.project(&goal, &response, dt)?;
            }
            self.solid.as_mut().unwrap().finish_step(
                &solid_step,
                &self.tool,
                &old,
                &response,
                dt,
                self.tool.impact_energy(&old, dt),
            )?;
            self.sample_finish(&liquid_step, dt)?;
            if !self.particles.is_empty() {
                self.sand
                    .as_mut()
                    .unwrap()
                    .finish(&mut self.particles, &liquid_step.old, dt)?;
            }
            self.tool.record_contact(&old, &response, dt);
            self.tool.finish(&old, dt);
            self.time += dt;
            self.solver_iterations += 5;
        }
        self.frames += 1;
        if self.frames % 4 == 0 {
            self.skin = skin::reconstruct_with_budget(self, 6000);
        }
        self.step_ms = wall.elapsed().as_secs_f64() * 1000.;
        self.total_step_ms += self.step_ms;
        self.worst_step_ms = self.worst_step_ms.max(self.step_ms);
        Ok(())
    }
}
