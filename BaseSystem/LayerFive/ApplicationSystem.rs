//! Layer-five application lifetime and input interpretation.
#[path="GameFrameTiming.rs"]
mod frame_timing;
use crate::host::Host;
use serde_json::{Value, json};

// Anchor deadlines to a 60 Hz clock, so loop bookkeeping does not accumulate
// into a 57-59 Hz cap. Coarse sleep plus a bounded final millisecond avoids OS
// timer coalescing. A missed whole period resets the clock; no catch-up burst.
fn pace_game_frame(start:std::time::Instant,next:&mut Option<std::time::Instant>){
    let period=std::time::Duration::from_nanos(16_666_667);
    let guard=std::time::Duration::from_millis(1);
    let deadline=next.unwrap_or(start+period);
    if let Some(remaining)=deadline.checked_duration_since(std::time::Instant::now()){
        if remaining>guard{std::thread::sleep(remaining-guard);}
        while std::time::Instant::now()<deadline{std::hint::spin_loop();}
    }
    let finished=std::time::Instant::now();
    let following=deadline+period;
    *next=Some(if finished>=following{finished+period}else{following});
}

fn computer_ray(eye:[f64;3],direction:[f64;3],center:[f64;3])->Option<f64> {
    if eye.iter().chain(&direction).chain(&center).any(|n|!n.is_finite()) || direction[2]<=1e-6 {return None;}
    // The authored front-of-house monitor faces local -Z. Its screen plane
    // is independent of the game's continuous terrain representation.
    let t=(center[2]-eye[2])/direction[2];
    if !(0.05..=4.).contains(&t) {return None;}
    let hit=std::array::from_fn::<_,3,_>(|i|eye[i]+direction[i]*t-center[i]);
    (hit[0].abs()<=0.62 && hit[1].abs()<=0.42).then_some(t)
}

#[cfg(test)]
mod computer_tests {
    use super::computer_ray;
    #[test]
    fn computer_focus_requires_near_front_face_and_screen_hit() {
        let computer=[0.,8.,-47.];
        assert_eq!(computer_ray([0.,8.,-50.],[0.,0.,1.],computer),Some(3.));
        assert!(computer_ray([0.,8.,-52.],[0.,0.,1.],computer).is_none());
        assert!(computer_ray([0.,8.,-46.],[0.,0.,-1.],computer).is_none());
        assert!(computer_ray([0.7,8.,-50.],[0.,0.,1.],computer).is_none());
        assert!(computer_ray([0.,8.5,-50.],[0.,0.,1.],computer).is_none());
        assert!(computer_ray([0.,8.,-50.],[f64::NAN,0.,1.],computer).is_none());
    }
}

