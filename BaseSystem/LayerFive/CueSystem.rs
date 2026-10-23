//! Executes a validated visual/sound cue through declared connection hooks.
use crate::host::Host;
use serde_json::{Value, json};

pub(crate) fn run(host: &mut Host, input: &Value) -> Result<Value, String> {
    let audio_system = if cfg!(target_os = "ios") {
        "IOSAudioConnectionSystem"
    } else {
        "JackConnectionSystem"
    };
    let cue = host.invoke("AudicleSystem.Create", Value::Null)?;
    let mut entity = None;
    let mut sound = None;
    let mut audio_report = Value::Null;
    let execution = (|| -> Result<Value, String> {
        let definition = host.invoke("CueProcedureSystem.Load", input.clone())?;
        let handle = host.invoke("EntitySystem.Create", definition["visual"].clone())?;
        entity = Some(handle.clone());
        host.invoke("AppearanceSystem.Prepare", handle.clone())?;
        if host.audio_enabled() && !definition["sound"].is_null() {
            let handle = host.invoke("EntitySystem.Create", definition["sound"].clone())?;
            sound = Some(handle.clone());
            let data = host.invoke("EntitySystem.Get", handle)?;
            host.invoke(&format!("{audio_system}.Prepare"), data)?;
        }
        host.invoke("AudicleSystem.Ready", cue.clone())?;
        host.invoke("AudicleSystem.Go", cue.clone())?;
        if let Some(sound) = &sound {
            host.invoke(&format!("{audio_system}.Start"), sound.clone())?;
        }
        let presentation = host.invoke("AppearanceSystem.Present", handle)?;
        if presentation["end_reason"] != "window_closed" {
            if let Some(sound) = &sound {
                audio_report = host.invoke(&format!("{audio_system}.Stop"), sound.clone())?;
            }
        }
        Ok(presentation)
    })();
    // Both success and failure follow declared cleanup routes. Do not discard
    // cleanup errors or claim Finished if resources could not be released.
    let mut cleanup_errors = Vec::new();
    if let Some(handle) = sound {
        if let Err(e) = host.invoke(&format!("{audio_system}.Release"), handle.clone()) {
            cleanup_errors.push(e);
        }
        if let Err(e) = host.invoke("EntitySystem.Remove", handle) {
            cleanup_errors.push(e);
        }
    }
    if let Some(handle) = entity {
        if let Err(e) = host.invoke("AppearanceSystem.Release", handle.clone()) {
            cleanup_errors.push(e);
        }
        if let Err(e) = host.invoke("EntitySystem.Remove", handle) {
            cleanup_errors.push(e);
        }
    }
    let cancelled = execution
        .as_ref()
        .is_ok_and(|r| r["end_reason"] == "window_closed");
    let end = if execution.is_ok() && cleanup_errors.is_empty() && !cancelled {
        "Finish"
    } else {
        "Cancel"
    };
    if let Err(e) = host.invoke(&format!("AudicleSystem.{end}"), cue.clone()) {
        cleanup_errors.push(e);
    }
    match execution {
        Ok(result) if cleanup_errors.is_empty() => Ok(
            json!({"mode":"cue","procedure":input["procedure"],"audicle":cue["audicle"],"stage":if cancelled{"Cancelled"}else{"Finished"},"reason":if cancelled{Some("window_closed")}else{None},"presentation":result,"audio":audio_report,"entities_remaining":host.entities.len()}),
        ),
        result => Err(format!(
            "cue failed: {}; cleanup: {:?}",
            result.err().unwrap_or_else(|| "cleanup failed".into()),
            cleanup_errors
        )),
    }
}

