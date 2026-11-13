//! Material initial conditions for the occupied game world. Workshop specimen
//! outlines and automatic choreography are deliberately not used here.
use super::*;

impl World {
    /// Volume quadrature clips parcels to occupied world material. Removed
    /// samples were air or already-owned matter, not newly excavated mass.
    pub(super) fn restrict_volume(&mut self, distance: impl Fn(V) -> f64) -> Result<(), String> {
        let mut mapping = vec![usize::MAX; self.particles.len()];
        let mut kept = Vec::new();
        for (i, p) in self.particles.iter().enumerate() {
            let half = p.radius * 0.65;
            let count = (0..8)
                .filter(|bits| {
                    distance(std::array::from_fn(|k| {
                        p.p[k] + if bits & (1 << k) == 0 { -half } else { half }
                    })) <= 0.
                })
                .count();
            if count == 0 {
                continue;
            }
            let mut p = p.clone();
            let fraction = count as f32 / 8.;
            p.mass *= fraction;
            p.radius *= fraction.cbrt();
            mapping[i] = kept.len();
            kept.push(p);
        }
        if kept.is_empty() {
            return Err("No unrefined material occupies this volume".into());
        }
        self.particles = kept;
        self.bonds.retain_mut(|b| {
            if mapping[b.a] == usize::MAX || mapping[b.b] == usize::MAX {
                return false;
            }
            b.a = mapping[b.a];
            b.b = mapping[b.b];
            true
        });
        self.initial_mass = self.particles.iter().map(|p| p.mass).sum();
        self.contact_order = (0..self.particles.len()).collect();
        if self.sand.is_some() {
            self.sand = Some(sand::Sand::new(self.particles.len()));
        }
        self.refresh_shape_neighbours();
        self.skin = skin::reconstruct_with_budget(self, 6000);
        Ok(())
    }
    pub(super) fn filled_box(min: V, max: V, kind: Kind, boundary: bool) -> Self {
        Self::filled_box_spaced(min,max,kind,boundary,if kind==Kind::Dirt {0.060} else {0.032})
    }
    pub(super) fn filled_box_spaced(min: V, max: V, kind: Kind, boundary: bool, minimum_spacing:f32) -> Self {
        let mut w = World::empty("game_material", false);
        w.particles.clear();
        w.bonds.clear();
        w.families.clear();
        w.garden = Garden::default();
        w.sand = None;
        let size = sub(max, min);
        let volume = size[0] * size[1] * size[2];
        let spacing = (volume / 640.)
            .cbrt()
            .max(minimum_spacing);
        let n = size.map(|s| (s / spacing).floor().max(1.) as usize);
        let cell: V = std::array::from_fn(|k| size[k] / n[k] as f32);
        let parcel = cell[0] * cell[1] * cell[2];
        for y in 0..n[1] {
            for z in 0..n[2] {
                for x in 0..n[0] {
                    let index = [x, y, z];
                    let p = std::array::from_fn(|k| min[k] + (index[k] as f32 + 0.5) * cell[k]);
                    w.particles.push(Particle {
                        p,
                        rest: p,
                        v: [0.; 3],
                        spin: [0.; 3],
                        radius: parcel.cbrt() * 0.55,
                        mass: parcel * kind.law().density,
                        kind,
                        anchored: boundary
                            && (y == 0 || x == 0 || x + 1 == n[0] || z == 0 || z + 1 == n[2]),
                        active: true,
                        level: 0,
                        quiet: 0.,
                        moisture: 0.45,
                        region: 0,
                        activity: 0.,
                    });
                }
            }
        }
        if kind == Kind::Dirt {
            let reach = length(cell) * 1.01;
            for a in 0..w.particles.len() {
                for b in a + 1..w.particles.len() {
                    if length(sub(w.particles[a].p, w.particles[b].p)) < reach {
                        w.bond(a, b, parcel.powf(2. / 3.) * 0.24, 1., 0.);
                    }
                }
            }
        } else {
            w.sand = Some(sand::Sand::new(w.particles.len()));
        }
        w.initial_mass = w.particles.iter().map(|p| p.mass).sum();
        w.contact_order = (0..w.particles.len()).collect();
        w.refresh_shape_neighbours();
        w.skin = skin::reconstruct_with_budget(&mut w, 6000);
        w
    }
    pub(super) fn filter_water(&mut self, occupied: &[bool]) -> Result<(), String> {
        if occupied.len() != self.particles.len() {
            return Err("water transfer sample count mismatch".into());
        }
        let mut i = 0;
        self.particles.retain(|_| {
            let retain = occupied[i];
            i += 1;
            retain
        });
        if self.particles.is_empty() {
            return Err("No water in reach".into());
        }
        self.contact_order = (0..self.particles.len()).collect();
        self.initial_mass = self.particles.iter().map(|p| p.mass).sum();
        self.sand = Some(sand::Sand::new(self.particles.len()));
        self.skin = skin::reconstruct_with_budget(self, 6000);
        Ok(())
    }
    pub(super) fn shear(&mut self, slope: [f32; 2]) -> Result<(), String> {
        for p in &mut self.particles {
            p.p[1] += slope[0] * p.p[0] + slope[1] * p.p[2];
            p.rest = p.p;
        }
        for b in &mut self.bonds {
            b.rest = length(sub(self.particles[b.a].rest, self.particles[b.b].rest));
        }
        if let Some(l) = &mut self.layers {
            l.shear(slope)?;
        }
        if !self.particles.is_empty() {
            self.skin = skin::reconstruct_with_budget(self, 6000);
        }
        self.refresh_shape_neighbours();
        Ok(())
    }
    pub(super) fn collision(&self) -> Vec<[V; 3]> {
        if let Some(l) = &self.layers {
            return l.collision();
        }
        if let Some(s) = &self.solid {
            return s.collision();
        }
        if let Some(t) = &self.turf {
            return t.collision();
        }
        self.skin
            .chunks_exact(3)
            .map(|t| std::array::from_fn(|k| [t[k][0], t[k][1], t[k][2]]))
            .collect()
    }
    pub(super) fn payload_count(&self) -> usize {
        let mut count = self.tool.carried_samples(
            self.particles
                .iter()
                .filter(|p| p.active)
                .map(|p| (p.p, p.radius)),
        );
        if let Some(l) = &self.layers {
            count += l.payload_count(&self.tool);
        }
        count
    }
    pub(super) fn moving(&self) -> bool {
        self.particles
            .iter()
            .any(|p| p.active && !p.anchored && length(p.v) > 0.08)
            || self.solid.as_ref().is_some_and(|s| s.moving())
            || self.turf.as_ref().is_some_and(|s| s.moving())
            || self.layers.as_ref().is_some_and(|s| s.moving())
    }
}
