fn workstation_bubble_open_fixture(host:&mut Host)->Value {
 let mut s=workstation_bubble_snapshot(host);
 let track=if let Some(t)=s["tracks"].as_array().unwrap().iter().find(|t|t["kind"]=="midi") {t["id"].as_str().unwrap().to_owned()} else {
  host.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":s["revision"],"id":"fixture-midi","name":"Test MIDI","kind":"midi"})).unwrap();
  s=workstation_bubble_snapshot(host);"fixture-midi".into()
 };
 let id=if let Some(c)=s["clips"].as_array().unwrap().iter().find(|c|c["kind"]=="midi"){c["id"].as_str().unwrap().to_owned()} else {
  host.invoke("WorkstationDataSystem.PutClip",json!({"document_revision":s["revision"],"id":"fixture-clip","track":track,"kind":"midi","start_seconds":0.,"bpm":120.,"length_quarters":16.,"meter":{"numerator":4,"denominator":4},"notes":[]})).unwrap();"fixture-clip".into()
 };
 host.invoke("AudioWorkstationSystem.OpenPianoRoll",json!({"id":id})).unwrap()
}
fn workstation_bubble_create_range(host:&mut Host)->Value {
 workstation_bubble_step(host,json!({"pointer":[640.,360.],"key_events":true,"create_range_pressed":true}))
}
fn workstation_bubble_double_click(host:&mut Host,p:[f64;2])->Value {
 use crate::workstation::arrangement;
 struct Reset;impl Drop for Reset{fn drop(&mut self){arrangement::test_click_clock(None);}}
 let _reset=Reset;
 for (t,left) in [(10.,true),(10.05,false),(10.15,true)] {arrangement::test_click_clock(Some(t));workstation_bubble_step(host,json!({"pointer":p,"alt":true,"left":left}));}
 arrangement::test_click_clock(Some(10.2));workstation_bubble_step(host,json!({"pointer":p,"alt":true,"left":false}))
}

// Included inside Host's tests module. These exercise registered control-thread
// routes with the existing graphics test driver; they do not open a native UI.

fn workstation_bubble_step(host: &mut Host, editor: Value) -> Value {
    host.invoke(
        "AudioWorkstationSystem.Step",
        json!({"dt":1.,"aspect":1280./720.,"viewport":[1280.,720.],
            "framebuffer":[1280.,720.],"canvas_scale":1.,
            "keys":[false,false,false,false,false],"editor":editor}),
    )
    .unwrap()
}

fn workstation_bubble_snapshot(host: &mut Host) -> Value {
    host.invoke("WorkstationDataSystem.Snapshot", Value::Null)
        .unwrap()
}

fn workstation_bubble_begin(host: &mut Host) {
    let status = host.invoke("AudioWorkstationSystem.Begin", Value::Null).unwrap();
    assert_eq!(status["open"], true);
    let settled = workstation_bubble_step(host, Value::Null);
    assert_eq!(settled["animating"], false);
    assert!(settled["panels"].as_array().unwrap().iter().all(|p| p["open"] == true));
    workstation_enable_browser(host);
}

fn workstation_enable_browser(host:&mut Host) {
    if cfg!(target_os="macos") {
        let view=crate::workstation::panel_layout::Viewport{width:1280.,height:720.,canvas_scale:1.};
        let mut motion=crate::workstation::panel_motion::State::default();motion.set_all_open(true);motion.advance(1.);
        let layout=crate::workstation::plugin_browser::layout(view,&motion.snapshot(),crate::workstation::panel_layout::Options::default(),&[],0,0.).unwrap();
        let r=layout.enable_button();let p=[r[0]+r[2]*0.5,r[1]+r[3]*0.5];
        workstation_bubble_step(host,json!({"pointer":p,"left":true}));
        assert_eq!(workstation_bubble_step(host,json!({"pointer":p,"left":false}))["browser_enabled"],true);
    }
}

fn workstation_bubble_click(host: &mut Host, x: f64, y: f64) -> Value {
    if host.invoke("AudioWorkstationSystem.Status",Value::Null).unwrap()["piano_roll"]["open"]!=true {
        workstation_bubble_open_fixture(host);
    }
    workstation_bubble_step(host, json!({"pointer":[x,y],"left":true}));
    workstation_bubble_step(host, json!({"pointer":[x,y],"left":false}))
}

fn workstation_bubble_close(host: &mut Host) {
    host.invoke("AudioWorkstationSystem.Close", Value::Null).unwrap();
    assert_eq!(host.entities.len(), 0);
    assert!(host.graphics.prepared_entity().is_none());
    assert!(host.audio.prepared_entity().is_none());
    assert!(host.active_audicle.is_none());
    assert_eq!(host.audicles.count(), 0);
    let status = host.invoke("AudioWorkstationSystem.Status", Value::Null).unwrap();
    assert_eq!(status["open"], false);
    assert_eq!(status["piano_roll"]["open"], false);
}

fn workstation_bubble_remove_trace(host: Host) {
    drop(host.trace_writer.borrow_mut().take());
    fs::remove_file(&host.trace).unwrap();
}

#[test]
fn workstation_arrangement_blank_field_requires_explicit_piano_roll() {
    let mut host = traced_host();
    let empty = workstation_bubble_snapshot(&mut host);
    workstation_bubble_begin(&mut host);
    assert_eq!(workstation_bubble_snapshot(&mut host), empty);

    // Start on the left gem, then cross into open air without releasing.
    workstation_bubble_step(&mut host, json!({"pointer":[120.,360.],"left":true}));
    let dragged = workstation_bubble_step(&mut host, json!({"pointer":[640.,360.],"left":true}));
    assert_eq!(dragged["piano_roll"]["open"], false);
    workstation_bubble_step(&mut host, json!({"pointer":[640.,360.],"left":false}));
    assert_eq!(workstation_bubble_snapshot(&mut host), empty);

    let blank = workstation_bubble_click_without_open(&mut host, 640., 360.);
    assert_eq!(blank["piano_roll"]["open"], false);
    assert_eq!(workstation_bubble_snapshot(&mut host), empty);
    let ignored=workstation_bubble_step(&mut host,json!({"open":true}));
    assert_eq!(ignored["piano_roll"]["open"],false);assert_eq!(workstation_bubble_snapshot(&mut host),empty);
    let opened=workstation_bubble_open_fixture(&mut host);
    assert_eq!(opened["piano_roll"]["open"],true);
    workstation_bubble_step(&mut host,Value::Null);
    let pressed = workstation_bubble_step(&mut host, json!({"pointer":[640.,360.],"left":true}));
    assert_eq!(pressed["piano_roll"]["open"], true);
    assert_eq!(pressed["piano_roll"]["gesture_active"], true);
    let created = workstation_bubble_snapshot(&mut host);
    assert_eq!(created["tracks"].as_array().unwrap().len(), 1);
    assert_eq!(created["clips"].as_array().unwrap().len(), 1);
    assert_eq!(created["clips"][0]["notes"], json!([]));

    // A held preview creates no further document identities or note commits.
    workstation_bubble_step(&mut host, json!({"pointer":[650.,360.],"left":true}));
    assert_eq!(workstation_bubble_snapshot(&mut host), created);
    workstation_bubble_close(&mut host);
    assert_eq!(workstation_bubble_snapshot(&mut host), created);
    workstation_bubble_remove_trace(host);
}

