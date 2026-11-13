//! User-approved macOS workstation settings, shared by normal CLI and app launches.
//! Environment values are optional diagnostic overrides; no process-global mutation.
const APPROVED: &[&str] = &[
    "WORMWOOD_WORKSTATION_GEM_COMPOSITION",
    "WORMWOOD_WORKSTATION_COMPONENT_MOTION",
    "WORMWOOD_WORKSTATION_CHANGED_UPLOADS",
    "WORMWOOD_WORKSTATION_PRERENDER",
    "WORMWOOD_WORKSTATION_DEFER_FLOOR",
    "WORMWOOD_WORKSTATION_MERGE_STATIC",
    "WORMWOOD_WORKSTATION_PRESENTATION_KEYS",
    "WORMWOOD_WORKSTATION_FRAME_OVERLAP",
    "WORMWOOD_WORKSTATION_DISPLAY_CADENCE",
    "WORMWOOD_WORKSTATION_SHARED_SNAPSHOT",
    "WORMWOOD_WORKSTATION_COMPACT_ATLAS",
    "WORMWOOD_WORKSTATION_INCREMENTAL_BAKE",
    "WORMWOOD_WORKSTATION_PLAIN_BACKDROP",
    "WORMWOOD_WORKSTATION_PREVIEW_CONTROLS",
    // Original pixels validated across camera/panel motion and live meter levels.
    "WORMWOOD_WORKSTATION_CAMERA_CACHE",
    "WORMWOOD_WORKSTATION_CAMERA_SPLIT",
    "WORMWOOD_WORKSTATION_METER_SPLIT",
    "WORMWOOD_COMBINED_DISPLAY_CADENCE",
    "WORMWOOD_WORKSTATION_METER_LEDGER",
    "WORMWOOD_WORKSTATION_METER_LEDGER_PLAYHEAD",
    "WORMWOOD_WORKSTATION_LEDGER_CAMERA_RGB",
];

pub(crate) fn enabled(name: &str) -> bool {
    resolve(name, cfg!(target_os = "macos"), &|key| std::env::var(key).ok())
}

fn resolve(name: &str, macos: bool, value: &impl Fn(&str) -> Option<String>) -> bool {
    if let Some(explicit) = value(name) { return explicit == "1"; }
    if !macos || !APPROVED.contains(&name) { return false; }
    match name {
        "WORMWOOD_WORKSTATION_PRERENDER" => value("WORMWOOD_RUBY_RESPONSE").as_deref() != Some("0"),
        "WORMWOOD_WORKSTATION_COMPACT_ATLAS" =>
            resolve("WORMWOOD_WORKSTATION_MERGE_STATIC", macos, value)
                && value("WORMWOOD_WORKSTATION_DEPTH_FIRST").as_deref() != Some("1"),
        "WORMWOOD_WORKSTATION_INCREMENTAL_BAKE" =>
            resolve("WORMWOOD_WORKSTATION_PRERENDER", macos, value)
                && resolve("WORMWOOD_WORKSTATION_MERGE_STATIC", macos, value)
                && resolve("WORMWOOD_WORKSTATION_COMPACT_ATLAS", macos, value)
                && value("WORMWOOD_WORKSTATION_DEPTH_FIRST").as_deref() != Some("1")
                && value("WORMWOOD_WORKSTATION_PIXEL_TRANSPORT").as_deref() != Some("1"),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approved_macos_defaults_match_the_accepted_profile_without_environment() {
        for name in APPROVED { assert!(resolve(name, true, &|_| None), "{name}"); }
        for name in ["WORMWOOD_WORKSTATION_DEPTH_FIRST", "WORMWOOD_WORKSTATION_BAKE_SETTLE_REPLAY",
            "WORMWOOD_WORKSTATION_DIAGNOSTIC_OFFSCREEN", "WORMWOOD_WORKSTATION_FINAL_RETENTION",
            "WORMWOOD_WORKSTATION_INPUT_VALIDATION", "WORMWOOD_WORKSTATION_BENCHMARK_PACED"] {
            assert!(!resolve(name, true, &|_| None), "diagnostic enabled: {name}");
        }
    }
    #[test]
    fn other_platforms_and_explicit_overrides_keep_their_selection() {
        for name in APPROVED {
            assert!(!resolve(name, false, &|_| None));
            assert!(!resolve(name, true, &|key| (key == *name).then(|| "0".into())));
            assert!(resolve(name, false, &|key| (key == *name).then(|| "1".into())));
            assert!(!resolve(name, true, &|key| (key == *name).then(|| "invalid".into())));
        }
    }
    #[test]
    fn reference_and_alternate_diagnostic_overrides_do_not_inherit_incompatible_defaults() {
        for (key, val) in [("WORMWOOD_RUBY_RESPONSE", "0"), ("WORMWOOD_WORKSTATION_PRERENDER", "0"),
            ("WORMWOOD_WORKSTATION_MERGE_STATIC", "0"), ("WORMWOOD_WORKSTATION_COMPACT_ATLAS", "0"),
            ("WORMWOOD_WORKSTATION_DEPTH_FIRST", "1"), ("WORMWOOD_WORKSTATION_PIXEL_TRANSPORT", "1")] {
            assert!(!resolve("WORMWOOD_WORKSTATION_INCREMENTAL_BAKE", true,
                &|name| (name == key).then(|| val.into())), "{key}");
        }
        assert!(!resolve("WORMWOOD_WORKSTATION_PRERENDER", true,
            &|name| (name == "WORMWOOD_RUBY_RESPONSE").then(|| "0".into())));
    }
}
