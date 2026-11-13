//! Original generic profiles plus central-PA species assets; artistic, not calibrated ecology.
#[derive(Clone, Copy, Debug)]
pub struct Profile {
    pub name: &'static str,
    pub mature_height: f64,
    pub crown_spread: f64,
    pub branch_rise: f64,
    pub leaf_length: f64,
    pub leaf_width: f64,
    pub leaf_curl: f64,
    pub leaf_color: [f32; 3],
    pub bark_color: [f32; 3],
    pub transmission: f32,
    pub roughness: f32,
    pub bark_ridge_scale: f64,
    pub growth_years: f64,
}
pub const PROFILES: [Profile; 9] = [
    Profile {
        name: "spreading_broadleaf",
        mature_height: 19.,
        crown_spread: 0.37,
        branch_rise: 0.18,
        leaf_length: 0.15,
        leaf_width: 0.085,
        leaf_curl: 0.15,
        leaf_color: [0.19, 0.36, 0.075],
        bark_color: [0.29, 0.23, 0.17],
        transmission: 0.48,
        roughness: 0.88,
        bark_ridge_scale: 29.,
        growth_years: 45.,
    },
    Profile {
        name: "upright_broadleaf",
        mature_height: 24.,
        crown_spread: 0.25,
        branch_rise: 0.34,
        leaf_length: 0.12,
        leaf_width: 0.065,
        leaf_curl: 0.21,
        leaf_color: [0.16, 0.31, 0.095],
        bark_color: [0.38, 0.36, 0.30],
        transmission: 0.39,
        roughness: 0.75,
        bark_ridge_scale: 19.,
        growth_years: 55.,
    },
    Profile { name: "white_oak", mature_height:24., crown_spread:0.40, branch_rise:0.22,
        leaf_length:0.18, leaf_width:0.12, leaf_curl:0.12, leaf_color:[0.20,0.34,0.075],
        bark_color:[0.36,0.32,0.27], transmission:0.42, roughness:0.90, bark_ridge_scale:27., growth_years:65. },
    Profile { name: "red_maple", mature_height:23., crown_spread:0.29, branch_rise:0.48,
        leaf_length:0.13, leaf_width:0.13, leaf_curl:0.16, leaf_color:[0.16,0.34,0.095],
        bark_color:[0.34,0.32,0.30], transmission:0.46, roughness:0.79, bark_ridge_scale:17., growth_years:40. },
    Profile { name: "eastern_white_pine", mature_height:30., crown_spread:0.27, branch_rise:0.06,
        leaf_length:0.11, leaf_width:0.007, leaf_curl:0.06, leaf_color:[0.13,0.26,0.17],
        bark_color:[0.30,0.25,0.21], transmission:0.25, roughness:0.89, bark_ridge_scale:31., growth_years:50. },
    Profile { name: "eastern_hemlock", mature_height:25., crown_spread:0.30, branch_rise:-0.16,
        leaf_length:0.025, leaf_width:0.006, leaf_curl:0.05, leaf_color:[0.085,0.22,0.12],
        bark_color:[0.30,0.23,0.19], transmission:0.22, roughness:0.92, bark_ridge_scale:35., growth_years:70. },
    Profile { name:"sugar_maple", mature_height:27., crown_spread:0.34, branch_rise:0.39, leaf_length:0.14, leaf_width:0.08, leaf_curl:0.14, leaf_color:[0.18, 0.33, 0.07], bark_color:[0.34, 0.31, 0.27], transmission:0.42, roughness:0.87, bark_ridge_scale:24., growth_years:55. },
    Profile { name:"black_cherry", mature_height:25., crown_spread:0.25, branch_rise:0.5, leaf_length:0.12, leaf_width:0.08, leaf_curl:0.14, leaf_color:[0.12, 0.27, 0.08], bark_color:[0.22, 0.18, 0.16], transmission:0.42, roughness:0.87, bark_ridge_scale:24., growth_years:55. },
    Profile { name:"yellow_birch", mature_height:23., crown_spread:0.32, branch_rise:0.3, leaf_length:0.11, leaf_width:0.08, leaf_curl:0.14, leaf_color:[0.22, 0.35, 0.1], bark_color:[0.48, 0.39, 0.24], transmission:0.42, roughness:0.87, bark_ridge_scale:24., growth_years:55. },
];

impl Profile {
    pub fn evergreen(self) -> bool { matches!(self.name, "eastern_white_pine" | "eastern_hemlock" | "jungle_broadleaf") }
    pub fn bark_tile(self) -> f32 { match self.name { "spreading_broadleaf" | "white_oak" => 8., "upright_broadleaf" => 9., "red_maple" | "sugar_maple" | "yellow_birch" => 10., "black_cherry" => 8., "eastern_white_pine" => 12., _ => 11. } }
}
/// Habitat weighting is artistic, with stable IDs/roots independent of species.
pub fn select(seed: u64, moisture: f64, soil: f64) -> usize {
    let x=super::math::random(seed,4);
    let diversity=super::math::random(seed,41);
    if soil>0.4 && diversity<0.34 {
        return if moisture>0.64 { if x<0.6 {8} else {6} }
            else if x<0.40 {6} else if x<0.78 {7} else {8};
    }
    if moisture>0.64 { if x<0.38 {5} else if x<0.78 {3} else if x<0.92 {2} else {4} }
    else if soil<0.4 { if x<0.68 {2} else if x<0.90 {4} else {3} }
    else if x<0.46 {2} else if x<0.76 {3} else if x<0.92 {4} else {5}
}

// Procedural evergreen broadleaf uses the existing generic branch/leaf mesh path.
// The nine authored Pennsylvania profiles and their baked atlas remain intact.
pub const JUNGLE:usize=9;
pub fn profile(index:usize)->Profile {
    if index==JUNGLE {Profile {name:"jungle_broadleaf",mature_height:32.,crown_spread:0.43,branch_rise:0.35,leaf_length:0.42,leaf_width:0.22,leaf_curl:0.2,leaf_color:[0.07,0.36,0.12],bark_color:[0.26,0.20,0.12],transmission:0.42,roughness:0.8,bark_ridge_scale:21.,growth_years:45.}}
    else {PROFILES[index]}
}
