//! Bounded ambient discovery against accepted water surfaces and moving parcels.
//! No inferred ocean beneath unknown or dry terrain.
use super::{flow::Kind, hydrology::State, math::*, ocean};
use serde_json::{Value, json};

fn retain(best: &mut Option<(V, f64, f64)>, eye: V, point: V, strength: f64) {
    let distance = len(sub(point, eye));
    if distance <= 96. && strength > 0. && best.is_none_or(|(_, d, _)| distance < d) {
        *best = Some((point, distance, strength));
    }
}
pub(crate) fn discover(
    state: &State, point: V, mean: f64, wave: &ocean::State,
    river_speed: f64, parcels: &Value, material: &Value,
) -> Value {
    let up = unit(point);
    let frame = Frame::radial(point, [0., 0., -1.]);
    let mut probes = vec![point];
    for radius in [4., 12., 28., 56., 92.] {
        for i in 0..8 {
            let angle = i as f64 * std::f64::consts::TAU / 8.;
            probes.push(frame.world([radius * angle.cos(), 0., radius * angle.sin()]));
        }
    }
    // Exact projections keep thin authored rivers from falling between probes.
    for river in &state.bodies.rivers {
        let mut closest = None;
        for pair in river.points.windows(2) {
            let d = sub(pair[1], pair[0]);
            let t = (dot(sub(point, pair[0]), d) / dot(d, d).max(1e-12)).clamp(0., 1.);
            let p = add(pair[0], mul(d, t));
            let distance = len(sub(p, point));
            if distance <= 96. && closest.is_none_or(|(_, old)| distance < old) {
                closest = Some((p, distance));
            }
        }
        if let Some((p, _)) = closest { probes.push(p); }
    }
    let mut surf = None;
    let mut flow = None;
    let mut submerged = false;
    for (index, probe) in probes.iter().enumerate() {
        if state.material_domains.iter().any(|d| d.distance(*probe) <= 0.) { continue; }
        let Some(wet) = state.bodies.surface(*probe, mean, wave, state) else { continue; };
        let source = mul(unit(*probe), crate::terrain::RADIUS + wet.surface_height);
        if index == 0 {
            let height = len(point) - crate::terrain::RADIUS;
            submerged = height < wet.surface_height - 0.15 && height >= wet.bed_height;
        }
        match wet.kind {
            Kind::Ocean => retain(&mut surf, point, source, 1.),
            Kind::River => retain(&mut flow, point, source, (river_speed / 1.3).clamp(0., 2.)),
            _ => {} // A still lake does not create surf or running-water noise.
        }
    }
    for row in parcels["parcels"].as_array().into_iter().flatten() {
        if row["covered"] != true || row["resting"] == true { continue; }
        let Ok(p) = super::types::vector(&row["position"]) else { continue; };
        let Ok(v) = super::types::vector(&row["velocity"]) else { continue; };
        let speed = len(v);
        if speed > 0.08 {
            retain(&mut flow, point, p, (speed / 1.3).clamp(0., 2.));
        }
    }
    for row in material["audio_sources"].as_array().into_iter().flatten() {
        if let (Ok(p), Some(speed)) = (super::types::vector(&row["position"]), row["speed"].as_f64()) {
            if speed > 0.08 { retain(&mut flow, point, p, (speed / 1.3).clamp(0., 2.)); }
        }
    }
    let packet = |source: Option<(V, f64, f64)>| source.map(|(position, distance, strength)|
        json!({"position":position,"distance":distance,"strength":strength}));
    let _ = up;
    json!({"ocean":packet(surf),"flow":packet(flow),"submerged":submerged,"probe_count":probes.len()})
}

#[cfg(test)]
mod audio_source_tests {
    use super::*;
    #[test]
    fn unknown_water_never_invents_ocean_or_submersion() {
        let result=discover(&State::default(),[0.,crate::terrain::RADIUS+2.,0.],0.,&ocean::State::default(),1.3,&json!({"parcels":[]}),&json!({"audio_sources":[]}));
        assert!(result["ocean"].is_null());assert!(result["flow"].is_null());assert_eq!(result["submerged"],false);
        assert_eq!(result["probe_count"],41);
    }
}
