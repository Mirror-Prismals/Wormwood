// Included in Host's tests: exercise the actual named route and native boundary
// without opening a device. The driver captures only after normal validation.
fn workstation_packet_fixture() -> (Host, u64, Vec<[f32; 4]>) {
    let mut h = traced_host();
    h.invoke("AudioWorkstationSystem.Begin", Value::Null)
        .unwrap();
    let entity = h.graphics.prepared_entity().unwrap();
    let rows = h.graphics.workstation_frame.clone().unwrap();
    assert!(!rows.is_empty());
    h.stack.push((9000, "WorkstationPanelSystem.View".into()));
    (h, entity, rows)
}

fn workstation_packet_bits(rows: &[[f32; 4]]) -> Vec<[u32; 4]> {
    rows.iter().map(|row| row.map(f32::to_bits)).collect()
}

fn close_workstation_packet_fixture(mut h: Host) {
    h.stack.pop();
    h.invoke("AudioWorkstationSystem.Close", Value::Null)
        .unwrap();
    drop(h.trace_writer.borrow_mut().take());
    fs::remove_file(&h.trace).unwrap();
}

#[test]
fn workstation_numeric_view_matches_json_and_keeps_invocation_attribution() {
    let (mut h, entity, mut rows) = workstation_packet_fixture();
    let event = h.active_audicle.unwrap();
    let triangles = [[0.0f32; 24]; 2];
    // Include finite extremes and signed zero, which must survive the new
    // borrowed path just as they survive the diagnostic JSON adapter.
    *rows.last_mut().unwrap() = [-0.0, f32::from_bits(1), f32::MIN, f32::MAX];
    let expected = workstation_packet_bits(&rows);
    h.set_workstation_view(entity, &triangles, &rows, true)
        .unwrap();
    assert_eq!(
        workstation_packet_bits(h.graphics.workstation_frame.as_ref().unwrap()),
        expected
    );
    assert_eq!(h.active_audicle, Some(event));
    assert_eq!(h.stack.last().unwrap().1, "WorkstationPanelSystem.View");

    h.invoke(
        "GraphicsConnectionSystem.SetView",
        json!({
            "entity":entity,"triangles":triangles,"workstation":rows,"capture":true,
        }),
    )
    .unwrap();
    assert_eq!(
        workstation_packet_bits(h.graphics.workstation_frame.as_ref().unwrap()),
        expected
    );
    h.trace_writer
        .borrow_mut()
        .as_mut()
        .unwrap()
        .1
        .flush()
        .unwrap();
    let trace: Vec<Value> = fs::read_to_string(&h.trace)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let started = trace
        .iter()
        .rev()
        .find(|row| {
            row["event"] == "started"
                && row["function"] == "GraphicsConnectionSystem.SetView"
                && row["numeric_batch"] == true
        })
        .unwrap();
    assert_eq!(started["caller"], "WorkstationPanelSystem.View");
    assert_eq!(started["parent"], 9000);
    assert_eq!(started["audicle"], event);
    assert_eq!(started["input_type"], "TriangleBatch");
    let finished = trace
        .iter()
        .find(|row| row["event"] == "finished" && row["invocation"] == started["invocation"])
        .unwrap();
    assert_eq!(finished["audicle"], event);
    assert_eq!(finished["numeric_batch"], true);
    close_workstation_packet_fixture(h);
}

#[test]
fn workstation_numeric_view_enforces_routes_and_optical_bounds() {
    let (mut h, entity, rows) = workstation_packet_fixture();
    let triangles = [[0.0f32; 24]; 2];
    let expected = workstation_packet_bits(&rows);
    let event = h.active_audicle.take();
    assert!(
        h.set_workstation_view(entity, &triangles, &rows, false)
            .unwrap_err()
            .contains("active Audicle")
    );
    h.active_audicle = event;
    h.enabled.insert("GraphicsConnectionSystem".into(), false);
    assert!(
        h.set_workstation_view(entity, &triangles, &rows, false)
            .unwrap_err()
            .contains("disabled System")
    );
    h.enabled.insert("GraphicsConnectionSystem".into(), true);
    h.stack.push((9001, "DiagnosticsSystem.Report".into()));
    assert!(
        h.set_workstation_view(entity, &triangles, &rows, false)
            .unwrap_err()
            .contains("undeclared")
    );
    h.stack.pop();
    h.halt = true;
    assert!(
        h.set_workstation_view(entity, &triangles, &rows, false)
            .unwrap_err()
            .contains("circuit breaker")
    );
    h.halt = false;

    assert!(
        h.set_workstation_view(entity + 1, &triangles, &rows, false)
            .unwrap_err()
            .contains("not prepared")
    );
    assert!(h.set_workstation_view(0, &triangles, &rows, false).is_err());
    for length in [0, rows.len() - 1, rows.len() + 1] {
        assert!(
            h.set_workstation_view(entity, &triangles, &vec![[0.0; 4]; length], false)
                .unwrap_err()
                .contains("optical frame")
        );
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut invalid = rows.clone();
        invalid[0][0] = value;
        assert!(
            h.set_workstation_view(entity, &triangles, &invalid, false)
                .unwrap_err()
                .contains("optical frame")
        );
    }
    let mut invalid_triangles = triangles;
    invalid_triangles[0][0] = f32::NAN;
    assert!(
        h.set_workstation_view(entity, &invalid_triangles, &rows, false)
            .unwrap_err()
            .contains("triangle batch")
    );
    assert!(
        h.invoke(
            "GraphicsConnectionSystem.SetView",
            json!({
                "entity":entity,"triangles":triangles,"workstation":[[0.0,0.0,0.0,0.0]],
            })
        )
        .unwrap_err()
        .contains("optical frame")
    );
    assert_eq!(
        workstation_packet_bits(h.graphics.workstation_frame.as_ref().unwrap()),
        expected
    );
    assert_eq!(h.active_audicle, event);
    assert_eq!(h.stack.len(), 1);
    close_workstation_packet_fixture(h);
}