#[test]
fn workstation_bubble_release_commits_to_backend_and_preserves_undo_after_sorting() {
    let mut host = traced_host();
    workstation_bubble_begin(&mut host);
    let first_status = workstation_bubble_click(&mut host, 850., 360.);
    assert_eq!(first_status["piano_roll"]["undo_available"], true);
    assert_eq!(first_status["piano_roll"]["pending_commit"], false);
    let first = workstation_bubble_snapshot(&mut host);
    let first_note = first["clips"][0]["notes"][0].clone();
    assert_eq!(first["clips"][0]["notes"].as_array().unwrap().len(), 1);

    // Draw an earlier note second: backend ordering differs from insertion order.
    let second_status = workstation_bubble_click(&mut host, 640., 360.);
    let second = workstation_bubble_snapshot(&mut host);
    assert_eq!(second["revision"].as_u64(), Some(first["revision"].as_u64().unwrap() + 1));
    let notes = second["clips"][0]["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 2);
    assert!(notes[0]["quarter"].as_f64().unwrap() < notes[1]["quarter"].as_f64().unwrap());
    assert_eq!(notes[1], first_note);
    assert_eq!(second_status["piano_roll"]["undo_available"], true);
    assert_eq!(second_status["piano_roll"]["pending_commit"], false);
    assert!(second_status["piano_roll"]["notice"].is_null());
    assert!(second_status["editor_error"].is_null());

    let undone = workstation_bubble_step(&mut host, json!({"command":true,"undo":true}));
    assert_eq!(undone["piano_roll"]["redo_available"], true);
    assert_eq!(workstation_bubble_snapshot(&mut host)["clips"][0]["notes"], json!([first_note]));
    workstation_bubble_step(&mut host, Value::Null);
    workstation_bubble_step(&mut host, json!({"command":true,"redo":true}));
    let redone = workstation_bubble_snapshot(&mut host);
    assert_eq!(redone["clips"], second["clips"]);
    workstation_bubble_close(&mut host);
    workstation_bubble_remove_trace(host);
}

#[test]
fn workstation_bubble_external_removal_closes_only_editor_and_discards_preview() {
    let mut host = traced_host();
    workstation_bubble_begin(&mut host);
    workstation_bubble_open_fixture(&mut host);
    let pressed = workstation_bubble_step(&mut host, json!({"pointer":[640.,360.],"left":true}));
    assert_eq!(pressed["piano_roll"]["gesture_active"], true);
    let before = workstation_bubble_snapshot(&mut host);
    host.invoke("WorkstationDataSystem.RemoveClip", json!({
        "document_revision":before["revision"],"id":before["clips"][0]["id"]
    })).unwrap();
    let removed = workstation_bubble_snapshot(&mut host);

    let state = workstation_bubble_step(&mut host, json!({"pointer":[640.,360.],"left":true}));
    assert_eq!(state["open"], true);
    assert_eq!(state["piano_roll"]["open"], false);
    assert!(state["editor_error"].as_str().unwrap().contains("removed"));
    assert_eq!(host.entities.len(), 1);
    assert!(host.graphics.prepared_entity().is_some());
    workstation_bubble_step(&mut host, json!({"pointer":[640.,360.],"left":false}));
    assert_eq!(workstation_bubble_snapshot(&mut host), removed);

    let reopened = workstation_bubble_open_fixture(&mut host);
    assert_eq!(reopened["open"], true);
    assert_eq!(reopened["piano_roll"]["open"], true);
    assert!(reopened["editor_error"].is_null());
    let after = workstation_bubble_snapshot(&mut host);
    assert_eq!(after["tracks"], before["tracks"]);
    assert_eq!(after["clips"].as_array().unwrap().len(), 1);
    assert_eq!(after["clips"][0]["notes"], json!([]));
    workstation_bubble_close(&mut host);
    workstation_bubble_remove_trace(host);
}

#[test]
fn workstation_bubble_external_edit_wins_over_an_uncommitted_gesture() {
    let mut host = traced_host();
    workstation_bubble_begin(&mut host);
    workstation_bubble_open_fixture(&mut host);
    workstation_bubble_step(&mut host, json!({"pointer":[640.,360.],"left":true}));
    let before = workstation_bubble_snapshot(&mut host);
    let mut replacement = before["clips"][0].clone();
    replacement["document_revision"] = before["revision"].clone();
    replacement["notes"] = json!([{"id":"external-note","quarter":2.,
        "length_quarters":1.,"pitch":72,"velocity":0.5}]);
    host.invoke("WorkstationDataSystem.PutClip", replacement).unwrap();
    let authoritative = workstation_bubble_snapshot(&mut host);

    let state = workstation_bubble_step(&mut host, json!({"pointer":[640.,360.],"left":false}));
    assert_eq!(state["open"], true);
    assert_eq!(state["piano_roll"]["open"], true);
    assert_eq!(state["piano_roll"]["gesture_active"], false);
    assert_eq!(state["piano_roll"]["pending_commit"], false);
    assert_eq!(state["piano_roll"]["undo_available"], false);
    assert_eq!(state["piano_roll"]["revision"], authoritative["revision"]);
    assert_eq!(workstation_bubble_snapshot(&mut host), authoritative);
    workstation_bubble_close(&mut host);
    workstation_bubble_remove_trace(host);
}

#[test]
fn workstation_bubble_close_releases_active_gesture_and_reopen_keeps_committed_notes() {
    let mut host = traced_host();
    workstation_bubble_begin(&mut host);
    workstation_bubble_click(&mut host, 850., 360.);
    let committed = workstation_bubble_snapshot(&mut host);
    let held = workstation_bubble_step(&mut host, json!({"pointer":[640.,360.],"left":true}));
    assert_eq!(held["piano_roll"]["gesture_active"], true);
    assert!(host.active_audicle.is_some());
    workstation_bubble_close(&mut host);
    assert_eq!(workstation_bubble_snapshot(&mut host), committed);

    workstation_bubble_begin(&mut host);
    assert_eq!(workstation_bubble_snapshot(&mut host), committed);
    let opened = host.invoke("AudioWorkstationSystem.OpenPianoRoll", json!({
        "id":committed["clips"][0]["id"]
    })).unwrap();
    assert_eq!(opened["piano_roll"]["open"], true);
    assert_eq!(opened["piano_roll"]["gesture_active"], false);
    assert_eq!(opened["piano_roll"]["undo_available"], false);
    workstation_bubble_step(&mut host, Value::Null);
    let settled = workstation_bubble_step(&mut host, Value::Null);
    assert_eq!(settled["animating"], false);
    assert_eq!(workstation_bubble_snapshot(&mut host), committed);
    workstation_bubble_close(&mut host);
    workstation_bubble_remove_trace(host);
}

fn workstation_bubble_click_without_open(host:&mut Host,x:f64,y:f64)->Value {
    workstation_bubble_step(host,json!({"pointer":[x,y],"left":true}));
    workstation_bubble_step(host,json!({"pointer":[x,y],"left":false}))
}

#[test]
fn workstation_arrangement_browser_creates_once_and_enters_selected_roll() {
    use crate::workstation::{browser,panel_layout::{Options,Viewport},panel_motion};
    let mut host=traced_host();
    workstation_bubble_begin(&mut host);
    let mut motion=panel_motion::State::default();motion.set_all_open(true);motion.advance(1.);
    let labels=workstation_bubble_component_targets(Viewport{width:1280.,height:720.,canvas_scale:1.},
        &motion.snapshot(),Options::default());
    let midi=labels[2].center();
    workstation_bubble_step(&mut host,json!({"pointer":midi,"left":true}));
    assert_eq!(workstation_bubble_snapshot(&mut host)["tracks"],json!([]));
    // Holding and leaving the browser must neither duplicate tracks nor begin
    // an arrangement/piano-roll gesture before the original press is released.
    let held=workstation_bubble_step(&mut host,json!({"pointer":[640.,360.],"left":true}));
    assert_eq!(held["piano_roll"]["open"],false);
    assert_eq!(held["arrangement"]["gesture_active"],false);
    workstation_bubble_step(&mut host,json!({"pointer":[640.,360.],"left":false}));
    let created=workstation_bubble_snapshot(&mut host);
    assert_eq!(created["tracks"].as_array().unwrap().len(),1);
    assert_eq!(created["tracks"][0]["kind"],"midi");
    assert_eq!(created["clips"],json!([]));
    workstation_bubble_select_range(&mut host);
    workstation_bubble_create_range(&mut host);
    let snapshot=workstation_bubble_snapshot(&mut host);
    assert_eq!(snapshot["clips"].as_array().unwrap().len(),1);
    assert_eq!(snapshot["clips"][0]["track"],created["tracks"][0]["id"]);
    let view=Viewport{width:1280.,height:720.,canvas_scale:1.};
    let c=crate::workstation::drum_rack_ui::Ui::default().timeline_rect(crate::workstation::piano_roll_scene::canvas(view,&motion.snapshot(),Options::default()).unwrap());
    let p=[c.left+crate::workstation::arrangement::BODY_OFFSET+64.,c.top+crate::workstation::arrangement::RULER_HEIGHT+32.];
    let opened=workstation_bubble_double_click(&mut host,p);
    assert_eq!(opened["piano_roll"]["open"],true);
    assert!(opened["arrangement_error"].is_null());
    // Opening a selected clip must not draw through the browser release.
    assert_eq!(workstation_bubble_snapshot(&mut host)["clips"][0]["notes"],json!([]));
    workstation_bubble_close(&mut host);
    workstation_bubble_remove_trace(host);
}

fn workstation_bubble_create_selected_arrangement_clip(host: &mut Host) -> [f64; 2] {
    use crate::workstation::{
        arrangement, browser,
        panel_layout::{Options, Viewport},
        panel_motion, piano_roll_scene,
    };
    let mut motion = panel_motion::State::default();
    motion.set_all_open(true);
    motion.advance(1.);
    let view = Viewport { width: 1280., height: 720., canvas_scale: 1. };
    let labels = workstation_bubble_component_targets(view, &motion.snapshot(), Options::default());
    let p=labels[2].center();
    workstation_bubble_step(host,json!({"pointer":p,"left":true}));
    let dropped=workstation_bubble_step(host,json!({"pointer":[640.,360.],"left":false}));
    assert!(dropped["arrangement_error"].is_null());
    workstation_bubble_select_range(host);
    let status=workstation_bubble_create_range(host);
    assert!(status["arrangement_error"].is_null());
    let snapshot = workstation_bubble_snapshot(host);
    assert_eq!(snapshot["clips"].as_array().unwrap().len(), 1);
    assert_eq!(snapshot["clips"][0]["notes"], json!([]));
    let status = host.invoke("AudioWorkstationSystem.Status", Value::Null).unwrap();
    assert_eq!(status["arrangement"]["selected_clip"], snapshot["clips"][0]["id"]);
    assert_eq!(status["piano_roll"]["open"], false);
    let rect = crate::workstation::drum_rack_ui::Ui::default().timeline_rect(piano_roll_scene::canvas(view, &motion.snapshot(), Options::default()).unwrap());
    [rect.left + arrangement::BODY_OFFSET + 16.,
        rect.top + arrangement::RULER_HEIGHT + arrangement::LANE_HEIGHT * 0.5]
}

#[test]
fn workstation_bubble_enter_does_not_open_but_inserts_inside_double_clicked_roll() {
 let mut host=traced_host();workstation_bubble_begin(&mut host);
 let p=workstation_bubble_create_selected_arrangement_clip(&mut host);let before=workstation_bubble_snapshot(&mut host);
 for e in [json!({"enter":true}),json!({"key_events":true,"enter_pressed":true})] {let s=workstation_bubble_step(&mut host,e);assert_eq!(s["piano_roll"]["open"],false);assert_eq!(workstation_bubble_snapshot(&mut host),before);}
 let opened=workstation_bubble_double_click(&mut host,p);assert_eq!(opened["piano_roll"]["open"],true);assert_eq!(workstation_bubble_snapshot(&mut host),before);
 workstation_bubble_step(&mut host,json!({"enter":true}));let after=workstation_bubble_snapshot(&mut host);assert_eq!(after["clips"][0]["notes"].as_array().unwrap().len(),1);
 workstation_bubble_step(&mut host,json!({"enter":true}));assert_eq!(workstation_bubble_snapshot(&mut host),after);
 workstation_bubble_close(&mut host);workstation_bubble_remove_trace(host);
}

#[test]
fn workstation_bubble_n_cannot_hijack_an_arrangement_drag() {
 let mut host=traced_host();workstation_bubble_begin(&mut host);let p=workstation_bubble_create_selected_arrangement_clip(&mut host);
 let before=workstation_bubble_snapshot(&mut host);workstation_bubble_step(&mut host,json!({"pointer":p,"left":true}));
 let s=workstation_bubble_step(&mut host,json!({"pointer":[p[0]+24.,p[1]],"left":true,"open":true,"open_pressed":true,"key_events":true}));
 assert_eq!(s["piano_roll"]["open"],false);assert_eq!(s["arrangement"]["gesture_active"],true);assert_eq!(workstation_bubble_snapshot(&mut host),before);
 workstation_bubble_step(&mut host,json!({"escape":true}));assert_eq!(workstation_bubble_snapshot(&mut host),before);
 workstation_bubble_close(&mut host);workstation_bubble_remove_trace(host);
}

#[test]
fn workstation_bubble_enter_after_double_click_commits_without_replay() {
 let mut host=traced_host();workstation_bubble_begin(&mut host);let p=workstation_bubble_create_selected_arrangement_clip(&mut host);
 workstation_bubble_double_click(&mut host,p);let before=workstation_bubble_snapshot(&mut host);
 workstation_bubble_step(&mut host,json!({"key_events":true,"enter_pressed":true,"enter_modifiers":0}));let after=workstation_bubble_snapshot(&mut host);
 assert_eq!(after["revision"].as_u64(),Some(before["revision"].as_u64().unwrap()+1));assert_eq!(after["clips"][0]["notes"].as_array().unwrap().len(),1);
 for _ in 0..2 {workstation_bubble_step(&mut host,json!({"key_events":true}));assert_eq!(workstation_bubble_snapshot(&mut host),after);}
 workstation_bubble_close(&mut host);workstation_bubble_remove_trace(host);
}

#[test]
fn workstation_bubble_event_mode_enter_hold_needs_a_new_press_event_to_insert() {
 let mut host=traced_host();workstation_bubble_begin(&mut host);let p=workstation_bubble_create_selected_arrangement_clip(&mut host);workstation_bubble_double_click(&mut host,p);
 let before=workstation_bubble_snapshot(&mut host);
 for _ in 0..2 {workstation_bubble_step(&mut host,json!({"key_events":true,"enter":true}));assert_eq!(workstation_bubble_snapshot(&mut host),before);}
 workstation_bubble_step(&mut host,json!({"key_events":true,"enter_pressed":true}));assert_eq!(workstation_bubble_snapshot(&mut host)["clips"][0]["notes"].as_array().unwrap().len(),1);
 workstation_bubble_close(&mut host);workstation_bubble_remove_trace(host);
}

#[test]
fn workstation_bubble_removed_n_shortcut_and_empty_editor_hook_do_not_create() {
 let mut host=traced_host();workstation_bubble_begin(&mut host);let before=workstation_bubble_snapshot(&mut host);
 assert!(host.invoke("AudioWorkstationSystem.OpenPianoRoll",Value::Null).is_err());
 for e in [json!({"open":true}),json!({"key_events":true,"open_pressed":true}),json!({"enter":true})] {
  let s=workstation_bubble_step(&mut host,e);assert_eq!(s["piano_roll"]["open"],false);assert_eq!(workstation_bubble_snapshot(&mut host),before);
 }
 workstation_bubble_close(&mut host);workstation_bubble_remove_trace(host);
}

#[test]
fn workstation_arrangement_browser_selection_cancel_and_drop_order(){
 use crate::workstation::{browser,panel_layout::{Options,Viewport},panel_motion,piano_roll_scene,arrangement};
 let mut host=traced_host();workstation_bubble_begin(&mut host);
 let mut motion=panel_motion::State::default();motion.set_all_open(true);motion.advance(1.);
 let view=Viewport{width:1280.,height:720.,canvas_scale:1.};let options=Options::default();
 let labels=workstation_bubble_component_targets(view,&motion.snapshot(),options);
 let rect=crate::workstation::drum_rack_ui::Ui::default().timeline_rect(piano_roll_scene::canvas(view,&motion.snapshot(),options).unwrap());let body=arrangement::clip_body(rect);let drop=[body.left+60.,body.top+10.];
 let a=labels[1].center();let m=labels[2].center();let c=labels[3].center();
 workstation_bubble_click_without_open(&mut host,a[0],a[1]);assert_eq!(workstation_bubble_snapshot(&mut host)["tracks"],json!([]));
 for start in [a,c]{workstation_bubble_step(&mut host,json!({"pointer":start,"left":true}));let done=workstation_bubble_step(&mut host,json!({"pointer":drop,"left":false}));assert!(done["arrangement_error"].is_null());}
 workstation_bubble_step(&mut host,json!({"pointer":m,"left":true}));let before=workstation_bubble_snapshot(&mut host);
 let selected=host.invoke("AudioWorkstationSystem.Status",Value::Null).unwrap()["arrangement"]["selected_track"].clone();
 workstation_bubble_step(&mut host,json!({"pointer":drop,"left":true,"focused":false}));
 let held=workstation_bubble_step(&mut host,json!({"pointer":drop,"left":true}));
 assert_eq!(held["arrangement"]["gesture_active"],false);assert_eq!(held["arrangement"]["selected_track"],selected);
 assert_eq!(held["browser"]["captured"],true);assert_eq!(held["browser"]["dragging"],false);
 workstation_bubble_step(&mut host,json!({"pointer":drop,"left":false}));assert_eq!(workstation_bubble_snapshot(&mut host),before);
 workstation_bubble_step(&mut host,json!({"pointer":m,"left":true}));workstation_bubble_step(&mut host,json!({"pointer":drop,"left":false}));
 let now=workstation_bubble_snapshot(&mut host);assert_eq!(now["tracks"].as_array().unwrap().iter().map(|t|t["kind"].as_str().unwrap()).collect::<Vec<_>>(),vec!["audio","midi","cc"]);
 assert_eq!(now["clips"],json!([]));assert_eq!(now["revision"].as_u64().unwrap(),before["revision"].as_u64().unwrap()+1);
 workstation_bubble_close(&mut host);workstation_bubble_remove_trace(host);
}

#[test]
#[cfg(target_os="macos")]
fn workstation_browser_ghost_is_live_without_changing_optical_frame(){
 use crate::workstation::{browser,panel_layout::{Options,Viewport},panel_motion};
 let mut host=traced_host();workstation_bubble_begin(&mut host);
 let mut motion=panel_motion::State::default();motion.set_all_open(true);motion.advance(1.);
 let labels=workstation_bubble_component_targets(Viewport{width:1280.,height:720.,canvas_scale:1.},&motion.snapshot(),Options::default());
 let p=labels[2].center();let frame=host.graphics.workstation_frame.clone();
 workstation_bubble_step(&mut host,json!({"pointer":p,"left":true}));
 assert_eq!(host.graphics.cursor_label,json!({"text":"Midi Track","position":[p[0]+12.,p[1]+8.],"preview":"midi"}));
 workstation_bubble_step(&mut host,json!({"pointer":[620.,350.],"left":true}));
 assert_eq!(host.graphics.cursor_label,json!({"text":"Midi Track","position":[632.,358.],"preview":"midi"}));
 assert!(host.graphics.workstation_frame==frame,"changed rows: {:?}",host.graphics.workstation_frame.as_ref().unwrap().iter().zip(frame.as_ref().unwrap()).enumerate().filter(|(_, (a,b))|a!=b).map(|(i,(a,b))|(i,*a,*b)).collect::<Vec<_>>());
 workstation_bubble_step(&mut host,json!({"pointer":[620.,350.],"left":true,"escape":true}));
 assert!(host.graphics.cursor_label.is_null());assert!(host.graphics.workstation_frame==frame,"changed rows: {:?}",host.graphics.workstation_frame.as_ref().unwrap().iter().zip(frame.as_ref().unwrap()).enumerate().filter(|(_, (a,b))|a!=b).map(|(i,(a,b))|(i,*a,*b)).collect::<Vec<_>>());
 workstation_bubble_step(&mut host,json!({"pointer":[620.,350.],"left":false}));
 workstation_bubble_step(&mut host,json!({"pointer":p,"left":true}));assert!(!host.graphics.cursor_label.is_null());
 workstation_bubble_close(&mut host);assert!(host.graphics.cursor_label.is_null());workstation_bubble_remove_trace(host);
}
#[test]
fn cursor_label_request_rejects_invalid_text_and_coordinates(){
 assert!(super::check_payload("CursorLabelRequest",&json!({"entity":1,"label":null})));
 assert!(super::check_payload("CursorLabelRequest",&json!({"entity":1,"label":{"text":"Midi Track","position":[12.,8.]}})));
 for label in [json!({"text":"Bad\nlabel","position":[0.,0.]}),json!({"text":"Midi","position":[0.]}),json!({"text":"Midi","position":[0.,50000.]}),json!({"text":"","position":[0.,0.]})]{assert!(!super::check_payload("CursorLabelRequest",&json!({"entity":1,"label":label})));}
}

#[test]
fn workstation_arrangement_double_click_opens_selected_midi_without_note_creation(){
 use crate::workstation::arrangement;
 struct ClockReset;impl Drop for ClockReset{fn drop(&mut self){arrangement::test_click_clock(None);}}
 let _reset=ClockReset;let mut host=traced_host();workstation_bubble_begin(&mut host);
 let p=workstation_bubble_create_selected_arrangement_clip(&mut host);let before=workstation_bubble_snapshot(&mut host);
 for (time,left) in [(1.,true),(1.05,false),(1.15,true)]{arrangement::test_click_clock(Some(time));let s=workstation_bubble_step(&mut host,json!({"pointer":p,"alt":true,"left":left}));assert_eq!(s["piano_roll"]["open"],false);}
 arrangement::test_click_clock(Some(1.25));let opened=workstation_bubble_step(&mut host,json!({"pointer":p,"alt":true,"left":false}));
 assert_eq!(opened["piano_roll"]["open"],true);assert_eq!(opened["piano_roll"]["clip_id"],before["clips"][0]["id"]);
 assert_eq!(opened["piano_roll"]["gesture_active"],false);assert_eq!(workstation_bubble_snapshot(&mut host),before);
 workstation_bubble_step(&mut host,json!({"pointer":p,"alt":true,"left":false}));assert_eq!(workstation_bubble_snapshot(&mut host),before);
 workstation_bubble_close(&mut host);workstation_bubble_remove_trace(host);
}
#[test]
fn workstation_arrangement_focus_loss_cannot_complete_a_double_click(){
 use crate::workstation::arrangement;
 struct ClockReset;impl Drop for ClockReset{fn drop(&mut self){arrangement::test_click_clock(None);}}
 let _reset=ClockReset;let mut host=traced_host();workstation_bubble_begin(&mut host);let p=workstation_bubble_create_selected_arrangement_clip(&mut host);
 for (time,left,focused)in[(1.,true,true),(1.05,false,true),(1.1,true,true),(1.15,false,false)]{
   arrangement::test_click_clock(Some(time));let s=workstation_bubble_step(&mut host,json!({"pointer":p,"left":left,"focused":focused}));assert_eq!(s["piano_roll"]["open"],false);
 }
 workstation_bubble_close(&mut host);workstation_bubble_remove_trace(host);
}

fn workstation_bubble_select_range(host:&mut Host){
 use crate::workstation::{arrangement,panel_layout::{Options,Viewport},panel_motion,piano_roll_scene};
 let mut motion=panel_motion::State::default();motion.set_all_open(true);motion.advance(1.);
 let c=crate::workstation::drum_rack_ui::Ui::default().timeline_rect(piano_roll_scene::canvas(Viewport{width:1280.,height:720.,canvas_scale:1.},&motion.snapshot(),Options::default()).unwrap());
 let body=arrangement::clip_body(c);let y=body.top+32.;
 // Start beyond the vertical navigation rail, which is present after the
 // explicit browser-enable frames have settled the workspace.
 for (x,left) in [(body.left+48.,true),(body.left+body.width*0.5,true),(body.left+body.width*0.5,false)]{workstation_bubble_step(host,json!({"pointer":[x,y],"left":left}));}
}
#[test]
#[cfg(target_os="macos")]fn workstation_arrangement_cardinal_shortcut_requires_range_and_escape_cancels(){
 use crate::workstation::{browser,panel_layout::{Options,Viewport},panel_motion};
 let mut host=traced_host();workstation_bubble_begin(&mut host);let mut motion=panel_motion::State::default();motion.set_all_open(true);motion.advance(1.);
 let targets=workstation_bubble_component_targets(Viewport{width:1280.,height:720.,canvas_scale:1.},&motion.snapshot(),Options::default());
 assert_eq!(targets.len(),4);let m=targets[2].center();workstation_bubble_step(&mut host,json!({"pointer":m,"left":true}));workstation_bubble_step(&mut host,json!({"pointer":[640.,360.],"left":false}));
 let before=workstation_bubble_snapshot(&mut host);workstation_bubble_create_range(&mut host);assert_eq!(workstation_bubble_snapshot(&mut host),before);
 workstation_bubble_select_range(&mut host);assert!(!host.graphics.selection_range.is_null());
 workstation_bubble_step(&mut host,json!({"escape":true}));assert!(host.graphics.selection_range.is_null());
 workstation_bubble_create_range(&mut host);assert_eq!(workstation_bubble_snapshot(&mut host),before);
 workstation_bubble_select_range(&mut host);
 for e in [json!({"focused":false,"pointer":[640.,360.],"create_range_pressed":true}),json!({"pointer":[640.,360.],"tab":true,"create_range_pressed":true})] {workstation_bubble_step(&mut host,e);assert_eq!(workstation_bubble_snapshot(&mut host),before);}
 workstation_bubble_close(&mut host);workstation_bubble_remove_trace(host);
}

