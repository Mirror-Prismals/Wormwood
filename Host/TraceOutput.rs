//! Bounded asynchronous diagnostic IO. Host serializes whole records; the
//! writer owns file writes/rotation. Audio callbacks never call this module.
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
};
const SEGMENT: u64 = 64 * 1024 * 1024;
const BATCH: usize = 64 * 1024;
enum Message {
    Bytes(Vec<u8>),
    Barrier(mpsc::SyncSender<Result<(), String>>),
}
struct Disk {
    path: PathBuf,
    file: Option<File>,
    bytes: u64,
}
impl Disk {
    fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let bytes = file.metadata()?.len();
        Ok(Self {
            path: path.to_owned(),
            file: Some(file),
            bytes,
        })
    }
    fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.bytes.saturating_add(bytes.len() as u64) > SEGMENT {
            self.file.as_mut().unwrap().flush()?;
            self.file.take();
            let previous = self.path.with_extension("previous.jsonl");
            if previous.exists() {
                std::fs::remove_file(&previous)?;
            }
            std::fs::rename(&self.path, previous)?;
            self.file = Some(File::create(&self.path)?);
            self.bytes = 0;
        }
        self.file.as_mut().unwrap().write_all(bytes)?;
        self.bytes += bytes.len() as u64;
        Ok(())
    }
}
struct Background {
    sender: Option<mpsc::SyncSender<Message>>,
    thread: Option<JoinHandle<()>>,
    failed: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
}
impl Background {
    fn new(mut disk: Disk) -> io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<Message>(32);
        let failed = Arc::new(AtomicBool::new(false));
        let error = Arc::new(Mutex::new(None));
        let flag = failed.clone();
        let failure = error.clone();
        let thread = std::thread::Builder::new()
            .name("mirror-diagnostic-writer".into())
            .spawn(move || {
                let mut problem = None;
                while let Ok(message) = rx.recv() {
                    match message {
                        Message::Bytes(bytes) => {
                            if problem.is_none() {
                                if let Err(e) = disk.write(&bytes) {
                                    problem = Some(e.to_string());
                                    *failure
                                        .lock()
                                        .unwrap_or_else(std::sync::PoisonError::into_inner) =
                                        problem.clone();
                                    flag.store(true, Ordering::Release);
                                }
                            }
                        }
                        Message::Barrier(done) => {
                            let result = problem.clone().map_or(Ok(()), Err);
                            let _ = done.send(result);
                        }
                    }
                }
                if let Some(file) = disk.file.as_mut() {
                    let _ = file.flush();
                }
            })?;
        Ok(Self {
            sender: Some(tx),
            thread: Some(thread),
            failed,
            error,
        })
    }
    fn check(&self) -> io::Result<()> {
        if self.failed.load(Ordering::Acquire) {
            return Err(io::Error::other(
                self.error
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
                    .unwrap_or_else(|| "diagnostic writer failed".into()),
            ));
        }
        Ok(())
    }
    fn send(&self, bytes: Vec<u8>) -> io::Result<()> {
        self.check()?;
        // Bounded backpressure preserves every lifecycle record. Thirty-two queued
        // batches absorb short disk stalls; frame work does not perform file IO.
        self.sender
            .as_ref()
            .unwrap()
            .send(Message::Bytes(bytes))
            .map_err(|_| io::Error::other("diagnostic writer terminated"))
    }
    fn barrier(&self) -> io::Result<()> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.sender
            .as_ref()
            .unwrap()
            .send(Message::Barrier(tx))
            .map_err(|_| io::Error::other("diagnostic writer terminated"))?;
        rx.recv()
            .map_err(|_| io::Error::other("diagnostic writer terminated"))?
            .map_err(io::Error::other)
    }
}
impl Drop for Background {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
enum Backend {
    Synchronous(Disk),
    Asynchronous(Background),
}
pub(crate) struct Writer {
    backend: Backend,
    buffer: Vec<u8>,
}
impl Writer {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        Self::with_mode(path, !cfg!(test))
    }
    fn with_mode(path: &Path, asynchronous: bool) -> io::Result<Self> {
        let disk = Disk::open(path)?;
        let backend = if asynchronous {
            Backend::Asynchronous(Background::new(disk)?)
        } else {
            Backend::Synchronous(disk)
        };
        Ok(Self {
            backend,
            buffer: Vec::with_capacity(BATCH),
        })
    }
    #[cfg(test)]
    pub(super) fn force_rotation_for_test(&mut self)->io::Result<()>{
        self.flush()?;
        match &mut self.backend{Backend::Synchronous(d)=>{d.bytes=SEGMENT;Ok(())},Backend::Asynchronous(_)=>Err(io::Error::other("synchronous test writer required"))}
    }
    pub(crate) fn finish_pending(&mut self) -> io::Result<()> {
        self.flush()?;
        match &self.backend {
            Backend::Asynchronous(w) => w.barrier(),
            Backend::Synchronous(_) => Ok(()),
        }
    }
}
impl Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Backend::Asynchronous(w) = &self.backend {
            w.check()?;
        }
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() >= BATCH {
            self.flush()?;
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        let bytes = std::mem::replace(&mut self.buffer, Vec::with_capacity(BATCH));
        match &mut self.backend {
            Backend::Synchronous(d) => d.write(&bytes),
            Backend::Asynchronous(w) => w.send(bytes),
        }
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        if let Err(e) = self.finish_pending() {
            eprintln!("Diagnostic trace writer: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn asynchronous_trace_preserves_order_complete_records_and_join() {
        let path =
            std::env::temp_dir().join(format!("mirror-trace-worker-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut w = Writer::with_mode(&path, true).unwrap();
        for i in 0..1000 {
            w.write_all(format!("{{\"index\":{i}}}\n").as_bytes())
                .unwrap();
        }
        w.finish_pending().unwrap();
        let rows: Vec<_> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str::<serde_json::Value>(s).unwrap())
            .collect();
        assert_eq!(rows.len(), 1000);
        assert_eq!(rows[0]["index"], 0);
        assert_eq!(rows[999]["index"], 999);
        drop(w);
        std::fs::remove_file(path).unwrap();
    }
}
