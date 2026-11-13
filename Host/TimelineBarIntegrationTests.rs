// Actual registered Host routes and audio queue; no device/window launch.
#[test]
fn waveform_timeline_refreshes_signed_source_detail_for_zoom_and_pan(){
 let mut h=traced_host();
 h.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":0,"id":"a","name":"Audio","kind":"audio"})).unwrap();
 let samples:Vec<f32>=(0..48000).map(|i|(std::f32::consts::TAU*440.*i as f32/48000.).sin()).collect();
 h.invoke("WorkstationDataSystem.PutSource",json!({"document_revision":1,"id":"s","sample_rate":48000,"channels":1,"samples":samples})).unwrap();
 h.invoke("WorkstationDataSystem.PutClip",json!({"document_revision":2,"id":"c","track":"a","kind":"audio","start_seconds":0.,"source":"s","duration_seconds":1.,"source_offset_seconds":0.})).unwrap();
 let doc=h.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap();
 let mut a=crate::workstation::arrangement::Arrangement::default();a.sync(&doc).unwrap();
 let r=crate::workstation::piano_roll::Rect{left:0.,top:0.,width:1000.,height:500.};
 let body=crate::workstation::arrangement::clip_body(r);
 a.update(&crate::workstation::piano_roll::Input{pointer:Some([body.left+1.,100.]),wheel:[0.,-45.],command:true,..Default::default()},r);
 h.stack.push((5,"AudioWorkstationSystem.Step".into()));
 a.refresh_waveforms(&mut h,r,false).unwrap();
 let first=a.presentation(r,None,Value::Null);
 let wave=first["clips"][0]["waveform_detail"].as_array().unwrap();assert_eq!(wave.len(),128);
 assert!(wave.iter().any(|p|p[0].as_f64().unwrap()>0.5));assert!(wave.iter().any(|p|p[1].as_f64().unwrap()< -0.5));
 let kit=crate::workstation::timeline_scene::Kit::load(Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
 let mut rows=vec![[0.;4];crate::workstation::timeline_scene::END];
 kit.append(&mut rows,crate::workstation::panel_layout::Viewport{width:1000.,height:500.,canvas_scale:1.},&first).unwrap();
 let wave_at=crate::workstation::timeline_scene::WAVE_BASE+crate::workstation::timeline_scene::WAVE_STRIDE;
 assert_eq!(rows[wave_at][0],2.,"signed detail must reach the production tape shader");
 assert!(rows[wave_at][1].abs()<1e-5);assert!((rows[wave_at][2]-1.).abs()<1e-5);
 for i in 0..128{for c in 0..4{assert_eq!(rows[wave_at+1+i][c],wave[i][c].as_f64().unwrap() as f32);}}
 a.refresh_waveforms(&mut h,r,false).unwrap();assert_eq!(first,a.presentation(r,None,Value::Null));
 a.update(&crate::workstation::piano_roll::Input{pointer:Some([body.left+1.,100.]),wheel:[0.,1.],..Default::default()},r);
 a.refresh_waveforms(&mut h,r,false).unwrap();let next=a.presentation(r,None,Value::Null);
 assert_ne!(first["clips"][0]["waveform_view"],next["clips"][0]["waveform_view"]);
 assert_eq!(h.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap()["revision"],doc["revision"]);
 h.stack.clear();let trace=h.trace.clone();drop(h);let _=fs::remove_file(trace);
}
#[test]
fn timeline_rebase_contract_commits_document_clock_and_loop_together_and_rewind_uses_zero(){
 let mut h=traced_host();
 h.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":0,"id":"m","name":"MIDI","kind":"midi"})).unwrap();
 h.invoke("WorkstationDataSystem.PutClip",json!({"document_revision":1,"id":"c","track":"m","kind":"midi","start_seconds":1.,"bpm":93.,"length_quarters":4.,"notes":[]})).unwrap();
 h.stack.push((5,"AudioWorkstationSystem.Step".into()));
 h.stack.push((5,"EnvironmentAudioSystem.Begin".into()));
 let event=h.invoke("AudicleSystem.Create",Value::Null).unwrap();h.invoke("AudicleSystem.Ready",event.clone()).unwrap();h.invoke("AudicleSystem.Go",event.clone()).unwrap();h.stack.pop();
 let _output=h.environment_audio.test_prepare_transport(8000,event);
 let before=h.invoke("EnvironmentAudioSystem.Status",Value::Null).unwrap();
 let rejected=json!({"action":"rebase","document_revision":0,"shift_frames":32000,"start_frame":16000,"enabled":true,"loop_start_frame":32000,"loop_end_frame":112000});
 assert!(h.invoke("EnvironmentAudioSystem.Transport",rejected.clone()).is_err());
 assert_eq!(before["transport"],h.invoke("EnvironmentAudioSystem.Status",Value::Null).unwrap()["transport"]);
 let mut accepted=rejected;accepted["document_revision"]=json!(2);
 let audio=h.invoke("EnvironmentAudioSystem.Transport",accepted).unwrap();
 assert_eq!(audio["position_frame"],16000);assert_eq!(audio["timeline_zero_frame"],32000);assert_eq!(audio["loop_start_frame"],32000);assert_eq!(audio["loop_end_frame"],112000);assert_eq!(audio["loop_enabled"],true);
 let doc=h.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap();assert_eq!(doc["clips"][0]["start_seconds"],5.);assert_eq!(doc["clips"][0]["bpm"],93.);assert_eq!(doc["timeline_zero_seconds"],4.);
 let rewind=h.invoke("EnvironmentAudioSystem.Transport",json!({"action":"stop"})).unwrap();assert_eq!(rewind["position_frame"],32000);
 h.stack.clear();
 h.invoke("WorkstationDataSystem.SetMusicalTime",json!({"document_revision":3,"bpm":150.,"grid_choice":"fixed_1_8"})).unwrap();
 let map=h.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap();assert_eq!(map["musical_time"]["bpm"],150.);assert_eq!(map["musical_time"]["revision"],2);assert_eq!(map["clips"],doc["clips"]);
 let trace=h.trace.clone();drop(h);let _=fs::remove_file(trace);
}

#[test]fn one_sample_loop_pump_uses_one_full_render_at_eight_khz(){
 let mut h=traced_host();h.stack.push((5,"AudioWorkstationSystem.Step".into()));
 h.stack.push((5,"EnvironmentAudioSystem.Begin".into()));
 let event=h.invoke("AudicleSystem.Create",Value::Null).unwrap();h.invoke("AudicleSystem.Ready",event.clone()).unwrap();h.invoke("AudicleSystem.Go",event.clone()).unwrap();h.stack.pop();
 let _output=h.environment_audio.test_prepare_transport(8000,event);
 h.invoke("EnvironmentAudioSystem.Transport",json!({"action":"loop","enabled":true,"loop_start_frame":0,"loop_end_frame":1})).unwrap();
 let status=h.invoke("EnvironmentAudioSystem.Transport",json!({"action":"play"})).unwrap();
 assert_eq!(status["render_calls_last_pump"],1);assert_eq!(status["queued_frames"],1024);assert_eq!(status["playing"],true);
 let trace=h.trace.clone();drop(h);let _=fs::remove_file(trace);
}