// Persistent visual resources and active event state belong to CueSystem.
#[derive(Default)]
pub(crate) struct State {
    visual: Option<Value>,
    procedure: Option<String>,
    cue: Option<Value>,
    sound: Option<Value>,
    started: Option<std::time::Instant>,
    finished: u64,
    cancelled: u64,
}
impl State {
    fn status(&self) -> Value {
        json!({"active":self.cue.is_some(),"visual":self.visual,"finished":self.finished,"cancelled":self.cancelled})
    }
    fn end(&mut self, host: &mut Host, cancel: bool) -> Result<(), String> {
        if self.cue.is_none() {
            return Ok(());
        }
        if let Some(sound) = &self.sound {
            if !cancel {
                host.invoke("JackConnectionSystem.Stop", sound.clone())?;
            }
            host.invoke("JackConnectionSystem.Release", sound.clone())?;
            host.invoke("EntitySystem.Remove", sound.clone())?;
            self.sound = None;
        }
        host.invoke(
            if cancel {
                "AudicleSystem.Cancel"
            } else {
                "AudicleSystem.Finish"
            },
            self.cue.clone().unwrap(),
        )?;
        self.cue = None;
        self.started = None;
        if cancel {
            self.cancelled += 1;
        } else {
            self.finished += 1;
        }
        Ok(())
    }
}
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let mut state = std::mem::take(&mut host.cue_session);
    let result = (|| {
        match hook {
            "Status" => return Ok(state.status()),
            "Begin" => {
                if state.cue.is_some() || host.active_audicle.is_some() {
                    return Err("a cue is already active".into());
                }
                let name = input["procedure"].as_str().ok_or("procedure required")?;
                if state.procedure.as_deref().is_some_and(|p| p != name) {
                    return Err("close the current scene before changing procedures".into());
                }
                state.cue = Some(host.invoke("AudicleSystem.Create", Value::Null)?);
                let prepare = (|| -> Result<(), String> {
                    let definition = host.invoke("CueProcedureSystem.Load", input.clone())?;
                    if state.visual.is_none() {
                        let visual =
                            host.invoke("EntitySystem.Create", definition["visual"].clone())?;
                        state.visual = Some(visual.clone());
                        host.invoke("AppearanceSystem.Prepare", visual)?;
                        state.procedure = Some(name.into());
                    }
                    if host.audio_enabled() && !definition["sound"].is_null() {
                        let sound =
                            host.invoke("EntitySystem.Create", definition["sound"].clone())?;
                        state.sound = Some(sound.clone());
                        let data = host.invoke("EntitySystem.Get", sound.clone())?;
                        host.invoke("JackConnectionSystem.Prepare", data)?;
                    }
                    let cue = state.cue.clone().unwrap();
                    host.invoke("AudicleSystem.Ready", cue.clone())?;
                    host.invoke("AudicleSystem.Go", cue)?;
                    if let Some(sound) = &state.sound {
                        host.invoke("JackConnectionSystem.Start", sound.clone())?;
                    }
                    state.started = Some(std::time::Instant::now());
                    Ok(())
                })();
                if let Err(error) = prepare {
                    let cleanup = state.end(host, true);
                    return Err(format!("cue preparation: {error}; cleanup: {cleanup:?}"));
                }
            }
            "Advance" => {
                if state.cue.is_some() {
                    if let Some(sound) = &state.sound {
                        if host.invoke("JackConnectionSystem.RouteChanged", sound.clone())? == true
                        {
                            state.end(host, true)?;
                            return Ok(state.status());
                        }
                    }
                    let complete = match &state.sound {
                        Some(sound) => {
                            host.invoke("JackConnectionSystem.Complete", sound.clone())? == true
                        }
                        None => true,
                    };
                    if complete {
                        state.end(host, false)?;
                    } else if state
                        .started
                        .is_some_and(|t| t.elapsed() > std::time::Duration::from_secs(5))
                    {
                        return Err("cue audio completion timed out".into());
                    }
                }
                return Ok(state.status());
            }
            "Cancel" => state.end(host, true)?,
            "Close" => {
                state.end(host, true)?;
                if let Some(visual) = &state.visual {
                    host.invoke("AppearanceSystem.Release", visual.clone())?;
                    host.invoke("EntitySystem.Remove", visual.clone())?;
                    state.visual = None;
                    state.procedure = None;
                }
            }
            _ => return Err("unknown cue hook".into()),
        }
        Ok(Value::Null)
    })();
    host.cue_session = state;
    result
}
