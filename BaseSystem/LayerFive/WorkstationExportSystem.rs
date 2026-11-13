//! Layer-five export workflow. Owns its Audicle, native dialog and bounded
//! advancement; data/render/file operations stay in WorkstationDataSystem.
use crate::{host::Host, workstation::export_dialog};
use serde_json::{Value, json};

#[derive(Default)]
pub(crate) struct State {
    event: Option<Value>,
    job: Option<Value>,
    dialog: bool,
    held: bool,
}
impl State {
    pub(crate) fn blocks_timeline_commands(&self)->bool {self.dialog || self.job.is_some() || self.event.is_some()}
    fn terminal(&mut self, host: &mut Host, report: &Value) -> Result<(), String> {
        let Some(event) = self.event.clone() else {
            return Ok(());
        };
        let hook = match report["phase"].as_str() {
            Some("completed") => "AudicleSystem.Finish",
            Some("cancelled") => "AudicleSystem.Cancel",
            Some("failed") => "AudicleSystem.Fail",
            _ => return Ok(()),
        };
        let mut event = event;
        event["reason"] = report["error"].clone();
        host.invoke(hook, event)?;
        self.event = None;
        self.job = None;
        Ok(())
    }
    fn begin(&mut self, host: &mut Host, input: &Value) -> Result<Value, String> {
        if self.event.is_some() {
            return Err("an export is already running".into());
        }
        let event = host.invoke("AudicleSystem.Create", Value::Null)?;
        self.event = Some(event.clone());
        let result: Result<Value, String> = (|| {
            let report = host.invoke("WorkstationDataSystem.ExportBegin", input.clone())?;
            self.job = Some(json!({"job_id":report["job_id"]}));
            host.invoke("AudicleSystem.Ready", event.clone())?;
            host.invoke("AudicleSystem.Go", event.clone())?;
            Ok(report)
        })();
        if let Err(error) = &result {
            if let Some(job) = &self.job {
                host.invoke("WorkstationDataSystem.ExportCancel", job.clone())?;
            }
            self.terminal(host, &json!({"phase":"failed","error":error}))?;
        }
        result
    }
    fn step(&mut self, host: &mut Host) -> Result<Value, String> {
        if let Some(job) = self.job.clone() {
            if let Some(report) = self.retire_stale_job(host, &job)? {
                return Ok(report);
            }
            if let Some(event) = &self.event {
                host.invoke("AudicleSystem.Resume", event.clone())?;
            }
            let report = host.invoke("WorkstationDataSystem.ExportStep", job)?;
            self.terminal(host, &report)?;
            Ok(report)
        } else {
            host.invoke("WorkstationDataSystem.ExportStatus", Value::Null)
        }
    }
    fn cancel(&mut self, host: &mut Host) -> Result<Value, String> {
        let report = if let Some(job) = self.job.clone() {
            if let Some(report) = self.retire_stale_job(host, &job)? {
                return Ok(report);
            }
            host.invoke("WorkstationDataSystem.ExportCancel", job)?
        } else {
            host.invoke("WorkstationDataSystem.ExportStatus", Value::Null)?
        };
        self.terminal(host, &report)?;
        Ok(report)
    }
    fn retire_stale_job(&mut self, host: &mut Host, job: &Value) -> Result<Option<Value>, String> {
        let current = host.invoke("WorkstationDataSystem.ExportStatus", Value::Null)?;
        if current["job_id"] == job["job_id"] {
            return Ok(None);
        }
        // A typed data client may have cancelled/replaced our job. Retire only
        // this workflow's event; its stale token cannot touch the replacement.
        let report = json!({"job_id":job["job_id"],"phase":"failed","progress":0.,
            "error":"Export job was replaced by another request"});
        self.terminal(host, &report)?;
        Ok(Some(report))
    }
    fn open(&mut self, host: &mut Host) -> Result<Value, String> {
        let snapshot = host.invoke("WorkstationDataSystem.Snapshot", Value::Null)?;
        let end = snapshot["clips"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|c| {
                c["start_seconds"].as_f64().unwrap_or(0.)
                    + c["duration_seconds"].as_f64().unwrap_or(0.)
            })
            .fold(0f64, f64::max);
        let rate = snapshot["sources"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|s| s["sample_rate"].as_u64())
            .unwrap_or(48000);
        export_dialog::open(&json!({"start_seconds":0.,"end_seconds":end,"sample_rate":rate}))?;
        self.dialog = true;
        Ok(json!({"dialog_open":true}))
    }
    fn should_open(&mut self, input: &Value) -> bool {
        let pressed = input["open"] == true;
        let open = input["open_pressed"] == true || (pressed && !self.held);
        self.held = pressed;
        open
    }
    fn pump(&mut self, host: &mut Host, input: &Value) -> Result<Value, String> {
        if self.should_open(input) {
            self.open(host)?;
        }
        if self.dialog {
            let action = export_dialog::poll()?;
            match action["action"].as_str() {
                Some("start") => {
                    let mut request = action;
                    request["document_revision"] = host
                        .invoke("WorkstationDataSystem.Snapshot", Value::Null)?["revision"]
                        .clone();
                    match self.begin(host, &request) {
                        Ok(report) => export_dialog::update(&report)?,
                        Err(error) => {
                            let report = json!({"phase":"failed","progress":0.,"error":error});
                            export_dialog::update(&report)?;
                            return Ok(report);
                        }
                    }
                }
                Some("cancel") => {
                    if self.job.is_some() {
                        let report = self.cancel(host)?;
                        export_dialog::update(&report)?;
                    }
                }
                _ => {}
            }
        }
        let advanced = self.job.is_some();
        let report = self.step(host)?;
        // Only update while running or at its terminal transition. A validation
        // error shown above must not be replaced by an old/idle export status.
        if self.dialog && advanced {
            export_dialog::update(&report)?;
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workstation_export_accepts_adjacent_native_presses_and_legacy_held_input() {
        let mut state = State::default();
        assert!(state.should_open(&json!({"open_pressed":true})));
        assert!(state.should_open(&json!({"open_pressed":true})));
        assert!(!state.should_open(&json!({"open_pressed":false})));
        assert!(state.should_open(&json!({"open":true})));
        assert!(!state.should_open(&json!({"open":true})));
        assert!(!state.should_open(&json!({"open":false})));
        assert!(state.should_open(&json!({"open":true})));
    }
}
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    let parent = host.active_audicle;
    let mut state = std::mem::take(&mut host.workstation_export);
    let result = match hook {
        "Open" => state.open(host),
        "Begin" => state.begin(host, input),
        "Step" => state.step(host),
        "Pump" => state.pump(host, input),
        "Status" => host.invoke("WorkstationDataSystem.ExportStatus", Value::Null),
        "Cancel" => state.cancel(host),
        "Close" => {
            let owned_job = state.job.is_some();
            state.cancel(host).and_then(|report| {
                if state.dialog {
                    if owned_job {
                        export_dialog::update(&report)?;
                    }
                    export_dialog::close()?;
                }
                state.dialog = false;
                state.held = false;
                Ok(report)
            })
        }
        _ => Err("unknown workstation export hook".into()),
    };
    host.active_audicle = parent.filter(|id| host.audicles.live(*id));
    host.workstation_export = state;
    result
}

#[cfg(test)] mod timeline_command_guard_tests {
 use super::*;
 #[test]fn dialog_and_active_job_block_until_ownership_released(){
  let mut s=State::default();assert!(!s.blocks_timeline_commands());s.dialog=true;assert!(s.blocks_timeline_commands());
  s.dialog=false;s.job=Some(json!({"job_id":1}));assert!(s.blocks_timeline_commands());s.job=None;s.event=Some(json!({}));assert!(s.blocks_timeline_commands());
  s.event=None;assert!(!s.blocks_timeline_commands());
 }
}
