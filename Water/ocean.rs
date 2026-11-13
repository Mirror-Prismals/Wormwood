//! Cardinal's incoming, grouped, warped and crossing wave equations on continuous shores.
use super::math::{V2, dot2, noise, unit2};
#[derive(Clone, Debug)]
pub(crate) struct Settings {
    pub amplitude: f64,
    pub period: f64,
    pub wavelength: f64,
    pub direction: V2,
    pub shore_depth: f64,
    pub shore_gain: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            amplitude: 0.9,
            period: 8.,
            wavelength: 24.,
            direction: [0.25, -1.],
            shore_depth: 10.,
            shore_gain: 1.6,
        }
    }
}
#[derive(Default)]
pub(crate) struct State {
    pub settings: Settings,
    pub time: f64,
}
impl State {
    pub fn offset(&self, p: V2, depth: f64, shore_distance: f64) -> f64 {
        let [x, z] = p;
        let time = self.time;
        let w = self.settings.wavelength.max(4.);
        let period = self.settings.period.max(1.);
        let broad = noise((x + time * 0.17) / 83., (z - time * 0.11) / 83.);
        let fine = noise(
            (x - time * 0.09) / 31. + 19.7,
            (z + time * 0.13) / 31. - 7.3,
        );
        let warped = shore_distance + broad * w * 0.42 + fine * w * 0.16;
        let tau = std::f64::consts::TAU;
        let incoming = (warped / w + time / period) * tau;
        let group = (warped / (w * 4.73) + time / (period * 6.17) + broad * 0.31) * tau;
        let strength = (0.38 + 0.62 * (0.5 + 0.5 * group.sin())).clamp(0., 1.);
        let crossing = (dot2(p, unit2(self.settings.direction)) / (w * 1.71)
            - time / (period * 1.43)
            + fine * 0.23)
            * tau;
        let irregular = noise(x / 17. + time / 29., z / 17. - time / 37.) * 0.13;
        let shallow = 1. - (depth / self.settings.shore_depth.max(0.5)).clamp(0., 1.);
        let amplitude = self.settings.amplitude.max(0.)
            * (1. + (self.settings.shore_gain.max(1.) - 1.) * shallow);
        (incoming.sin() * strength + crossing.sin() * 0.24 + irregular).clamp(-1., 1.) * amplitude
    }
    pub fn surface(&self, p: V2, mean: f64, bed: f64, shore_distance: f64) -> f64 {
        mean + self.offset(p, (mean - bed).max(0.), shore_distance)
    }
    pub fn normal(
        &self,
        p: V2,
        mean: f64,
        bed: f64,
        shore_distance: f64,
        shore_gradient: V2,
    ) -> [f64; 3] {
        let e = 0.035;
        let center = self.surface(p, mean, bed, shore_distance);
        let dx = self.surface(
            [p[0] + e, p[1]],
            mean,
            bed,
            shore_distance + e * shore_gradient[0],
        ) - center;
        let dz = self.surface(
            [p[0], p[1] + e],
            mean,
            bed,
            shore_distance + e * shore_gradient[1],
        ) - center;
        super::math::unit([-dx / e, 1., -dz / e])
    }
}
