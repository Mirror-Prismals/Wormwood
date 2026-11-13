//! Bounded appearances for the spatial audio spheres. Signal analysis and mixer
//! gestures belong to their control owners; this module only validates and packs
//! already projected ellipsoids for the shared native optical renderer.
use super::{bubble_notes, engraving};
use serde_json::Value;

pub(crate) const HEADER: usize = 240;
pub(crate) const BASE: usize = bubble_notes::DYNAMIC_END;
pub(crate) const STRIDE: usize = 4;
pub(crate) const MAX_SPHERES: usize = 64;
pub(crate) const END: usize = BASE + STRIDE * MAX_SPHERES;
pub(crate) const PANEL_ID_BASE: usize = 90000;

fn number(value: &Value, low: f64, high: f64, label: &str) -> Result<f32, String> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && (low..=high).contains(v))
        .map(|v| v as f32)
        .ok_or_else(|| format!("audio sphere {label} must be finite and within {low}..={high}"))
}

fn vector<const N: usize>(
    value: &Value,
    low: f64,
    high: f64,
    label: &str,
) -> Result<[f32; N], String> {
    let source = value
        .as_array()
        .filter(|a| a.len() == N)
        .ok_or_else(|| format!("audio sphere {label} requires {N} numbers"))?;
    let mut result = [0.; N];
    for (target, value) in result.iter_mut().zip(source) {
        *target = number(value, low, high, label)?;
    }
    Ok(result)
}

