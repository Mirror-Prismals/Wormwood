use crate::workstation::document::{State, dispatch};
use serde_json::{Value, json};
fn edit(s: &mut State, hook: &str, mut v: Value) -> Value {
    v["document_revision"] = json!(s.revision);
    dispatch(s, hook, &v).unwrap()
}
fn fixture() -> State {
    let mut s = State::default();
    edit(
        &mut s,
        "PutTrack",
        json!({"id":"a","name":"AUDIO 1","kind":"audio"}),
    );
    let samples: Vec<f64> = (0..24000)
        .map(|i| 0.5 * (std::f64::consts::TAU * 1000. * i as f64 / 48000.).sin())
        .collect();
    edit(
        &mut s,
        "PutSource",
        json!({"id":"s","sample_rate":48000,"channels":1,"samples":samples}),
    );
    edit(
        &mut s,
        "PutClip",
        json!({"id":"c","track":"a","kind":"audio","start_seconds":0.,"source":"s","source_offset_seconds":0.,"duration_seconds":0.5}),
    );
    s
}

#[test]
fn waveform_zoom_resolves_440hz_cycles_samples_offsets_and_signed_stereo() {
    let mut s=fixture();
    let samples:Vec<f32>=(0..48000).flat_map(|i|{let v=(std::f32::consts::TAU*440.*i as f32/48000.).sin();[v,-v]}).collect();
    edit(&mut s,"PutSource",json!({"id":"s","sample_rate":48000,"channels":2,"samples":samples}));
    edit(&mut s,"PutClip",json!({"id":"c","track":"a","kind":"audio","start_seconds":0.,"source":"s","source_offset_seconds":0.25,"duration_seconds":0.5}));
    let revision=s.revision;
    let p=edit(&mut s,"ClipEditorPreview",json!({"id":"c","columns":480,"wave_view":[0.,0.02]}));
    let wave=p["stereo_waveform"].as_array().unwrap();
    let mut crossings=0;
    for (i,v) in wave.iter().enumerate(){
        let expected=samples[(12000+i)*2] as f64;
        assert!((v[0].as_f64().unwrap()-expected).abs()<1e-6);
        assert_eq!(v[0],v[1]);assert_eq!(v[2],v[3]);
        assert!((v[0].as_f64().unwrap()+v[2].as_f64().unwrap()).abs()<1e-6);
        if i>0 && wave[i-1][0].as_f64().unwrap()<0. && expected>=0. {crossings+=1;}
    }
    assert!((4..=5).contains(&crossings),"ten milliseconds must reveal 4.4 cycles");
    let p=edit(&mut s,"ClipEditorPreview",json!({"id":"c","columns":128,"wave_view":[0.1,0.1+1./24000.]}));
    assert!(p["stereo_waveform"].as_array().unwrap().iter().all(|v|*v==p["stereo_waveform"][0]));
    assert_eq!(s.revision,revision,"zoom must not edit the audio document");
    assert!(dispatch(&mut s,"ClipEditorPreview",&json!({"document_revision":revision,"id":"c","wave_view":[0.7,0.2]})).is_err());
}
fn mask() -> Value {
    json!({"id":"c","mask":{"source_start_frame":4000,"source_end_frame":20000,"low_hz":500.,"high_hz":1500.,"attenuation_db":-48.,"feather":0.12}})
}
fn render(s: &mut State) -> Value {
    edit(
        s,
        "RenderRange",
        json!({"sample_rate":48000,"start_frame":8000,"frames":4096}),
    )
}
fn energy(v: &Value) -> f64 {
    v["lsffr"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f[2].as_f64().unwrap().powi(2))
        .sum::<f64>()
}
#[test]
fn production_renderer_uses_repair_and_undo_restores_pcm() {
    let mut s = fixture();
    let original = render(&mut s);
    edit(&mut s, "AddSpectralMask", mask());
    let repaired = render(&mut s);
    assert!(energy(&repaired) < energy(&original) * 0.001);
    edit(&mut s, "UndoSpectralMask", json!({"id":"c"}));
    assert_eq!(render(&mut s)["lsffr"], original["lsffr"]);
}
#[test]
fn stale_revision_and_bad_mask_are_atomic() {
    let mut s = fixture();
    let before = dispatch(&mut s, "Snapshot", &Value::Null).unwrap();
    let mut bad = mask();
    bad["document_revision"] = json!(0);
    assert!(dispatch(&mut s, "AddSpectralMask", &bad).is_err());
    bad["document_revision"] = json!(s.revision);
    bad["mask"]["high_hz"] = json!(500.);
    assert!(dispatch(&mut s, "AddSpectralMask", &bad).is_err());
    assert_eq!(dispatch(&mut s, "Snapshot", &Value::Null).unwrap(), before);
}
#[test]
fn source_replacement_rebuilds_cache_and_rate_change_is_atomic() {
    let mut s = fixture();
    edit(&mut s, "AddSpectralMask", mask());
    let revision = s.revision;
    let invalid = json!({"document_revision":revision,"id":"s","sample_rate":44100,"channels":1,"samples":[0.,0.]});
    assert!(dispatch(&mut s, "PutSource", &invalid).is_err());
    assert_eq!(s.revision, revision);
    edit(
        &mut s,
        "PutSource",
        json!({"id":"s","sample_rate":48000,"channels":1,"samples":vec![0.;24000]}),
    );
    assert_eq!(energy(&render(&mut s)), 0.);
}
#[test]
fn independent_clips_sharing_source_do_not_share_masks() {
    let mut s = fixture();
    edit(
        &mut s,
        "PutClip",
        json!({"id":"clean","track":"a","kind":"audio","start_seconds":0.,"source":"s","source_offset_seconds":0.,"duration_seconds":0.5}),
    );
    let original = render(&mut s);
    edit(&mut s, "AddSpectralMask", mask());
    assert_eq!(render(&mut s)["lsffr"], original["lsffr"]);
    edit(&mut s, "RemoveClip", json!({"id":"clean"}));
    assert!(energy(&render(&mut s)) < energy(&original) * 0.001);
}
#[test]
fn trim_retains_source_masks_and_source_reassignment_clears_them() {
    let mut s = fixture();
    edit(&mut s, "AddSpectralMask", mask());
    edit(
        &mut s,
        "PutClip",
        json!({"id":"c","track":"a","kind":"audio","start_seconds":0.,"source":"s","source_offset_seconds":0.05,"duration_seconds":0.4}),
    );
    assert!(!s.clip_treatments.is_empty());
    edit(
        &mut s,
        "PutSource",
        json!({"id":"other","sample_rate":48000,"channels":1,"samples":vec![0.;24000]}),
    );
    edit(
        &mut s,
        "PutClip",
        json!({"id":"c","track":"a","kind":"audio","start_seconds":0.,"source":"other","source_offset_seconds":0.,"duration_seconds":0.5}),
    );
    assert!(s.clip_treatments.is_empty());
}
#[test]
fn shared_snapshot_reflects_mask_edits_and_preview_has_processed_spectrum() {
    let mut s = fixture();
    let before = s.shared_snapshot();
    edit(&mut s, "AddSpectralMask", mask());
    let after = s.shared_snapshot();
    assert!(!std::sync::Arc::ptr_eq(&before.document, &after.document));
    assert_eq!(
        before.document["clip_treatments"].as_array().unwrap().len(),
        0
    );
    assert_eq!(
        after.document["clip_treatments"].as_array().unwrap().len(),
        1
    );
    let preview = edit(
        &mut s,
        "ClipEditorPreview",
        json!({"id":"c","spectrogram":true,"columns":100,"bins":80}),
    );
    assert_eq!(preview["waveform"].as_array().unwrap().len(), 100);
    assert_eq!(
        preview["spectrogram"]["intensity"]
            .as_array()
            .unwrap()
            .len(),
        8000
    );
}
#[test]
fn active_export_freezes_repaired_cache_before_clear() {
    let mut s = fixture();
    edit(&mut s, "AddSpectralMask", mask());
    let root = std::env::temp_dir().join(format!("bottom-spectral-export-{}", std::process::id()));
    let destination = root.join("stems");
    std::fs::create_dir_all(&root).unwrap();
    let job = edit(
        &mut s,
        "ExportBegin",
        json!({"sample_rate":48000,"start_seconds":8000./48000.,"end_seconds":12096./48000.,"destination":destination}),
    );
    edit(&mut s, "ClearSpectralMasks", json!({"id":"c"}));
    for _ in 0..8 {
        let status = dispatch(&mut s, "ExportStep", &json!({"job_id":job["job_id"]})).unwrap();
        if status["phase"] == "completed" {
            break;
        }
    }
    let bytes = std::fs::read(destination.join("master_F.wav")).unwrap();
    let data = bytes.windows(4).position(|s| s == b"data").unwrap() + 8;
    let pcm: Vec<f32> = bytes[data..]
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
        .collect();
    assert_eq!(pcm.len(), 4096);
    assert!(pcm.iter().map(|v| v * v).sum::<f32>() < 0.1);
    assert!(energy(&render(&mut s)) > 100.);
    std::fs::remove_dir_all(root).unwrap();
}

