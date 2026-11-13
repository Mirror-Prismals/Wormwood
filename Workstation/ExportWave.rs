//! Control/worker-side streaming export, never an audio callback operation.
//!
//! Four mono IEEE-float WAV files preserve raw L/S/FF/R samples, including
//! headroom. Each 58-byte little-endian header contains RIFF/WAVE, an 18-byte
//! WAVEFORMATEX `fmt ` chunk (format 3, cbSize 0), `fact` frame count, and `data`.
//! The complete length is checked before creating a new destination directory.
//! Files are staged there, synced, then published using non-replacing hard links.
//! A successful finish owns the output permanently; abort/Drop only clean up an
//! unfinished bundle. File identity checks preserve externally replaced entries.
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

const HEADER_BYTES: u64 = 58;
const BLOCK_FRAMES: usize = 4096;

#[cfg(unix)]
type Identity = (u64, u64);
#[cfg(not(unix))]
type Identity = ();

#[cfg(unix)]
fn identity(metadata: &Metadata) -> Identity {
    use std::os::unix::fs::MetadataExt;
    (metadata.dev(), metadata.ino())
}
#[cfg(not(unix))]
fn identity(_: &Metadata) -> Identity {}

#[derive(Debug)]
pub(super) struct Bundle {
    destination: PathBuf,
    directory_identity: Identity,
    directory_owned: bool,
    partial: [PathBuf; 4],
    paths: [PathBuf; 4],
    files: [Option<File>; 4],
    published: [bool; 4],
    total_frames: u64,
    written_frames: u64,
    active: bool,
    finished: bool,
}

fn validate_names(names: &[String; 4]) -> Result<(), String> {
    for (index, name) in names.iter().enumerate() {
        if name.is_empty()
            || name.len() > 200
            || name.trim() != name
            || name == "."
            || name == ".."
            || name.ends_with('.')
            || name
                .chars()
                .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
        {
            return Err(format!("invalid export stem name at bus {index}"));
        }
        let folded = name.to_lowercase();
        if names[..index]
            .iter()
            .any(|other| other.to_lowercase() == folded)
        {
            return Err("export stem names must be distinct ignoring case".into());
        }
    }
    Ok(())
}

