//! Resolves Entity appearance data before crossing the graphics connection.
#[path = "EnvironmentGeometry.rs"]
mod environment_geometry;
#[path = "Projection.rs"]
pub(crate) mod projection;
use crate::host::Host;
use serde_json::Value;

pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    match hook {
        "View" => {
            let entity = host.invoke(
                "EntitySystem.Get",
                serde_json::json!({"entity":input["entity"]}),
            )?;
            let data =
                crate::base_entity::Entity::from_spec(entity["id"].as_u64().unwrap(), &entity)?;
            let crate::base_entity::Appearance::Visual { vertices, .. } = data.appearance else {
                return Err("view requires a visual".into());
            };
            let triangles = match entity["shader"].as_str() {
                Some("water_volume") => environment_geometry::water(input),
                Some("planet_surface") => environment_geometry::planet(input),
                _ => projection::project(&vertices, input),
            };
            host.invoke(
                "GraphicsConnectionSystem.SetView",
                serde_json::json!({"entity":input["entity"],"triangles":triangles}),
            )
        }
        "Prepare" => {
            let entity = host.invoke("EntitySystem.Get", input.clone())?;
            host.invoke("GraphicsConnectionSystem.Prepare", entity)
        }
        "Present" => {
            host.invoke("EntitySystem.Get", input.clone())?;
            host.invoke("GraphicsConnectionSystem.Present", input.clone())
        }
        "Pump" => {
            host.invoke("EntitySystem.Get", input.clone())?;
            host.invoke("GraphicsConnectionSystem.Pump", input.clone())
        }
        "Release" => host.invoke("GraphicsConnectionSystem.Release", input.clone()),
        _ => Err("unknown appearance hook".into()),
    }
}
