//! Bounded, worker-side rhythm observations of the post-mix master bus.
//!
//! The spectral-flux onset calculation adapts Cardinal's DawWarpSystem.cpp.
//! Its whole-source BPM, source clips, warp maps, and global musical clock are
//! deliberately absent. Only consecutive, locally regular observed pulses form
//! a region. A pulse rate does not establish a musical beat level or a meter.
//!
//! This prototype does not bridge missed onsets, resolve subdivisions, infer
//! downbeats, reconcile successive reports, or establish detection accuracy.
//! A rolling 8 kHz window follows contiguous native master frames. Its weighted
//! box resampling treats each native sample as constant over its frame. This is
//! an onset-analysis approximation with limited anti-alias rejection, not an
//! audio playback resampler. Its spectrum loses content above 4 kHz and can alias.
//! Report publication/revision belongs to the caller. FFT-window boundaries can
//! miss nearby attacks. Gathering reports must not replace published regions.
//! This operation allocates and must never run on an audio callback.

use serde_json::{Value, json};
use std::collections::VecDeque;
use std::f64::consts::PI;

const MAX_SAMPLES: usize = 393_216;
// Keep integer frame positions exactly representable when projecting to f64.
const MAX_WINDOW_END_SECONDS: u64 = 86_400;
const ANALYSIS_RATE: u64 = 8_000;
const MIN_ANALYSIS_FRAMES: usize = 10 * ANALYSIS_RATE as usize;
const MAX_ANALYSIS_FRAMES: usize = 20 * ANALYSIS_RATE as usize;
const FFT_SIZE: usize = 256;
const MIN_PULSES: usize = 5;
const INTERVAL_TOLERANCE: f64 = 0.12;
// These bound the observed pulse rate, not a project tempo or MIDI clip clock.
const MIN_INTERVAL_SECONDS: f64 = 0.2;
const MAX_INTERVAL_SECONDS: f64 = 2.0;

#[derive(Clone, Copy, Default)]
struct Complex {
    real: f64,
    imaginary: f64,
}

impl Complex {
    fn times(self, other: Self) -> Self {
        Self {
            real: self.real * other.real - self.imaginary * other.imaginary,
            imaginary: self.real * other.imaginary + self.imaginary * other.real,
        }
    }

    fn magnitude(self) -> f64 {
        self.real.hypot(self.imaginary)
    }
}

fn fft(values: &mut [Complex]) {
    let count = values.len();
    let mut reversed = 0;
    for index in 1..count {
        let mut bit = count >> 1;
        while reversed & bit != 0 {
            reversed ^= bit;
            bit >>= 1;
        }
        reversed ^= bit;
        if index < reversed {
            values.swap(index, reversed);
        }
    }
    let mut width = 2;
    while width <= count {
        let angle = -2.0 * PI / width as f64;
        let root = Complex {
            real: angle.cos(),
            imaginary: angle.sin(),
        };
        for start in (0..count).step_by(width) {
            let mut phase = Complex {
                real: 1.0,
                imaginary: 0.0,
            };
            for index in 0..width / 2 {
                let even = values[start + index];
                let odd = values[start + index + width / 2].times(phase);
                values[start + index] = Complex {
                    real: even.real + odd.real,
                    imaginary: even.imaginary + odd.imaginary,
                };
                values[start + index + width / 2] = Complex {
                    real: even.real - odd.real,
                    imaginary: even.imaginary - odd.imaginary,
                };
                phase = phase.times(root);
            }
        }
        width <<= 1;
    }
}

#[derive(Clone, Copy)]
struct Onset {
    frame: usize,
    flux: f64,
}

