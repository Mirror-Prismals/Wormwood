//! Non-real-time audio preparation. The native callback never enters this
//! module: it consumes the existing fixed SPSC queues without taking a lock.
use super::State;
use crate::host::AudioPreparationContext;
use std::{
    sync::{Arc, Mutex, mpsc, atomic::{AtomicBool, Ordering}},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(super) struct Worker {
    pub(super) state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    // Return the context to the joining control thread. Its final document
    // reference can own native plugin editors, which must die on that thread.
    thread: Option<JoinHandle<AudioPreparationContext>>,
}
impl Worker {
    pub(super) fn start(mut state: State, mut context: AudioPreparationContext) -> Result<Self, (State, String)> {
        state.preparation_active = true;
        let shared = Arc::new(Mutex::new(state));
        let stop = Arc::new(AtomicBool::new(false));
        let inner = shared.clone();
        let stopping = stop.clone();
        let (ready_tx,ready_rx)=mpsc::sync_channel(1);
        let spawned = thread::Builder::new().name("mirror-audio-prepare".into()).spawn(move || {
            let qos=crate::audio::worker_scheduling::request();
            {
                let mut state=inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                state.preparation_qos=Some(qos);
                state.preparation_ready=true;
            }
            // Acknowledge only after startup is complete and release the state
            // guard before the control thread can activate native callbacks.
            let _=ready_tx.send(());
            while !stopping.load(Ordering::Acquire) {
                {
                    let mut state = inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    if stopping.load(Ordering::Acquire) { break; }
                    if state.playback_error.is_none() {
                        let start = Instant::now();
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| context.with_document(|document|state.pump(document, false))));
                        state.preparation_ticks = state.preparation_ticks.saturating_add(1);
                        state.preparation_max_us = state.preparation_max_us.max(start.elapsed().as_micros().min(u64::MAX as u128) as u64);
                        let failure = match result {
                            Err(_) => Some("audio preparation worker panicked; playback stopped".to_owned()),
                            Ok(Err(error)) => Some(error),
                            Ok(Ok(())) => None,
                        };
                        if let Some(error) = failure {
                            if let Some(transport) = state.transport.as_mut() {
                                transport.set_playing(false);
                                transport.set_recording(false);
                            }
                            state.recording = false;
                            state.playback_error = Some(error);
                        }
                    }
                }
                // Polling happens only on this allocating preparation thread.
                // No callback syscall, notification, mutex or allocation is added.
                thread::park_timeout(Duration::from_millis(1));
            }
            context
        });
        match spawned {
            Ok(thread) => {
                let mut worker=Self { state: shared, stop, thread: Some(thread) };
                // No worker or document guard is held while waiting. On failure
                // join before returning resource ownership to the caller.
                match ready_rx.recv_timeout(Duration::from_secs(5)) {
                    Ok(()) => Ok(worker),
                    Err(error) => { let state=worker.take_state();Err((state,format!("audio preparation worker did not become ready: {error}"))) }
                }
            },
            Err(error) => {
                let mut state = Arc::try_unwrap(shared).ok().expect("failed worker has no second owner")
                    .into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
                state.preparation_active = false;
                Err((state, format!("cannot start audio preparation worker: {error}")))
            }
        }
    }
    pub(super) fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let result = thread.join();
            let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            state.preparation_active = false;
            state.preparation_ready = false;
            if result.is_err() {
                state.playback_error = Some("audio preparation worker terminated unexpectedly".into());
            }
            // A successful result drops the context here, on the control thread.
        }
    }
    pub(super) fn take_state(&mut self) -> State {
        self.stop();
        std::mem::take(&mut *self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }
}
impl Drop for Worker { fn drop(&mut self) { self.stop(); } }
