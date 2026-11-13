//! End-to-end dry monitor regressions: document samples through the same
//! four-bus queue and output fold used by the native callback. These tests do
//! not open a hardware device or claim third-party plugin transparency.
use super::document::{State, dispatch};
use crate::audio::transport;
use serde_json::{Value, json};

fn edit(state: &mut State, hook: &str, mut input: Value) {
    input["document_revision"] = json!(state.revision);
    dispatch(state, hook, &input).unwrap();
}

fn stereo_track() -> State {
    let mut state = State::default();
    edit(&mut state, "PutTrack", json!({"id":"signal","name":"Signal","kind":"audio"}));
    // Explicit stereo routing is deliberate: the existing authored FF/FF
    // default is a mono fold and is a separate product decision.
    edit(&mut state, "SetTrackMix", json!({"id":"signal","output_bus_l":"L","output_bus_r":"R"}));
    state
}

fn source(state: &mut State, rate: u32, samples: &[[f32; 2]]) {
    edit(state, "PutSource", json!({"id":"pcm","sample_rate":rate,"channels":2,
        "samples":samples.iter().flatten().copied().collect::<Vec<_>>()}));
    edit(state, "PutClip", json!({"id":"clip","track":"signal","kind":"audio","source":"pcm",
        "start_seconds":0.,"source_offset_seconds":0.,"duration_seconds":samples.len() as f64 / rate as f64}));
}

fn render(state: &mut State, rate: u32, start: usize, frames: usize, live: Option<&[[f32; 2]]>) -> Vec<[f32; 4]> {
    let mut request = json!({"document_revision":state.revision,"sample_rate":rate,
        "start_frame":start,"frames":frames});
    if let Some(samples) = live {
        request["monitor_only"] = json!(true);
        request["live_inputs"] = json!({"physical":[samples.iter().map(|s|s[0]).collect::<Vec<_>>(),
            samples.iter().map(|s|s[1]).collect::<Vec<_>>()],"virtual_mic":null});
    }
    let rendered = dispatch(state, "RenderRange", &request).unwrap();
    serde_json::from_value(rendered["lsffr"].clone()).unwrap()
}

fn partition(total: usize, sizes: &[usize], mut block: impl FnMut(usize, usize)) {
    let mut start = 0;
    let mut index = 0;
    while start < total {
        let count = sizes[index % sizes.len()].min(total - start);
        block(start, count);
        start += count;
        index += 1;
    }
}

fn through_native_output(state: &mut State, rate: u32, samples: &[[f32; 2]], live: bool,
    render_sizes: &[usize], callback_sizes: &[usize]) -> Vec<[f32; 2]> {
    let (mut producer, mut output) = transport::prepare(rate).unwrap();
    producer.set_world(false);
    producer.set_playing(!live);
    producer.set_monitoring(live);
    partition(samples.len(), render_sizes, |start, frames| {
        let buses = render(state, rate, if live {0} else {start}, frames,
            live.then_some(&samples[start..start + frames]));
        // Stereo routes must not leak into the sub/front-fill buses.
        assert!(buses.iter().all(|frame| frame[1] == 0. && frame[2] == 0.));
        producer.enqueue_live(&buses, !live).unwrap();
    });
    let mut heard = Vec::with_capacity(samples.len());
    partition(samples.len(), callback_sizes, |_, frames| {
        output.begin_block();
        for _ in 0..frames {
            heard.push(output.frame());
        }
        output.end_block(frames);
    });
    let status = producer.snapshot();
    assert_eq!(status["underruns"], 0);
    assert_eq!(status["queued_frames"], 0);
    assert_eq!(producer.position(), if live {0} else {samples.len() as u64});
    heard
}

fn wideband(frames: usize) -> Vec<[f32; 2]> {
    let mut seed = 0x23a8_175bu32;
    (0..frames).map(|_| std::array::from_fn(|_| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        // Exact binary fractions, with independent channel content and ample
        // floating-point headroom. No trigonometric/tolerance oracle needed.
        ((seed & 0x7fff) as i32 - 16384) as f32 / 2048.
    })).collect()
}

#[test]
fn dry_stereo_impulses_preserve_channel_polarity_timing_and_headroom() {
    let mut samples = vec![[0.; 2]; 1025];
    for (at, value) in [(0, [2., 0.]), (1, [0., -3.]), (255, [-4., 0.]),
        (256, [0., 5.]), (511, [1.25, -6.5]), (1024, [7., -8.])] {
        samples[at] = value;
    }
    let mut state = stereo_track();
    source(&mut state, 48000, &samples);
    let heard = through_native_output(&mut state, 48000, &samples, false,
        &[1, 127, 385, 256], &[256, 31, 1, 509]);
    assert_eq!(heard, samples, "dry path must not clip, filter, crossfeed or move impulses");
}

#[test]
fn dry_stereo_wideband_samples_are_identical_across_rates_and_block_partitions() {
    let samples = wideband(8191);
    for rate in [44100, 48000, 96000] {
        let mut state = stereo_track();
        source(&mut state, rate, &samples);
        let regular = through_native_output(&mut state, rate, &samples, false, &[256], &[256]);
        let irregular = through_native_output(&mut state, rate, &samples, false,
            &[1, 511, 64, 4096, 3, 1536], &[17, 256, 1, 509, 64]);
        assert_eq!(regular, samples, "dry PCM changed at {rate} Hz");
        assert_eq!(irregular, samples, "block partition changed PCM at {rate} Hz");
    }
}

#[test]
fn stopped_live_stereo_monitor_preserves_opposing_phase_and_unity_gain() {
    let samples: Vec<_> = wideband(4093).into_iter().map(|frame| [frame[0], -frame[0]]).collect();
    let mut state = stereo_track();
    edit(&mut state, "SetTrackMix", json!({"id":"signal","input_monitor_mode":2,"stereo_input_pair_12":true}));
    let heard = through_native_output(&mut state, 48000, &samples, true,
        &[63, 1024, 1, 257], &[256, 127, 1]);
    assert_eq!(heard, samples, "live stereo must preserve phase instead of summing to mono");
}
