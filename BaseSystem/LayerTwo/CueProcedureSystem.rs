//! Bounded procedure data loading. No function names or execution routes come from these files.
use crate::{base_entity::Entity, host::Host};
use serde_json::{Value, json};
use std::{fs::File, io::Read};
const MAX_BYTES: u64 = 256 * 1024;
pub(crate) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
pub(crate) fn validate(value: &Value) -> Result<(), String> {
    let object = value.as_object().ok_or("cue must be an object")?;
    if object
        .keys()
        .any(|k| !matches!(k.as_str(), "schema_version" | "visual" | "sound"))
    {
        return Err("unknown cue field".into());
    }
    if value["schema_version"] != 1 {
        return Err("unsupported cue schema_version".into());
    }
    let visual = &value["visual"];
    if visual["kind"] != "visual"
        || visual.as_object().is_none_or(|v| {
            v.keys()
                .any(|k| !matches!(k.as_str(), "kind" | "vertices" | "shader"))
        })
    {
        return Err("cue visual must be visual Entity data".into());
    }
    Entity::from_spec(1, visual)?;
    let sound = &value["sound"];
    if !sound.is_null() {
        if sound["kind"] != "sound"
            || sound
                .as_object()
                .is_none_or(|v| v.keys().any(|k| !matches!(k.as_str(), "kind" | "source")))
        {
            return Err("cue sound must be sound Entity data".into());
        }
        Entity::from_spec(1, sound)?;
    }
    Ok(())
}
pub(crate) fn load(host: &mut Host, input: &Value) -> Result<Value, String> {
    let name = input["procedure"]
        .as_str()
        .filter(|s| valid_name(s))
        .ok_or("invalid cue procedure name")?;
    let directory = host
        .root()
        .join("Procedures/Cues")
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let path = directory
        .join(format!("{name}.json"))
        .canonicalize()
        .map_err(|e| format!("cue {name}: {e}"))?;
    if !path.starts_with(&directory) {
        return Err("cue path escapes Procedures/Cues".into());
    }
    let mut bytes = Vec::new();
    File::open(&path)
        .map_err(|e| e.to_string())?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("cue exceeds 256 KiB preparation budget".into());
    }
    let definition: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    validate(&definition)?;
    host.emit(
        json!({"event":"procedure_loaded","procedure":name,"schema_version":1,"bytes":bytes.len()}),
    )?;
    Ok(definition)
}
