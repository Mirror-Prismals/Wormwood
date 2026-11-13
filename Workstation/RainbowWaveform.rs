//! Bounded source-space preview analysis on the document control thread.
use super::playback::Source;
use serde_json::{Value, json};

pub(super) fn analyze(source: &Source, offset: f64, duration: f64) -> Value {
    let frames = source.samples.len() / source.channels;
    let mut columns = Vec::with_capacity(128);
    for column in 0..128 {
        let start = ((offset + duration * column as f64 / 128.) * source.sample_rate as f64) as usize;
        let end = ((offset + duration * (column + 1) as f64 / 128.) * source.sample_rate as f64) as usize;
        let mut peaks = [0_f32; 2];
        for frame in start.min(frames)..end.min(frames) {
            for channel in 0..2 {
                peaks[channel] = peaks[channel].max(source.samples[frame * source.channels + channel.min(source.channels - 1)].abs());
            }
        }
        // Windowed frequency probes preserve several simultaneous color families.
        let mut bands = [0_f64; 6];
        for (band, frequency) in [110., 330., 990., 2970., 6500., 12000.].into_iter().enumerate() {
            let mut real = 0.;
            let mut imaginary = 0.;
            for n in 0..256 {
                let frame = start.saturating_add(n);
                if frame >= frames || frequency >= source.sample_rate as f64 * 0.5 { break; }
                let sample = (0..source.channels).map(|c| source.samples[frame * source.channels + c] as f64).sum::<f64>() / source.channels as f64;
                let window = 0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / 255.).cos();
                let phase = std::f64::consts::TAU * frequency * n as f64 / source.sample_rate as f64;
                real += sample * window * phase.cos();
                imaginary += sample * window * phase.sin();
            }
            bands[band] = (real * real + imaginary * imaginary).sqrt();
        }
        let total = bands.iter().sum::<f64>();
        let centroid = bands.iter().enumerate().map(|(i, energy)| i as f64 * energy).sum::<f64>() / total.max(1e-12) / 5.;
        let concentration = bands.iter().copied().fold(0_f64, f64::max) / total.max(1e-12);
        columns.push(json!([peaks[0], peaks[1], centroid, concentration]));
    }
    json!(columns)
}
