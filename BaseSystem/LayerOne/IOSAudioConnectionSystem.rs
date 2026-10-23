//! iOS output connection; the native host is Apple RemoteIO.
use serde_json::{Value, json};
#[path = "IOSAudio.rs"]
mod audio;
#[derive(Default)]
pub(crate) struct State {
    session: Option<audio::Session>,
}
impl State {
    pub(crate) fn prepared_entity(&self) -> Option<u64> {
        self.session.as_ref().map(|s| s.entity)
    }
}
pub(crate) fn dispatch(
    host: &mut crate::host::Host,
    hook: &str,
    input: &Value,
) -> Result<Value, String> {
    match hook {
        "Prepare" => {
            if host.ios_audio.session.is_some() {
                return Err("audio already prepared".into());
            }
            if input["kind"] != "sound" || input["source"] != "diagnostic_sine_440" {
                return Err("unsupported diagnostic sound".into());
            }
            let id = input["id"].as_u64().ok_or("sound Entity ID required")?;
            let cue = host
                .active_audicle
                .ok_or("audio preparation requires Audicle attribution")?;
            let session = audio::Session::prepare(host.root(), id, cue)?;
            let info = session.info();
            host.ios_audio.session = Some(session);
            host.emit(json!({"event":"audio_prepared","entity":id,"configuration":info}))?;
            Ok(Value::Null)
        }
        "Start" => {
            let id = input["entity"].as_u64().ok_or("sound Entity required")?;
            let s = host
                .ios_audio
                .session
                .as_mut()
                .ok_or("audio not prepared")?;
            if s.entity != id || Some(s.audicle) != host.active_audicle {
                return Err("audio ownership mismatch".into());
            }
            s.start()?;
            host.emit(json!({"event":"audio_started","entity":id}))?;
            Ok(Value::Null)
        }
        "Stop" => {
            let id = input["entity"].as_u64().ok_or("sound Entity required")?;
            if host.ios_audio.prepared_entity() != Some(id) {
                return Err("audio ownership mismatch".into());
            }
            let result = host.ios_audio.session.as_mut().unwrap().stop();
            match result {
                Ok(report) => {
                    host.emit(json!({"event":"audio_completed","entity":id,"report":report}))?;
                    Ok(report)
                }
                Err(e) => Err(e),
            }
        }
        "Release" => {
            let id = input["entity"].as_u64().ok_or("sound Entity required")?;
            if let Some(current) = host.ios_audio.prepared_entity() {
                if current != id {
                    return Err("audio ownership mismatch".into());
                }
            }
            if let Some(s) = host.ios_audio.session.as_mut() {
                s.release()?;
            }
            host.ios_audio.session.take();
            host.emit(json!({"event":"audio_released","entity":id}))?;
            Ok(Value::Null)
        }
        _ => Err("unknown iOS audio hook".into()),
    }
}
