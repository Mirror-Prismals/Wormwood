//! Experimental continuous cave field inspired by an older Cardinal noise-cave
//! configuration. The live cave system uses this field within its configured bounds.
fn lattice(x: i64, y: i64, z: i64, seed: u64) -> f64 {
    let mut h = seed ^ (x as u64).wrapping_mul(0x9e3779b185ebca87)
        ^ (y as u64).wrapping_mul(0xc2b2ae3d27d4eb4f)
        ^ (z as u64).wrapping_mul(0x165667b19e3779f9);
    h ^= h >> 30; h = h.wrapping_mul(0xbf58476d1ce4e5b9);
    h ^= h >> 27; h = h.wrapping_mul(0x94d049bb133111eb); h ^= h >> 31;
    (h >> 11) as f64 / ((1u64 << 53) as f64)
}
fn noise(p: [f64;3], seed: u64) -> f64 {
    let base = p.map(|v|v.floor() as i64);
    let blend = std::array::from_fn::<_,3,_>(|i| { let t = p[i] - base[i] as f64; t*t*(3.-2.*t) });
    let mut out=0.;
    for z in 0..2 { for y in 0..2 { for x in 0..2 {
        let k=[x,y,z];
        let w=(0..3).map(|i| if k[i]==0 {1.-blend[i]} else {blend[i]}).product::<f64>();
        out += w*lattice(base[0]+x,base[1]+y,base[2]+z,seed);
    } } }
    out
}
/// Positive values describe cave air; negative values describe solid rock.
/// Coordinates are metres in the cave's local chart; depth is below local ground.
pub(crate) fn air_score(p: [f64;3], depth: f64) -> f64 {
    let t=(depth/24.).clamp(0.,1.);
    let a=noise([p[0]/64.,p[1]/48.,p[2]/64.],1337);
    let b=noise([p[0]/128.,p[1]/128.,p[2]/128.],7331);
    (a-(0.72-0.10*t)).max(b-(0.68-0.10*t))
}

// Cardinal's Depths band runs from Y=-20 to Y=-64. Keep this separate until
// its noise source and absolute-height mapping are integrated together.
#[allow(dead_code)]
fn depths_score(a: f64, b: f64, y: f64) -> f64 {
    let band = ((y + 64.) / 44.).clamp(0., 1.);
    (a - (0.57 + 0.07 * band)).max(b - (0.53 + 0.08 * band))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn depths_thresholds_match_cardinal_band_endpoints() {
        assert!((depths_score(0.64, 0.61, -20.)).abs() < 1e-12);
        assert!((depths_score(0.57, 0.53, -64.)).abs() < 1e-12);
        assert!(depths_score(0.60, 0.50, -64.) > 0.);
        assert!(depths_score(0.60, 0.50, -20.) < 0.);
    }
    #[test]
    fn field_is_repeatable_continuous_and_contains_air_and_rock() {
        let p=[21.25,-15.,42.];
        assert_eq!(air_score(p,15.),air_score(p,15.));
        let q=[p[0]+0.001,p[1],p[2]];
        assert!((air_score(p,15.)-air_score(q,15.)).abs()<0.001);
        let values:Vec<_>=(-8..=8).flat_map(|x|(-8..=8).map(move |z|air_score([x as f64*16.,-20.,z as f64*16.],20.))).collect();
        assert!(values.iter().any(|v|*v>0.) && values.iter().any(|v|*v<0.));
    }
    #[test]
    fn organic_field_can_be_meshed_with_surface_nets() {
        let mesh=super::super::surface_net::extract([-128.,-64.,-128.],[32,8,32],8.,|p|air_score(p,-p[1])).unwrap();
        assert!(!mesh.vertices.is_empty() && !mesh.triangles.is_empty());
        assert!(mesh.triangles.iter().flatten().all(|i| (*i as usize)<mesh.vertices.len()));
    }
}
