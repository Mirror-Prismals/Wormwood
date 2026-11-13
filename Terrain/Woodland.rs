//! Shared Pennsylvania-inspired tree habitat selection. Artistic thresholds,
//! not a surveyed ecological map. Ground substance lives in matter/world_field.
use super::Point;
pub(crate) const BYTES: &[u8] = include_bytes!("../Procedures/Assets/Woodland/materials.rgba");
pub(crate) const TILE_COUNT: usize = 48;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Habitat {
    DryRidge,
    Woodland,
    CoolRavine,
    Meadow,
    StreamBank,
}
pub(crate) fn habitat(normal: Point) -> Habitat {
    let s = super::sample(normal);
    if super::drainage::water(normal).is_some() {
        return Habitat::StreamBank;
    }
    if s.soil_depth < 0.4 {
        return Habitat::DryRidge;
    }
    if s.moisture > 0.72 {
        return Habitat::CoolRavine;
    }
    let (x, z) = super::coordinates(normal);
    if s.soil_depth > 0.85
        && s.moisture < 0.65
        && (x / 720. + 0.6 * (z / 530.).sin()).sin() * (z / 890.).cos() > 0.50
    {
        Habitat::Meadow
    } else {
        Habitat::Woodland
    }
}
pub(crate) fn density(normal: Point) -> f64 {
    if !super::regions::forest(normal) { return 0.; }
    match habitat(normal) {
        Habitat::Meadow => 0.12,
        Habitat::StreamBank => 0.15,
        Habitat::DryRidge => 0.90,
        _ => 1.,
    }
}
