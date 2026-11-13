//! Cardinal lightbox controls adapted to the authored rectangular studio lights.
//! No material, tone-map or sample-count changes. Orbit offsets are relative to the exported studio.
use serde_json::{Value, json};
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Rig {
    pub enabled: bool,
    pub lights: [[f64; 11]; 3],
}
// Azimuth/elevation offsets, RGB multipliers, brightness, width/height scale,
// rotation offset, aim X/Y in studio metres. Offsets retain the authored rig.
const DEFAULT: [f64; 11] = [0., 0., 1., 1., 1., 1., 1., 1., 0., 0., 0.];
impl Default for Rig {
    fn default() -> Self {
        let mut lights = [DEFAULT; 3];
        // Orbit the cool fill away from the broad face of the right ruby.
        lights[1][0] = 148.32;
        lights[1][1] = -85.;
        Self {
            enabled: true,
            lights,
        }
    }
}
impl Rig {
    pub fn value(&self) -> Value {
        json!({"enabled":self.enabled,"lights":self.lights})
    }
    pub fn parse(v: &Value) -> Result<Self, String> {
        let enabled = v["enabled"]
            .as_bool()
            .ok_or("invalid lightbox enabled flag")?;
        let lights: [[f64; 11]; 3] =
            serde_json::from_value(v["lights"].clone()).map_err(|_| "invalid lightbox rig")?;
        for light in lights {
            for (i, n) in light.into_iter().enumerate() {
                let (lo, hi, _) = limits(i);
                if !n.is_finite() || n < lo || n > hi {
                    return Err("lightbox value outside its allowed range".into());
                }
            }
        }
        Ok(Self { enabled, lights })
    }
    pub fn adjust(&mut self, light: usize, parameter: usize, direction: f64) {
        let (lo, hi, step) = limits(parameter);
        self.lights[light][parameter] =
            (self.lights[light][parameter] + step * direction).clamp(lo, hi);
    }
}
pub fn limits(i: usize) -> (f64, f64, f64) {
    match i {
        0 | 8 => (-180., 180., 5.),
        1 => (-85., 85., 5.),
        2..=4 => (0., 1., 0.1),
        5 => (0., 4., 0.1),
        6 | 7 => (0.1, 2., 0.1),
        _ => (-1., 1., 0.1),
    }
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn unit(v: [f64; 3]) -> [f64; 3] {
    let n = dot(v, v).sqrt().max(1e-12);
    v.map(|v| v / n)
}
fn rotate(v: [f64; 3], axis: [f64; 3], angle: f64) -> [f64; 3] {
    let (s, c) = angle.to_radians().sin_cos();
    let q = cross(axis, v);
    let d = dot(axis, v);
    std::array::from_fn(|i| v[i] * c + q[i] * s + axis[i] * d * (1. - c))
}
fn xyz(r: [f32; 4]) -> [f64; 3] {
    [r[0] as f64, r[1] as f64, r[2] as f64]
}
fn put(r: &mut [f32; 4], v: [f64; 3]) {
    for i in 0..3 {
        r[i] = v[i] as f32;
    }
}
pub(crate) fn apply(rows: &mut [[f32; 4]], value: &Value) -> Result<(), String> {
    let rig = if value.is_null() { Rig::default() } else { Rig::parse(value)? };
    if rows.len() < 31 {
        return Err("lightbox requires a complete light packet".into());
    }
    for (i, p) in rig.lights.iter().enumerate() {
        let at = 16 + i * 5;
        if !rig.enabled {
            rows[at + 4][0..3].fill(0.);
            continue;
        }
        if *p == DEFAULT {
            continue;
        }
        let mut position = xyz(rows[at]);
        let mut u = xyz(rows[at + 1]);
        let mut v = xyz(rows[at + 2]);
        let mut n = xyz(rows[at + 3]);
        let azimuth = position[1].atan2(position[0]).to_degrees();
        let elevation = position[2]
            .atan2(position[0].hypot(position[1]))
            .to_degrees();
        let elevation_delta = (elevation + p[1]).clamp(-89., 89.) - elevation;
        let axis = [
            -(azimuth + p[0]).to_radians().sin(),
            (azimuth + p[0]).to_radians().cos(),
            0.,
        ];
        for vector in [&mut position, &mut u, &mut v, &mut n] {
            *vector = rotate(rotate(*vector, [0., 0., 1.], p[0]), axis, -elevation_delta);
        }
        if p[9] != 0. || p[10] != 0. {
            let target = unit([p[9] - position[0], p[10] - position[1], -position[2]]);
            let axis = cross(n, target);
            let length = dot(axis, axis).sqrt();
            if length > 1e-9 {
                let axis = unit(axis);
                let angle = dot(n, target).clamp(-1., 1.).acos().to_degrees();
                u = rotate(u, axis, angle);
                v = rotate(v, axis, angle);
            }
            n = target;
        }
        u = rotate(u, n, p[8]);
        v = rotate(v, n, p[8]);
        put(&mut rows[at], position);
        put(&mut rows[at + 1], u);
        put(&mut rows[at + 2], v);
        put(&mut rows[at + 3], n);
        rows[at + 1][3] *= p[6] as f32;
        rows[at + 2][3] *= p[7] as f32;
        for channel in 0..3 {
            rows[at + 4][channel] *= (p[2 + channel] * p[5]) as f32;
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn frame() -> Vec<[f32; 4]> {
        let v: Value = serde_json::from_str(include_str!(
            "../Art/AudioWorkstation/GemPanels/GemPanels_runtime.json"
        ))
        .unwrap();
        let mut r = vec![[0.; 4]; 32];
        for (i, l) in v["lights"].as_array().unwrap().iter().enumerate() {
            for (j, key) in [
                "position_m",
                "axis_u",
                "axis_v",
                "emission_normal",
                "radiance_watts_per_sr_m2_rgb",
            ]
            .iter()
            .enumerate()
            {
                for a in 0..3 {
                    r[16 + i * 5 + j][a] = l[key][a].as_f64().unwrap() as f32;
                }
            }
            r[17 + i * 5][3] = l["size_m"][0].as_f64().unwrap() as f32 / 2.;
            r[18 + i * 5][3] = l["size_m"][1].as_f64().unwrap() as f32 / 2.;
        }
        r
    }
    #[test]
    fn default_orbits_only_the_cool_fill() {
        let mut r = frame();
        let old = r.clone();
        apply(&mut r, &Rig::default().value()).unwrap();
        assert_eq!(&r[..21], &old[..21]);
        assert_ne!(&r[21..25], &old[21..25]);
        assert_eq!(&r[25..], &old[25..]);
        let mut implicit = old;
        apply(&mut implicit, &Value::Null).unwrap();
        assert_eq!(r, implicit);
    }
    #[test]
    fn orbit_preserves_distance_and_orthonormal_emitter() {
        let mut r = frame();
        let original = r.clone();
        let mut rig = Rig::default();
        rig.lights[1][0] = 65.;
        rig.lights[1][1] = -35.;
        rig.lights[1][8] = 30.;
        apply(&mut r, &rig.value()).unwrap();
        assert_eq!(&r[16..21], &original[16..21]);
        assert!(
            (dot(xyz(r[21]), xyz(r[21])) - dot(xyz(original[21]), xyz(original[21]))).abs() < 1e-5
        );
        for k in 22..25 {
            assert!((dot(xyz(r[k]), xyz(r[k])) - 1.).abs() < 1e-5);
        }
        assert!(dot(xyz(r[22]), xyz(r[23])).abs() < 1e-5);
    }
    #[test]
    fn disable_and_brightness_change_only_radiance() {
        let mut r = frame();
        let old = r.clone();
        let mut rig = Rig::default();
        rig.enabled = false;
        apply(&mut r, &rig.value()).unwrap();
        for i in 0..3 {
            assert_eq!(r[20 + i * 5], [0.; 4]);
            assert_eq!(&r[16 + i * 5..20 + i * 5], &old[16 + i * 5..20 + i * 5]);
        }
    }
    #[test]
    fn malformed_preferences_are_rejected() {
        assert!(Rig::parse(&json!({"enabled":true,"lights":[]})).is_err());
        let mut r = Rig::default();
        r.lights[0][6] = 0.;
        assert!(Rig::parse(&r.value()).is_err());
    }
    #[test]
    fn edited_lighting_invalidates_retained_illumination() {
        let mut r = frame();
        r.resize(256, [0.; 4]);
        let mut original = r.clone();
        apply(&mut original, &Rig::default().value()).unwrap();
        assert!(!crate::workstation::deferred_floor::lighting_changed(
            &original, &original
        ));
        for parameter in 0..11 {
            let mut rig = Rig::default();
            rig.adjust(1, parameter, if parameter == 1 { 1. } else { -1. });
            let mut edited = r.clone();
            apply(&mut edited, &rig.value()).unwrap();
            assert!(
                crate::workstation::deferred_floor::lighting_changed(&original, &edited),
                "parameter {parameter}"
            );
        }
    }
}
