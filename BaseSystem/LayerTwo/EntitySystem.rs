//! Tracks Entity instances without imposing scene containers.
use crate::{base_entity::Entity, host::Host};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct State {
    next: u64,
    entities: BTreeMap<u64, Entity>,
}

pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    match hook {
        "Create" => {
            let id = host.entities.next + 1;
            let entity = Entity::from_spec(id, input)?;
            host.emit(json!({"event":"entity_created","entity":id,"appearance":entity.data()}))?;
            host.entities.next = id;
            host.entities.entities.insert(id, entity);
            Ok(json!({"entity":id}))
        }
        "Get" => {
            let id = input["entity"].as_u64().ok_or("Entity ID required")?;
            host.entities
                .entities
                .get(&id)
                .map(Entity::data)
                .ok_or_else(|| format!("unknown Entity {id}"))
        }
        "Remove" => {
            let id = input["entity"].as_u64().ok_or("Entity ID required")?;
            if !host.entities.entities.contains_key(&id) {
                return Err(format!("unknown Entity {id}"));
            }
            host.emit(json!({"event":"entity_removed","entity":id}))?;
            host.entities.entities.remove(&id);
            Ok(Value::Null)
        }
        _ => Err("unknown Entity hook".into()),
    }
}

impl State {
    pub(crate) fn len(&self) -> usize {
        self.entities.len()
    }
}
