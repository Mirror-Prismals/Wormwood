//! Runtime selection remains separate from retention and circuit breaking.
use crate::host::Host;
use serde_json::Value;
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    match hook {
        "State" => Ok(host.registry_state()),
        "SetEnabled" => host.set_system_enabled(
            input["system"].as_str().ok_or("System required")?,
            input["enabled"]
                .as_bool()
                .ok_or("enabled boolean required")?,
        ),
        _ => Err("unknown registry hook".into()),
    }
}
