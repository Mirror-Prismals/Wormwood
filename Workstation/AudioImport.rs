//! User-library import runs outside audio callbacks. The native chooser only
//! returns paths; one worker converts, validates and publishes files atomically.
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
};

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn ww_audio_import_open(plugins: i32) -> i32;
    fn ww_audio_import_poll(buffer: *mut std::ffi::c_char, capacity: usize) -> i32;
    fn ww_audio_import_close();
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Kind {
    #[default]
    Audio,
    Plugins,
}
#[derive(Default)]
pub(crate) struct Importer {
    choosing: bool,
    kind: Kind,
    worker: Option<mpsc::Receiver<String>>,
    cancel: Arc<AtomicBool>,
    pub notice: Option<String>,
}
impl Importer {
    pub fn open(&mut self, kind: Kind) -> Result<(), String> {
        if self.worker.is_some() || self.choosing {
            return Ok(());
        }
        #[cfg(target_os = "macos")]
        {
            if unsafe { ww_audio_import_open(i32::from(kind == Kind::Plugins)) } != 0 {
                return Err("Could not open audio file chooser".into());
            }
            self.kind = kind;
            self.choosing = true;
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err("Audio file chooser is available on macOS".into())
        }
    }
    pub fn poll(&mut self, root: &Path) -> Result<Option<Kind>, String> {
        if let Some(worker) = &self.worker {
            match worker.try_recv() {
                Ok(notice) => {
                    self.notice = Some(notice);
                    self.worker = None;
                    return Ok(Some(self.kind));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.worker = None;
                    return Err("Audio import worker stopped".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        #[cfg(target_os = "macos")]
        if self.choosing {
            let mut buffer = vec![0u8; 1_048_576];
            let status = unsafe { ww_audio_import_poll(buffer.as_mut_ptr().cast(), buffer.len()) };
            if status < 0 {
                self.choosing = false;
                return Err("Audio file selection could not be read".into());
            }
            if status > 0 {
                self.choosing = false;
                let end = buffer
                    .iter()
                    .position(|b| *b == 0)
                    .ok_or("Invalid audio file selection")?;
                let paths: Vec<PathBuf> =
                    serde_json::from_slice(&buffer[..end]).map_err(|e| e.to_string())?;
                if paths.is_empty() {
                    return Ok(None);
                }
                let destination = super::audio_library::user_root(root);
                self.cancel = Arc::new(AtomicBool::new(false));
                let cancel = self.cancel.clone();
                let (tx, rx) = mpsc::channel();
                self.worker = Some(rx);
                self.notice = Some(
                    if self.kind == Kind::Plugins {
                        "Importing plugins…"
                    } else {
                        "Importing audio…"
                    }
                    .into(),
                );
                let kind = self.kind;
                std::thread::spawn(move || {
                    let mut imported = 0;
                    let mut errors = Vec::new();
                    for path in paths {
                        if cancel.load(Ordering::Relaxed) {
                            break;
                        }
                        match if kind == Kind::Plugins {
                            super::plugin_import::import_file(&path, &destination.join("USER_SAMPLES"), &cancel)
                        } else {
                            import_file(&path, &destination.join("USER_SAMPLES"), &cancel)
                        } {
                            Ok(_) => imported += 1,
                            Err(e) => errors.push(format!(
                                "{}: {e}",
                                path.file_name().unwrap_or_default().to_string_lossy()
                            )),
                        }
                    }
                    let mut notice = if kind == Kind::Plugins {
                        format!("Added {imported} plugin bundle(s) to the user library")
                    } else {
                        format!("Imported {imported} audio file(s) into User Samples")
                    };
                    if !errors.is_empty() {
                        notice.push_str(&format!("; {}", errors.join("; ")));
                    }
                    let _ = tx.send(notice);
                });
            }
        }
        let _ = root;
        Ok(None)
    }
    pub fn close(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        #[cfg(target_os = "macos")]
        if self.choosing {
            unsafe {
                ww_audio_import_close();
            }
        }
        self.choosing = false;
        self.worker = None;
    }
}
impl Drop for Importer {
    fn drop(&mut self) {
        self.close();
    }
}

fn import_file(source: &Path, directory: &Path, cancel: &AtomicBool) -> Result<PathBuf, String> {
    use std::{
        fs,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    if !source.is_file() {
        return Err("Not a regular audio file".into());
    }
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    // A private staging directory prevents partial imports appearing in scans.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let stage = directory.join(format!(
        ".audio-import-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&stage).map_err(|e| e.to_string())?;
    let temp = stage.join("audio.wav");
    let result = (|| {
        if source
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
        {
            fs::copy(source, &temp).map_err(|e| e.to_string())?;
        } else {
            let mut child = Command::new("/usr/bin/afconvert")
                .args(["-f", "WAVE", "-d", "LEF32"])
                .arg(source)
                .arg(&temp)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?;
            let start = Instant::now();
            loop {
                if cancel.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(120) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("Audio conversion cancelled or timed out".into());
                }
                match child.try_wait().map_err(|e| e.to_string())? {
                    Some(status) => {
                        if !status.success() {
                            return Err("macOS could not decode this audio format".into());
                        }
                        break;
                    }
                    None => std::thread::sleep(Duration::from_millis(25)),
                }
            }
        }
        super::audio_library::drag_wave(&temp,cancel).map_err(|e|format!("Unsupported mono/stereo WAV: {e}"))?;
        if cancel.load(Ordering::Relaxed) {
            return Err("Audio import cancelled".into());
        }
        let stem = source.file_stem().unwrap_or_default().to_string_lossy();
        for n in 0..10_000 {
            let name = if n == 0 {
                format!("{stem}.wav")
            } else {
                format!("{stem} ({n}).wav")
            };
            let destination = directory.join(name);
            match fs::hard_link(&temp, &destination) {
                Ok(()) => return Ok(destination),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
        Err("Too many files with the same name".into())
    })();
    let _ = fs::remove_dir_all(stage);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires WORMWOOD_IMPORT_FIXTURE_SOURCE and WORMWOOD_IMPORT_FIXTURE_USER"]
    fn selected_audio_import_fixtures() {
        let sources=PathBuf::from(std::env::var_os("WORMWOOD_IMPORT_FIXTURE_SOURCE").unwrap());
        let user=PathBuf::from(std::env::var_os("WORMWOOD_IMPORT_FIXTURE_USER").unwrap());
        for name in ["glass_beach_-_motions_Official_Audio.wav","Last_Christmas_Single_Version.wav"] {
            let source=sources.join(name);
            let copied=import_file(&source,&user.join("USER_SAMPLES"),&AtomicBool::new(false)).unwrap();
            assert_eq!(std::fs::read(source).unwrap(),std::fs::read(&copied).unwrap());
            assert!(super::super::audio_library::scan(&user).unwrap().iter()
                .any(|entry|entry["path"]==copied.to_string_lossy().as_ref()));
        }
    }
    #[test]
    fn streaming_wav_is_copied_unchanged_and_discovered() {
        let root=std::env::temp_dir().join(format!("streaming-import-{}",std::process::id()));
        let library=root.join("USER_SAMPLES");
        std::fs::create_dir_all(&root).unwrap();
        // Match streaming exports: unknown RIFF/data lengths and metadata.
        let mut bytes=b"RIFF".to_vec();bytes.extend(u32::MAX.to_le_bytes());
        bytes.extend(b"WAVEfmt ");bytes.extend(16u32.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());bytes.extend(2u16.to_le_bytes());
        bytes.extend(48000u32.to_le_bytes());bytes.extend(192000u32.to_le_bytes());
        bytes.extend(4u16.to_le_bytes());bytes.extend(16u16.to_le_bytes());
        bytes.extend(b"LIST");bytes.extend(4u32.to_le_bytes());bytes.extend(b"INFO");
        bytes.extend(b"data");bytes.extend(u32::MAX.to_le_bytes());
        bytes.extend([0u8,32,0,224,0,0,0,0]);
        let source=root.join("streaming.wav");std::fs::write(&source,&bytes).unwrap();
        let copied=import_file(&source,&library,&AtomicBool::new(false)).unwrap();
        assert_eq!(std::fs::read(&copied).unwrap(),bytes);
        let decoded=super::super::drum_sample::read_timeline(&copied).unwrap();
        assert_eq!(&*decoded.samples,&[0.25,-0.25,0.,0.]);
        // scan directly to avoid the application's configured user-root override.
        let entries=super::super::audio_library::scan(&root).unwrap();
        assert!(entries.iter().any(|entry|entry["path"]==copied.to_string_lossy().as_ref()));
        // Explicit lengths must still reject truncation.
        bytes[4..8].copy_from_slice(&1000u32.to_le_bytes());
        std::fs::write(&source,&bytes).unwrap();
        assert!(import_file(&source,&library,&AtomicBool::new(false)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn imports_song_larger_than_drum_sample_limit() {
        let root=std::env::temp_dir().join(format!("song-import-{}",std::process::id()));
        std::fs::create_dir_all(&root).unwrap();let source=root.join("song.wav");
        let n=9*1024*1024usize;let mut bytes=Vec::new();
        bytes.extend_from_slice(b"RIFF");bytes.extend_from_slice(&((n+36)as u32).to_le_bytes());bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());bytes.extend_from_slice(&1u16.to_le_bytes());bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&48000u32.to_le_bytes());bytes.extend_from_slice(&192000u32.to_le_bytes());bytes.extend_from_slice(&4u16.to_le_bytes());bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");bytes.extend_from_slice(&(n as u32).to_le_bytes());bytes.resize(n+44,0);std::fs::write(&source,&bytes).unwrap();
        let path=import_file(&source,&root.join("USER_SAMPLES"),&AtomicBool::new(false)).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(),bytes.len()as u64);
        assert!(super::super::drum_sample::read(&path).is_err());
        let song=super::super::drum_sample::read_timeline(&path).unwrap();assert_eq!(song.samples.len(),n/2);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn imports_without_overwriting_and_rejects_invalid_audio() {
        let dir = std::env::temp_dir().join(format!("audio-import-test-{}", std::process::id()));
        let source = dir.join("source");
        let library = dir.join("USER");
        std::fs::create_dir_all(&source).unwrap();
        let wav = source.join("tone.wav");
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&38u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        for v in [1u16, 1] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes.extend_from_slice(&48000u32.to_le_bytes());
        bytes.extend_from_slice(&96000u32.to_le_bytes());
        for v in [2u16, 16] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&0i16.to_le_bytes());
        std::fs::write(&wav, &bytes).unwrap();
        let cancel = AtomicBool::new(false);
        assert_eq!(
            import_file(&wav, &library, &cancel).unwrap(),
            library.join("tone.wav")
        );
        assert_eq!(
            import_file(&wav, &library, &cancel).unwrap(),
            library.join("tone (1).wav")
        );
        assert_eq!(std::fs::read(library.join("tone.wav")).unwrap(), bytes);
        #[cfg(target_os = "macos")]
        {
            let aiff = source.join("converted.aiff");
            assert!(
                std::process::Command::new("/usr/bin/afconvert")
                    .args(["-f", "AIFF", "-d", "BEI16"])
                    .arg(&wav)
                    .arg(&aiff)
                    .status()
                    .unwrap()
                    .success()
            );
            let converted = import_file(&aiff, &library, &cancel).unwrap();
            assert_eq!(converted, library.join("converted.wav"));
            assert!(super::super::audio_library::drag_wave(&converted, &cancel).is_ok());
            std::fs::remove_file(converted).unwrap();
        }
        std::fs::write(&wav, b"broken").unwrap();
        assert!(import_file(&wav, &library, &cancel).is_err());
        assert_eq!(std::fs::read_dir(&library).unwrap().count(), 2);
        cancel.store(true, Ordering::Relaxed);
        assert!(import_file(&library.join("tone.wav"), &library, &cancel).is_err());
        assert_eq!(std::fs::read_dir(&library).unwrap().count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
