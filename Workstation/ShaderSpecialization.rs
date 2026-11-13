//! One optional, precompiled variant for frames without four geometry families.
//! Marker or validation changes return an error: the renderer keeps its full
//! shader. No samples, ray budgets, geometry, lighting or display math change.

const FEATURES: [usize; 4] = [234, 240, 245, 248];

pub(crate) fn feature_counts(rows: &[[f32; 4]], expected_rows: usize) -> Option<[f32; 4]> {
    if rows.len() != expected_rows || rows.len() <= 248
        || rows[7][0] != expected_rows as f32 || rows[7][1] != 57.
        || !rows.iter().flatten().all(|v| v.is_finite())
    {
        return None;
    }
    let mut counts=FEATURES.map(|row| rows[row][0]);if rows.len()>255 && rows[255][1]==1. {counts[3]+=rows[255][2].max(1.);}
    if rows.len()>43152 {counts[3]+=rows[43152][0];}
    Some(counts)
}

#[cfg(test)]
fn controls_only_frame(rows: &[[f32; 4]], expected_rows: usize) -> bool {
    feature_counts(rows, expected_rows).is_some_and(|counts| counts == [0.; 4])
}

fn replace_once(source: &mut String, old: &str, new: &str) -> Result<(), String> {
    if source.matches(old).count() != 1 {
        return Err(format!("workstation specialization marker changed: {old}"));
    }
    *source = source.replacen(old, new, 1);
    Ok(())
}

fn no_hit(source: &mut String, name: &str) -> Result<(), String> {
    let marker = format!("fn {name}(");
    if source.matches(&marker).count() != 1 {
        return Err(format!("workstation specialization function changed: {name}"));
    }
    let start = source.find(&marker).ok_or("specialization function missing")?;
    let opening = start + source[start..].find('{').ok_or("specialization body missing")?;
    let mut depth = 1;
    let mut end = opening + 1;
    let bytes = source.as_bytes();
    while depth > 0 && end < bytes.len() {
        match bytes[end] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => (),
        }
        end += 1;
    }
    if depth != 0 { return Err("specialization body unclosed".into()); }
    source.replace_range(opening + 1..end - 1, "\nreturn GemHit(farLimit,vec3<f32>(0.0),-1);\n");
    Ok(())
}

fn validate(source: &str) -> Result<(), String> {
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|error| format!("workstation specialization: {}", error.emit_to_string(source)))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module).map_err(|error| format!("workstation specialization: {error:?}"))?;
    Ok(())
}

pub(crate) fn controls_only_source(original: &str) -> Result<String, String> {
    if original.len() > 2 * 1024 * 1024 { return Err("workstation shader exceeds specialization bound".into()); }
    validate(original)?;
    let mut source = original.to_owned();
    for name in ["gemBubbleHit", "gemAudioSphereHit", "userStyleHit", "gemTimelineHit"] {
        no_hit(&mut source, name)?;
    }
    for (old, new) in [
        ("var membrane:bool=hit.panel>=100000&&hit.panel<102048;", "var membrane:bool=false;"),
        ("var sphere:bool=hit.panel>=90000&&hit.panel<90064;", "var sphere:bool=false;"),
        ("var custom:bool=hit.panel>=200000&&hit.panel<600000;", "var custom:bool=false;"),
        ("if(inside&&current.medium>=700000&&current.medium<704096){", "if(false){"),
    ] {
        replace_once(&mut source, old, new)?;
    }
    validate(&source)?;
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    const ROWS: usize = 256;
    fn frame() -> Vec<[f32; 4]> {
        let mut rows = vec![[0.; 4]; ROWS];
        rows[7] = [ROWS as f32, 57., 0., 0.];
        rows
    }
    #[test]
    fn workstation_specialization_requires_complete_known_frame_and_exact_absence() {
        let rows = frame();
        assert!(controls_only_frame(&rows, ROWS));
        assert!(!controls_only_frame(&rows[..ROWS-1], ROWS));
        assert!(!controls_only_frame(&[], ROWS));
        for header in FEATURES {
            for value in [1., 0.25, -1., f32::NAN, f32::INFINITY] {
                let mut present = rows.clone();
                present[header][0] = value;
                assert!(!controls_only_frame(&present, ROWS), "header {header}: {value}");
            }
        }
        let mut unknown = rows.clone();
        unknown[7][0] += 1.;
        assert!(!controls_only_frame(&unknown, ROWS));
        unknown = rows;
        unknown[8][3] = f32::NAN;
        assert!(!controls_only_frame(&unknown, ROWS));
    }
    #[test]
    fn workstation_specialization_validates_and_preserves_integrator_and_ruler() {
        let original = include_str!("../Procedures/Shaders/workstation.wgsl");
        let source = controls_only_source(original).unwrap();
        for expression in ["gemRow(8).z", "gemRow(8).w", "gemRow(9).x", "gemRow(9).y",
                           "hit.panel==719999", "hit.panel==719998", "gemMicroNormal", "gemDisplay"] {
            assert_eq!(source.matches(expression).count(), original.matches(expression).count(), "{expression}");
        }
        assert!(source.contains("var membrane:bool=false;"));
        assert!(source.contains("@fragment fn fs_main"));
        assert!(controls_only_source(&original.replace("var membrane:bool=", "var membrane :bool=")).is_err());
        assert!(controls_only_source("invalid WGSL").is_err());
    }
}
