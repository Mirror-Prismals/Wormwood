//! Continuous water data and ordinary implementation helpers. Application
//! events originate only in the registered layer-five WaterSystem.
pub(crate) mod ambient;
#[path = "bodies.rs"]
pub(crate) mod bodies;
#[path = "config.rs"]
pub(crate) mod config;
#[path = "floodgate.rs"]
pub(crate) mod floodgate;
#[path = "flow.rs"]
pub(crate) mod flow;
#[path = "hydrology.rs"]
pub(crate) mod hydrology;
#[path = "math.rs"]
pub(crate) mod math;
#[path = "ocean.rs"]
pub(crate) mod ocean;
#[path = "optics.rs"]
pub(crate) mod optics;
#[path = "runtime.rs"]
pub(crate) mod runtime;
#[path = "slides.rs"]
pub(crate) mod slides;
#[path = "soft_body.rs"]
pub(crate) mod soft_body;
#[path = "surface_mesh.rs"]
pub(crate) mod surface_mesh;
pub(crate) mod visibility;
#[path = "tide.rs"]
pub(crate) mod tide;
#[path = "types.rs"]
pub(crate) mod types;
#[path = "whitewater.rs"]
pub(crate) mod whitewater;
#[derive(Default)]
pub(crate) struct State {
    pub application: crate::base_system::layer_five::water_system::State,
    pub presentation: crate::base_system::layer_four::water_render_system::State,
    pub tide: tide::State,
    pub ocean: ocean::State,
    pub flow: flow::State,
    pub slides: slides::State,
    pub hydrology: hydrology::State,
    pub soft_body: soft_body::State,
    pub floodgate: floodgate::State,
}
/// Structural transport bound; each receiving hook additionally checks its own
/// semantic envelope before changing state or submitting native work.
pub(crate) fn valid_data(value: &serde_json::Value) -> bool {
    let mut stack = vec![(value, 0usize)];
    let mut count = 0usize;
    while let Some((v, depth)) = stack.pop() {
        count += 1;
        if count > 8_000_000 || depth > 24 {
            return false;
        }
        match v {
            serde_json::Value::Number(n) => {
                if !n.as_f64().is_some_and(|n| n.is_finite() && n.abs() <= 1e12) {
                    return false;
                }
            }
            serde_json::Value::String(s) => {
                if s.len() > 4096 {
                    return false;
                }
            }
            serde_json::Value::Array(a) => {
                if a.len() > 393216 {
                    return false;
                }
                stack.extend(a.iter().map(|p| (p, depth + 1)));
            }
            serde_json::Value::Object(o) => {
                if o.len() > 128 {
                    return false;
                }
                stack.extend(o.values().map(|p| (p, depth + 1)));
            }
            _ => {}
        }
    }
    true
}
#[cfg(test)]
#[path = "tests.rs"]
mod tests;

pub(crate) mod presentation;
