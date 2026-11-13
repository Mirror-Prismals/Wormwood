//! Bounded wall-clock measurements shared by visual and headless workshops.
use serde_json::{Value, json};
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct Performance {
    recent: VecDeque<[f64; 6]>, // frame, physics, mesh, render/present, simulated seconds, running
    frames: u64,
    totals: [f64; 6],
    worst: f64,
    frame_samples: Vec<f64>,
}
impl Performance {
    pub fn record(
        &mut self,
        wall: f64,
        physics: f64,
        mesh: f64,
        render: f64,
        simulated: f64,
        running: bool,
    ) {
        let sample = [
            wall,
            physics,
            mesh,
            render,
            simulated.max(0.),
            if running { wall } else { 0. },
        ];
        self.frames += 1;
        for (total, v) in self.totals.iter_mut().zip(sample) {
            *total += v;
        }
        self.worst = self.worst.max(wall);
        if self.frame_samples.len() < 4096 {
            self.frame_samples.push(wall);
        } else {
            self.frame_samples[(self.frames as usize - 1) % 4096] = wall;
        }
        self.recent.push_back(sample);
        // At least half a second of samples, at most 240, including slow frames.
        while self.recent.len() > 1
            && (self.recent.len() > 240
                || self.recent.iter().skip(1).map(|r| r[0]).sum::<f64>() > 0.5)
        {
            self.recent.pop_front();
        }
    }
    pub fn caption(&self, backend: &str) -> String {
        if self.recent.is_empty() {
            return format!("{backend} · Measuring FPS…");
        }
        let mut sum = [0.; 6];
        for row in &self.recent {
            for (v, x) in sum.iter_mut().zip(row) {
                *v += x;
            }
        }
        let n = self.recent.len() as f64;
        format!(
            "{backend} · {:.1} FPS · Simulation {:.0}% · CPU physics {:.1} ms · mesh {:.1} ms · render {:.1} ms",
            n / sum[0].max(1e-9),
            100. * sum[4] / sum[5].max(1e-9),
            sum[1] * 1000. / n,
            sum[2] * 1000. / n,
            sum[3] * 1000. / n
        )
    }
    pub fn report(&self) -> Value {
        let mut sorted = self.frame_samples.clone();
        sorted.sort_by(f64::total_cmp);
        let percentile = |p: f64| {
            sorted
                .get(((sorted.len().saturating_sub(1)) as f64 * p).round() as usize)
                .copied()
                .unwrap_or(0.)
                * 1000.
        };
        let n = self.frames.max(1) as f64;
        json!({"frames":self.frames,"wall_seconds":self.totals[0],"frames_per_second":self.frames as f64/self.totals[0].max(1e-9),
            "simulation_speed":self.totals[4]/self.totals[5].max(1e-9),"simulated_seconds_advanced":self.totals[4],
            "frame_ms_mean":self.totals[0]*1000./n,"frame_ms_p50":percentile(0.5),"frame_ms_p95":percentile(0.95),"frame_ms_worst":self.worst*1000.,
            "percentile_sample_count":sorted.len(),"physics_ms_mean":self.totals[1]*1000./n,"mesh_ms_mean":self.totals[2]*1000./n,"render_present_ms_mean":self.totals[3]*1000./n,
            "render_timing":"CPU submission plus present wait; not GPU timestamp duration","debug_build":cfg!(debug_assertions)})
    }
}

/// Wall-time scheduling with fixed physical steps. Never enlarge the timestep
/// or skip tool motion to make an overloaded solver appear real-time.
pub(super) struct Clock {
    debt: f64,
}
impl Clock {
    pub const STEP: f64 = 1. / 60.;
    pub fn new() -> Self {
        Self { debt: 0. }
    }
    pub fn reset(&mut self) {
        self.debt = 0.;
    }
    pub fn accrue(&mut self, seconds: f64) {
        self.debt = (self.debt + seconds.max(0.)).min(4. * Self::STEP);
    }
    pub fn due(&self) -> bool {
        self.debt + 1e-9 >= Self::STEP
    }
    pub fn consume(&mut self) {
        self.debt = (self.debt - Self::STEP).max(0.);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workshop_clock_keeps_simulation_rate_independent_of_render_rate() {
        for hz in [30., 60., 120.] {
            let mut clock = Clock::new();
            let mut steps = 0;
            for _ in 0..hz as usize {
                clock.accrue(1. / hz);
                while clock.due() {
                    clock.consume();
                    steps += 1;
                }
            }
            assert_eq!(steps, 60);
        }
        let mut clock = Clock::new();
        clock.accrue(30.);
        let mut steps = 0;
        while clock.due() {
            clock.consume();
            steps += 1;
        }
        assert_eq!(steps, 4); // no unbounded catch-up after suspension
        clock.reset();
        assert!(!clock.due());
    }
    #[test]
    fn workshop_counter_exposes_slow_physics_separately_from_rendering() {
        let mut p = Performance::default();
        for _ in 0..10 {
            p.record(0.1, 0.09, 0.002, 0.008, 1. / 60., true);
        }
        let report = p.report();
        assert!((report["frames_per_second"].as_f64().unwrap() - 10.).abs() < 1e-6);
        assert!((report["simulation_speed"].as_f64().unwrap() - 1. / 6.).abs() < 1e-6);
    }
}