#[test]fn selection_range_request_rejects_unbounded_or_malformed_geometry(){
 assert!(super::check_payload("SelectionRangeRequest",&json!({"entity":1,"range":null})));
 assert!(super::check_payload("SelectionRangeRequest",&json!({"entity":1,"range":{"rect":[1.,2.,3.,4.],"holes":[[0.,0.,1.,1.]]}})));
 for r in [json!({"rect":[1.,2.,-1.,4.],"holes":[]}),json!({"rect":[1.,2.,3.],"holes":[]}),json!({"rect":[0.,0.,50000.,1.],"holes":[]}),json!({"rect":[0.,0.,1.,1.],"holes":vec![json!([0.,0.,1.,1.]);130]})]{assert!(!super::check_payload("SelectionRangeRequest",&json!({"entity":1,"range":r})));}
}

// Uses the real producer/consumer and registered transport dispatcher; no JACK,
// CoreAudio device, native window or rendering device is opened by these tests.
fn workstation_transport_audio(host:&mut Host)->crate::audio::transport::Output {
 // Establish the same declared owner as EnvironmentAudioSystem.Begin,
 // while replacing only its native device connection with an in-memory queue.
 let parent=host.active_audicle;host.stack.push((9001,"EnvironmentAudioSystem.Begin".into()));
 let event=host.invoke("AudicleSystem.Create",Value::Null).unwrap();
 host.invoke("AudicleSystem.Ready",event.clone()).unwrap();host.invoke("AudicleSystem.Go",event.clone()).unwrap();
 host.stack.pop();host.active_audicle=parent;
 host.environment_audio.test_prepare_transport(48000,event)
}
fn workstation_transport_step(host:&mut Host,editor:Value)->Value {
 host.invoke("AudioWorkstationSystem.Step",json!({"dt":0.075,"aspect":1280./720.,"viewport":[1280.,720.],"framebuffer":[1280.,720.],"canvas_scale":1.,"keys":[false,false,false,false,false],"transport_available":true,"editor":editor})).unwrap()
}
fn workstation_transport_point(host:&Host,i:usize)->[f64;2]{
 let f=host.graphics.workstation_frame.as_ref().unwrap();let p=f[192+2*i];
 [f64::from((p[0]/f[2][3]+1.)*f[8][0]*0.5),f64::from((1.-p[1]/f[3][3])*f[8][1]*0.5)]
}
fn workstation_transport_short_click(host:&mut Host,i:usize)->Value{
 let p=workstation_transport_point(host,i);
 workstation_transport_step(host,json!({"pointer":p,"left":false,"transport_pointer_events":[{"pointer":p,"left":true},{"pointer":p,"left":false}]}))
}
#[test]fn workstation_transport_real_packet_hits_drive_play_pause_rewind_once(){
 let mut h=traced_host();let mut output=workstation_transport_audio(&mut h);workstation_bubble_begin(&mut h);
 let neutral=h.graphics.workstation_frame.as_ref().unwrap().clone();
 let pressed=workstation_transport_short_click(&mut h,3);assert!(pressed["transport_pressure"][3].as_f64().unwrap()>0.);
 let down=h.graphics.workstation_frame.as_ref().unwrap();
 assert!(down[198][2]<neutral[198][2],"actual uploaded Play geometry must move inward");
 assert!(down[198][0]<neutral[198][0],"Cardinal press must move the actual projected button left");
 let projected_shift=f64::from(neutral[198][0]-down[198][0])*720./1.3;
 assert!((projected_shift-10.*pressed["transport_pressure"][3].as_f64().unwrap()).abs()<0.0001);
 assert_eq!(down[198][1],neutral[198][1]);assert_eq!(down[198][3],neutral[198][3]);
 assert_eq!(down[199][3],neutral[198][2]);assert_eq!(&down[199][..3],&neutral[199][..3]);
 assert_eq!(down[13],neutral[13]);assert_eq!(down[14],neutral[14]);
 assert!(crate::workstation::decorative_controls::derived_instance_range(down).is_some());
 assert!(!crate::workstation::transport_pose::optics_changed(&neutral,down,true),"changed rows: {:?}",neutral.iter().zip(down).enumerate().filter(|(_, (a,b))|a!=b).map(|(i,(a,b))|(i,*a,*b)).collect::<Vec<_>>());
 assert_eq!(h.environment_audio.playback_status()["playing"],false);
 let activated=workstation_transport_step(&mut h,Value::Null);assert!(activated["transport_notice"].is_null(),"{activated}");assert_eq!(h.environment_audio.playback_status()["playing"],true);
 output.begin_block();for _ in 0..64{output.frame();}output.end_block(64);let position=h.environment_audio.playback_status()["position_frame"].clone();assert_eq!(position,64);
 workstation_transport_short_click(&mut h,1);workstation_transport_step(&mut h,Value::Null);assert_eq!(h.environment_audio.playback_status()["playing"],false);assert_eq!(h.environment_audio.playback_status()["position_frame"],position);
 workstation_transport_short_click(&mut h,0);workstation_transport_step(&mut h,Value::Null);assert_eq!(h.environment_audio.playback_status()["position_frame"],0);
 host_close_test_transport(&mut h);workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}
