//! Connection to the explicitly selected native graphics API.
use crate::{base_entity::Entity, host::Host};
use serde_json::{Value, json};

#[cfg(not(target_os = "ios"))]
#[path = "../../Render/mod.rs"]
mod renderer;
#[cfg(target_os = "ios")]
#[path = "../../Render/ios.rs"]
mod renderer;

#[derive(Default)]
pub(crate) struct State {
    session: Option<renderer::Session>,
    entity: Option<u64>,
    #[cfg(test)]
    pub(crate) test_driver: bool,
    #[cfg(test)]
    pub(crate) fail_present: bool,
    #[cfg(test)]
    pub(crate) fail_prepare: bool,
    #[cfg(test)]
    pub(crate) close_after: Option<u32>,
}

impl State {
    pub(crate) fn prepared_entity(&self) -> Option<u64> {
        self.entity
    }
}

pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let id = if hook == "Prepare" {
        input["id"].as_u64()
    } else {
        input["entity"].as_u64()
    }
    .ok_or("Entity ID required")?;
    match hook {
        "Prepare" => {
            if host.graphics.entity.is_some() {
                return Err("initial graphics connection already has a prepared Entity".into());
            }
            let entity = Entity::from_spec(id, input)?;
            #[cfg(test)]
            if host.graphics.test_driver {
                if host.graphics.fail_prepare {
                    return Err("injected preparation failure".into());
                }
                host.graphics.entity = Some(id);
                host.emit(json!({"event":"native_prepared","entity":id,"backend":"TestDriver","visible":false}))?;
                return Ok(Value::Null);
            }
            #[cfg(not(target_os = "ios"))]
            let mut session = renderer::Session::prepare(
                host.root(),
                host.output_root(),
                host.backend(),
                &entity,
            )?;
            #[cfg(target_os = "ios")]
            let mut session = renderer::Session::prepare(
                host.root(),
                host.output_root(),
                host.backend(),
                &entity,
            )?;
            #[cfg(not(target_os = "ios"))]
            session.environment(input["shader"].as_str().unwrap_or("vertex_color"));
            #[cfg(not(target_os = "ios"))]
            if let Ok(bytes) = std::fs::read(host.root().join("Project.json")) {
                if let Ok(project) = serde_json::from_slice::<Value>(&bytes) {
                    if let Some(name) = project["display_name"].as_str() {
                        session.title(name)?;
                    }
                }
            }
            let info = session.info();
            host.graphics.session = Some(session);
            host.graphics.entity = Some(id);
            host.emit(json!({"event":"native_prepared","entity":id,"backend":host.backend(),"visible":false,"device":info}))?;
            Ok(Value::Null)
        }
        "Present" => {
            if host.graphics.entity != Some(id) {
                return Err("Entity has not been prepared by this connection".into());
            }
            #[cfg(test)]
            if host.graphics.test_driver {
                if host.graphics.fail_present {
                    return Err("injected presentation failure".into());
                }
                let frames = host.graphics.close_after.unwrap_or(1);
                if frames > 0 {
                    host.emit(json!({"event":"frame_presented","entity":id,"backend":"TestDriver","frame":1}))?;
                }
                let reason = if host.graphics.close_after.is_some() {
                    "window_closed"
                } else {
                    "frame_limit"
                };
                host.emit(json!({"event":"presentation_ended","entity":id,"reason":reason,"frames":frames}))?;
                return Ok(json!({"backend":"TestDriver","frames":frames,"end_reason":reason}));
            }
            let mut session = host
                .graphics
                .session
                .take()
                .ok_or("native session missing")?;
            let frames = host.frame_limit();
            #[cfg(not(target_os="ios"))]
            let result=session.present(if host.live(){None}else{Some(frames)},host.frame_period_us(),host.close_after_frames(),|frame|host.emit(json!({"event":"frame_presented","entity":id,"backend":host.backend(),"frame":frame})));
            #[cfg(target_os="ios")]
            let result=session.present(frames,|frame|host.emit(json!({"event":"frame_presented","entity":id,"backend":host.backend(),"frame":frame})));

            host.graphics.session = Some(session);
            if let Ok(report) = &result {
                host.emit(json!({"event":"presentation_ended","entity":id,"reason":report["end_reason"],"close_source":report["close_source"],"frames":report["frames"]}))?;
            }
            result
        }
        "SetView" => {
            if host.graphics.entity != Some(id) {
                return Err("view Entity is not prepared".into());
            }
            let triangles: Vec<[f32; 24]> =
                serde_json::from_value(input["triangles"].clone()).map_err(|e| e.to_string())?;
            #[cfg(test)]
            if host.graphics.test_driver {
                return Ok(Value::Null);
            }
            #[cfg(target_os = "ios")]
            return Err("flight views are currently desktop only".into());
            #[cfg(not(target_os = "ios"))]
            {
                host.graphics
                    .session
                    .as_mut()
                    .ok_or("native session missing")?
                    .set_batch(&triangles)?;
                host.emit(json!({"event":"view_updated","entity":id,"triangles":triangles.len()}))?;
                Ok(Value::Null)
            }
        }
        "Pump" => {
            if host.graphics.entity != Some(id) {
                return Err("Entity not prepared".into());
            }
            #[cfg(test)]
            if host.graphics.test_driver {
                return Ok(json!({"closed":false,"keys":[false,false,false],"drawn":true}));
            }
            #[cfg(target_os = "ios")]
            return Err("interactive window is currently desktop only".into());
            #[cfg(not(target_os = "ios"))]
            {
                let report = host
                    .graphics
                    .session
                    .as_mut()
                    .ok_or("native session missing")?
                    .pump()?;
                host.emit(json!({"event":"window_pumped","entity":id,"drawn":report["drawn"],"closed":report["closed"],"readback_verified":report["device"]["readback_verified"]}))?;
                Ok(report)
            }
        }
        "Release" => {
            if let Some(prepared) = host.graphics.entity {
                if prepared != id {
                    return Err("cannot release another Entity's resources".into());
                }
                if let Some(mut session) = host.graphics.session.take() {
                    if let Err(error) = session.finish() {
                        host.graphics.session = Some(session);
                        return Err(error);
                    }
                }
                host.graphics.entity = None;
                host.emit(json!({"event":"native_released","entity":id}))?;
            }
            Ok(Value::Null)
        }
        _ => Err("unknown graphics hook".into()),
    }
}
