//! Incremental, non-real-time Cardinal four-stem export. A frozen document
//! owns each job; neither the monitoring master nor environment audio is read.
use super::{
    document::{Content, State as Document},
    export_wave::Bundle,
    playback,
};
use serde_json::{Value, json};
use std::path::PathBuf;

const BLOCK: u64 = 4096;
// Match the shared native transport's explicit built-in instrument selection.
const INSTRUMENT: &str = "cardinal_sine";
struct Job {
    snapshot: Box<Document>,
    writer: Bundle,
    rate: u32,
    start: u64,
    end: u64,
    cursor: u64,
}
pub(super) struct State {
    job: Option<Job>,
    report: Value,
    next: u64,
}
impl Default for State {
    fn default() -> Self {
        Self {
            job: None,
            report: json!({"phase":"idle","progress":0.}),
            next: 0,
        }
    }
}
fn seconds(input: &Value, key: &str) -> Result<f64, String> {
    input[key]
        .as_f64()
        .filter(|v| v.is_finite() && (0.0..=86400.).contains(v))
        .ok_or_else(|| format!("{key} must be finite elapsed seconds within 0..86400"))
}
fn names(input: &Value) -> Result<[String; 4], String> {
    let defaults = ["master_L", "master_S", "master_F", "master_R"];
    let Some(values) = input.get("stem_names") else {
        return Ok(defaults.map(str::to_owned));
    };
    let values = values
        .as_array()
        .filter(|v| v.len() == 4)
        .ok_or("four stem names are required in L/S/FF/R order")?;
    let mut result = defaults.map(str::to_owned);
    for (i, value) in values.iter().enumerate() {
        let name = value
            .as_str()
            .filter(|s| s.chars().count() <= 48)
            .ok_or("stem names must be text of at most 48 characters")?;
        let clean: String = name
            .trim()
            .chars()
            .map(|c| {
                if c.is_control() || "/\\:*?\"<>|".contains(c) {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let clean = clean.trim_matches(|c| c == ' ' || c == '.');
        result[i] = if clean.is_empty() {
            format!("stem_{}", i + 1)
        } else {
            clean.to_owned()
        };
    }
    Ok(result)
}
fn preflight(snapshot: &Document, rate: u32, start: u64, end: u64) -> Result<(), String> {
    let solo = snapshot.tracks.values().any(|t| t.kind != "cc" && t.solo);
    for id in &snapshot.clip_order {
        let clip = &snapshot.clips[id];
        let track = &snapshot.tracks[&clip.track];
        if track.mute || (solo && !track.solo) {
            continue;
        }
        let first = (clip.start * rate as f64).round() as u64;
        let last = first + ((clip.duration() * rate as f64).round() as u64).max(1);
        if matches!(clip.content, Content::Audio { .. }) && first < end && last > start {
            // Same production validator as playback, including the full source
            // extent. Validate every contributing region before creating files.
            playback::render(
                snapshot,
                &json!({"sample_rate":rate,
                "start_frame":first.max(start),"frames":1,"instrument":INSTRUMENT}),
            )?;
        }
    }
    Ok(())
}
impl State {
    pub fn active(&self) -> bool {
        self.job.is_some()
    }
    pub fn status(&self) -> Value {
        self.report.clone()
    }
    fn token(&self, input: &Value) -> Result<(), String> {
        if input["job_id"].as_u64() != self.report["job_id"].as_u64()
            || input["job_id"].as_u64().is_none()
        {
            return Err("stale or missing export job_id".into());
        }
        Ok(())
    }
    pub fn begin(&mut self, snapshot: Box<Document>, input: &Value) -> Result<Value, String> {
        if self.active() {
            return Err("an export is already running".into());
        }
        let rate = input["sample_rate"]
            .as_u64()
            .filter(|r| (8000..=192000).contains(r))
            .ok_or("sample_rate must be an integer within 8000..192000")? as u32;
        let start = (seconds(input, "start_seconds")? * rate as f64).round() as u64;
        let end = (seconds(input, "end_seconds")? * rate as f64).round() as u64;
        if end <= start {
            return Err("Export end must be after start by at least one audio sample".into());
        }
        let destination = PathBuf::from(
            input["destination"]
                .as_str()
                .filter(|p| !p.is_empty())
                .ok_or("export destination required")?,
        );
        if !destination.is_absolute() {
            return Err("export destination must be an absolute path to a new folder".into());
        }
        let names = names(input)?;
        preflight(&snapshot, rate, start, end)?;
        let next = self
            .next
            .checked_add(1)
            .ok_or("export job counter exhausted")?;
        let writer = Bundle::create(&destination, &names, rate, end - start)?;
        self.next = next;
        self.report = json!({"job_id":self.next,"phase":"rendering","progress":0.,
            "document_revision":snapshot.revision,"destination":destination,"stem_names":names,
            "sample_rate":rate,"start_frame":start,"end_frame":end,"frames_written":0,
            "instrument":INSTRUMENT,
            "bus_order":["L","S","FF","R"],"format":"mono_float32_wav","export_tap":"pre_stereo_master",
            "files":[],"error":Value::Null});
        self.job = Some(Job {
            snapshot,
            writer,
            rate,
            start,
            end,
            cursor: start,
        });
        Ok(self.status())
    }
    pub fn step(&mut self, input: &Value) -> Result<Value, String> {
        self.token(input)?;
        let Some(mut job) = self.job.take() else {
            return Ok(self.status());
        };
        let result = (|| -> Result<Option<[PathBuf; 4]>, String> {
            let count = BLOCK.min(job.end - job.cursor);
            let block = playback::render(
                &job.snapshot,
                &json!({"sample_rate":job.rate,
                "stem_export":true,"start_frame":job.cursor,"frames":count,"instrument":INSTRUMENT}),
            )?;
            if block["audio_complete"] != true {
                return Err("MIDI instrument rendering is incomplete".into());
            }
            let frames: Vec<[f32; 4]> =
                serde_json::from_value(block["lsffr"].clone()).map_err(|e| e.to_string())?;
            job.writer.write(&frames)?;
            job.cursor += count;
            self.report["frames_written"] = json!(job.cursor - job.start);
            // Completion includes checked file flush/publication, not only rendering.
            self.report["progress"] =
                json!(((job.cursor - job.start) as f64 / (job.end - job.start) as f64).min(0.99));
            if job.cursor == job.end {
                Ok(Some(job.writer.finish()?))
            } else {
                Ok(None)
            }
        })();
        match result {
            Ok(Some(files)) => {
                self.report["phase"] = json!("completed");
                self.report["progress"] = json!(1.);
                self.report["files"] = json!(files);
            }
            Ok(None) => self.job = Some(job),
            Err(error) => {
                let cleanup = job.writer.abort();
                self.report["phase"] = json!("failed");
                self.report["error"] = json!(match cleanup {
                    Ok(()) => error,
                    Err(e) => format!("{error}; cleanup incomplete: {e}"),
                });
            }
        }
        Ok(self.status())
    }
    pub fn cancel(&mut self, input: &Value) -> Result<Value, String> {
        self.token(input)?;
        if let Some(mut job) = self.job.take() {
            match job.writer.abort() {
                Ok(()) => self.report["phase"] = json!("cancelled"),
                Err(error) => {
                    self.report["phase"] = json!("failed");
                    self.report["error"] =
                        json!(format!("Export cancelled; cleanup incomplete: {error}"));
                }
            }
        }
        Ok(self.status())
    }
}