#[test]fn workstation_transport_outside_cancel_and_space_final_decision(){
 let mut h=traced_host();let _output=workstation_transport_audio(&mut h);workstation_bubble_begin(&mut h);let p=workstation_transport_point(&h,3);
 workstation_transport_step(&mut h,json!({"pointer":[640.,360.],"transport_pointer_events":[{"pointer":p,"left":true},{"pointer":[640.,360.],"left":false}]}));
 workstation_transport_step(&mut h,Value::Null);assert_eq!(h.environment_audio.playback_status()["playing"],false);
 workstation_transport_short_click(&mut h,3);let toggled=workstation_transport_step(&mut h,json!({"transport_toggle_pressed":true}));assert!(toggled["transport_notice"].is_null(),"{toggled}");assert_eq!(h.environment_audio.playback_status()["playing"],false);
 workstation_transport_step(&mut h,Value::Null);assert_eq!(h.environment_audio.playback_status()["playing"],false);
 host_close_test_transport(&mut h);workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}

// Append inside the existing Host tests include; reuses transport fixtures.
#[test]fn workstation_transport_same_step_short_click_and_space_wait_then_stop(){
 let mut h=traced_host();let _output=workstation_transport_audio(&mut h);workstation_bubble_begin(&mut h);let p=workstation_transport_point(&h,3);
 let queued=workstation_transport_step(&mut h,json!({"pointer":p,"left":false,"transport_toggle_pressed":true,
  "transport_pointer_events":[{"pointer":p,"left":true},{"pointer":p,"left":false}]}));
 assert!(queued["transport_notice"].is_null(),"{queued}");
 assert_eq!(h.environment_audio.playback_status()["playing"],false,"new button and its Space both wait one update");
 let applied=workstation_transport_step(&mut h,Value::Null);assert!(applied["transport_notice"].is_null(),"{applied}");
 assert_eq!(h.environment_audio.playback_status()["playing"],false,"Play runs before Space");
 workstation_transport_step(&mut h,Value::Null);assert_eq!(h.environment_audio.playback_status()["playing"],false);
 workstation_transport_step(&mut h,json!({"transport_toggle_pressed":true}));assert_eq!(h.environment_audio.playback_status()["playing"],true,"keyboard-only Space is immediate");
 host_close_test_transport(&mut h);workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}
