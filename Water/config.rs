//! Data configuration for independent tide/wave/current/optical/slide leaves.
//! Parcel material and source ownership are intentionally configured separately.
use super::{floodgate, flow, ocean, optics, slides, soft_body, tide};
use serde_json::Value;
#[derive(Default)]
pub(crate) struct Settings {
    pub tide: tide::Settings,
    pub ocean: ocean::Settings,
    pub flow: flow::Settings,
    pub optics: optics::Settings,
    pub slides: slides::Settings,
    pub soft_body: soft_body::Settings,
    pub floodgate: floodgate::Settings,
}
fn section<'a>(v: &'a Value, key: &str) -> Result<&'a Value, String> {
    let s = &v[key];
    if s.is_null() || s.is_object() {
        Ok(s)
    } else {
        Err(format!("water settings {key} must be an object"))
    }
}
fn number(
    v: &Value,
    key: &str,
    default: f64,
    range: std::ops::RangeInclusive<f64>,
) -> Result<f64, String> {
    if v[key].is_null() {
        return if default.is_finite() && range.contains(&default) {
            Ok(default)
        } else {
            Err(format!(
                "water default {key} conflicts with configured bounds"
            ))
        };
    }
    v[key]
        .as_f64()
        .filter(|n| n.is_finite() && range.contains(n))
        .ok_or_else(|| format!("invalid water setting {key}"))
}
fn boolean(v: &Value, key: &str, default: bool) -> Result<bool, String> {
    if v[key].is_null() {
        Ok(default)
    } else {
        v[key]
            .as_bool()
            .ok_or_else(|| format!("invalid water setting {key}"))
    }
}
fn vector<const N: usize>(v: &Value, key: &str, default: [f64; N]) -> Result<[f64; N], String> {
    if v[key].is_null() {
        return Ok(default);
    }
    let a = v[key]
        .as_array()
        .filter(|a| a.len() == N)
        .ok_or_else(|| format!("invalid water setting {key}"))?;
    let mut out = [0.; N];
    for (i, n) in a.iter().enumerate() {
        out[i] = n
            .as_f64()
            .filter(|n| n.is_finite() && n.abs() <= 1e6)
            .ok_or_else(|| format!("invalid water setting {key}"))?;
    }
    Ok(out)
}
impl Settings {
    pub fn parse(v: &Value) -> Result<Self, String> {
        if !v.is_object() {
            return Err("water settings must be an object".into());
        }
        let mut out = Self::default();
        let s = section(v, "tide")?;
        out.tide.enabled = boolean(s, "enabled", out.tide.enabled)?;
        out.tide.high = number(s, "high", out.tide.high, -10000.0..=10000.)?;
        out.tide.low = number(s, "low", out.tide.low, -10000.0..=10000.)?;
        out.tide.period = number(s, "period", out.tide.period, 60.0..=1e9)?;
        out.tide.phase_offset = number(s, "phase_offset", 0., -1e9..=1e9)?;
        let s = section(v, "ocean")?;
        out.ocean.amplitude = number(s, "amplitude", out.ocean.amplitude, 0.0..=100.)?;
        out.ocean.period = number(s, "period", out.ocean.period, 1.0..=10000.)?;
        out.ocean.wavelength = number(s, "wavelength", out.ocean.wavelength, 4.0..=100000.)?;
        out.ocean.direction = vector(s, "direction", out.ocean.direction)?;
        out.ocean.shore_depth = number(s, "shore_depth", out.ocean.shore_depth, 0.5..=10000.)?;
        out.ocean.shore_gain = number(s, "shore_gain", out.ocean.shore_gain, 1.0..=10.)?;
        let s = section(v, "flow")?;
        out.flow.ocean_push = number(s, "ocean_push", out.flow.ocean_push, 0.0..=100.)?;
        out.flow.river_push = number(s, "river_push", out.flow.river_push, 0.0..=100.)?;
        out.flow.response = number(s, "response", out.flow.response, 0.0..=100.)?;
        let s = section(v, "optics")?;
        out.optics.murkiness = number(s, "murkiness", out.optics.murkiness, 0.0..=1.)?;
        out.optics.sky_color_intensity = number(
            s,
            "sky_color_intensity",
            out.optics.sky_color_intensity,
            0.0..=1.,
        )?;
        out.optics.neutral_reflection_intensity = number(
            s,
            "neutral_reflection_intensity",
            out.optics.neutral_reflection_intensity,
            0.0..=1.,
        )?;
        out.optics.planar_reflection =
            boolean(s, "planar_reflection", out.optics.planar_reflection)?;
        out.optics.sky_top = vector(s, "sky_top", out.optics.sky_top)?;
        out.optics.sky_bottom = vector(s, "sky_bottom", out.optics.sky_bottom)?;
        out.optics.light = vector(s, "light", out.optics.light)?;
        if super::math::len(out.optics.light) < 1e-9 {
            return Err("water light direction is zero".into());
        }
        let s = section(v, "slides")?;
        out.slides.min_radius = number(s, "min_radius", out.slides.min_radius, 0.05..=10.)?;
        out.slides.max_radius = number(
            s,
            "max_radius",
            out.slides.max_radius,
            out.slides.min_radius..=10.,
        )?;
        out.slides.radius_step = number(s, "radius_step", out.slides.radius_step, 0.01..=5.)?;
        out.slides.lift = number(s, "lift", out.slides.lift, 0.0..=10.)?;
        out.slides.steps_per_meter =
            number(s, "steps_per_meter", out.slides.steps_per_meter, 1.0..=32.)?;
        out.slides.wall_height = number(s, "wall_height", out.slides.wall_height, 0.0..=12.)?;
        let sides = number(s, "sides", out.slides.sides as f64, 8.0..=48.)?;
        if sides.fract() != 0. || sides as usize % 2 != 0 {
            return Err("water slide sides must be even".into());
        }
        out.slides.sides = sides as usize;
        let s = section(v, "soft_body")?;
        out.soft_body.gravity = number(s, "gravity", 18., 0.0..=100.)?;
        out.soft_body.air_drag = number(s, "air_drag", 0.12, 0.0..=100.)?;
        out.soft_body.downhill = number(s, "downhill", 9., 0.0..=100.)?;
        out.soft_body.neighbour_radius = number(s, "neighbour_radius", 1.55, 0.1..=10.)?;
        out.soft_body.rest_distance = number(
            s,
            "rest_distance",
            0.94,
            0.05..=out.soft_body.neighbour_radius,
        )?;
        out.soft_body.cohesion = number(s, "cohesion", 2.4, 0.0..=100.)?;
        out.soft_body.viscosity = number(s, "viscosity", 3.2, 0.0..=100.)?;
        out.soft_body.radius = number(s, "radius", 0.48, 0.01..=out.soft_body.rest_distance)?;
        out.soft_body.max_speed = number(s, "max_speed", 28., 0.1..=100.)?;
        out.soft_body.settle_speed = number(s, "settle_speed", 0.16, 0.0..=10.)?;
        out.soft_body.settle_seconds = number(s, "settle_seconds", 1.25, 0.1..=60.)?;
        out.soft_body.capacity = number(s, "capacity", 1000., 1.0..=1000.)? as usize;
        let s = section(v, "floodgate")?;
        out.floodgate.enabled = boolean(s, "enabled", true)?;
        out.floodgate.course_start = if s["course_start"].is_null() {
            None
        } else {
            Some(
                s["course_start"]
                    .as_u64()
                    .filter(|n| *n > 0 && *n <= 1_000_000_000_000)
                    .ok_or("floodgate course_start requires a positive tie id")?,
            )
        };
        out.floodgate.target = number(s, "target", 300., 0.0..=1000.)? as usize;
        out.floodgate.spawn_per_step = number(s, "spawn_per_step", 12., 0.0..=100.)? as usize;
        out.floodgate.initial_speed = number(s, "initial_speed", 0.75, 0.0..=28.)?;
        Ok(out)
    }
}
