//! Continuous impact and wake records for water appearance and declared consumers.
use super::{math::*, soft_body::Parcel};
#[derive(Clone, Copy, Debug)]
pub(crate) struct Emitter {
    pub position: V,
    pub velocity: V,
    pub strength: f64,
}
pub(crate) fn emitters(parcels: &[Parcel]) -> Vec<Emitter> {
    parcels
        .iter()
        .filter(|p| p.covered && p.impact > 0.05)
        .take(128)
        .map(|p| Emitter {
            position: p.position,
            velocity: p.velocity,
            strength: p.impact.clamp(0., 1.),
        })
        .collect()
}