#[test]
fn capture_transport_applies_master_gain_mode_and_speaker_controls_before_play() {
 let mut h=traced_host();
 h.stack.push((5,"AudioWorkstationSystem.Step".into()));
 h.stack.push((5,"EnvironmentAudioSystem.Begin".into()));
 let event=h.invoke("AudicleSystem.Create",Value::Null).unwrap();
 h.invoke("AudicleSystem.Ready",event.clone()).unwrap();h.invoke("AudicleSystem.Go",event.clone()).unwrap();h.stack.pop();
 let mut output=h.environment_audio.test_prepare_transport(48000,event);
 let _capture=h.environment_audio.test_attach_capture();
 h.invoke("EnvironmentAudioSystem.Transport",json!({"action":"mode","mode":"world"})).unwrap();
 let playing=h.invoke("EnvironmentAudioSystem.Transport",json!({"action":"play","mode":"stereo","master_gain":0.25})).unwrap();
 assert_eq!(playing["playing"],true);assert_eq!(playing["mode"],"stereo");
 h.environment_audio.test_enqueue_buses(&[[0.8,0.,0.,-0.4]]);
 output.begin_block();assert_eq!(output.frame(),[0.2,-0.1]);output.end_block(1);
 let muted=h.invoke("EnvironmentAudioSystem.Transport",json!({"action":"speakers","enabled":false})).unwrap();assert_eq!(muted["speakers_enabled"],false);
 let paused=h.invoke("EnvironmentAudioSystem.Transport",json!({"action":"pause","mode":"world","master_gain":0.5})).unwrap();assert_eq!(paused["playing"],false);assert_eq!(paused["mode"],"world");assert_eq!(paused["speakers_enabled"],false);
 h.invoke("EnvironmentAudioSystem.Transport",json!({"action":"play"})).unwrap();
 h.environment_audio.test_enqueue_buses(&[[0.8,0.,0.,-0.4],[0.8,0.,0.,-0.4]]);
 output.begin_block();assert_eq!(output.frame(),[0.,0.]);output.end_block(1);
 h.invoke("EnvironmentAudioSystem.Transport",json!({"action":"mode","mode":"stereo"})).unwrap();
 output.begin_block();assert_eq!(output.frame(),[0.4,-0.2]);output.end_block(1);
 h.stack.clear();let trace=h.trace.clone();drop(h);let _=fs::remove_file(trace);
}