fn detect_onsets(
    samples: &VecDeque<f64>,
    channels: usize,
    sample_rate: u64,
    fft_size: usize,
) -> Vec<Onset> {
    let sample_frames = samples.len() / channels;
    if sample_frames < fft_size {
        return Vec::new();
    }
    let hop = fft_size / 4;
    let frame_count = 1 + (sample_frames - fft_size) / hop;
    let window: Vec<f64> = (0..fft_size)
        .map(|index| 0.5 - 0.5 * (2.0 * PI * index as f64 / (fft_size - 1) as f64).cos())
        .collect();
    let mut spectrum = vec![Complex::default(); fft_size];
    let bins = fft_size / 2 + 1;
    let mut previous = vec![0.0; bins * channels];
    let mut flux = vec![0.0; frame_count];
    let mut energy = vec![0.0; frame_count];
    for frame in 0..frame_count {
        let offset = frame * hop;
        let mut sum_squares = 0.0;
        let mut frame_flux = 0.0;
        for channel in 0..channels {
            for index in 0..fft_size {
                let sample = samples[(offset + index) * channels + channel];
                spectrum[index] = Complex {
                    real: sample * window[index],
                    imaginary: 0.0,
                };
                sum_squares += sample * sample;
            }
            fft(&mut spectrum);
            for bin in 1..=fft_size / 2 {
                let magnitude = spectrum[bin].magnitude();
                let previous_bin = channel * bins + bin;
                let increase = (magnitude - previous[previous_bin]).max(0.0);
                // Sum channel onset energy instead of averaging PCM, so
                // opposite-phase stereo material does not cancel the detector.
                frame_flux += increase * (0.5 + bin as f64 / fft_size as f64);
                previous[previous_bin] = magnitude;
            }
        }
        flux[frame] = frame_flux;
        energy[frame] = (sum_squares / (fft_size * channels) as f64).sqrt();
    }

    let radius = 8;
    let minimum_gap = ((0.045 * sample_rate as f64 / hop as f64).ceil() as usize).max(1);
    let mut last_accepted: Option<usize> = None;
    let mut onsets: Vec<Onset> = Vec::new();
    for frame in 1..frame_count.saturating_sub(1) {
        let begin = frame.saturating_sub(radius);
        let end = (frame + radius + 1).min(frame_count);
        let neighborhood = &flux[begin..end];
        let mean = neighborhood.iter().sum::<f64>() / neighborhood.len() as f64;
        let variance = neighborhood
            .iter()
            .map(|value| (value - mean).powi(2))
            .sum::<f64>()
            / neighborhood.len() as f64;
        let threshold = mean + 1.35 * variance.sqrt();
        if flux[frame] <= threshold
            || flux[frame] < flux[frame - 1]
            || flux[frame] <= flux[frame + 1]
            || energy[frame] <= 1e-5
        {
            continue;
        }
        if last_accepted.is_some_and(|last| frame - last < minimum_gap) {
            if onsets.last().is_some_and(|last| flux[frame] > last.flux) {
                onsets.pop();
            } else {
                continue;
            }
        }
        // The centre of the analysis window is an approximate attack location,
        // quantized to a hop. It is not a sample-exact transient boundary.
        onsets.push(Onset {
            frame: frame * hop + fft_size / 2,
            flux: flux[frame],
        });
        last_accepted = Some(frame);
    }
    onsets
}

