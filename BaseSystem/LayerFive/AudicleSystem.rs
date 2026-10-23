//! Layer-five cue lifecycle. Function invocation records are separate events.
use crate::host::Host;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Preparing,
    Ready,
    Go,
    Finished,
}
impl Stage {
    fn label(self) -> &'static str {
        match self {
            Self::Preparing => "Preparing",
            Self::Ready => "Ready",
            Self::Go => "Go",
            Self::Finished => "Finished",
        }
    }
}
#[derive(Default)]
pub(crate) struct State {
    next: u64,
    stages: BTreeMap<u64, Stage>,
}

pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    if hook == "Create" {
        if host.active_audicle.is_some() {
            return Err("initial scheduler supports one active cue at a time".into());
        }
        let id = host.audicles.next + 1;
        host.active_audicle = Some(id);
        if let Err(error) = host.emit(json!({"event":"audicle_stage","stage":"Preparing"})) {
            host.active_audicle = None;
            return Err(error);
        }
        host.audicles.next = id;
        host.audicles.stages.insert(id, Stage::Preparing);
        return Ok(json!({"audicle":id}));
    }
    let id = input["audicle"].as_u64().ok_or("Audicle ID required")?;
    if host.active_audicle != Some(id) {
        return Err("Audicle is not active".into());
    }
    let current = *host.audicles.stages.get(&id).ok_or("unknown Audicle")?;
    let next = match (hook, current) {
        ("Ready", Stage::Preparing) => Stage::Ready,
        ("Go", Stage::Ready) => Stage::Go,
        ("Finish", Stage::Go) => Stage::Finished,
        ("Cancel", Stage::Preparing | Stage::Ready | Stage::Go) => {
            host.emit(json!({"event":"audicle_cancelled","previous_stage":current.label()}))?;
            host.audicles.stages.remove(&id);
            host.active_audicle = None;
            return Ok(Value::Null);
        }
        _ => return Err(format!("invalid Audicle transition {current:?} -> {hook}")),
    };
    host.emit(json!({"event":"audicle_stage","stage":next.label()}))?;
    if matches!(next, Stage::Finished) {
        host.audicles.stages.remove(&id);
        host.active_audicle = None;
    } else {
        host.audicles.stages.insert(id, next);
    }
    Ok(Value::Null)
}

impl State {
    pub(crate) fn is_go(&self, id: Option<u64>) -> bool {
        id.and_then(|id| self.stages.get(&id)) == Some(&Stage::Go)
    }
}
