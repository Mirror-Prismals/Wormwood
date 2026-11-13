// Included inside Host's existing tests module to exercise real contract routing.
fn export_fixture(h: &mut Host) {
    for (hook, mut input) in [
        (
            "PutTrack",
            json!({"id":"audio","name":"Audio","kind":"audio"}),
        ),
        (
            "PutSource",
            json!({"id":"source","sample_rate":8000,"channels":1,"samples":[2.,-3.]}),
        ),
        (
            "PutClip",
            json!({"id":"clip","track":"audio","kind":"audio","source":"source",
            "start_seconds":0.,"source_offset_seconds":0.,"duration_seconds":0.00025}),
        ),
        (
            "SetTrackMix",
            json!({"id":"audio","output_bus_l":"L","output_bus_r":"FF"}),
        ),
    ] {
        input["document_revision"] = h
            .invoke("WorkstationDataSystem.Snapshot", Value::Null)
            .unwrap()["revision"]
            .clone();
        h.invoke(&format!("WorkstationDataSystem.{hook}"), input)
            .unwrap();
    }
}
#[test]
fn workstation_export_workflow_routes_snapshot_and_finishes_its_audicle() {
    let mut h = traced_host();
    export_fixture(&mut h);
    let destination = std::env::temp_dir().join(format!("mirror-export-host-{}", h.run_id));
    let revision = h
        .invoke("WorkstationDataSystem.Snapshot", Value::Null)
        .unwrap()["revision"]
        .clone();
    let report = h
        .invoke(
            "WorkstationExportSystem.Begin",
            json!({"document_revision":revision,
        "sample_rate":8000,"start_seconds":0.,"end_seconds":0.00025,"destination":destination}),
        )
        .unwrap();
    assert_eq!(report["phase"], "rendering");
    assert_eq!(h.audicles.count(), 1);
    assert!(h.active_audicle.is_none());
    assert!(
        h.set_system_enabled("WorkstationDataSystem", false)
            .is_err()
    );
    assert!(
        h.set_system_enabled("WorkstationExportSystem", false)
            .is_err()
    );
    let done = h
        .invoke("WorkstationExportSystem.Step", Value::Null)
        .unwrap();
    assert_eq!(done["phase"], "completed");
    assert_eq!(h.audicles.count(), 0);
    assert!(h.active_audicle.is_none());
    let b = fs::read(done["files"][0].as_str().unwrap()).unwrap();
    assert_eq!(&b[..4], b"RIFF");
    assert_eq!(
        f32::from_le_bytes(b[b.len() - 8..b.len() - 4].try_into().unwrap()),
        2.
    );
    assert_eq!(
        f32::from_le_bytes(b[b.len() - 4..].try_into().unwrap()),
        -3.
    );
    h.invoke("WorkstationExportSystem.Close", Value::Null)
        .unwrap();
    assert_eq!(fs::read_dir(&destination).unwrap().count(), 4);
    h.trace_writer.borrow_mut().take();
    let trace = fs::read_to_string(&h.trace).unwrap();
    assert!(
        trace
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .any(|r| r["event"] == "audicle_stage"
                && r["stage"] == "Finished"
                && r["owner"] == "WorkstationExportSystem")
    );
    fs::remove_dir_all(destination).unwrap();
    fs::remove_file(&h.trace).unwrap();
}
#[test]
fn workstation_export_cancel_and_failed_preparation_have_distinct_outcomes() {
    let mut h = traced_host();
    export_fixture(&mut h);
    let destination = std::env::temp_dir().join(format!("mirror-export-cancel-{}", h.run_id));
    let revision = h
        .invoke("WorkstationDataSystem.Snapshot", Value::Null)
        .unwrap()["revision"]
        .clone();
    let mut request = json!({"document_revision":revision,"sample_rate":8000,"start_seconds":0.,
        "end_seconds":0.00025,"destination":destination});
    h.invoke("WorkstationExportSystem.Begin", request.clone())
        .unwrap();
    let report = h
        .invoke("WorkstationExportSystem.Cancel", Value::Null)
        .unwrap();
    assert_eq!(report["phase"], "cancelled");
    assert!(!destination.exists());
    assert_eq!(h.audicles.count(), 0);
    request["sample_rate"] = json!(16000);
    assert!(
        h.invoke("WorkstationExportSystem.Begin", request)
            .unwrap_err()
            .contains("sample rate")
    );
    assert_eq!(h.audicles.count(), 0);
    assert!(!destination.exists());
    h.trace_writer.borrow_mut().take();
    let rows: Vec<Value> = fs::read_to_string(&h.trace)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(
        rows.iter()
            .any(|r| r["event"] == "audicle_cancelled" && r["owner"] == "WorkstationExportSystem")
    );
    assert!(
        rows.iter()
            .any(|r| r["event"] == "audicle_failed" && r["owner"] == "WorkstationExportSystem")
    );
    fs::remove_file(&h.trace).unwrap();
}