struct TempoClock {revision:u64,bpm:f64}
impl super::warp_treatment::Clock for TempoClock{
 fn revision(&self)->u64{self.revision}
 fn seconds_to_beat(&self,s:f64)->Result<f64,String>{Ok(s*self.bpm/60.)}
 fn beat_to_seconds(&self,b:f64)->Result<f64,String>{Ok(b*60./self.bpm)}
 fn breakpoints(&self,_:f64,_:f64)->Result<Vec<f64>,String>{Ok(Vec::new())}
}
fn clock(s:&mut State,bpm:f64,revision:u64){s.install_warp_clock(std::sync::Arc::new(TempoClock{revision,bpm})).unwrap();}
fn wait_warp(s:&mut State){
 let start=std::time::Instant::now();loop{dispatch(s,"PumpWarp",&Value::Null).unwrap();let snap=dispatch(s,"Snapshot",&Value::Null).unwrap();if snap["warp"].as_array().unwrap().iter().all(|e|e["status"]=="ready"||e["status"]=="disabled"){break}assert!(start.elapsed().as_secs()<10,"{snap}");std::thread::sleep(std::time::Duration::from_millis(2));}
}
#[test]fn warp_uses_owned_shared_default_map_and_rejects_stale_edits_atomically(){
 let mut s=fixture();edit(&mut s,"SetClipWarp",json!({"id":"c","enabled":true}));assert_eq!(s.clips["c"].duration(),0.5);let rev=s.revision;
 assert!(dispatch(&mut s,"SetClipWarp",&json!({"document_revision":rev-1,"id":"c","enabled":false})).is_err());assert_eq!(s.revision,rev);
}

