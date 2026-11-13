//! Control-thread port of Salamander137 DawSpectralEditorSystem.cpp.
//! Source PCM is immutable; ordered masks rebuild a derived cache. Neither FFT,
//! allocation nor preview generation may run in a device callback.
use std::f32::consts::PI;

const FFT_SIZE: usize = 2048;
const HOP: usize = FFT_SIZE / 4;
pub(crate) const MAX_MASKS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Mask {
    pub start: u64,
    pub end: u64,
    pub low_hz: f32,
    pub high_hz: f32,
    pub attenuation_db: f32,
    pub feather: f32,
}
impl Mask {
    pub(crate) fn validated(mut self) -> Result<Self, String> {
        if [self.low_hz, self.high_hz, self.attenuation_db, self.feather]
            .iter()
            .any(|v| !v.is_finite())
        {
            return Err("spectral mask contains a non-finite value".into());
        }
        if self.end < self.start {
            std::mem::swap(&mut self.start, &mut self.end);
        }
        self.low_hz = self.low_hz.max(0.);
        self.high_hz = self.high_hz.max(self.low_hz);
        self.attenuation_db = self.attenuation_db.clamp(-96., 0.);
        self.feather = self.feather.clamp(0., 0.45);
        if self.end <= self.start || self.high_hz <= self.low_hz {
            return Err("spectral mask needs positive time and frequency extents".into());
        }
        Ok(self)
    }
}
#[derive(Clone, Default, Debug)]
pub(crate) struct Edits {
    masks: Vec<Mask>,
    revision: u64,
}
impl Edits {
    pub(crate) fn masks(&self) -> &[Mask] {
        &self.masks
    }
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
    pub(crate) fn add(&mut self, mask: Mask) -> Result<(), String> {
        let mask = mask.validated()?;
        if self.masks.len() >= MAX_MASKS {
            return Err("spectral mask capacity reached".into());
        }
        self.masks.push(mask);
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }
    pub(crate) fn undo(&mut self) -> bool {
        if self.masks.pop().is_none() {
            return false;
        }
        self.revision = self.revision.wrapping_add(1);
        true
    }
    pub(crate) fn clear(&mut self) -> bool {
        if self.masks.is_empty() {
            return false;
        }
        self.masks.clear();
        self.revision = self.revision.wrapping_add(1);
        true
    }
    /// Rebuild from the original source every time, never from the previous cache.
    pub(crate) fn render(
        &self,
        channels: &[Vec<f32>],
        sample_rate: u32,
    ) -> Result<Vec<Vec<f32>>, String> {
        validate_audio(channels, sample_rate)?;
        let mut result = channels.to_vec();
        for mask in &self.masks {
            for channel in &mut result {
                apply(channel, *mask, sample_rate as f32);
            }
        }
        Ok(result)
    }
}
fn validate_audio(channels: &[Vec<f32>], rate: u32) -> Result<(), String> {
    if !(8000..=192000).contains(&rate)
        || !(1..=2).contains(&channels.len())
        || channels
            .iter()
            .any(|c| c.len() != channels[0].len() || c.iter().any(|x| !x.is_finite()))
    {
        return Err(
            "spectral audio needs one or two equal-length finite channels at 8–192 kHz".into(),
        );
    }
    Ok(())
}
#[derive(Clone, Copy, Default)]
struct Complex {
    re: f32,
    im: f32,
}
impl Complex {
    fn times(self, other: Self) -> Self {
        Self {
            re: self.re * other.re - self.im * other.im,
            im: self.re * other.im + self.im * other.re,
        }
    }
    fn scaled(self, scale: f32) -> Self {
        Self {
            re: self.re * scale,
            im: self.im * scale,
        }
    }
    fn magnitude(self) -> f32 {
        self.re.hypot(self.im)
    }
}
fn fft(a: &mut [Complex], inverse: bool) {
    let n = a.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            a.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = (if inverse { 2. } else { -2. }) * PI / len as f32;
        let root = Complex {
            re: angle.cos(),
            im: angle.sin(),
        };
        for start in (0..n).step_by(len) {
            let mut w = Complex { re: 1., im: 0. };
            for j in 0..len / 2 {
                let u = a[start + j];
                let v = a[start + j + len / 2].times(w);
                a[start + j] = Complex {
                    re: u.re + v.re,
                    im: u.im + v.im,
                };
                a[start + j + len / 2] = Complex {
                    re: u.re - v.re,
                    im: u.im - v.im,
                };
                w = w.times(root);
            }
        }
        len <<= 1;
    }
    if inverse {
        for v in a {
            *v = v.scaled(1. / n as f32);
        }
    }
}
fn window() -> Vec<f32> {
    (0..FFT_SIZE)
        .map(|i| 0.5 - 0.5 * (2. * PI * i as f32 / (FFT_SIZE - 1) as f32).cos())
        .collect()
}
fn smooth_edge(x: f32, lo: f32, hi: f32, feather: f32) -> f32 {
    if hi <= lo || x < lo || x > hi {
        return 0.;
    }
    let edge = ((hi - lo) * feather.clamp(0., 0.45)).max(1e-6);
    let t = if x < lo + edge {
        (x - lo) / edge
    } else if x > hi - edge {
        (hi - x) / edge
    } else {
        return 1.;
    }
    .clamp(0., 1.);
    t * t * (3. - 2. * t)
}
fn apply(channel: &mut [f32], mask: Mask, rate: f32) {
    let n = channel.len();
    let start = mask.start.min(n as u64) as usize;
    let end = mask.end.min(n as u64) as usize;
    if end <= start {
        return;
    }
    let region_start = start.saturating_sub(FFT_SIZE);
    let region_end = end.saturating_add(FFT_SIZE).min(n);
    let mut accumulation = vec![0.; region_end - region_start];
    let mut normalization = vec![0.; accumulation.len()];
    let mut spectrum = vec![Complex::default(); FFT_SIZE];
    let window = window();
    let target = 10f32.powf(mask.attenuation_db.clamp(-96., 0.) / 20.);
    let low = mask.low_hz.min(mask.high_hz).clamp(0., rate * 0.5);
    let high = mask.low_hz.max(mask.high_hz).clamp(low, rate * 0.5);
    let frame0 = ((region_start / HOP) * HOP).saturating_sub(FFT_SIZE);
    for frame in (frame0..region_end).step_by(HOP) {
        let time_weight = smooth_edge(
            (frame + FFT_SIZE / 2) as f32,
            start as f32,
            end as f32,
            mask.feather,
        );
        if time_weight <= 0. && frame + FFT_SIZE < start {
            continue;
        }
        for i in 0..FFT_SIZE {
            spectrum[i] = Complex {
                re: channel.get(frame + i).copied().unwrap_or(0.) * window[i],
                im: 0.,
            };
        }
        fft(&mut spectrum, false);
        if time_weight > 0. {
            for bin in 0..=FFT_SIZE / 2 {
                let weight = time_weight
                    * smooth_edge(bin as f32 * rate / FFT_SIZE as f32, low, high, mask.feather);
                let gain = 1. + (target - 1.) * weight;
                spectrum[bin] = spectrum[bin].scaled(gain);
                if bin > 0 && bin < FFT_SIZE / 2 {
                    spectrum[FFT_SIZE - bin] = spectrum[FFT_SIZE - bin].scaled(gain);
                }
            }
        }
        fft(&mut spectrum, true);
        for i in 0..FFT_SIZE {
            let dst = frame + i;
            if dst >= region_start && dst < region_end {
                accumulation[dst - region_start] += spectrum[i].re * window[i];
                normalization[dst - region_start] += window[i] * window[i];
            }
        }
    }
    for i in 0..accumulation.len() {
        if normalization[i] > 1e-6 {
            channel[region_start + i] = accumulation[i] / normalization[i];
        }
    }
}
/// Cardinal uses a 30 Hz logarithmic scale, with a special DC row.
pub(crate) fn norm_to_hz(norm: f32, sample_rate: u32) -> f32 {
    let norm = norm.clamp(0., 1.);
    let nyquist = (sample_rate as f32 * 0.5).max(1.);
    if norm <= 0.001 {
        return 0.;
    }
    let floor = 30f32.min(nyquist);
    if nyquist <= floor {
        nyquist * norm
    } else {
        floor * (nyquist / floor).powf(norm)
    }
}
pub(crate) fn hz_to_norm(hz: f32, sample_rate: u32) -> f32 {
    if hz <= 0. {
        return 0.;
    }
    let nyquist = (sample_rate as f32 * 0.5).max(1.);
    let floor = 30f32.min(nyquist);
    if nyquist <= floor {
        (hz / nyquist).clamp(0., 1.)
    } else {
        ((hz.max(floor) / floor).ln() / (nyquist / floor).ln()).clamp(0., 1.)
    }
}
#[derive(Debug)]
pub(crate) struct Spectrogram {
    pub columns: usize,
    pub bins: usize,
    pub intensity: Vec<f32>,
}
/// Analyze the processed cache so the image reflects what the audio renderer hears.
pub(crate) fn spectrogram(
    channels: &[Vec<f32>],
    rate: u32,
    start: usize,
    length: usize,
    columns: usize,
    bins: usize,
) -> Result<Spectrogram, String> {
    validate_audio(channels, rate)?;
    let columns = columns.clamp(64, 512);
    let bins = bins.clamp(32, 160);
    let length = length.min(channels[0].len().saturating_sub(start));
    if length == 0 {
        return Err("spectrogram source range is empty".into());
    }
    let window = window();
    let amplitude_scale = 2. / window.iter().sum::<f32>();
    let mut spectrum = vec![Complex::default(); FFT_SIZE];
    let mut intensity = vec![0.; columns * bins];
    for x in 0..columns {
        let center = start + ((x as f64 + 0.5) * length as f64 / columns as f64) as usize;
        let frame0 = center as i64 - (FFT_SIZE / 2) as i64;
        for i in 0..FFT_SIZE {
            let index = frame0 + i as i64;
            let sample = if index < 0 || index as usize >= channels[0].len() {
                0.
            } else {
                channels.iter().map(|c| c[index as usize]).sum::<f32>() / channels.len() as f32
            };
            spectrum[i] = Complex {
                re: sample * window[i],
                im: 0.,
            };
        }
        fft(&mut spectrum, false);
        for y in 0..bins {
            let hz = norm_to_hz(y as f32 / (bins - 1) as f32, rate);
            let position = (hz * FFT_SIZE as f32 / rate as f32).clamp(0., (FFT_SIZE / 2) as f32);
            let a = position.floor() as usize;
            let b = (a + 1).min(FFT_SIZE / 2);
            let blend = position - a as f32;
            let mag = (spectrum[a].magnitude() * (1. - blend) + spectrum[b].magnitude() * blend)
                * amplitude_scale;
            let db = 20. * mag.max(1e-7).log10();
            intensity[y * columns + x] = ((db + 100.) / 94.).clamp(0., 1.);
        }
    }
    Ok(Spectrogram {
        columns,
        bins,
        intensity,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn tone(hz: f32) -> Vec<f32> {
        (0..24000)
            .map(|i| (2. * PI * hz * i as f32 / 48000.).sin() * 0.5)
            .collect()
    }
    fn energy(v: &[f32]) -> f32 {
        v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32
    }
    fn mask() -> Mask {
        Mask {
            start: 4000,
            end: 20000,
            low_hz: 500.,
            high_hz: 1500.,
            attenuation_db: -48.,
            feather: 0.12,
        }
    }
    #[test]
    fn repair_attenuates_selected_band_and_keeps_unselected_tone() {
        let mut edits = Edits::default();
        edits.add(mask()).unwrap();
        let a = tone(1000.);
        let b = tone(6000.);
        let out = edits.render(&[a.clone(), b.clone()], 48000).unwrap();
        assert!(energy(&out[0][8000..16000]) < energy(&a[8000..16000]) * 0.001);
        assert!((energy(&out[1][8000..16000]) / energy(&b[8000..16000]) - 1.).abs() < 0.002);
        assert_eq!(&out[0][..1000], &a[..1000]);
        assert_eq!(&out[0][23000..], &a[23000..]);
    }
    #[test]
    fn ordered_masks_undo_clear_rebuild_from_source() {
        let input = vec![tone(1000.)];
        let mut edits = Edits::default();
        edits.add(mask()).unwrap();
        let once = edits.render(&input, 48000).unwrap();
        edits.add(mask()).unwrap();
        let twice = edits.render(&input, 48000).unwrap();
        assert!(energy(&twice[0][8000..16000]) < energy(&once[0][8000..16000]));
        assert!(edits.undo());
        assert_eq!(edits.render(&input, 48000).unwrap(), once);
        assert!(edits.clear());
        assert_eq!(edits.render(&input, 48000).unwrap(), input);
        assert!(!edits.undo());
    }
    #[test]
    fn rejected_mask_is_atomic() {
        let mut e = Edits::default();
        let mut m = mask();
        m.feather = f32::NAN;
        assert!(e.add(m).is_err());
        assert_eq!(e.revision(), 0);
        assert!(e.masks().is_empty());
    }
    #[test]
    fn logarithmic_frequency_roundtrips() {
        assert_eq!(norm_to_hz(0., 48000), 0.);
        for x in [0.1, 0.3, 0.6, 0.9, 1.] {
            assert!((hz_to_norm(norm_to_hz(x, 48000), 48000) - x).abs() < 1e-5);
        }
    }
    #[test]
    fn preview_tracks_repair_and_bounds_dimensions() {
        let pcm = vec![tone(1000.)];
        let mut e = Edits::default();
        e.add(mask()).unwrap();
        let before = spectrogram(&pcm, 48000, 8000, 8000, 0, 9999).unwrap();
        let after =
            spectrogram(&e.render(&pcm, 48000).unwrap(), 48000, 8000, 8000, 64, 160).unwrap();
        assert_eq!((before.columns, before.bins), (64, 160));
        let row = (hz_to_norm(1000., 48000) * 159.).round() as usize;
        let k = row * 64 + 32;
        assert!(after.intensity[k] < before.intensity[k] - 0.35);
    }
}
