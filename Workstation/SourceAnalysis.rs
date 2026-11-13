//! Bounded observations of rendered source stereo, before LSFFR routing.
//! Allocating analysis belongs to the control/worker thread, never a callback.
use serde_json::{Value, json};
use std::f64::consts::PI;

pub(super) struct Observation {
    pre_energy: f64,
    energy: [f64; 2],
    cross: f64,
    gain_energy: f64,
    automated: bool,
    pcm: Vec<[f64; 2]>,
}

impl Observation {
    pub(super) fn new(frames: usize) -> Self {
        Self {
            pre_energy: 0.,
            energy: [0.; 2],
            cross: 0.,
            gain_energy: 0.,
            automated: false,
            pcm: Vec::with_capacity(frames),
        }
    }

    pub(super) fn push(&mut self, pre: [f32; 2], gain: f32, automated: bool) {
        // Match the renderer's f32 multiplication before accumulating in f64.
        let post = pre.map(|sample| f64::from(sample * gain));
        let pre = pre.map(f64::from);
        let gain = f64::from(gain);
        self.pre_energy += pre[0] * pre[0] + pre[1] * pre[1];
        self.energy[0] += post[0] * post[0];
        self.energy[1] += post[1] * post[1];
        self.cross += post[0] * post[1];
        self.gain_energy += gain * gain;
        self.automated |= automated;
        self.pcm.push(post);
    }

    pub(super) fn snapshot(&self, sample_rate: u32) -> Value {
        let frames = self.pcm.len().max(1) as f64;
        let total = self.energy[0] + self.energy[1];
        let pre = (self.pre_energy / (2. * frames)).sqrt();
        let post = (total / (2. * frames)).sqrt();
        let balance = if total > 0. {
            ((self.energy[1] - self.energy[0]) / total).clamp(-1., 1.)
        } else {
            0.
        };
        let product = (self.energy[0] * self.energy[1]).sqrt();
        // Correlation distinguishes a stereo image from simple level imbalance.
        // A single occupied channel is narrow; balanced opposing phase is wide.
        let width = if product > 0. && total > 0. {
            let correlation = (self.cross / product).clamp(-1., 1.);
            ((1. - correlation) * 0.5).sqrt() * (2. * product / total)
        } else {
            0.
        };
        json!({
            "rms_pre_gain":pre, "rms_post_gain":post,
            "rms_l":(self.energy[0]/frames).sqrt(),
            "rms_r":(self.energy[1]/frames).sqrt(),
            "stereo_balance":balance, "stereo_width":width.clamp(0.,1.),
            "spectral_centroid_hz":if total>0. {centroid(&self.pcm,sample_rate)} else {None},
            "effective_gain":if pre>0. {post/pre} else {(self.gain_energy/frames).sqrt()},
            "automated":self.automated,
        })
    }
}

#[derive(Clone, Copy, Default)]
pub(super) struct Complex {
    pub(super) re: f64,
    pub(super) im: f64,
}

pub(super) fn fft(values: &mut [Complex]) {
    let n = values.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            values.swap(i, j);
        }
    }
    let mut width = 2;
    while width <= n {
        let angle = -2. * PI / width as f64;
        let step = Complex {
            re: angle.cos(),
            im: angle.sin(),
        };
        for start in (0..n).step_by(width) {
            let mut phase = Complex { re: 1., im: 0. };
            for offset in 0..width / 2 {
                let a = values[start + offset];
                let b = values[start + offset + width / 2];
                let odd = Complex {
                    re: b.re * phase.re - b.im * phase.im,
                    im: b.re * phase.im + b.im * phase.re,
                };
                values[start + offset] = Complex {
                    re: a.re + odd.re,
                    im: a.im + odd.im,
                };
                values[start + offset + width / 2] = Complex {
                    re: a.re - odd.re,
                    im: a.im - odd.im,
                };
                phase = Complex {
                    re: phase.re * step.re - phase.im * step.im,
                    im: phase.re * step.im + phase.im * step.re,
                };
            }
        }
        width *= 2;
    }
}

fn centroid(pcm: &[[f64; 2]], rate: u32) -> Option<f64> {
    // Tiny blocks cannot meaningfully locate a frequency. A render is bounded
    // to 4096 frames; there is no retained PCM or growing spectral history.
    if pcm.len() < 16 {
        return None;
    }
    let size = pcm.len().next_power_of_two();
    let mut bins = vec![Complex::default(); size];
    let mut weighted = 0.;
    let mut power = 0.;
    for channel in 0..2 {
        bins.fill(Complex::default());
        for (i, sample) in pcm.iter().enumerate() {
            let window = 0.5 - 0.5 * (2. * PI * i as f64 / (pcm.len() - 1) as f64).cos();
            bins[i].re = sample[channel] * window;
        }
        fft(&mut bins);
        for (i, bin) in bins.iter().enumerate().take(size / 2 + 1) {
            // One-sided power includes the mirrored negative-frequency energy.
            let factor = if i == 0 || i == size / 2 { 1. } else { 2. };
            let energy = (bin.re * bin.re + bin.im * bin.im) * factor;
            power += energy;
            weighted += energy * i as f64 * rate as f64 / size as f64;
        }
    }
    (power > 0.).then(|| weighted / power)
}
