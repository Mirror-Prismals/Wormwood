//! Cardinal ocean/river currents in a continuous water body's tangent frame.
//! Membership and bed/surface intersection come from the continuous hydrology query;
//! this module does not create sources, settle parcels, or infer occupied cells.
use super::math::{Frame, V, V2, dot2, finite, mix, unit2};

#[derive(Clone, Debug)]
pub(crate) struct Settings {
    pub ocean_push: f64,
    pub river_push: f64,
    pub response: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            ocean_push: 1.65,
            river_push: 1.30,
            response: 3.2,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Kind {
    #[default]
    Dry,
    Ocean,
    River,
    Lake,
}
/// Result of a water body query, with metre-space coordinates in its tangent frame.
/// The owner only constructs this for points inside a known wet region and above
/// its bed. None means dry or outside finite query coverage, never an invented bed.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WetPoint {
    pub frame: Frame,
    pub point: V,
    pub uv: V2,
    pub surface_height: f64,
    pub mean_height: f64,
    pub bed_height: f64,
    pub shore_distance: f64,
    pub shore_gradient: V2,
    pub wave_amplitude: f64,
    /// Toward the actual continuous shore (not a hardcoded island centre).
    pub shore_direction: V2,
    /// Signed along the continuous river course by its hydrology owner.
    pub downstream_direction: V2,
    pub kind: Kind,
}
impl WetPoint {
    fn valid(&self) -> bool {
        finite(self.point)
            && self
                .uv
                .iter()
                .chain(self.shore_gradient.iter())
                .all(|n| n.is_finite())
            && self.bed_height.is_finite()
            && self.shore_distance.is_finite()
            && finite(self.frame.origin)
            && finite(self.frame.right)
            && finite(self.frame.up)
            && finite(self.frame.forward)
            && [self.surface_height, self.mean_height, self.wave_amplitude]
                .iter()
                .all(|n| n.is_finite())
            && self
                .shore_direction
                .iter()
                .chain(self.downstream_direction.iter())
                .all(|n| n.is_finite())
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Sample {
    /// World-space metres per second. Strength is the separate submergence factor.
    pub velocity: V,
    pub surface_height: f64,
    pub depth: f64,
    pub strength: f64,
    pub kind: Kind,
    pub occupied: bool,
}
#[derive(Default)]
pub(crate) struct State {
    pub settings: Settings,
    pub time: f64,
    pub step_dt: f64,
    pub filters: std::collections::BTreeMap<u64, Filter>,
}
impl State {
    pub fn advance(&mut self, dt: f64) {
        if dt.is_finite() {
            self.step_dt = dt.clamp(0., 0.1);
            self.time += self.step_dt;
        }
    }
    pub fn sample(&self, wet: Option<&WetPoint>) -> Sample {
        let Some(w) = wet.filter(|w| w.valid()) else {
            return Sample::default();
        };
        let depth = w.surface_height - w.point[1];
        if depth < 0. || w.kind == Kind::Dry {
            return Sample::default();
        }
        let position = w.uv;
        let (velocity, strength) = match w.kind {
            Kind::Ocean => {
                let shore = unit2(w.shore_direction);
                let normalized = ((w.surface_height - w.mean_height) / w.wave_amplitude.max(0.125))
                    .clamp(-1., 1.);
                let push = 0.34 + normalized * 0.66;
                let side =
                    (position[0] * 0.071 + position[1] * 0.047 + self.time * 0.31).sin() * 0.18;
                (
                    [
                        shore[0] * push - shore[1] * side,
                        shore[1] * push + shore[0] * side,
                    ]
                    .map(|v| v * self.settings.ocean_push),
                    (depth / 1.4).clamp(0.15, 1.),
                )
            }
            Kind::River => {
                let direction = unit2(w.downstream_direction);
                let pulse =
                    0.72 + 0.28 * (dot2(position, direction) / 11. - self.time * 1.17).sin();
                (
                    direction.map(|v| v * pulse * self.settings.river_push),
                    (depth / 1.25).clamp(0.2, 1.),
                )
            }
            Kind::Lake => ([0.; 2], (depth / 1.25).clamp(0., 1.)),
            Kind::Dry => unreachable!(),
        };
        Sample {
            velocity: w.frame.vector([velocity[0], 0., velocity[1]]),
            surface_height: w.surface_height,
            depth,
            strength,
            kind: w.kind,
            occupied: true,
        }
    }
}
/// Per-consumer smoothing. Separate actors must not overwrite one shared sample.
#[derive(Default)]
pub(crate) struct Filter {
    velocity: V,
}
impl Filter {
    pub fn apply(&mut self, mut target: Sample, dt: f64, response: f64) -> Sample {
        let dt = if dt.is_finite() { dt.max(0.) } else { 0. };
        let response = if response.is_finite() {
            response.max(0.)
        } else {
            0.
        };
        self.velocity = mix(
            self.velocity,
            if target.occupied {
                target.velocity
            } else {
                [0.; 3]
            },
            1. - (-dt * response).exp(),
        );
        target.velocity = self.velocity;
        target
    }
}
/// Legacy downstream blend, now fed continuous bed probes and a signed course
/// direction by hydrology. The 6m probe spacing belongs to that query provider.
pub(crate) fn downstream(bed_xp: f64, bed_xm: f64, bed_zp: f64, bed_zm: f64, outward: V2) -> V2 {
    let outward = unit2(outward);
    let d = [bed_xm - bed_xp, bed_zm - bed_zp];
    if dot2(d, d) <= 1e-6 {
        return outward;
    }
    let mut downhill = unit2(d);
    if dot2(downhill, outward) < -0.25 {
        downhill = downhill.map(|v| -v);
    }
    unit2(std::array::from_fn(|i| {
        downhill[i] * 0.82 + outward[i] * 0.18
    }))
}
/// Principal-axis direction from actual neighbouring continuous channel samples.
/// Samples have tangent-frame offsets and are never integer occupancy probes.
pub(crate) fn channel_direction(samples: &[V2], fallback: V2) -> V2 {
    let fallback = unit2(fallback);
    let mut covariance = [0.; 3];
    let mut count = 0;
    for &p in samples {
        let d2 = dot2(p, p);
        if !d2.is_finite() || d2 <= 1e-12 || d2 > 20. {
            continue;
        }
        let weight = 1. / d2.sqrt().max(1.);
        covariance[0] += p[0] * p[0] * weight;
        covariance[1] += p[0] * p[1] * weight;
        covariance[2] += p[1] * p[1] * weight;
        count += 1;
    }
    if count < 2 {
        return fallback;
    }
    let angle = 0.5 * (2. * covariance[1]).atan2(covariance[0] - covariance[2]);
    let mut axis = [angle.cos(), angle.sin()];
    if dot2(axis, fallback) < 0. {
        axis = axis.map(|v| -v);
    }
    unit2(std::array::from_fn(|i| axis[i] * 0.88 + fallback[i] * 0.12))
}