#[test]fn workstation_transport_focus_cancels_deferred_space_and_timeline_filters_modifiers(){
 let mut h=traced_host();let _output=workstation_transport_audio(&mut h);workstation_bubble_begin(&mut h);let p=workstation_transport_point(&h,3);
 workstation_transport_step(&mut h,json!({"pointer":p,"left":false,"transport_toggle_pressed":true,
  "transport_pointer_events":[{"pointer":p,"left":true},{"pointer":p,"left":false}]}));
 workstation_transport_step(&mut h,json!({"focused":false,"transport_pointer_cancelled":true}));
 workstation_transport_step(&mut h,Value::Null);assert_eq!(h.environment_audio.playback_status()["playing"],false);
 workstation_transport_step(&mut h,json!({"transport_toggle_pressed":true,"transport_toggle_timeline_pressed":false}));
 assert_eq!(h.environment_audio.playback_status()["playing"],false,"modified timeline Space is excluded even after modifiers are released");
 workstation_bubble_open_fixture(&mut h);
 workstation_transport_step(&mut h,json!({"transport_toggle_pressed":true,"transport_toggle_timeline_pressed":false,"command":true,"alt":true}));
 assert_eq!(h.environment_audio.playback_status()["playing"],true,"ordinary piano-roll Space remains available");
 workstation_transport_short_click(&mut h,1);workstation_transport_step(&mut h,Value::Null);
 assert_eq!(h.environment_audio.playback_status()["playing"],true,"piano roll still blocks top pointer buttons");
 host_close_test_transport(&mut h);workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}

fn host_close_test_transport(host:&mut Host){
 // This fixture supplies an external EnvironmentAudioSystem owner; the DAW
 // must not destroy it. Close that owner explicitly before asserting no leaks.
 let parent=host.active_audicle;host.stack.push((9002,"AudioWorkstationSystem.Run".into()));
 let closed=host.invoke("EnvironmentAudioSystem.Close",Value::Null);
 host.stack.pop();host.active_audicle=parent;closed.unwrap();
}

fn workstation_loop_command(host:&mut Host,request:Value)->Result<Value,String>{
 let parent=host.active_audicle;host.stack.push((9003,"AudioWorkstationSystem.Step".into()));
 let value=host.invoke("EnvironmentAudioSystem.Transport",request);
 host.stack.pop();host.active_audicle=parent;value
}
#[test] fn workstation_loop_registered_route_wraps_clock_and_preserves_disabled_edits(){
 let mut h=traced_host();let mut out=workstation_transport_audio(&mut h);workstation_bubble_begin(&mut h);
 let initial=h.environment_audio.playback_status();
 workstation_loop_command(&mut h,json!({"action":"loop","enabled":false,"loop_start_frame":2048,"loop_end_frame":4096})).unwrap();
 assert_eq!(h.environment_audio.playback_status()["generation"],initial["generation"]);
 workstation_loop_command(&mut h,json!({"action":"loop","enabled":true,"loop_start_frame":2048,"loop_end_frame":4096})).unwrap();
 workstation_loop_command(&mut h,json!({"action":"seek","start_frame":4094})).unwrap();
 workstation_loop_command(&mut h,json!({"action":"play"})).unwrap();out.begin_block();
 let generation=h.environment_audio.playback_status()["generation"].clone();
 for expected in [4095,2048,2049,2050]{out.frame();assert_eq!(h.environment_audio.playback_status()["position_frame"],expected);}
 assert_eq!(h.environment_audio.playback_status()["generation"],generation);
 workstation_loop_command(&mut h,json!({"action":"seek","start_frame":5000})).unwrap();
 assert_eq!(h.environment_audio.playback_status()["position_frame"],2048);
 workstation_loop_command(&mut h,json!({"action":"loop","enabled":false,"loop_start_frame":2048,"loop_end_frame":4096})).unwrap();
 let before=h.environment_audio.playback_status();
 workstation_loop_command(&mut h,json!({"action":"loop","enabled":false,"loop_start_frame":1000,"loop_end_frame":2000})).unwrap();
 let after=h.environment_audio.playback_status();
 for key in ["generation","position_frame","queued_frames","queued_sequence","consumed_sequence"]{assert_eq!(after[key],before[key],"{key}");}
 assert!(workstation_loop_command(&mut h,json!({"action":"loop","enabled":true,"loop_start_frame":3,"loop_end_frame":3})).is_err());
 assert_eq!(h.environment_audio.playback_status(),after);
 host_close_test_transport(&mut h);workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}