#[test]
#[ignore = "explicit CPU packet transport benchmark; run with --release --ignored --nocapture"]
fn workstation_packet_transport_benchmark() {
    let (mut h, entity, rows) = workstation_packet_fixture();
    let triangles = [[0.0f32; 24]; 2];
    let expected = workstation_packet_bits(&rows);
    let mut json_ms = Vec::new();
    let mut numeric_ms = Vec::new();
    // Paired, alternating order; two warmup pairs are excluded. Both variants
    // retain validation, registered invocation tracing and the driver's copy.
    for pair in 0..22 {
        for numeric in if pair % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            let started = std::time::Instant::now();
            if numeric {
                h.set_workstation_view(entity, &triangles, &rows, false)
                    .unwrap();
            } else {
                h.invoke(
                    "GraphicsConnectionSystem.SetView",
                    json!({
                        "entity":entity,"triangles":triangles,"workstation":rows,"capture":false,
                    }),
                )
                .unwrap();
            }
            let elapsed = started.elapsed().as_secs_f64() * 1000.0;
            if pair >= 2 {
                if numeric {
                    numeric_ms.push(elapsed);
                } else {
                    json_ms.push(elapsed);
                }
            }
            assert_eq!(
                workstation_packet_bits(h.graphics.workstation_frame.as_ref().unwrap()),
                expected
            );
            std::hint::black_box(&h.graphics.workstation_frame);
        }
    }
    let stats = |mut samples: Vec<f64>| {
        samples.sort_by(f64::total_cmp);
        json!({"samples":samples.len(),"mean_ms":samples.iter().sum::<f64>() / samples.len() as f64,
            "p50_ms":samples[samples.len()/2],"p95_ms":samples[samples.len()*95/100],
            "worst_ms":samples.last()})
    };
    println!(
        "{}",
        json!({"benchmark":"workstation_packet_transport","rows":rows.len(),
        "native_bytes":std::mem::size_of_val(rows.as_slice()),"bit_identical":true,
        "scope":"CPU Host dispatch, packet conversion and validation with a capture-only test driver; no GPU upload or rendering",
        "warmup_pairs":2,"json":stats(json_ms),"numeric":stats(numeric_ms)})
    );
    close_workstation_packet_fixture(h);
}