fn header(sample_rate: u32, total_frames: u64) -> Result<[u8; 58], String> {
    if !(8000..=192000).contains(&sample_rate) {
        return Err("export sample rate must be within 8000..192000 Hz".into());
    }
    if total_frames == 0 {
        return Err("export must contain at least one frame".into());
    }
    let data_bytes = total_frames
        .checked_mul(4)
        .ok_or("WAV data size overflow")?;
    let riff_bytes = data_bytes
        .checked_add(HEADER_BYTES - 8)
        .filter(|n| *n <= u32::MAX as u64)
        .ok_or("export exceeds the RIFF32 WAV size limit")?;
    let frame_count = u32::try_from(total_frames).map_err(|_| "WAV fact count overflow")?;
    let byte_rate = sample_rate.checked_mul(4).ok_or("WAV byte rate overflow")?;
    let mut result = [0; 58];
    result[0..4].copy_from_slice(b"RIFF");
    result[4..8].copy_from_slice(&(riff_bytes as u32).to_le_bytes());
    result[8..12].copy_from_slice(b"WAVE");
    result[12..16].copy_from_slice(b"fmt ");
    result[16..20].copy_from_slice(&18u32.to_le_bytes());
    result[20..22].copy_from_slice(&3u16.to_le_bytes());
    result[22..24].copy_from_slice(&1u16.to_le_bytes());
    result[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    result[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    result[32..34].copy_from_slice(&4u16.to_le_bytes());
    result[34..36].copy_from_slice(&32u16.to_le_bytes());
    // Bytes 36..38 are WAVEFORMATEX.cbSize = 0.
    result[38..42].copy_from_slice(b"fact");
    result[42..46].copy_from_slice(&4u32.to_le_bytes());
    result[46..50].copy_from_slice(&frame_count.to_le_bytes());
    result[50..54].copy_from_slice(b"data");
    result[54..58].copy_from_slice(&(data_bytes as u32).to_le_bytes());
    Ok(result)
}

impl Bundle {
    pub(super) fn create(
        destination: &Path,
        names: &[String; 4],
        sample_rate: u32,
        total_frames: u64,
    ) -> Result<Self, String> {
        // Other platforms must supply equally reliable file identities before
        // enabling cleanup. Refuse before mutation instead of deleting by name.
        if !cfg!(unix) {
            return Err("safe export file ownership is not implemented on this platform".into());
        }
        validate_names(names)?;
        let header = header(sample_rate, total_frames)?;
        if !matches!(
            destination.components().next_back(),
            Some(Component::Normal(_))
        ) {
            return Err("export destination must name a new directory".into());
        }
        let destination = if destination.is_absolute() {
            destination.to_path_buf()
        } else {
            std::env::current_dir()
                .map_err(|e| format!("resolve export directory: {e}"))?
                .join(destination)
        };
        let parent = destination
            .parent()
            .ok_or("export directory needs a parent")?;
        if !parent.is_dir() {
            return Err("export destination parent must already exist".into());
        }
        // create_dir is exclusive, unlike create_dir_all. Never adopt an existing
        // directory, including a symlink, as an owned export destination.
        fs::create_dir(&destination)
            .map_err(|e| format!("create new export directory {}: {e}", destination.display()))?;
        let directory_metadata = fs::symlink_metadata(&destination).map_err(|e| {
            format!(
                "cannot establish ownership of new export directory {}; directory retained: {e}",
                destination.display()
            )
        })?;
        if !directory_metadata.is_dir() {
            return Err(
                "new export directory changed before ownership could be established".into(),
            );
        }
        let mut bundle = Self {
            directory_identity: identity(&directory_metadata),
            directory_owned: true,
            partial: std::array::from_fn(|i| {
                destination.join(format!(".{}.wav.partial", names[i]))
            }),
            paths: std::array::from_fn(|i| destination.join(format!("{}.wav", names[i]))),
            destination,
            files: std::array::from_fn(|_| None),
            published: [false; 4],
            total_frames,
            written_frames: 0,
            active: true,
            finished: false,
        };
        for index in 0..4 {
            if let Err(error) = bundle.check_directory() {
                return Err(bundle.fail(error));
            }
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&bundle.partial[index]);
            match file {
                Ok(file) => bundle.files[index] = Some(file),
                Err(error) => {
                    return Err(bundle.fail(format!("create export bus {index}: {error}")));
                }
            }
            if let Err(error) = bundle.files[index].as_mut().unwrap().write_all(&header) {
                return Err(bundle.fail(format!("write WAV header for bus {index}: {error}")));
            }
        }
        Ok(bundle)
    }

    pub(super) fn write(&mut self, frames: &[[f32; 4]]) -> Result<(), String> {
        if !self.active {
            return Err("export bundle is no longer writable".into());
        }
        if frames.len() > BLOCK_FRAMES {
            return Err(self.fail("export write exceeds 4096 frames".into()));
        }
        let next = self.written_frames.checked_add(frames.len() as u64);
        if next.is_none_or(|n| n > self.total_frames) {
            return Err(self.fail("export write exceeds declared frame count".into()));
        }
        if frames.iter().flatten().any(|sample| !sample.is_finite()) {
            return Err(self.fail("export samples must all be finite".into()));
        }
        if let Err(error) = self.check_directory() {
            return Err(self.fail(error));
        }
        let mut bytes = [0u8; BLOCK_FRAMES * 4];
        for bus in 0..4 {
            if let Err(error) = self.check_file(bus, &self.partial[bus]) {
                return Err(self.fail(error));
            }
            for (index, frame) in frames.iter().enumerate() {
                bytes[index * 4..index * 4 + 4].copy_from_slice(&frame[bus].to_le_bytes());
            }
            if let Err(error) = self.files[bus]
                .as_mut()
                .unwrap()
                .write_all(&bytes[..frames.len() * 4])
            {
                return Err(self.fail(format!("write export bus {bus}: {error}")));
            }
        }
        self.written_frames = next.unwrap();
        Ok(())
    }

    pub(super) fn finish(&mut self) -> Result<[PathBuf; 4], String> {
        if !self.active {
            return Err("export bundle is no longer active".into());
        }
        if self.written_frames != self.total_frames {
            return Err(self.fail("export ended before its declared frame count".into()));
        }
        let result = (|| {
            self.check_directory()?;
            // Sync every complete stem before any final filename becomes visible.
            for bus in 0..4 {
                self.check_file(bus, &self.partial[bus])?;
                let file = self.files[bus].as_mut().unwrap();
                file.flush()
                    .and_then(|_| file.sync_all())
                    .map_err(|e| format!("flush/sync export bus {bus}: {e}"))?;
                let size = file
                    .metadata()
                    .map_err(|e| format!("inspect completed bus {bus}: {e}"))?
                    .len();
                if size != HEADER_BYTES + self.total_frames * 4 {
                    return Err(format!(
                        "export bus {bus} length changed before publication"
                    ));
                }
            }
            for bus in 0..4 {
                self.check_directory()?;
                self.check_file(bus, &self.partial[bus])?;
                fs::hard_link(&self.partial[bus], &self.paths[bus]).map_err(|e| {
                    format!(
                        "publish {} without replacing a file: {e}",
                        self.paths[bus].display()
                    )
                })?;
                self.published[bus] = true;
                self.check_file(bus, &self.paths[bus])?;
            }
            self.sync_directory()?;
            for bus in 0..4 {
                self.check_directory()?;
                self.remove_owned_file(bus, &self.partial[bus])?;
            }
            self.sync_directory()?;
            Ok::<(), String>(())
        })();
        if let Err(error) = result {
            return Err(self.fail(error));
        }
        self.finished = true;
        self.active = false;
        self.files = std::array::from_fn(|_| None);
        Ok(self.paths.clone())
    }

    /// Idempotent cleanup; completed exports are never removed by abort or Drop.
    pub(super) fn abort(&mut self) -> Result<(), String> {
        if self.finished || !self.directory_owned {
            return Ok(());
        }
        self.active = false;
        self.cleanup()
    }

    fn fail(&mut self, error: String) -> String {
        match self.abort() {
            Ok(()) => error,
            Err(cleanup) => format!("{error}; cleanup incomplete: {cleanup}"),
        }
    }

    fn check_directory(&self) -> Result<(), String> {
        let metadata = fs::symlink_metadata(&self.destination).map_err(|e| {
            format!(
                "inspect owned export directory {}: {e}",
                self.destination.display()
            )
        })?;
        if !metadata.is_dir() || identity(&metadata) != self.directory_identity {
            return Err(format!(
                "export directory identity changed; retained {}",
                self.destination.display()
            ));
        }
        Ok(())
    }

    fn check_file(&self, bus: usize, path: &Path) -> Result<(), String> {
        let owned = self.files[bus]
            .as_ref()
            .ok_or("export file ownership missing")?
            .metadata()
            .map_err(|e| format!("inspect open export bus {bus}: {e}"))?;
        let found = fs::symlink_metadata(path)
            .map_err(|e| format!("inspect export file {}: {e}", path.display()))?;
        if !found.is_file() || identity(&found) != identity(&owned) {
            return Err(format!(
                "export file identity changed; retained {}",
                path.display()
            ));
        }
        Ok(())
    }

    fn remove_owned_file(&self, bus: usize, path: &Path) -> Result<(), String> {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(format!("inspect cleanup path {}: {error}", path.display())),
            Ok(_) => {}
        }
        self.check_file(bus, path)?;
        fs::remove_file(path)
            .map_err(|e| format!("remove owned export file {}: {e}", path.display()))
    }

    fn sync_directory(&self) -> Result<(), String> {
        self.check_directory()?;
        let directory = File::open(&self.destination)
            .map_err(|e| format!("open export directory for sync: {e}"))?;
        if identity(&directory.metadata().map_err(|e| e.to_string())?) != self.directory_identity {
            return Err("export directory changed before sync".into());
        }
        directory
            .sync_all()
            .map_err(|e| format!("sync export directory: {e}"))
    }

    fn cleanup(&mut self) -> Result<(), String> {
        self.check_directory()?;
        let mut errors = Vec::new();
        for bus in 0..4 {
            if self.files[bus].is_none() {
                continue;
            }
            if self.published[bus] {
                if let Err(error) = self.remove_owned_file(bus, &self.paths[bus]) {
                    errors.push(error);
                }
            }
            if let Err(error) = self.remove_owned_file(bus, &self.partial[bus]) {
                errors.push(error);
            }
        }
        // remove_dir deliberately leaves external files and nonempty folders.
        match self.check_directory().and_then(|_| {
            fs::remove_dir(&self.destination).map_err(|e| {
                format!(
                    "retain export directory {}: {e}",
                    self.destination.display()
                )
            })
        }) {
            Ok(()) => self.directory_owned = false,
            Err(error) => errors.push(error),
        }
        if errors.is_empty() {
            self.files = std::array::from_fn(|_| None);
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

impl Drop for Bundle {
    fn drop(&mut self) {
        if !self.finished && self.directory_owned {
            let _ = self.cleanup();
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Root(PathBuf);
    impl Root {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "wormwood-export-wave-{}-{time}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn destination(&self) -> PathBuf {
            self.0.join("export")
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn names() -> [String; 4] {
        ["L", "S", "FF", "R"].map(str::to_owned)
    }
    fn u16_at(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
    }
    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    // Walk RIFF chunks, independently of the writer's fixed byte offsets.
    fn decode(path: &Path, rate: u32, count: u32) -> Vec<f32> {
        let bytes = fs::read(path).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(u32_at(&bytes, 4) as usize, bytes.len() - 8);
        let mut at = 12;
        let mut format = false;
        let mut fact = false;
        let mut samples = None;
        while at < bytes.len() {
            let size = u32_at(&bytes, at + 4) as usize;
            let data = &bytes[at + 8..at + 8 + size];
            match &bytes[at..at + 4] {
                b"fmt " => {
                    assert_eq!(size, 18);
                    assert_eq!(u16_at(data, 0), 3);
                    assert_eq!(u16_at(data, 2), 1);
                    assert_eq!(u32_at(data, 4), rate);
                    assert_eq!(u32_at(data, 8), rate * 4);
                    assert_eq!(u16_at(data, 12), 4);
                    assert_eq!(u16_at(data, 14), 32);
                    assert_eq!(u16_at(data, 16), 0);
                    format = true;
                }
                b"fact" => {
                    assert_eq!(size, 4);
                    assert_eq!(u32_at(data, 0), count);
                    fact = true;
                }
                b"data" => {
                    assert_eq!(size, count as usize * 4);
                    samples = Some(
                        data.chunks_exact(4)
                            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                            .collect(),
                    );
                }
                other => panic!("unexpected chunk {other:?}"),
            }
            at += 8 + size + size % 2;
        }
        assert_eq!(at, bytes.len());
        assert!(format && fact);
        samples.unwrap()
    }

    #[test]
    fn streaming_wavs_preserve_bus_order_sample_bits_and_headroom() {
        let root = Root::new();
        let destination = root.destination();
        let frames = [
            [1., 0., 0., 0.],
            [0., -2., 0., 0.],
            [0., 0., 3., 0.],
            [0., 0., 0., -4.],
            [-0., f32::MAX, f32::MIN_POSITIVE, -f32::MAX],
        ];
        let mut bundle =
            Bundle::create(&destination, &names(), 48000, frames.len() as u64).unwrap();
        bundle.write(&frames[..2]).unwrap();
        bundle.write(&frames[2..]).unwrap();
        assert!(!destination.join("L.wav").exists());
        let paths = bundle.finish().unwrap();
        bundle.abort().unwrap(); // Successful output survives abort and Drop.
        drop(bundle);
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 4);
        for bus in 0..4 {
            assert_eq!(
                paths[bus],
                destination.join(format!("{}.wav", names()[bus]))
            );
            let samples = decode(&paths[bus], 48000, 5);
            assert_eq!(
                samples.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
                frames
                    .iter()
                    .map(|frame| frame[bus].to_bits())
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn invalid_names_rates_ranges_and_missing_parent_create_nothing() {
        let root = Root::new();
        let destination = root.destination();
        for invalid in [
            "",
            ".",
            "..",
            "../escape",
            "a/b",
            "a\\b",
            "a:b",
            "a\0b",
            " a",
            "a.",
        ] {
            let mut stems = names();
            stems[1] = invalid.into();
            assert!(
                Bundle::create(&destination, &stems, 48000, 1).is_err(),
                "{invalid:?}"
            );
            assert!(!destination.exists());
        }
        let mut duplicate = names();
        duplicate[1] = "l".into();
        assert!(Bundle::create(&destination, &duplicate, 48000, 1).is_err());
        for rate in [0, 7999, 192001, u32::MAX] {
            assert!(Bundle::create(&destination, &names(), rate, 1).is_err());
        }
        let max = (u32::MAX as u64 - (HEADER_BYTES - 8)) / 4;
        assert!(header(48000, max).is_ok());
        for frames in [0, max + 1, u64::MAX] {
            assert!(Bundle::create(&destination, &names(), 48000, frames).is_err());
            assert!(!destination.exists());
        }
        assert!(Bundle::create(&root.0.join("missing/export"), &names(), 48000, 1).is_err());
        assert!(!root.0.join("missing").exists());
    }

    #[test]
    fn existing_destination_is_never_adopted_or_modified() {
        let root = Root::new();
        let destination = root.destination();
        fs::create_dir(&destination).unwrap();
        let sentinel = destination.join("L.wav");
        fs::write(&sentinel, b"original").unwrap();
        assert!(Bundle::create(&destination, &names(), 48000, 1).is_err());
        assert_eq!(fs::read(sentinel).unwrap(), b"original");
        let file = root.0.join("file");
        fs::write(&file, b"keep").unwrap();
        assert!(Bundle::create(&file, &names(), 48000, 1).is_err());
        assert_eq!(fs::read(file).unwrap(), b"keep");
    }

    #[test]
    fn abort_drop_and_invalid_audio_remove_unfinished_owned_outputs() {
        let root = Root::new();
        let destination = root.destination();
        let mut bundle = Bundle::create(&destination, &names(), 8000, 2).unwrap();
        bundle.write(&[[0.; 4]]).unwrap();
        bundle.abort().unwrap();
        bundle.abort().unwrap();
        assert!(!destination.exists());
        assert!(bundle.write(&[[0.; 4]]).is_err());
        drop(bundle);
        {
            let mut unfinished = Bundle::create(&destination, &names(), 8000, 2).unwrap();
            unfinished.write(&[[0.; 4]]).unwrap();
        }
        assert!(!destination.exists());
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut bundle = Bundle::create(&destination, &names(), 8000, 1).unwrap();
            assert!(bundle.write(&[[0., 0., bad, 0.]]).is_err());
            assert!(!destination.exists());
        }
    }

    #[test]
    fn frame_count_and_block_limit_failures_clean_up() {
        let root = Root::new();
        let destination = root.destination();
        let mut short = Bundle::create(&destination, &names(), 48000, 2).unwrap();
        short.write(&[[0.; 4]]).unwrap();
        assert!(short.finish().is_err());
        assert!(!destination.exists());
        let mut long = Bundle::create(&destination, &names(), 48000, 1).unwrap();
        assert!(long.write(&[[0.; 4]; 2]).is_err());
        assert!(!destination.exists());
        let mut block = Bundle::create(&destination, &names(), 48000, 4097).unwrap();
        assert!(block.write(&vec![[0.; 4]; 4097]).is_err());
        assert!(!destination.exists());
    }

    #[test]
    fn write_errors_and_truncated_staging_data_never_publish() {
        let root = Root::new();
        let destination = root.destination();
        let mut bundle = Bundle::create(&destination, &names(), 48000, 1).unwrap();
        // An actual read-only descriptor makes write_all fail after earlier buses
        // have received data, without depending on disk capacity or user IDs.
        bundle.files[2] = Some(File::open(&bundle.partial[2]).unwrap());
        assert!(
            bundle
                .write(&[[1.; 4]])
                .unwrap_err()
                .contains("write export bus 2")
        );
        assert!(!destination.exists());
        let mut truncated = Bundle::create(&destination, &names(), 48000, 1).unwrap();
        truncated.write(&[[1.; 4]]).unwrap();
        OpenOptions::new()
            .write(true)
            .open(&truncated.partial[1])
            .unwrap()
            .set_len(HEADER_BYTES)
            .unwrap();
        assert!(truncated.finish().unwrap_err().contains("length changed"));
        assert!(!destination.exists());
    }

    #[test]
    fn publication_conflict_preserves_external_file_and_removes_own_links() {
        let root = Root::new();
        let destination = root.destination();
        let mut bundle = Bundle::create(&destination, &names(), 48000, 1).unwrap();
        bundle.write(&[[1., 2., 3., 4.]]).unwrap();
        let conflict = destination.join("FF.wav");
        fs::write(&conflict, b"external").unwrap();
        let error = bundle.finish().unwrap_err();
        assert!(error.contains("without replacing"));
        assert!(error.contains("cleanup incomplete"));
        drop(bundle);
        assert_eq!(fs::read(&conflict).unwrap(), b"external");
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
    }

    #[test]
    fn replaced_staging_file_is_not_deleted_as_owned() {
        let root = Root::new();
        let destination = root.destination();
        let mut bundle = Bundle::create(&destination, &names(), 48000, 1).unwrap();
        bundle.write(&[[0.; 4]]).unwrap();
        let replaced = destination.join(".S.wav.partial");
        fs::remove_file(&replaced).unwrap();
        fs::write(&replaced, b"external replacement").unwrap();
        let error = bundle.finish().unwrap_err();
        assert!(error.contains("identity changed"));
        drop(bundle);
        assert_eq!(fs::read(&replaced).unwrap(), b"external replacement");
        assert_eq!(fs::read_dir(&destination).unwrap().count(), 1);
    }

    #[test]
    fn replaced_directory_is_preserved_and_cleanup_is_reported() {
        let root = Root::new();
        let destination = root.destination();
        let mut bundle = Bundle::create(&destination, &names(), 48000, 1).unwrap();
        let moved = root.0.join("moved");
        fs::rename(&destination, &moved).unwrap();
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("external"), b"keep").unwrap();
        assert!(
            bundle
                .abort()
                .unwrap_err()
                .contains("directory identity changed")
        );
        drop(bundle);
        assert_eq!(fs::read(destination.join("external")).unwrap(), b"keep");
        assert_eq!(fs::read_dir(moved).unwrap().count(), 4);
    }
}
