//! Layer-five events retain ownership, parent identity and terminal outcomes.
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
    owners: BTreeMap<u64, String>,
    parents: BTreeMap<u64, Option<u64>>,
}
impl State {
    pub(crate) fn is_go(&self, id: Option<u64>) -> bool {
        id.and_then(|id| self.stages.get(&id)) == Some(&Stage::Go)
    }
    pub(crate) fn live(&self, id: u64) -> bool {
        self.stages.contains_key(&id)
    }
    pub(crate) fn owned_by(&self, system: &str) -> bool {
        self.owners.values().any(|s| s == system)
    }
    pub(crate) fn count(&self) -> usize {
        self.stages.len()
    }
}
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let owner = host.invoking_system().to_owned();
    if hook == "Create" {
        if host.audicles.stages.len() >= 128 {
            return Err("concurrent Audicle budget reached".into());
        }
        let parent = host.active_audicle;
        let id = host.audicles.next + 1;
        host.active_audicle = Some(id);
        if let Err(error)=host.emit(json!({"event":"audicle_stage","stage":"Preparing","parent_audicle":parent,"owner":owner})) {
            host.active_audicle=parent;
            return Err(error);
        }
        host.audicles.next = id;
        host.audicles.stages.insert(id, Stage::Preparing);
        host.audicles.owners.insert(id, owner);
        host.audicles.parents.insert(id, parent);
        return Ok(json!({"audicle":id}));
    }
    let id = input["audicle"].as_u64().ok_or("Audicle ID required")?;
    let current = *host.audicles.stages.get(&id).ok_or("unknown Audicle")?;
    if host.audicles.owners.get(&id) != Some(&owner) {
        return Err("Audicle belongs to another System".into());
    }
    if hook == "Resume" {
        host.active_audicle = Some(id);
        return Ok(Value::Null);
    }
    if !matches!(hook, "Cancel" | "Fail" | "Finish") && host.active_audicle != Some(id) {
        return Err("Audicle is not active".into());
    }
    let before = host.active_audicle;
    host.active_audicle = Some(id);
    if matches!(hook, "Cancel" | "Fail") {
        let event = if hook == "Fail" {
            json!({"event":"audicle_failed","previous_stage":current.label(),"reason":input["reason"],"owner":owner})
        } else {
            json!({"event":"audicle_cancelled","previous_stage":current.label(),"owner":owner})
        };
        if let Err(error) = host.emit(event) {
            host.active_audicle = before;
            return Err(error);
        }
    } else {
        let next = match (hook, current) {
            ("Ready", Stage::Preparing) => Stage::Ready,
            ("Go", Stage::Ready) => Stage::Go,
            ("Finish", Stage::Go) => Stage::Finished,
            _ => {
                host.active_audicle = before;
                return Err(format!("invalid Audicle transition {current:?} -> {hook}"));
            }
        };
        if let Err(error) =
            host.emit(json!({"event":"audicle_stage","stage":next.label(),"owner":owner}))
        {
            host.active_audicle = before;
            return Err(error);
        }
        if next != Stage::Finished {
            host.audicles.stages.insert(id, next);
            return Ok(Value::Null);
        }
    }
    host.audicles.stages.remove(&id);
    host.audicles.owners.remove(&id);
    let parent = host.audicles.parents.remove(&id).flatten();
    host.active_audicle = before
        .filter(|other| *other != id && host.audicles.live(*other))
        .or_else(|| parent.filter(|parent| host.audicles.live(*parent)));
    Ok(Value::Null)
}
