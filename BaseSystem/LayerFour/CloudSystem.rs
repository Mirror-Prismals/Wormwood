//! Native multilayer cloud appearance. Original periodic noise and cloud shape/
//! scattering equations are retained; bounded authored layer fields replace the
//! old engine's unported regional condensate/convection simulation for now.
use crate::vegetation::math::*;
use serde_json::{Value, json};
pub(crate) const NOISE: &[u8] = include_bytes!("../../Procedures/Assets/Clouds/noise.rgba");
pub(crate) const ROWS: usize = 12;
pub(crate) type Frame = [[f32; 4]; ROWS];
pub(crate) fn read_frame(view: &Value) -> Result<Frame, String> {
    if view["clouds"].is_null() {
        return Ok([[0.; 4]; ROWS]);
    }
    let frame: Frame = serde_json::from_value(view["clouds"]["rows"].clone())
        .map_err(|_| "cloud frame requires twelve vec4 rows")?;
    if !frame.iter().flatten().all(|v| v.is_finite()) {
        return Err("nonfinite cloud frame".into());
    }
    Ok(frame)
}
pub(crate) fn dispatch(input: &Value) -> Result<Value, String> {
    let position: V =
        serde_json::from_value(input["position"].clone()).map_err(|_| "cloud camera required")?;
    if position.iter().any(|v| !v.is_finite()) || length(position) < 1. {
        return Err("invalid cloud camera".into());
    }
    let light = super::atmosphere::Lighting::from_view(input);
    let epoch = input["calendar"]["unix_seconds"]
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or("sampled calendar required for clouds")?;
    let day = input["calendar"]["day_of_year"].as_f64().unwrap_or(180.);
    let winter = 0.5 + 0.5 * (std::f64::consts::TAU * (day - 15.) / 365.).cos();
    // Authored initial weather, deliberately distinct from a simulated climate.
    let front = 0.5 + 0.5 * (epoch / 21600.).sin();
    let offset = |speed: f64| (epoch * speed).rem_euclid(65536.) as f32;
    let row = |v: V, w: f64| [v[0] as f32, v[1] as f32, v[2] as f32, w as f32];
    let sun_y = dot(light.sun, light.up);
    let sunset = (1. - smooth(0., 0.30, sun_y.abs())) * light.daylight;
    let direct: V = std::array::from_fn(|i| {
        let day = [1., 0.97, 0.90][i] * (1. - sunset * 0.85) + [1., 0.46, 0.22][i] * sunset * 0.85;
        [0.12, 0.18, 0.30][i] * (1. - light.daylight) + day * light.daylight
    });
    let top = super::atmosphere::palette(light.fraction, 0.);
    let bottom = super::atmosphere::palette(light.fraction, 1.);
    let ambient: V = std::array::from_fn(|i| top[i] * 0.7 + bottom[i] * 0.3);
    let frame: Frame = [
        row(position, 1.),
        row(light.east, 0.),
        row(light.up, light.fraction),
        row(light.north, 48000.),
        [offset(6.), offset(2.), epoch.rem_euclid(8192.) as f32, 0.7],
        [offset(7.7), offset(3.9), offset(10.5), offset(3.5)],
        [
            1600.,
            900.,
            (0.52 + front * 0.20 + winter * 0.08) as f32,
            0.42,
        ],
        [
            3800.,
            1000.,
            (0.28 + front * 0.25 + winter * 0.08) as f32,
            0.,
        ],
        [7600., 850., (0.28 + front * 0.15) as f32, 0.],
        row(light.primary, light.daylight),
        row(direct, 0.12 + light.daylight * 0.62),
        row(ambient, 0.006),
    ];
    Ok(
        json!({"rows":frame,"field_model":"authored_three_layer_preview","noise":"original_64_cubed_seven_mips","max_steps_per_layer":24}),
    )
}
fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cloud_frame_uses_sampled_calendar_and_actual_camera() {
        let input = super::super::atmosphere::Lighting::enrich(
            &json!({"entity":1,"position":[0.,6371002.,0.],"yaw":0.,"pitch":0.}),
        );
        let clouds = dispatch(&input).unwrap();
        let mut view = input;
        view["clouds"] = clouds;
        let frame = read_frame(&view).unwrap();
        assert_eq!(frame[0], [0., 6371002., 0., 1.]);
        assert!(frame[6][0] < frame[7][0] && frame[7][0] < frame[8][0]);
        assert!(frame.iter().flatten().all(|v| v.is_finite()));
        assert_eq!(read_frame(&json!({})).unwrap(), [[0.; 4]; 12]);
        assert!(read_frame(&json!({"clouds":{"rows":[1,2,3]}})).is_err());
    }
}
