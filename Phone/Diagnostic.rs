//! Explicit native diagnostic. Commands select real phone apps in the normal
//! planet scene; sensor values and the selfie model are never substituted.
use crate::host::Host;
use serde_json::{Value, json};
use std::{fs, io::Read, path::Path, time::{Duration, Instant}};

const SCREENS: [&str; 5] = ["home", "flashlight", "camera", "meter", "weather"];
const MAX_PUMPS: usize = 32;
const MAX_CAPTURE_BYTES: u64 = 256 * 1024 * 1024;

struct Capture { width: u64, height: u64, bytes: u64 }
fn capture_info(path: &Path) -> Result<Option<Capture>, String> {
    let metadata = match fs::metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("phone readback metadata: {error}")),
    };
    if !metadata.is_file() || metadata.len() > MAX_CAPTURE_BYTES {
        return Err("phone readback is not a bounded image file".into());
    }
    let mut header = [0u8; 128];
    let count = metadata.len().min(header.len() as u64) as usize;
    if count == 0 { return Ok(None); }
    fs::File::open(path).and_then(|mut file| file.read_exact(&mut header[..count]))
        .map_err(|error| format!("phone readback header: {error}"))?;
    let Some(end) = header[..count].iter().enumerate().filter(|(_, byte)| **byte == b'\n').nth(2).map(|(i, _)| i+1) else {
        return Ok(None);
    };
    let text = std::str::from_utf8(&header[..end]).map_err(|_| "phone readback header is not P6")?;
    let mut words = text.split_ascii_whitespace();
    if words.next() != Some("P6") { return Err("phone readback is not native P6".into()); }
    let dimension = |word: Option<&str>| -> Result<u64, String> {
        word.and_then(|word| word.parse::<u64>().ok()).filter(|n| (1..=16384).contains(n))
            .ok_or_else(|| "phone readback dimensions are invalid".into())
    };
    let width = dimension(words.next())?;
    let height = dimension(words.next())?;
    if words.next() != Some("255") || words.next().is_some() {
        return Err("phone readback must contain native 8-bit RGB".into());
    }
    let expected = end as u64 + width*height*3;
    if expected > MAX_CAPTURE_BYTES { return Err("phone readback pixel budget exceeded".into()); }
    if metadata.len() < expected { return Ok(None); }
    if metadata.len() != expected { return Err("phone readback pixel length is invalid".into()); }
    Ok(Some(Capture { width, height, bytes: expected }))
}

fn step(host: &mut Host, entity: &Value, aspect: f64, reset: bool) -> Result<Value, String> {
    host.invoke("PlanetNavigationSystem.Step", json!({"entity":entity["entity"],
        "dt":1./60.,"move":[false,false,false,false,false,false],"look":[0,0],
        "boost":false,"reset":reset,"aspect":aspect,"dig":false}))
}
fn pump(host: &mut Host, entity: &Value) -> Result<Value, String> {
    let result = host.invoke("AppearanceSystem.Pump", json!({"entity":entity["entity"],"force_draw":true}))?;
    if result["closed"] == true { return Err("phone diagnostic window closed before completion".into()); }
    Ok(result)
}
fn remove_old_capture(path: &Path) -> Result<(), String> {
    // This is the renderer's replaceable readback, not a saved phone capture.
    // Removing it makes file existence evidence of this requested frame rather
    // than accepting a stale image from a previous backend/session/app.
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("prepare fresh phone readback: {error}")),
    }
}

