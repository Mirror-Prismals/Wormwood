//! Cardinal's EDO choices with a physical-key layout independent of tuning.
//! C4 (physical MIDI key 60) is the reference. Each successive key advances
//! one EDO degree; canonical document notes remain MIDI anchors plus cents.
use serde_json::{Value, json};
pub(crate) const PRESETS: [u16; 10] = [5, 7, 12, 17, 19, 22, 24, 31, 53, 72];
pub(crate) const SCALES: [&str; 7] = [
    "CHROMATIC",
    "MAJOR",
    "HARM MIN",
    "MELOD MIN",
    "HUNG MIN",
    "NEAP MAJ",
    "DBL HARM",
];
pub(crate) fn edo(v: &Value) -> Result<u16, String> {
    match v.as_u64() {
        Some(n) if (1..=4096).contains(&n) => Ok(n as u16),
        None if v.is_null() => Ok(12),
        _ => Err("EDO must be an integer within 1..4096".into()),
    }
}
pub(crate) fn semitones(key: u8, edo: u16) -> f64 {
    (60. + (f64::from(key) - 60.) * 12. / f64::from(edo)).clamp(0., 127.)
}
pub(crate) fn encode(key: u8, edo: u16) -> (u8, f64) {
    let p = semitones(key, edo);
    let anchor = p.round() as u8;
    (anchor, (p - f64::from(anchor)) * 100.)
}
pub(crate) fn key(anchor: f64, cents: f64, edo: u16) -> u8 {
    (60. + (anchor + cents / 100. - 60.) * f64::from(edo) / 12.)
        .round()
        .clamp(0., 127.) as u8
}
pub(crate) fn frequency(pitch: u8, cents: f64) -> f64 {
    440. * 2f64.powf((f64::from(pitch) + cents / 100. - 69.) / 12.)
}
pub(crate) fn cents(v: &Value, pitch: u8) -> Result<f64, String> {
    let c = v
        .as_f64()
        .unwrap_or(if v.is_null() { 0. } else { f64::NAN });
    if c.is_finite() && (0. ..=127.).contains(&(f64::from(pitch) + c / 100.)) {
        Ok(c)
    } else {
        Err("note tuning must produce a finite MIDI pitch within 0..127".into())
    }
}
pub(crate) fn note(n: &Value, edo: u16) -> Value {
    let mut out = n.clone();
    let (pitch, cents) = encode(n["pitch"].as_u64().unwrap_or(60) as u8, edo);
    out["pitch"] = json!(pitch);
    if cents.abs() > 1e-9 {
        out["tuning_cents"] = json!(cents);
    } else {
        out.as_object_mut().unwrap().remove("tuning_cents");
    }
    out
}
pub(crate) fn black(key: u8) -> bool {
    [1, 3, 6, 8, 10].contains(&(key % 12))
}
pub(crate) fn in_scale(key: u8, scale: u8, root: u8) -> bool {
    let intervals = match scale {
        1 => [0, 2, 4, 5, 7, 9, 11],
        2 => [0, 2, 3, 5, 7, 8, 11],
        3 => [0, 2, 3, 5, 7, 9, 11],
        4 => [0, 2, 3, 6, 7, 8, 11],
        5 => [0, 1, 4, 5, 7, 9, 11],
        6 => [0, 1, 4, 5, 7, 8, 11],
        _ => return true,
    };
    intervals.contains(&((key + 12 - root % 12) % 12))
}
pub(crate) fn snap_key(key: u8, scale: u8, root: u8, edo: u16) -> u8 {
    if edo != 12 {
        return key;
    }
    for d in 0..12 {
        if key >= d && in_scale(key - d, scale, root) {
            return key - d;
        }
        if key <= 127 - d && in_scale(key + d, scale, root) {
            return key + d;
        }
    }
    key
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tuning_is_a_clock_independent_physical_keyboard() {
        for edo in PRESETS {
            let (a, c) = encode(61, edo);
            assert!(
                (frequency(a, c) / frequency(60, 0.) - 2f64.powf(1. / f64::from(edo))).abs()
                    < 1e-10
            );
            assert_eq!(key(f64::from(a), c, edo), 61);
            assert!(black(61));
        }
        assert_eq!(encode(69, 12), (69, 0.));
        assert!(edo(&json!(4097)).is_err());
    }
    #[test]
    fn accidentals_are_physical_not_tuning_degrees() {
        assert!(black(61));
        assert!(!black(62));
        assert_eq!(snap_key(61, 1, 0, 12), 60);
        assert_eq!(snap_key(61, 1, 0, 19), 61);
    }
}
#[cfg(test)]
mod playback_tests {
    use super::*;
    #[test]
    fn nineteen_steps_double_the_frequency_and_black_keys_still_have_fractional_pitch() {
        let (a, c) = encode(79, 19);
        assert!((frequency(a, c) / frequency(60, 0.) - 2.).abs() < 1e-10);
        let (a, c) = encode(61, 19);
        assert!(c.abs() > 1.);
        assert!((frequency(a, c) - frequency(61, 0.)).abs() > 1.);
        assert!(black(61));
    }
}
