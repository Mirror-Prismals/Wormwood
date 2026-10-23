use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
struct Hook {
    system: String,
    layer: u64,
    input: String,
    output: String,
    calls: BTreeSet<String>,
}

pub(crate) struct Host {
    root: PathBuf,
    output: PathBuf,
    enabled: BTreeMap<String, bool>,
    hooks: BTreeMap<String, Hook>,
    roots: Vec<String>,
    startup_steps: Vec<String>,
    startup_order: Vec<String>,
    startup_attempted: bool,
    stack: Vec<(u64, String)>,
    next_id: u64,
    halt: bool,
    trace: PathBuf,
    run_id: String,
    pub(crate) entities: crate::base_system::layer_two::entity_system::State,
    pub(crate) audicles: crate::base_system::layer_five::audicle_system::State,
    pub(crate) graphics: crate::base_system::layer_one::graphics_connection_system::State,
    pub(crate) active_audicle: Option<u64>,
    pub(crate) audio: crate::base_system::layer_one::jack_connection_system::State,
    pub(crate) ios_audio: crate::base_system::layer_one::ios_audio_connection_system::State,
    pub(crate) environment: crate::base_system::layer_five::environment_system::State,
    pub(crate) flight: crate::base_system::layer_five::flight_system::State,
    pub(crate) cue_session: crate::base_system::layer_five::cue_system::State,
    pub(crate) verify_session: bool,
    audio_enabled: bool,
    backend: String,
    frame_limit: u32,
    frame_period_us: u64,
    live: bool,
    close_after_frames: Option<u32>,
}

fn read(root: &Path, path: &str) -> Result<Value, String> {
    let bytes = fs::read(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{path}: {e}"))
}
fn object<'a>(value: &'a Value, label: &str) -> Result<&'a serde_json::Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))
}
fn names(value: &Value, label: &str) -> Result<Vec<String>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("{label} must be an array"))?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{label} needs string entries"))
        })
        .collect()
}
fn identifier(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}
fn builtin(name: &str) -> Option<(u64, &'static str, &'static str)> {
    match name {
        "JackConnectionSystem.RouteChanged" => Some((1, "EntityRef", "Boolean")),
        "JackConnectionSystem.Complete" => Some((1, "EntityRef", "Boolean")),
        "GraphicsConnectionSystem.Pump" => Some((1, "EntityRef", "WindowFrame")),
        "AppearanceSystem.Pump" => Some((4, "EntityRef", "WindowFrame")),
        "ApplicationSystem.Run" => Some((5, "ProcedureRef", "SessionReport")),
        "CueSystem.Begin" => Some((5, "ProcedureRef", "Unit")),
        "CueSystem.Advance" => Some((5, "Unit", "CueStatus")),
        "CueSystem.Cancel" => Some((5, "Unit", "Unit")),
        "CueSystem.Close" => Some((5, "Unit", "Unit")),
        "CueSystem.Status" => Some((5, "Unit", "CueStatus")),
        "PlanetNavigationSystem.Step" => Some((5, "EnvironmentInput", "FlightState")),
        "PlanetNavigationSystem.Stop" => Some((5, "Unit", "Unit")),
        "FlightSystem.Step" => Some((5, "FlightInput", "FlightState")),
        "FlightSystem.Stop" => Some((5, "Unit", "Unit")),
        "AppearanceSystem.View" => Some((4, "ViewRequest", "Unit")),
        "GraphicsConnectionSystem.SetView" => Some((1, "TriangleBatch", "Unit")),
        "NativeInventorySystem.Probe" => Some((1, "Unit", "NativeInventory")),
        "DiagnosticsSystem.ValidateRuntime" => Some((2, "Unit", "RuntimeValidation")),
        "DiagnosticsSystem.Report" => Some((2, "Unit", "NativeInventory")),
        "DiagnosticsSystem.CheckAudio" => Some((2, "Unit", "JackDiagnostic")),
        "JackConnectionSystem.CheckConnection" => Some((1, "Unit", "JackDiagnostic")),
        "JackConnectionSystem.Prepare" => Some((1, "EntityData", "Unit")),
        "JackConnectionSystem.Start" => Some((1, "EntityRef", "Unit")),
        "JackConnectionSystem.Stop" => Some((1, "EntityRef", "AudioReport")),
        "JackConnectionSystem.Release" => Some((1, "EntityRef", "Unit")),
        "IOSAudioConnectionSystem.Prepare" => Some((1, "EntityData", "Unit")),
        "IOSAudioConnectionSystem.Start" => Some((1, "EntityRef", "Unit")),
        "IOSAudioConnectionSystem.Stop" => Some((1, "EntityRef", "IOSAudioReport")),
        "IOSAudioConnectionSystem.Release" => Some((1, "EntityRef", "Unit")),
        "EntitySystem.Create" => Some((2, "EntitySpec", "EntityRef")),
        "EntitySystem.Get" => Some((2, "EntityRef", "EntityData")),
        "EntitySystem.Remove" => Some((2, "EntityRef", "Unit")),
        "AudicleSystem.Create" => Some((5, "Unit", "AudicleRef")),
        "AudicleSystem.Ready" => Some((5, "AudicleRef", "Unit")),
        "AudicleSystem.Go" => Some((5, "AudicleRef", "Unit")),
        "AudicleSystem.Finish" => Some((5, "AudicleRef", "Unit")),
        "AudicleSystem.Cancel" => Some((5, "AudicleRef", "Unit")),
        "AppearanceSystem.Prepare" => Some((4, "EntityRef", "Unit")),
        "AppearanceSystem.Present" => Some((4, "EntityRef", "Presentation")),
        "AppearanceSystem.Release" => Some((4, "EntityRef", "Unit")),
        "GraphicsConnectionSystem.Prepare" => Some((1, "EntityData", "Unit")),
        "GraphicsConnectionSystem.Present" => Some((1, "EntityRef", "Presentation")),
        "GraphicsConnectionSystem.Release" => Some((1, "EntityRef", "Unit")),
        "CueSystem.Run" => Some((5, "ProcedureRef", "CueResult")),
        "CueProcedureSystem.Load" => Some((2, "ProcedureRef", "CueDefinition")),
        _ => None,
    }
}
fn check_payload(kind: &str, value: &Value) -> bool {
    match kind {
        "Unit" => value.is_null(),
        "Boolean" => value.is_boolean(),
        "EnvironmentInput" => crate::base_system::layer_five::environment_system::valid(value),
        "FlightInput" => crate::base_system::layer_five::flight_system::valid(value),
        "FlightState" => {
            value["moving"].is_boolean()
                && value["position"].as_array().is_some_and(|v| {
                    v.len() == 3 && v.iter().all(|n| n.as_f64().is_some_and(f64::is_finite))
                })
        }
        "ViewRequest" => {
            crate::base_system::layer_four::appearance_system::projection::valid(value)
        }
        "TriangleBatch" => {
            value["entity"].as_u64().is_some_and(|n| n > 0)
                && value["triangles"].as_array().is_some_and(|v| {
                    v.len() <= 128
                        && v.iter().all(|t| {
                            t.as_array().is_some_and(|t| {
                                t.len() == 24
                                    && t.iter().all(|n| {
                                        n.as_f64().is_some_and(|n| {
                                            n.is_finite() && (n as f32).is_finite()
                                        })
                                    })
                            })
                        })
                })
        }
        "WindowFrame" => {
            value["closed"].is_boolean()
                && value["drawn"].is_boolean()
                && value["keys"]
                    .as_array()
                    .is_some_and(|k| k.len() == 3 && k.iter().all(Value::is_boolean))
        }
        "CueStatus" => {
            value["active"].is_boolean()
                && (value["visual"].is_null() || check_payload("EntityRef", &value["visual"]))
        }
        "SessionReport" => {
            value["mode"] == "interactive"
                && value["entities_remaining"] == 0
                && value["cues_finished"].is_u64()
                && value["cues_cancelled"].is_u64()
        }
        "JackDiagnostic" => {
            value["mode"] == "jack_connection_diagnostic"
                && matches!(
                    value["status"].as_str(),
                    Some(
                        "library_unavailable"
                            | "api_unavailable"
                            | "connection_failed"
                            | "connected_and_closed"
                            | "close_failed"
                    )
                )
                && value["server_autostart"] == false
                && value["audio_activated"] == false
                && value["audio_produced"] == false
        }
        "RuntimeValidation" => {
            value["mode"] == "runtime_validation"
                && value["contracts_valid"] == true
                && value["graphics_verified"] == false
                && value["audio_verified"] == false
        }
        "NativeInventory" => {
            value["mode"] == "inventory_only"
                && value["backends"].as_array().is_some_and(|b| b.len() == 3)
                && value["jack"].is_object()
        }
        "ProcedureRef" => {
            value.as_object().is_some_and(|v| v.len() == 1)
                && value["procedure"]
                    .as_str()
                    .is_some_and(crate::base_system::layer_two::cue_procedure_system::valid_name)
        }
        "CueDefinition" => {
            crate::base_system::layer_two::cue_procedure_system::validate(value).is_ok()
        }
        "EntitySpec" => crate::base_entity::Entity::from_spec(1, value).is_ok(),
        "EntityData" => {
            value["id"].as_u64().is_some_and(|id| id > 0)
                && crate::base_entity::Entity::from_spec(1, value).is_ok()
        }
        "EntityRef" => value["entity"].as_u64().is_some_and(|id| id > 0),
        "AudicleRef" => value["audicle"].as_u64().is_some_and(|id| id > 0),
        "AudioReport" => {
            value["loopback_verified"] == true
                && value["samples_produced"].as_u64().is_some_and(|n| n > 0)
        }
        "IOSAudioReport" => {
            value["api"] == "Apple RemoteIO"
                && value["completed"] == true
                && value["output_buffer_verified"] == true
                && value["active_callback_discontinuities"] == 0
                && value["samples_produced"].as_u64().is_some_and(|n| n > 0)
        }
        "Presentation" => {
            value["backend"].is_string()
                && value["frames"]
                    .as_u64()
                    .is_some_and(|n| match value.get("end_reason") {
                        None => n > 0,
                        Some(reason) if reason == "frame_limit" => n > 0,
                        Some(reason) if reason == "window_closed" => true,
                        _ => false,
                    })
        }
        "CueResult" => {
            value["mode"] == "cue"
                && (value["stage"] == "Finished"
                    || (value["stage"] == "Cancelled" && value["reason"] == "window_closed"))
                && value["entities_remaining"] == 0
        }
        _ => false,
    }
}

