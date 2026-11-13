//! Opt-in release benchmark of the game's actual material activation and mining path.
//! No graphics/device FPS is inferred from these CPU timings.
use super::*;
use std::time::Instant;

fn stats(v: &[f64]) -> Value {
    if v.is_empty() {
        return Value::Null;
    }
    let mut sorted = v.to_vec();
    sorted.sort_by(f64::total_cmp);
    json!({"mean_ms":v.iter().sum::<f64>()/v.len() as f64,
        "p95_ms":sorted[((v.len()-1) as f64*0.95) as usize],
        "max_ms":sorted[v.len()-1],"samples":v.len()})
}

fn mining_step(h: &mut Host, eye: [f64; 3], work: bool, pour: bool) -> (Value, f64) {
    let terrain = h.invoke("PlanetSurfaceSystem.Frame", Value::Null).unwrap();
    let started = Instant::now();
    let status = h.invoke("PhysicsSystem.GameStep", json!({"position":eye,
        "aim":[0.,-0.875,-0.484122918],"up":[0.,1.,0.],"dt":1./60.,
        "terrain":terrain,"solver_benchmark":true,"tools":{"slot":0,"work":work,"pour":pour}})).unwrap();
    (status, started.elapsed().as_secs_f64() * 1000.)
}

#[test]
#[ignore = "explicit CPU mining stress benchmark; run with --release --ignored --nocapture"]
fn mining_stress() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let label = std::env::var("MIRROR_MINING_LABEL").unwrap_or_else(|_| "latest".into());
    assert!(label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    let sites: usize = std::env::var("MIRROR_MINING_SITES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let frames: usize = std::env::var("MIRROR_MINING_FRAMES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(240);
    let snapshots = std::env::var_os("MIRROR_MINING_WATER").is_some();
    let render = std::env::var_os("MIRROR_MINING_RENDER").is_some();
    let revisits: usize = std::env::var("MIRROR_MINING_REVISITS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    assert!(revisits <= 8);
    assert!((1..=64).contains(&sites) && (67..=3600).contains(&frames),
        "replay needs 36 charge frames, release, then its separate final-30-frame pour input");
    let output = root.join("output/benchmarks/mining-20261029").join(&label);
    fs::create_dir_all(&output).unwrap();
    let mut h = Host::load_paths(root, &output).unwrap();
    h.invoke("CaveSystem.Configure", Value::Null).unwrap();
    h.stack.push((1, "PlanetNavigationSystem.Step".into()));
    let event = h.invoke("AudicleSystem.Create", Value::Null).unwrap();
    h.invoke("AudicleSystem.Ready", event.clone()).unwrap();
    h.invoke("AudicleSystem.Go", event.clone()).unwrap();
    let mut runs = Vec::new();
    let mut failed = false;
    let mut total_frames = 0;
    for visit in 0..sites * (1 + revisits) {
        let site = visit % sites;
        let before = h.physics.mining_diagnostics();
        let region_frames_before = before[site]["physics"]["frames"].as_u64().unwrap_or(0);
        let x = site as f64 * 3.;
        let r = crate::terrain::RADIUS;
        let l = (x * x + r * r).sqrt();
        let height = crate::terrain::height([x / l, r / l, 0.]);
        let eye = [x, r + height + 1.45, 0.8];
        let mut step = Vec::new();
        let mut transport = Vec::new();
        let mut collision = Vec::new();
        let mut water_snapshot = Vec::new();
        let mut water_index = Vec::new();
        let mut presentation = Vec::new();
        let mut activation = 0.;
        let mut idle = Vec::new();
        let mut status = Value::Null;
        let mut pour_started = false;
        for frame in 0..frames {
            let was_active = status["active"] == true;
            let (next, ms) = mining_step(&mut h, eye, frame < 36, frame + 30 >= frames);
            status = next;
            if frame + 30 == frames && !was_active && status["active"] == true {
                pour_started = true;
            }
            if frame == 36 {
                assert!(!was_active, "visit {visit} dig release must begin with no active stroke");
                assert_eq!(status["active"], true,
                    "visit {visit} must accept its dig release: {status}");
                assert_eq!(h.physics.mining_diagnostics().as_array().unwrap().len(),
                    if visit < sites { site + 1 } else { sites },
                    "visit {visit} must activate its intended real material region");
                activation = ms;
            } else if frame < 36 {
                idle.push(ms);
            } else {
                step.push(ms);
            }
            let started = Instant::now();
            let mesh = h
                .material_vertices(eye, [0., -0.875, -0.484122918], [0., 1., 0.], 1.7)
                .unwrap();
            transport.push(started.elapsed().as_secs_f64() * 1000.);
            if render {
                let surface = h.invoke("PlanetSurfaceSystem.Frame", Value::Null).unwrap();
                let view = json!({"position":eye,"right":[1.,0.,0.],"up":[0.,0.484122918,-0.875],"forward":[0.,-0.875,-0.484122918],"yaw":0.,"pitch":0.,"aspect":1.7,"day_night":{"fraction":0.5}});
                let started = Instant::now();
                let geometry = crate::base_system::layer_four::appearance_system::benchmark_planet(
                    &view, &surface, &mesh,
                )
                .unwrap();
                std::hint::black_box(geometry);
                presentation.push(started.elapsed().as_secs_f64() * 1000.);
            }
            std::hint::black_box(mesh);
            let started = Instant::now();
            h.invoke(
                "PhysicsSystem.GameResolveBody",
                json!({"previous":eye,"position":eye,"eye_height":1.45}),
            )
            .unwrap();
            collision.push(started.elapsed().as_secs_f64() * 1000.);
            if snapshots {
                let started = Instant::now();
                h.stack.push((2, "WaterSystem.Step".into()));
                let snapshot = h.water_collision_snapshot(eye).unwrap();
                water_snapshot.push(started.elapsed().as_secs_f64() * 1000.);
                let started = Instant::now();
                h.update_water_collision(snapshot).unwrap();
                water_index.push(started.elapsed().as_secs_f64() * 1000.);
                h.stack.pop();
            }
        }
        // The measured replay can be shorter than the actual dig/pour stroke.
        // Finish the accepted work here, at the same location, before the next
        // visit sends a fresh charge/release. These extra ticks are reported
        // separately and do not enter the measured frame pipeline samples.
        let settling_started = Instant::now();
        let mut settling = Vec::new();
        while status["active"] == true {
            assert!(settling.len() < 600, "visit {visit} stroke did not finish: {status}");
            let (next, ms) = mining_step(&mut h, eye, false, false);
            status = next;
            settling.push(ms);
        }
        let finishing_measured_stroke_frames = settling.len();
        let explicit_pour = !pour_started;
        if explicit_pour {
            // A pour edge during the dig is intentionally ignored by gameplay.
            // Release that input, then perform one actual complete pour instead
            // of carrying an unfinished tool stroke into the next location.
            let (next, ms) = mining_step(&mut h, eye, false, false);
            status = next;
            settling.push(ms);
            assert_ne!(status["active"], true, "visit {visit} did not settle before pour");
            let (next, ms) = mining_step(&mut h, eye, false, true);
            status = next;
            settling.push(ms);
            assert_eq!(status["active"], true, "visit {visit} must accept its pour: {status}");
            while status["active"] == true {
                assert!(settling.len() < 600, "visit {visit} pour did not finish: {status}");
                let (next, ms) = mining_step(&mut h, eye, false, false);
                status = next;
                settling.push(ms);
            }
        }
        let settling_wall_ms = settling_started.elapsed().as_secs_f64() * 1000.;
        let visit_total_frames = frames + settling.len();
        total_frames += visit_total_frames;
        if render {
            let surface = h.invoke("PlanetSurfaceSystem.Frame", Value::Null).unwrap();
            let view = json!({"position":eye,"right":[1.,0.,0.],"up":[0.,0.484122918,-0.875],"forward":[0.,-0.875,-0.484122918],"yaw":0.,"pitch":0.,"aspect":1.7,"day_night":{"fraction":0.5}});
            let visible = h
                .material_vertices(eye, [0., -0.875, -0.484122918], [0., 1., 0.], 1.7)
                .unwrap();
            let all = h.invoke("PhysicsSystem.GameFrame", Value::Null).unwrap();
            let all: Vec<[f64; 6]> = serde_json::from_value(all["vertices"].clone()).unwrap();
            let project = crate::base_system::layer_four::appearance_system::benchmark_planet;
            assert_eq!(
                project(&view, &surface, &visible).unwrap(),
                project(&view, &surface, &all).unwrap(),
                "coarse material culling must retain every visible triangle"
            );
        }
        let diagnostics = h.physics.mining_diagnostics();
        let region_frames_after = diagnostics[site]["physics"]["frames"].as_u64()
            .expect("visited material region must exist");
        assert!(region_frames_after - region_frames_before >= (visit_total_frames - 36) as u64,
            "visit {visit} must advance intended region {site} throughout its accepted work");
        failed |= diagnostics
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["physics"]["halted"] == true);
        let run = json!({"site":site,"visit":visit,"idle":stats(&idle),"activation_ms":activation,
            "measured_frames":frames,"settling_frames":settling.len(),"total_frames":visit_total_frames,
            "finishing_measured_stroke_frames":finishing_measured_stroke_frames,
            "explicit_completed_pour":explicit_pour,"settling_game_step":stats(&settling),
            "settling_wall_ms":settling_wall_ms,"region_frames_before":region_frames_before,
            "region_frames_after":region_frames_after,
            "live_step":stats(&step),"mesh_transport":stats(&transport),"body_collision":stats(&collision),
            "water_snapshot":stats(&water_snapshot),
            "water_index":stats(&water_index),
            "terrain_presentation_cpu":stats(&presentation),
            "status":status["status"],"regions":diagnostics});
        eprintln!("mining {}", serde_json::to_string(&run).unwrap());
        runs.push(run);
    }
    let report = json!({"label":label,"replay_version":2,"sites":sites,"revisits":revisits,"water_snapshots":snapshots,"numeric_snapshot":true,"terrain_presentation":render,"frames_per_site":frames,
        "measured_frames_per_site":frames,"total_frames":total_frames,
        "settling_policy":"finish accepted stroke at same site; perform and finish pour if measured pour edge was ignored; settling runs GameStep only",
        "fixed_dt":1./60.,"cpu_only":true,"failed":failed,"runs":runs});
    fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    assert!(!failed, "material simulation halted; see report");
    assert_eq!(
        h.physics.mining_diagnostics().as_array().unwrap().len(),
        sites,
        "each stroke must activate real world matter"
    );
    h.invoke("AudicleSystem.Finish", event).unwrap();
    h.stack.pop();
    h.stack.push((2, "PlanetNavigationSystem.Stop".into()));
    h.invoke("PhysicsSystem.GameClose", Value::Null).unwrap();
}

#[test]
fn click_mine_and_refill() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = std::env::temp_dir().join(format!("wormwood-mine-{}",std::process::id()));
    let mut h = Host::load_paths(root,&out).unwrap();
    h.invoke("CaveSystem.Configure",Value::Null).unwrap();
    h.stack.push((1,"PlanetNavigationSystem.Step".into()));
    let event=h.invoke("AudicleSystem.Create",Value::Null).unwrap();
    h.invoke("AudicleSystem.Ready",event.clone()).unwrap();
    h.invoke("AudicleSystem.Go",event).unwrap();
    let r=crate::terrain::RADIUS;
    let eye=[0.21,r+crate::terrain::height([0.,1.,0.])+1.45,0.84];
    let step=|host:&mut Host, work, fill| {
        let surface=host.invoke("PlanetSurfaceSystem.Frame",Value::Null).unwrap();
        let start=Instant::now();
        let result=host.invoke("PhysicsSystem.GameStep",json!({"position":eye,"aim":[0.,-0.72,-0.693974],
            "up":[0.,1.,0.],"dt":1./60.,"terrain":surface,"tools":{"slot":0,"work":work,"pour":fill}})).unwrap();
        eprintln!("click {:?} {}",start.elapsed(),result["status"]);
        host.invoke("PlanetSurfaceSystem.Frame",Value::Null).unwrap()["edits"].clone()
    };
    let render = |host:&mut Host| {
        let surface=host.invoke("PlanetSurfaceSystem.Frame",Value::Null).unwrap();
        let view=json!({"position":eye,"right":[1.,0.,0.],"up":[0.,0.693974,-0.72],
            "forward":[0.,-0.72,-0.693974],"yaw":0.,"pitch":0.,"aspect":1.7,"day_night":{"fraction":0.5}});
        let start=Instant::now();
        let mesh=crate::base_system::layer_four::appearance_system::benchmark_planet(&view,&surface,&[]).unwrap();
        eprintln!("terrain presentation {:?} ({} triangles)",start.elapsed(),mesh.len());
        mesh
    };
    let original=render(&mut h);
    let cut=step(&mut h,true,false);
    let excavated=render(&mut h);
    assert!(excavated != original);
    if let Some(path)=std::env::var_os("MIRROR_MINING_PREVIEW") {
        fs::write(path,serde_json::to_vec(&[&original,&excavated]).unwrap()).unwrap();
    }
    assert_eq!(cut.as_array().unwrap().len(),1);
    let domain=crate::terrain::material_domain::Domain::parse(&cut[0]).unwrap();
    let center=domain.world([0.325;3]);
    let feet=[center[0],r+crate::terrain::height([0.,1.,0.])+1.84,center[2]];
    let support=h.invoke("PlanetSurfaceSystem.Height",json!({"position":feet,"eye_height":1.84})).unwrap();
    assert!(support["height"].as_f64().unwrap() <= domain.origin[1]-r+0.02);
    assert_eq!(step(&mut h,true,false),cut);
    assert_eq!(step(&mut h,false,false),cut);
    assert_eq!(step(&mut h,false,true),json!([]));
    assert!(render(&mut h) == original);
    assert_eq!(h.physics.mining_diagnostics(),json!([]));
}
