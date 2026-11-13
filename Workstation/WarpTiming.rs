//! Bridge to the shared Cardinal MusicalTime model. This file owns no alternate
//! tempo map: every conversion delegates to an immutable Timing::for_clock Map.
use super::{
    document::{Content, State},
    musical_time::{Map, Timing},
    warp_treatment::{self, Clock},
};
use std::sync::Arc;
#[derive(Clone)]
pub(super) struct SharedClock {
    timing: Arc<Timing>,
    zero_seconds: f64,
    map: Map,
}
impl SharedClock {
    pub(super) fn new(timing: Arc<Timing>, rate: u32, zero_seconds: f64) -> Result<Self, String> {
        if !(8000..=192000).contains(&rate)
            || !zero_seconds.is_finite()
            || !(0. ..=86400.).contains(&zero_seconds)
        {
            return Err("invalid shared warp render clock".into());
        }
        let map = timing.for_clock(rate as f64, zero_seconds);
        if !map.bpm.is_finite()
            || !(20. ..=400.).contains(&map.bpm)
            || map.regions.iter().any(|r| !r.valid())
            || map
                .regions
                .windows(2)
                .any(|r| r[0].end() > r[1].start_quarter)
        {
            return Err("invalid authoritative musical map".into());
        }
        Ok(Self {
            timing,
            zero_seconds,
            map,
        })
    }
    fn origin(&self) -> f64 {
        self.map.zero_sample as f64 / self.map.sample_rate
    }
}
impl Clock for SharedClock {
    fn revision(&self) -> u64 {
        self.timing.revision
    }
    fn cache_identity(&self) -> Vec<u64> {
        vec![
            0x4d55534943414c31,
            self.timing.revision,
            self.map.sample_rate.to_bits(),
            self.map.zero_sample as u64,
        ]
    }
    fn for_sample_rate(&self, rate: u32) -> Result<Option<Arc<dyn Clock>>, String> {
        if rate as f64 == self.map.sample_rate {
            Ok(None)
        } else {
            Ok(Some(Arc::new(Self::new(
                self.timing.clone(),
                rate,
                self.zero_seconds,
            )?)))
        }
    }
    fn seconds_to_beat(&self, seconds: f64) -> Result<f64, String> {
        if !seconds.is_finite() {
            return Err("finite warp time required".into());
        }
        let result = self.map.quarter_at_seconds(seconds - self.origin());
        if result.is_finite() {
            Ok(result)
        } else {
            Err("non-finite musical quarter".into())
        }
    }
    fn beat_to_seconds(&self, beat: f64) -> Result<f64, String> {
        if !beat.is_finite() {
            return Err("finite warp quarter required".into());
        }
        let result = self.origin() + self.map.seconds_at(beat);
        if result.is_finite() {
            Ok(result)
        } else {
            Err("non-finite musical time".into())
        }
    }
    fn breakpoints(&self, start: f64, end: f64) -> Result<Vec<f64>, String> {
        if !start.is_finite() || !end.is_finite() || end < start {
            return Err("invalid warp breakpoint interval".into());
        }
        let mut times = Vec::new();
        for region in &self.map.regions {
            if self.origin() + self.map.seconds_at(region.end()) <= start {
                continue;
            }
            if self.origin() + self.map.seconds_at(region.start_quarter) >= end {
                break;
            }
            for i in 0..region.seconds.len() {
                // quarter_at_index retains the partial final quarter (e.g. a2.5q region).
                let time = self.origin() + self.map.seconds_at(region.quarter_at_index(i));
                if time > start && time < end {
                    times.push(time);
                    if times.len() > 4096 {
                        return Err("warp interval exceeds4096 musical breakpoints".into());
                    }
                }
            }
        }
        times.sort_by(f64::total_cmp);
        times.dedup();
        Ok(times)
    }
}
fn owned(timing: &Timing, rate: u32, zero: f64) -> Result<Arc<dyn Clock>, String> {
    Ok(Arc::new(SharedClock::new(
        Arc::new(timing.clone()),
        rate,
        zero,
    )?))
}
fn stored_rate(timing: &Timing) -> Result<u32, String> {
    let rate = timing.map.sample_rate;
    if !rate.is_finite() || rate.fract() != 0. || !(8000. ..=192000.).contains(&rate) {
        return Err("invalid authoritative sample rate".into());
    }
    Ok(rate as u32)
}
pub(super) fn edit(state: &mut State, hook: &str, input: &serde_json::Value) -> Result<(), String> {
    let absent = state.warp.clock.is_none();
    if absent {
        let id = input["id"].as_str().ok_or("audio clip id required")?;
        let clip = state.clips.get(id).ok_or("unknown audio clip")?;
        let Content::Audio { source, .. } = &clip.content else {
            return Err("warp requires an audio clip".into());
        };
        let rate = state
            .sources
            .get(source)
            .ok_or("decoded source required")?
            .sample_rate;
        state.warp.clock = Some(owned(
            &state.musical_time,
            rate,
            state.timeline_zero_seconds,
        )?);
    }
    let result = warp_treatment::edit(state, hook, input);
    if result.is_err() && absent {
        state.warp.clock = None;
    }
    result
}
pub(super) fn ensure(state: &mut State) -> Result<(), String> {
    if state.warp.clock.is_none() && !state.warp.entries.is_empty() {
        state.warp.clock = Some(owned(
            &state.musical_time,
            stored_rate(&state.musical_time)?,
            state.timeline_zero_seconds,
        )?);
    }
    Ok(())
}
/// Prepare without changing map, clip starts, clip durations, revision or jobs.
/// shift is used only for rebases; old endpoints are read before starts move.
pub(super) fn prepare(
    state: &State,
    next: &Timing,
    zero: f64,
    shift: f64,
) -> Result<warp_treatment::ClockChange, String> {
    let next = owned(next, stored_rate(next)?, zero)?;
    warp_treatment::prepare_clock_change(state, next, shift)
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn timing() -> Timing {
        Timing::from_snapshot(
            &json!({"bpm":120.,"revision":7,"sample_rate":48000.,"rubato_regions":[{"start_quarter":1.,"seconds":[0.,0.3,1.2,1.4],"length_quarters":2.5,"ripple":false}]}),
        )
    }
    #[test]
    fn delegates_to_actual_model_including_partial_region_end_and_origin() {
        let t = Arc::new(timing());
        let c = SharedClock::new(t.clone(), 44100, 3.00001).unwrap();
        let m = t.for_clock(44100., 3.00001);
        let origin = m.zero_sample as f64 / 44100.;
        for q in [-2., 0., 1., 1.4, 2., 3., 3.5, 4., 8.] {
            let s = origin + m.seconds_at(q);
            assert!((c.beat_to_seconds(q).unwrap() - s).abs() < 1e-12);
            assert!((c.seconds_to_beat(s).unwrap() - q).abs() < 1e-12);
        }
        let points = c.breakpoints(origin, origin + 3.).unwrap();
        assert_eq!(
            points,
            vec![origin + 0.5, origin + 0.8, origin + 1.7, origin + 1.9]
        );
    }
    #[test]
    fn cache_identity_uses_real_timing_revision_rate_and_rounded_origin() {
        let t = Arc::new(timing());
        let a = SharedClock::new(t.clone(), 44100, 0.00001).unwrap();
        let b = a.for_sample_rate(48000).unwrap().unwrap();
        assert_ne!(a.cache_identity(), b.cache_identity());
        let c = SharedClock::new(t.clone(), 44100, 0.00003).unwrap();
        assert_ne!(a.cache_identity(), c.cache_identity());
        let mut next = (*t).clone();
        next.revision += 1;
        let d = SharedClock::new(Arc::new(next), 44100, 0.00001).unwrap();
        assert_ne!(a.cache_identity(), d.cache_identity());
    }
}