#[test]fn warp_identity_ready_pcm_matches_source_and_shared_snapshot(){
 let mut s=fixture();let before=render(&mut s);clock(&mut s,120.,1);edit(&mut s,"SetClipWarp",json!({"id":"c","enabled":true}));wait_warp(&mut s);
 assert_eq!(render(&mut s)["lsffr"],before["lsffr"]);let owned=dispatch(&mut s,"Snapshot",&Value::Null).unwrap();let shared=s.shared_snapshot();let mut combined=(*shared.document).clone();combined.as_object_mut().unwrap().extend(shared.live.as_object().unwrap().clone());assert_eq!(owned,combined);assert_eq!(owned["clips"][0]["warp_enabled"],true);
}
#[test]fn warp_clock_change_preserves_source_end_and_pitch(){
 let mut s=fixture();clock(&mut s,120.,1);edit(&mut s,"SetClipWarp",json!({"id":"c","enabled":true}));clock(&mut s,80.,2);assert!((s.clips["c"].duration()-0.75).abs()<1e-12);wait_warp(&mut s);
 let out=edit(&mut s,"RenderRange",json!({"sample_rate":48000,"start_frame":6000,"frames":4096}));let f=out["lsffr"].as_array().unwrap();let crossings=f.windows(2).filter(|x|x[0][2].as_f64().unwrap()<=0.&&x[1][2].as_f64().unwrap()>0.).count();assert!((crossings as i32-85).abs()<=2);
 edit(&mut s,"SetClipWarp",json!({"id":"c","enabled":false}));assert!((s.clips["c"].duration()-0.5).abs()<1e-12);
}
#[test]fn stale_warp_job_cannot_publish_after_source_replacement(){
 let mut s=fixture();clock(&mut s,120.,1);edit(&mut s,"SetClipWarp",json!({"id":"c","enabled":true}));clock(&mut s,80.,2);dispatch(&mut s,"PumpWarp",&Value::Null).unwrap();
 edit(&mut s,"PutSource",json!({"id":"s","sample_rate":48000,"channels":1,"samples":vec![0.;24000]}));wait_warp(&mut s);assert_eq!(energy(&render(&mut s)),0.);
}
#[test]fn warp_consumes_spectral_repair_and_preparing_export_is_rejected(){
 let mut s=fixture();edit(&mut s,"AddSpectralMask",mask());clock(&mut s,120.,1);edit(&mut s,"SetClipWarp",json!({"id":"c","enabled":true}));
 clock(&mut s,80.,2);assert!(super::warp_treatment::ready_for_export(&s).is_err());wait_warp(&mut s);assert!(super::warp_treatment::ready_for_export(&s).is_ok());let repaired=edit(&mut s,"RenderRange",json!({"sample_rate":48000,"start_frame":12000,"frames":4096}));assert!(energy(&repaired)<0.1);
 edit(&mut s,"ClearSpectralMasks",json!({"id":"c"}));assert!(super::warp_treatment::ready_for_export(&s).is_err());wait_warp(&mut s);assert!(energy(&render(&mut s))>100.);
}
#[test]fn invalid_warp_markers_leave_duration_and_revision_untouched(){
 let mut s=fixture();clock(&mut s,120.,1);edit(&mut s,"SetClipWarp",json!({"id":"c","enabled":true}));let rev=s.revision;let duration=s.clips["c"].duration();
 assert!(dispatch(&mut s,"SetWarpMap",&json!({"document_revision":rev,"id":"c","markers":[{"destination_beat":0.,"source_frame":24000.},{"destination_beat":1.,"source_frame":0.}]})).is_err());assert_eq!(s.revision,rev);assert_eq!(s.clips["c"].duration(),duration);
}
#[test]fn warp_source_truncation_is_atomic(){let mut s=fixture();clock(&mut s,120.,1);edit(&mut s,"SetClipWarp",json!({"id":"c","enabled":true}));let rev=s.revision;assert!(dispatch(&mut s,"PutSource",&json!({"document_revision":rev,"id":"s","sample_rate":48000,"channels":1,"samples":vec![0.;2000]})).is_err());assert_eq!(s.revision,rev);assert_eq!(s.sources["s"].samples.len(),24000);}
struct Rubato;
impl super::warp_treatment::Clock for Rubato{
 fn revision(&self)->u64{3}
 fn seconds_to_beat(&self,s:f64)->Result<f64,String>{Ok(if s<0.25{2.*s}else{0.5+s-0.25})}
 fn beat_to_seconds(&self,b:f64)->Result<f64,String>{Ok(if b<0.5{b*0.5}else{0.25+b-0.5})}
 fn breakpoints(&self,a:f64,b:f64)->Result<Vec<f64>,String>{Ok(if a<0.25&&b>0.25{vec![0.25]}else{vec![]})}
}
#[test]fn warp_initial_map_preserves_audio_across_clock_breakpoints(){let mut s=fixture();let original=render(&mut s);s.install_warp_clock(std::sync::Arc::new(Rubato)).unwrap();edit(&mut s,"SetClipWarp",json!({"id":"c","enabled":true}));assert!(super::warp_treatment::ready_for_export(&s).is_ok());assert_eq!(render(&mut s)["lsffr"],original["lsffr"]);clock(&mut s,120.,4);assert!((s.clips["c"].duration()-0.375).abs()<1e-12);wait_warp(&mut s);}
#[test]fn move_warp_across_clock_change_preserves_source_range(){let mut s=fixture();s.install_warp_clock(std::sync::Arc::new(Rubato)).unwrap();edit(&mut s,"SetClipWarp",json!({"id":"c","enabled":true}));let original=super::warp_treatment::source_span(&s,"c").unwrap();edit(&mut s,"MoveClip",json!({"id":"c","start_seconds":1.}));assert_eq!(super::warp_treatment::source_span(&s,"c").unwrap(),original);assert!((s.clips["c"].duration()-0.75).abs()<1e-12);}
#[test]fn marker_edit_retains_clip_timeline_length(){let mut s=fixture();clock(&mut s,120.,1);edit(&mut s,"SetClipWarp",json!({"id":"c","enabled":true}));edit(&mut s,"SetWarpMap",json!({"id":"c","markers":[{"destination_beat":0.,"source_frame":0.},{"destination_beat":2.,"source_frame":24000.}]}));assert_eq!(s.clips["c"].duration(),0.5);assert_eq!(super::warp_treatment::source_span(&s,"c").unwrap(),(0,12000));wait_warp(&mut s);}
#[test]fn accelerated_warp_endpoint_zero_pads_past_source(){let mut s=fixture();clock(&mut s,120.,1);edit(&mut s,"SetClipWarp",json!({"id":"c","enabled":true}));edit(&mut s,"SetWarpMap",json!({"id":"c","markers":[{"destination_beat":0.,"source_frame":0.},{"destination_beat":0.5,"source_frame":24000.}]}));wait_warp(&mut s);assert!(render(&mut s).is_object());let tail=edit(&mut s,"RenderRange",json!({"sample_rate":48000,"start_frame":20000,"frames":2048}));assert_eq!(energy(&tail),0.);}
#[test]fn compressed_editor_uses_original_source_display_interval(){let mut s=fixture();clock(&mut s,120.,1);edit(&mut s,"SetClipWarp",json!({"id":"c","enabled":true}));clock(&mut s,240.,2);let preview=edit(&mut s,"ClipEditorPreview",json!({"id":"c","columns":100}));assert_eq!(preview["frames"],12000);assert_eq!(preview["clip_frames"],12000);}
#[test]fn stretched_editor_allows_full_width_spectral_mask_past_pcm(){let mut s=fixture();clock(&mut s,120.,1);edit(&mut s,"SetClipWarp",json!({"id":"c","enabled":true}));clock(&mut s,80.,2);let mut m=mask();m["mask"]["source_start_frame"]=json!(0);m["mask"]["source_end_frame"]=json!(36000);edit(&mut s,"AddSpectralMask",m);let preview=edit(&mut s,"ClipEditorPreview",json!({"id":"c","columns":100}));assert_eq!(preview["frames"],24000);assert_eq!(preview["clip_frames"],36000);wait_warp(&mut s);}
#[test]fn disabling_fast_warp_preserves_duration_and_silent_tail(){let mut s=fixture();clock(&mut s,120.,1);edit(&mut s,"SetClipWarp",json!({"id":"c","enabled":true}));edit(&mut s,"SetWarpMap",json!({"id":"c","markers":[{"destination_beat":0.,"source_frame":0.},{"destination_beat":0.5,"source_frame":24000.}]}));edit(&mut s,"SetClipWarp",json!({"id":"c","enabled":false}));assert_eq!(s.clips["c"].duration(),1.);let tail=edit(&mut s,"RenderRange",json!({"sample_rate":48000,"start_frame":30000,"frames":2048}));assert_eq!(energy(&tail),0.);assert!(energy(&render(&mut s))>100.);}
#[test]fn mixer_observation_uses_actual_post_route_pcm_without_extra_render(){let mut s=fixture();edit(&mut s,"SetTrackMix",json!({"id":"a","gain":0.5,"output_bus_l":"L","output_bus_r":"R"}));let o=edit(&mut s,"RenderRange",json!({"sample_rate":48000,"start_frame":0,"frames":4096,"publish_visualization":true,"master_gain":0.5}));let v=&o["visualization"];assert!((v["tracks"][0]["meter_peak"].as_f64().unwrap()-0.25).abs()<1e-6);assert!((v["master_meter_peaks"][0].as_f64().unwrap()-0.25).abs()<1e-6);assert!((v["monitor_meter_peaks"][0].as_f64().unwrap()-0.125).abs()<1e-6);assert_eq!(v["master_meter_peaks"][1],0.);}

#[test]
fn clip_gain_changes_pcm_and_survives_waveform_trim() {
    let mut s=fixture();
    let original=render(&mut s);
    edit(&mut s,"SetClipGain",json!({"id":"c","gain":0.5}));
    assert!((energy(&render(&mut s))/energy(&original)-0.25).abs()<1e-6);
    edit(&mut s,"TrimAudioClip",json!({"id":"c","start_fraction":0.25,"end_fraction":0.75}));
    let clip=&s.clips["c"];
    assert_eq!(clip.start,0.125);
    assert_eq!(clip.duration(),0.25);
    assert_eq!(clip.gain,0.5);
    assert!((energy(&render(&mut s))/energy(&original)-0.25).abs()<1e-6);
    let revision=s.revision;
    assert!(dispatch(&mut s,"SetClipGain",&json!({"document_revision":revision,"id":"c","gain":17.})).is_err());
    assert_eq!(s.revision,revision);
    assert_eq!(s.clips["c"].gain,0.5);
}