// Append inside Host's existing tests module, e.g. PianoRollIntegrationTests.rs.
// Kept separate to avoid overwriting the parent's transport/final-pose tests.
// Uses existing graphics fixture and in-memory transport; no native UI/device.
fn workstation_timeline_zoom_point(host:&mut Host)->[f64;2] {
 // Begin's first frame completes the opening animation after its input phase.
 // Let the next input frame adopt that final canvas before choosing a target.
 workstation_bubble_step(host,Value::Null);
 let s=host.invoke("AudioWorkstationSystem.Status",Value::Null).unwrap();
 let r=&s["arrangement"]["timeline_bars"]["zoom"];
 [r[0].as_f64().unwrap()+r[2].as_f64().unwrap()*0.5,r[1].as_f64().unwrap()+r[3].as_f64().unwrap()*0.5]
}
#[test]fn workstation_timeline_focus_cancellation_preserves_held_history(){
 for transient in [false,true] {
  let mut h=traced_host();workstation_bubble_begin(&mut h);let p=workstation_timeline_zoom_point(&mut h);
  let down=workstation_bubble_step(&mut h,json!({"pointer":p,"left":true}));assert_eq!(down["arrangement"]["gesture_active"],true);
  let q=[p[0]+60.,p[1]+5.];
  let cancelled=workstation_bubble_step(&mut h,json!({"pointer":q,"left":true,"focused":transient,"transport_pointer_cancelled":true}));
  assert_eq!(cancelled["arrangement"]["gesture_active"],false);
  for key in ["start_seconds","seconds_per_width"] {assert_eq!(cancelled["arrangement"][key],down["arrangement"][key]);}
  let still_held=workstation_bubble_step(&mut h,json!({"pointer":p,"left":true,"focused":true}));
  assert_eq!(still_held["arrangement"]["gesture_active"],false);
  workstation_bubble_step(&mut h,json!({"pointer":p,"left":false}));
  assert_eq!(workstation_bubble_step(&mut h,json!({"pointer":p,"left":true}))["arrangement"]["gesture_active"],true);
  workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
 }
}
#[test]fn workstation_timeline_export_cancels_capture_without_rearming_held_press(){
 let mut h=traced_host();export_fixture(&mut h);workstation_bubble_begin(&mut h);
 let p=workstation_timeline_zoom_point(&mut h);
 let down=workstation_bubble_step(&mut h,json!({"pointer":p,"left":true}));assert_eq!(down["arrangement"]["gesture_active"],true);
 let destination=std::env::temp_dir().join(format!("mirror-timeline-export-{}",h.run_id));
 let revision=workstation_bubble_snapshot(&mut h)["revision"].clone();
 h.invoke("WorkstationExportSystem.Begin",json!({"document_revision":revision,"sample_rate":8000,
  "start_seconds":0.,"end_seconds":0.00025,"destination":destination})).unwrap();
 let cancelled=workstation_bubble_step(&mut h,json!({"pointer":[p[0]+60.,p[1]+5.],"left":true,"wheel":[0.,1.]}));
 assert_eq!(cancelled["arrangement"]["gesture_active"],false);
 for key in ["start_seconds","seconds_per_width"] {assert_eq!(cancelled["arrangement"][key],down["arrangement"][key]);}
 h.invoke("WorkstationExportSystem.Cancel",Value::Null).unwrap();
 assert_eq!(workstation_bubble_step(&mut h,json!({"pointer":p,"left":true}))["arrangement"]["gesture_active"],false);
 workstation_bubble_step(&mut h,json!({"pointer":p,"left":false}));
 assert_eq!(workstation_bubble_step(&mut h,json!({"pointer":p,"left":true}))["arrangement"]["gesture_active"],true);
 workstation_bubble_close(&mut h);assert!(!destination.exists());workstation_bubble_remove_trace(h);
}
#[test]fn workstation_timeline_signed_pan_accepts_render_packet_and_keeps_transport_time(){
 let mut h=traced_host();let _output=workstation_transport_audio(&mut h);workstation_bubble_begin(&mut h);
 let p=workstation_timeline_zoom_point(&mut h);let before=h.environment_audio.playback_status();
 workstation_transport_step(&mut h,json!({"pointer":p,"left":true}));
 let moved=workstation_transport_step(&mut h,json!({"pointer":[p[0]+60.,p[1]],"left":true}));
 assert!(moved["arrangement"]["start_seconds"].as_f64().unwrap()<0.);assert!(moved["arrangement_error"].is_null());
 assert_eq!(h.environment_audio.playback_status()["position_frame"],before["position_frame"]);
 assert!(h.graphics.workstation_frame.as_ref().unwrap().iter().flatten().all(|v|v.is_finite()));
 workstation_transport_step(&mut h,json!({"pointer":p,"left":false}));
 host_close_test_transport(&mut h);workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}

// Append inside Host's tests module. Uses existing fixture helpers; unrun.
fn workstation_io_step(host:&mut Host,editor:Value,page:usize)->Value {
 host.invoke("AudioWorkstationSystem.Step",json!({"dt":1.,"aspect":1.6,"viewport":[1600.,1000.],
  "framebuffer":[1600.,1000.],"canvas_scale":1.,"keys":[false,false,false,false,false],
  "right_panel_page":page,"editor":editor})).unwrap()
}
fn workstation_io_fixture(host:&mut Host)->[f64;2] {
 let revision=workstation_bubble_snapshot(host)["revision"].clone();
 host.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":revision,"id":"io-track","kind":"audio","name":"I/O"})).unwrap();
 workstation_bubble_begin(host);
 let status=workstation_io_step(host,Value::Null,0);let r=&status["track_io"]["controls"][1]["rect"];
 [r[0].as_f64().unwrap()+18.,r[1].as_f64().unwrap()+18.]
}
#[test]fn workstation_io_registered_action_refreshes_authoritative_state_in_same_step(){
 let mut h=traced_host();let p=workstation_io_fixture(&mut h);
 let before=workstation_bubble_snapshot(&mut h);let revision=before["revision"].as_u64().unwrap();
 workstation_io_step(&mut h,json!({"pointer":p,"left":true}),0);
 workstation_io_step(&mut h,json!({"pointer":p,"left":false}),0);
 assert_eq!(workstation_bubble_snapshot(&mut h)["revision"],revision);
 let applied=workstation_io_step(&mut h,json!({"pointer":p,"left":false}),0);
 assert!(applied["track_io_error"].is_null(),"{applied}");
 assert_eq!(workstation_bubble_snapshot(&mut h)["tracks"][0]["arm_mode"],3);
 assert_eq!(applied["track_io"]["controls"][1]["crosswalk_palette"],1);
 assert_eq!(applied["arrangement"]["revision"],revision+1);
 workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}
#[test]fn workstation_io_transient_focus_cancels_pending_and_holds_without_new_press(){
 for pending in [false,true] {
  let mut h=traced_host();let p=workstation_io_fixture(&mut h);let before=workstation_bubble_snapshot(&mut h);
  workstation_io_step(&mut h,json!({"pointer":p,"left":true}),0);
  if pending{workstation_io_step(&mut h,json!({"pointer":p,"left":false}),0);}
  workstation_io_step(&mut h,json!({"pointer":p,"left":!pending,"focused":true,"transport_pointer_cancelled":true}),0);
  workstation_io_step(&mut h,json!({"pointer":p,"left":!pending}),0);
  workstation_io_step(&mut h,json!({"pointer":p,"left":false}),0);
  workstation_io_step(&mut h,json!({"pointer":p,"left":false}),0);
  assert_eq!(workstation_bubble_snapshot(&mut h),before);
  workstation_io_step(&mut h,json!({"pointer":p,"left":true}),0);
  workstation_io_step(&mut h,json!({"pointer":p,"left":false}),0);
  let applied=workstation_io_step(&mut h,json!({"pointer":p,"left":false}),0);
  assert_eq!(applied["track_io"]["controls"][1]["crosswalk_palette"],1);
  workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
 }
}

#[test]fn workstation_io_cycles_crosswalk_during_playback_with_consumed_visualization(){
 let mut h=traced_host();let mut output=workstation_transport_audio(&mut h);
 let p=workstation_io_fixture(&mut h);
 for (mode,palette) in [(3,1),(1,2),(0,0)] {
  // Play prepares real document audio through the registered dispatcher,
  // without requiring the native device/route check used by Pump.
  workstation_loop_command(&mut h,json!({"action":"play"})).unwrap();
  output.begin_block();for _ in 0..4096{output.frame();}output.end_block(4096);
  assert!(h.environment_audio.playback_status()["visualization"].is_object());
  workstation_io_step(&mut h,json!({"pointer":p,"left":true}),0);
  workstation_io_step(&mut h,json!({"pointer":p,"left":false}),0);
  let applied=workstation_io_step(&mut h,json!({"pointer":p,"left":false}),0);
  assert!(applied["track_io_error"].is_null(),"{applied}");
  assert_eq!(workstation_bubble_snapshot(&mut h)["tracks"][0]["arm_mode"],mode);
  assert_eq!(applied["track_io"]["controls"][1]["crosswalk_palette"],palette);
  let packet=h.graphics.workstation_frame.as_ref().unwrap();
  let descriptor=crate::workstation::track_io_scene::POSE_BASE+crate::workstation::track_io_scene::STRIDE+1;
  assert_eq!(packet[descriptor][3],(crate::workstation::track_io_scene::BASE+palette*39*40) as f32);
  assert_eq!(h.environment_audio.playback_status()["playing"],true);
 }
 host_close_test_transport(&mut h);workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}

