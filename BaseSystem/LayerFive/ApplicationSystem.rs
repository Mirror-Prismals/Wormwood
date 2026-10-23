//! Layer-five application lifetime and input interpretation.
use crate::host::Host;
use serde_json::{Value, json};
pub(crate) fn run(host: &mut Host, input: &Value) -> Result<Value, String> {
    if !cfg!(target_os = "macos") {
        return Err("interactive application currently enabled on macOS".into());
    }
    let environment = input["procedure"] == "planet_surface";
    let step = if environment {
        "PlanetNavigationSystem.Step"
    } else {
        "FlightSystem.Step"
    };
    let stop = if environment {
        "PlanetNavigationSystem.Stop"
    } else {
        "FlightSystem.Stop"
    };
    let flight = input["procedure"] == "hello_cube" || environment;
    let mut last = std::time::Instant::now();
    let mut last_reset = false;
    let mut flight_state = Value::Null;
    let mut frames = 0u64;
    let mut previous = [false; 3];
    let mut phase = 0u8;
    let mut phase_frames = 0u64;
    let execution = (|| -> Result<Value, String> {
        host.invoke("CueSystem.Begin", input.clone())?;
        if flight {
            let status = host.invoke("CueSystem.Advance", Value::Null)?;
            flight_state=host.invoke(step,json!({"entity":status["visual"]["entity"],"dt":0.,"move":[false,false,false,false,false,false],"look":[0,0],"reset":true,"roll":0.,"boost":false}))?;
        }
        loop {
            let status = host.invoke("CueSystem.Status", Value::Null)?;
            let window = host.invoke("AppearanceSystem.Pump", status["visual"].clone())?;
            frames += 1;
            let keys = std::array::from_fn::<_, 3, _>(|i| window["keys"][i] == true);
            let mut trigger = keys[0] && !previous[0];
            let mut cancel = keys[1] && !previous[1];
            let mut close = window["closed"] == true || keys[2];
            previous = keys;
            // Bounded diagnostic commands exercise this same application path.
            // They are labelled as injected commands, not physical key presses.
            if host.verify_session && !flight {
                phase_frames += 1;
                match phase {
                    0 if status["active"] == false => {
                        trigger = true;
                        phase = 1;
                        phase_frames = 0;
                    }
                    1 if phase_frames >= 12 => {
                        cancel = true;
                        phase = 2;
                        phase_frames = 0;
                    }
                    2 if phase_frames >= 6 => {
                        trigger = true;
                        phase = 3;
                        phase_frames = 0;
                    }
                    3 if status["active"] == false => close = true,
                    _ => {}
                }
                if frames > 900 {
                    return Err("interactive verification exceeded frame bound".into());
                }
            }
            if flight {
                let dt = if host.verify_session {
                    1. / 60.
                } else {
                    last.elapsed().as_secs_f64().min(0.05)
                };
                last = std::time::Instant::now();
                let mut movement = window["flight"]["move"].clone();
                let mut look = window["flight"]["look"].clone();
                let pressed = window["flight"]["reset"] == true;
                let mut reset = pressed && !last_reset;
                last_reset = pressed;
                if environment {
                    close = window["closed"] == true || keys[1];
                }
                if host.verify_session {
                    movement = json!([
                        frames >= 12 && frames < 30,
                        false,
                        false,
                        frames >= 30 && frames < 42,
                        false,
                        false
                    ]);
                    look = if (42..54).contains(&frames) {
                        json!([3., -1.])
                    } else {
                        json!([0., 0.])
                    };
                    if environment {
                        movement = json!([
                            false,
                            false,
                            false,
                            false,
                            (12..24).contains(&frames),
                            false
                        ]);
                        look = if (30..42).contains(&frames) {
                            json!([0., 20.])
                        } else {
                            json!([0., 0.])
                        };
                    }
                    reset = frames == 60;
                    close = frames >= 72;
                }
                if close {
                    return Ok(status);
                }
                flight_state=host.invoke(step,json!({"entity":status["visual"]["entity"],"dt":dt,"move":movement,"look":look,"reset":reset,"roll":if environment {window["flight"]["roll"].as_f64().unwrap_or(0.)}else{0.},"boost":environment&&(host.verify_session||window["flight"]["boost"]==true)}))?;
                std::thread::sleep(std::time::Duration::from_micros(16_667));
                continue;
            }
            if trigger || cancel || close {
                host.emit(json!({"event":"application_input","trigger":trigger,"cancel":cancel,"close":close,"source":if host.verify_session{"diagnostic_commands"}else{"window_input"}}))?;
            }
            if close {
                return Ok(status);
            }
            if cancel {
                host.invoke("CueSystem.Cancel", Value::Null)?;
            } else if trigger && status["active"] == false {
                if let Err(error) = host.invoke("CueSystem.Begin", input.clone()) {
                    // A temporarily unavailable output must not destroy the scene.
                    // Begin has cancelled its event and released its sound resources.
                    if host.active_audicle.is_some() {
                        return Err(error);
                    }
                    host.emit(json!({"event":"application_audio_error","error":error,"action":"wait_for_next_trigger"}))?;
                }
            }
            host.invoke("CueSystem.Advance", Value::Null)?;
            std::thread::sleep(std::time::Duration::from_micros(16_667));
        }
    })();
    // Application shutdown owns scene cleanup even when a hook failed.
    let flight_cleanup = if flight {
        host.invoke(stop, Value::Null)
    } else {
        Ok(Value::Null)
    };
    let scene_cleanup = host.invoke("CueSystem.Close", Value::Null);
    let cleanup = flight_cleanup.and(scene_cleanup);
    match (execution, cleanup) {
        (Ok(_), Ok(_)) => {
            if flight {
                flight_state["moving"] = json!(false);
            }
            let status = host.invoke("CueSystem.Status", Value::Null)?;
            Ok(
                json!({"mode":"interactive","frames":frames,"cues_finished":status["finished"],"cues_cancelled":status["cancelled"],"entities_remaining":host.entities.len(),"diagnostic_commands":host.verify_session,"flight":flight_state}),
            )
        }
        (result, cleanup) => Err(format!(
            "application failed: {:?}; cleanup: {:?}",
            result.err(),
            cleanup.err()
        )),
    }
}
