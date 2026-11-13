// Exercise the registered application routes and actual optical packet.
#[test]
fn fullscreen_tests_registered_shortcuts_visibility_and_escape() {
    let mut h=traced_host();
    h.invoke("AudioWorkstationSystem.Begin",Value::Null).unwrap();
    let step=|h:&mut Host,keys:[bool;5],expand:bool,editor:Value|h.invoke("AudioWorkstationSystem.Step",json!({
        "dt":1.,"aspect":1280./720.,"viewport":[1280.,720.],"framebuffer":[1280.,720.],
        "canvas_scale":1.,"keys":keys,"expand":expand,"editor":editor})).unwrap();
    step(&mut h,[false;5],false,Value::Null);
    for (index,edge) in [(0,"top"),(2,"left"),(1,"bottom"),(3,"right")] {
        let mut keys=[false;5];keys[index]=true;
        let status=step(&mut h,keys,true,Value::Null);
        assert_eq!(status["fullscreen_panel"],edge);
        assert_eq!(status["panel_view"],"extended");
        let rows=h.graphics.workstation_frame.as_ref().unwrap();
        for i in 0..4 {assert_eq!(rows[32+40*i][0],if i==index{1.}else{0.});}
        // Held input must not repeatedly toggle the full view.
        assert_eq!(step(&mut h,keys,true,Value::Null)["fullscreen_panel"],edge);
        assert_eq!(step(&mut h,keys,true,Value::Null)["panel_view"],"extended");
        step(&mut h,[false;5],false,Value::Null);
        assert_eq!(step(&mut h,keys,true,Value::Null)["panel_view"],"fullscreen");
        step(&mut h,[false;5],false,Value::Null);
        assert_eq!(step(&mut h,keys,true,Value::Null)["panel_view"],"normal");
        step(&mut h,[false;5],false,Value::Null);
        assert_eq!(step(&mut h,keys,true,Value::Null)["panel_view"],"extended");
        let status=step(&mut h,[false;5],false,json!({"escape":true}));
        assert!(status["fullscreen_panel"].is_null());
        let rows=h.graphics.workstation_frame.as_ref().unwrap();
        assert!((0..4).all(|i|rows[32+40*i][0]==1.));
        step(&mut h,[false;5],false,Value::Null);
    }
    h.invoke("AudioWorkstationSystem.SetPanel",json!({"edge":"bottom","expanded":true})).unwrap();
    step(&mut h,[false;5],false,Value::Null);
    let status=step(&mut h,[false,true,false,false,false],false,Value::Null);
    assert_eq!(status["panels"][1]["expanded"],false);
    assert_eq!(status["panels"][1]["open"],true);
    assert_eq!(h.invoke("AudioWorkstationSystem.SetPanel",json!({"edge":"top","fullscreen":true})).unwrap()["panel_view"],"fullscreen");
    workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}

#[test]
fn workstation_normal_track_fader_registered_input_routes() {
    let mut h=traced_host();
    h.invoke("AudioWorkstationSystem.Begin",Value::Null).unwrap();
    let revision=h.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap()["revision"].clone();
    h.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":revision,"id":"fader-check","name":"Fader check","kind":"audio"})).unwrap();
    let step=|h:&mut Host,editor:Value|h.invoke("AudioWorkstationSystem.Step",json!({"dt":1.,"aspect":1280./800.,"viewport":[1280.,800.],"framebuffer":[1280.,800.],"canvas_scale":1.,"keys":[false,false,false,false,false],"editor":editor})).unwrap();
    let status=step(&mut h,json!({"focused":true}));
    let knob=&status["bottom_panel"]["faders"][0]["knob"];
    let x=knob[0].as_f64().unwrap()+12.5;let y=knob[1].as_f64().unwrap()+12.5;
    let down=step(&mut h,json!({"pointer":[x,y],"left":true,"focused":true}));
    assert_eq!(down["arrangement"]["selected_track"],"fader-check","{}",down["bottom_panel"]);
    let moved=step(&mut h,json!({"pointer":[x,y+35.],"left":true,"focused":true}));
    assert_ne!(moved["bottom_panel"]["faders"][0]["normalized"],status["bottom_panel"]["faders"][0]["normalized"],"{}",moved["bottom_panel"]);
    step(&mut h,json!({"pointer":[x,y+35.],"left":false,"focused":true}));
    workstation_bubble_close(&mut h);workstation_bubble_remove_trace(h);
}