#[test]fn workstation_io_short_callback_click_changes_crosswalk_once(){
 let mut h=traced_host();let p=workstation_io_fixture(&mut h);
 for (mode,palette) in [(3,1),(1,2),(0,0)] {
  let revision=workstation_bubble_snapshot(&mut h)["revision"].as_u64().unwrap();
  workstation_io_step(&mut h,json!({"pointer":p,"left":false,
   "transport_pointer_events":[{"pointer":p,"left":true},{"pointer":p,"left":false}]}),0);
  assert_eq!(workstation_bubble_snapshot(&mut h)["revision"],revision);
  let applied=workstation_io_step(&mut h,json!({"pointer":p,"left":false}),0);
  assert_eq!(workstation_bubble_snapshot(&mut h)["tracks"][0]["arm_mode"],mode);
  assert_eq!(applied["track_io"]["controls"][1]["crosswalk_palette"],palette);
  workstation_io_step(&mut h,json!({"pointer":p,"left":false}),0);
  assert_eq!(workstation_bubble_snapshot(&mut h)["revision"],revision+1);
 }
 workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}

// Coordinates come from the same Cardinal layout as the rendered controls.
fn workstation_bottom_test_layout(h:&mut Host,page:crate::workstation::bottom_panel::Page,tab:crate::workstation::bottom_panel::EditorTab)->crate::workstation::bottom_panel::Layout {
 use crate::workstation::{bottom_panel as b,bottom_panel_adapter,panel_layout::{self,Viewport,Options},panel_motion};
 let mut motion=panel_motion::State::default();motion.set_all_open(true);motion.advance(1.);
 let r=panel_layout::visual_bounds(Viewport{width:1280.,height:720.,canvas_scale:1.},&motion.snapshot(),Options::default()).unwrap()[1];
 let snapshot=workstation_bubble_snapshot(h);let mut state=b::State::default();state.set_page(page,false);state.editor_tab=tab;
 state.layout(b::Rect{x:r.x,y:r.y,w:r.width,h:r.height},&bottom_panel_adapter::context(&snapshot,Some("bottom-audio"),0))
}
fn workstation_bottom_test_button(h:&mut Host,page:crate::workstation::bottom_panel::Page,tab:crate::workstation::bottom_panel::EditorTab,id:crate::workstation::bottom_panel::ControlId){
 let layout=workstation_bottom_test_layout(h,page,tab);let r=layout.controls.iter().find(|c|c.id==id).unwrap().rect;
 workstation_bubble_click_without_open(h,r.x+r.w*0.5,r.y+r.h*0.5);
}
#[test]fn workstation_bottom_pages_faders_and_disabled_spectral_use_real_routes(){
 use crate::workstation::{bottom_panel::{Page as P,EditorTab as T,ControlId as C},bottom_panel_scene as scene,arrangement,panel_layout::{Options,Viewport},panel_motion,piano_roll_scene};
 let mut h=traced_host();
 for (hook,mut request) in [("PutTrack",json!({"id":"bottom-track","name":"Recorded audio","kind":"audio"})),("PutSource",json!({"id":"bottom-source","channels":1,"sample_rate":8000,"samples":(0..4000).map(|i|(i as f64*0.3).sin()*0.5).collect::<Vec<_>>()})),("PutClip",json!({"id":"bottom-audio","track":"bottom-track","kind":"audio","source":"bottom-source","start_seconds":0.,"source_offset_seconds":0.,"duration_seconds":0.5}))]{request["document_revision"]=workstation_bubble_snapshot(&mut h)["revision"].clone();h.invoke(&format!("WorkstationDataSystem.{hook}"),request).unwrap();}
 workstation_bubble_begin(&mut h);
 let l=workstation_bottom_test_layout(&mut h,P::Mixer,T::Waveform);let r=l.faders[0].knob;let p=[r.x+r.w*0.5,r.y+r.h*0.5];
 let before=workstation_bubble_snapshot(&mut h)["tracks"][0]["gain"].as_f64().unwrap();let old_pose=h.graphics.workstation_frame.as_ref().unwrap()[crate::workstation::fader_components::BASE+9];
 workstation_bubble_step(&mut h,json!({"pointer":p,"left":true}));
 workstation_bubble_step(&mut h,json!({"pointer":[p[0],p[1]+20.],"left":true}));
 workstation_bubble_step(&mut h,json!({"pointer":[p[0],p[1]+20.],"left":false}));
 assert!(workstation_bubble_snapshot(&mut h)["tracks"][0]["gain"].as_f64().unwrap()<before,"actual fader drag must change document gain");
 assert_ne!(h.graphics.workstation_frame.as_ref().unwrap()[crate::workstation::fader_components::BASE+9],old_pose,"actual uploaded slider must move");
 let mut motion=panel_motion::State::default();motion.set_all_open(true);motion.advance(1.);let canvas=crate::workstation::drum_rack_ui::Ui::default().timeline_rect(piano_roll_scene::canvas(Viewport{width:1280.,height:720.,canvas_scale:1.},&motion.snapshot(),Options::default()).unwrap());let body=arrangement::clip_body(canvas);
 let selected=workstation_bubble_click_without_open(&mut h,body.left+5.,body.top+32.);assert_eq!(selected["arrangement"]["selected_clip"],"bottom-audio");
 for (page,next) in [(P::Mixer,1.),(P::Plugins,3.),(P::ClipEditor,0.)]{workstation_bottom_test_button(&mut h,page,T::Waveform,C::SelectPage(next as usize));assert_eq!(h.graphics.workstation_frame.as_ref().unwrap()[scene::BASE+2][1],next);}
 assert!(h.graphics.workstation_frame.as_ref().unwrap()[scene::BASE+2][2]>0.,"selected audio waveform must reach native packet");
 workstation_bottom_test_button(&mut h,P::ClipEditor,T::Waveform,C::Spectral);
 assert_eq!(h.graphics.workstation_frame.as_ref().unwrap()[scene::BASE+4][0],0.,"disabled spectral editor must not create a selection");
 assert!(workstation_bubble_snapshot(&mut h)["clip_treatments"].as_array().unwrap().is_empty());
 workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}

#[test]
fn workstation_piano_room_lower_notes_and_arrows_cannot_operate_hidden_panels(){
 let mut host=traced_host();workstation_bubble_begin(&mut host);workstation_bubble_open_fixture(&mut host);
 let before=host.invoke("AudioWorkstationSystem.Status",Value::Null).unwrap();
 workstation_bubble_click(&mut host,600.,640.);
 let snapshot=workstation_bubble_snapshot(&mut host);
 assert_eq!(snapshot["clips"][0]["notes"].as_array().unwrap().len(),1,"lower piano field must draw instead of operating the hidden mixer");
 workstation_bubble_step(&mut host,json!({"arrows":[false,false,true,false]}));
 let after=host.invoke("AudioWorkstationSystem.Status",Value::Null).unwrap();assert_eq!(before["panels"],after["panels"]);
 assert_eq!(after["piano_workspace"]["separate_scene"],true);
 let rows=host.graphics.workstation_frame.as_ref().unwrap();assert_eq!(rows[crate::workstation::piano_scene::BASE][0],1.);
 workstation_bubble_close(&mut host);workstation_bubble_remove_trace(host);
}

#[test]
fn retained_piano_click_survives_a_complete_native_press_release_between_game_frames(){
 for mode in ["Begin","Resume"]{
  let mut host=traced_host();host.invoke(&format!("AudioWorkstationSystem.{mode}"),Value::Null).unwrap();workstation_bubble_step(&mut host,Value::Null);workstation_bubble_open_fixture(&mut host);
  let tap=json!({"pointer":[600.,360.],"left":false,"focused":true,"transport_pointer_events":[{"pointer":[600.,360.],"left":true},{"pointer":[600.,360.],"left":false}]});
  let state=workstation_bubble_step(&mut host,tap);let doc=workstation_bubble_snapshot(&mut host);
  assert_eq!(doc["clips"][0]["notes"].as_array().unwrap().len(),1,"{mode}: a complete queued click must place a note even though the current held state is released");
  assert!(state["editor_error"].is_null());assert_eq!(state["piano_roll"]["gesture_active"],false);
  workstation_bubble_step(&mut host,json!({"pointer":[600.,360.],"left":false,"focused":true}));assert_eq!(workstation_bubble_snapshot(&mut host),doc);
  workstation_bubble_close(&mut host);workstation_bubble_remove_trace(host);
 }
}

fn retained_piano_host()->Host{let mut h=traced_host();h.invoke("AudioWorkstationSystem.Resume",Value::Null).unwrap();workstation_bubble_step(&mut h,Value::Null);workstation_bubble_open_fixture(&mut h);h}
#[test]
fn retained_piano_two_clicks_keep_callback_positions_and_do_not_retoggle_held_snap(){
 let mut h=retained_piano_host();let snap=workstation_bubble_step(&mut h,json!({"snap_toggle":true}))["piano_roll"]["snap_quarters"].clone();
 let s=workstation_bubble_step(&mut h,json!({"pointer":[1000.,450.],"left":false,"focused":true,"snap_toggle":true,"transport_pointer_events":[{"pointer":[500.,350.],"left":true},{"pointer":[500.,350.],"left":false},{"pointer":[800.,400.],"left":true},{"pointer":[800.,400.],"left":false}]}));
 let d=workstation_bubble_snapshot(&mut h);let notes=d["clips"][0]["notes"].as_array().unwrap();assert_eq!(notes.len(),2);assert!(notes[0]["quarter"].as_f64().unwrap()<6.);assert!(notes[1]["quarter"].as_f64().unwrap()<10.);assert_eq!(s["piano_roll"]["snap_quarters"],snap);assert!(s["editor_error"].is_null());
 workstation_bubble_step(&mut h,json!({"command":true,"undo":true}));assert_eq!(workstation_bubble_snapshot(&mut h)["clips"][0]["notes"].as_array().unwrap().len(),1);
 workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}