fn targets_computer(host:&mut Host,window:&Value)->Result<bool,String> {
    let Some(center)=host.theatre.computer_position() else{return Ok(false);};
    let view=host.environment.view();
    let Ok(eye)=serde_json::from_value::<[f64;3]>(view["position"].clone()) else{return Ok(false);};
    let Ok(mut direction)=serde_json::from_value::<[f64;3]>(view["forward"].clone()) else{return Ok(false);};
    if window["flight"]["mobile"]==true {
        if let (Ok(pointer),Ok(right),Ok(up))=(
            serde_json::from_value::<[f64;2]>(window["flight"]["pointer"].clone()),
            serde_json::from_value::<[f64;3]>(view["right"].clone()),
            serde_json::from_value::<[f64;3]>(view["up"].clone())) {
            let tangent=(70f64.to_radians()*0.5).tan();
            let aspect=window["aspect"].as_f64().unwrap_or(4./3.);
            direction=std::array::from_fn(|i|direction[i]+right[i]*pointer[0]*aspect*tangent+up[i]*pointer[1]*tangent);
            let length=direction.iter().map(|n|n*n).sum::<f64>().sqrt();
            direction=direction.map(|n|n/length);
        }
    }
    let Some(distance)=computer_ray(eye,direction,center) else{return Ok(false);};
    let obstruction=host.invoke("TheatreSystem.Trace",json!({"position":eye,"direction":direction,"distance":distance}))?;
    Ok(obstruction["distance"].as_f64().is_none_or(|d|d>=distance-0.35))
}
// Session IO stays with the layer-five application, outside audio callbacks.
fn load_vegetation(host:&mut Host)->Result<(),String> {
    let path=host.output_root().join("vegetation.json");
    match std::fs::read(&path) {
        Ok(bytes)=>{let snapshot:Value=serde_json::from_slice(&bytes).map_err(|e|format!("vegetation save: {e}"))?;host.invoke("VegetationSystem.Restore",snapshot)?;Ok(())},
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>Ok(()),
        Err(error)=>Err(format!("read vegetation save: {error}")),
    }
}
fn save_vegetation(host:&mut Host)->Result<(),String> {
    use std::io::Write;
    let snapshot=host.invoke("VegetationSystem.Snapshot",Value::Null)?;
    let path=host.output_root().join("vegetation.json");
    if snapshot["plants"].as_array().is_some_and(Vec::is_empty) && !path.exists() {return Ok(());}
    let directory=host.output_root();
    std::fs::create_dir_all(directory).map_err(|e|e.to_string())?;
    let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e|e.to_string())?.as_nanos();
    let pending=directory.join(format!("vegetation-{}-{stamp}.pending",std::process::id()));
    let bytes=serde_json::to_vec_pretty(&snapshot).map_err(|e|e.to_string())?;
    let mut file=std::fs::OpenOptions::new().write(true).create_new(true).open(&pending).map_err(|e|e.to_string())?;
    file.write_all(&bytes).and_then(|_|file.sync_all()).map_err(|e|format!("write vegetation save {}: {e}",pending.display()))?;
    std::fs::rename(&pending,&path).map_err(|e|format!("commit vegetation save (pending preserved): {e}"))
}
pub(crate) fn run(host: &mut Host, input: &Value) -> Result<Value, String> {
    if !cfg!(any(target_os = "macos", target_os = "ios")) {
        return Err("interactive application currently enabled on macOS and iOS".into());
    }
    let environment = input["procedure"] == "planet_surface";
    let benchmark_request = host.game_workstation_benchmark.clone();
    if benchmark_request.is_some() && !environment { return Err("combined benchmark requires the real planet scene".into()); }
    let mut combined_workstation = Value::Null;
    let environment_audio=environment && host.system_enabled("EnvironmentAudioSystem");
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
    let mut paused = false;
    let mut last_select = false;
    let mut phone_was_equipped = false;
    let mut flight_state = Value::Null;
    let mut menu_settings = Value::Null;
    let mut frames = 0u64;
    let mut previous = [false; 3];
    let mut phase = 0u8;
    let mut phase_frames = 0u64;
    // Explicit bounded diagnostic workload, never inferred from live input.
    let game_replay=if host.verify_session && host.close_after_frames().is_some(){std::env::var("WORMWOOD_GAME_REPLAY").ok()}else{None};
    let capture_frames:Vec<u64>=if host.verify_session {
        std::env::var("WORMWOOD_GAME_CAPTURE_FRAMES").unwrap_or_default().split(',').filter_map(|s|s.parse().ok()).collect()
    }else{Vec::new()};
    let mut vegetation_loaded=false;
    let mut frame_timing=frame_timing::Timing::default();
    let mut game_deadline=None;
    let focus_repeats=if benchmark_request.is_some(){std::env::var("WORMWOOD_RESIDENT_FOCUS_VERIFY").ok().map(|v|v.parse::<u32>().ok().filter(|n|(1..=8).contains(n)).ok_or("resident focus verification requires 1..8 entries")).transpose()?.unwrap_or(1)}else{1};
    let mut focus_reports=Vec::new();
    let execution = (|| -> Result<Value, String> {
        if environment && host.system_enabled("VegetationSystem") {load_vegetation(host)?;vegetation_loaded=true;}
        if cfg!(target_os="macos") && environment && host.system_enabled("AudioWorkstationSystem")
            && std::env::var("WORMWOOD_WORKSTATION_PREWARM").as_deref()!=Ok("0") {
            host.invoke("AudioWorkstationSystem.Prewarm",Value::Null)?;
        }
        host.invoke("CueSystem.Begin", input.clone())?;
        if environment && host.system_enabled("PhoneSystem") {host.invoke("PhoneSystem.Configure",Value::Null)?;}
        if environment_audio { host.invoke("EnvironmentAudioSystem.Begin",Value::Null)?; }
        if environment && host.system_enabled("WaterSystem"){host.invoke("WaterSystem.Configure",Value::Null)?;}
        if flight {
            let status = host.invoke("CueSystem.Advance", Value::Null)?;
            flight_state=host.invoke(step,json!({"entity":status["visual"]["entity"],"dt":0.,"move":[false,false,false,false,false,false],"look":[0,0],"reset":true,"initialize":true,"boost":false}))?;
        }
        if environment && host.system_enabled("AudioWorkstationSystem") {
            host.invoke("PlanetNavigationSystem.Pause",Value::Null)?;
            host.invoke("AudioWorkstationSystem.PrimeWorld",Value::Null)?;
        }
        if environment {
            let status = host.invoke("CueSystem.Status", Value::Null)?;
            host.invoke(
                "GraphicsConnectionSystem.InputMode",
                json!({"entity":status["visual"]["entity"],"pointer":false}),
            )?;
        }
        loop {
            let frame_started = std::time::Instant::now();
            frame_timing.begin(frames+1,frame_started);
            let status = host.invoke("CueSystem.Status", Value::Null)?;
            let pump_started=std::time::Instant::now();
            let window = host.invoke("AppearanceSystem.Pump", status["visual"].clone())?;
            // A capture requested by the preceding update is drawn at Pump.
            if capture_frames.contains(&frames) {
                let source=host.output_root().join(format!("triangle-{}.ppm",host.backend()));
                if source.exists(){std::fs::copy(source,host.output_root().join(format!("game-frame-{frames:05}.ppm"))).map_err(|e|e.to_string())?;}
            }
            frame_timing.pump(pump_started.elapsed().as_secs_f64()*1000.,window["drawn"]==true,
                host.scene_preparation.as_ref().map_or(frames+1,|w|w.sequence()));
            if window["suspended"] == true {
                last = std::time::Instant::now();
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }
            frames += 1;
            if host.close_after_frames().is_some_and(|limit|frames>=limit as u64) {
                return Ok(status);
            }
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
                let mut axis = window["flight"]["axis"].clone();
                let mut touch = window["flight"]["touch"].clone();
                let mut tool_input = window["flight"]["tool_input"].clone();
                let mut tools = window["flight"]["tools"].clone();
                let mut water_edit = window["flight"]["water_edit"].clone();
                let mut phone = Value::Null;
                let mut glasses = Value::Null;
                let mut crafting = Value::Null;
                let mut monastery = Value::Null;
                let pressed = window["flight"]["reset"] == true;
                let mut reset = pressed && !last_reset && (!environment || flight_state["cheats"] == true);
                last_reset = pressed;
                if environment {
                    close = window["closed"] == true || window["flight"]["quit"] == true;
                    let menu=&window["flight"]["pause_menu"];
                    if menu["active"]==true {
                        menu_settings=menu.clone();
                        close |= menu["quit"]==true;
                    }
                    let select = window["flight"]["select"] == true;
                    let mut selected = select && !last_select;
                    last_select = select;
                    if host.system_enabled("PhoneSystem") {
                        let view = host.environment.view().clone();
                        let mut phone_window=window.clone();
                        if host.glasses.held() || host.glasses.owns_interact() || host.crafting.carrying() || host.crafting.owns_interact(){phone_window["flight"]["phone"]["interact"]=json!(false);}
                        phone = host.invoke("PhoneSystem.Input",json!({"entity":status["visual"]["entity"],
                            "view":view,"window":phone_window,"dt":dt,"blocked":paused||close,"escape":cancel}))?;
                        if phone["consumed_select"] == true { selected = false; }
                        if phone["consumed_escape"] == true { cancel = false; }
                        if phone["consumed_reset"] == true { reset = false; }
                        if phone["consumed_interact"] == true { water_edit = Value::Null; }
                        let equipped = phone["equipped"] == true;
                        if phone["focus_changed"] == true || equipped != phone_was_equipped
                            || (phone["consumed_interact"] == true && !equipped) {
                            // Cancel a pending tool stroke before a pickup or
                            // focus transition can look like its release.
                            host.invoke("PlanetNavigationSystem.Pause",Value::Null)?;
                        }
                        phone_was_equipped = equipped;
                    }
                    if host.system_enabled("GlassesSystem") {
                        glasses=host.invoke("GlassesSystem.Input",json!({"window":window,"view":host.environment.view(),
                            "blocked":paused||close,"phone_focused":phone["focused"],"phone_held":phone["equipped"],"phone_consumed":phone["consumed_interact"]==true||host.crafting.carrying()||host.crafting.owns_interact()}))?;
                        if glasses["consumed_interact"]==true {water_edit=Value::Null;}
                        if glasses["changed"]==true || glasses["open"]==true {host.invoke("PlanetNavigationSystem.Pause",Value::Null)?;}
                        if glasses["changed"]==true {
                            let text=if glasses["worn"]==true {"Mirror | Glasses equipped · E audio workstation · B remove"}
                                else if glasses["held"]==true {"Mirror | Glasses in hand · B equip · F place on a surface"}
                                else {"Mirror | Glasses placed · F pick up · B equip · E audio workstation"};
                            host.invoke("GraphicsConnectionSystem.Caption",json!({"entity":status["visual"]["entity"],"text":text}))?;
                        }
                    }
                    if host.system_enabled("CraftingSystem") {
                        crafting=host.invoke("CraftingSystem.Input",json!({"window":window,"view":host.environment.view(),
                            "blocked":paused||close||phone["equipped"]==true||phone["consumed_interact"]==true||glasses["held"]==true||glasses["consumed_interact"]==true}))?;
                        if crafting["consumed_select"]==true{selected=false;}
                        if crafting["consumed_interact"]==true{water_edit=Value::Null;}
                        if crafting["changed"]==true{host.invoke("PlanetNavigationSystem.Pause",Value::Null)?;}
                        if let Some(text)=crafting["caption"].as_str(){host.invoke("GraphicsConnectionSystem.Caption",json!({"entity":status["visual"]["entity"],"text":text}))?;}
                    }
                    if host.system_enabled("MonasterySystem") {
                        monastery=host.invoke("MonasterySystem.Input",json!({"window":window,"view":host.environment.view(),
                            "blocked":paused||close||phone["equipped"]==true||phone["consumed_interact"]==true||glasses["held"]==true||glasses["consumed_interact"]==true||crafting["held"]==true||crafting["consumed_interact"]==true}))?;
                        if monastery["consumed_interact"]==true {water_edit=Value::Null;}
                        if monastery["caption_changed"]==true {
                            let text=monastery["caption"].as_str().unwrap_or("Mirror | Esc pause");
                            host.invoke("GraphicsConnectionSystem.Caption",json!({"entity":status["visual"]["entity"],"text":text}))?;
                        }
                    }
                    if !paused && !close && phone["equipped"]!=true && glasses["held"]!=true && !host.crafting.carrying() && host.system_enabled("TheatreSystem") {
                        let wheel=window["flight"]["workstation"]["editor"]["wheel"][1].as_f64().unwrap_or(0.);
                        if wheel!=0. {
                            let view=host.environment.view();
                            host.invoke("TheatreSystem.ChairInput",json!({"position":view["position"],"direction":view["forward"],"wheel":wheel}))?;
                        }
                    }
                    // Cardinal PerfSystem::prepareHybridDawCapture enters the real
                    // Computer session directly. Reuse this production transition;
                    // no fake DAW-only state or synthetic physical input is claimed.
                    let benchmark_entry = benchmark_request.is_some() && frames >= 2
                        && host.theatre.computer_position().is_some();
                    if benchmark_request.is_some() && frames > 120 { return Err("combined benchmark could not find the real theatre computer".into()); }
                    if (benchmark_entry || !paused) && !close && host.system_enabled("TheatreSystem")
                        && phone["focused"] != true && host.system_enabled("AudioWorkstationSystem")
                        && (benchmark_entry || glasses["open"]==true || (selected && targets_computer(host,&window)?)) {
                        host.invoke("PlanetNavigationSystem.Pause",Value::Null)?;
                        let visual=status["visual"].clone();
                        host.invoke("GraphicsConnectionSystem.Suspend",visual.clone())?;
                        let mut request=benchmark_request.clone().unwrap_or_else(||json!({}));
                        if (!benchmark_entry && glasses["open"]!=true) || std::env::var("WORMWOOD_COMPUTER_SCREEN_VERIFY").as_deref()==Ok("1") {request["computer_screen"]=json!(true);}
                        if glasses["open"]==true {request["eyewear"]=json!(host.glasses.worn_kind());}
                        // Benchmark selection wears the real pair without enabling
                        // the separate ray/camera-switch verification workload.
                        if benchmark_entry {
                            if let Some(kind)=request["eyewear"].as_str() {
                                for pair in &mut host.glasses.pairs {
                                    pair.location=if pair.kind==kind {super::glasses_system::Carry::Worn}else{super::glasses_system::Carry::Placed};
                                }
                            }
                        }
                        if benchmark_entry {if let Ok(kind)=std::env::var("WORMWOOD_WEARABLE_VERIFY"){
                            if !["ar","vr"].contains(&kind.as_str()){return Err("wearable verification must select ar or vr".into());}
                            for pair in &mut host.glasses.pairs{pair.location=if pair.kind==kind{super::glasses_system::Carry::Worn}else{super::glasses_system::Carry::Placed};}
                            request["eyewear"]=json!(kind);request["wearable_verify"]=json!(true);
                        }}
                        let mut workstation=host.invoke("AudioWorkstationSystem.RunEmbedded",request);
                        // Suspend has completed/saved the workstation GPU queries.
                        // Read them before restoring the game's separate GPU logger.
                        if benchmark_entry {
                            if let Ok(report) = &mut workstation {
                                let backend=host.backend().to_owned();
                                if let Err(error)=crate::workstation::benchmark::augment_gpu(report,host.output_root(),&backend){
                                    report["gpu_error"]=json!(error); report["benchmark"]["complete"]=json!(false);
                                }
                                if report["gpu"]["category"] != "DawGraphics" {
                                    report["gpu_error"]=json!("GPU report was not from the embedded workstation session");
                                    report["benchmark"]["complete"]=json!(false);
                                }
                                report["entry"]=json!({"method":"real theatre computer / shared production Suspend-RunEmbedded-Resume transition",
                                    "computer_world_position":host.theatre.computer_position(),"game_visual":visual,
                                    "game_frames_before_entry":frames,"player_view":host.environment.view()});
                            }
                        }
                        // Always restore the retained scene, including after a
                        // failed workstation asset load. No Cue/terrain reset.
                        host.invoke("GraphicsConnectionSystem.Resume",visual.clone())?;
                        host.invoke("GraphicsConnectionSystem.InputMode",json!({"entity":visual["entity"],"pointer":false}))?;
                        let caption=match &workstation {
                            Ok(_)=>if host.glasses.worn(){"Mirror | Glasses equipped · E audio workstation · B remove".to_owned()}else{"Mirror | Click the theatre computer to open the audio workstation | Esc pause".to_owned()},
                            Err(error)=>format!("Mirror | Workstation unavailable: {error}"),
                        };
                        host.invoke("GraphicsConnectionSystem.Caption",json!({"entity":visual["entity"],"text":caption}))?;
                        if benchmark_entry {
                            combined_workstation=workstation.clone()?;
                            focus_reports.push(combined_workstation.clone());
                            if focus_reports.len()>=focus_repeats as usize {
                                combined_workstation["focus_verification"]=json!({"requested_entries":focus_repeats,"entries":focus_reports});
                                return Ok(status);
                            }
                        }
                        if workstation.as_ref().is_ok_and(|s|s["end_reason"]=="window_closed") {return Ok(status);}
                        // Rebuild the retained view before presenting the new
                        // surface; held controls cannot become a digging action.
                        flight_state=host.invoke(step,json!({"entity":visual["entity"],"dt":0.,"move":[false,false,false,false,false,false],"look":[0,0],"reset":false,"boost":false,"aspect":window["aspect"]}))?;
                        previous=[false;3];last_select=true;last_reset=false;
                        last=std::time::Instant::now();
                        continue;
                    }
                    let focused = host.verify_session || window["flight"]["focused"] != false;
                    let next_pause = if !focused {
                        true
                    } else if paused && menu["resume"]==true {
                        false
                    } else if cancel {
                        !paused
                    } else if paused && selected && !cfg!(target_os="macos") {
                        false
                    } else {
                        paused
                    };
                    if next_pause != paused {
                        paused = next_pause;
                        host.invoke(
                            "GraphicsConnectionSystem.InputMode",
                            json!({"entity":status["visual"]["entity"],"pointer":paused,"pause_menu":flight_state}),
                        )?;
                        host.invoke("PlanetNavigationSystem.Pause", Value::Null)?;
                        host.invoke("GraphicsConnectionSystem.Caption",json!({"entity":status["visual"]["entity"],"text":if paused {
                            "Mirror | Paused | Resume / Esc | Cmd+Q quit"
                        } else {"Mirror | Mouse look | 1–4 select tool · Hold left mouse, release to work · Right mouse pour · Esc pause menu"}}))?;
                        last = std::time::Instant::now();
                        // The click that resumes the game is not an excavation request.
                        if !close {
                            continue;
                        }
                    }
                    if paused && !close {
                        if environment_audio {host.invoke("EnvironmentAudioSystem.RefreshMix",Value::Null)?;}
                        std::thread::sleep(std::time::Duration::from_millis(16));
                        continue;
                    }
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
                    close = frames >= host.close_after_frames().unwrap_or(72) as u64;
                }
                if close {
                    return Ok(status);
                }
                if environment {
                    match game_replay.as_deref() {
                        Some("area-soak")=>{movement=json!([false,false,false,false,false,false]);look=json!([0.01,0.]);reset=false;},
                        Some("turn")=>{look=json!([2.,0.]);movement=json!([false,false,false,false,false,false]);reset=false;},
                        Some("walk")=>{movement=json!([false,false,frames>=4,false,false,false]);look=json!([0.,0.]);reset=false;},
                        Some("exit-back")=>{
                            movement=json!([frames>=6&&frames!=7&&frames<270,false,false,false,false,true]);
                            look=if frames==4||frames==5{json!([900.,0.])}else if frames==270||frames==271{json!([900.,-53.])}else{json!([0.,0.])};reset=false;
                        },
                        Some("exit")=>{
                            movement=json!([frames>=6&&frames!=7&&frames<900,false,false,false,false,true]);
                            look=if frames==4||frames==5{json!([900.,0.])}else if frames>=900{
                                let phase=(frames-900)as f64/150.;
                                json!([4.,4.*phase.cos()])
                            }else{json!([0.,0.])};reset=false;
                        },
                        _=>{},
                    }
                }
                let phone_blocks_tools = monastery["consumed_interact"]==true || crafting["held"]==true || crafting["consumed_select"]==true || crafting["consumed_interact"]==true || glasses["held"]==true || glasses["consumed_interact"]==true || glasses["consumed_open"]==true || glasses["changed"]==true || phone["equipped"] == true || phone["consumed_select"] == true
                    || phone["consumed_interact"] == true || phone["consumed_escape"] == true
                    || phone["consumed_reset"] == true;
                if phone_blocks_tools {
                    tool_input = Value::Null;
                    tools = json!({"slot":null,"work":false,"pour":false});
                    touch = Value::Null;
                }
                if phone["focused"] == true {
                    movement = json!([false,false,false,false,false,false]);
                    look = json!([0.,0.]);
                    axis = json!([0.,0.]);
                    touch = Value::Null;
                }
                if phone["consumed_reset"] == true { reset = false; }
                let cheats=if menu_settings.is_object(){&menu_settings["cheats"]}else{&window["flight"]["cheats"]};
                let update_started=std::time::Instant::now();
                flight_state=host.invoke(step,json!({"entity":status["visual"]["entity"],"dt":dt,"capture":capture_frames.contains(&frames),"move":movement,"axis":axis,"mobile":window["flight"]["mobile"],"touch":touch,"tool_input":tool_input,"tools":tools,"water_edit":water_edit,"npc_talk":environment&&!phone_blocks_tools&&window["flight"]["npc_talk"]==true,"sit":environment&&window["flight"]["sit"]==true,"park_gate":environment&&window["flight"]["park_gate"]==true,"cheats":cheats,"diagnostic_route_frame":if game_replay.as_deref()==Some("area-soak"){json!(frames)}else{Value::Null},"time_override_minutes":menu_settings["time_override_minutes"],"cheat_keys":window["flight"]["cheat_keys"],"look":look,"reset":reset,"dig":environment&&!phone_blocks_tools&&(if host.verify_session{game_replay.is_none()&&frames==4}else{window["flight"]["dig"]==true}),"mode_request":window["flight"]["mode_request"],"boost":environment&&(host.verify_session||window["flight"]["boost"]==true),"aspect":window["aspect"]}))?;
                frame_timing.update(update_started.elapsed().as_secs_f64()*1000.);
                if environment && host.system_enabled("AudioWorkstationSystem") {
                    host.invoke("AudioWorkstationSystem.Background",Value::Null)?;
                }
                if game_replay.as_deref()==Some("area-soak") && frames%60==0 {
                    use std::io::Write;
                    let mut file=std::fs::OpenOptions::new().create(true).append(true).open(host.output_root().join("route-poses.jsonl")).map_err(|e|e.to_string())?;
                    writeln!(file,"{}",json!({"frame":frames,"cycle":(frames-1)/1200,"segment":((frames-1)%1200)/100,"flight":flight_state})).map_err(|e|e.to_string())?;
                }
                menu_settings=Value::Null;
                // Budget the whole frame, including rendering and simulation.
                // Sleeping a full frame after that work unnecessarily adds latency.
                pace_game_frame(frame_started,&mut game_deadline);
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
            std::thread::sleep(std::time::Duration::from_micros(16_667).saturating_sub(frame_started.elapsed()));
        }
    })();
    frame_timing.finish();
    if crate::host::performance_telemetry::enabled() || std::env::var("WORMWOOD_GAME_FRAME_TIMING").as_deref()==Ok("1") {
        let mut timing_report=frame_timing.report();
        if let Some(worker)=&host.scene_preparation{timing_report["scene_preparation"]=worker.report();}
        let report=serde_json::to_vec_pretty(&timing_report).map_err(|e|e.to_string());
        // Diagnostics must not bypass native/audio cleanup on an IO failure.
        if let Err(error)=report.and_then(|bytes|std::fs::write(host.output_root().join("game-frame-timing.json"),bytes).map_err(|e|e.to_string())) {
            eprintln!("Game frame timing report: {error}");
        }
    }
    // Application shutdown owns scene cleanup even when a hook failed.
    let flight_cleanup = if flight {
        if let Err(error) = &execution {
            host.invoke(if environment { "PlanetNavigationSystem.Abort" } else { "FlightSystem.Abort" }, json!({"reason":error}))
        } else {
            host.invoke(stop, Value::Null)
        }
    } else {
        Ok(Value::Null)
    };
    let monastery_cleanup=if environment && host.system_enabled("MonasterySystem"){host.invoke("MonasterySystem.Close",Value::Null)}else{Ok(Value::Null)};
    let crafting_cleanup=if environment && host.system_enabled("CraftingSystem"){host.invoke("CraftingSystem.Close",Value::Null)}else{Ok(Value::Null)};
    let glasses_cleanup=if environment && host.system_enabled("GlassesSystem") {host.invoke("GlassesSystem.Close",Value::Null)}else{Ok(Value::Null)};
    let phone_cleanup=if environment && host.system_enabled("PhoneSystem") { host.invoke("PhoneSystem.Close",Value::Null) } else {Ok(Value::Null)};
    let workstation_cleanup=if environment && host.system_enabled("AudioWorkstationSystem") {host.invoke("AudioWorkstationSystem.Close",Value::Null)}else{Ok(Value::Null)};
    let audio_cleanup=if environment && host.system_enabled("EnvironmentAudioSystem") { host.invoke("EnvironmentAudioSystem.Close",Value::Null) } else {Ok(Value::Null)};
    let audio_report=audio_cleanup.as_ref().ok().cloned().unwrap_or(Value::Null);
    let water_cleanup=if environment && host.system_enabled("WaterSystem"){host.invoke("WaterSystem.Close",Value::Null)}else{Ok(Value::Null)};
    let vegetation_save=if vegetation_loaded {save_vegetation(host).map(|_|Value::Null)}else{Ok(Value::Null)};
    let scene_cleanup = host.invoke("CueSystem.Close", Value::Null);
    let cleanup = flight_cleanup.and(workstation_cleanup).and(monastery_cleanup).and(crafting_cleanup).and(glasses_cleanup).and(phone_cleanup).and(audio_cleanup).and(water_cleanup).and(vegetation_save).and(scene_cleanup);
    match (execution, cleanup) {
        (Ok(_), Ok(_)) => {
            if flight {
                flight_state["moving"] = json!(false);
            }
            let status = host.invoke("CueSystem.Status", Value::Null)?;
            Ok(
                json!({"mode":"interactive","frames":frames,"cues_finished":status["finished"],"cues_cancelled":status["cancelled"],"entities_remaining":host.entities.len(),"diagnostic_commands":host.verify_session,"flight":flight_state,"environment_audio":audio_report,"combined_workstation":combined_workstation}),
            )
        }
        (result, cleanup) => Err(format!(
            "application failed: {:?}; cleanup: {:?}",
            result.err(),
            cleanup.err()
        )),
    }
}
