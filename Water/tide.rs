//! Cardinal's continuous cosine tide; integer cell activation/remeshing is not inherited.
#[derive(Clone, Debug)]
pub(crate) struct Settings {
    pub enabled: bool,
    pub high: f64,
    pub low: f64,
    pub period: f64,
    pub phase_offset: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            high: 0.,
            low: -7.,
            period: 44700.,
            phase_offset: 0.,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Sample {
    pub height: f64,
    pub normalized: f64,
    pub rising: bool,
    pub phase: f64,
}
#[derive(Default)]
pub(crate) struct State {
    pub settings: Settings,
    pub age: f64,
    initialized: bool,
}
impl State {
    pub fn advance(&mut self, dt: f64, world_age: Option<f64>) -> Result<Sample, String> {
        if !dt.is_finite() || dt < 0. || world_age.is_some_and(|n| !n.is_finite()) {
            return Err("invalid tide clock".into());
        }
        if !self.initialized {
            self.age = world_age.unwrap_or(0.);
            self.initialized = true;
        } else {
            self.age += dt.clamp(0., 0.1);
            if let Some(clock) = world_age {
                if (clock - self.age).abs() > 1.25 {
                    self.age = clock;
                } else {
                    self.age = self.age.clamp(clock, clock + 1.);
                }
            }
        }
        Ok(self.sample())
    }
    pub fn sample(&self) -> Sample {
        let high = self.settings.high.max(self.settings.low);
        let low = self.settings.low.min(self.settings.high);
        let phase = ((self.age + self.settings.phase_offset) / self.settings.period.max(60.))
            .rem_euclid(1.);
        let angle = phase * std::f64::consts::TAU;
        let normalized = if self.settings.enabled {
            0.5 + 0.5 * angle.cos()
        } else {
            1.
        };
        Sample {
            height: low + normalized * (high - low),
            normalized,
            rising: self.settings.enabled && angle.sin() < 0.,
            phase,
        }
    }
}
