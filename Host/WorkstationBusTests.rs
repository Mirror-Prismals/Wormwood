// Registered Host routes and the real Step controller, using the graphics test
// driver. This is control/geometry evidence, not native visual or device QA.
#[test]
fn workstation_output_bus_meter_selection_delete_and_io_are_registered() {
    let mut h = traced_host();
    workstation_bubble_begin(&mut h);
    let status = workstation_bubble_step(&mut h, Value::Null);
    let controls = status["bottom_panel"]["controls"].as_array().unwrap();
    let c = controls
        .iter()
        .find(|c| {
            c["id"]
                .as_str()
                .is_some_and(|id| id == "SelectBus(\"matrix-L\")")
        })
        .expect("matrix meter hit target");
    let r = &c["rect"];
    let p = [r[0].as_f64().unwrap() + 15., r[1].as_f64().unwrap() + 20.];
    workstation_bubble_step(&mut h, json!({"pointer":p,"left":true}));
    let selected = workstation_bubble_step(&mut h, json!({"pointer":p,"left":false}));
    assert_eq!(selected["bottom_panel"]["selected_bus"], "matrix-L");
    let io = selected["track_io"]["controls"].as_array().unwrap();
    assert!(
        io.iter()
            .any(|c| c["key"] == "bus_input" && c["track"] == "matrix-L")
    );
    assert!(io.iter().filter(|c| c["track"] == "matrix-L")
        .all(|c| c["key"] != "bus_output" && c["key"] != "bus_channel"));
    let deleted = workstation_bubble_step(&mut h, json!({"pointer":p,"delete":true}));
    assert!(deleted["bottom_panel"]["selected_bus"].is_null());
    let doc = workstation_bubble_snapshot(&mut h);
    assert_eq!(doc["tracks"], json!([]));
    assert!(
        !doc["buses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["id"] == "matrix-L")
    );
    h.invoke(
        "WorkstationDataSystem.Undo",
        json!({"document_revision":doc["revision"]}),
    )
    .unwrap();
    assert!(
        workstation_bubble_snapshot(&mut h)["buses"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["id"] == "matrix-L")
    );
    workstation_bubble_close(&mut h);
    workstation_bubble_remove_trace(h);
}
#[cfg(target_os = "macos")]
#[test]
fn workstation_output_bus_browser_creates_mix_and_accepts_stock_fx_only_on_mix() {
    use crate::workstation::{
        panel_layout::{Options, Viewport},
        panel_motion, plugin_browser,
    };
    let mut h = traced_host();
    workstation_bubble_begin(&mut h);
    let mut motion = panel_motion::State::default();
    motion.set_all_open(true);
    motion.advance(1.);
    let view = Viewport {
        width: 1280.,
        height: 720.,
        canvas_scale: 1.,
    };
    let layout =
        plugin_browser::layout(view, &motion.snapshot(), Options::default(), &[], 0, 0.).unwrap();
    let row = layout
        .rows
        .iter()
        .find(|r| r.item == Some(plugin_browser::Item::Component("mix")))
        .unwrap();
    let p = [row.rect[0] + 12., row.rect[1] + row.rect[3] * 0.5];
    let status = workstation_bubble_step(&mut h, Value::Null);
    let r = &status["bottom_panel"]["panel"];
    let dest = [r[0].as_f64().unwrap() + 160., r[1].as_f64().unwrap() + 50.];
    workstation_bubble_step(&mut h, json!({"pointer":p,"left":true}));
    workstation_bubble_step(&mut h, json!({"pointer":dest,"left":true}));
    workstation_bubble_step(&mut h, json!({"pointer":dest,"left":false}));
    let doc = workstation_bubble_snapshot(&mut h);
    assert_eq!(doc["tracks"], json!([]));
    assert_eq!(
        doc["buses"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|b| b["kind"] == "mix")
            .count(),
        2
    );
    let bus = doc["buses"].as_array().unwrap().last().unwrap()["id"].clone();
    let page = workstation_bubble_step(&mut h, Value::Null);
    let c = page["bottom_panel"]["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "SelectPage(1)")
        .unwrap();
    let r = &c["rect"];
    let p = [r[0].as_f64().unwrap() + 11., r[1].as_f64().unwrap() + 11.];
    workstation_bubble_step(&mut h, json!({"pointer":p,"left":true}));
    workstation_bubble_step(&mut h, json!({"pointer":p,"left":false}));
    let catalog = h.workstation.with(|s| Ok(s.plugins.status())).unwrap()["catalog"]
        .as_array()
        .unwrap()
        .clone();
    let layout = plugin_browser::layout(
        view,
        &motion.snapshot(),
        Options::default(),
        &catalog,
        0,
        0.,
    )
    .unwrap();
    let row = layout
        .rows
        .iter()
        .find(|r| r.text == "U Shaping EQ")
        .unwrap();
    let from = [row.rect[0] + 8., row.rect[1] + row.rect[3] * 0.5];
    workstation_bubble_step(&mut h, json!({"pointer":from,"left":true}));
    workstation_bubble_step(&mut h, json!({"pointer":dest,"left":true}));
    let dropped = workstation_bubble_step(&mut h, json!({"pointer":dest,"left":false}));
    assert!(dropped["arrangement_error"].is_null(), "{dropped}");
    let status = h.workstation.with(|s| Ok(s.plugins.status())).unwrap();
    assert!(
        status["inserts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["track"] == bus)
    );
    assert!(
        crate::base_system::layer_one::plugin_connection_system::dispatch(
            &mut h,
            "Command",
            &json!({"op":"add","track":"matrix-L","stock":"LShaping"})
        )
        .is_err()
    );
    workstation_bubble_close(&mut h);
    workstation_bubble_remove_trace(h);
}

#[test]
fn workstation_trackless_clip_channel_routing_uses_registered_hooks_and_undo(){
 let mut h=traced_host();
 let mut edit=|hook:&str,mut v:Value|{let snap=h.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap();v["document_revision"]=snap["revision"].clone();h.invoke(hook,v).unwrap()};
 edit("WorkstationDataSystem.PutSource",json!({"id":"src","sample_rate":8000,"channels":2,"samples":[1.,2.,0.,0.]}));
 edit("WorkstationDataSystem.PutBus",json!({"id":"channel-1","kind":"channel","name":"Channel Bus 1","output":"matrix-L"}));
 edit("WorkstationDataSystem.PutClip",json!({"id":"free","kind":"audio","source":"src","start_seconds":0.,"source_offset_seconds":0.,"duration_seconds":2./8000.,"output":"channel-1"}));
 edit("WorkstationDataSystem.SetClipRouting",json!({"id":"free","output":"matrix-R"}));
 let pcm=edit("WorkstationDataSystem.RenderRange",json!({"sample_rate":8000,"start_frame":0,"frames":2}));assert_eq!(pcm["matrix_pcm"][0],json!([0.,0.,0.,3.]));
 edit("WorkstationDataSystem.Undo",json!({}));
 let pcm=edit("WorkstationDataSystem.RenderRange",json!({"sample_rate":8000,"start_frame":0,"frames":2}));assert_eq!(pcm["matrix_pcm"][0],json!([3.,0.,0.,0.]));
 let snapshot=h.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap();assert_eq!(snapshot["tracks"],json!([]));
 assert_eq!(snapshot["buses"].as_array().unwrap().last().unwrap()["timeline"],false);
 drop(h.trace_writer.borrow_mut().take());fs::remove_file(&h.trace).unwrap();
}