/// Append inside unused dynamic engraving storage, preserving every prior
/// optical and immutable offset. Validation is atomic with respect to rows.
pub(crate) fn append(rows: &mut Vec<[f32; 4]>, presentation: &Value) -> Result<(), String> {
    if END > engraving::BASE + engraving::ROWS || rows.len() < END || rows.len() < HEADER + 4 {
        return Err("audio spheres require the complete dynamic workstation frame".into());
    }
    if presentation.is_null() {
        rows[HEADER..HEADER + 4].fill([0.; 4]);
        rows[BASE..END].fill([0.; 4]);
        return Ok(());
    }
    let time = number(&presentation["time_seconds"], 0., 1e12, "animation time")?;
    let clip = vector::<4>(&presentation["clip_rect"], -1000., 1000., "clip rectangle")?;
    if clip[0] >= clip[2] || clip[1] >= clip[3] {
        return Err("audio sphere clip rectangle must have positive dimensions".into());
    }
    let instances = presentation["spheres"]
        .as_array()
        .ok_or("audio sphere presentation requires a spheres array")?;
    if instances.len() > MAX_SPHERES {
        return Err(format!(
            "audio sphere presentation exceeds {MAX_SPHERES} sources"
        ));
    }
    let mut dynamic = [[0.; 4]; STRIDE * MAX_SPHERES];
    let mut minimum = [f32::INFINITY; 3];
    let mut maximum = [f32::NEG_INFINITY; 3];
    for (index, instance) in instances.iter().enumerate() {
        let center = vector::<3>(&instance["center"], -1000., 1000., "center")?;
        let radii = vector::<3>(&instance["radii"], 0.000001, 100., "radii")?;
        let color = vector::<3>(&instance["color_linear_rgb"], 0., 1., "linear color")?;
        let selected = match instance.get("selected") {
            Some(value) => value
                .as_bool()
                .ok_or("audio sphere selected must be boolean")?,
            None => false,
        };
        let activity = match instance.get("activity") {
            Some(value) => number(value, 0., 1., "activity")?,
            None => 0.,
        };
        let at = index * STRIDE;
        dynamic[at] = [
            center[0],
            center[1],
            center[2],
            if selected { 1. } else { 0. },
        ];
        dynamic[at + 1] = [radii[0], radii[1], radii[2], activity];
        dynamic[at + 2] = [color[0], color[1], color[2], 0.9];
        dynamic[at + 3] = [color[0], color[1], color[2], 0.2];
        for axis in 0..3 {
            minimum[axis] = minimum[axis].min(center[axis] - radii[axis]);
            maximum[axis] = maximum[axis].max(center[axis] + radii[axis]);
        }
    }
    let mut bounds = [[0.; 4]; 2];
    if !instances.is_empty() {
        for axis in 0..3 {
            bounds[0][axis] = (minimum[axis] + maximum[axis]) * 0.5;
            bounds[1][axis] = (maximum[axis] - minimum[axis]) * 0.5 + 0.000001;
        }
    }
    rows[BASE..END].copy_from_slice(&dynamic);
    rows[HEADER] = [instances.len() as f32, BASE as f32, STRIDE as f32, time];
    rows[HEADER + 1] = clip;
    rows[HEADER + 2] = bounds[0];
    rows[HEADER + 3] = bounds[1];
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sphere() -> Value {
        json!({"center":[0.1,-0.2,-0.1], "radii":[0.08,0.04,0.04],
               "color_linear_rgb":[0.2,0.4,0.8], "selected":true, "activity":0.5})
    }

    fn presentation(count: usize) -> Value {
        json!({"time_seconds":12.5,"clip_rect":[-1.,-0.5,1.,0.5],
               "spheres":(0..count).map(|_|sphere()).collect::<Vec<_>>()})
    }

    #[test]
    fn audio_sphere_capacity_preserves_bubbles_headers_and_immutable_offsets() {
        let mut rows = vec![[123.; 4]; engraving::BASE + engraving::ROWS];
        append(&mut rows, &presentation(MAX_SPHERES)).unwrap();
        assert_eq!(rows.len(), engraving::BASE + engraving::ROWS);
        assert_eq!(BASE, bubble_notes::DYNAMIC_END);
        assert_eq!(rows[HEADER - 1], [123.; 4]);
        assert_eq!(rows[HEADER + 4], [123.; 4]);
        assert_eq!(rows[BASE - 1], [123.; 4]);
        assert_eq!(rows[END], [123.; 4]);
        assert_eq!(rows[HEADER], [64., BASE as f32, 4., 12.5]);
        assert_eq!(rows[BASE + 2], [0.2, 0.4, 0.8, 0.9]);
        assert_eq!(rows[BASE + 3][3], 0.2);
        assert!(PANEL_ID_BASE + MAX_SPHERES < bubble_notes::PANEL_ID_BASE);
        for axis in 0..3 {
            assert!(
                rows[HEADER + 2][axis] - rows[HEADER + 3][axis]
                    <= rows[BASE][axis] - rows[BASE + 1][axis]
            );
            assert!(
                rows[HEADER + 2][axis] + rows[HEADER + 3][axis]
                    >= rows[BASE][axis] + rows[BASE + 1][axis]
            );
        }
    }

    #[test]
    fn audio_sphere_invalid_packets_never_partially_modify_a_frame() {
        let original = vec![[123.; 4]; engraving::BASE + engraving::ROWS];
        let mut bad_radius = presentation(2);
        bad_radius["spheres"][1]["radii"][0] = json!(0.);
        let mut bad_color = presentation(1);
        bad_color["spheres"][0]["color_linear_rgb"][1] = json!(1.01);
        let mut bad_clip = presentation(1);
        bad_clip["clip_rect"] = json!([1., 0., -1., 0.5]);
        let mut bad_selected = presentation(1);
        bad_selected["spheres"][0]["selected"] = json!(1);
        for invalid in [
            bad_radius,
            bad_color,
            bad_clip,
            bad_selected,
            presentation(MAX_SPHERES + 1),
        ] {
            let mut rows = original.clone();
            assert!(append(&mut rows, &invalid).is_err());
            assert_eq!(rows, original);
        }
    }

    #[test]
    fn audio_sphere_absence_removes_stale_instances_without_changing_background() {
        let mut rows = vec![[123.; 4]; engraving::BASE + engraving::ROWS];
        append(&mut rows, &presentation(2)).unwrap();
        append(&mut rows, &Value::Null).unwrap();
        assert!(rows[HEADER..HEADER + 4].iter().all(|r| *r == [0.; 4]));
        assert!(rows[BASE..END].iter().all(|r| *r == [0.; 4]));
        assert!(rows[..HEADER].iter().all(|r| *r == [123.; 4]));
        append(&mut rows, &presentation(0)).unwrap();
        assert_eq!(rows[HEADER][0], 0.);
        assert!(rows.iter().flatten().all(|v| v.is_finite()));
    }
}