fn steady_regions(onsets: &[Onset], sample_rate: u64, origin_seconds: f64) -> Vec<Value> {
    let intervals: Vec<f64> = onsets
        .windows(2)
        .map(|pair| (pair[1].frame - pair[0].frame) as f64 / sample_rate as f64)
        .collect();
    let absolute_seconds =
        |index: usize| origin_seconds + onsets[index].frame as f64 / sample_rate as f64;
    let regular = |minimum: f64, maximum: f64, mean: f64| {
        minimum >= MIN_INTERVAL_SECONDS
            && maximum <= MAX_INTERVAL_SECONDS
            && minimum >= mean * (1.0 - INTERVAL_TOLERANCE)
            && maximum <= mean * (1.0 + INTERVAL_TOLERANCE)
    };
    let mut regions = Vec::new();
    let mut begin = 0;
    while begin + MIN_PULSES <= onsets.len() {
        let mut end = begin + MIN_PULSES - 1;
        let seed = &intervals[begin..end];
        let mut sum = seed.iter().sum::<f64>();
        let mut minimum = seed.iter().copied().fold(f64::INFINITY, f64::min);
        let mut maximum = seed.iter().copied().fold(0.0, f64::max);
        if !regular(minimum, maximum, sum / seed.len() as f64) {
            begin += 1;
            continue;
        }
        while end < intervals.len() {
            let candidate_sum = sum + intervals[end];
            let candidate_minimum = minimum.min(intervals[end]);
            let candidate_maximum = maximum.max(intervals[end]);
            let candidate_mean = candidate_sum / (end - begin + 1) as f64;
            if !regular(candidate_minimum, candidate_maximum, candidate_mean) {
                break;
            }
            sum = candidate_sum;
            minimum = candidate_minimum;
            maximum = candidate_maximum;
            end += 1;
        }
        let period = sum / (end - begin) as f64;
        let variance = intervals[begin..end]
            .iter()
            .map(|interval| (interval - period).powi(2))
            .sum::<f64>()
            / (end - begin) as f64;
        // This is a normalized interval-regularity score, not calibrated
        // confidence that these attacks establish a musical beat or time meter.
        let regularity = (1.0 - variance.sqrt() / (period * INTERVAL_TOLERANCE)).clamp(0.0, 1.0);
        let beats: Vec<f64> = (begin..=end).map(absolute_seconds).collect();
        regions.push(json!({
            "start_seconds": absolute_seconds(begin),
            "end_seconds": absolute_seconds(end),
            "pulse_bpm": 60.0 / period,
            "beat_seconds": beats,
            "regularity_score": regularity,
            "meter": null
        }));
        // Adjacent regions may share their boundary pulse, but never an interval.
        begin = end;
    }
    regions
}

fn format(input: &Value) -> Result<(u64, usize), String> {
    if !input.is_object() {
        return Err("Master rhythm analysis expects an object".into());
    }
    let sample_rate = input["sample_rate"]
        .as_u64()
        .filter(|rate| (8_000..=192_000).contains(rate))
        .ok_or("Master sample_rate must be an integer from 8000 through 192000")?;
    let channels = input["channels"]
        .as_u64()
        .filter(|count| *count == 1 || *count == 2)
        .ok_or("Master channels must be explicitly 1 or 2")? as usize;
    Ok((sample_rate, channels))
}

/// One master capture generation. The owner validates generation and document
/// revision and may clone this bounded state before an atomic publication.
#[derive(Clone)]
pub(super) struct Analyzer {
    sample_rate: u64,
    channels: usize,
    first_native_frame: Option<u64>,
    next_native_frame: Option<u64>,
    samples: VecDeque<f64>,
    completed_frames: u64,
    partial_weight: u64,
    partial_sum: [f64; 2],
}

impl Analyzer {
    pub(super) fn new(sample_rate: u64, channels: usize) -> Result<Self, String> {
        if !(8_000..=192_000).contains(&sample_rate) || !(1..=2).contains(&channels) {
            return Err("Master analyzer requires 8000..192000 Hz and 1 or 2 channels".into());
        }
        Ok(Self {
            sample_rate,
            channels,
            first_native_frame: None,
            next_native_frame: None,
            samples: VecDeque::with_capacity(MAX_ANALYSIS_FRAMES * channels),
            completed_frames: 0,
            partial_weight: 0,
            partial_sum: [0.0; 2],
        })
    }

    fn integrate_frame(&mut self, native: [f64; 2]) {
        // One native frame spans ANALYSIS_RATE units; one resampled frame spans
        // sample_rate units. Integer overlap weights preserve fractional phase
        // exactly across calls, including rates such as 44100 Hz.
        let mut remaining = ANALYSIS_RATE;
        while remaining != 0 {
            let weight = remaining.min(self.sample_rate - self.partial_weight);
            for (channel, sample) in native.iter().enumerate().take(self.channels) {
                self.partial_sum[channel] += sample * weight as f64;
            }
            self.partial_weight += weight;
            remaining -= weight;
            if self.partial_weight == self.sample_rate {
                // Evict before insertion, keeping resident PCM at <=20 seconds.
                if self.samples.len() == MAX_ANALYSIS_FRAMES * self.channels {
                    for _ in 0..self.channels {
                        self.samples.pop_front();
                    }
                }
                for channel in 0..self.channels {
                    self.samples
                        .push_back(self.partial_sum[channel] / self.sample_rate as f64);
                }
                self.completed_frames += 1;
                self.partial_sum = [0.0; 2];
                self.partial_weight = 0;
            }
        }
    }

