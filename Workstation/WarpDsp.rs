//! Direct port of Cardinal Host/DawGeneralWarpDSP.h (algorithm version1).
//! Immutable input, stereo phase-locked spectral stretch, transient phase reset,
//! optional pitch shift via windowed-sinc resampling. Run on a worker/control
//! thread; never a device callback. Anchor coordinates are samples, not a new
//! project clock. The map owner must convert its authoritative clock first.
use std::{
    f64::consts::PI,
    sync::atomic::{AtomicBool, Ordering},
};
const TAU: f64 = 2. * PI;
const MAX_FRAMES: usize = 16_777_216;
#[derive(Clone, Copy, Debug)]
pub(crate) struct Anchor {
    pub destination: f64,
    pub source: f64,
}
pub(crate) struct Input<'a> {
    pub left: &'a [f32],
    pub right: &'a [f32],
    pub anchors: &'a [Anchor],
    pub frames: usize,
    pub sample_rate: f64,
    pub pitch: f64,
}
#[derive(Debug)]
pub(crate) struct Output {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}
#[derive(Clone, Copy, Default)]
struct C {
    re: f64,
    im: f64,
}
impl C {
    fn polar(m: f64, p: f64) -> Self {
        Self {
            re: m * p.cos(),
            im: m * p.sin(),
        }
    }
    fn norm(self) -> f64 {
        self.re * self.re + self.im * self.im
    }
    fn mag(self) -> f64 {
        self.re.hypot(self.im)
    }
    fn phase(self) -> f64 {
        self.im.atan2(self.re)
    }
    fn conj(self) -> Self {
        Self {
            re: self.re,
            im: -self.im,
        }
    }
    fn mul(self, b: Self) -> Self {
        Self {
            re: self.re * b.re - self.im * b.im,
            im: self.re * b.im + self.im * b.re,
        }
    }
}
struct Fft {
    reverse: Vec<usize>,
    roots: Vec<C>,
}
impl Fft {
    fn new(n: usize) -> Self {
        Self {
            reverse: (0..n)
                .map(|i| i.reverse_bits() >> (usize::BITS - n.trailing_zeros()))
                .collect(),
            roots: (0..n / 2)
                .map(|i| C::polar(1., -TAU * i as f64 / n as f64))
                .collect(),
        }
    }
    fn transform(&self, x: &mut [C], inverse: bool) {
        let n = x.len();
        for i in 0..n {
            if i < self.reverse[i] {
                x.swap(i, self.reverse[i]);
            }
        }
        let mut len = 2;
        while len <= n {
            for start in (0..n).step_by(len) {
                for j in 0..len / 2 {
                    let r = self.roots[j * n / len];
                    let r = if inverse { r.conj() } else { r };
                    let a = x[start + j];
                    let b = x[start + j + len / 2].mul(r);
                    x[start + j] = C {
                        re: a.re + b.re,
                        im: a.im + b.im,
                    };
                    x[start + j + len / 2] = C {
                        re: a.re - b.re,
                        im: a.im - b.im,
                    };
                }
            }
            len *= 2;
        }
        if inverse {
            for v in x {
                v.re /= n as f64;
                v.im /= n as f64;
            }
        }
    }
}
fn wrap(p: f64) -> f64 {
    // std::remainder reduces before subtracting the nearest multiple. A direct
    // p - round(p / TAU) * TAU loses low bits and can alter phase-peak choices.
    let r = p % TAU;
    if r.abs() > PI || (r.abs() == PI && ((p - r) / TAU) % 2. != 0.) {
        r - TAU.copysign(p)
    } else {
        r
    }
}
fn sample(x: &[f32], position: i64) -> f64 {
    if position < 0 {
        return 0.;
    }
    x.get(position as usize)
        .copied()
        .filter(|v| v.is_finite())
        .unwrap_or(0.) as f64
}
fn map_source(anchors: &[Anchor], destination: f64) -> f64 {
    let right = anchors
        .partition_point(|a| a.destination <= destination)
        .clamp(1, anchors.len() - 1);
    let a = anchors[right - 1];
    let b = anchors[right];
    a.source
        + (destination - a.destination) * (b.source - a.source) / (b.destination - a.destination)
}
fn canceled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|c| c.load(Ordering::Relaxed))
}
pub(crate) fn render(
    input: Input<'_>,
    cancel: Option<&AtomicBool>,
) -> Result<Option<Output>, String> {
    if input.anchors.len() < 2
        || input.anchors.len() > 4096
        || input.frames == 0
        || input.frames > MAX_FRAMES
        || input.left.is_empty()
        || input.left.len() > MAX_FRAMES
        || (!input.right.is_empty() && input.right.len() != input.left.len())
        || !input.sample_rate.is_finite()
        || !(8000. ..=192000.).contains(&input.sample_rate)
    {
        return Err("Empty or out-of-bounds warp source or schedule".into());
    }
    if !input.pitch.is_finite() || !(0.25..=4.).contains(&input.pitch) {
        return Err("General warp supports pitch from -24 to +24 semitones".into());
    }
    if input
        .anchors
        .iter()
        .any(|a| !a.source.is_finite() || !a.destination.is_finite())
    {
        return Err("Invalid warp anchor".into());
    }
    let pitch = if (input.pitch - 1.).abs() < 1e-10 {
        1.
    } else {
        input.pitch
    };
    for pair in input.anchors.windows(2) {
        let a = pair[0];
        let b = pair[1];
        let ratio = (b.source - a.source) / (b.destination - a.destination) / pitch;
        if !ratio.is_finite() || b.destination <= a.destination || !(0.25..=4.).contains(&ratio) {
            return Err(
                "General warp local time/pitch ratio exceeds the initial 1:4 to 4:1 range".into(),
            );
        }
    }
    let count = input.frames;
    let source_start = map_source(input.anchors, 0.);
    let identity = pitch == 1.
        && input
            .anchors
            .iter()
            .all(|a| (a.source - source_start - a.destination).abs() < 1e-6);
    let mut output = Output {
        left: vec![0.; count],
        right: vec![0.; count],
    };
    let right = if input.right.is_empty() {
        input.left
    } else {
        input.right
    };
    if identity && (source_start - source_start.round()).abs() < 1e-6 {
        let start = source_start.round() as i64;
        for i in 0..count {
            if i & 4095 == 0 && canceled(cancel) {
                return Ok(None);
            }
            output.left[i] = sample(input.left, start + i as i64) as f32;
            output.right[i] = sample(right, start + i as i64) as f32;
        }
        return Ok(Some(output));
    }
    let mut n = 2048;
    while (n as f64) < input.sample_rate * 0.08 && n < 16384 {
        n *= 2;
    }
    let hop = n / 16;
    let bins = n / 2 + 1;
    let intermediate = (count as f64 * pitch).ceil() as usize;
    if intermediate > MAX_FRAMES {
        return Err("Warp intermediate exceeds bounded worker capacity".into());
    }
    let mut out_l = vec![0f32; intermediate];
    let mut out_r = vec![0f32; intermediate];
    let mut weights = vec![0f32; intermediate];
    let window: Vec<f64> = (0..n)
        .map(|i| 0.5 - 0.5 * (TAU * i as f64 / n as f64).cos())
        .collect();
    let mut mag = vec![0f64; bins];
    let mut old_mag = mag.clone();
    let mut phase_l = mag.clone();
    let mut phase_r = mag.clone();
    let mut old_l = mag.clone();
    let mut old_r = mag.clone();
    let mut output_phase_l = mag.clone();
    let mut output_phase_r = mag.clone();
    let mut peak_phase = mag.clone();
    let mut left = vec![C::default(); n];
    let mut right_spectrum = left.clone();
    let mut peaks = Vec::with_capacity(bins);
    let fft = Fft::new(n);
    let mut first = true;
    let mut old_center = 0i64;
    let mut center = -(n as i64 / 2);
    while center < (intermediate + n / 2) as i64 {
        if canceled(cancel) {
            return Ok(None);
        }
        let mapped = map_source(input.anchors, center as f64 / pitch);
        if !mapped.is_finite() || mapped.abs() > i64::MAX as f64 / 2. {
            return Err("Invalid general warp source position".into());
        }
        let source_center = mapped.round() as i64;
        for i in 0..n {
            let source = source_center + i as i64 - n as i64 / 2;
            left[i] = C {
                re: sample(input.left, source) * window[i],
                im: 0.,
            };
            right_spectrum[i] = C {
                re: sample(right, source) * window[i],
                im: 0.,
            };
        }
        fft.transform(&mut left, false);
        fft.transform(&mut right_spectrum, false);
        let mut positive_flux = 0.;
        let mut total = 0.;
        let mut maximum = 0f64;
        for k in 0..bins {
            mag[k] = (left[k].norm() + right_spectrum[k].norm()).sqrt();
            phase_l[k] = left[k].phase();
            phase_r[k] = right_spectrum[k].phase();
            positive_flux += (mag[k] - old_mag[k]).max(0.);
            total += mag[k];
            maximum = maximum.max(mag[k]);
        }
        let attack = positive_flux > 0.35 * total.max(1e-12);
        peaks.clear();
        peaks.push(0);
        for k in 1..bins - 1 {
            if mag[k] > mag[k - 1] && mag[k] >= mag[k + 1] {
                peaks.push(k);
            }
        }
        peaks.push(bins - 1);
        let source_hop = (source_center - old_center) as f64;
        for &peak in &peaks {
            let ref_left = left[peak].norm() >= right_spectrum[peak].norm();
            let phase = if ref_left {
                phase_l[peak]
            } else {
                phase_r[peak]
            };
            let previous = if ref_left { old_l[peak] } else { old_r[peak] };
            let previous_output = if ref_left {
                output_phase_l[peak]
            } else {
                output_phase_r[peak]
            };
            let omega = TAU * peak as f64 / n as f64;
            let frequency = if source_hop > 0. {
                omega + wrap(phase - previous - omega * source_hop) / source_hop
            } else {
                omega
            };
            let reset = attack && mag[peak] > 2.5 * old_mag[peak] && mag[peak] > maximum * 0.1;
            peak_phase[peak] = if first || reset {
                phase
            } else {
                wrap(previous_output + frequency * hop as f64)
            };
        }
        let mut region = 0;
        for k in 0..bins {
            while region + 1 < peaks.len() && 2 * k > peaks[region] + peaks[region + 1] {
                region += 1;
            }
            let peak = peaks[region];
            let reference = if left[peak].norm() >= right_spectrum[peak].norm() {
                phase_l[peak]
            } else {
                phase_r[peak]
            };
            output_phase_l[k] = wrap(peak_phase[peak] + phase_l[k] - reference);
            output_phase_r[k] = wrap(peak_phase[peak] + phase_r[k] - reference);
            left[k] = C::polar(left[k].mag(), output_phase_l[k]);
            right_spectrum[k] = C::polar(right_spectrum[k].mag(), output_phase_r[k]);
            if k == 0 || k == n / 2 {
                left[k].im = 0.;
                right_spectrum[k].im = 0.;
            } else {
                left[n - k] = left[k].conj();
                right_spectrum[n - k] = right_spectrum[k].conj();
            }
        }
        old_mag.copy_from_slice(&mag);
        old_l.copy_from_slice(&phase_l);
        old_r.copy_from_slice(&phase_r);
        old_center = source_center;
        first = false;
        fft.transform(&mut left, true);
        fft.transform(&mut right_spectrum, true);
        for i in 0..n {
            let destination = center + i as i64 - n as i64 / 2;
            if destination < 0 || destination as usize >= intermediate {
                continue;
            }
            let at = destination as usize;
            out_l[at] += (left[i].re * window[i]) as f32;
            out_r[at] += (right_spectrum[i].re * window[i]) as f32;
            weights[at] += (window[i] * window[i]) as f32;
        }
        center += hop as i64;
    }
    for i in 0..intermediate {
        let divisor = weights[i].max(1e-9);
        out_l[i] /= divisor;
        out_r[i] /= divisor;
    }
    if pitch == 1. {
        output.left = out_l;
        output.right = out_r;
    } else {
        let radius = 32i64;
        let cutoff = 0.94 * (1. / pitch).min(1.);
        for i in 0..count {
            if i & 4095 == 0 && canceled(cancel) {
                return Ok(None);
            }
            let position = i as f64 * pitch;
            let base = position.floor() as i64;
            let (mut sum_l, mut sum_r, mut sum_weights) = (0., 0., 0.);
            for tap in -radius + 1..=radius {
                let source = base + tap;
                let distance = position - source as f64;
                if distance.abs() >= radius as f64 {
                    continue;
                }
                let x = PI * distance * cutoff;
                let sinc = if x.abs() < 1e-12 { 1. } else { x.sin() / x };
                let window = 0.42
                    + 0.5 * (PI * distance / radius as f64).cos()
                    + 0.08 * (TAU * distance / radius as f64).cos();
                let weight = cutoff * sinc * window;
                sum_l += sample(&out_l, source) * weight;
                sum_r += sample(&out_r, source) * weight;
                sum_weights += weight;
            }
            output.left[i] = (sum_l / sum_weights.max(1e-12)) as f32;
            output.right[i] = (sum_r / sum_weights.max(1e-12)) as f32;
        }
    }
    if output
        .left
        .iter()
        .chain(&output.right)
        .any(|v| !v.is_finite())
    {
        return Err("Non-finite general warp output".into());
    }
    Ok(Some(output))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn tone() -> Vec<f32> {
        (0..24000)
            .map(|i| (TAU * 440. * i as f64 / 48000.).sin() as f32 * 0.4)
            .collect()
    }
    #[test]
    fn identity_preserves_sample_bits_and_mono_duplicate() {
        let pcm = tone();
        let a = [
            Anchor {
                destination: 0.,
                source: 100.,
            },
            Anchor {
                destination: 20000.,
                source: 20100.,
            },
        ];
        let o = render(
            Input {
                left: &pcm,
                right: &[],
                anchors: &a,
                frames: 20000,
                sample_rate: 48000.,
                pitch: 1.,
            },
            None,
        )
        .unwrap()
        .unwrap();
        assert_eq!(o.left, pcm[100..20100]);
        assert_eq!(o.left, o.right);
    }
    #[test]
    fn stretch_preserves_pitch_and_stereo_opposition() {
        let pcm = tone();
        let r: Vec<f32> = pcm.iter().map(|x| -x).collect();
        let a = [
            Anchor {
                destination: 0.,
                source: 0.,
            },
            Anchor {
                destination: 36000.,
                source: 24000.,
            },
        ];
        let o = render(
            Input {
                left: &pcm,
                right: &r,
                anchors: &a,
                frames: 36000,
                sample_rate: 48000.,
                pitch: 1.,
            },
            None,
        )
        .unwrap()
        .unwrap();
        let crossings = o.left[6000..30000]
            .windows(2)
            .filter(|v| v[0] <= 0. && v[1] > 0.)
            .count();
        assert!((crossings as i32 - 220).abs() <= 2);
        assert!(
            o.left
                .iter()
                .zip(&o.right)
                .all(|(l, r)| (l + r).abs() < 1e-5)
        );
    }
    #[test]
    fn invalid_ratio_rejected_and_cancel_is_not_partial_audio() {
        let pcm = tone();
        let a = [
            Anchor {
                destination: 0.,
                source: 0.,
            },
            Anchor {
                destination: 1000.,
                source: 24000.,
            },
        ];
        assert!(
            render(
                Input {
                    left: &pcm,
                    right: &[],
                    anchors: &a,
                    frames: 1000,
                    sample_rate: 48000.,
                    pitch: 1.
                },
                None
            )
            .is_err()
        );
        let a = [
            Anchor {
                destination: 0.,
                source: 0.,
            },
            Anchor {
                destination: 24000.,
                source: 24000.,
            },
        ];
        let cancel = AtomicBool::new(true);
        assert!(
            render(
                Input {
                    left: &pcm,
                    right: &[],
                    anchors: &a,
                    frames: 24000,
                    sample_rate: 48000.,
                    pitch: 1.
                },
                Some(&cancel)
            )
            .unwrap()
            .is_none()
        );
    }
}
