#[test]
fn audio_library_import_uses_registered_host_route_and_existing_pcm_mixer() {
    let mut h=traced_host();
    let path=h.trace.with_extension("wav");
    let mut bytes=Vec::new();
    bytes.extend(b"RIFF");bytes.extend(44u32.to_le_bytes());bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());bytes.extend(1u16.to_le_bytes());bytes.extend(2u16.to_le_bytes());
    bytes.extend(48000u32.to_le_bytes());bytes.extend(192000u32.to_le_bytes());bytes.extend(4u16.to_le_bytes());bytes.extend(16u16.to_le_bytes());
    bytes.extend(b"data");bytes.extend(8u32.to_le_bytes());
    for s in [8192i16,-16384,16384,-8192] {bytes.extend(s.to_le_bytes());}
    fs::write(&path,bytes).unwrap();
    h.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":0,"id":"a","name":"Audio","kind":"audio"})).unwrap();
    h.stack.push((5,"AudioWorkstationSystem.Step".into()));
    let result=h.invoke("WorkstationDataSystem.ImportAudio",json!({"document_revision":1,"track":"a","path":path,"start_seconds":1.,"sample_rate":48000})).unwrap();
    h.stack.pop();
    assert_eq!(result["revision"],2);
    let pcm=h.invoke("WorkstationDataSystem.RenderRange",json!({"document_revision":2,"start_frame":48000,"frames":2,"sample_rate":48000})).unwrap();
    assert_eq!(pcm["lsffr"],json!([[0.,0.,-0.25,0.],[0.,0.,0.25,0.]]));
    assert_eq!(h.entities.len(),0);assert!(h.active_audicle.is_none());
    drop(h.trace_writer.borrow_mut().take());
    fs::remove_file(path).unwrap();fs::remove_file(&h.trace).unwrap();
}

#[test]
fn workstation_tutorial_retained_clicks_are_visible_and_do_not_leak_into_the_editor() {
    let mut h=traced_host();
    workstation_bubble_begin(&mut h);
    let before=workstation_bubble_snapshot(&mut h);
    let status=h.invoke("AudioWorkstationSystem.Step",json!({"dt":1.,"aspect":1280./720.,"viewport":[1280.,720.],
        "framebuffer":[1280.,720.],"canvas_scale":1.,"keys":[false,false,false,false,false],"right_panel_page":1,"editor":null})).unwrap();
    let control=&status["track_io"]["controls"][0];
    assert_eq!(control["text"],"LEARN");
    let r: [f64;4]=serde_json::from_value(control["rect"].clone()).unwrap();
    let p=[r[0]+r[2]*0.5,r[1]+r[3]*0.5];
    // A complete press/release between frames activates the ruby on the next update.
    workstation_bubble_step(&mut h,json!({"pointer":p,"left":false,
        "transport_pointer_events":[{"pointer":p,"left":true},{"pointer":p,"left":false}]}));
    let status=workstation_bubble_step(&mut h,json!({"pointer":p,"left":false}));
    assert_eq!(status["tutorial"]["open"],true);
    let status=workstation_bubble_step(&mut h,json!({"pointer":[430.,62.],"left":false,
        "transport_pointer_events":[{"pointer":[430.,62.],"left":true},{"pointer":[430.,62.],"left":false}]}));
    assert_eq!(status["tutorial"]["lesson"],"audio");
    assert_eq!(workstation_bubble_snapshot(&mut h),before);
    // Use the real existing piano editor to prove card actions cannot draw notes.
    workstation_bubble_open_fixture(&mut h);
    let before=workstation_bubble_snapshot(&mut h);
    let status=workstation_bubble_step(&mut h,json!({"pointer":[670.,62.],"left":false,
        "transport_pointer_events":[{"pointer":[670.,62.],"left":true},{"pointer":[670.,62.],"left":false}]}));
    assert_eq!(status["tutorial"]["lesson"],"midi");
    assert_eq!(workstation_bubble_snapshot(&mut h),before);
    workstation_bubble_close(&mut h);
    drop(h.trace_writer.borrow_mut().take());fs::remove_file(&h.trace).unwrap();
}

#[test]
fn audio_library_import_without_a_track_uses_available_matrix_and_waveform_route(){
 let mut h=traced_host();let path=h.trace.with_extension("wav");
 let mut wav=Vec::new();wav.extend(b"RIFF");wav.extend(40u32.to_le_bytes());wav.extend(b"WAVEfmt ");wav.extend(16u32.to_le_bytes());wav.extend(1u16.to_le_bytes());wav.extend(1u16.to_le_bytes());wav.extend(8000u32.to_le_bytes());wav.extend(16000u32.to_le_bytes());wav.extend(2u16.to_le_bytes());wav.extend(16u16.to_le_bytes());wav.extend(b"data");wav.extend(4u32.to_le_bytes());wav.extend(8192i16.to_le_bytes());wav.extend(0i16.to_le_bytes());fs::write(&path,wav).unwrap();
 let imported=h.invoke("WorkstationDataSystem.ImportAudio",json!({"document_revision":0,"path":path,"sample_rate":8000,"start_seconds":1.,"lane":2})).unwrap();
 let snapshot=h.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap();assert_eq!(snapshot["tracks"],json!([]));assert_eq!(snapshot["clips"][0]["track"],"");assert_eq!(snapshot["clips"][0]["lane"],2);assert_eq!(snapshot["clips"][0]["output"],"matrix-FF");
 h.stack.push((5,"AudioWorkstationSystem.Step".into()));
 h.invoke("WorkstationDataSystem.SetClipRouting",json!({"document_revision":snapshot["revision"],"id":imported["clip"],"output":"matrix-R"})).unwrap();h.stack.pop();
 let snapshot=h.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap();
 let pcm=h.invoke("WorkstationDataSystem.RenderRange",json!({"document_revision":snapshot["revision"],"sample_rate":8000,"start_frame":8000,"frames":2})).unwrap();assert_eq!(pcm["matrix_pcm"][0],json!([0.,0.,0.,0.5]));
 drop(h.trace_writer.borrow_mut().take());fs::remove_file(path).unwrap();fs::remove_file(&h.trace).unwrap();
}
