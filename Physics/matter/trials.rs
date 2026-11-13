//! Equal-duration, fresh-specimen force comparisons. A drive limit is not
//! impact force: report both it and measured contact load/work separately.
use super::*;
pub(super) const LIMITS: [f32; 3] = [45., 240., 800.];
pub(super) const LABELS: [&str; 3] = ["Light", "Medium", "Strong"];
pub(super) const DURATION: f32 = 17.;
#[derive(Clone)]
pub(super) struct Comparison {
    pub trial: usize,
    pub complete: bool,
    pub outcomes: Vec<Value>,
    pub elapsed: f32,
}
impl Comparison {
    pub fn new() -> Self {
        Self {
            trial: 0,
            complete: false,
            outcomes: vec![],
            elapsed: 0.,
        }
    }
}
impl World {
    pub fn advance(&mut self, host: &mut Host) -> Result<(), String> {
        if self.comparison.as_ref().is_some_and(|c| c.complete) {
            return Ok(());
        }
        self.advance_frame(host)?;
        if self.halted.is_some() || self.time < DURATION || self.comparison.is_none() {
            return Ok(());
        }
        let report = self.report();
        let mut comparison = self.comparison.take().unwrap();
        comparison.outcomes.push(json!({"force":LABELS[comparison.trial],"drive_limit_n":LIMITS[comparison.trial],
            "peak_contact_force_n":report["tool"]["peak_contact_force_n"],"peak_kinetic_energy_j":report["tool"]["peak_kinetic_energy_j"],
            "contact_work_j":report["tool"]["contact_work_j"],"fractures":report["fractures"],
            "solid_pieces":report["solid"]["components"],"turf_pieces":report["turf"]["pieces"],
            "lifted_samples":report["lifted_samples"],"peak_lifted_samples":self.peak_lifted,
            "peak_bucket_samples":self.peak_bucket,"mass_kg":report["mass_kg"]}));
        if comparison.trial == 2 {
            comparison.complete = true;
            self.comparison = Some(comparison);
            return Ok(());
        }
        comparison.trial += 1;
        comparison.elapsed += self.time;
        let mut next = Self::new(&self.case);
        next.tool.force_limit = Some(LIMITS[comparison.trial]);
        next.comparison = Some(comparison);
        // Keep wall-cost accounting and displayed simulation time monotonic.
        next.frames = self.frames;
        next.total_step_ms = self.total_step_ms;
        next.worst_step_ms = self.worst_step_ms;
        next.timings = self.timings;
        next.solver_iterations = self.solver_iterations;
        next.configure(host)?;
        *self = next;
        Ok(())
    }
}
