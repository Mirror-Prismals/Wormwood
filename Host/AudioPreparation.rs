// Restricted named-call context for the preparation thread, not the callback.
// A bounded journal is flushed by the application; no worker file I/O.
#[derive(Default)]
struct PreparationJournal {
    events: std::collections::VecDeque<Value>,
    omitted: u64,
}
pub(crate) struct AudioPreparationContext {
    document: crate::workstation::Owner,
    hooks: BTreeMap<String, Hook>,
    journal: std::sync::Arc<std::sync::Mutex<PreparationJournal>>,
    next: u64,
    parent: Option<u64>,
    audicle: Option<u64>,
    identity: u64,
}
pub(crate) struct AudioPreparationTransaction<'a> {
    context: &'a mut AudioPreparationContext,
    document: &'a mut crate::workstation::State,
}
impl AudioPreparationContext {
    fn event(&self, event: Value) {
        if let Ok(mut journal) = self.journal.lock() {
            if journal.events.len() == 512 {
                journal.events.pop_front();
                journal.omitted += 1;
            }
            journal.events.push_back(event);
        }
    }
    pub(crate) fn invoke(&mut self, target: &str, input: Value) -> Result<Value, String> {
        self.with_document(|transaction| transaction.invoke(target, input))
    }
    pub(crate) fn with_document<R>(
        &mut self,
        run: impl FnOnce(&mut AudioPreparationTransaction<'_>) -> Result<R, String>,
    ) -> Result<R, String> {
        let owner = self.document.clone();
        owner.with(|document| {
            run(&mut AudioPreparationTransaction {
                context: self,
                document,
            })
        })
    }
}
impl AudioPreparationTransaction<'_> {
    /// Typed projection of the registered Snapshot operation. Audio preparation
    /// needs only its revision, not a cloned UI document and plugin catalog.
    pub(crate) fn document_revision(&mut self) -> Result<u64, String> {
        let target = "WorkstationDataSystem.Snapshot";
        let hook = self
            .context
            .hooks
            .get(target)
            .ok_or("audio preparation Snapshot is not authorized")?;
        self.context.next += 1;
        let invocation = format!(
            "preparation-{}-{}",
            self.context.identity, self.context.next
        );
        let start = std::time::Instant::now();
        self.context.event(json!({"event":"started","thread":"audio_preparation","function":target,"invocation":invocation,"parent":self.context.parent,"audicle":self.context.audicle,"caller":"EnvironmentAudioSystem.Pump","input_type":hook.input,"projection":"document_revision"}));
        let revision = self.document.document_revision();
        self.context.event(json!({"event":"finished","thread":"audio_preparation","function":target,"invocation":invocation,"parent":self.context.parent,"audicle":self.context.audicle,"elapsed_us":start.elapsed().as_micros(),"output_type":hook.output,"projection":"document_revision"}));
        Ok(revision)
    }
    pub(crate) fn invoke(&mut self, target: &str, input: Value) -> Result<Value, String> {
        let hook = self
            .context
            .hooks
            .get(target)
            .ok_or("audio preparation hook is not authorized")?;
        if !check_payload(&hook.input, &input) {
            return Err(format!("invalid preparation input for {target}"));
        }
        self.context.next += 1;
        let invocation = format!(
            "preparation-{}-{}",
            self.context.identity, self.context.next
        );
        let start = std::time::Instant::now();
        self.context.event(json!({"event":"started","thread":"audio_preparation","function":target,"invocation":invocation,"parent":self.context.parent,"audicle":self.context.audicle,"caller":"EnvironmentAudioSystem.Pump","input_type":hook.input}));
        let result =
            crate::workstation::dispatch(self.document, target.rsplit('.').next().unwrap(), &input)
                .and_then(|v| {
                    if check_payload(&hook.output, &v) {
                        Ok(v)
                    } else {
                        Err(format!("invalid preparation output for {target}"))
                    }
                });
        self.context.event(json!({"event":if result.is_ok(){"finished"}else{"failed"},"thread":"audio_preparation","function":target,"invocation":invocation,"parent":self.context.parent,"audicle":self.context.audicle,"elapsed_us":start.elapsed().as_micros(),"output_type":hook.output,"error":result.as_ref().err()}));
        result
    }
}
impl Host {
    pub(crate) fn audio_preparation_context(&mut self) -> Result<AudioPreparationContext, String> {
        if self.halt {
            return Err("execution halted by circuit breaker".into());
        }
        if !self.system_enabled("EnvironmentAudioSystem") {
            return Err("audio preparation System is disabled".into());
        }
        let caller = self
            .hooks
            .get("EnvironmentAudioSystem.Pump")
            .ok_or("audio preparation contract missing")?;
        let mut hooks = BTreeMap::new();
        for target in [
            "WorkstationDataSystem.Snapshot",
            "WorkstationDataSystem.CaptureInput",
            "WorkstationDataSystem.RenderRange",
        ] {
            let hook = self
                .hooks
                .get(target)
                .ok_or("audio preparation target missing")?;
            if !caller.calls.contains(target) || !self.system_enabled(&hook.system) {
                return Err(format!("audio preparation cannot invoke {target}"));
            }
            hooks.insert(target.to_string(), hook.clone());
        }
        let journal = std::sync::Arc::new(std::sync::Mutex::new(PreparationJournal::default()));
        self.flush_audio_preparation_events()?;
        self.audio_preparation_journal = Some(journal.clone());
        let identity = self.next_id;
        self.next_id += 1;
        Ok(AudioPreparationContext {
            document: self.workstation.clone(),
            hooks,
            journal,
            next: 0,
            parent: self.stack.last().map(|p| p.0),
            audicle: self.active_audicle,
            identity,
        })
    }
    pub(crate) fn flush_audio_preparation_events(&self) -> Result<(), String> {
        let Some(journal) = &self.audio_preparation_journal else {
            return Ok(());
        };
        let (events, omitted) = {
            let mut j = journal.lock().map_err(|_| "preparation journal poisoned")?;
            (
                std::mem::take(&mut j.events),
                std::mem::take(&mut j.omitted),
            )
        };
        if omitted > 0 {
            self.record(
                json!({"event":"preparation_trace_omitted","count":omitted,"capacity":512}),
            )?;
        }
        for event in events {
            self.record(event)?;
        }
        Ok(())
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.environment_audio.stop_preparation_worker();
        let _ = self.flush_audio_preparation_events();
    }
}

