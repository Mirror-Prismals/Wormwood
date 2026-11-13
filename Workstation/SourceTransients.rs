//! Cardinal DawWarpSystem.cpp algorithm1 transient marks, in original source
//! frames. Run once per immutable source revision on a control/worker thread.
//! This is not a tempo map and does not modify source audio or warp markers.
use std::f32::consts::PI;
pub(crate) const ALGORITHM_VERSION: u32 = 1;
const FFT_SIZE: usize = 1024;
const HOP: usize = 256;
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Transient {
    pub source_frame: u64,
    pub spectral_flux: f32,
    pub strength: f32,
    pub confidence: f32,
}
#[derive(Clone, Copy, Default)]
struct Complex {
    re: f32,
    im: f32,
}
impl Complex {
    fn mul(self, b: Self) -> Self {
        Self {
            re: self.re * b.re - self.im * b.im,
            im: self.re * b.im + self.im * b.re,
        }
    }
    fn abs(self) -> f32 {
        self.re.hypot(self.im)
    }
}
fn fft(values: &mut [Complex]) {
    let n = values.len();
    let mut j = 0usize;
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
    let mut len = 2;
    while len <= n {
        let angle = -2f32 * PI / len as f32;
        let root = Complex {
            re: angle.cos(),
            im: angle.sin(),
        };
        for start in (0..n).step_by(len) {
            let mut phase = Complex { re: 1., im: 0. };
            for j in 0..len / 2 {
                let even = values[start + j];
                let odd = values[start + j + len / 2].mul(phase);
                values[start + j] = Complex {
                    re: even.re + odd.re,
                    im: even.im + odd.im,
                };
                values[start + j + len / 2] = Complex {
                    re: even.re - odd.re,
                    im: even.im - odd.im,
                };
                phase = phase.mul(root);
            }
        }
        len *= 2;
    }
}
/// Same1024/256 source analysis as Cardinal. Stereo is averaged before the FFT
/// (including original cancellation for opposite-phase channels). Output order
/// and source_frame addresses are preserved; they are NOT FFT-window centers.
pub(crate) fn detect(
    samples: &[f32],
    channels: usize,
    sample_rate: u32,
) -> Result<Vec<Transient>, String> {
    if !(1..=2).contains(&channels)
        || samples.len() % channels != 0
        || samples.len() > 16_777_216
        || !(8000..=192000).contains(&sample_rate)
        || samples.iter().any(|v| !v.is_finite())
    {
        return Err("transient analysis requires bounded finite mono/stereo PCM at8–192kHz".into());
    }
    let frames = samples.len() / channels;
    if frames < FFT_SIZE {
        return Ok(Vec::new());
    }
    let count = 1 + (frames - FFT_SIZE) / HOP;
    let mut flux = vec![0f32; count];
    let mut energy = vec![0f32; count];
    let mut previous = vec![0f32; FFT_SIZE / 2 + 1];
    let mut spectrum = vec![Complex::default(); FFT_SIZE];
    for frame in 0..count {
        let offset = frame * HOP;
        let mut squares = 0f32;
        for i in 0..FFT_SIZE {
            let at = (offset + i) * channels;
            let left = samples[at];
            let right = if channels > 1 { samples[at + 1] } else { left };
            let mono = 0.5f32 * (left + right);
            let window = 0.5f32 - 0.5f32 * (2f32 * PI * i as f32 / (FFT_SIZE - 1) as f32).cos();
            spectrum[i] = Complex {
                re: mono * window,
                im: 0.,
            };
            squares += mono * mono;
        }
        fft(&mut spectrum);
        let mut sum = 0f32;
        for bin in 1..=FFT_SIZE / 2 {
            let magnitude = spectrum[bin].abs();
            let delta = magnitude - previous[bin];
            if delta > 0. {
                let weight = 0.5f32 + bin as f32 / FFT_SIZE as f32;
                sum += delta * weight;
            }
            previous[bin] = magnitude;
        }
        flux[frame] = sum;
        energy[frame] = (squares / FFT_SIZE as f32).sqrt();
    }
    let radius = 8;
    let gap = ((0.045f64 * sample_rate as f64 / HOP as f64) as usize).max(1);
    let mut last = None;
    let mut marks: Vec<Transient> = Vec::new();
    for frame in 1..count.saturating_sub(1) {
        let begin = frame.saturating_sub(radius);
        let end = count.min(frame + radius + 1);
        let mut mean = 0f32;
        for v in &flux[begin..end] {
            mean += *v;
        }
        mean /= (end - begin).max(1) as f32;
        let mut variance = 0f32;
        for v in &flux[begin..end] {
            let d = *v - mean;
            variance += d * d;
        }
        let deviation = (variance / (end - begin).max(1) as f32).sqrt();
        let threshold = mean + deviation * 1.35f32;
        let peak = flux[frame] > threshold
            && flux[frame] >= flux[frame - 1]
            && flux[frame] > flux[frame + 1]
            && energy[frame] > 1e-5f32;
        if !peak {
            continue;
        }
        if last.is_some_and(|prior| frame - prior < gap) {
            if marks.last().is_some_and(|m| flux[frame] > m.spectral_flux) {
                marks.pop();
            } else {
                continue;
            }
        }
        marks.push(Transient {
            source_frame: (frame * HOP) as u64,
            spectral_flux: flux[frame],
            strength: flux[frame] / threshold.max(1e-9),
            confidence: ((flux[frame] - threshold) / threshold.max(1e-9)).clamp(0., 1.),
        });
        last = Some(frame);
    }
    Ok(marks)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn attacks() -> Vec<f32> {
        let mut x = vec![0.; 65536];
        let mut state = 17u32;
        for start in [4000, 16000, 17024, 32000, 48000] {
            for i in 0..256 {
                state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                x[start + i] =
                    (state as i32 as f32 / 2147483648f32) * if start == 17024 { 1. } else { 0.4 };
            }
        }
        x
    }
    #[test]
    fn silence_short_and_low_energy_have_no_marks() {
        assert!(detect(&[], 1, 48000).unwrap().is_empty());
        assert!(detect(&vec![1.; 1023], 1, 48000).unwrap().is_empty());
        assert!(detect(&vec![0.; 32768], 1, 48000).unwrap().is_empty());
        let quiet: Vec<f32> = attacks().into_iter().map(|x| x * 1e-7).collect();
        assert!(detect(&quiet, 1, 48000).unwrap().is_empty());
    }
    #[test]
    fn source_marks_keep_hop_clock_and_original_minimum_gap() {
        let marks = detect(&attacks(), 1, 48000).unwrap();
        assert!(marks.len() >= 4);
        assert!(marks.iter().all(|m| m.source_frame % 256 == 0
            && m.strength > 1.
            && m.confidence >= 0.
            && m.confidence <= 1.));
        let gap = (0.045f64 * 48000. / 256.) as u64 * 256;
        assert!(
            marks
                .windows(2)
                .all(|m| m[1].source_frame - m[0].source_frame >= gap)
        );
    }
    #[test]
    fn stereo_average_has_original_phase_cancellation() {
        let mono = attacks();
        let same: Vec<f32> = mono.iter().flat_map(|x| [*x, *x]).collect();
        let opposite: Vec<f32> = mono.iter().flat_map(|x| [*x, -*x]).collect();
        assert_eq!(
            detect(&mono, 1, 48000).unwrap(),
            detect(&same, 2, 48000).unwrap()
        );
        assert!(detect(&opposite, 2, 48000).unwrap().is_empty());
    }
    #[test]
    fn malformed_or_nonfinite_input_is_rejected() {
        assert!(detect(&[0., 1., 2.], 2, 48000).is_err());
        assert!(detect(&[f32::NAN; 1024], 1, 48000).is_err());
        assert!(detect(&[0.; 1024], 1, 0).is_err());
    }
}