#[test]
fn retained_piano_cancelled_or_unfocused_batches_cannot_create_notes(){
 for cancelled in [false,true]{let mut h=retained_piano_host();let before=workstation_bubble_snapshot(&mut h);
 workstation_bubble_step(&mut h,json!({"pointer":[600.,360.],"left":false,"focused":cancelled,"transport_pointer_cancelled":cancelled,"transport_pointer_events":[{"pointer":[600.,360.],"left":true},{"pointer":[600.,360.],"left":false}]}));
 assert_eq!(workstation_bubble_snapshot(&mut h),before);workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);}
}
#[test]
fn retained_piano_blur_cancels_a_draft_instead_of_committing_it(){let mut h=retained_piano_host();let before=workstation_bubble_snapshot(&mut h);workstation_bubble_step(&mut h,json!({"pointer":[600.,360.],"left":true,"focused":true}));workstation_bubble_step(&mut h,json!({"focused":false,"transport_pointer_cancelled":true,"left":false}));assert_eq!(workstation_bubble_snapshot(&mut h),before);workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);}
#[test]
fn retained_piano_ignores_clicks_starting_outside_the_note_field(){let mut h=retained_piano_host();let before=workstation_bubble_snapshot(&mut h);workstation_bubble_step(&mut h,json!({"pointer":[600.,360.],"left":false,"focused":true,"transport_pointer_events":[{"pointer":[20.,360.],"left":true},{"pointer":[600.,360.],"left":false}]}));assert_eq!(workstation_bubble_snapshot(&mut h),before);workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);}

#[test]
fn retained_piano_held_press_commits_once_on_queued_release(){let mut h=retained_piano_host();let down=workstation_bubble_step(&mut h,json!({"pointer":[600.,360.],"left":true,"focused":true,"transport_pointer_events":[{"pointer":[600.,360.],"left":true}]}));assert_eq!(down["piano_roll"]["gesture_active"],true);assert_eq!(workstation_bubble_snapshot(&mut h)["clips"][0]["notes"].as_array().unwrap().len(),0);workstation_bubble_step(&mut h,json!({"pointer":[640.,360.],"left":false,"focused":true,"transport_pointer_events":[{"pointer":[640.,360.],"left":false}]}));assert_eq!(workstation_bubble_snapshot(&mut h)["clips"][0]["notes"].as_array().unwrap().len(),1);workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);}
#[test]
fn retained_piano_invalid_batch_cancels_without_partial_notes(){let mut h=retained_piano_host();let before=workstation_bubble_snapshot(&mut h);let s=workstation_bubble_step(&mut h,json!({"pointer":[600.,360.],"left":false,"focused":true,"transport_pointer_events":[{"pointer":[600.,360.],"left":true},{"pointer":[600.],"left":false}]}));assert_eq!(workstation_bubble_snapshot(&mut h),before);assert!(s["editor_error"].as_str().unwrap().contains("pointer"));workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);}

#[test]
fn retained_piano_field_click_does_not_inherit_keyboard_ruler_context(){let mut h=retained_piano_host();workstation_bubble_step(&mut h,json!({"pointer":[30.,360.],"left":false,"focused":true}));workstation_bubble_step(&mut h,json!({"pointer":[600.,360.],"left":false,"focused":true,"transport_pointer_events":[{"pointer":[600.,360.],"left":true},{"pointer":[600.,360.],"left":false}]}));assert_eq!(workstation_bubble_snapshot(&mut h)["clips"][0]["notes"].as_array().unwrap().len(),1);workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);}

#[test]
fn retained_piano_click_keeps_callback_context_when_cursor_moves_to_keyboard(){let mut h=retained_piano_host();workstation_bubble_step(&mut h,json!({"pointer":[30.,360.],"left":false,"focused":true,"transport_pointer_events":[{"pointer":[600.,360.],"left":true},{"pointer":[600.,360.],"left":false}]}));assert_eq!(workstation_bubble_snapshot(&mut h)["clips"][0]["notes"].as_array().unwrap().len(),1);workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);}

#[test]fn workstation_clip_gain_split_trim_registered_host_routes(){std::thread::Builder::new().stack_size(16*1024*1024).spawn(||{let mut h=traced_host();for(hook,mut v)in[("PutTrack",json!({"id":"a","kind":"audio","name":"Audio"})),("PutSource",json!({"id":"s","sample_rate":8000,"channels":1,"samples":vec![0.5;64]})),("PutClip",json!({"id":"c","track":"a","kind":"audio","source":"s","start_seconds":0.,"source_offset_seconds":0.,"duration_seconds":0.008})),("SetClipGain",json!({"id":"c","gain":0.5})),("TrimAudioClip",json!({"id":"c","start_fraction":0.25,"end_fraction":0.75})),("SplitClip",json!({"id":"c","time_seconds":0.004}))]{v["document_revision"]=workstation_bubble_snapshot(&mut h)["revision"].clone();h.invoke(&format!("WorkstationDataSystem.{hook}"),v).unwrap();}let snapshot=workstation_bubble_snapshot(&mut h);assert_eq!(snapshot["clips"].as_array().unwrap().len(),2);assert!(snapshot["clips"].as_array().unwrap().iter().all(|c|c["gain"]==0.5));workstation_bubble_remove_trace(h);}).unwrap().join().unwrap();}

#[test]
fn workstation_cardinal_undo_routes_and_timeline_shortcuts() {
    let mut host=traced_host();
    workstation_bubble_begin(&mut host);
    let before=workstation_bubble_snapshot(&mut host);
    host.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":before["revision"],"id":"undo-route","name":"Undo route","kind":"audio"})).unwrap();
    let status=host.invoke("WorkstationDataSystem.UndoStatus",json!({})).unwrap();
    assert_eq!(status["undo_available"],true);
    workstation_bubble_step(&mut host,json!({"command":true,"undo":true}));
    assert!(!workstation_bubble_snapshot(&mut host)["tracks"].as_array().unwrap().iter().any(|t|t["id"]=="undo-route"));
    workstation_bubble_step(&mut host,json!({}));
    workstation_bubble_step(&mut host,json!({"command":true,"redo":true}));
    assert!(workstation_bubble_snapshot(&mut host)["tracks"].as_array().unwrap().iter().any(|t|t["id"]=="undo-route"));
    workstation_bubble_close(&mut host);workstation_bubble_remove_trace(host);
}

#[test]fn cursor_artwork_request_validates_preview_and_waveform(){
 for kind in ["audio","midi","cc","comp","matrix","mix","tape","plugin"]{
  assert!(super::check_payload("CursorLabelRequest",&json!({"entity":1,"label":{"text":"Preview","position":[12.,8.],"preview":kind,"wave":vec![[-1.,1.];128]}})));
 }
 for label in [json!({"text":"Preview","position":[12.,8.],"preview":"unknown"}),json!({"text":"Preview","position":[12.,8.],"preview":"eq"}),json!({"text":"Preview","position":[12.,8.],"preview":"tape","wave":[[0.,1.]]})]{assert!(!super::check_payload("CursorLabelRequest",&json!({"entity":1,"label":label})));}
 let graph=crate::workstation::stock_eq::presentation(&json!({"state":{},"response_db":vec![0.;97]}),[0.,36.,230.,132.],1.);
 assert!(super::check_payload("CursorLabelRequest",&json!({"entity":1,"label":{"text":"EQ","position":[12.,8.],"preview":"eq","eq":graph}})));
}

#[test]
fn drum_rack_tab_round_robin_owns_fullscreen_and_retains_document() {
 let mut host=traced_host();workstation_bubble_begin(&mut host);
 let before=workstation_bubble_snapshot(&mut host);
 let first=workstation_bubble_step(&mut host,json!({"tab":true,"pointer":[640.,360.]}));
 assert_eq!(first["piano_workspace"]["active"],true);assert_eq!(first["drum_rack_open"],false);
 workstation_bubble_step(&mut host,json!({"tab":false}));
 let second=workstation_bubble_step(&mut host,json!({"tab":true,"pointer":[640.,360.]}));
 assert_eq!(second["piano_workspace"]["active"],false);assert_eq!(second["drum_rack_open"],true);
 workstation_bubble_step(&mut host,json!({"tab":false,"pointer":[640.,360.]}));
 assert_eq!(host.graphics.workstation_frame.as_ref().unwrap()[crate::workstation::piano_scene::BASE][3],1.);
 let held=workstation_bubble_step(&mut host,json!({"tab":true,"pointer":[640.,360.]}));
 assert_eq!(held["drum_rack_open"],false);assert_eq!(held["piano_workspace"]["active"],false);
 let held=workstation_bubble_step(&mut host,json!({"tab":true}));assert_eq!(held["piano_workspace"]["active"],false);
 assert_eq!(workstation_bubble_snapshot(&mut host),before);
 workstation_bubble_close(&mut host);workstation_bubble_remove_trace(host);
}
