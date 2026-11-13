//! Preallocated per-bus stereo mixer. Only the serialized output callback mutates filters/delays.
use super::acoustic::Response;
#[derive(Clone, Copy)]
pub(crate) struct Route {
    pub active: bool,
    pub gain: f32,
    pub pan: f32,
    pub alpha: f32,
    pub echo_seconds: f32,
    pub echo_gain: f32,
    pub itd_ms: f32,
}
impl Default for Route {
    fn default() -> Self {
        Self {
            active: false,
            gain: 0.,
            pan: 0.,
            alpha: 0.,
            echo_seconds: 0.,
            echo_gain: 0.,
            itd_ms: 0.,
        }
    }
}
impl Route {
    pub fn from_response(r: &Response, user_gain: f64) -> Self {
        Self {
            active: true,
            gain: (r.gain * if r.occluded { 0.2 } else { 1. } * user_gain.clamp(0., 4.)) as f32,
            pan: r.pan as f32,
            alpha: r.hf_alpha as f32,
            echo_seconds: r.echo_delay as f32,
            echo_gain: r.echo_gain as f32,
            itd_ms: 0.,
        }
    }
}
struct Bus {
    echo: Box<[f32]>,
    itd: Box<[f32]>,
    write: usize,
    itd_write: usize,
    filter: f32,
    was_active: bool,
}
pub(crate) struct Mixer {
    rate: f32,
    buses: Vec<Bus>,
}
impl Mixer {
    pub fn new(rate: u32) -> Result<Self, String> {
        if !(8000..=192000).contains(&rate) {
            return Err("unsupported spatial sample rate".into());
        }
        Ok(Self {
            rate: rate as f32,
            buses: (0..17)
                .map(|_| Bus {
                    echo: vec![0.; rate as usize * 2 + 1].into_boxed_slice(),
                    itd: vec![0.; (rate as usize / 100) + 1].into_boxed_slice(),
                    write: 0,
                    itd_write: 0,
                    filter: 0.,
                    was_active: false,
                })
                .collect(),
        })
    }
    pub fn frame(&mut self, input: &[f32; 17], routes: &[Route; 17]) -> [f32; 2] {
        let mut out = [0.; 2];
        for (ch, b) in self.buses.iter_mut().enumerate() {
            let r = routes[ch];
            if !r.active {
                b.filter = 0.;
                b.was_active = false;
                continue;
            }
            // Delay history is tagged through a sample count, avoiding an O(rate) clear in callback.
            if !b.was_active {
                b.write = 0;
                b.itd_write = 0;
                b.filter = 0.;
                b.was_active = true;
            }
            let mut x = if input[ch].is_finite() {
                input[ch] * r.gain.clamp(0., 4.)
            } else {
                0.
            };
            let delay = (r.echo_seconds.clamp(0., 2.) * self.rate) as usize;
            let index = b.write % b.echo.len();
            let old = if delay > 0 && b.write >= delay {
                b.echo[(index + b.echo.len() - delay.min(b.echo.len() - 1)) % b.echo.len()]
            } else {
                0.
            };
            b.echo[index] = x;
            b.write = b.write.saturating_add(1);
            if delay > 0 {
                x += old * r.echo_gain.clamp(0., 1.);
            }
            if r.alpha > 0. {
                b.filter += r.alpha.clamp(0., 1.) * (x - b.filter);
                x = b.filter;
            }
            // AudioSystem.cpp applies rayPanStrength (0.35) before its linear
            // stereo gains, including each independent bird bus.
            let pan = (r.pan * 0.35).clamp(-1., 1.);
            let itd = (pan.abs() * r.itd_ms.clamp(0., 10.) * self.rate / 1000.).round() as usize;
            let ii = b.itd_write % b.itd.len();
            let delayed = if itd > 0 && b.itd_write >= itd {
                b.itd[(ii + b.itd.len() - itd.min(b.itd.len() - 1)) % b.itd.len()]
            } else {
                0.
            };
            b.itd[ii] = x;
            b.itd_write = b.itd_write.saturating_add(1);
            let l = if itd > 0 && pan >= 0. { delayed } else { x };
            let r = if itd > 0 && pan < 0. { delayed } else { x };
            out[0] += l * 0.5 * (1. - pan);
            out[1] += r * 0.5 * (1. + pan);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_bus_routes_pan_and_silence_without_cross_talk() {
        let mut mixer = Mixer::new(48000).unwrap();
        let mut frame = [0.; 17];
        frame[12] = 1.;
        frame[13] = 0.5;
        let mut routes = [Route::default(); 17];
        routes[12] = Route {
            active: true,
            gain: 1.,
            pan: -1.,
            ..Route::default()
        };
        routes[13] = Route {
            active: true,
            gain: 1.,
            pan: 1.,
            ..Route::default()
        };
        let out = mixer.frame(&frame, &routes);
        assert!((out[0] - 0.8375).abs() < 1e-6);
        assert!((out[1] - 0.6625).abs() < 1e-6);
        routes[12].active = false;
        let out = mixer.frame(&frame, &routes);
        assert!((out[0] - 0.1625).abs() < 1e-6);
        assert!((out[1] - 0.3375).abs() < 1e-6);
    }
    #[test]
    fn reactivated_bus_cannot_replay_old_delay_history() {
        let mut mixer = Mixer::new(8000).unwrap();
        let mut routes = [Route::default(); 17];
        routes[12] = Route {
            active: true,
            gain: 1.,
            echo_seconds: 0.001,
            echo_gain: 1.,
            ..Route::default()
        };
        let mut frame = [0.; 17];
        frame[12] = 1.;
        mixer.frame(&frame, &routes);
        routes[12].active = false;
        frame[12] = 0.;
        mixer.frame(&frame, &routes);
        routes[12].active = true;
        for _ in 0..20 {
            assert_eq!(mixer.frame(&frame, &routes), [0., 0.]);
        }
    }
    #[test]
    fn echo_arrives_at_sample_delay_on_the_routed_side_only() {
        let mut mixer = Mixer::new(8000).unwrap();
        let mut routes = [Route::default(); 17];
        routes[12] = Route {
            active: true,
            gain: 1.,
            pan: -1.,
            echo_seconds: 0.001,
            echo_gain: 0.5,
            ..Route::default()
        };
        let mut frame = [0.; 17];
        frame[12] = 1.;
        let out = mixer.frame(&frame, &routes);
        assert!((out[0] - 0.675).abs() < 1e-6);
        assert!((out[1] - 0.325).abs() < 1e-6);
        frame[12] = 0.;
        for _ in 1..8 {
            assert_eq!(mixer.frame(&frame, &routes), [0., 0.]);
        }
        let out = mixer.frame(&frame, &routes);
        assert!((out[0] - 0.3375).abs() < 1e-6);
        assert!((out[1] - 0.1625).abs() < 1e-6);
        assert_eq!(mixer.frame(&frame, &routes), [0., 0.]);
    }
}
