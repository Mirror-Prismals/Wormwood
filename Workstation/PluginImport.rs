//! Explicitly selected plugin bundles only. No system plugin-directory search.
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

fn folder(path: &Path) -> Result<&'static str, String> {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("vst3") => Ok("USER_VST3"),
        Some("vst") => Ok("USER_VST2"),
        Some("clap") => Ok("USER_CLAP"),
        Some("component") => Ok("USER_AUDIOUNIT"),
        _ => Err("Choose a .vst3, .vst, .clap or .component plugin bundle".into()),
    }
}
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn ww_import_publish_bundle(
        source: *const std::ffi::c_char,
        destination: *const std::ffi::c_char,
    ) -> i32;
}

pub(super) fn import_file(
    source: &Path,
    user: &Path,
    cancel: &AtomicBool,
) -> Result<PathBuf, String> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (source, user, cancel);
        return Err("Plugin bundle import requires macOS".into());
    }
    #[cfg(target_os = "macos")]
    {
        use std::{
            ffi::CString,
            fs,
            os::unix::ffi::OsStrExt,
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        let directory = user.join(folder(source)?);
        if !source.is_dir()
            || !source.join("Contents/Info.plist").is_file()
            || !source.join("Contents/MacOS").is_dir()
        {
            return Err("Not a macOS plugin bundle".into());
        }
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let destination = directory.join(source.file_name().ok_or("Plugin filename is missing")?);
        if destination.exists() {
            return Err("A plugin with this filename is already in the user library".into());
        }
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let stage = directory.join(format!(
            ".plugin-import-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&stage).map_err(|e| e.to_string())?;
        let copied = stage.join(source.file_name().unwrap());
        let result = (|| {
            let mut child = Command::new("/usr/bin/ditto")
                .args(["--rsrc", "--extattr"])
                .arg(source)
                .arg(&copied)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?;
            let start = Instant::now();
            loop {
                if cancel.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(120) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("Plugin copy cancelled or timed out".into());
                }
                match child.try_wait().map_err(|e| e.to_string())? {
                    Some(s) => {
                        if !s.success() {
                            return Err("Could not copy plugin bundle".into());
                        }
                        break;
                    }
                    None => std::thread::sleep(Duration::from_millis(25)),
                }
            }
            if cancel.load(Ordering::Relaxed) {
                return Err("Plugin import cancelled".into());
            }
            let from = CString::new(copied.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
            let to = CString::new(destination.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
            // RENAME_EXCL prevents a concurrent import replacing an existing bundle.
            if unsafe { ww_import_publish_bundle(from.as_ptr(), to.as_ptr()) } != 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(destination)
        })();
        let _ = fs::remove_dir_all(stage);
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plugin_formats_only() {
        for (ext, expected) in [
            ("vst3", "USER_VST3"),
            ("vst", "USER_VST2"),
            ("clap", "USER_CLAP"),
            ("component", "USER_AUDIOUNIT"),
        ] {
            assert_eq!(folder(Path::new(&format!("Test.{ext}"))).unwrap(), expected);
        }
        assert!(folder(Path::new("setup.pkg")).is_err());
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn copies_selected_bundle_preserving_links_and_never_replaces() {
        use std::fs;
        let root = std::env::temp_dir().join(format!("plugin-import-test-{}", std::process::id()));
        let bundle = root.join("source/Test.vst3");
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::write(bundle.join("Contents/Info.plist"), b"test metadata").unwrap();
        fs::write(
            bundle.join("Contents/MacOS/Test"),
            b"test fixture, not executable",
        )
        .unwrap();
        std::os::unix::fs::symlink("MacOS/Test", bundle.join("Contents/link")).unwrap();
        let cancel = AtomicBool::new(false);
        let user = root.join("USER");
        let copied = import_file(&bundle, &user, &cancel).unwrap();
        assert_eq!(copied, user.join("USER_VST3/Test.vst3"));
        assert_eq!(
            fs::read_link(copied.join("Contents/link")).unwrap(),
            PathBuf::from("MacOS/Test")
        );
        assert!(import_file(&bundle, &user, &cancel).is_err());
        assert_eq!(
            fs::read(copied.join("Contents/MacOS/Test")).unwrap(),
            b"test fixture, not executable"
        );
        assert_eq!(fs::read_dir(user.join("USER_VST3")).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