#[cfg(test)]
mod audio_preparation_context_tests {
    use super::*;
    fn host(name: &str) -> Host {
        let output = std::env::temp_dir().join(format!(
            "wormwood-preparation-{name}-{}",
            std::process::id()
        ));
        Host::load_paths(Path::new(env!("CARGO_MANIFEST_DIR")), &output).unwrap()
    }
    #[test]
    fn preparation_context_rejects_control_hooks_and_preserves_named_observations() {
        let mut host = host("routes");
        let mut context = host.audio_preparation_context().unwrap();
        assert!(
            context
                .invoke("PluginConnectionSystem.Command", json!({"op":"scan"}))
                .is_err()
        );
        assert!(
            context
                .invoke("WorkstationDataSystem.Edit", json!({}))
                .is_err()
        );
        context
            .with_document(|document| {
                let snapshot = document.invoke("WorkstationDataSystem.Snapshot", Value::Null)?;
                assert!(snapshot["tracks"].is_array());
                Ok(())
            })
            .unwrap();
        host.flush_audio_preparation_events().unwrap();
        host.trace_writer
            .borrow_mut()
            .as_mut()
            .unwrap()
            .1
            .flush()
            .unwrap();
        let rows: Vec<Value> = fs::read_to_string(&host.trace)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert!(
            rows.iter()
                .any(|r| r["function"] == "WorkstationDataSystem.Snapshot"
                    && r["thread"] == "audio_preparation"
                    && r["event"] == "finished")
        );
        drop(context);
        let output = host.output.clone();
        drop(host);
        let _ = fs::remove_dir_all(output);
    }
    #[test]
    fn preparation_observations_have_bounded_retention_and_unique_session_ids() {
        let mut host = host("bounded");
        let mut context = host.audio_preparation_context().unwrap();
        let old_identity = context.identity;
        for _ in 0..600 {
            context
                .invoke("WorkstationDataSystem.Snapshot", Value::Null)
                .unwrap();
        }
        {
            let journal = context.journal.lock().unwrap();
            assert_eq!(journal.events.len(), 512);
            assert_eq!(journal.omitted, 688);
        }
        drop(context);
        let context = host.audio_preparation_context().unwrap();
        assert_ne!(old_identity, context.identity);
        drop(context);
        let output = host.output.clone();
        drop(host);
        let _ = fs::remove_dir_all(output);
    }
    #[test]
    fn diagnostic_rotation_preserves_previous_segment_and_continues_writing() {
        let host = host("rotation");
        host.record(json!({"event":"before_rotation"})).unwrap();
        // Exercise the threshold without writing a 64 MiB fixture.
        host.trace_writer.borrow_mut().as_mut().unwrap().1.force_rotation_for_test().unwrap();
        host.record(json!({"event":"window_pumped"})).unwrap();
        let previous = fs::read_to_string(host.trace.with_extension("previous.jsonl")).unwrap();
        assert!(previous.contains("before_rotation"));
        let current = fs::read_to_string(&host.trace).unwrap();
        assert!(current.contains("window_pumped"));
        assert!(!current.contains("before_rotation"));
        let output = host.output.clone();
        drop(host);
        let _ = fs::remove_dir_all(output);
    }
}