fn exercise_stale_export_with_live_parent(operation: &str) {
    let mut h = traced_host();
    export_fixture(&mut h);
    // Establish a real layer-five parent through its declared Audicle calls;
    // no graphics device is needed to exercise ownership/context restoration.
    h.stack.push((9100, "AudioWorkstationSystem.Begin".into()));
    let parent = h.invoke("AudicleSystem.Create", Value::Null).unwrap();
    h.invoke("AudicleSystem.Ready", parent.clone()).unwrap();
    h.invoke("AudicleSystem.Go", parent.clone()).unwrap();
    h.stack.pop();
    let parent_id = parent["audicle"].as_u64().unwrap();
    let assert_parent = |h: &Host, count: usize| {
        assert_eq!(h.active_audicle, Some(parent_id));
        assert!(h.audicles.is_go(Some(parent_id)));
        assert!(h.audicles.owned_by("AudioWorkstationSystem"));
        assert_eq!(h.audicles.count(), count);
    };
    assert_parent(&h, 1);

    let destination = |suffix: &str| {
        std::env::temp_dir().join(format!(
            "mirror-export-stale-{operation}-{suffix}-{}",
            h.run_id
        ))
    };
    let a_path = destination("a");
    let b_path = destination("b");
    let c_path = destination("c");
    let revision = h
        .invoke("WorkstationDataSystem.Snapshot", Value::Null)
        .unwrap()["revision"]
        .clone();
    let request = |path: &std::path::Path| {
        json!({"document_revision":revision,"sample_rate":8000,
            "start_seconds":0.,"end_seconds":0.00025,"destination":path})
    };

    let a = h
        .invoke("WorkstationExportSystem.Begin", request(&a_path))
        .unwrap();
    assert_parent(&h, 2);
    assert!(h.audicles.owned_by("WorkstationExportSystem"));
    let cancelled_a = h
        .invoke(
            "WorkstationDataSystem.ExportCancel",
            json!({"job_id":a["job_id"]}),
        )
        .unwrap();
    assert_eq!(cancelled_a["phase"], "cancelled");
    assert!(!a_path.exists());
    // The data operation has ended A, but the workflow still owns A's event.
    assert_parent(&h, 2);
    let b = h
        .invoke("WorkstationDataSystem.ExportBegin", request(&b_path))
        .unwrap();
    assert_ne!(a["job_id"], b["job_id"]);
    assert_eq!(b["phase"], "rendering");
    assert_parent(&h, 2);

    let failed = h
        .invoke(&format!("WorkstationExportSystem.{operation}"), Value::Null)
        .unwrap();
    assert_eq!(failed["phase"], "failed");
    assert_parent(&h, 1);
    assert!(!h.audicles.owned_by("WorkstationExportSystem"));
    assert_eq!(
        h.invoke("WorkstationDataSystem.ExportStatus", Value::Null)
            .unwrap(),
        b,
        "retiring stale A must neither advance nor cancel direct job B"
    );
    assert_eq!(fs::read_dir(&b_path).unwrap().count(), 4);
    h.invoke("WorkstationExportSystem.Close", Value::Null)
        .unwrap();
    assert_parent(&h, 1);
    assert_eq!(
        h.invoke("WorkstationDataSystem.ExportStatus", Value::Null)
            .unwrap(),
        b
    );

    // Starting a workflow while B is active may create a preparation event,
    // but its rejection must terminate that event without touching B or parent.
    assert!(
        h.invoke("WorkstationExportSystem.Begin", request(&c_path))
            .is_err()
    );
    assert!(!c_path.exists());
    assert_parent(&h, 1);
    assert!(!h.audicles.owned_by("WorkstationExportSystem"));
    assert_eq!(
        h.invoke("WorkstationDataSystem.ExportStatus", Value::Null)
            .unwrap(),
        b
    );
    let cancelled_b = h
        .invoke(
            "WorkstationDataSystem.ExportCancel",
            json!({"job_id":b["job_id"]}),
        )
        .unwrap();
    assert_eq!(cancelled_b["phase"], "cancelled");
    assert!(!b_path.exists());
    assert_parent(&h, 1);

    let c = h
        .invoke("WorkstationExportSystem.Begin", request(&c_path))
        .unwrap();
    assert_ne!(c["job_id"], b["job_id"]);
    assert_parent(&h, 2);
    let complete = h
        .invoke("WorkstationExportSystem.Step", Value::Null)
        .unwrap();
    assert_eq!(complete["job_id"], c["job_id"]);
    assert_eq!(complete["phase"], "completed");
    assert_eq!(complete["files"].as_array().unwrap().len(), 4);
    assert_parent(&h, 1);
    assert!(!h.audicles.owned_by("WorkstationExportSystem"));
    h.invoke("WorkstationExportSystem.Close", Value::Null)
        .unwrap();
    assert_parent(&h, 1);
    assert_eq!(fs::read_dir(&c_path).unwrap().count(), 4);

    h.stack.push((9101, "AudioWorkstationSystem.Close".into()));
    h.invoke("AudicleSystem.Cancel", parent).unwrap();
    h.stack.pop();
    assert!(h.active_audicle.is_none());
    assert_eq!(h.audicles.count(), 0);
    h.trace_writer.borrow_mut().take();
    let rows: Vec<Value> = fs::read_to_string(&h.trace)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let workflow: Vec<&Value> = rows
        .iter()
        .filter(|row| row["owner"] == "WorkstationExportSystem")
        .collect();
    assert_eq!(
        workflow
            .iter()
            .filter(|row| row["event"] == "audicle_failed" && row["previous_stage"] == "Go")
            .count(),
        1,
        "stale A receives exactly one failed terminal outcome"
    );
    assert_eq!(
        workflow
            .iter()
            .filter(|row| row["event"] == "audicle_stage" && row["stage"] == "Finished")
            .count(),
        1,
        "only C completes the workflow"
    );
    assert!(
        !workflow
            .iter()
            .any(|row| row["event"] == "audicle_cancelled")
    );
    let preparations: Vec<&Value> = workflow
        .iter()
        .copied()
        .filter(|row| row["event"] == "audicle_stage" && row["stage"] == "Preparing")
        .collect();
    assert!(!preparations.is_empty());
    assert!(
        preparations
            .iter()
            .all(|row| row["parent_audicle"] == parent_id)
    );
    fs::remove_dir_all(c_path).unwrap();
    fs::remove_file(&h.trace).unwrap();
}