#[test]
fn drum_rack_registered_route_uses_revision_and_renders_samples(){
 let mut h=traced_host();
 h.invoke("WorkstationDataSystem.PutSource",json!({"document_revision":0,"id":"hit","sample_rate":8000,"channels":1,"samples":[1.,0.5,0.25]})).unwrap();
 h.stack.push((123,"AudioWorkstationSystem.Step".into()));
 for (revision,command) in [json!({"action":"add","track":"rack","name":"Hit","source":"hit"}),json!({"action":"steps","channel":0,"steps":1}),json!({"action":"place","channel":0,"id":"pattern","start_seconds":0.})].into_iter().enumerate(){let mut command=command;command["document_revision"]=json!(revision+1);h.invoke("WorkstationDataSystem.DrumRack",command).unwrap();}
 h.stack.pop();
 let result=h.invoke("WorkstationDataSystem.RenderRange",json!({"document_revision":4,"sample_rate":8000,"start_frame":0,"frames":3})).unwrap();
 assert!((result["lsffr"][0][2].as_f64().unwrap()-1.64).abs()<1e-6);
 assert!(h.invoke("WorkstationDataSystem.DrumRack",json!({"document_revision":0,"action":"preview","enabled":true})).is_err());

}

#[test]
fn drum_rack_preview_stops_on_scene_close_and_patterns_remain(){
 let mut h=traced_host();
 h.invoke("WorkstationDataSystem.DrumRack",json!({"document_revision":0,"action":"add","track":"drum","name":"Hit"})).unwrap();
 h.invoke("AudioWorkstationSystem.Begin",Value::Null).unwrap();
 h.invoke("AudioWorkstationSystem.DrumRack",json!({"action":"preview","enabled":true})).unwrap();
 assert_eq!(h.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap()["drum_rack"]["preview"],true);
 h.invoke("AudioWorkstationSystem.Close",Value::Null).unwrap();
 let snapshot=h.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap();assert_eq!(snapshot["drum_rack"]["preview"],false);assert_eq!(snapshot["tracks"][0]["id"],"drum");
}
