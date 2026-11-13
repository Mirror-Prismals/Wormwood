//! Authored cave rock palette, separate from stone's mechanical material law.
//! Linear elevations match the existing Depths field; colors are sRGB appearance.
pub(crate) const BROWN: [f32;3] = [0.2836261,0.20529686,0.12704249];
pub(crate) const BLUE: [f32;3] = [0.09831836,0.15777506,0.2661084];
/// Grey remains an available future Nightworld palette, not selected for caves.
#[allow(dead_code)]
pub(crate) const NIGHTWORLD_GREY: [f32;3] = [0.42,0.44,0.46];
pub(crate) fn kind(point:[f64;3])->i8 {
    if super::underground::local(point)[1] <= -20. {1} else {0}
}
pub(crate) fn base_color(point:[f64;3])->[f32;3] {if kind(point)==1 {BLUE} else {BROWN}}
pub(crate) fn color(point:[f64;3])->[f32;3] {super::cave_formations::mineral_color(point).unwrap_or_else(||base_color(point))}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stone_uses_brown_above_and_original_atlas_blue_below_depths_roof() {
        let upper=super::super::underground::world([20.,-12.,-10.]);
        let lower=super::super::underground::world([20.,-30.,-10.]);
        assert_eq!(color(upper),BROWN);assert_eq!(color(lower),BLUE);
        assert!(BROWN[0]>BROWN[1] && BROWN[1]>BROWN[2]);
        assert!(BLUE[2]>BLUE[1] && BLUE[1]>BLUE[0]);
        assert_eq!(crate::physics::matter::world_field::substance(upper).color_at(upper),BROWN);
        assert_eq!(crate::physics::matter::world_field::substance(lower).color_at(lower),BLUE);
    }
}
