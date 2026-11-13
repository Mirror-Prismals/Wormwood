//! Registered non-Audicle continuous water operation.
use crate::host::Host;
use serde_json::Value;
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    crate::water::runtime::soft_water(host, hook, input)
}