impl Host {
    pub(crate) fn load(root: &Path) -> Result<Self, String> {
        Self::load_paths(root, &root.join("output"))
    }
    pub(crate) fn load_paths(root: &Path, output: &Path) -> Result<Self, String> {
        let registry = read(root, "BaseSystem/registry.json")?;
        let mut enabled = BTreeMap::new();
        for (name, flag) in object(&registry, "registry")? {
            if !identifier(name) {
                return Err(format!("invalid System name {name}"));
            }
            enabled.insert(
                name.clone(),
                flag.as_bool()
                    .ok_or_else(|| format!("registry {name} must be boolean"))?,
            );
        }
        let breaker = read(root, "BaseSystem/circuit_breaker.json")?;
        let breaker_fields = object(&breaker, "circuit breaker")?;
        if breaker_fields.len() != 1 || !breaker_fields.contains_key("halt_execution") {
            return Err("circuit breaker only accepts halt_execution; System toggles belong in registry.json".into());
        }
        let halt = breaker["halt_execution"]
            .as_bool()
            .ok_or("halt_execution must be boolean")?;
        let catalog = read(root, "Procedures/functionregistry.json")?;
        if catalog["schema_version"] != 1 {
            return Err("unsupported function registry version".into());
        }
        let mut hooks = BTreeMap::new();
        let mut contracts = BTreeMap::new();
        for (system, entry) in object(&catalog["systems"], "function registry systems")? {
            if !identifier(system) || !enabled.contains_key(system) {
                return Err(format!("unregistered System {system}"));
            }
            let layer = entry["layer"]
                .as_u64()
                .filter(|n| (1..=5).contains(n))
                .ok_or("invalid layer")?;
            let contract = read(root, &format!("Systems/{system}.json"))?;
            if contract["system"] != system.as_str() || contract["layer"] != layer {
                return Err(format!("contract identity/layer mismatch for {system}"));
            }
            let declarations = object(&contract["functions"], "contract functions")?;
            let exports = object(&entry["functions"], "catalog functions")?;
            if declarations.keys().collect::<Vec<_>>() != exports.keys().collect::<Vec<_>>() {
                return Err(format!("catalog/contract function mismatch for {system}"));
            }
            for (name, signature) in exports {
                if !identifier(name) {
                    return Err("invalid function name".into());
                }
                let qualified = format!("{system}.{name}");
                let (actual_layer, input, output) =
                    builtin(&qualified).ok_or_else(|| format!("no compiled hook {qualified}"))?;
                if layer != actual_layer
                    || signature["input"] != input
                    || signature["output"] != output
                {
                    return Err(format!("compiled signature mismatch for {qualified}"));
                }
                hooks.insert(
                    qualified,
                    Hook {
                        system: system.clone(),
                        layer,
                        input: input.into(),
                        output: output.into(),
                        calls: names(&declarations[name]["calls"], "calls")?
                            .into_iter()
                            .collect(),
                    },
                );
            }
            if contract.get("update_steps").is_some() || contract.get("cleanup_steps").is_some() {
                return Err(format!(
                    "{system}: System contracts declare init_steps and entry_steps; systems.json defines startup order only"
                ));
            }
            contracts.insert(system.clone(), contract);
        }
        if enabled.len() != contracts.len() {
            return Err("registry and function catalog must list the same Systems".into());
        }
        for (name, hook) in &hooks {
            for target in &hook.calls {
                let destination = hooks
                    .get(target)
                    .ok_or_else(|| format!("{name} declares unknown hook {target}"))?;
                if target == "AudicleSystem.Create" && hook.layer != 5 {
                    return Err(format!("only layer five may originate Audicles: {name}"));
                }
                if enabled[&hook.system] && !enabled[&destination.system] {
                    return Err(format!(
                        "{name} requires disabled System {}",
                        destination.system
                    ));
                }
            }
        }
        let schedule = read(root, "Procedures/systems.json")?;
        let mut order = Vec::new();
        let mut seen = BTreeSet::new();
        for file in names(&schedule["systems_order"], "systems_order")? {
            let name = file
                .strip_suffix(".json")
                .filter(|n| identifier(n))
                .ok_or("invalid schedule filename")?;
            if !contracts.contains_key(name) || !seen.insert(name.to_owned()) {
                return Err(format!("unknown/duplicate scheduled System {name}"));
            }
            order.push(name.to_owned());
        }
        if order.len() != contracts.len() {
            return Err("schedule must account for every declared System".into());
        }
        let startup_order: Vec<String> = order.iter().filter(|s| enabled[*s]).cloned().collect();
        let mut startup_steps = Vec::new();
        let mut seen_startup = BTreeSet::new();
        for system in &order {
            for step in names(&contracts[system]["init_steps"], "init_steps")? {
                let name = format!("{system}.{step}");
                let hook = hooks
                    .get(&name)
                    .ok_or_else(|| format!("unknown startup hook {name}"))?;
                if hook.input != "Unit" {
                    return Err(format!("startup hook {name} requires input"));
                }
                if !seen_startup.insert(name.clone()) {
                    return Err(format!("duplicate startup hook {name}"));
                }
                if enabled[system] {
                    startup_steps.push(name);
                }
            }
        }
        let mut roots = startup_steps.clone();
        for system in &order {
            let Some(entries) = contracts[system].get("entry_steps") else {
                continue;
            };
            for step in names(entries, "entry_steps")? {
                let name = format!("{system}.{step}");
                if !hooks.contains_key(&name) {
                    return Err(format!("unknown entry hook {name}"));
                }
                if enabled[system] {
                    if roots.contains(&name) {
                        return Err(format!("duplicate/automatic entry hook {name}"));
                    }
                    roots.push(name);
                }
            }
        }
        let trace = output.join("execution.jsonl");
        let run_id = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        );
        Ok(Self {
            root: root.into(),
            output: output.into(),
            enabled,
            hooks,
            roots,
            startup_steps,
            startup_order,
            startup_attempted: false,
            stack: Vec::new(),
            next_id: 1,
            halt,
            trace,
            run_id,
            entities: Default::default(),
            audicles: Default::default(),
            graphics: Default::default(),
            active_audicle: None,
            audio: Default::default(),
            ios_audio: Default::default(),
            environment: Default::default(),
            flight: Default::default(),
            cue_session: Default::default(),
            verify_session: false,
            audio_enabled: false,
            backend: "OpenGL".into(),
            frame_limit: 120,
            frame_period_us: 0,
            live: false,
            close_after_frames: None,
        })
    }

    fn permitted(&self, target: &str, input: &Value) -> Result<Hook, String> {
        if self.halt {
            return Err("execution halted by circuit breaker".into());
        }
        let hook = self
            .hooks
            .get(target)
            .ok_or_else(|| format!("unknown function {target}"))?;
        if !self.enabled[&hook.system] {
            return Err(format!("disabled System {}", hook.system));
        }
        if let Some((_, caller)) = self.stack.last() {
            if !self.hooks[caller].calls.contains(target) {
                return Err(format!("undeclared call {caller} -> {target}"));
            }
        } else if !self.roots.iter().any(|name| name == target) {
            return Err(format!("Host cannot invoke unscheduled hook {target}"));
        }
        if matches!(
            target,
            "GraphicsConnectionSystem.Present"
                | "GraphicsConnectionSystem.SetView"
                | "JackConnectionSystem.Start"
                | "IOSAudioConnectionSystem.Start"
        ) && !self.audicles.is_go(self.active_audicle)
        {
            return Err("presentation requires an active Audicle in Go".into());
        }
        if target == "EntitySystem.Remove"
            && (self.graphics.prepared_entity() == input["entity"].as_u64()
                || self.audio.prepared_entity() == input["entity"].as_u64()
                || self.ios_audio.prepared_entity() == input["entity"].as_u64())
        {
            return Err("release native resources before removing this Entity".into());
        }
        if !check_payload(&hook.input, input) {
            return Err(format!("invalid input for {target}"));
        }
        Ok(hook.clone())
    }

    pub(crate) fn configuration_summary(&self) -> Value {
        json!({"mode":"runtime_validation","contracts_valid":true,"enabled_systems":self.enabled.values().filter(|v|**v).count(),"registered_functions":self.hooks.len(),"graphics_verified":false,"audio_verified":false})
    }
    pub(crate) fn validate_runtime(&mut self) -> Result<Value, String> {
        self.invoke("DiagnosticsSystem.ValidateRuntime", Value::Null)
    }
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
    #[cfg(target_os = "ios")]
    pub(crate) fn execution_run_id(&self) -> &str {
        &self.run_id
    }
    pub(crate) fn output_root(&self) -> &Path {
        &self.output
    }
    pub(crate) fn audio_enabled(&self) -> bool {
        self.audio_enabled
    }
    pub(crate) fn run_av(&mut self, backend: &str, frames: u32) -> Result<(), String> {
        let previous = std::mem::replace(&mut self.audio_enabled, true);
        let result = self.run_triangle(backend, frames);
        self.audio_enabled = previous;
        result
    }
    pub(crate) fn close_after_frames(&self) -> Option<u32> {
        self.close_after_frames
    }
    pub(crate) fn live(&self) -> bool {
        self.live
    }
    pub(crate) fn run_interactive(
        &mut self,
        procedure: &str,
        backend: &str,
        verify: bool,
    ) -> Result<(), String> {
        if !cfg!(target_os = "macos") {
            return Err("interactive sessions currently enabled for macOS".into());
        }
        self.backend = backend.into();
        self.audio_enabled = true;
        self.verify_session = verify;
        let report = self.invoke("ApplicationSystem.Run", json!({"procedure":procedure}))?;
        fs::write(
            self.output.join(format!("interactive-{backend}.json")),
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        println!("{report}");
        Ok(())
    }
    pub(crate) fn run_live(
        &mut self,
        procedure: &str,
        backend: &str,
        close_after: Option<u32>,
    ) -> Result<(), String> {
        if !cfg!(target_os = "macos") {
            return Err("live cue mode is currently enabled for macOS".into());
        }
        if close_after.is_some_and(|n| !(1..=600).contains(&n)) {
            return Err("diagnostic close count must be 1..600".into());
        }
        let previous_close = std::mem::replace(&mut self.close_after_frames, close_after);
        let previous_live = std::mem::replace(&mut self.live, true);
        let previous_period = std::mem::replace(&mut self.frame_period_us, 16_667);
        let result = self.run_procedure(procedure, backend, 120);
        self.close_after_frames = previous_close;
        self.live = previous_live;
        self.frame_period_us = previous_period;
        result
    }
    pub(crate) fn frame_period_us(&self) -> u64 {
        self.frame_period_us
    }
    pub(crate) fn run_soak(
        &mut self,
        backend: &str,
        seconds: u32,
        repeats: u32,
    ) -> Result<(), String> {
        if !(1..=10).contains(&seconds) || !(1..=20).contains(&repeats) {
            return Err("soak requires seconds 1..10 and repeats 1..20".into());
        }
        let previous = std::mem::replace(&mut self.frame_period_us, 16_667);
        let mut runs = Vec::new();
        let result = (|| {
            for _ in 0..repeats {
                self.run_av(backend, seconds * 60)?;
                let bytes = fs::read(self.output.join(format!("av-{backend}.json")))
                    .map_err(|e| e.to_string())?;
                let report: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                if self.entities.len() != 0
                    || self.active_audicle.is_some()
                    || self.graphics.prepared_entity().is_some()
                    || self.audio.prepared_entity().is_some()
                    || self.ios_audio.prepared_entity().is_some()
                {
                    return Err(
                        "soak retained active native/Entity/Audicle ownership between cues".into(),
                    );
                }
                let finished = report["stage"] == "Finished";
                runs.push(report);
                if !finished {
                    return Err("soak cue was cancelled".into());
                }
            }
            Ok(())
        })();
        self.frame_period_us = previous;
        let report = json!({"mode":"native_callback_render_soak","backend":backend,"seconds_per_cue":seconds,"frame_period_us":16_667,"requested_repeats":repeats,"same_process":true,"execution_run_id":self.run_id,"passed":result.is_ok(),"runs":runs,"error":result.as_ref().err()});
        fs::write(
            self.output.join(format!("soak-{backend}.json")),
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        result
    }
    pub(crate) fn backend(&self) -> &str {
        &self.backend
    }
    pub(crate) fn frame_limit(&self) -> u32 {
        self.frame_limit
    }

    pub(crate) fn emit(&self, mut event: Value) -> Result<(), String> {
        let (_, caller) = self
            .stack
            .last()
            .ok_or("lifecycle event requires an active registered hook")?;
        let kind = event["event"].as_str().ok_or("event kind required")?;
        let allowed = match kind {
            "entity_created" => caller == "EntitySystem.Create",
            "entity_removed" => caller == "EntitySystem.Remove",
            "audicle_stage" | "audicle_cancelled" => {
                caller.starts_with("AudicleSystem.") && self.hooks[caller].layer == 5
            }
            "audio_prepared" => matches!(
                caller.as_str(),
                "JackConnectionSystem.Prepare" | "IOSAudioConnectionSystem.Prepare"
            ),
            "audio_started" => matches!(
                caller.as_str(),
                "JackConnectionSystem.Start" | "IOSAudioConnectionSystem.Start"
            ),
            "audio_completed" => matches!(
                caller.as_str(),
                "JackConnectionSystem.Stop" | "IOSAudioConnectionSystem.Stop"
            ),
            "audio_released" => matches!(
                caller.as_str(),
                "JackConnectionSystem.Release" | "IOSAudioConnectionSystem.Release"
            ),
            "environment_step" => caller == "PlanetNavigationSystem.Step",
            "flight_step" => caller == "FlightSystem.Step",
            "view_updated" => caller == "GraphicsConnectionSystem.SetView",
            "window_pumped" => caller == "GraphicsConnectionSystem.Pump",
            "application_input" | "application_audio_error" => caller == "ApplicationSystem.Run",
            "audio_route_changed" => caller == "JackConnectionSystem.RouteChanged",
            "procedure_loaded" => caller == "CueProcedureSystem.Load",
            "presentation_ended" => caller == "GraphicsConnectionSystem.Present",
            "native_prepared" => caller == "GraphicsConnectionSystem.Prepare",
            "native_released" => caller == "GraphicsConnectionSystem.Release",
            "frame_presented" => caller == "GraphicsConnectionSystem.Present",
            _ => false,
        };
        if !allowed {
            return Err(format!("{caller} cannot emit {kind}"));
        }
        event["function"] = json!(caller);
        event["invocation"] = json!(self.stack.last().map(|(id, _)| id));
        event["audicle"] = json!(self.active_audicle);
        self.record(event)
    }

    pub(crate) fn run_procedure(
        &mut self,
        procedure: &str,
        backend: &str,
        frames: u32,
    ) -> Result<(), String> {
        let previous = std::mem::replace(&mut self.audio_enabled, true);
        let result = self.run_named_cue(procedure, backend, frames);
        self.audio_enabled = previous;
        result
    }
    pub(crate) fn run_triangle(&mut self, backend: &str, frames: u32) -> Result<(), String> {
        self.run_named_cue("hello_triangle", backend, frames)
    }
    fn run_named_cue(&mut self, procedure: &str, backend: &str, frames: u32) -> Result<(), String> {
        if !matches!(backend, "OpenGL" | "Vulkan" | "WebGPU") {
            return Err("backend must be OpenGL, Vulkan, or WebGPU".into());
        }
        if !(1..=600).contains(&frames) {
            return Err("frame limit must be 1..600".into());
        }
        self.backend = backend.into();
        self.frame_limit = frames;
        let result = self.invoke("CueSystem.Run", json!({"procedure":procedure}));
        let report = match &result {
            Ok(value) => value.clone(),
            Err(error) => {
                json!({"mode":"cue","backend":backend,"error":error,"success":false})
            }
        };
        fs::create_dir_all(&self.output).map_err(|e| e.to_string())?;
        fs::write(
            self.output.join(format!(
                "{}-{backend}.json",
                if self.live {
                    "live"
                } else if self.audio_enabled {
                    "av"
                } else {
                    "triangle"
                }
            )),
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        result.map(|value| println!("{value}"))
    }

    fn record(&self, mut event: Value) -> Result<(), String> {
        event
            .as_object_mut()
            .ok_or("trace event must be an object")?
            .insert("run_id".into(), json!(self.run_id));
        fs::create_dir_all(&self.output).map_err(|e| e.to_string())?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.trace)
            .map_err(|e| e.to_string())?;
        writeln!(file, "{event}").map_err(|e| e.to_string())
    }

    pub(crate) fn invoke(&mut self, target: &str, input: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let parent = self.stack.last().map(|(id, _)| *id);
        let caller = self
            .stack
            .last()
            .map(|(_, name)| name.as_str())
            .unwrap_or("Host");
        let hook = match self.permitted(target, &input) {
            Ok(hook) => hook,
            Err(error) => {
                self.record(json!({"event": "rejected", "invocation": id, "parent": parent, "caller": caller, "function": target, "error": error}))?;
                return Err(error);
            }
        };
        self.record(
            json!({"event": "started", "invocation": id, "parent": parent, "caller": caller,
            "function": target, "layer": hook.layer, "input_type": hook.input, "audicle": self.active_audicle}),
        )?;
        let event_audicle = self.active_audicle;
        self.stack.push((id, target.into()));
        let result = match target {
            "NativeInventorySystem.Probe" => {
                crate::base_system::layer_one::native_inventory_system::probe()
            }
            "DiagnosticsSystem.ValidateRuntime" => {
                crate::base_system::layer_two::diagnostics_system::validate_runtime(self)
            }
            "DiagnosticsSystem.Report" => {
                crate::base_system::layer_two::diagnostics_system::report(self)
            }
            "DiagnosticsSystem.CheckAudio" => {
                crate::base_system::layer_two::diagnostics_system::check_audio(self)
            }
            "JackConnectionSystem.CheckConnection" => {
                crate::base_system::layer_one::jack_connection_system::check_connection()
            }
            "JackConnectionSystem.Prepare"
            | "JackConnectionSystem.Start"
            | "JackConnectionSystem.Stop"
            | "JackConnectionSystem.Release" => {
                crate::base_system::layer_one::jack_connection_system::dispatch(
                    self,
                    target.split_once('.').unwrap().1,
                    &input,
                )
            }
            "IOSAudioConnectionSystem.Prepare"
            | "IOSAudioConnectionSystem.Start"
            | "IOSAudioConnectionSystem.Stop"
            | "IOSAudioConnectionSystem.Release" => {
                crate::base_system::layer_one::ios_audio_connection_system::dispatch(
                    self,
                    target.split_once('.').unwrap().1,
                    &input,
                )
            }
            "EntitySystem.Create" => {
                crate::base_system::layer_two::entity_system::dispatch(self, "Create", &input)
            }
            "EntitySystem.Get" => {
                crate::base_system::layer_two::entity_system::dispatch(self, "Get", &input)
            }
            "EntitySystem.Remove" => {
                crate::base_system::layer_two::entity_system::dispatch(self, "Remove", &input)
            }
            "AudicleSystem.Create" => {
                crate::base_system::layer_five::audicle_system::dispatch(self, "Create", &input)
            }
            "AudicleSystem.Ready" => {
                crate::base_system::layer_five::audicle_system::dispatch(self, "Ready", &input)
            }
            "AudicleSystem.Go" => {
                crate::base_system::layer_five::audicle_system::dispatch(self, "Go", &input)
            }
            "AudicleSystem.Finish" => {
                crate::base_system::layer_five::audicle_system::dispatch(self, "Finish", &input)
            }
            "AudicleSystem.Cancel" => {
                crate::base_system::layer_five::audicle_system::dispatch(self, "Cancel", &input)
            }
            "AppearanceSystem.Prepare" => {
                crate::base_system::layer_four::appearance_system::dispatch(self, "Prepare", &input)
            }
            "AppearanceSystem.Present" => {
                crate::base_system::layer_four::appearance_system::dispatch(self, "Present", &input)
            }
            "AppearanceSystem.Release" => {
                crate::base_system::layer_four::appearance_system::dispatch(self, "Release", &input)
            }
            "GraphicsConnectionSystem.Prepare" => {
                crate::base_system::layer_one::graphics_connection_system::dispatch(
                    self, "Prepare", &input,
                )
            }
            "GraphicsConnectionSystem.Present" => {
                crate::base_system::layer_one::graphics_connection_system::dispatch(
                    self, "Present", &input,
                )
            }
            "GraphicsConnectionSystem.Release" => {
                crate::base_system::layer_one::graphics_connection_system::dispatch(
                    self, "Release", &input,
                )
            }
            "CueProcedureSystem.Load" => {
                crate::base_system::layer_two::cue_procedure_system::load(self, &input)
            }
            "PlanetNavigationSystem.Step" | "PlanetNavigationSystem.Stop" => {
                crate::base_system::layer_five::environment_system::dispatch(
                    self,
                    target.rsplit('.').next().unwrap(),
                    &input,
                )
            }
            "FlightSystem.Step" | "FlightSystem.Stop" => {
                crate::base_system::layer_five::flight_system::dispatch(
                    self,
                    target.rsplit('.').next().unwrap(),
                    &input,
                )
            }
            "AppearanceSystem.View" => {
                crate::base_system::layer_four::appearance_system::dispatch(self, "View", &input)
            }
            "GraphicsConnectionSystem.SetView" => {
                crate::base_system::layer_one::graphics_connection_system::dispatch(
                    self, "SetView", &input,
                )
            }
            "ApplicationSystem.Run" => {
                crate::base_system::layer_five::application_system::run(self, &input)
            }
            "CueSystem.Begin" | "CueSystem.Advance" | "CueSystem.Cancel" | "CueSystem.Close"
            | "CueSystem.Status" => crate::base_system::layer_five::cue_system::dispatch(
                self,
                target.rsplit('.').next().unwrap(),
                &input,
            ),
            "AppearanceSystem.Pump" => {
                crate::base_system::layer_four::appearance_system::dispatch(self, "Pump", &input)
            }
            "GraphicsConnectionSystem.Pump" => {
                crate::base_system::layer_one::graphics_connection_system::dispatch(
                    self, "Pump", &input,
                )
            }
            "JackConnectionSystem.RouteChanged" => {
                crate::base_system::layer_one::jack_connection_system::dispatch(
                    self,
                    "RouteChanged",
                    &input,
                )
            }
            "JackConnectionSystem.Complete" => {
                crate::base_system::layer_one::jack_connection_system::dispatch(
                    self, "Complete", &input,
                )
            }
            "CueSystem.Run" => crate::base_system::layer_five::cue_system::run(self, &input),
            _ => Err(format!("no implementation for {target}")),
        }
        .and_then(|value| {
            if check_payload(&hook.output, &value) {
                Ok(value)
            } else {
                Err(format!("invalid output for {target}"))
            }
        });
        self.stack.pop();
        self.record(
            json!({"event": if result.is_ok() { "finished" } else { "failed" },
            "invocation": id, "parent": parent, "function": target, "layer": hook.layer,
            "output_type": hook.output, "audicle": event_audicle.or(self.active_audicle), "error": result.as_ref().err()}),
        )?;
        result
    }

    pub(crate) fn startup(&mut self) -> Result<Value, String> {
        if self.startup_attempted {
            return Err("Host startup has already been attempted".into());
        }
        self.startup_attempted = true;
        self.record(
            json!({"event":"host_startup","state":"Starting","systems":self.startup_order}),
        )?;
        let mut steps = Vec::new();
        let mut failure = None;
        for hook in self.startup_steps.clone() {
            match self.invoke(&hook, Value::Null) {
                Ok(output) => steps.push(json!({"function":hook,"output":output})),
                Err(error) => {
                    failure = Some((hook, error));
                    break;
                }
            }
        }
        let report = json!({"mode":"system_startup","started":failure.is_none(),"systems_order":self.startup_order,"completed_steps":steps,"failure":failure});
        self.record(json!({"event":"host_startup","state":if failure.is_none(){"Started"}else{"Failed"},"failure":failure}))?;
        fs::create_dir_all(&self.output).map_err(|e| e.to_string())?;
        fs::write(
            self.output.join("startup.json"),
            serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        match failure {
            Some((hook, error)) => Err(format!("startup failed at {hook}: {error}")),
            None => Ok(report),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn host() -> Host {
        Host::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap()
    }
    #[test]
    fn persistent_scene_outlives_finish_cancel_and_replay() {
        let mut host = traced_host();
        host.stack.push((1, "ApplicationSystem.Run".into()));
        let input = json!({"procedure":"hello_triangle"});
        host.invoke("CueSystem.Begin", input.clone()).unwrap();
        let visual = host.graphics.prepared_entity();
        assert!(host.invoke("CueSystem.Begin", input.clone()).is_err());
        let status = host.invoke("CueSystem.Advance", Value::Null).unwrap();
        assert_eq!(status["finished"], 1);
        assert_eq!(host.entities.len(), 1);
        assert!(host.active_audicle.is_none());
        host.invoke("AppearanceSystem.Pump", status["visual"].clone())
            .unwrap();
        host.invoke("CueSystem.Begin", input.clone()).unwrap();
        assert_eq!(visual, host.graphics.prepared_entity());
        host.invoke("CueSystem.Cancel", Value::Null).unwrap();
        assert_eq!(visual, host.graphics.prepared_entity());
        host.invoke("CueSystem.Begin", input).unwrap();
        host.invoke("CueSystem.Advance", Value::Null).unwrap();
        host.invoke("CueSystem.Close", Value::Null).unwrap();
        let status = host.invoke("CueSystem.Status", Value::Null).unwrap();
        assert_eq!(status["finished"], 2);
        assert_eq!(status["cancelled"], 1);
        assert_eq!(host.entities.len(), 0);
        assert!(host.graphics.prepared_entity().is_none());
        let rows: Vec<Value> = fs::read_to_string(&host.trace)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(
            rows.iter()
                .filter(|r| r["event"] == "native_prepared")
                .count(),
            1
        );
        assert_eq!(
            rows.iter()
                .filter(|r| r["event"] == "native_released")
                .count(),
            1
        );
        fs::remove_file(host.trace).unwrap();
    }
    #[test]
    fn interactive_preparation_failure_cancels_and_shutdown_releases_scene() {
        let mut host = traced_host();
        host.graphics.fail_prepare = true;
        host.stack.push((1, "ApplicationSystem.Run".into()));
        assert!(
            host.invoke("CueSystem.Begin", json!({"procedure":"hello_triangle"}))
                .is_err()
        );
        assert!(host.active_audicle.is_none());
        host.invoke("CueSystem.Close", Value::Null).unwrap();
        assert_eq!(host.entities.len(), 0);
        host.graphics.fail_prepare = false;
        host.invoke("CueSystem.Begin", json!({"procedure":"hello_triangle"}))
            .unwrap();
        host.invoke("CueSystem.Close", Value::Null).unwrap();
        assert_eq!(host.entities.len(), 0);
        assert!(host.active_audicle.is_none());
        fs::remove_file(host.trace).unwrap();
    }
    #[test]
    fn interactive_hooks_require_declared_caller_and_correct_payloads() {
        let mut host = host();
        assert!(
            host.permitted("CueSystem.Begin", &json!({"procedure":"hello_triangle"}))
                .is_err()
        );
        host.stack.push((1, "CueSystem.Status".into()));
        assert!(
            host.permitted("AudicleSystem.Create", &Value::Null)
                .is_err()
        );
        assert!(!check_payload(
            "WindowFrame",
            &json!({"closed":false,"drawn":true,"keys":[true]})
        ));
        assert!(!check_payload(
            "CueStatus",
            &json!({"active":true,"visual":{"entity":0}})
        ));
    }
    #[test]
    fn flight_holds_one_event_until_idle_and_finishes_before_scene_cleanup() {
        let mut host = traced_host();
        host.stack.push((1, "ApplicationSystem.Run".into()));
        host.invoke("CueSystem.Begin", json!({"procedure":"hello_cube"}))
            .unwrap();
        let scene = host.invoke("CueSystem.Advance", Value::Null).unwrap();
        let mut step = json!({"entity":scene["visual"]["entity"],"dt":0.02,"move":[true,false,false,false,false,false],"look":[0,0],"reset":false});
        host.invoke("FlightSystem.Step", step.clone()).unwrap();
        let event = host.active_audicle;
        assert!(event.is_some());
        host.invoke("FlightSystem.Step", step.clone()).unwrap();
        assert_eq!(host.active_audicle, event);
        step["move"] = json!([false, false, false, false, false, false]);
        host.invoke("FlightSystem.Step", step.clone()).unwrap();
        assert!(host.active_audicle.is_none());
        step["reset"] = json!(true);
        host.invoke("FlightSystem.Step", step).unwrap();
        host.invoke("FlightSystem.Stop", Value::Null).unwrap();
        assert!(host.active_audicle.is_none());
        host.invoke("CueSystem.Close", Value::Null).unwrap();
        assert_eq!(host.entities.len(), 0);
        let rows: Vec<Value> = fs::read_to_string(&host.trace)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let steps: Vec<_> = rows
            .iter()
            .filter(|r| r["event"] == "flight_step")
            .collect();
        assert_eq!(steps.len(), 3);
        assert_eq!(steps[0]["audicle"], steps[1]["audicle"]);
        assert_ne!(steps[1]["audicle"], steps[2]["audicle"]);
        fs::remove_file(host.trace).unwrap();
    }
    #[test]
    fn view_changes_cannot_bypass_the_active_event_requirement() {
        let mut host = host();
        host.stack.push((1, "AppearanceSystem.View".into()));
        assert!(
            host.permitted(
                "GraphicsConnectionSystem.SetView",
                &json!({"entity":1,"triangles":[]})
            )
            .unwrap_err()
            .contains("active Audicle")
        );
        assert!(!check_payload(
            "TriangleBatch",
            &json!({"entity":1,"triangles":[[1,2,3]]})
        ));
    }
    #[test]
    fn configured_route_is_permitted() {
        let mut host = host();
        host.stack.push((1, "DiagnosticsSystem.Report".into()));
        assert!(
            host.permitted("NativeInventorySystem.Probe", &Value::Null)
                .is_ok()
        );
    }
    #[test]
    fn undeclared_cross_system_call_is_rejected() {
        let mut host = host();
        host.stack.push((1, "NativeInventorySystem.Probe".into()));
        assert!(
            host.permitted("DiagnosticsSystem.Report", &Value::Null)
                .unwrap_err()
                .contains("undeclared")
        );
    }
    #[test]
    fn disabled_system_is_rejected() {
        let mut host = host();
        host.enabled.insert("DiagnosticsSystem".into(), false);
        assert!(
            host.permitted("DiagnosticsSystem.Report", &Value::Null)
                .is_err()
        );
    }
    #[test]
    fn unscheduled_host_entry_is_rejected() {
        assert!(
            host()
                .permitted("NativeInventorySystem.Probe", &Value::Null)
                .is_err()
        );
    }
    #[test]
    fn wrong_input_type_is_rejected() {
        assert!(
            host()
                .permitted("DiagnosticsSystem.Report", &json!(42))
                .is_err()
        );
    }
    #[test]
    fn jack_route_requires_declared_audio_caller() {
        let mut host = host();
        host.stack.push((1, "DiagnosticsSystem.Report".into()));
        assert!(
            host.permitted("JackConnectionSystem.CheckConnection", &Value::Null)
                .is_err()
        );
        host.stack.pop();
        host.stack.push((2, "DiagnosticsSystem.CheckAudio".into()));
        assert!(
            host.permitted("JackConnectionSystem.CheckConnection", &Value::Null)
                .is_ok()
        );
        host.enabled.insert("JackConnectionSystem".into(), false);
        assert!(
            host.permitted("JackConnectionSystem.CheckConnection", &Value::Null)
                .is_err()
        );
    }
    #[test]
    fn halt_circuit_is_respected() {
        let mut host = host();
        host.halt = true;
        assert!(
            host.permitted("DiagnosticsSystem.Report", &Value::Null)
                .is_err()
        );
    }
    fn traced_host() -> Host {
        let mut host = host();
        host.trace = std::env::temp_dir().join(format!("wormwood-test-{}.jsonl", host.run_id));
        host.graphics.test_driver = true;
        host
    }
    fn procedure_host(value: Value) -> Host {
        let mut host = traced_host();
        host.root = std::env::temp_dir().join(format!("wormwood-procedure-{}", host.run_id));
        let folder = host.root.join("Procedures/Cues");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("authored.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        host
    }
    fn default_procedure() -> Value {
        serde_json::from_str(include_str!("../Procedures/Cues/hello_triangle.json")).unwrap()
    }
    fn remove_procedure_host(host: Host) {
        fs::remove_dir_all(host.root).unwrap();
        fs::remove_file(host.trace).unwrap();
    }
    #[test]
    fn authored_cue_data_reaches_entity_creation_through_registered_loader() {
        let mut definition = default_procedure();
        definition["visual"]["vertices"][0][3] = json!(0.37);
        definition.as_object_mut().unwrap().remove("sound");
        let mut host = procedure_host(definition.clone());
        host.audio_enabled = true;
        let result = host
            .invoke("CueSystem.Run", json!({"procedure":"authored"}))
            .unwrap();
        assert_eq!(result["procedure"], "authored");
        assert!(result["audio"].is_null());
        let rows: Vec<Value> = fs::read_to_string(&host.trace)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let created = rows
            .iter()
            .find(|r| r["event"] == "entity_created")
            .unwrap();
        assert!((created["appearance"]["vertices"][0][3].as_f64().unwrap() - 0.37).abs() < 0.00001);
        assert!(rows.iter().any(|r| r["event"] == "procedure_loaded"
            && r["function"] == "CueProcedureSystem.Load"
            && r["audicle"].is_number()));
        assert!(!rows.iter().any(|r| r["event"] == "audio_prepared"));
        assert!(!host.startup_steps.iter().any(|r| r == "CueSystem.Run"));
        remove_procedure_host(host);
    }
    #[test]
    fn invalid_cue_cannot_create_entities_or_reach_ready() {
        let mut definition = default_procedure();
        definition["calls"] = json!(["NativeInventorySystem.Probe"]);
        let mut host = procedure_host(definition);
        assert!(
            host.invoke("CueSystem.Run", json!({"procedure":"authored"}))
                .unwrap_err()
                .contains("unknown cue field")
        );
        let text = fs::read_to_string(&host.trace).unwrap();
        let rows: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert!(!rows.iter().any(|r| r["event"] == "entity_created"
            || r["stage"] == "Ready"
            || r["stage"] == "Go"));
        assert!(rows.iter().any(|r| r["event"] == "audicle_cancelled"));
        assert!(host.active_audicle.is_none());
        remove_procedure_host(host);
    }
    #[test]
    fn cue_reference_rejects_paths_and_loader_requires_declared_caller() {
        let mut host = host();
        for name in ["../LICENSE", "/tmp/cue", "", "a/b", "a\\b"] {
            assert!(
                host.permitted("CueSystem.Run", &json!({"procedure":name}))
                    .is_err()
            );
        }
        host.stack.push((123, "DiagnosticsSystem.Report".into()));
        assert!(
            host.permitted(
                "CueProcedureSystem.Load",
                &json!({"procedure":"hello_triangle"})
            )
            .unwrap_err()
            .contains("undeclared call")
        );
    }
    fn startup_host() -> Host {
        let mut host = traced_host();
        host.output = std::env::temp_dir().join(format!("wormwood-startup-{}", host.run_id));
        fs::create_dir_all(&host.output).unwrap();
        host.trace = host.output.join("execution.jsonl");
        host
    }
    #[test]
    fn startup_runs_once_and_is_not_repeated_between_cues() {
        let mut host = startup_host();
        host.startup_steps = vec!["DiagnosticsSystem.ValidateRuntime".into()];
        let report = host.startup().unwrap();
        assert_eq!(report["started"], true);
        assert_eq!(report["completed_steps"].as_array().unwrap().len(), 1);
        for _ in 0..2 {
            host.invoke("CueSystem.Run", json!({"procedure":"hello_triangle"}))
                .unwrap();
        }
        assert!(
            host.startup()
                .unwrap_err()
                .contains("already been attempted")
        );
        let rows: Vec<Value> = fs::read_to_string(&host.trace)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(
            rows.iter()
                .filter(|r| r["event"] == "started"
                    && r["function"] == "DiagnosticsSystem.ValidateRuntime")
                .count(),
            1
        );
        assert_eq!(
            rows.iter()
                .filter(|r| r["event"] == "audicle_stage" && r["stage"] == "Finished")
                .count(),
            2
        );
        fs::remove_dir_all(host.output).unwrap();
    }
    #[test]
    fn startup_failure_stops_later_steps_and_cannot_be_retried() {
        let mut host = startup_host();
        host.startup_steps = vec![
            "DiagnosticsSystem.ValidateRuntime".into(),
            "NativeInventorySystem.Probe".into(),
            "DiagnosticsSystem.Report".into(),
        ];
        host.roots.push("NativeInventorySystem.Probe".into());
        host.enabled.insert("NativeInventorySystem".into(), false);
        assert!(
            host.startup()
                .unwrap_err()
                .contains("NativeInventorySystem.Probe")
        );
        assert!(host.startup().is_err());
        let report: Value =
            serde_json::from_slice(&fs::read(host.output.join("startup.json")).unwrap()).unwrap();
        assert_eq!(report["started"], false);
        assert_eq!(report["completed_steps"].as_array().unwrap().len(), 1);
        let rows: Vec<Value> = fs::read_to_string(&host.trace)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert!(
            !rows
                .iter()
                .any(|r| r["event"] == "started" && r["function"] == "DiagnosticsSystem.Report")
        );
        fs::remove_dir_all(host.output).unwrap();
    }
    #[test]
    fn startup_plan_follows_systems_list_and_keeps_entries_separate() {
        let original = host();
        let root = std::env::temp_dir().join(format!("wormwood-startup-plan-{}", original.run_id));
        for dir in ["Systems", "BaseSystem", "Procedures"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        for entry in fs::read_dir(original.root.join("Systems")).unwrap() {
            let path = entry.unwrap().path();
            fs::copy(&path, root.join("Systems").join(path.file_name().unwrap())).unwrap();
        }
        for name in [
            "BaseSystem/registry.json",
            "BaseSystem/circuit_breaker.json",
            "Procedures/functionregistry.json",
            "Procedures/systems.json",
        ] {
            fs::copy(original.root.join(name), root.join(name)).unwrap();
        }
        let mut native = read(&root, "Systems/NativeInventorySystem.json").unwrap();
        native["init_steps"] = json!(["Probe"]);
        fs::write(
            root.join("Systems/NativeInventorySystem.json"),
            serde_json::to_vec(&native).unwrap(),
        )
        .unwrap();
        let mut schedule = read(&root, "Procedures/systems.json").unwrap();
        let order = schedule["systems_order"].as_array_mut().unwrap();
        let index = order
            .iter()
            .position(|v| v == "NativeInventorySystem.json")
            .unwrap();
        let native = order.remove(index);
        order.push(native);
        fs::write(
            root.join("Procedures/systems.json"),
            serde_json::to_vec(&schedule).unwrap(),
        )
        .unwrap();
        let loaded = Host::load(&root).unwrap();
        assert_eq!(
            loaded.startup_steps,
            vec![
                "DiagnosticsSystem.ValidateRuntime",
                "DiagnosticsSystem.Report",
                "DiagnosticsSystem.CheckAudio",
                "NativeInventorySystem.Probe"
            ]
        );
        assert!(!loaded.startup_steps.iter().any(|h| h == "CueSystem.Run"));
        assert!(loaded.roots.iter().any(|h| h == "CueSystem.Run"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn window_close_cancels_cue_before_or_after_first_frame_and_allows_reuse() {
        for frames in [0, 1] {
            let mut host = traced_host();
            host.graphics.close_after = Some(frames);
            let report = host
                .invoke("CueSystem.Run", json!({"procedure":"hello_triangle"}))
                .unwrap();
            assert_eq!(report["stage"], "Cancelled");
            assert_eq!(report["reason"], "window_closed");
            assert_eq!(report["presentation"]["frames"], frames);
            assert_eq!(host.entities.len(), 0);
            assert!(host.active_audicle.is_none());
            assert!(host.graphics.prepared_entity().is_none());
            let rows: Vec<Value> = fs::read_to_string(&host.trace)
                .unwrap()
                .lines()
                .map(|l| serde_json::from_str(l).unwrap())
                .collect();
            assert_eq!(
                rows.iter()
                    .filter(|r| r["event"] == "audicle_cancelled")
                    .count(),
                1
            );
            assert!(!rows.iter().any(|r| r["stage"] == "Finished"));
            assert_eq!(
                rows.iter()
                    .filter(|r| r["event"] == "entity_removed")
                    .count(),
                1
            );
            assert_eq!(
                rows.iter()
                    .filter(|r| r["event"] == "native_released")
                    .count(),
                1
            );
            host.graphics.close_after = None;
            assert_eq!(
                host.invoke("CueSystem.Run", json!({"procedure":"hello_triangle"}))
                    .unwrap()["stage"],
                "Finished"
            );
            fs::remove_file(host.trace).unwrap();
        }
    }
    #[test]
    fn cancellation_payload_does_not_accept_empty_normal_presentation_or_unknown_reason() {
        assert!(!check_payload(
            "Presentation",
            &json!({"backend":"OpenGL","frames":3,"end_reason":"ignored_failure"})
        ));
        assert!(!check_payload(
            "Presentation",
            &json!({"backend":"OpenGL","frames":0})
        ));
        assert!(check_payload(
            "Presentation",
            &json!({"backend":"OpenGL","frames":0,"end_reason":"window_closed"})
        ));
        assert!(!check_payload(
            "CueResult",
            &json!({"mode":"cue","stage":"Cancelled","reason":"ignored_failure","entities_remaining":0})
        ));
    }
    #[test]
    fn visual_cue_traces_lifecycle_and_removes_resources() {
        let mut host = traced_host();
        let result = host
            .invoke("CueSystem.Run", json!({"procedure":"hello_triangle"}))
            .unwrap();
        assert_eq!(result["stage"], "Finished");
        assert_eq!(result["presentation"]["backend"], "TestDriver");
        assert!(host.active_audicle.is_none());
        assert_eq!(host.entities.len(), 0);
        assert_eq!(host.graphics.prepared_entity(), None);
        let rows: Vec<Value> = std::fs::read_to_string(&host.trace)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let stages: Vec<&str> = rows
            .iter()
            .filter(|e| e["event"] == "audicle_stage")
            .map(|e| e["stage"].as_str().unwrap())
            .collect();
        assert_eq!(stages, vec!["Preparing", "Ready", "Go", "Finished"]);
        let ready = rows
            .iter()
            .position(|e| e["event"] == "audicle_stage" && e["stage"] == "Ready")
            .unwrap();
        let prepared = rows
            .iter()
            .position(|e| e["event"] == "native_prepared")
            .unwrap();
        let drawn = rows
            .iter()
            .position(|e| e["event"] == "frame_presented")
            .unwrap();
        let go = rows
            .iter()
            .position(|e| e["event"] == "audicle_stage" && e["stage"] == "Go")
            .unwrap();
        assert!(prepared < ready && ready < go && go < drawn);
        assert!(
            rows.iter()
                .filter(|e| e["event"] == "frame_presented")
                .all(|e| e["audicle"].as_u64().is_some())
        );
        std::fs::remove_file(host.trace).unwrap();
    }
    #[test]
    fn presentation_failure_cleans_up_and_cancels() {
        let mut host = traced_host();
        host.graphics.fail_present = true;
        let error = host
            .invoke("CueSystem.Run", json!({"procedure":"hello_triangle"}))
            .unwrap_err();
        assert!(error.contains("injected presentation failure"));
        assert_eq!(host.entities.len(), 0);
        assert!(host.active_audicle.is_none());
        assert_eq!(host.graphics.prepared_entity(), None);
        let text = std::fs::read_to_string(&host.trace).unwrap();
        assert!(text.contains("audicle_cancelled"));
        assert!(!text.contains("\"stage\":\"Finished\""));
        std::fs::remove_file(host.trace).unwrap();
    }
    #[test]
    fn host_can_reuse_cue_state_after_cancel_and_finish() {
        let mut host = traced_host();
        host.graphics.fail_present = true;
        assert!(
            host.invoke("CueSystem.Run", json!({"procedure":"hello_triangle"}))
                .is_err()
        );
        host.graphics.fail_present = false;
        for _ in 0..2 {
            let report = host
                .invoke("CueSystem.Run", json!({"procedure":"hello_triangle"}))
                .unwrap();
            assert_eq!(report["stage"], "Finished");
            assert_eq!(host.entities.len(), 0);
            assert!(host.active_audicle.is_none());
            assert!(host.graphics.prepared_entity().is_none());
        }
        let text = std::fs::read_to_string(&host.trace).unwrap();
        let rows: Vec<Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(
            rows.iter()
                .filter(|r| r["event"] == "audicle_cancelled")
                .count(),
            1
        );
        assert_eq!(
            rows.iter()
                .filter(|r| r["event"] == "audicle_stage" && r["stage"] == "Finished")
                .count(),
            2
        );
        std::fs::remove_file(&host.trace).unwrap();
    }
    #[test]
    fn go_before_ready_is_rejected() {
        let mut host = traced_host();
        host.stack.push((999, "CueSystem.Run".into()));
        let cue = host.invoke("AudicleSystem.Create", Value::Null).unwrap();
        assert!(
            host.invoke("AudicleSystem.Go", cue.clone())
                .unwrap_err()
                .contains("invalid Audicle transition")
        );
        host.invoke("AudicleSystem.Cancel", cue).unwrap();
        std::fs::remove_file(host.trace).unwrap();
    }
    #[test]
    fn presentation_without_go_is_rejected() {
        let mut host = host();
        host.stack.push((999, "AppearanceSystem.Present".into()));
        assert!(
            host.permitted("GraphicsConnectionSystem.Present", &json!({"entity":1}))
                .unwrap_err()
                .contains("in Go")
        );
    }
    #[test]
    fn ios_audio_routes_enforce_go_and_caller_contract() {
        let mut host = host();
        host.stack.push((999, "CueSystem.Run".into()));
        assert!(
            host.permitted("IOSAudioConnectionSystem.Start", &json!({"entity":1}))
                .unwrap_err()
                .contains("in Go")
        );
        host.stack.pop();
        host.stack
            .push((999, "DiagnosticsSystem.CheckAudio".into()));
        assert!(
            host.permitted(
                "IOSAudioConnectionSystem.Prepare",
                &json!({"id":1,"kind":"sound","source":"diagnostic_sine_440"})
            )
            .unwrap_err()
            .contains("undeclared call")
        );
    }
    #[test]
    fn audio_start_without_go_and_undeclared_calls_are_rejected() {
        let mut host = host();
        host.stack.push((999, "CueSystem.Run".into()));
        assert!(
            host.permitted("JackConnectionSystem.Start", &json!({"entity":1}))
                .unwrap_err()
                .contains("in Go")
        );
        host.stack.pop();
        host.stack.push((1000, "DiagnosticsSystem.Report".into()));
        assert!(
            host.permitted(
                "JackConnectionSystem.Prepare",
                &json!({"id":1,"kind":"sound","source":"diagnostic_sine_440"})
            )
            .unwrap_err()
            .contains("undeclared")
        );
        assert!(host.emit(json!({"event":"audio_completed"})).is_err());
    }
    #[test]
    fn unknown_entity_and_malformed_geometry_are_rejected() {
        let mut host = traced_host();
        host.stack.push((999, "AppearanceSystem.Prepare".into()));
        assert!(
            host.invoke("EntitySystem.Get", json!({"entity":123}))
                .unwrap_err()
                .contains("unknown Entity")
        );
        assert!(!check_payload(
            "EntitySpec",
            &json!({"kind":"visual","vertices":[[0,1]],"shader":"vertex_color"})
        ));
        std::fs::remove_file(host.trace).unwrap();
    }
    #[test]
    fn diagnostics_cannot_forge_entity_lifecycle_records() {
        let mut host = host();
        host.stack.push((999, "DiagnosticsSystem.Report".into()));
        assert!(
            host.emit(json!({"event":"entity_created","entity":1}))
                .unwrap_err()
                .contains("cannot emit")
        );
    }
    #[test]
    fn preparation_failure_never_reaches_ready_or_go() {
        let mut host = traced_host();
        host.graphics.fail_prepare = true;
        assert!(
            host.invoke("CueSystem.Run", json!({"procedure":"hello_triangle"}))
                .unwrap_err()
                .contains("injected preparation failure")
        );
        assert_eq!(host.entities.len(), 0);
        assert!(host.active_audicle.is_none());
        let rows: Vec<Value> = std::fs::read_to_string(&host.trace)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert!(!rows.iter().any(|e| e["stage"] == "Ready"
            || e["stage"] == "Go"
            || e["event"] == "frame_presented"));
        assert!(rows.iter().any(|e| e["event"] == "audicle_cancelled"));
        std::fs::remove_file(host.trace).unwrap();
    }
}
