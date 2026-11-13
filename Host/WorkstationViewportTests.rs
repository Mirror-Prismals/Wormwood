// Real registered Step input and snapshot path, with the graphics test driver.
#[cfg(target_os="macos")]
#[test]
fn workstation_browser_starts_paused_and_enables_only_after_a_complete_click() {
    let mut h=traced_host();
    let started=h.invoke("AudioWorkstationSystem.Begin",Value::Null).unwrap();
    assert_eq!(started["browser_enabled"],false);
    let paused=workstation_bubble_step(&mut h,Value::Null);
    assert_eq!(paused["browser_enabled"],false);
    assert_eq!(h.graphics.workstation_frame.as_ref().unwrap()[116][3],1.);
    h.stack.push((5,"EnvironmentAudioSystem.Begin".into()));
    h.invoke("AudioWorkstationSystem.PreparePlugins",Value::Null).unwrap();
    h.stack.pop();
    assert_eq!(h.workstation.with(|d|Ok(d.plugins.status())).unwrap()["scanned"],false);
    let view=crate::workstation::panel_layout::Viewport{width:1280.,height:720.,canvas_scale:1.};
    let mut motion=crate::workstation::panel_motion::State::default();motion.set_all_open(true);motion.advance(1.);
    let layout=crate::workstation::plugin_browser::layout(view,&motion.snapshot(),crate::workstation::panel_layout::Options::default(),&[],0,0.).unwrap();
    let r=layout.enable_button();let p=[r[0]+r[2]*0.5,r[1]+r[3]*0.5];
    let down=workstation_bubble_step(&mut h,json!({"pointer":p,"left":true}));assert_eq!(down["browser_enabled"],false);
    let enabled=workstation_bubble_step(&mut h,json!({"pointer":p,"left":false}));assert_eq!(enabled["browser_enabled"],true);
    assert_eq!(h.graphics.workstation_frame.as_ref().unwrap()[116][3],0.);
    workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}

#[test]
fn workstation_audio_clip_refreshes_after_overview_document_change() {
    let mut h=traced_host();
    workstation_bubble_begin(&mut h);
    let step=|h:&mut Host,scale:f64| h.invoke("AudioWorkstationSystem.Step",json!({
        "dt":1.,"aspect":1280./720.,"viewport":[1280.,720.],"framebuffer":[1280.,720.],
        "canvas_scale":scale,"keys":[false,false,false,false,false],"editor":{}})).unwrap();
    for scale in [0.61,1.] {step(&mut h,scale);}
    // Settle all layout work before an audio document arrives between Steps.
    step(&mut h,1.);
    assert_eq!(h.graphics.workstation_frame.as_ref().unwrap()[248][0],0.);
    for (hook,mut edit) in [
        ("PutSource",json!({"id":"overview-source","sample_rate":8000,"channels":1,"samples":vec![0.25;8000]})),
        ("PutClip",json!({"id":"overview-clip","track":"","kind":"audio","source":"overview-source","lane":0.25,"start_seconds":1.,"duration_seconds":1.,"source_offset_seconds":0.})),
    ] {
        edit["document_revision"]=workstation_bubble_snapshot(&mut h)["revision"].clone();
        h.invoke(&format!("WorkstationDataSystem.{hook}"),edit).unwrap();
    }
    let state=step(&mut h,1.);
    assert!(state["arrangement_error"].is_null());
    let frame=h.graphics.workstation_frame.as_ref().unwrap();
    assert_eq!(frame[248][0],1.,"accepted audio must reach the next visual frame without another gesture");
    for scale in [0.61,1.] {
        step(&mut h,scale);
        let frame=h.graphics.workstation_frame.as_ref().unwrap();
        assert_eq!(frame[248][0],1.,"overview transitions must retain the audio clip");
        let at=frame[248][1] as usize;
        let pose=frame[at];let shape=frame[at+2];let clip=frame[249];
        assert!(pose[3]>0. && shape[0]>0.);
        assert!(pose[0]<clip[2] && pose[0]+shape[0]*pose[3]>clip[0]);
        assert!(pose[1]>clip[1] && pose[1]<clip[3]);
    }
    workstation_bubble_close(&mut h);
    workstation_bubble_remove_trace(h);
}

#[test]
fn workstation_viewports_page_switches_tracks_without_document_edits() {
    use crate::workstation::{
        panel_layout::{Options, Viewport},
        panel_motion, plugin_browser,
    };
    let mut h = traced_host();
    workstation_bubble_begin(&mut h);
    for (id, kind, name) in [
        ("viewport-a", "audio", "Audio 1"),
        ("viewport-b", "midi", "Midi 1"),
    ] {
        let s = workstation_bubble_snapshot(&mut h);
        h.invoke(
            "WorkstationDataSystem.PutTrack",
            json!({"document_revision":s["revision"],"id":id,"kind":kind,"name":name}),
        )
        .unwrap();
    }
    workstation_bubble_step(&mut h, Value::Null);
    let original = workstation_bubble_snapshot(&mut h);
    let mut motion = panel_motion::State::default();
    motion.set_all_open(true);
    motion.advance(1.);
    let view = Viewport {
        width: 1280.,
        height: 720.,
        canvas_scale: 1.,
    };
    let l =
        plugin_browser::layout(view, &motion.snapshot(), Options::default(), &[], 0, 0.).unwrap();
    let c = l.clip;
    let click = |h: &mut Host, p: [f64; 2]| {
        workstation_bubble_step(h, json!({"pointer":p,"left":true}));
        workstation_bubble_step(h, json!({"pointer":p,"left":false}))
    };
    let s = click(&mut h, [c[0] + c[2] * 0.75, c[1] - 16.]);
    assert_eq!(s["viewports"]["page"], "properties");
    let s = click(&mut h, [c[0] + 20., c[1] + 45.]);
    assert_eq!(s["viewports"]["tracks"][0]["visible"], false);
    let s = click(&mut h, [c[0] + c[2] - 15., c[1] + c[3] * 0.62 + 15.]);
    assert_eq!(s["viewports"]["selected"], 1);
    assert!(
        s["viewports"]["tracks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["visible"] == false)
    );
    let s = click(&mut h, [c[0] + 20., c[1] + 75.]);
    assert_eq!(s["viewports"]["tracks"][1]["visible"], true);
    let s = click(&mut h, [c[0] + 20., c[1] + c[3] * 0.62 + 45.]);
    assert_eq!(s["viewports"]["selected"], 0);
    assert_eq!(s["viewports"]["tracks"][0]["visible"], false);
    assert_eq!(s["viewports"]["tracks"][1]["visible"], true);
    assert_eq!(workstation_bubble_snapshot(&mut h), original);
    workstation_bubble_close(&mut h);
    workstation_bubble_remove_trace(h);
}

#[test]
fn workstation_mix_preview_registered_step_preserves_document_and_stays_hidden() {
    let mut h=traced_host();workstation_bubble_begin(&mut h);
    let original=workstation_bubble_snapshot(&mut h);
    let state=workstation_bubble_step(&mut h,Value::Null);
    let rows=state["viewports"]["tracks"].as_array().unwrap();
    assert!(rows.iter().any(|t|t["id"]==crate::workstation::mix_preview::ID&&t["visible"]==false));
    assert_eq!(workstation_bubble_snapshot(&mut h),original);
    let result=h.invoke("WorkstationDataSystem.MixPreview",json!({})).unwrap();
    assert_eq!(result["phase"],"empty");
    h.invoke("AudioWorkstationSystem.Close",Value::Null).unwrap();
}
