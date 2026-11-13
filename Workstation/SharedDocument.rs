//! Serialized control/preparation access. This mutex is never acquired by the
//! native audio callback. Graphics only retains owned snapshots after unlocking.
use super::{SharedSnapshot, State};
use serde_json::Value;
use std::sync::{Arc, Mutex};
struct SerializedState(State);
impl State {
    pub(crate) fn document_revision(&self) -> u64 {
        self.revision
    }
}
// SAFETY: all access is exclusive through Owner. Native plugin control/UI calls
// remain on their creating thread; the preparation worker only runs document
// capture/render operations. The worker returns its Owner in its join result,
// and Host joins it before releasing the main Owner and native editors.
unsafe impl Send for SerializedState {}
#[derive(Clone, Default)]
pub(crate) struct Owner(Arc<Mutex<SerializedState>>);
impl Default for SerializedState {
    fn default() -> Self {
        Self(State::default())
    }
}
impl Owner {
    pub fn with<R>(&self, f: impl FnOnce(&mut State) -> Result<R, String>) -> Result<R, String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| "audio document owner was poisoned")?;
        f(&mut state.0)
    }
    pub fn dispatch(&self, hook: &str, input: &Value) -> Result<Value, String> {
        self.with(|s| super::dispatch(s, hook, input))
    }
    pub fn shared_snapshot(&self) -> Result<SharedSnapshot, String> {
        self.with(|s| Ok(s.shared_snapshot()))
    }
    pub fn export_active(&self) -> bool {
        self.with(|s| Ok(s.export_active())).unwrap_or(true)
    }
}