// Keep one immutable source analysis; unrelated document/clip edits do not
// re-run FFT analysis, and this cache cannot accumulate source allocations.
#[derive(Default)]pub(super) struct Cache{entry:Option<(super::playback::Source,std::sync::Arc<Vec<Transient>>)>}
impl Cache{pub(super) fn marks(&mut self,source:&super::playback::Source)->Result<std::sync::Arc<Vec<Transient>>,String>{
 if let Some((old,marks))=&self.entry{if old.sample_rate==source.sample_rate&&old.channels==source.channels&&std::sync::Arc::ptr_eq(&old.samples,&source.samples){return Ok(marks.clone());}}
 let marks=std::sync::Arc::new(detect(&source.samples,source.channels,source.sample_rate)?);self.entry=Some((source.clone(),marks.clone()));Ok(marks)
}}
#[cfg(test)]mod cache_tests{use super::*;#[test]fn immutable_source_identity_controls_analysis_reuse(){let mut c=Cache::default();let a=super::super::playback::Source{sample_rate:8000,channels:1,samples:vec![0.;2048].into()};let x=c.marks(&a).unwrap();let y=c.marks(&a).unwrap();assert!(std::sync::Arc::ptr_eq(&x,&y));let b=super::super::playback::Source{samples:vec![0.;2048].into(),..a};assert!(!std::sync::Arc::ptr_eq(&x,&c.marks(&b).unwrap()));}}
