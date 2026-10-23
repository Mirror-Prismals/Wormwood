//! Explicitly unavailable until native iOS surface clients are integrated.
use crate::base_entity::Entity;
use serde_json::Value;
use std::path::Path;
pub(super) struct Session;
impl Session {
    pub(super) fn prepare(_: &Path, _: &str, _: &Entity) -> Result<Self, String> {
        Err("iOS native graphics cue is not integrated yet".into())
    }
    pub(super) fn info(&self) -> Value {
        Value::Null
    }
    pub(super) fn present(
        &mut self,
        _: u32,
        _: impl FnMut(u32) -> Result<(), String>,
    ) -> Result<Value, String> {
        Err("iOS presentation unavailable".into())
    }
    pub(super) fn finish(&mut self) -> Result<(), String> {
        Ok(())
    }
}
