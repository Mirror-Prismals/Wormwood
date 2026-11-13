//! Bounded, opt-in complete game-loop timing; never enters an audio callback.
use serde_json::{Value, json};
use std::{collections::VecDeque, time::Instant};
const CAPACITY: usize = 36_000;
#[derive(Default)]
pub(super) struct Timing {
    previous: Option<Instant>,
    current: Option<Frame>,
    frames: VecDeque<Frame>,
    dropped: u64,
}
struct Frame {
    index: u64,
    total_ms: f64,
    pump_ms: f64,
    update_ms: f64,
    active: bool,
    drawn: bool,
    scene_sequence: u64,
}
impl Timing {
    pub fn begin(&mut self, index: u64, now: Instant) {
        static FRAME_ONLY:std::sync::OnceLock<bool>=std::sync::OnceLock::new();
        let frame_only=*FRAME_ONLY.get_or_init(||std::env::var("WORMWOOD_GAME_FRAME_TIMING").as_deref()==Ok("1"));
        if !frame_only&&!crate::host::performance_telemetry::enabled(){return;}
        if let (Some(start), Some(mut frame)) = (self.previous.take(), self.current.take()) {
            frame.total_ms = now.duration_since(start).as_secs_f64() * 1000.;
            if self.frames.len() == CAPACITY {
                self.frames.pop_front();
                self.dropped += 1;
            }
            self.frames.push_back(frame);
        }
        self.previous = Some(now);
        self.current = Some(Frame {
            index,
            total_ms: 0.,
            pump_ms: 0.,
            update_ms: 0.,
            active: false,
            drawn: false,
            scene_sequence: 0,
        });
    }
    pub fn pump(&mut self, ms: f64, drawn: bool, scene_sequence: u64) {
        if let Some(f) = &mut self.current {
            f.pump_ms = ms;
            f.drawn = drawn;
            f.scene_sequence = scene_sequence;
        }
    }
    pub fn update(&mut self, ms: f64) {
        if let Some(f) = &mut self.current {
            f.update_ms = ms;
            f.active = true;
        }
    }
    pub fn finish(&mut self) {
        self.begin(0, Instant::now());
        self.previous = None;
        self.current = None;
    }
    pub fn report(&self) -> Value {
        let active: Vec<_> = self.frames.iter().filter(|f| f.active).collect();
        let mut times: Vec<_> = active.iter().map(|f| f.total_ms).collect();
        times.sort_by(f64::total_cmp);
        let percentile = |q: f64| {
            times
                .get(((times.len().saturating_sub(1)) as f64 * q).ceil() as usize)
                .copied()
        };
        let seconds = active.iter().map(|f| f.total_ms).sum::<f64>() / 1000.;
        let mut previous = None;
        let fresh = active
            .iter()
            .filter(|f| {
                let changed = previous != Some(f.scene_sequence);
                previous = Some(f.scene_sequence);
                changed && f.drawn
            })
            .count();
        let mut worst = active.clone();
        worst.sort_by(|a, b| b.total_ms.total_cmp(&a.total_ms));
        json!({"schema":1,"active_frames":active.len(),"retained_frames":self.frames.len(),"dropped_frames":self.dropped,
            "loop_fps":if seconds>0.{Some(active.len()as f64/seconds)}else{None},
            "draw_submission_fps":if seconds>0.{Some(active.iter().filter(|f|f.drawn).count()as f64/seconds)}else{None},
            "fresh_scene_frames":fresh,"fresh_scene_fps":if seconds>0.{Some(fresh as f64/seconds)}else{None},
            "median_ms":percentile(0.5),"p95_ms":percentile(0.95),"p99_ms":percentile(0.99),"max_ms":times.last(),
            "over_16_667_ms":times.iter().filter(|&&t|t>16.667).count(),"over_33_333_ms":times.iter().filter(|&&t|t>33.333).count(),"over_50_ms":times.iter().filter(|&&t|t>50.).count(),
            "worst":worst.iter().take(32).map(|f|json!({"frame":f.index,"total_ms":f.total_ms,"pump_ms":f.pump_ms,"update_ms":f.update_ms,"other_and_pacing_ms":(f.total_ms-f.pump_ms-f.update_ms).max(0.),"drawn":f.drawn,"scene_sequence":f.scene_sequence})).collect::<Vec<_>>(),
            "frames":self.frames.iter().map(|f|json!([f.index,f.total_ms,f.pump_ms,f.update_ms,f.active,f.drawn,f.scene_sequence])).collect::<Vec<_>>(),
            "columns":["frame","total_ms","pump_ms","update_ms","active","drawn","scene_sequence"],
            "scope":"Adjacent loop-start monotonic intervals, including event/render pump, simulation, instrumentation and pacing. Startup before loop excluded. Paused/suspended frames retained separately. Final interval ends at application-loop exit. Draws are application submissions, not physical display scanout. Fixed diagnostic replay dt is distinct from elapsed wall time."})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_frame_statistics_include_spikes_and_exclude_pauses() {
        let mut t = Timing::default();
        for (index, total_ms, active) in [(1, 16., true), (2, 60., true), (3, 900., false)] {
            t.frames.push_back(Frame {
                index,
                total_ms,
                pump_ms: 2.,
                update_ms: 5.,
                active,
                drawn: true,
                scene_sequence: index,
            });
        }
        let r = t.report();
        assert_eq!(r["active_frames"], 2);
        assert_eq!(r["max_ms"], 60.);
        assert_eq!(r["over_50_ms"], 1);
        assert_eq!(r["worst"][0]["frame"], 2);
        assert!((r["loop_fps"].as_f64().unwrap() - 2000. / 76.).abs() < 1e-10);
    }
}