#[test]
fn security_camera_registered_route_composes_outdoors_behind_workstation() {
    let mut h = traced_host();
    h.invoke("TheatreSystem.Configure", Value::Null).unwrap();
    h.invoke("CaveSystem.Configure",Value::Null).unwrap();
    h.stack.push((9000,"CueSystem.Begin".into()));
    let source=h.invoke("EntitySystem.Create",json!({"kind":"visual","shader":"planet_surface","vertices":[[-1.,-1.,0.,1.,1.,1.],[1.,-1.,0.,1.,1.,1.],[0.,1.,0.,1.,1.,1.]]})).unwrap();
    h.stack.pop();
    h.environment.set_camera_test_source(json!({"entity":source["entity"],"instances":[],"time_override_minutes":720,"time":42.}));
    let request=json!({"enabled":true,"embedded":true,"page":"timeline","aspect":16./9.,"dt":0.,"keys":{}});
    let camera=h.invoke("SecurityCameraSystem.View",request.clone()).unwrap();
    assert!(camera["position"].is_array());
    assert_eq!(camera["id"],"riverbank");
    assert_eq!(camera["outdoor"],true);
    assert_eq!(camera["entity"],source["entity"]);
    assert_eq!(camera["time_override_minutes"],720);
    assert_eq!(camera["day_night"]["fraction"],0.5);
    assert!(camera["water_instances"].is_array());
    let mut standalone=request.clone();standalone["embedded"]=json!(false);
    assert!(h.invoke("SecurityCameraSystem.View",standalone).unwrap().is_null());
    h.invoke("AudioWorkstationSystem.Resume",Value::Null).unwrap();
    let entity=h.graphics.prepared_entity().unwrap();
    let mut view=json!({"entity":entity,"width":1280.,"height":720.,"framebuffer":[1280.,720.],"canvas_scale":1.,
        "extensions":[0.,0.,0.,0.],"open":[true,true,true,true],"opening_order":[0,1,2,3],
        "left_width_scale":1.,"bottom_height_scale":1.,"overview_hide":[0.,0.,0.,0.],"camera_view":camera});
    h.stack.push((9001,"AudioWorkstationSystem.Step".into()));
    h.invoke("WorkstationPanelSystem.View",view.clone()).unwrap();
    assert_eq!(h.graphics.workstation_frame.as_ref().unwrap()[9][2],1.);
    let scene=h.graphics.scene_frame.as_ref().unwrap();
    assert_eq!(scene.view["day_night"]["fraction"],0.5);
    assert!(!scene.water.is_empty(),"riverbank must use native water packets");
    assert!(scene.vegetation.is_some(),"camera must retain the normal detailed vegetation pass");
    assert!(scene.triangles.iter().any(|t|t[15]< -9.5),"normal per-pixel sky must survive");
    assert!(!scene.triangles.iter().any(|t|t[15]==90.),"no baked scenic material/depth conversion");
    h.stack.pop();
    let mut cycle=request.clone();cycle["keys"]=json!({"cycle":true});
    h.invoke("SecurityCameraSystem.View",cycle.clone()).unwrap();
    h.invoke("SecurityCameraSystem.View",request.clone()).unwrap();
    let theatre=h.invoke("SecurityCameraSystem.View",cycle).unwrap();
    assert_eq!(theatre["id"],"theatre");assert_eq!(theatre["outdoor"],false);
    h.stack.push((9001,"AudioWorkstationSystem.Step".into()));
    view["camera_view"]=theatre;
    h.invoke("WorkstationPanelSystem.View",view.clone()).unwrap();
    assert_eq!(h.graphics.workstation_frame.as_ref().unwrap()[9][2],1.);
    view["camera_view"]=Value::Null;
    h.invoke("WorkstationPanelSystem.View",view).unwrap();
    assert_eq!(h.graphics.workstation_frame.as_ref().unwrap()[9][2],0.);
    assert!(h.graphics.scene_frame.is_none());
    h.stack.pop();
    h.invoke("AudioWorkstationSystem.Close",Value::Null).unwrap();
    drop(h.trace_writer.borrow_mut().take());
    let trace=fs::read_to_string(&h.trace).unwrap();
    assert!(trace.contains("SecurityCameraSystem.View"));
    fs::remove_file(&h.trace).unwrap();
}

#[test]
fn embedded_daw_reuses_prepared_world_without_replacing_it_or_losing_display_rows(){
 let (mut host,entity,mut rows)=workstation_packet_fixture();
 assert!(host.reuse_workstation_scene(entity,&rows,false,&[]).unwrap_err().contains("not prepared"));
 let triangle=[0.125f32;24];host.graphics.scene_frame=Some(crate::base_system::layer_four::appearance_system::Scene{triangles:vec![triangle],view:json!({"position":[1,2,3]}),water:Vec::new(),vegetation:None});
 rows[252][0]=0.42;host.reuse_workstation_scene(entity,&rows,false,&[]).unwrap();
 assert_eq!(host.graphics.scene_frame.as_ref().unwrap().triangles,vec![triangle]);
 assert_eq!(host.graphics.workstation_frame.as_ref().unwrap()[252][0],0.42);
 close_workstation_packet_fixture(host);
}

#[test]
fn wearable_resume_accepts_typed_context_and_preserves_ray_toggle_on_suspend(){
 let mut h=traced_host();
 for kind in ["ar","vr"]{
  assert!(h.invoke("AudioWorkstationSystem.Resume",json!({"eyewear":"invalid"})).unwrap_err().contains("invalid input"));
  let status=h.invoke("AudioWorkstationSystem.Resume",json!({"eyewear":kind})).unwrap();assert_eq!(status["eyewear"],kind);
  h.audio_workstation.audio_rays=true;
  h.invoke("AudioWorkstationSystem.Suspend",Value::Null).unwrap();
  assert_eq!(h.invoke("AudioWorkstationSystem.Status",Value::Null).unwrap()["audio_rays"],true);
 }
 h.invoke("AudioWorkstationSystem.Resume",Value::Null).unwrap();assert!(h.invoke("AudioWorkstationSystem.Status",Value::Null).unwrap()["eyewear"].is_null());h.invoke("AudioWorkstationSystem.Close",Value::Null).unwrap();
 drop(h.trace_writer.borrow_mut().take());fs::remove_file(&h.trace).unwrap();
}
