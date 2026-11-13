// Private diagnostic source, appended only to the frozen evidence checkout.
// All appearance preparation/submission still uses registered production hooks.
fn state_family_request(input: &Value, embedded: bool, limit: Option<u64>, benchmark: bool)
    -> Result<Option<Value>, String>
{
    let Some(request) = std::env::var_os("WORMWOOD_STATE_FAMILY") else { return Ok(None); };
    let request: Value = serde_json::from_str(&request.to_string_lossy()).map_err(|e| format!("state-family JSON: {e}"))?;
    let states = request["states"].as_array().filter(|s| !s.is_empty() && s.len() <= 128)
        .ok_or("state-family requires 1..128 explicit states")?;
    let run = request["run"].as_str().ok_or("state-family run name missing")?;
    let safe = |name: &str| !name.is_empty() && name.len() <= 64
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if !safe(run) || embedded || benchmark || limit != Some(states.len() as u64)
        || input["diagnostic_bubbles"] == true || input["diagnostic_spheres"] == true
        || input["diagnostic_arrangement"] == true || input["diagnostic_controls"] == true
    { return Err("state-family requires its matching standalone finite frame count".into()); }
    let mut ids = std::collections::BTreeSet::new();
    for state in states {
        let id = state["id"].as_str().ok_or("state-family id missing")?;
        let extensions: [f64; 4] = serde_json::from_value(state["extensions"].clone())
            .map_err(|_| "state-family needs four extensions")?;
        if !safe(id) || !ids.insert(id) || extensions.iter().any(|x| !x.is_finite() || !(0.0..=1.0).contains(x)) {
            return Err("invalid or duplicate state-family state".into());
        }
    }
    Ok(Some(request))
}

fn state_family_step(state: &mut State, host: &mut Host, request: &Value) -> Result<Value, String> {
    if std::env::var_os("WORMWOOD_STATE_FAMILY").is_none() {
        return Err("state-family Step requires explicit diagnostic mode".into());
    }
    let extensions: [f64; 4] = serde_json::from_value(request["extensions"].clone())
        .map_err(|_| "state-family needs four extensions")?;
    if extensions.iter().any(|x| !x.is_finite() || !(0.0..=1.0).contains(x)) {
        return Err("state-family extension outside 0..1".into());
    }
    state.begin_event(host)?;
    let motion = crate::workstation::panel_motion::Snapshot {
        extensions, open: [true; 4], opening_order: [1, 2, 3, 4],
        left_width_scale: 1., bottom_height_scale: 1., overview_hide: [0.; 4],
    };
    let view = Viewport { width: 640., height: 480., canvas_scale: 1. };
    let rect = piano_roll_scene::canvas(view, &motion, Options::default())?;
    // A new standalone process starts with the actual empty document. Keep
    // its current arrangement and plugin presentation, at a frozen playhead.
    let timeline = state.arrangement.presentation(rect, Some(0.), Value::Null);
    let plugins = state.plugins.presentation();
    host.invoke("WorkstationPanelSystem.View", json!({
        "entity":state.visual.as_ref().ok_or("state-family scene missing")?["entity"],
        "extensions":motion.extensions,"open":motion.open,"opening_order":motion.opening_order,
        "left_width_scale":1.,"bottom_height_scale":1.,"overview_hide":motion.overview_hide,
        "canvas_scale":1.,"swap_sides":false,"swap_horizontal":false,
        "width":640.,"height":480.,"framebuffer":[640.,480.],"capture":true,"style_time":0.,
        "bubbles":Value::Null,"spheres":Value::Null,"timeline":timeline,"plugins":plugins,
    }))?;
    state.frames += 1;
    Ok(state.status())
}

fn state_family_capture(host: &mut Host, request: &Value, visual: &Value, frames: &mut u64)
    -> Result<(), String>
{
    let states = request["states"].as_array().ok_or("state-family states missing")?;
    let run = request["run"].as_str().ok_or("state-family run missing")?;
    let root = host.output_root().to_path_buf();
    let directory = root.join("captures");
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let optical_path = root.join("workstation-optical-frame.json");
    let image_path = root.join("triangle-WebGPU.ppm");
    let manifest_path = root.join(format!("state-family-{run}.json"));
    let mut completed = Vec::new();
    let started = Instant::now();
    for state in states {
        if started.elapsed() >= Duration::from_secs(300) {
            return Err("state-family capture exceeded 300 seconds".into());
        }
        let id = state["id"].as_str().ok_or("state-family id missing")?;
        for path in [&optical_path, &image_path] {
            match std::fs::remove_file(path) {
                Ok(()) => {},
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                Err(error) => return Err(error.to_string()),
            }
        }
        let before = Instant::now();
        host.invoke("AudioWorkstationSystem.Step", json!({"state_family":state}))?;
        let mut pump = visual.clone();
        pump["force_draw"] = json!(true);
        let window = host.invoke("GraphicsConnectionSystem.Pump", pump)?;
        if window["closed"] == true || window["drawn"] != true
            || window["width"].as_u64() != Some(640) || window["height"].as_u64() != Some(480)
            || window["logical_width"].as_f64() != Some(640.) || window["logical_height"].as_f64() != Some(480.)
        { return Err(format!("state-family capture {id}: drawable/logical dimensions changed: {window}")); }
        let optical_bytes = std::fs::read(&optical_path).map_err(|e| e.to_string())?;
        let optical: Value = serde_json::from_slice(&optical_bytes).map_err(|e| e.to_string())?;
        if optical["logical_viewport"] != json!([640.,480.])
            || optical["rows"][8][0].as_f64() != Some(640.) || optical["rows"][8][1].as_f64() != Some(480.)
        { return Err(format!("state-family {id}: optical sampling dimensions changed")); }
        let image = std::fs::read(&image_path).map_err(|e| e.to_string())?;
        let header = b"P6\n640 480\n255\n";
        if !image.starts_with(header) || image.len() != header.len() + 640 * 480 * 3 {
            return Err(format!("state-family {id}: invalid canonical 640x480 capture"));
        }
        std::fs::write(directory.join(format!("{id}.ppm")), image).map_err(|e| e.to_string())?;
        std::fs::write(directory.join(format!("{id}.json")), optical_bytes).map_err(|e| e.to_string())?;
        *frames += 1;
        completed.push(json!({"state":state,"frame":*frames,"wall_ms":before.elapsed().as_secs_f64()*1000.,
            "image":format!("captures/{id}.ppm"),"optical":format!("captures/{id}.json"),
            "dimensions":[640,480],"logical_dimensions":[640,480]}));
        let report = json!({"schema":1,"run":run,"complete":completed.len()==states.len(),
            "requested":states.len(),"captures":completed,"backend":"WebGPU",
            "scope":"Canonical native fragment render and readback at exact supplied panel extensions; no quality override."});
        let temporary = manifest_path.with_extension("json.tmp");
        std::fs::write(&temporary, serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        std::fs::rename(temporary, &manifest_path).map_err(|e| e.to_string())?;
        println!("state-family {}/{} {} captured at 640x480", completed.len(), states.len(), id);
    }
    Ok(())
}
