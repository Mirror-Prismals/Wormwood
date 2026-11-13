//! Physical layout shared by keyboard solids, bubble projection and picking.
//! Seven equal white-key fronts per octave; raised accidentals lie between them.
pub(crate) const WHITE_HEIGHT: f64 = 23. / 24.;
pub(crate) const BLACK_HEIGHT: f64 = 13. / 24.;
pub(crate) const BLACK_LENGTH: f64 = 89. / 145.;
const CENTERS: [f64; 12] = [0.5, 1., 1.5, 2., 2.5, 3.5, 4., 4.5, 5., 5.5, 6., 6.5];
fn center(p: i32) -> f64 {
    p.div_euclid(12) as f64 * 7. + CENTERS[p.rem_euclid(12) as usize]
}
/// Continuous MIDI row coordinates have note centers at pitch+0.5.
pub(crate) fn physical(row: f64) -> f64 {
    let pitch = row - 0.5;
    let lower = pitch.floor() as i32;
    let t = pitch - f64::from(lower);
    center(lower) + (center(lower + 1) - center(lower)) * t
}
pub(crate) fn row(position: f64) -> f64 {
    let mut lo = -128.;
    let mut hi = 256.;
    for _ in 0..48 {
        let mid = (lo + hi) * 0.5;
        if physical(mid) < position {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) * 0.5
}
pub(crate) fn height(pitch: u8) -> f64 {
    if super::piano_tuning::black(pitch) {
        BLACK_HEIGHT
    } else {
        WHITE_HEIGHT
    }
}
pub(crate) fn hit(position: f64, black_region: bool) -> Option<u8> {
    if black_region {
        for key in 0..=127 {
            if super::piano_tuning::black(key)
                && (physical(f64::from(key) + 0.5) - position).abs() <= BLACK_HEIGHT * 0.5
            {
                return Some(key);
            }
        }
    }
    (0..=127).find(|key| {
        !super::piano_tuning::black(*key)
            && (physical(f64::from(*key) + 0.5) - position).abs() <= WHITE_HEIGHT * 0.5
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_key_fronts_are_equal_and_accidentals_form_two_and_three() {
        let whites = [60, 62, 64, 65, 67, 69, 71, 72];
        for pair in whites.windows(2) {
            assert!(
                (physical(pair[1] as f64 + 0.5) - physical(pair[0] as f64 + 0.5) - 1.).abs()
                    < 1e-12
            );
            assert_eq!(height(pair[0]), WHITE_HEIGHT);
        }
        for key in [61, 63, 66, 68, 70] {
            assert_eq!(height(key), BLACK_HEIGHT);
        }
        assert!(BLACK_HEIGHT < WHITE_HEIGHT);
    }
    #[test]
    fn projection_inverts_every_fractional_row_and_preserves_physical_key_picking() {
        for i in 0..1024 {
            let q = i as f64 / 8.;
            assert!((row(physical(q)) - q).abs() < 1e-10);
        }
        assert_eq!(hit(physical(61.5), true), Some(61));
        assert_eq!(hit(physical(60.5), false), Some(60));
    }
}