    pub(super) fn push(&mut self, input: &Value) -> Result<Value, String> {
        let (sample_rate, channels) = format(input)?;
        if sample_rate != self.sample_rate || channels != self.channels {
            return Err(
                "Master sample_rate and channels must match this analysis generation".into(),
            );
        }
        let max_frame = sample_rate * MAX_WINDOW_END_SECONDS;
        let start_frame = input["start_frame"]
            .as_u64()
            .filter(|frame| *frame <= max_frame)
            .ok_or("Master start_frame must be a nonnegative frame index within 86400 seconds")?;
        if self
            .next_native_frame
            .is_some_and(|next| next != start_frame)
        {
            return Err(
                "Master chunks must be contiguous; begin a new generation after a discontinuity"
                    .into(),
            );
        }
        let source = input["samples"]
            .as_array()
            .filter(|samples| {
                !samples.is_empty()
                    && samples.len() <= MAX_SAMPLES
                    && samples.len() % channels == 0
            })
            .ok_or("Master samples must be nonempty, at most 393216 interleaved samples, and complete frames")?;
        let end_frame = start_frame
            .checked_add((source.len() / channels) as u64)
            .filter(|frame| *frame <= max_frame)
            .ok_or("Master analysis capture must end within 86400 seconds")?;
        // Validate the entire immutable chunk before changing phase, PCM,
        // continuity, or origin. Rejected chunks leave the analyzer untouched.
        for sample in source {
            if !sample
                .as_f64()
                .is_some_and(|value| value.is_finite() && (-64.0..=64.0).contains(&value))
            {
                return Err("Master samples must be finite interleaved PCM in [-64,64]".into());
            }
        }

        let first_native_frame = *self.first_native_frame.get_or_insert(start_frame);
        for frame in source.chunks_exact(channels) {
            let native = [
                frame[0].as_f64().expect("validated master PCM"),
                if channels == 2 {
                    frame[1].as_f64().expect("validated master PCM")
                } else {
                    0.0
                },
            ];
            self.integrate_frame(native);
        }
        self.next_native_frame = Some(end_frame);

        let retained_frames = self.samples.len() / channels;
        let dropped_frames = self.completed_frames - retained_frames as u64;
        let origin_seconds = first_native_frame as f64 / sample_rate as f64;
        let window_start = origin_seconds + dropped_frames as f64 / ANALYSIS_RATE as f64;
        // Excludes the incomplete resampled frame retained in partial_sum.
        let window_end = origin_seconds + self.completed_frames as f64 / ANALYSIS_RATE as f64;
        let ready = retained_frames >= MIN_ANALYSIS_FRAMES;
        let (onset_count, regions) = if ready {
            let onsets = detect_onsets(&self.samples, channels, ANALYSIS_RATE, FFT_SIZE);
            let regions = steady_regions(&onsets, ANALYSIS_RATE, window_start);
            (onsets.len(), regions)
        } else {
            (0, Vec::new())
        };
        Ok(json!({
            "status": if ready { "analyzed" } else { "gathering" },
            "analysis_source": "master_bus",
            "algorithm": "master_streaming_spectral_flux_steady_pulses_v2",
            "chunk_start_frame": start_frame,
            "chunk_end_frame": end_frame,
            "window_start_seconds": window_start,
            "window_end_seconds": window_end,
            "sample_rate": sample_rate,
            "channels": channels,
            "analysis_sample_rate": ANALYSIS_RATE,
            "analysis_frame_count": retained_frames,
            "minimum_window_seconds": 10,
            "onset_count": onset_count,
            "window_frames": FFT_SIZE,
            "hop_frames": FFT_SIZE / 4,
            "regions": regions
        }))
    }
}
