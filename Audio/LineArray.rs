//! Cardinal theatre cabinet propagation. Geometry/route preparation runs off callback;
//! only the preallocated Mixer and complete route snapshots enter native audio.
use super::{V, cross, dot, length, sub, unit};

pub(crate) const MAX_CABINETS: usize = 48;
const HISTORY: usize = 262144;

#[derive(Clone, Debug)]
pub(crate) struct Cabinet {
    pub id: u64,
    pub bus: usize,
    /// World-space acoustic mouth, outside the cabinet enclosure.
    pub position: V,
    pub forward: V,
    pub up: V,
    pub horizontal: [f32; 3],
    pub vertical: [f32; 3],
    pub band_gain: [f32; 3],
    pub gain: f32,
}
impl Cabinet {
    pub fn validate(&self) -> Result<(), String> {
        if self.bus >= 4
            || !self.gain.is_finite()
            || !(0.0..=16.).contains(&self.gain)
            || self
                .position
                .iter()
                .chain(self.forward.iter())
                .chain(self.up.iter())
                .any(|v| !v.is_finite())
            || length(self.forward) < 0.5
            || length(self.up) < 0.5
            || length(cross(unit(self.up), unit(self.forward))) < 0.5
            || self
                .horizontal
                .iter()
                .chain(self.vertical.iter())
                .any(|v| !v.is_finite() || !(1.0..=360.).contains(v))
            || self
                .band_gain
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=16.).contains(v))
        {
            return Err("invalid theatre cabinet".into());
        }
        Ok(())
    }
    pub fn polar(&self, listener: V, band: usize) -> f32 {
        let delta = sub(listener, self.position);
        if length(delta) < 0.001 {
            return self.band_gain[band];
        }
        let forward = unit(self.forward);
        let right = unit(cross(self.up, forward));
        let up = unit(cross(forward, right));
        let x = dot(delta, right);
        let y = dot(delta, up);
        let z = dot(delta, forward);
        let h = x.atan2(z).to_degrees() as f32;
        let v = y.atan2((x * x + z * z).sqrt()).to_degrees() as f32;
        let lobe = |angle: f32, width: f32| {
            if width >= 359. {
                1.
            } else {
                (-std::f32::consts::LN_2 * (2. * angle / width.max(1.)).powi(2)).exp()
            }
        };
        self.band_gain[band] * lobe(h, self.horizontal[band]) * lobe(v, self.vertical[band])
    }
    pub fn route(&self, listener: V, listener_right: V, visible: bool) -> Voice {
        let delta = sub(self.position, listener);
        let distance = length(delta);
        let pan = if distance > 0.001 {
            dot(unit(delta), unit(listener_right)).clamp(-1., 1.) as f32
        } else {
            0.
        };
        let falloff = if visible && distance < 180. {
            self.gain * 4. / (distance as f32).max(4.)
        } else {
            0.
        };
        Voice {
            bus: self.bus,
            delay: (distance / 343.) as f32,
            left: (0.5 * (1. - pan)).sqrt(),
            right: (0.5 * (1. + pan)).sqrt(),
            bands: std::array::from_fn(|i| falloff * self.polar(listener, i)),
            ..Voice::default()
        }
    }
}

