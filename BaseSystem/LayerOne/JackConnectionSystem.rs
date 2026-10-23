//! External JACK client diagnostics and tracked cue connections.
use libloading::Library;
use serde_json::{Value, json};
use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::path::PathBuf;

// Public JACK C ABI: jack_options_t/jack_status_t are unsigned C enum bitmasks;
// jack_nframes_t is uint32_t. The client pointer is opaque and never dereferenced.
type Open = unsafe extern "C" fn(*const c_char, c_uint, *mut c_uint, ...) -> *mut c_void;
type Close = unsafe extern "C" fn(*mut c_void) -> c_int;
type Frames = unsafe extern "C" fn(*mut c_void) -> u32;
type Version = unsafe extern "C" fn() -> *const c_char;
const NO_START_SERVER: c_uint = 0x01;

fn candidates() -> Vec<PathBuf> {
    // Explicit override is authoritative: report failure rather than silently
    // using another library. This also permits compatible replacement libraries.
    if let Some(path) = std::env::var_os("WORMWOOD_JACK_LIBRARY") {
        return vec![path.into()];
    }
    if cfg!(target_os = "macos") {
        [
            "/opt/homebrew/lib/libjack.dylib",
            "/usr/local/lib/libjack.dylib",
            "libjack.dylib",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    } else if cfg!(target_os = "windows") {
        [
            r"C:\Windows\libjack64.dll",
            r"C:\Windows\System32\libjack64.dll",
            "libjack64.dll",
            "libjack.dll",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    } else {
        ["libjack.so.0", "libjack.so"]
            .into_iter()
            .map(PathBuf::from)
            .collect()
    }
}

pub(crate) fn check_connection() -> Result<Value, String> {
    let mut attempts = Vec::new();
    for path in candidates() {
        // SAFETY: libjack is executable native code from the user's installation
        // or explicit override. Its constructors/destructors run during loading.
        let library = match unsafe { Library::new(&path) } {
            Ok(library) => library,
            Err(error) => {
                attempts.push(json!({"path": path, "error": error.to_string()}));
                continue;
            }
        };
        let mut result = json!({
            "mode": "jack_connection_diagnostic", "library": path,
            "server_autostart": false, "audio_activated": false,
            "audio_produced": false, "load_attempts": attempts
        });
        // SAFETY: signatures match the public JACK API. Resolve every symbol
        // before opening a client. Keep the library alive until after close;
        // no symbol or client pointer escapes this diagnostic invocation.
        unsafe {
            let symbols = (|| -> Result<_, libloading::Error> {
                Ok((
                    *library.get::<Open>(b"jack_client_open\0")?,
                    *library.get::<Close>(b"jack_client_close\0")?,
                    *library.get::<Frames>(b"jack_get_sample_rate\0")?,
                    *library.get::<Frames>(b"jack_get_buffer_size\0")?,
                    *library.get::<Version>(b"jack_get_version_string\0")?,
                ))
            })();
            let (open, close, sample_rate, buffer_size, version) = match symbols {
                Ok(api) => api,
                Err(error) => {
                    result["status"] = json!("api_unavailable");
                    result["error"] = json!(error.to_string());
                    return Ok(result);
                }
            };
            let version = version();
            result["version"] = if version.is_null() {
                Value::Null
            } else {
                json!(CStr::from_ptr(version).to_string_lossy())
            };
            let mut status: c_uint = 0;
            // No optional arguments: NO_START_SERVER does not require any.
            let client = open(c"WormwoodDiagnostic".as_ptr(), NO_START_SERVER, &mut status);
            result["jack_status_bits"] = json!(status);
            if client.is_null() {
                result["status"] = json!("connection_failed");
                return Ok(result);
            }
            // Copy scalar results and close before JSON allocation. This is not
            // a real-time callback and never activates the processing graph.
            let rate = sample_rate(client);
            let frames = buffer_size(client);
            let close_code = close(client);
            if close_code != 0 {
                // Avoid unloading code that a failed-to-close client may still
                // reference. Process exit reclaims it; report the failure.
                std::mem::forget(library);
            }
            result["sample_rate_hz"] = json!(rate);
            result["buffer_frames"] = json!(frames);
            result["close_return_code"] = json!(close_code);
            result["status"] = json!(if close_code == 0 {
                "connected_and_closed"
            } else {
                "close_failed"
            });
        }
        return Ok(result);
    }
    Ok(json!({
        "mode": "jack_connection_diagnostic", "status": "library_unavailable",
        "load_attempts": attempts, "server_autostart": false,
        "audio_activated": false, "audio_produced": false
    }))
}

#[path = "JackAudio.rs"]
mod audio;
#[path = "MacAudioRoute.rs"]
mod route;
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
            if host.audio.session.is_some() {
                return Err("audio already prepared".into());
            }
            if input["kind"] != "sound" || input["source"] != "diagnostic_sine_440" {
                return Err("unsupported diagnostic sound".into());
            }
            let id = input["id"].as_u64().ok_or("sound Entity ID required")?;
            let cue = host
                .active_audicle
                .ok_or("audio preparation requires Audicle attribution")?;
            let session = audio::Session::prepare(host.output_root(), id, cue)?;
            let info = session.info();
            host.audio.session = Some(session);
            host.emit(json!({"event":"audio_prepared","entity":id,"configuration":info}))?;
            Ok(Value::Null)
        }
        "Start" => {
            let id = input["entity"].as_u64().ok_or("sound Entity required")?;
            let s = host.audio.session.as_mut().ok_or("audio not prepared")?;
            if s.entity != id || Some(s.audicle) != host.active_audicle {
                return Err("audio ownership mismatch".into());
            }
            s.start()?;
            host.emit(json!({"event":"audio_started","entity":id}))?;
            Ok(Value::Null)
        }
        "RouteChanged" => {
            let id = input["entity"].as_u64().ok_or("sound Entity required")?;
            let session = host.audio.session.as_mut().ok_or("audio not prepared")?;
            if session.entity != id || Some(session.audicle) != host.active_audicle {
                return Err("audio ownership mismatch".into());
            }
            if session.route_changed() {
                host.emit(json!({"event":"audio_route_changed","entity":id,"action":"cancel_current_cue_follow_default_on_next_cue"}))?;
                Ok(json!(true))
            } else {
                Ok(json!(false))
            }
        }
        "Complete" => {
            let id = input["entity"].as_u64().ok_or("sound Entity required")?;
            let s = host.audio.session.as_ref().ok_or("audio not prepared")?;
            if s.entity != id || Some(s.audicle) != host.active_audicle {
                return Err("audio ownership mismatch".into());
            }
            Ok(json!(s.complete()))
        }
        "Stop" => {
            let id = input["entity"].as_u64().ok_or("sound Entity required")?;
            if host.audio.prepared_entity() != Some(id) {
                return Err("audio ownership mismatch".into());
            }
            let result = host.audio.session.as_mut().unwrap().stop();
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
            if let Some(current) = host.audio.prepared_entity() {
                if current != id {
                    return Err("audio ownership mismatch".into());
                }
            }
            if let Some(s) = host.audio.session.as_mut() {
                s.release()?;
            }
            host.audio.session.take();
            host.emit(json!({"event":"audio_released","entity":id}))?;
            Ok(Value::Null)
        }
        _ => Err("unknown JACK hook".into()),
    }
}
