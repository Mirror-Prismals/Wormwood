// Exercise the actual numbered page controls; Cardinal starts on Mixer.
#[cfg(target_os="macos")]
fn workstation_open_plugins_page(h:&mut Host,step:&mut Value){
 use crate::workstation::{bottom_panel as bottom,panel_layout::{self,Options,Viewport},panel_motion};
 let mut motion=panel_motion::State::default();motion.set_all_open(true);motion.advance(1.);
 let r=panel_layout::visual_bounds(Viewport{width:1280.,height:720.,canvas_scale:1.},&motion.snapshot(),Options::default()).unwrap()[1];
 let layout=bottom::State::default().layout(bottom::Rect{x:r.x,y:r.y,w:r.width,h:r.height},&bottom::Context::default());
 let c=layout.controls.iter().find(|c|c.id==bottom::ControlId::SelectPage(1)).unwrap();
 let p=[c.rect.x+c.rect.w*0.5,c.rect.y+c.rect.h*0.5];
 step["dt"]=json!(0.);
 for left in [true,false]{step["editor"]=json!({"pointer":p,"left":left});h.invoke("AudioWorkstationSystem.Step",step.clone()).unwrap();}
}

// Included by Host::tests. Uses the real Step/Command/RenderRange routes with
// the existing graphics test driver; never scans or opens third-party windows.
#[cfg(target_os = "macos")]
#[test]
fn cardinal_plugin_browser_drop_stock_graph_and_labels_use_registered_routes() {
    use crate::workstation::{
        arrangement,
        panel_layout::{Options, Viewport},
        panel_motion, piano_roll_scene, plugin_browser,
    };
    let mut h = traced_host();
    h.invoke(
        "WorkstationDataSystem.PutTrack",
        json!({"document_revision":0,"id":"a","name":"Audio","kind":"audio"}),
    )
    .unwrap();
    h.invoke("AudioWorkstationSystem.Begin", Value::Null)
        .unwrap();
    let mut step = json!({"dt":1.,"aspect":1280./720.,"viewport":[1280.,720.],"framebuffer":[1280.,720.],"keys":[false,false,false,false,false],"editor":{"pointer":[640.,360.],"left":false}});
    h.invoke("AudioWorkstationSystem.Step", step.clone())
        .unwrap();
    workstation_enable_browser(&mut h);
    let mut motion = panel_motion::State::default();
    motion.set_all_open(true);
    motion.advance(1.);
    let view = Viewport {
        width: 1280.,
        height: 720.,
        canvas_scale: 1.,
    };
    let options = Options::default();
    let catalog = h.workstation.with(|state| Ok(state.plugins.status())).unwrap()["catalog"]
        .as_array()
        .unwrap()
        .clone();
    let layout =
        plugin_browser::layout(view, &motion.snapshot(), options, &catalog, 0, 0.).unwrap();
    let row = layout
        .rows
        .iter()
        .find(|r| r.text == "U Shaping EQ")
        .unwrap()
        .rect;
    let from = [row[0] + 8., row[1] + row[3] * 0.5];
    let canvas = piano_roll_scene::canvas(view, &motion.snapshot(), options).unwrap();
    let body = arrangement::clip_body(canvas);
    let to = [body.left + 60., body.top + 32.];
    // Previous-frame bottom-panel hover must not steal the browser press.
    let bottom =
        crate::workstation::plugin_inserts::layout(view, &motion.snapshot(), options).unwrap();
    step["dt"] = json!(0.);
    step["editor"] = json!({"pointer":[bottom.viewport[0]+20.,bottom.top+20.],"left":false});
    h.invoke("AudioWorkstationSystem.Step", step.clone())
        .unwrap();
    step["editor"] = json!({"pointer":from,"left":true});
    h.invoke("AudioWorkstationSystem.Step", step.clone())
        .unwrap();
    assert_eq!(h.graphics.cursor_label["text"], "U Shaping EQ");
    assert!(
        h.graphics.plugin_surface["groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["browser_faces"]
                .as_array()
                .is_some_and(|faces| faces.iter().any(|f| f["alpha"] == 0.28)))
    );
    assert!(
        h.workstation.with(|state| Ok(state.plugins.status())).unwrap()["inserts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    step["editor"] = json!({"pointer":to,"left":false});
    h.invoke("AudioWorkstationSystem.Step", step.clone())
        .unwrap();
    let status = h.workstation.with(|state| Ok(state.plugins.status())).unwrap();
    assert_eq!(status["inserts"].as_array().unwrap().len(), 1);
    assert_eq!(status["inserts"][0]["stock"], true);
    assert_eq!(status["inserts"][0]["track"], "a");
    assert!(h.graphics.cursor_label.is_null());
    let surface = h.graphics.plugin_surface.clone();
    assert!(crate::workstation::plugin_surface::valid(&surface));
    let sparkle = surface["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| {
            g["labels"]
                .as_array()
                .is_some_and(|labels| labels.len() == 8 && labels[0]["text"] == "*")
        })
        .unwrap();
    assert_eq!(sparkle["labels"][0]["size"], 22.);
    assert_eq!(sparkle["labels"][1]["text"], "+");
    workstation_open_plugins_page(&mut h,&mut step);
    let surface=h.graphics.plugin_surface.clone();
    let graph = surface["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|g| g["graphs"].as_array().and_then(|a| a.first()))
        .unwrap();
    assert_eq!(graph["gem_surface"],true);
    let optical=h.graphics.workstation_frame.as_ref().unwrap();
    assert_eq!(optical[232][1],1.);
    let pose=optical[232][0]as usize;
    assert!(optical[pose+1][0]>=crate::workstation::stock_eq_gems::BASE as f32);
    assert!(optical[12][2]>0.,"stock geometry must be in the live instance hierarchy");
    assert_eq!(graph["response"].as_array().unwrap().len(), 97);
    assert_eq!(graph["title"], "U SHAPING  +0.0 dB");
    let edges = &graph["edges"];
    let x = (edges[0].as_f64().unwrap() + edges[1].as_f64().unwrap()) * 0.5;
    let y = graph["gain_y"].as_f64().unwrap();
    step["editor"] = json!({"pointer":[x,y],"left":true});
    h.invoke("AudioWorkstationSystem.Step", step.clone())
        .unwrap();
    step["editor"] = json!({"pointer":[x,y-10.],"left":true});
    h.invoke("AudioWorkstationSystem.Step", step.clone())
        .unwrap();
    let updated = h.workstation.with(|state| Ok(state.plugins.status())).unwrap();
    assert!(updated["inserts"][0]["state"]["gain_db"].as_f64().unwrap() > 0.);
    step["editor"] = json!({"pointer":[x,y-10.],"left":false});
    h.invoke("AudioWorkstationSystem.Step", step.clone())
        .unwrap();
    // Stock EQ has Cardinal's distinct selected-lane fallback on release.
    let select = [canvas.left + 40., body.top + 32.];
    for left in [true, false] {
        step["editor"] = json!({"pointer":select,"left":left});
        h.invoke("AudioWorkstationSystem.Step", step.clone())
            .unwrap();
    }
    for left in [true, false] {
        step["editor"] = json!({"pointer":from,"left":left});
        h.invoke("AudioWorkstationSystem.Step", step.clone())
            .unwrap();
    }
    assert_eq!(
        h.workstation.with(|state| Ok(state.plugins.status())).unwrap()["inserts"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    step["dt"] = json!(0.55);
    h.invoke("AudioWorkstationSystem.Step", step.clone())
        .unwrap();
    assert!(
        !h.graphics.plugin_surface["groups"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["labels"]
                .as_array()
                .is_some_and(|labels| labels.len() == 8 && labels[0]["text"] == "*"))
    );
    // Select the stock title, bypass once per right press, then delete.
    step["dt"]=json!(0.);
    let chain=crate::workstation::plugin_inserts::layout(view,&motion.snapshot(),options).unwrap();
    let current=h.workstation.with(|state|Ok(state.plugins.status())).unwrap();
    let r=chain.chain_slot(current["inserts"].as_array().unwrap(),0,0.).rect;
    let point=[r[0]+r[2]*0.5,r[1]+12.];
    for left in [true,false]{
        step["editor"]=json!({"pointer":point,"left":left});
        h.invoke("AudioWorkstationSystem.Step",step.clone()).unwrap();
    }
    step["editor"]=json!({"pointer":point,"right":true});
    for _ in 0..2{h.invoke("AudioWorkstationSystem.Step",step.clone()).unwrap();}
    let bypassed=h.workstation.with(|state|Ok(state.plugins.status())).unwrap();
    assert_eq!(bypassed["inserts"][0]["bypassed"],true,"held right click toggles once");
    step["editor"]=json!({"pointer":point,"delete":true});
    for _ in 0..2{h.invoke("AudioWorkstationSystem.Step",step.clone()).unwrap();}
    let removed=h.workstation.with(|state|Ok(state.plugins.status())).unwrap();
    assert_eq!(removed["inserts"].as_array().unwrap().len(),1,"held Delete removes only selection");
    h.invoke("AudioWorkstationSystem.Close", Value::Null)
        .unwrap();
    assert!(h.graphics.plugin_surface.is_null());
    drop(h.trace_writer.borrow_mut().take());
    fs::remove_file(&h.trace).unwrap();
}

// Integration gestures use the live desktop catalog geometry. The mobile
// component-only view retains its engraved targets.
fn workstation_bubble_component_targets(
    view: crate::workstation::panel_layout::Viewport,
    motion: &crate::workstation::panel_motion::Snapshot,
    options: crate::workstation::panel_layout::Options,
) -> [crate::workstation::panel_layout::Rect; 4] {
    #[cfg(target_os = "macos")]
    {
        let layout =
            crate::workstation::plugin_browser::layout(view, motion, options, &[], 0, 0.).unwrap();
        std::array::from_fn(|i| {
            let r = layout.rows[i].rect;
            crate::workstation::panel_layout::Rect {
                x: r[0],
                y: r[1],
                width: r[2],
                height: r[3],
            }
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        crate::workstation::browser::layout(view, motion, options)
            .unwrap()
            .targets
    }
}

// These regressions deliberately enter through the production Step route:
// unit tests of the browser alone cannot detect a missing ownership predicate.
#[cfg(target_os = "macos")]
fn workstation_plugin_cancellation_fixture() -> (Host, Value, [f64; 2], [f64; 2]) {
    use crate::workstation::{arrangement, panel_layout::{Options, Viewport}, panel_motion,
        piano_roll_scene, plugin_browser};
    let mut h = traced_host();
    export_fixture(&mut h);
    h.invoke("AudioWorkstationSystem.Begin", Value::Null).unwrap();
    let mut step = json!({"dt":1.,"aspect":1280./720.,"viewport":[1280.,720.],
        "framebuffer":[1280.,720.],"keys":[false,false,false,false,false],
        "editor":{"pointer":[640.,360.],"left":false}});
    h.invoke("AudioWorkstationSystem.Step", step.clone()).unwrap();
    // Adopt the settled canvas before starting the gesture, as real frames do.
    step["dt"] = json!(0.);
    h.invoke("AudioWorkstationSystem.Step", step.clone()).unwrap();
    workstation_enable_browser(&mut h);
    let mut motion = panel_motion::State::default();
    motion.set_all_open(true);
    motion.advance(1.);
    let view = Viewport { width:1280., height:720., canvas_scale:1. };
    let options = Options::default();
    let catalog = h.workstation.with(|state| Ok(state.plugins.status())).unwrap()["catalog"].as_array().unwrap().clone();
    let layout = plugin_browser::layout(view, &motion.snapshot(), options, &catalog, 0, 0.).unwrap();
    let row = layout.rows.iter().find(|r| r.text == "U Shaping EQ").unwrap().rect;
    let from = [row[0] + 8., row[1] + row[3] * 0.5];
    let canvas = piano_roll_scene::canvas(view, &motion.snapshot(), options).unwrap();
    let body = arrangement::clip_body(canvas);
    (h, step, from, [body.left + 60., body.top + 32.])
}

#[cfg(target_os = "macos")]
#[test]
fn cardinal_plugin_browser_cancellation_blocks_release_and_preserves_held_history() {
    // 0: unfocused; 1: blur/regain in one callback batch; 2: export ownership.
    for reason in 0..3 {
        let (mut h, mut step, from, to) = workstation_plugin_cancellation_fixture();
        step["editor"] = json!({"pointer":from,"left":true});
        h.invoke("AudioWorkstationSystem.Step", step.clone()).unwrap();
        assert_eq!(h.graphics.cursor_label["text"], "U Shaping EQ");
        let destination = std::env::temp_dir().join(format!("mirror-plugin-cancel-{}", h.run_id));
        if reason == 2 {
            let revision = h.invoke("WorkstationDataSystem.Snapshot", Value::Null).unwrap()["revision"].clone();
            h.invoke("WorkstationExportSystem.Begin", json!({"document_revision":revision,
                "sample_rate":8000,"start_seconds":0.,"end_seconds":0.00025,
                "destination":destination})).unwrap();
        }
        step["editor"] = match reason {
            0 => json!({"pointer":to,"left":true,"focused":false}),
            1 => json!({"pointer":to,"left":true,"focused":true,"transport_pointer_cancelled":true}),
            _ => json!({"pointer":to,"left":true}),
        };
        h.invoke("AudioWorkstationSystem.Step", step.clone()).unwrap();
        assert!(h.graphics.cursor_label.is_null(), "reason {reason}");
        if reason == 2 {
            h.invoke("WorkstationExportSystem.Cancel", Value::Null).unwrap();
        }
        // Regaining focus/ending export while held cannot start another drag.
        step["editor"] = json!({"pointer":from,"left":true,"focused":true});
        h.invoke("AudioWorkstationSystem.Step", step.clone()).unwrap();
        assert!(h.graphics.cursor_label.is_null(), "reason {reason}");
        step["editor"] = json!({"pointer":to,"left":false,"focused":true});
        h.invoke("AudioWorkstationSystem.Step", step.clone()).unwrap();
        assert!(h.workstation.with(|state| Ok(state.plugins.status())).unwrap()["inserts"].as_array().unwrap().is_empty(), "reason {reason}");
        // A genuine new press still works: cancellation is not a permanent lock.
        step["editor"] = json!({"pointer":from,"left":true});
        h.invoke("AudioWorkstationSystem.Step", step.clone()).unwrap();
        assert_eq!(h.graphics.cursor_label["text"], "U Shaping EQ");
        step["editor"] = json!({"pointer":to,"left":false});
        h.invoke("AudioWorkstationSystem.Step", step.clone()).unwrap();
        assert_eq!(h.workstation.with(|state| Ok(state.plugins.status())).unwrap()["inserts"].as_array().unwrap().len(), 1);
        workstation_bubble_close(&mut h);
        assert!(!destination.exists());
        workstation_bubble_remove_trace(h);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn cardinal_plugin_stock_cancellation_stops_drag_without_rearming_held_press() {
    for transient in [false, true] {
        let (mut h, mut step, from, to) = workstation_plugin_cancellation_fixture();
        for (pointer, left) in [(from,true), (to,false)] {
            step["editor"] = json!({"pointer":pointer,"left":left});
            h.invoke("AudioWorkstationSystem.Step", step.clone()).unwrap();
        }
        workstation_open_plugins_page(&mut h,&mut step);
        let surface = h.graphics.plugin_surface.clone();
        let graph = surface["groups"].as_array().unwrap().iter()
            .find_map(|g| g["graphs"].as_array().and_then(|a| a.first())).unwrap();
        let x = (graph["edges"][0].as_f64().unwrap() + graph["edges"][1].as_f64().unwrap()) * 0.5;
        let y = graph["gain_y"].as_f64().unwrap();
        step["editor"] = json!({"pointer":[x,y],"left":true});
        h.invoke("AudioWorkstationSystem.Step", step.clone()).unwrap();
        let gain = h.workstation.with(|state| Ok(state.plugins.status())).unwrap()["inserts"][0]["state"]["gain_db"].clone();
        step["editor"] = json!({"pointer":[x,y-10.],"left":true,"focused":transient,
            "transport_pointer_cancelled":transient});
        h.invoke("AudioWorkstationSystem.Step", step.clone()).unwrap();
        assert_eq!(h.workstation.with(|state| Ok(state.plugins.status())).unwrap()["inserts"][0]["state"]["gain_db"], gain);
        // The old drag must not resume after focus returns while still held.
        for left in [true, false] {
            step["editor"] = json!({"pointer":[x,y-20.],"left":left,"focused":true});
            h.invoke("AudioWorkstationSystem.Step", step.clone()).unwrap();
            assert_eq!(h.workstation.with(|state| Ok(state.plugins.status())).unwrap()["inserts"][0]["state"]["gain_db"], gain);
        }
        for p in [[x,y], [x,y-10.]] {
            step["editor"] = json!({"pointer":p,"left":true});
            h.invoke("AudioWorkstationSystem.Step", step.clone()).unwrap();
        }
        assert!(h.workstation.with(|state| Ok(state.plugins.status())).unwrap()["inserts"][0]["state"]["gain_db"].as_f64().unwrap()
            > gain.as_f64().unwrap());
        workstation_bubble_close(&mut h);
        workstation_bubble_remove_trace(h);
    }
}