#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct Voice {
    pub bus: usize,
    pub delay: f32,
    pub left: f32,
    pub right: f32,
    pub bands: [f32; 3],
    previous_delay: f32,
    delay_fade: f32,
}
#[derive(Clone, Copy)]
pub(crate) struct Routes {
    pub count: usize,
    pub voices: [Voice; MAX_CABINETS],
}
impl Default for Routes {
    fn default() -> Self {
        Self {
            count: 0,
            voices: [Voice::default(); MAX_CABINETS],
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [f32; 2],
}
fn clean(x: f32) -> f32 {
    if x.is_finite() && x.abs() >= 1e-20 {
        x
    } else {
        0.
    }
}
impl Biquad {
    fn new(rate: f32, hz: f32, kind: u8, gain: f32, q: f32) -> Self {
        if hz <= 0. {
            return Self {
                b: [1., 0., 0.],
                ..Self::default()
            };
        }
        let w = std::f32::consts::TAU * hz.clamp(1., rate * 0.45) / rate;
        let c = w.cos();
        let alpha = w.sin() / (2. * q);
        let a = 10f32.powf(gain / 40.);
        let (b, den, a1, a2) = match kind {
            0 => (
                [(1. + c) * 0.5, -(1. + c), (1. + c) * 0.5],
                1. + alpha,
                -2. * c,
                1. - alpha,
            ),
            1 => (
                [(1. - c) * 0.5, 1. - c, (1. - c) * 0.5],
                1. + alpha,
                -2. * c,
                1. - alpha,
            ),
            _ => (
                [1. + alpha * a, -2. * c, 1. - alpha * a],
                1. + alpha / a,
                -2. * c,
                1. - alpha / a,
            ),
        };
        Self {
            b: b.map(|x| x / den),
            a: [a1 / den, a2 / den],
            z: [0.; 2],
        }
    }
    fn process(&mut self, x: f32) -> f32 {
        if !x.is_finite() {
            self.z = [0.; 2];
            return 0.;
        }
        let y = self.b[0] * x + self.z[0];
        self.z = [
            clean(self.b[1] * x - self.a[0] * y + self.z[1]),
            clean(self.b[2] * x - self.a[1] * y),
        ];
        clean(y)
    }
}
pub(crate) struct Mixer {
    rate: f32,
    history: Box<[[f32; 3]]>,
    filters: [[Biquad; 5]; 4],
    low: [f32; 4],
    upper: [f32; 4],
    voices: [Voice; MAX_CABINETS],
    targets: Routes,
    write: usize,
    filled: usize,
    alpha_low: f32,
    alpha_upper: f32,
    peaks: [f32; 4],
}
impl Mixer {
    pub fn new(rate: u32) -> Result<Self, String> {
        if !(8000..=192000).contains(&rate) {
            return Err("invalid cabinet sample rate".into());
        }
        let rate = rate as f32;
        let filters = std::array::from_fn(|bus| {
            let (hp, lp, hz, db, q) = match bus {
                1 => (30., 100., 55., 2., 0.707),
                2 => (120., 18000., 250., -2., 0.8),
                _ => (100., 18000., 2500., -1.5, 1.),
            };
            [
                Biquad::new(rate, hp, 0, 0., std::f32::consts::FRAC_1_SQRT_2),
                Biquad::new(rate, hp, 0, 0., std::f32::consts::FRAC_1_SQRT_2),
                Biquad::new(rate, lp, 1, 0., std::f32::consts::FRAC_1_SQRT_2),
                Biquad::new(rate, lp, 1, 0., std::f32::consts::FRAC_1_SQRT_2),
                Biquad::new(rate, hz, 2, db, q),
            ]
        });
        Ok(Self {
            rate,
            history: vec![[0.; 3]; 4 * HISTORY].into_boxed_slice(),
            filters,
            low: [0.; 4],
            upper: [0.; 4],
            voices: [Voice::default(); MAX_CABINETS],
            targets: Routes::default(),
            write: 0,
            filled: 0,
            alpha_low: 1. - (-std::f32::consts::TAU * 120. / rate).exp(),
            alpha_upper: 1. - (-std::f32::consts::TAU * 1600. / rate).exp(),
            peaks: [0.; 4],
        })
    }
    /// Invalidate history by sample count, never clear large buffers in callback.
    pub fn reset(&mut self) {
        self.write = 0;
        self.filled = 0;
        self.low = [0.; 4];
        self.upper = [0.; 4];
        self.peaks = [0.; 4];
        for bank in &mut self.filters {
            for f in bank {
                f.z = [0.; 2];
            }
        }
        self.voices = [Voice::default(); MAX_CABINETS];
    }
    pub fn set_routes(&mut self, routes: Routes) {
        self.targets = routes;
    }
    pub fn begin_block(&mut self) {
        self.peaks = [0.; 4];
    }
    pub fn peaks(&self) -> [f32; 4] {
        self.peaks
    }
    pub fn frame(&mut self, input: [f32; 4]) -> [f32; 2] {
        for bus in 0..4 {
            let mut shaped = clean(input[bus]);
            for f in &mut self.filters[bus] {
                shaped = f.process(shaped);
            }
            self.low[bus] = clean(self.low[bus] + self.alpha_low * (shaped - self.low[bus]));
            self.upper[bus] =
                clean(self.upper[bus] + self.alpha_upper * (shaped - self.upper[bus]));
            self.peaks[bus] = self.peaks[bus].max(shaped.abs());
            self.history[bus * HISTORY + self.write] = [
                self.low[bus],
                self.upper[bus] - self.low[bus],
                shaped - self.upper[bus],
            ];
        }
        let mut out = [0.; 2];
        for i in 0..self.targets.count.min(MAX_CABINETS) {
            let target = self.targets.voices[i];
            let v = &mut self.voices[i];
            if self.filled == 0 || v.bus != target.bus {
                *v = Voice {
                    bus: target.bus,
                    delay: target.delay,
                    previous_delay: target.delay,
                    delay_fade: 1.,
                    ..Voice::default()
                };
            }
            if v.delay_fade >= 1. && (target.delay - v.delay).abs() * self.rate >= 1. {
                v.previous_delay = v.delay;
                v.delay = target.delay;
                v.delay_fade = 0.;
            }
            v.left = clean(v.left + (target.left - v.left) * 0.002);
            v.right = clean(v.right + (target.right - v.right) * 0.002);
            for band in 0..3 {
                v.bands[band] = clean(v.bands[band] + (target.bands[band] - v.bands[band]) * 0.002);
            }
            let read = |seconds: f32| {
                let delay = seconds * self.rate;
                if !delay.is_finite() || delay < 0. || delay >= (HISTORY - 2) as f32 || v.bus >= 4 {
                    return 0.;
                }
                let whole = delay as usize;
                if whole + 1 >= self.filled {
                    return 0.;
                }
                let fraction = delay - whole as f32;
                let a = (self.write + HISTORY - whole) % HISTORY;
                let b = (a + HISTORY - 1) % HISTORY;
                (0..3)
                    .map(|band| {
                        (self.history[v.bus * HISTORY + a][band] * (1. - fraction)
                            + self.history[v.bus * HISTORY + b][band] * fraction)
                            * v.bands[band]
                    })
                    .sum::<f32>()
            };
            let mut sample = read(v.delay);
            if v.delay_fade < 1. {
                let old = read(v.previous_delay);
                sample = old + (sample - old) * v.delay_fade;
                v.delay_fade = (v.delay_fade + 1. / (0.02 * self.rate)).min(1.);
            }
            out[0] += sample * v.left;
            out[1] += sample * v.right;
        }
        self.write = (self.write + 1) % HISTORY;
        self.filled = (self.filled + 1).min(HISTORY);
        out.map(clean)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cabinet() -> Cabinet {
        Cabinet {
            id: 1,
            bus: 0,
            position: [0.; 3],
            forward: [0., 0., 1.],
            up: [0., 1., 0.],
            horizontal: [90.; 3],
            vertical: [30.; 3],
            band_gain: [1.; 3],
            gain: 1.,
        }
    }
    #[test]
    fn cabinet_polar_and_distance_follow_authored_axes() {
        let c = cabinet();
        c.validate().unwrap();
        assert!((c.polar([0., 0., 10.], 1) - 1.).abs() < 1e-6);
        assert!((c.polar([10., 0., 10.], 1) - 0.5).abs() < 1e-6);
        let route = c.route([0., 0., 8.], [1., 0., 0.], true);
        assert!((route.bands[1] - 0.5).abs() < 1e-6);
        assert_eq!(c.route([0., 0., 8.], [1., 0., 0.], false).bands, [0.; 3]);
    }
    #[test]
    fn theatre_camera_pan_reverses_ears_without_changing_distance_or_spectrum() {
        let c=cabinet();let position=[4.,0.,10.];
        let facing=c.route(position,[1.,0.,0.],true);
        let reverse=c.route(position,[-1.,0.,0.],true);
        assert!(facing.left>facing.right);
        assert_eq!(facing.left,reverse.right);assert_eq!(facing.right,reverse.left);
        assert_eq!(facing.bands,reverse.bands);assert_eq!(facing.delay,reverse.delay);
        let outside=c.route([1000.,0.,0.],[1.,0.,0.],true);
        assert_eq!(outside.bands,[0.;3]);
    }
    #[test]
    fn four_buses_do_not_feed_another_cabinet_group() {
        let mut m = Mixer::new(8000).unwrap();
        let mut r = Routes::default();
        r.count = 1;
        r.voices[0] = Voice {
            bus: 3,
            left: 1.,
            right: 0.,
            bands: [1.; 3],
            ..Voice::default()
        };
        m.set_routes(r);
        for _ in 0..200 {
            assert_eq!(m.frame([1., 0., 0., 0.]), [0.; 2]);
        }
        assert!(m.peaks()[0] > 0.);
        assert_eq!(m.peaks()[3], 0.);
        let mut energy = 0.;
        for _ in 0..200 {
            energy += m.frame([0., 0., 0., 1.])[0].abs();
        }
        assert!(energy > 0.01);
    }
    #[test]
    fn reset_suppresses_old_propagation_history() {
        let mut m = Mixer::new(8000).unwrap();
        let mut r = Routes::default();
        r.count = 1;
        r.voices[0] = Voice {
            bus: 0,
            left: 1.,
            right: 1.,
            bands: [1.; 3],
            delay: 0.01,
            ..Voice::default()
        };
        m.set_routes(r);
        for _ in 0..100 {
            m.frame([1., 0., 0., 0.]);
        }
        m.reset();
        for _ in 0..200 {
            assert_eq!(m.frame([0.; 4]), [0.; 2]);
        }
    }
}
