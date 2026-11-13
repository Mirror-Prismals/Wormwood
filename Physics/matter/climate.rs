//! Deterministic Pennsylvania-like air conditions. This is an authored seasonal
//! climate, not fetched weather or a calibrated atmospheric simulation.
use serde_json::{Value, json};
#[derive(Clone, Copy)]
pub(crate) struct Air {
    pub temperature_c: f32,
    pub wind_m_s: f32,
    pub cloud: f32,
}
pub(crate) fn sample(calendar: &Value, altitude_m: f64) -> Air {
    let day = calendar["day_of_year"].as_f64().unwrap_or(180.);
    let days = calendar["days_in_year"].as_f64().unwrap_or(365.);
    let hour = calendar["fraction"].as_f64().unwrap_or(0.5) * 24.;
    let epoch = calendar["unix_seconds"].as_f64().unwrap_or(0.);
    let front = 0.5 + 0.5 * (epoch / 21600.).sin(); // same authored front as CloudSystem
    let cloud = (0.52 + front * 0.20).clamp(0., 1.);
    let season = 11.5 - 14.5 * (std::f64::consts::TAU * (day - 20.) / days).cos();
    let diurnal = 5.0 * (1. - cloud * 0.6) * (std::f64::consts::TAU * (hour - 15.) / 24.).cos();
    Air {
        temperature_c: (season + diurnal - 0.0065 * (altitude_m - 200.) - 4. * front) as f32,
        wind_m_s: (1.5 + 5. * front) as f32,
        cloud: cloud as f32,
    }
}
impl Air {
    pub fn report(self) -> Value {
        json!({"temperature_c":self.temperature_c,"wind_m_s":self.wind_m_s,"cloud_fraction":self.cloud,"model":"authored seasonal, diurnal, altitude and weather-front climate"})
    }
}
pub(crate) fn now(altitude: f64) -> Air {
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0., |d| d.as_secs_f64());
    sample(
        &crate::base_system::layer_four::atmosphere::calendar_at(epoch),
        altitude,
    )
}
