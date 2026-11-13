//! Persistent CPU terrain preparation. Exactly one submitted job is joined in
//! the same View; no stale camera, dropped scene, quality or geometry change.
use serde_json::Value;
use std::sync::{Arc, mpsc};
struct Input {
    view: Value,
    surface: Value,
    material: Option<Arc<Vec<[f64; 6]>>>,
    recycle: Vec<[f32; 24]>,
}
struct Worker {
    input: Option<mpsc::SyncSender<Input>>,
    output: mpsc::Receiver<Result<Vec<[f32; 24]>, String>>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Worker {
    fn start() -> Result<Self, String> {
        let (tx, rx) = mpsc::sync_channel::<Input>(1);
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("mirror-terrain-scene".into())
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                while let Ok(input) = rx.recv() {
                    let _scope = crate::host::performance_telemetry::Scope::new(
                        crate::host::performance_telemetry::Category::GameGraphics,
                    );
                    super::environment_geometry::recycle_projection_output(input.recycle);
                    let result = super::environment_geometry::planet(
                        &input.view,
                        &input.surface,
                        input.material.as_deref().map(Vec::as_slice),
                    );
                    if done_tx.send(result).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| format!("terrain scene worker: {e}"))?;
        Ok(Self {
            input: Some(tx),
            output: done_rx,
            thread: Some(thread),
        })
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.input.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
thread_local! {static WORKER:std::cell::RefCell<Option<Worker>>=const{std::cell::RefCell::new(None)};}
pub(super) fn enabled() -> bool {
    cfg!(target_os = "macos")
        && !cfg!(test)
        && std::env::var("WORMWOOD_GAME_TERRAIN_SYNC").as_deref() != Ok("1")
}
pub(super) fn begin(
    view: Value,
    surface: Value,
    material: Option<Arc<Vec<[f64; 6]>>>,
    recycle: Vec<[f32; 24]>,
) -> Result<Job, String> {
    WORKER.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_none() {
            *slot = Some(Worker::start()?);
        }
        slot.as_ref()
            .unwrap()
            .input
            .as_ref()
            .unwrap()
            .send(Input {
                view,
                surface,
                material,
                recycle,
            })
            .map_err(|_| "terrain scene worker terminated".to_owned())?;
        Ok(Job { pending: true })
    })
}
pub(super) struct Job {
    pending: bool,
}
impl Job {
    pub fn finish(mut self) -> Result<Vec<[f32; 24]>, String> {
        self.pending = false;
        receive()
    }
}
fn receive() -> Result<Vec<[f32; 24]>, String> {
    WORKER.with(|slot| {
        slot.borrow()
            .as_ref()
            .ok_or("terrain scene worker missing")?
            .output
            .recv()
            .map_err(|_| "terrain scene worker terminated".to_owned())?
    })
}
// On a fallible scene dependency, drain this one job before reusing the worker.
impl Drop for Job {
    fn drop(&mut self) {
        if self.pending {
            let _ = receive();
        }
    }
}
pub(super) fn close() {
    WORKER.with(|slot| {
        slot.borrow_mut().take();
    });
}
