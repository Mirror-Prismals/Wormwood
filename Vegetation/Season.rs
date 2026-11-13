//! Gradual northern Pennsylvania visual phenology; calendar time never changes
//! plant age, damage or branch identity. Individual leaf thresholds are stable.
use serde_json::Value;
#[derive(Clone, Copy)]
pub struct Foliage {
    pub cover: f64,
    pub color: [f32; 3],
    pub fallen: f64,
}
fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}
impl Foliage {
    pub fn at(day: f64, variation: f64) -> Self {
        let day = day + variation * 7.;
        let leaf_out = smooth(92., 137., day);
        // Nearby trees change and shed at different times. Late October still
        // carries a substantial canopy, including slower-changing green trees.
        let autumn = smooth(252. + variation * 13., 307. + variation * 13., day);
        let drop = smooth(300. + variation * 9., 334. + variation * 9., day);
        let cover = leaf_out * (1. - drop);
        let green = [0.19, 0.36, 0.075];
        let gold = [0.66, 0.43, 0.10];
        let red = [0.51, 0.19, 0.09];
        let fall = if variation > 0. { gold } else { red };
        let color = std::array::from_fn(|i| (green[i] * (1. - autumn) + fall[i] * autumn) as f32);
        Self {
            cover,
            color,
            fallen: drop,
        }
    }
    pub fn for_profile(view: &Value, site_id:u64, profile:super::profiles::Profile) -> Self {
        if profile.evergreen() { return Self {cover:1.,color:profile.leaf_color,fallen:0.}; }
        let mut f=Self::from_view(view,site_id);
        // Blend each species' summer green toward its characteristic autumn palette.
        let a=((f.color[0]-0.19)/0.47).clamp(0.,1.);
        let fall=match profile.name { "red_maple" => [0.62,0.12,0.045], "sugar_maple" => [0.78,0.38,0.045], "yellow_birch" => [0.78,0.61,0.08], "black_cherry" => [0.58,0.39,0.09], _ => [0.49,0.29,0.095] };
        f.color=std::array::from_fn(|i|profile.leaf_color[i]*(1.-a)+fall[i]*a);
        f
    }
    pub fn from_view(view: &Value, site_id: u64) -> Self {
        let owned;
        let calendar = if view["calendar"].is_object() {
            &view["calendar"]
        } else {
            use std::time::{SystemTime, UNIX_EPOCH};
            owned = crate::base_system::layer_four::atmosphere::calendar_at(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0., |d| d.as_secs_f64()),
            );
            &owned
        };
        // Normalize leap years so the visual schedule has no date drift.
        let day = calendar["day_of_year"].as_f64().unwrap_or(180.) * 365.
            / calendar["days_in_year"].as_f64().unwrap_or(365.);
        Self::at(day, super::math::random(site_id, 908) * 2. - 1.)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deciduous_cycle_drops_leaves_and_returns_to_green() {
        let winter = Foliage::at(15., 0.);
        let summer = Foliage::at(190., 0.);
        let autumn = Foliage::at(285., 0.);
        assert_eq!(winter.cover, 0.);
        assert_eq!(summer.cover, 1.);
        assert!(autumn.color[0] > summer.color[0]);
        assert!(Foliage::at(335., 0.).cover < 0.01);
        assert!(Foliage::at(115., 0.).cover > 0. && Foliage::at(115., 0.).cover < 1.);
    }
    #[test]
    fn late_october_retains_leaves_with_mixed_change_timing() {
        let trees:Vec<_>=(-10..=10).map(|i|Foliage::at(300.,i as f64/10.)).collect();
        assert!(trees.iter().all(|f|f.cover>0.4));
        let spread=trees.iter().map(|f|f.color[0]).fold(f32::NEG_INFINITY,f32::max)
            - trees.iter().map(|f|f.color[0]).fold(f32::INFINITY,f32::min);
        assert!(spread>0.05);
    }
}
