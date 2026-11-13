//! Bounded GPU samples. CPU elapsed time is never substituted for GPU queries.
use crate::host::performance_telemetry;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
};

pub(super) fn enabled() -> bool {
    performance_telemetry::enabled()
}

pub(super) struct GpuLog {
    backend: &'static str,
    category: &'static str,
    supported: bool,
    method: &'static str,
    scope: &'static str,
    output: PathBuf,
    submitted: u64,
    completed: u64,
    incomplete: u64,
    samples: VecDeque<(u64, f64)>,
    missing: VecDeque<u64>,
    pending: BTreeMap<u64, u64>,
    query_context: Value,
    query_observations: VecDeque<Value>,
    query_observation_count: u64,
    finished: bool,
    sample_limit: usize,
}
impl GpuLog {
    pub(super) fn new(
        output: &Path,
        backend: &'static str,
        workstation: bool,
        supported: bool,
        method: &'static str,
        scope: &'static str,
    ) -> Self {
        let category = if workstation {
            "DawGraphics"
        } else {
            "GameGraphics"
        };
        if !supported {
            performance_telemetry::gpu_record(
                backend,
                performance_telemetry::generation(),
                None,
                false,
                method,
                category,
            );
        }
        Self {
            backend,
            category,
            supported,
            method,
            scope,
            output: output.join(format!("performance-gpu-{backend}.json")),
            submitted: 0,
            completed: 0,
            incomplete: 0,
            samples: VecDeque::new(),
            missing: VecDeque::new(),
            pending: BTreeMap::new(),
            query_context: Value::Null,
            query_observations: VecDeque::new(),
            query_observation_count: 0,
            finished: false,
            sample_limit: if workstation && std::env::var("WORMWOOD_WORKSTATION_BENCHMARK_MIN_SECONDS")
                .ok().and_then(|v|v.parse::<f64>().ok()).is_some_and(|s|s.is_finite() && s>0. && s<=120.) {
                20_120
            } else { 4_096 },
        }
    }
    pub(super) fn set_query_context(&mut self, context: Value) {
        self.query_context = context;
    }
    /// Preserve raw backend evidence without inferring GPU cost from CPU waits.
    /// Like converted samples, these opt-in observations have a fixed limit.
    pub(super) fn observe_query(&mut self, serial: u64, observation: Value) {
        self.query_observation_count += 1;
        if self.query_observations.len() == self.sample_limit {
            self.query_observations.pop_front();
        }
        self.query_observations
            .push_back(json!({"draw":serial,"raw":observation}));
    }
    pub(super) fn submitted(&mut self, serial: u64) {
        self.submitted = self.submitted.max(serial);
        if self.supported {
            self.pending
                .insert(serial, performance_telemetry::generation());
        } else {
            // A new telemetry generation still needs the capability result.
            performance_telemetry::gpu_record(
                self.backend,
                performance_telemetry::generation(),
                None,
                false,
                self.method,
                self.category,
            );
        }
    }
    pub(super) fn record(&mut self, serial: u64, milliseconds: f64) {
        if !milliseconds.is_finite() || milliseconds <= 0. {
            self.missing(serial);
            return;
        }
        let generation = self
            .pending
            .remove(&serial)
            .unwrap_or_else(performance_telemetry::generation);
        self.completed += 1;
        if self.samples.len() == self.sample_limit {
            self.samples.pop_front();
        }
        self.samples.push_back((serial, milliseconds));
        performance_telemetry::gpu_record(
            self.backend,
            generation,
            Some((milliseconds * 1_000_000.).round().min(u64::MAX as f64) as u64),
            true,
            self.method,
            self.category,
        );
    }
    pub(super) fn missing(&mut self, serial: u64) {
        if !self.supported {
            return;
        }
        let generation = self
            .pending
            .remove(&serial)
            .unwrap_or_else(performance_telemetry::generation);
        self.incomplete += 1;
        if self.missing.len() == self.sample_limit {
            self.missing.pop_front();
        }
        self.missing.push_back(serial);
        performance_telemetry::gpu_record(
            self.backend,
            generation,
            None,
            true,
            self.method,
            self.category,
        );
    }
    pub(super) fn finish(&mut self) {
        if self.finished {
            return;
        }
        for serial in self.pending.keys().copied().collect::<Vec<_>>() {
            self.missing(serial);
        }
        self.snapshot();
        self.finished=true;
    }
    /// Persist completed observations without retiring the resident query stream.
    pub(super) fn snapshot(&self) {
        let mut samples = self.samples.iter().copied().collect::<Vec<_>>();
        samples.sort_unstable_by_key(|sample| sample.0);
        let mut missing = self.missing.iter().copied().collect::<Vec<_>>();
        missing.sort_unstable();
        let report = json!({"backend": self.backend, "category": self.category,
            "supported": self.supported, "method": self.method, "scope": self.scope,
            "cpu_time_substituted": false, "submitted_frames": self.submitted,
            "completed_samples": self.completed, "incomplete_samples": self.incomplete,
            "sample_limit": self.sample_limit, "samples_truncated": self.completed > self.sample_limit as u64,
            "incomplete_truncated": self.incomplete > self.sample_limit as u64,
            "query_context": self.query_context,
            "query_observations": self.query_observations,
            "query_observations_truncated": self.query_observation_count > self.sample_limit as u64,
            "samples": samples.iter().map(|(draw, ms)| json!({"draw": draw, "gpu_ms": ms})).collect::<Vec<_>>(),
            "incomplete_draws": missing});
        match serde_json::to_vec_pretty(&report) {
            Ok(bytes) => {
                if let Err(error) = std::fs::write(&self.output, &bytes) {
                    eprintln!("GPU telemetry report {}: {error}", self.output.display());
                }
                // Prewarming a DAW client may finish after the game client.
                // Preserve each category as well as the legacy last-client file.
                let category_output=self.output.with_file_name(format!("performance-gpu-{}-{}.json",self.backend,self.category));
                if let Err(error)=std::fs::write(&category_output,&bytes){eprintln!("GPU telemetry report {}: {error}",category_output.display());}
            }
            Err(error) => eprintln!("GPU telemetry serialization: {error}"),
        }
    }
}
impl Drop for GpuLog {
    fn drop(&mut self) {
        self.finish();
    }
}
