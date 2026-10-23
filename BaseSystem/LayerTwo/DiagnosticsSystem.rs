//! Diagnostic behavior and its output live in this System, outside Host startup orchestration.
use crate::host::Host;
use serde_json::Value;
fn save(host: &Host, name: &str, value: &Value) -> Result<(), String> {
    std::fs::create_dir_all(host.output_root()).map_err(|e| e.to_string())?;
    std::fs::write(
        host.output_root().join(name),
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
pub(crate) fn report(host: &mut Host) -> Result<Value, String> {
    let value = host.invoke("NativeInventorySystem.Probe", Value::Null)?;
    save(host, "native-inventory.json", &value)?;
    Ok(value)
}
pub(crate) fn check_audio(host: &mut Host) -> Result<Value, String> {
    let value = host.invoke("JackConnectionSystem.CheckConnection", Value::Null)?;
    save(host, "jack-diagnostic.json", &value)?;
    Ok(value)
}
pub(crate) fn validate_runtime(host: &Host) -> Result<Value, String> {
    let value = host.configuration_summary();
    save(host, "runtime-validation.json", &value)?;
    Ok(value)
}