#[test]
fn workstation_export_stale_step_preserves_new_job_and_live_parent() {
    exercise_stale_export_with_live_parent("Step");
}

#[test]
fn workstation_export_stale_cancel_preserves_new_job_and_live_parent() {
    exercise_stale_export_with_live_parent("Cancel");
}

#[test]
fn workstation_export_stale_close_preserves_new_job_and_live_parent() {
    exercise_stale_export_with_live_parent("Close");
}

#[test]
fn workstation_clip_fades_route_through_host_and_audio_workstation_contract(){
 let mut h=traced_host();export_fixture(&mut h);
 let contract:Value=serde_json::from_slice(&fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("Systems/AudioWorkstationSystem.json")).unwrap()).unwrap();
 assert!(contract["functions"]["Step"]["calls"].as_array().unwrap().iter().any(|x|x=="WorkstationDataSystem.SetClipFades"));
 for request in [json!({"id":"clip","fade_in_seconds":0.000125,"fade_out_seconds":0.000125}),json!({"action":"undo"}),json!({"action":"redo"})]{
  let mut request=request;request["document_revision"]=h.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap()["revision"].clone();h.invoke("WorkstationDataSystem.SetClipFades",request).unwrap();
 }
 let snapshot=h.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap();assert_eq!(snapshot["clips"][0]["fade_in_seconds"],0.000125);
}

#[test]
fn track_freeze_registered_routes_finish_cancel_accept_and_flatten(){
 let mut h=traced_host();export_fixture(&mut h);
 let begin=|h:&mut Host|{let r=h.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap()["revision"].clone();h.invoke("WorkstationDataSystem.TrackFreezeBegin",json!({"id":"audio","sample_rate":8000,"document_revision":r})).unwrap()};
 let job=begin(&mut h);let token=json!({"job_id":job["job_id"]});
 assert_eq!(h.invoke("WorkstationDataSystem.TrackFreezeStep",token.clone()).unwrap()["phase"],"finished");
 h.invoke("WorkstationDataSystem.TrackFreezeCancel",token).unwrap();
 assert_eq!(h.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap()["clips"][0]["id"],"clip");
 let job=begin(&mut h);let token=json!({"job_id":job["job_id"]});h.invoke("WorkstationDataSystem.TrackFreezeStep",token.clone()).unwrap();h.invoke("WorkstationDataSystem.TrackFreezeAccept",token).unwrap();
 let s=h.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap();assert_eq!(s["tracks"][0]["frozen"],true);
 h.invoke("WorkstationDataSystem.TrackFreezeFlatten",json!({"id":"audio","document_revision":s["revision"]})).unwrap();
 assert_eq!(h.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap()["tracks"][0]["frozen"],false);
}
