//! Layer-one embedded VM ownership. The backend exclusively owns Output after Prepare.
use crate::audio::{
    chuck::{self, Event},
    dsp::Route,
};
use serde_json::{Value, json};
use std::path::Path;
#[derive(Default)]
pub(crate) struct State {
    session: Option<chuck::Session>,
    output: Option<chuck::Output>,
    entity: Option<u64>,
    mix_routes: [Route;17],
    pending_events: Vec<Value>,
    retired_requests: std::collections::BTreeSet<u64>,
}
impl State {
    fn refresh_mix(&self)->Result<(),String> {
        let mut routes=self.mix_routes;
        #[cfg(target_os="macos")]
        for (channel,route) in routes.iter_mut().enumerate() {
            unsafe extern "C" { fn ww_environment_sound_gain(channel:u32)->f32; }
            route.gain*=unsafe {ww_environment_sound_gain(channel as u32)};
        }
        self.session.as_ref().ok_or("ChucK not prepared")?.routes(routes)
    }
    /// Transfer the unique application producer for the existing native output.
    pub fn take_workstation_transport(&mut self) -> Result<crate::audio::transport::Producer,String> {
        self.session.as_mut().ok_or("ChucK not prepared")?.take_workstation_transport()
    }
    pub fn take_input_capture(&mut self)->Result<crate::audio::input_capture::Reader,String>{
        self.session.as_mut().ok_or("ChucK not prepared")?.take_input_capture()
    }
    pub fn microphone_routes(&self,routes:[Route;17],available:bool)->Result<(),String>{
        self.session.as_ref().ok_or("ChucK not prepared")?.microphone_routes(routes,available)
    }
    /// Read-only lifetime guard for Host System/Entity disablement checks.
    pub fn prepared_entity(&self) -> Option<u64> {
        self.entity
    }
    /// Borrow the transfer slot so failed native attachment retains ownership.
    pub fn output_slot(&mut self) -> &mut Option<chuck::Output> {
        &mut self.output
    }
    /// Shared integration must transfer this once into its pinned native callback state.
    pub fn take_output(&mut self) -> Option<chuck::Output> {
        self.output.take()
    }
    pub fn dispatch(&mut self, root: &Path, hook: &str, input: &Value) -> Result<Value, String> {
        match hook {
            "Prepare" => {
                if self.session.is_some() {
                    return Err("ChucK already prepared".into());
                }
                let id = input["entity"]
                    .as_u64()
                    .filter(|n| *n > 0)
                    .ok_or("sound Entity required")?;
                let rate = input["sample_rate"]
                    .as_u64()
                    .filter(|n| *n <= u32::MAX as u64)
                    .ok_or("sample rate required")? as u32;
                let (s, o) = chuck::Session::prepare(root, rate)?;
                self.session = Some(s);
                self.output = Some(o);
                self.entity = Some(id);
                Ok(
                    json!({"entity":id,"sample_rate":rate,"channels":17,"requires_output_attachment":true}),
                )
            }
            "Compile" => {
                let path = input["script"].as_str().ok_or("script path required")?;
                let session = self.session.as_mut().ok_or("ChucK not prepared")?;
                if let Some(gain) = input.get("feedback_gain") {
                    let gain = gain.as_f64().filter(|g| (0.0..=1.0).contains(g))
                        .ok_or("feedback gain must be within 0..1")?;
                    let request = session.compile_feedback(path, gain)?;
                    return Ok(json!({"request":request}));
                }
                let channel = input["channel"]
                    .as_u64()
                    .filter(|n| *n < 17)
                    .ok_or("bus channel required")? as usize;
                let request = session.compile(path, channel)?;
                Ok(json!({"request":request}))
            }
            "Remove" => {
                let request = input["request"].as_u64().ok_or("ChucK request required")?;
                let discard = input["discard_events"] == true;
                if discard
                    && self.retired_requests.len() >= 256
                    && !self.retired_requests.contains(&request)
                {
                    return Err("ChucK cancellation budget exceeded".into());
                }
                self.session
                    .as_ref()
                    .ok_or("ChucK not prepared")?
                    .remove(request)?;
                if discard {
                    self.retired_requests.insert(request);
                    self.pending_events
                        .retain(|event| event["request"].as_u64() != Some(request));
                }
                Ok(Value::Null)
            }
            "Poll" => {
                let s = self.session.as_ref().ok_or("ChucK not prepared")?;
                let retired = &mut self.retired_requests;
                let incoming: Vec<Value> = s
                    .events()
                    .into_iter()
                    .map(|e| match e {
                        Event::Compiled { request, shred } => {
                            json!({"request":request,"stage":"compiled","shred":shred})
                        }
                        Event::Completed { request } => {
                            json!({"request":request,"stage":"completed"})
                        }
                        Event::Cancelled { request } => {
                            json!({"request":request,"stage":"cancelled"})
                        }
                        Event::Failed { request, error } => {
                            json!({"request":request,"stage":"failed","error":error})
                        }
                    })
                    .filter(|event| {
                        let Some(request) = event["request"].as_u64() else {
                            return true;
                        };
                        if !retired.contains(&request) {
                            return true;
                        }
                        // A completion can already be queued when Remove is
                        // accepted; retain suppression through its cancellation
                        // acknowledgement so neither terminal event is orphaned.
                        if event["stage"] == "cancelled" {
                            retired.remove(&request);
                        }
                        false
                    })
                    .collect();
                self.pending_events.extend(incoming);
                if self.pending_events.len() > 256 {
                    return Err("unconsumed ChucK event budget exceeded".into());
                }
                let wanted = input["requests"].as_array().map(|a| {
                    a.iter()
                        .filter_map(Value::as_u64)
                        .collect::<std::collections::BTreeSet<_>>()
                });
                let mut events = Vec::new();
                self.pending_events.retain(|e| {
                    let take = wanted
                        .as_ref()
                        .is_none_or(|w| e["request"].as_u64().is_some_and(|id| w.contains(&id)));
                    if take {
                        events.push(e.clone());
                    }
                    !take
                });
                Ok(json!({"events":events,"telemetry":s.telemetry()}))
            }
            "SetRoutes" | "SetMicrophoneRoutes" => {
                let rows = input["sources"]
                    .as_array()
                    .ok_or("spatial sources required")?;
                if rows.len() > 17 {
                    return Err("spatial source budget exceeded".into());
                }
                let mut routes = [Route::default(); 17];
                let mut seen = [false; 17];
                for row in rows {
                    let ch = row["channel"]
                        .as_u64()
                        .filter(|n| *n < 17)
                        .ok_or("spatial channel required")? as usize;
                    if seen[ch] {
                        return Err("multiple sources mapped to one ChucK bus".into());
                    }
                    seen[ch] = true;
                    let number = |k: &str| {
                        row[k]
                            .as_f64()
                            .filter(|n| n.is_finite())
                            .ok_or_else(|| format!("spatial {k} required"))
                    };
                    let gain = number("gain")? * if row["occluded"] == true { 0.2 } else { 1. };
                    let user = input["user_gain"].as_f64().unwrap_or(0.8);
                    if !user.is_finite() {
                        return Err("invalid user gain".into());
                    }
                    routes[ch] = Route {
                        active: true,
                        gain: (gain * user.clamp(0., 4.)) as f32,
                        pan: number("pan")? as f32,
                        alpha: number("hf_alpha")? as f32,
                        echo_seconds: number("echo_delay")? as f32,
                        echo_gain: number("echo_gain")? as f32,
                        itd_ms: row["itd_ms"].as_f64().unwrap_or(0.) as f32,
                    };
                }
                if hook == "SetMicrophoneRoutes" {
                    for route in &mut routes { route.pan=0.; route.itd_ms=0.; }
                    self.microphone_routes(routes,input["available"]!=false)?;
                    return Ok(Value::Null);
                }
                self.mix_routes=routes;
                self.refresh_mix()?;
                Ok(Value::Null)
            }
            "RefreshMix" => {self.refresh_mix()?;Ok(Value::Null)},
            "Telemetry" => Ok(self
                .session
                .as_ref()
                .ok_or("ChucK not prepared")?
                .telemetry()),
            "Release" => {
                if self.output.is_none()
                    && self.session.is_some()
                    && input["output_detached"] != true
                {
                    return Err("deactivate and detach ChucK output before Release".into());
                }
                self.output.take();
                if let Some(mut s) = self.session.take() {
                    s.close()?;
                }
                self.mix_routes=[Route::default();17];
                self.entity = None;
                self.pending_events.clear();
                self.retired_requests.clear();
                Ok(Value::Null)
            }
            _ => Err("unknown ChucK hook".into()),
        }
    }
}