pub fn run(host: &mut Host) -> Result<Value, String> {
    if !cfg!(target_os = "macos") { return Err("native phone capture diagnostic currently requires macOS".into()); }
    let backend = host.backend().to_owned();
    if !matches!(backend.as_str(), "OpenGL" | "Vulkan" | "WebGPU") {
        return Err("phone diagnostic needs an explicitly selected graphics backend".into());
    }
    if !host.system_enabled("PhoneSystem") || host.invoke("PhoneSystem.Status", Value::Null)?.is_null() {
        return Err("phone diagnostic requires the configured physical handset".into());
    }
    let before = host.invoke("CueSystem.Status", Value::Null)?;
    if before["active"] == true || !before["visual"].is_null() {
        return Err("close the existing scene before running the phone diagnostic".into());
    }
    let audio_enabled = host.system_enabled("EnvironmentAudioSystem");
    if audio_enabled && host.invoke("EnvironmentAudioSystem.Status", Value::Null)?["active"] == true {
        return Err("close the existing audio session before running the phone diagnostic".into());
    }
    let output = host.output_root().join(format!("phone-{backend}"));
    fs::create_dir_all(&output).map_err(|e| format!("phone capture directory: {e}"))?;
    let source = host.output_root().join(format!("triangle-{backend}.ppm"));
    let initial_entities = host.entities.len();
    let mut cue_attempted = false;
    let mut navigation_attempted = false;
    let mut selection_attempted = false;
    let mut captures = Vec::with_capacity(SCREENS.len());
    let execution = (|| -> Result<(), String> {
        cue_attempted = true;
        host.invoke("CueSystem.Begin", json!({"procedure":"planet_surface"}))?;
        let ready = host.invoke("CueSystem.Advance", Value::Null)?;
        let entity = ready["visual"].clone();
        if entity["entity"].as_u64().is_none() || ready["active"] != false {
            return Err("phone scene did not finish normal cue preparation".into());
        }
        navigation_attempted = true;
        // This is the same first frame used by the planet application. Step
        // owns EnvironmentAudio.Begin/Frame when that System is enabled.
        step(host, &entity, 4./3., true)?;
        let window = pump(host, &entity)?;
        let mut aspect = window["aspect"].as_f64().filter(|v| v.is_finite() && (0.2..=8.).contains(v))
            .ok_or("phone diagnostic viewport dimensions unavailable")?;
        for screen in SCREENS {
            selection_attempted = true;
            host.invoke("PhoneSystem.CheckScene", json!({"screen":screen}))?;
            remove_old_capture(&source)?;
            // reset requests the native readback through the normal navigation
            // view, and returns the player to the configured diagnostic spawn.
            step(host, &entity, aspect, true)?;
            let packet = host.environment.view()["phone"].clone();
            if packet["screen"] != screen || packet["equipped"] != true
                || packet["focused"] != true || packet["visible"] != true {
                return Err(format!("{screen} did not reach the actual held phone view"));
            }
            if screen == "camera" && packet["camera_live"] != true {
                return Err("phone camera did not use its active scene model".into());
            }
            if screen == "meter" && audio_enabled && packet["meter"]["available"] != true {
                return Err("phone meter did not receive actual native output telemetry".into());
            }
            if screen == "weather" && !packet["weather"]["temperature_c"].is_number() {
                return Err("phone weather did not receive current game climate".into());
            }
            let started = Instant::now();
            let mut captured = false;
            for attempt in 0..MAX_PUMPS {
                if started.elapsed() > Duration::from_secs(10) { break; }
                let window = pump(host, &entity)?;
                if window["drawn"] != true { continue; }
                let Some(image) = capture_info(&source)? else { continue; };
                if window["width"].as_u64() != Some(image.width) || window["height"].as_u64() != Some(image.height) {
                    return Err(format!("{screen} readback does not match the current native framebuffer"));
                }
                let path = output.join(format!("phone-{screen}.ppm"));
                let copied = fs::copy(&source, &path).map_err(|e| format!("save {screen} phone capture: {e}"))?;
                if copied != image.bytes { return Err(format!("{screen} phone capture was incomplete")); }
                captures.push(json!({"screen":screen,"path":path,"width":image.width,"height":image.height,
                    "bytes":image.bytes,"pumps":attempt+1,"native_drawn":true,"fresh_readback":true,
                    "phone":packet,"device":window["device"]}));
                if let Some(value) = window["aspect"].as_f64().filter(|v| v.is_finite() && (0.2..=8.).contains(v)) {
                    aspect = value;
                }
                captured = true;
                break;
            }
            if !captured { return Err(format!("{screen} native phone capture exceeded its pump/time bound")); }
        }
        Ok(())
    })();

    // Attempt every applicable cleanup even after preparation, capture, or a
    // previous cleanup fails. CheckScene restores state without persistence.
    let mut cleanup_errors = Vec::new();
    if selection_attempted {
        if let Err(e) = host.invoke("PhoneSystem.CheckScene", json!({"restore":true})) { cleanup_errors.push(e); }
    }
    if navigation_attempted {
        let end = if let Err(error) = &execution {
            host.invoke("PlanetNavigationSystem.Abort", json!({"reason":error}))
        } else { host.invoke("PlanetNavigationSystem.Stop", Value::Null) };
        if let Err(e) = end { cleanup_errors.push(e); }
    }
    let mut audio = Value::Null;
    if navigation_attempted && audio_enabled {
        match host.invoke("EnvironmentAudioSystem.Close", Value::Null) {
            Ok(value) => audio = value,
            Err(e) => cleanup_errors.push(e),
        }
    }
    if navigation_attempted && host.system_enabled("WaterSystem") {
        if let Err(e) = host.invoke("WaterSystem.Close", Value::Null) { cleanup_errors.push(e); }
    }
    if cue_attempted {
        if let Err(e) = host.invoke("CueSystem.Close", Value::Null) { cleanup_errors.push(e); }
    }
    if host.entities.len() != initial_entities {
        cleanup_errors.push(format!("phone diagnostic Entity count changed from {initial_entities} to {}", host.entities.len()));
    }
    let report = json!({"mode":"phone_native_check","backend":backend,"diagnostic_commands":true,
        "complete":execution.is_ok() && cleanup_errors.is_empty() && captures.len()==SCREENS.len(),
        "captures":captures,"environment_audio":audio,"initial_entities":initial_entities,
        "entities_remaining":host.entities.len(),"execution_error":execution.as_ref().err(),
        "cleanup_errors":cleanup_errors,"pump_limit_per_screen":MAX_PUMPS,
        "sensor_data":"actual native output and game climate; no substituted readings",
        "verification":"fresh native P6 captures; visual review and physical input playtesting remain separate"});
    let report_path = output.join("report.json");
    fs::write(&report_path, serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?)
        .map_err(|e| format!("save phone diagnostic report: {e}"))?;
    if report["complete"] != true {
        return Err(format!("phone diagnostic failed: {}; cleanup: {}; report: {}",
            execution.err().unwrap_or_else(|| "incomplete cleanup".into()), report["cleanup_errors"], report_path.display()));
    }
    Ok(report)
}
