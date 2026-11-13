//! Original authored Expanse palette and sunset, adapted to a spherical local
//! horizon. Wall time is Eastern civil time (including US DST), at real speed.
//! The same per-view sample owns sky, terrain, water and vegetation lighting.
use crate::vegetation::math::*;
use serde_json::{Value, json};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

fn days(year: i64, month: i64, day: i64) -> i64 {
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yo = y - era * 400;
    let m = month + if month > 2 { -3 } else { 9 };
    era * 146097 + yo * 365 + yo / 4 - yo / 100 + (153 * m + 2) / 5 + day - 1 - 719468
}
pub(crate) fn calendar_at(epoch: f64) -> Value {
    fn civil(day: i64) -> (i64, i64, i64) {
        let z = day + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let month = mp + if mp < 10 { 3 } else { -9 };
        (
            yoe + era * 400 + i64::from(month <= 2),
            month,
            doy - (153 * mp + 2) / 5 + 1,
        )
    }
    let (year, _, _) = civil((epoch / 86400.).floor() as i64);
    let sunday = |m| 1 + (7 - (days(year, m, 1) + 4).rem_euclid(7)) % 7;
    let start = (days(year, 3, sunday(3) + 7) * 86400 + 7 * 3600) as f64;
    let end = (days(year, 11, sunday(11)) * 86400 + 6 * 3600) as f64;
    let offset = if epoch >= start && epoch < end {
        -240
    } else {
        -300
    };
    let seconds = epoch + offset as f64 * 60.;
    let local_day = (seconds / 86400.).floor() as i64;
    let (year, month, day) = civil(local_day);
    let season = if month == 12 || month <= 2 {
        "winter"
    } else if month <= 5 {
        "spring"
    } else if month <= 8 {
        "summer"
    } else {
        "autumn"
    };
    json!({"unix_seconds":epoch,"year":year,"month":month,"day":day,"day_of_year":local_day-days(year,1,1)+1,"days_in_year":days(year+1,1,1)-days(year,1,1),"fraction":seconds.rem_euclid(86400.)/86400.,"season":season,"timezone":"America/New_York","offset_minutes":offset})
}
fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0., |d| d.as_secs_f64())
}
pub(super) fn fraction_at(epoch: f64) -> f64 {
    calendar_at(epoch)["fraction"].as_f64().unwrap()
}
pub(crate) fn current_fraction() -> f64 {
    fraction_at(now_epoch())
}
fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
fn mix(a: V, b: V, t: f64) -> V {
    add(mul(a, 1. - t), mul(b, t))
}
pub(crate) fn palette(fraction: f64, z: f64) -> V {
    static DATA: OnceLock<Value> = OnceLock::new();
    let profiles = DATA.get_or_init(|| {
        serde_json::from_str(include_str!("../../Procedures/Atmosphere/sky_palette.json"))
            .expect("authored sky palette")
    });
    let profiles = profiles["profiles"].as_array().unwrap();
    let minute = fraction.rem_euclid(1.) * 1440.;
    let z = z.clamp(0., 1.);
    let sample = |p: &Value| {
        let stops = p["stops"].as_array().unwrap();
        let pair = stops
            .windows(2)
            .find(|s| z <= s[1]["z"].as_f64().unwrap())
            .unwrap_or(&stops[stops.len() - 2..]);
        let a = pair[0]["z"].as_f64().unwrap();
        let b = pair[1]["z"].as_f64().unwrap();
        mix(
            std::array::from_fn(|i| pair[0]["rgb"][i].as_f64().unwrap() / 255.),
            std::array::from_fn(|i| pair[1]["rgb"][i].as_f64().unwrap() / 255.),
            (z - a) / (b - a),
        )
    };
    let pair = profiles
        .windows(2)
        .find(|s| minute <= s[1]["minute"].as_f64().unwrap())
        .unwrap();
    let a = pair[0]["minute"].as_f64().unwrap();
    let b = pair[1]["minute"].as_f64().unwrap();
    mix(sample(&pair[0]), sample(&pair[1]), (minute - a) / (b - a))
}
pub(crate) struct Lighting {
    pub fraction: f64,
    pub sun: V,
    pub primary: V,
    pub tint: V,
    pub daylight: f64,
    pub east: V,
    pub north: V,
    pub up: V,
}
impl Lighting {
    pub(crate) fn at(fraction: f64, up: V) -> Self {
        let fraction = fraction.rem_euclid(1.);
        let hour = fraction * 24.;
        let angle = (hour - 6.) / 12. * std::f64::consts::PI;
        let (east, north) = basis(up);
        let sun = add(mul(up, angle.sin()), mul(north, -angle.cos()));
        let primary = if (6. ..18.).contains(&hour) {
            sun
        } else {
            mul(sun, -1.)
        };
        let night = [102. / 255., 125. / 255., 1.];
        let dawn = [1., 184. / 255., 120. / 255.];
        let day = [1., 244. / 255., 222. / 255.];
        let tint = if hour < 5. || hour >= 20. {
            night
        } else if hour < 6.5 {
            mix(night, dawn, smooth(5., 6.5, hour))
        } else if hour < 8. {
            mix(dawn, day, smooth(6.5, 8., hour))
        } else if hour < 17. {
            day
        } else if hour < 18.5 {
            mix(day, dawn, smooth(17., 18.5, hour))
        } else {
            mix(dawn, night, smooth(18.5, 20., hour))
        };
        Self {
            fraction,
            sun,
            primary,
            tint,
            daylight: smooth(-0.12, 0.18, angle.sin()),
            east,
            north,
            up,
        }
    }
    pub(crate) fn from_view(view: &Value) -> Self {
        let position: V = serde_json::from_value(view["position"].clone()).unwrap_or([0., 1., 0.]);
        let up = if length(position) > 1. {
            unit(position)
        } else {
            [0., 1., 0.]
        };
        let fraction = view["day_night"]["fraction"]
            .as_f64()
            .filter(|v| v.is_finite())
            .unwrap_or_else(current_fraction);
        Self::at(fraction, up)
    }
    pub(crate) fn sun_color(&self) -> V {
        self.tint.map(|c| c * (0.035 + 0.745 * self.daylight))
    }
    pub(crate) fn ambient(&self) -> V {
        self.tint.map(|c| c * (0.045 + 0.235 * self.daylight))
    }
    pub(crate) fn brightness(&self) -> f64 {
        0.08 + 0.92 * self.daylight
    }
    pub(crate) fn enrich(view: &Value) -> Value {
        let mut out = view.clone();
        let calendar = calendar_at(now_epoch());
        out["day_night"]["fraction"] = calendar["fraction"].clone();
        #[cfg(target_os="macos")]
        if let Some(minutes)=view["time_override_minutes"].as_u64().filter(|m|*m<1440) {
            out["day_night"]["fraction"]=json!(minutes as f64/1440.);
        }
        #[cfg(target_os = "ios")]
        if let Some(fraction) = crate::ios_touch::time_override_fraction() {
            out["day_night"]["fraction"] = json!(fraction);
        }
        let overridden = out["day_night"]["fraction"] != calendar["fraction"];
        if out["position"][1].as_f64().is_some_and(|y|y<0.) {
            out["day_night"]["fraction"]=json!((out["day_night"]["fraction"].as_f64().unwrap()+0.5).rem_euclid(1.));
        }
        let altitude=out["position"].as_array().map_or(200.,|p|p.iter().map(|v|v.as_f64().unwrap_or(0.).powi(2)).sum::<f64>().sqrt()-crate::terrain::RADIUS);
        out["air"]=crate::physics::matter::climate::sample(&calendar,altitude).report();
        out["calendar"] = calendar;

        let light = Self::from_view(&out);
        out["day_night"] = json!({"fraction":light.fraction,"clock":"America/New_York","speed":if overridden {0} else {1}});
        out
    }
    /// Two clip-space triangles carry local sky rays; native fragments evaluate
    /// the original profile and directional sunset per pixel. No sky mesh LOD.
    pub(crate) fn sky(&self, view: &Value) -> Vec<[f32; 24]> {
        let yaw = view["yaw"].as_f64().unwrap_or(0.);
        let pitch = view["pitch"].as_f64().unwrap_or(0.);
        let axis = |key: &str, fallback: V| {
            serde_json::from_value::<V>(view[key].clone()).unwrap_or(fallback)
        };
        let right = axis("right", [yaw.cos(), 0., yaw.sin()]);
        let up = axis(
            "up",
            [
                -yaw.sin() * pitch.sin(),
                pitch.cos(),
                yaw.cos() * pitch.sin(),
            ],
        );
        let forward = axis(
            "forward",
            [
                yaw.sin() * pitch.cos(),
                pitch.sin(),
                -yaw.cos() * pitch.cos(),
            ],
        );
        let slope = 35f64.to_radians().tan();
        let aspect = view["aspect"].as_f64().unwrap_or(4. / 3.);
        [
            [[-1., -1.], [1., -1.], [1., 1.]],
            [[-1., -1.], [1., 1.], [-1., 1.]],
        ]
        .into_iter()
        .map(|corners| {
            let mut t = [0.; 24];
            for (i, [x, y]) in corners.into_iter().enumerate() {
                let ray = add(
                    forward,
                    add(mul(right, x * slope * aspect), mul(up, y * slope)),
                );
                t[i * 4..i * 4 + 4].copy_from_slice(&[
                    x as f32,
                    y as f32,
                    f32::from_bits(1f32.to_bits() - 1),
                    1.,
                ]);
                t[12 + i * 4..16 + i * 4].copy_from_slice(&[
                    dot(ray, self.east) as f32,
                    dot(ray, self.up) as f32,
                    dot(ray, self.north) as f32,
                    (-10. - self.fraction) as f32,
                ]);
            }
            t
        })
        .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eastern_clock_tracks_dst_boundaries_and_real_seconds() {
        let epoch = (days(2026, 3, 8) * 86400 + 7 * 3600) as f64;
        assert!((fraction_at(epoch - 1.) * 86400. - 7199.).abs() < 1e-6);
        assert!((fraction_at(epoch) * 86400. - 10800.).abs() < 1e-6);
        let epoch = (days(2026, 11, 1) * 86400 + 6 * 3600) as f64;
        assert!((fraction_at(epoch - 1.) * 86400. - 7199.).abs() < 1e-6);
        assert!((fraction_at(epoch) * 86400. - 3600.).abs() < 1e-6);
    }
    #[test]
    fn calendar_uses_eastern_date_at_new_year_and_original_season_months() {
        let jan_utc = (days(2027, 1, 1) * 86400) as f64;
        let local = calendar_at(jan_utc);
        assert_eq!(local["year"], 2026);
        assert_eq!(local["month"], 12);
        assert_eq!(local["day"], 31);
        for (month, season) in [(1, "winter"), (4, "spring"), (7, "summer"), (10, "autumn")] {
            let sample = calendar_at((days(2026, month, 15) * 86400 + 17 * 3600) as f64);
            assert_eq!(sample["season"], season);
            assert_eq!(sample["month"], month);
        }
        assert_eq!(
            calendar_at((days(2028, 7, 1) * 86400) as f64)["days_in_year"],
            366
        );
    }
    #[test]
    fn palette_wraps_and_sun_tracks_local_horizon() {
        assert_eq!(palette(0., 0.), [2. / 255., 7. / 255., 25. / 255.]);
        assert_eq!(palette(0., 0.7), palette(1., 0.7));
        for up in [[0., 1., 0.], [1., 0., 0.], [0., 0., -1.]] {
            let noon = Lighting::at(0.5, up);
            let midnight = Lighting::at(0., up);
            assert!(dot(noon.sun, up) > 0.999999);
            assert!(dot(midnight.sun, up) < -0.999999);
            assert!(noon.brightness() > midnight.brightness() * 10.);
        }
    }
}

#[cfg(all(test,target_os="macos"))] mod nightworld_tests {
 use super::*;
 #[test] fn nightworld_clock_keeps_exact_half_day_offset(){
  for minute in [0,360,720,1080,1439] {
   let v=json!({"position":[0.,crate::terrain::RADIUS,0.],"time_override_minutes":minute});let mut other=v.clone();other["position"]=json!([0.,-crate::terrain::RADIUS,0.]);
   let a=Lighting::enrich(&v);let b=Lighting::enrich(&other);
   let delta=(b["day_night"]["fraction"].as_f64().unwrap()-a["day_night"]["fraction"].as_f64().unwrap()).rem_euclid(1.);
   assert!((delta-0.5).abs()<1e-10);
  }
  let a=Lighting::enrich(&json!({"position":[0.,-crate::terrain::RADIUS,0.]}));assert_eq!(a["day_night"]["speed"],1);
 }
}
