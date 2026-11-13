//! Local cohesive soil law. OGC supplies contact, not material strength.
//! These are explicit, uncalibrated material parameters in SI units.
use super::math::*;

#[derive(Clone, Copy)]
pub(super) struct Cohesion {
    pub tension_pa: f32,
    pub shear_pa: f32,
    pub friction: f32,
    pub separation_m: f32,
}
impl Cohesion {
    pub const LOAM: Self = Self {
        tension_pa: 7500.,
        shear_pa: 7500.,
        friction: 0.6,
        separation_m: 0.018,
    };

    /// `force` is the restoring force on the second side of the interface.
    /// Compression supports friction; it must never count as tensile failure.
    pub fn fails(self, separation: V, force: V, normal: V, area_m2: f32) -> bool {
        let opening = dot(separation, normal);
        let slip = length(sub(separation, scale(normal, opening)));
        let normal_force = dot(force, normal);
        let tension = (-normal_force).max(0.);
        let compression = normal_force.max(0.);
        let shear = length(sub(force, scale(normal, normal_force)));
        let tensile_capacity = self.tension_pa * area_m2;
        let shear_capacity = self.shear_pa * area_m2 + self.friction * compression;
        opening.max(0.).hypot(slip) > self.separation_m
            || (opening.max(0.).hypot(slip) > 0.00005
                && (tension / tensile_capacity.max(1e-8)).powi(2)
                    + (shear / shear_capacity.max(1e-8)).powi(2)
                    > 1.)
    }
}

/// A material face frame follows its deformation. A rest vector expressed in
/// this frame rotates with the earth, instead of tethering soil to world axes.
pub(super) fn face_frame(face: [V; 3]) -> [V; 3] {
    let tangent = unit(sub(face[1], face[0]));
    let normal = unit(cross(tangent, sub(face[2], face[0])));
    [tangent, cross(normal, tangent), normal]
}
pub(super) fn coordinates(vector: V, frame: [V; 3]) -> V {
    frame.map(|axis| dot(vector, axis))
}
pub(super) fn transport(vector: V, frame: [V; 3]) -> V {
    (0..3).fold([0.; 3], |out, i| add(out, scale(frame[i], vector[i])))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compression_does_not_tear_soil_and_supports_shear() {
        let soil = Cohesion::LOAM;
        let n = [0., 1., 0.];
        assert!(!soil.fails([0., -0.01, 0.], [0., 10000., 0.], n, 0.01));
        assert!(soil.fails([0., 0.001, 0.], [0., -100., 0.], n, 0.01));
        assert!(soil.fails([0.001, 0., 0.], [-100., 0., 0.], n, 0.01));
        assert!(!soil.fails([0.001, -0.001, 0.], [-100., 100., 0.], n, 0.01));
    }
    #[test]
    fn subdividing_a_face_preserves_its_strength() {
        let soil = Cohesion::LOAM;
        for load in [50., 100.] {
            let full = soil.fails([0.001, 0., 0.], [-load, 0., 0.], [0., 1., 0.], 0.01);
            let quarter = soil.fails([0.001, 0., 0.], [-load / 4., 0., 0.], [0., 1., 0.], 0.0025);
            assert_eq!(full, quarter);
        }
    }
    #[test]
    fn cohesive_rest_separation_rotates_with_its_material() {
        let face = [[0., 0., 0.], [1., 0., 0.], [0., 0., 1.]];
        let rest = [0.002, 0.006, -0.003];
        let local = coordinates(rest, face_frame(face));
        let rotate = |p: V| [-p[1], p[0], p[2]];
        let moved = face.map(|p| add(rotate(p), [3., 2., -4.]));
        assert!(length(sub(transport(local, face_frame(moved)), rotate(rest))) < 1e-7);
    }
}
