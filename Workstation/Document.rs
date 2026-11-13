use super::{automation, playback, routing::Route};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

const MAX_SECONDS: f64 = 86_400.;
const MAX_NOTES: usize = 8192;

#[derive(Clone)]
pub(super) struct Note {
    pub bend: f64,
    pub id: String,
    pub quarter: f64,
    pub length: f64,
    pub pitch: u8,
    pub tuning_cents: f64,
    pub velocity: f64,
}
#[derive(Clone)]
pub(super) enum Content {
    Audio {
        source: String,
        offset: f64,
        duration: f64,
    },
    Midi {
        edo: u16,
        scale: u8,
        root: u8,
        bpm: f64,
        quarters: f64,
        numerator: u64,
        denominator: u64,
        notes: Vec<Note>,
    },
    Cc {
        duration: f64,
        points: Vec<automation::Point>,
    },
}
#[derive(Clone)]
pub(super) struct Clip {
    pub output: String,
    pub lane: f64,
    pub id: String,
    pub track: String,
    pub start: f64,
    pub gain:f64,
    pub fade_in:f64,
    pub fade_out:f64,
    pub content: Content,
}
impl Clip {
    pub(super) fn duration(&self) -> f64 {
        match &self.content {
            Content::Audio { duration, .. } => *duration,
            Content::Midi { bpm, quarters, .. } => quarters * 60. / bpm,
            Content::Cc { duration, .. } => *duration,
        }
    }
    fn note_count(&self) -> usize {
        match &self.content {
            Content::Midi { notes, .. } => notes.len(),
            _ => 0,
        }
    }
    fn point_count(&self) -> usize {
        match &self.content {
            Content::Cc { points, .. } => points.len(),
            _ => 0,
        }
    }
    pub(super) fn snapshot(&self) -> Value {
        let mut value = json!({"id":self.id,"track":self.track,"start_seconds":self.start,
            "duration_seconds":self.duration(),"gain":self.gain,"output":self.output,"lane":if self.lane.fract()==0.{json!(self.lane as u64)}else{json!(self.lane)},"trackless":self.track.is_empty()});
        match &self.content {
            Content::Cc {
                duration: _,
                points,
            } => {
                value["kind"] = json!("cc");
                value["points"] = json!(
                    points
                        .iter()
                        .map(|p| json!({
                            "offset_seconds":p.offset_seconds,"value":p.value
                        }))
                        .collect::<Vec<_>>()
                );
            }
            Content::Audio { source, offset, .. } => {
                value["kind"] = json!("audio");
                value["fade_in_seconds"]=json!(self.fade_in);
                value["fade_out_seconds"]=json!(self.fade_out);
                value["source"] = json!(source);
                value["source_offset_seconds"] = json!(offset);
            }
            Content::Midi {
                edo, scale, root,
                bpm,
                quarters,
                numerator,
                denominator,
                notes,
            } => {
                value["kind"] = json!("midi");
                value["edo"] = json!(edo);value["scale"] = json!(scale);value["root"] = json!(root);
                value["bpm"] = json!(bpm);
                value["length_quarters"] = json!(quarters);
                value["meter"] = json!({"numerator":numerator,"denominator":denominator});
                value["notes"] = json!(
                    notes
                        .iter()
                        .map(|n| super::note_bend::write(json!({"id":n.id,
                    "quarter":n.quarter,"length_quarters":n.length,"pitch":n.pitch,
                    "velocity":n.velocity,"tuning_cents":n.tuning_cents,"bend_semitones":n.bend,"time_seconds":self.start+n.quarter*60./bpm,
                    "duration_seconds":n.length*60./bpm}),n.bend))
                        .collect::<Vec<_>>()
                );
            }
        }
        value
    }
}

#[derive(Clone)]
pub(super) struct Track {
    pub output: String,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub gain: f32,
    pub mute: bool,
    pub solo: bool,
    pub default_sound_muted: bool,
    pub route: Route,
    pub io: super::track_io::TrackIo,
    // First built-in target. Hosted device parameters remain a later adapter.
    pub gain_target: Option<String>,
    pub cc_lane: super::track_io::CcLane,
    pub cc_device_slot: usize,
    pub cc_parameter_slot: usize,
}
impl Track {
    fn snapshot(&self) -> Value {
        let mut value = json!({"id":self.id,"name":self.name,"kind":self.kind,"output":self.output});
        if self.kind == "cc" {
            value["arm_mode"]=json!(self.io.arm_mode);
            value["cc_target_lane"]=self.cc_lane.snapshot();
            value["cc_target_device_label"]=json!("NONE");
            value["cc_target_parameter_label"]=json!("NONE");
            value["target"] = self
                .gain_target
                .as_ref()
                .map_or(Value::Null, |id| json!({"track":id,"parameter":"gain"}));
        } else {
            value["gain"] = json!(self.gain);
            value["mute"] = json!(self.mute);
            value["solo"] = json!(self.solo);
            value["default_sound_muted"] = json!(self.default_sound_muted);
            for (key,field) in self.io.snapshot().as_object().unwrap() { if self.kind=="audio" || matches!(key.as_str(),"arm_mode"|"input_monitor_mode") {value[key]=field.clone();} }
            let routes = self.route.snapshot();
            value["output_bus_l"] = routes["output_bus_l"].clone();
            value["output_bus_r"] = routes["output_bus_r"].clone();
        }
        value
    }
}

/// Typed equivalent of owned Snapshot: heavy revision-owned document data is
/// shared; plugin/analysis/visualization metadata is fresh on every invocation.
pub(crate) struct SharedSnapshot {
    pub document:Arc<Value>,
    pub live:Value,
}
#[cfg(test)] impl SharedSnapshot {
    fn owned(&self)->Value {let mut value=(*self.document).clone();value.as_object_mut().unwrap().extend(self.live.as_object().unwrap().clone());value}
}
#[derive(Default)]
pub(crate) struct State {
    pub(crate) plugins: crate::base_system::layer_one::plugin_connection_system::Connection,
    pub(super) revision: u64,
    pub(super) timeline_zero_seconds:f64,
    pub(super) musical_time:super::musical_time::Timing,
    pub(super) undo: Box<super::undo::Journal>,
    pub(super) fade_history:super::clip_fades::History,
    pub(super) retrospective_insert: super::retrospective_insert::State,
    pub(super) tracks: BTreeMap<String, Track>,
    pub(super) track_order: Vec<String>,
    pub(super) buses: super::buses::Buses,
    pub(super) clips: BTreeMap<String, Clip>,
    pub(super) clip_order: Vec<String>,
    pub(super) sources: BTreeMap<String, playback::Source>,
    pub(super) warp: super::warp_treatment::Engine,
    pub(super) clip_treatments: BTreeMap<String, super::clip_treatment::Treatment>,
    pub(super) transient_cache:std::cell::RefCell<super::source_transients::Cache>,
    pub(super) recording: super::recording::State,
    pub(super) drum_rack: super::drum_rack::Rack,
    pub(super) audio_playlists:super::audio_playlists::State,
    master_regions: Vec<Value>,
    capture_epoch: Option<String>,
    analysis_generation: u64,
    analyzer: Option<super::rhythm::Analyzer>,
    export: super::export::State,
    pub(crate) freeze:super::track_freeze::State,
    pub(super) mix_preview:super::mix_preview::State,
    visualization: Option<Value>,
    visualization_sequence: u64,
    snapshot_cache:Option<(u64,Arc<Value>)>,
}

fn name(input: &Value, key: &str) -> Result<String, String> {
    input[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128 && s.chars().all(|c| !c.is_control()))
        .map(str::to_owned)
        .ok_or_else(|| format!("{key} requires a nonempty name of at most 128 bytes"))
}
fn number(input: &Value, key: &str, min: f64, max: f64) -> Result<f64, String> {
    input[key]
        .as_f64()
        .filter(|v| v.is_finite() && *v >= min && *v <= max)
        .ok_or_else(|| format!("{key} must be finite and in {min}..={max}"))
}
fn fits(clip: &Clip) -> Result<(), String> {
    if clip.start + clip.duration() > MAX_SECONDS {
        return Err("clip exceeds the 24-hour document bound".into());
    }
    Ok(())
}
fn parse_clip(input: &Value) -> Result<Clip, String> {
    let id = name(input, "id")?;
    let track = if input["track"].is_null() || input["track"]=="" {String::new()} else {name(input,"track")?};
    let start = number(input, "start_seconds", 0., MAX_SECONDS)?;
    let content = match input["kind"].as_str() {
        Some("cc") => {
            let duration = number(input, "duration_seconds", 0.000001, MAX_SECONDS)?;
            let points = automation::parse(&input["points"], duration)?;
            Content::Cc { duration, points }
        }
        Some("audio") => {
            let source = name(input, "source")?;
            let offset = number(input, "source_offset_seconds", 0., MAX_SECONDS)?;
            let duration = number(input, "duration_seconds", 0.000001, MAX_SECONDS)?;
            if offset + duration > MAX_SECONDS {
                return Err("audio source range exceeds bound".into());
            }
            Content::Audio {
                source,
                offset,
                duration,
            }
        }
        Some("midi") => {
            let edo=super::piano_tuning::edo(&input["edo"])?;
            let scale=input["scale"].as_u64().unwrap_or(0).min(6)as u8;
            let root=input["root"].as_u64().unwrap_or(0).min(11)as u8;
            let bpm = number(input, "bpm", 1., 1000.)?;
            let quarters = number(input, "length_quarters", 0.000001, 65536.)?;
            let (numerator, denominator) = if input.get("meter").is_none() {
                (4, 4)
            } else {
                let m = &input["meter"];
                let n = m["numerator"]
                    .as_u64()
                    .filter(|v| (1..=32).contains(v))
                    .ok_or("invalid MIDI meter numerator")?;
                let d = m["denominator"]
                    .as_u64()
                    .filter(|v| [1, 2, 4, 8, 16, 32].contains(v))
                    .ok_or("invalid MIDI meter denominator")?;
                (n, d)
            };
            let source = input["notes"]
                .as_array()
                .filter(|n| n.len() <= 2048)
                .ok_or("notes must be an array of at most 2048 notes")?;
            let mut ids = BTreeSet::new();
            let mut notes = Vec::with_capacity(source.len());
            for n in source {
                let id = name(n, "id")?;
                if !ids.insert(id.clone()) {
                    return Err("duplicate note id".into());
                }
                let quarter = number(n, "quarter", 0., quarters)?;
                let length = number(n, "length_quarters", 0.000001, quarters)?;
                if quarter + length > quarters + 1e-9 {
                    return Err("note extends beyond its piano roll".into());
                }
                let pitch = n["pitch"]
                    .as_u64()
                    .filter(|v| *v <= 127)
                    .ok_or("MIDI pitch must be 0..127")? as u8;
                let velocity = number(n, "velocity", 0., 1.)?;
                let tuning_cents=super::piano_tuning::cents(&n["tuning_cents"],pitch)?;
                notes.push(Note {
                    bend: super::note_bend::read(&n["bend_semitones"])?,
                    tuning_cents,
                    id,
                    quarter,
                    length,
                    pitch,
                    velocity,
                });
            }
            notes.sort_by(|a, b| a.quarter.total_cmp(&b.quarter).then(a.id.cmp(&b.id)));
            Content::Midi { edo, scale, root,
                bpm,
                quarters,
                numerator,
                denominator,
                notes,
            }
        }
        _ => return Err("clip kind must be audio, midi or cc".into()),
    };
    let clip = Clip {
        output: input["output"].as_str().unwrap_or("OFF").into(),
        lane: input.get("lane").map(|_|number(input,"lane",0.,511.)).transpose()?.unwrap_or(0.),
        gain: input.get("gain").map(|_|number(input,"gain",0.,16.)).transpose()?.unwrap_or(1.),
        fade_in: input.get("fade_in_seconds").map(|_|number(input,"fade_in_seconds",0.,MAX_SECONDS)).transpose()?.unwrap_or(0.),
        fade_out: input.get("fade_out_seconds").map(|_|number(input,"fade_out_seconds",0.,MAX_SECONDS)).transpose()?.unwrap_or(0.),
        id,
        track,
        start,
        content,
    };
    super::clip_fades::validate(&clip)?;
    fits(&clip)?;
    Ok(clip)
}

impl State {
    pub(super) fn retrospective_changed(&mut self){self.changed();}

    pub(crate) fn install_warp_clock(&mut self,clock:Arc<dyn super::warp_treatment::Clock>)->Result<(),String>{super::warp_treatment::install_clock(self,clock)?;self.changed();Ok(())}
    fn clip_snapshot(&self,id:&str)->Value{let mut v=self.clips[id].snapshot();if let Some(e)=self.warp.entries.get(id){v["warp_enabled"]=json!(e.map.enabled);}
        if let Content::Audio { source, offset, duration } = &self.clips[id].content {
            if let Some(audio) = self.sources.get(source) {
                v["rainbow_waveform"] = super::rainbow_waveform::analyze(audio, *offset, *duration);
            }
        }
        v}

    pub(crate) fn plugin_changed(&mut self) { self.changed(); }
    pub(super) fn recording_changed(&mut self) {self.changed();}
    pub(crate) fn export_active(&self) -> bool {
        self.export.active()
    }
    // Immutable render input for an export. PCM is shared by Arc; source
    // replacement in the editing document cannot alter an in-flight export.
    pub(super) fn freeze_for_export(&self) -> Box<Self> {
        Box::new(Self {
            revision: self.revision,
            timeline_zero_seconds:self.timeline_zero_seconds,
            musical_time:self.musical_time.clone(),
            drum_rack:self.drum_rack.frozen(),
            tracks: self.tracks.clone(),
            track_order: self.track_order.clone(),
            buses:self.buses.clone(),
            clips: self.clips.clone(),
            clip_order: self.clip_order.clone(),
            sources: self.sources.clone(),
            clip_treatments: self.clip_treatments.clone(),
            warp:self.warp.frozen(),
            ..Self::default()
        })
    }
    pub(super) fn changed(&mut self) -> Value {
        // Any edit can change the audible master mix; stale guides must disappear.
        self.revision += 1;
        self.master_regions.clear();
        self.capture_epoch = None;
        self.analyzer = None;
        self.visualization = None;
        json!({"revision":self.revision})
    }
    pub(super) fn require_revision(&self, input: &Value) -> Result<(), String> {
        if input["document_revision"].as_u64() != Some(self.revision) {
            return Err("stale or missing document_revision".into());
        }
        Ok(())
    }
    // Public JSON Snapshot stays owned. The typed adapter can share its immutable
    // equivalent while checking fields that legitimately change without revision.
    fn snapshot_metadata(&self)->Value {json!({"revision":self.revision,
        "drum_rack":self.drum_rack.snapshot(),"plugins":self.plugins.status(),"rhythm_regions":self.master_regions,
        "capture_epoch":self.capture_epoch,"analysis_generation":self.analysis_generation,
        "undo":self.undo.status(),"visualization":self.visualization,"recording":self.recording.status(),"warp":super::warp_treatment::snapshot(self)})}
    pub(crate) fn shared_snapshot(&mut self)->SharedSnapshot {
        let live=self.snapshot_metadata();
        let document=match &self.snapshot_cache {
            Some((revision,value)) if *revision==self.revision=>Arc::clone(value),
            _=>{let value=Arc::new(self.snapshot_document());self.snapshot_cache=Some((self.revision,Arc::clone(&value)));value}
        };
        SharedSnapshot{document,live}
    }
    fn track_snapshot(&self,id:&str)->Value{
        let track=&self.tracks[id];let mut value=track.snapshot();value["frozen"]=json!(self.freeze.frozen(id));
        if track.kind=="cc" {
            match if track.cc_lane==super::track_io::CcLane::Workspace{Ok(crate::theatre::lighting::catalog().clone())}else{self.plugins.automation_catalog(false)}{
                Ok(catalog)=>{value.as_object_mut().unwrap().extend(super::cc_automation::resolved(track,&catalog).as_object().unwrap().clone());},
                Err(error)=>{value["cc_target_error"]=json!(error);}
            }
        }value
    }
    fn snapshot_document(&self)->Value {
        json!({"schema_version":1,"revision":self.revision,"time_basis":"seconds","timeline_zero_seconds":self.timeline_zero_seconds,"musical_time":self.musical_time.snapshot(),
            "tracks":self.track_order.iter().map(|id|self.track_snapshot(id)).collect::<Vec<_>>(),
            "clips":self.clip_order.iter().map(|id|self.clip_snapshot(id)).collect::<Vec<_>>(),
            "drum_rack":self.drum_rack.snapshot(),"audio_playlists":self.audio_playlists.snapshot(self),
            "sources":self.sources.iter().map(|(id,s)|s.snapshot(id)).collect::<Vec<_>>(),
            "clip_treatments":super::clip_treatment::snapshot(self),
            "buses":self.buses.snapshot(),"physical_outputs":super::buses::physical_outputs(),
            "bus_order":["L","S","FF","R"],"export_tap":"pre_stereo_master","rhythm_source":"master_bus"})
    }
    fn snapshot(&self) -> Value {
        json!({"schema_version":1,"revision":self.revision,"time_basis":"seconds","timeline_zero_seconds":self.timeline_zero_seconds,"musical_time":self.musical_time.snapshot(),
            "plugins":self.plugins.status(),"tracks":self.track_order.iter().map(|id|self.track_snapshot(id)).collect::<Vec<_>>(),
            "clips":self.clip_order.iter().map(|id|self.clip_snapshot(id)).collect::<Vec<_>>(),
            "drum_rack":self.drum_rack.snapshot(),
            "sources":self.sources.iter().map(|(id,s)|s.snapshot(id)).collect::<Vec<_>>(),
            "clip_treatments":super::clip_treatment::snapshot(self),
            "buses":self.buses.snapshot(),"physical_outputs":super::buses::physical_outputs(),
            "bus_order":["L","S","FF","R"],"export_tap":"pre_stereo_master",
            "rhythm_source":"master_bus","rhythm_regions":self.master_regions,
            "capture_epoch":self.capture_epoch,"analysis_generation":self.analysis_generation,
            "undo":self.undo.status(),"visualization":self.visualization,"recording":self.recording.status(),"audio_playlists":self.audio_playlists.snapshot(self),"warp":super::warp_treatment::snapshot(self)})
    }

    /// Cardinal DawLaneShortcutSystem::createMidiClipsFromTimelineSelection /
    /// createAutomationClipsFromTimelineSelection: each kind preflights all its
    /// tracks before mutation; MIDI and CC may succeed independently.
    fn create_range_clips(&mut self, input: &Value) -> Result<Value, String> {
        let first = name(input, "first_track")?;
        let last = name(input, "last_track")?;
        let first_index = self.track_order.iter().position(|id| id == &first)
            .ok_or("range first_track is unknown")?;
        let last_index = self.track_order.iter().position(|id| id == &last)
            .ok_or("range last_track is unknown")?;
        let a = number(input, "start_seconds", 0., MAX_SECONDS)?;
        let b = number(input, "end_seconds", 0., MAX_SECONDS)?;
        let start = a.min(b);
        let end = a.max(b);
        let duration = end - start;
        if duration < 0.000001 {
            return Err("range duration must be at least 0.000001 seconds".into());
        }
        let tracks = self.track_order[first_index.min(last_index)..=first_index.max(last_index)].to_vec();
        if !tracks.iter().any(|id| matches!(self.tracks[id].kind.as_str(), "midi" | "cc")) {
            return Err("range contains no MIDI or CC tracks".into());
        }
        let mut created = Vec::<String>::new();
        let mut selected_clip = None::<String>;
        let mut rejected = Vec::<Value>::new();
        for kind in ["midi", "cc"] {
            let targets = tracks.iter().filter(|id| self.tracks[*id].kind == kind)
                .cloned().collect::<Vec<_>>();
            if targets.is_empty() { continue; }
            let staged = (|| -> Result<Vec<Clip>, String> {
                if self.clips.len()+self.audio_playlists.extra_count() + targets.len() > 512 {
                    return Err("clip capacity reached for the complete selected track group".into());
                }
                for clip in self.clips.values() {
                    if targets.contains(&clip.track)
                        && clip.start < end
                        // MIDI quarter conversion can put a mathematically
                        // touching computed end1-2ULPs above the requested start.
                        // Only the computed end gets this roundoff allowance.
                        && clip.start + clip.duration() > f64::from_bits(start.to_bits()+2) {
                        return Err(format!("range overlaps clip {} on track {}", clip.id, clip.track));
                    }
                }
                let mut used = self.clips.keys().cloned().chain(self.audio_playlists.extra_ids()).collect::<BTreeSet<_>>();
                let mut staged = Vec::with_capacity(targets.len());
                for track in &targets {
                    // Bounded search: among N+1 candidates one is absent from
                    // N existing/staged identities. Existing IDs never change.
                    let id = (1..=used.len() + 1).map(|n| format!("range-clip-{n}"))
                        .find(|id| !used.contains(id)).unwrap();
                    used.insert(id.clone());
                    let mut request = json!({"id":id,"track":track,"kind":kind,"start_seconds":start});
                    if kind == "midi" {
                        request["bpm"] = json!(120.);
                        request["length_quarters"] = json!(duration * 2.);
                        request["meter"] = json!({"numerator":4,"denominator":4});
                        request["notes"] = json!([]);
                    } else {
                        request["duration_seconds"] = json!(duration);
                        request["points"] = json!([]);
                    }
                    // Preserve normal bounds and parsing. Long MIDI ranges
                    // that exceed the local quarter limit reject, never clamp.
                    staged.push(parse_clip(&request)?);
                }
                Ok(staged)
            })();
            match staged {
                Err(reason) => rejected.push(json!({"kind":kind,"reason":reason})),
                Ok(clips) => {
                    selected_clip = clips.first().map(|clip| clip.id.clone());
                    for clip in clips {
                        created.push(clip.id.clone());
                        self.clip_order.push(clip.id.clone());
                        if self.warp.entries.get(&clip.id).is_some_and(|e|!matches!(&clip.content,Content::Audio{source,..} if *source==e.source_id)){self.warp.entries.remove(&clip.id);}
        self.clips.insert(clip.id.clone(), clip);
                    }
                    let _ = self.changed();
                }
            }
        }
        Ok(json!({"revision":self.revision,"created":created,"selected_clip":selected_clip,
            "rejected":rejected,"start_seconds":start,"end_seconds":end}))
    }

    /// Decode first, validate every limit, then publish source and clip together.
    fn import_audio(&mut self, input: &Value) -> Result<Value, String> {
        let track = if input["track"].is_null() || input["track"]=="" {String::new()} else {name(input,"track")?};
        if !track.is_empty() && self.tracks.get(&track).is_none_or(|t| t.kind != "audio") {
            return Err("Drop audio files onto an audio lane".into());
        }
        let lane=input.get("comp_lane").map(|v|v.as_u64().and_then(|n|usize::try_from(n).ok()).ok_or("invalid comp lane")).transpose()?.unwrap_or(0);
        if (track.is_empty() && lane>0) || lane>self.audio_playlists.lane_count(&track){return Err("unknown comp lane".into());}
        if self.recording.busy(&track){return Err("audio track is busy recording".into());}
        let start = number(input, "start_seconds", 0., MAX_SECONDS)?;
        let path = input["path"].as_str().filter(|p| !p.is_empty() && p.len() <= 4096).ok_or("WAV path missing or too long")?;
        let rate = input["sample_rate"].as_u64().filter(|r| (8000..=192000).contains(r)).ok_or("invalid audio import sample rate")?;
        if self.clips.len()+self.audio_playlists.extra_count() >= 512 || self.sources.len() >= 256 { return Err("audio import document capacity reached".into()); }
        let source = super::drum_sample::read_timeline(std::path::Path::new(path))?;
        if u64::from(source.sample_rate) != rate {
            return Err(format!("WAV is {} Hz; playback is {rate} Hz. Convert the WAV to {rate} Hz before importing.", source.sample_rate));
        }
        if self.sources.values().map(|s|s.samples.len()).sum::<usize>() + source.samples.len() > 16_777_216 {
            return Err("decoded source memory capacity reached".into());
        }
        let duration = (source.samples.len()/source.channels) as f64 / source.sample_rate as f64;
        let mut serial = self.revision;
        let (id, source_id) = loop {
            let id=format!("import-clip-{serial}"); let source_id=format!("import-source-{serial}");
            if !self.clips.contains_key(&id) && !self.audio_playlists.contains(&id) && !self.sources.contains_key(&source_id) {break (id,source_id);}
            serial+=1;
        };
        let clip=Clip{output:if track.is_empty(){super::channels::default_output(self)}else{"OFF".into()},lane:input.get("lane").map(|_|number(input,"lane",0.,511.)).transpose()?.unwrap_or(0.),gain:1., fade_in:0.,fade_out:0.,id:id.clone(),track,start,content:Content::Audio{source:source_id.clone(),offset:0.,duration}};
        fits(&clip)?;
        if lane>0{self.audio_playlists.append_clip(&clip.track.clone(),lane,clip)?;}else{self.clip_order.push(id.clone());self.clips.insert(id.clone(),clip);}
        self.sources.insert(source_id.clone(),source);
        let mut report=self.changed(); report["clip"]=json!(id); report["source"]=json!(source_id);
        Ok(report)
    }

    fn put_clip(&mut self, input: &Value) -> Result<Value, String> {
        let clip = parse_clip(input)?;
        let track = self.tracks.get(&clip.track);
        let kind = match &clip.content {
            Content::Audio { .. } => "audio",
            Content::Midi { .. } => "midi",
            Content::Cc { .. } => "cc",
        };
        if (clip.track.is_empty() && kind!="audio") || (!clip.track.is_empty() && track.is_none_or(|t|t.kind!=kind)) {
            return Err("clip kind must match its track".into());
        }
        super::channels::validate_clip(self,&clip)?;
        if self.audio_playlists.contains(&clip.id){return Err("clip ID belongs to a comp lane".into());}
        if self.clips.len()+self.audio_playlists.extra_count() >= 512 && !self.clips.contains_key(&clip.id) {
            return Err("clip capacity reached".into());
        }
        let old_count = self.clips.get(&clip.id).map_or(0, Clip::note_count);
        if self.clips.values().map(Clip::note_count).sum::<usize>() - old_count + clip.note_count()
            > MAX_NOTES
        {
            return Err("document note capacity reached".into());
        }
        let old_points = self.clips.get(&clip.id).map_or(0, Clip::point_count);
        if self.clips.values().map(Clip::point_count).sum::<usize>() - old_points
            + clip.point_count()
            > 8192
        {
            return Err("document CC point capacity reached".into());
        }
        if !self.clips.contains_key(&clip.id) {
            self.clip_order.push(clip.id.clone());
        }
        if self.clip_treatments.get(&clip.id).is_some_and(|t| !matches!(&clip.content, Content::Audio {source,..} if *source==t.source_id)) {
            self.clip_treatments.remove(&clip.id);
        }
        if self.warp.entries.get(&clip.id).is_some_and(|e|!matches!(&clip.content,Content::Audio{source,..} if *source==e.source_id)){self.warp.entries.remove(&clip.id);}
        self.clips.insert(clip.id.clone(), clip);
        Ok(self.changed())
    }
    fn snap(&self, input: &Value) -> Result<Value, String> {
        let id = name(input, "id")?;
        let clip = self.clips.get(&id).ok_or("unknown clip")?;
        let Content::Midi { bpm, notes, .. } = &clip.content else {
            return Err("SnapMidi requires a MIDI clip".into());
        };
        let proposed = number(input, "proposed_start_seconds", 0., MAX_SECONDS)?;
        let tolerance = number(input, "tolerance_seconds", 0., 1.)?;
        if proposed + clip.duration() > MAX_SECONDS {
            return Err("proposed clip exceeds document bound".into());
        }
        // The UI converts its pixel hit radius to seconds at the current zoom.
        let mut targets = Vec::new();
        for other in self.clips.values().filter(|c| c.id != id) {
            if let Content::Midi { bpm, notes, .. } = &other.content {
                for note in notes {
                    targets.push((other.start + note.quarter * 60. / bpm, &other.id, &note.id));
                }
            }
        }
        targets.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(b.1)).then(a.2.cmp(b.2)));
        let mut best: Option<(f64, f64, &str, &str, &str, f64)> = None;
        for note in notes {
            let onset = proposed + note.quarter * 60. / bpm;
            let index = targets.partition_point(|t| t.0 < onset);
            for candidate in [
                index.checked_sub(1),
                (index < targets.len()).then_some(index),
            ]
            .into_iter()
            .flatten()
            {
                let (time, target_clip, target_note) = targets[candidate];
                let delta = time - onset;
                let moved = proposed + delta;
                if delta.abs() > tolerance || moved < 0. || moved + clip.duration() > MAX_SECONDS {
                    continue;
                }
                let score = (
                    delta.abs(),
                    time,
                    target_clip.as_str(),
                    target_note.as_str(),
                    note.id.as_str(),
                    delta,
                );
                if best.as_ref().is_none_or(|b| score < *b) {
                    best = Some(score);
                }
            }
        }
        Ok(match best {
            Some((_, time, target_clip, target_note, source_note, delta)) => json!({
                "revision":self.revision,"snapped":true,"start_seconds":proposed+delta,
                "delta_seconds":delta,"source_note":source_note,"target_clip":target_clip,
                "target_note":target_note,"target_time_seconds":time,"bpm":bpm}),
            None => {
                json!({"revision":self.revision,"snapped":false,"start_seconds":proposed,"delta_seconds":0.,"bpm":bpm})
            }
        })
    }
    fn snap_clip(&self, input: &Value) -> Result<Value, String> {
        let id = name(input, "id")?;
        let clip = self.clips.get(&id).ok_or("unknown clip")?;
        let proposed = number(input, "proposed_start_seconds", 0., MAX_SECONDS)?;
        let tolerance = number(input, "tolerance_seconds", 0., 1.)?;
        if proposed + clip.duration() > MAX_SECONDS {
            return Err("proposed clip exceeds document bound".into());
        }
        let anchor = match input.get("anchor") {
            None => "start",
            Some(v) => v.as_str().ok_or("anchor must be start or notes")?,
        };
        let anchors: Vec<(f64, Option<&str>)> = match anchor {
            "start" => vec![(0., None)],
            "notes" => match &clip.content {
                Content::Midi { bpm, notes, .. } => notes
                    .iter()
                    .map(|n| (n.quarter * 60. / bpm, Some(n.id.as_str())))
                    .collect(),
                _ => return Err("note anchors require a piano-roll clip".into()),
            },
            _ => return Err("anchor must be start or notes".into()),
        };
        let mut targets = Vec::new();
        for other in self.clips.values().filter(|c| c.track != clip.track) {
            if let Content::Midi { bpm, notes, .. } = &other.content {
                for note in notes {
                    targets.push((
                        other.start + note.quarter * 60. / bpm,
                        other.id.as_str(),
                        note.id.as_str(),
                    ));
                }
            }
        }
        targets.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(b.1)).then(a.2.cmp(b.2)));
        let mut best: Option<(f64, f64, &str, &str, Option<&str>, f64)> = None;
        for (offset, source_note) in anchors {
            let onset = proposed + offset;
            let index = targets.partition_point(|t| t.0 < onset);
            for i in [
                index.checked_sub(1),
                (index < targets.len()).then_some(index),
            ]
            .into_iter()
            .flatten()
            {
                let (time, target_clip, target_note) = targets[i];
                let delta = time - onset;
                let moved = proposed + delta;
                if delta.abs() > tolerance || moved < 0. || moved + clip.duration() > MAX_SECONDS {
                    continue;
                }
                let score = (
                    delta.abs(),
                    time,
                    target_clip,
                    target_note,
                    source_note,
                    delta,
                );
                if best.as_ref().is_none_or(|old| score < *old) {
                    best = Some(score);
                }
            }
        }
        Ok(match best {
            Some((_, time, target_clip, target_note, source_note, delta)) => json!({
                "revision":self.revision,"snapped":true,"start_seconds":proposed+delta,
                "delta_seconds":delta,"source_note":source_note,"target_clip":target_clip,
                "target_note":target_note,"target_time_seconds":time}),
            None => {
                json!({"revision":self.revision,"snapped":false,"start_seconds":proposed,"delta_seconds":0.})
            }
        })
    }
    fn piano_roll(&self, input: &Value) -> Result<Value, String> {
        let id = name(input, "id")?;
        let clip = self.clips.get(&id).ok_or("unknown clip")?;
        let Content::Midi {
            bpm,
            quarters,
            numerator,
            denominator,
            ..
        } = &clip.content
        else {
            return Err("PianoRoll requires MIDI".into());
        };
        let step = number(input, "grid_quarters", 1. / 64., 4.)?;
        let first = number(input, "start_quarter", 0., *quarters)?;
        let last = number(input, "end_quarter", first, *quarters)?;
        let count = ((last - first) / step).ceil() as usize + 1;
        if count > 4096 {
            return Err("piano-roll grid viewport exceeds 4096 lines".into());
        }
        let bar = *numerator as f64 * 4. / *denominator as f64;
        let first_index = (first / step).ceil() as u64;
        let grid: Vec<_> = (0..count)
            .map(|i| (first_index + i as u64) as f64 * step)
            .take_while(|q| *q <= last + 1e-9)
            .map(|q| {
                json!({"quarter":q,"local_seconds":q*60./bpm,
                "bar_line":(q/bar - (q/bar).round()).abs()<1e-8})
            })
            .collect();
        Ok(json!({"clip":clip.snapshot(),"grid":grid,"grid_scope":"this_piano_roll_only"}))
    }
    fn begin_master(&mut self, input: &Value) -> Result<Value, String> {
        self.require_revision(input)?;
        let epoch = name(input, "capture_epoch")?;
        let rate = input["sample_rate"]
            .as_u64()
            .ok_or("integer sample_rate required")?;
        let channels = input["channels"]
            .as_u64()
            .filter(|c| *c == 1 || *c == 2)
            .ok_or("channels must be 1 or 2")?;
        let analyzer = super::rhythm::Analyzer::new(rate, channels as usize)?;
        let generation = self
            .analysis_generation
            .checked_add(1)
            .ok_or("analysis generation exhausted")?;
        self.analyzer = Some(analyzer);
        self.analysis_generation = generation;
        self.capture_epoch = Some(epoch);
        self.master_regions.clear();
        Ok(
            json!({"revision":self.revision,"capture_epoch":self.capture_epoch,"analysis_generation":generation}),
        )
    }
    fn analyze_master(&mut self, input: &Value) -> Result<Value, String> {
        self.require_revision(input)?;
        let epoch = name(input, "capture_epoch")?;
        let observed = number(input, "observed_at_seconds", 0., MAX_SECONDS)?;
        if self.capture_epoch.as_deref() != Some(epoch.as_str())
            || input["analysis_generation"].as_u64() != Some(self.analysis_generation)
        {
            return Err("inactive master capture epoch or analysis generation".into());
        }
        let mut analyzer = self
            .analyzer
            .as_ref()
            .ok_or("BeginMasterAnalysis is required")?
            .clone();
        let report = analyzer.push(input)?;
        if report["status"] == "gathering" {
            self.analyzer = Some(analyzer);
            return Ok(
                json!({"revision":self.revision,"analysis":report,"rhythm_regions":self.master_regions}),
            );
        }
        let regions = super::guides::reconcile(
            &self.master_regions,
            &report,
            self.analysis_generation,
            self.revision,
            observed,
        )?;
        self.master_regions = regions;
        self.analyzer = Some(analyzer);
        Ok(json!({"revision":self.revision,"analysis":report,"rhythm_regions":self.master_regions}))
    }
}

// Control-side atomic split. The source PCM stays shared and unchanged.
fn split_audio_clip(state:&mut State,input:&Value)->Result<Value,String>{
    let id=input["id"].as_str().ok_or("clip id required")?;
    let time=input["time_seconds"].as_f64().filter(|t|t.is_finite()).ok_or("finite split time required")?;
    let original=state.clips.get(id).ok_or("unknown clip")?.clone();
    let Content::Audio{source,offset,duration}=original.content.clone() else{return Err("Select an audio clip to split".into())};
    if state.recording.busy(&original.track){return Err("track is busy recording or awaiting commit".into());}
    let local=time-original.start;
    if local<=0. || local>=duration{return Err("Place the edit cursor inside the selected clip".into());}
    if state.clips.len()+state.audio_playlists.extra_count()>=512{return Err("clip capacity reached".into());}
    if state.warp.entries.get(id).is_some_and(|e|e.map.enabled){return Err("Disable clip warp before splitting".into());}
    if local<original.fade_in || duration-local<original.fade_out{return Err("Place the split outside the fade ramps".into());}
    state.revision.checked_add(1).ok_or("document revision exhausted")?;
    let right_id=(1u64..).map(|n|format!("{id}-split-{n}")).find(|name|!state.clips.contains_key(name)&&!state.audio_playlists.extra_ids().contains(name)).unwrap();
    let mut left=original.clone();let mut right=original;
    left.content=Content::Audio{source:source.clone(),offset,duration:local};left.fade_out=0.;
    right.id=right_id.clone();right.start=time;right.fade_in=0.;
    right.content=Content::Audio{source,offset:offset+local,duration:duration-local};
    if let Some(t)=state.clip_treatments.get(id).cloned(){state.clip_treatments.insert(right_id.clone(),t);}
    if let Some(e)=state.warp.entries.get(id).cloned(){state.warp.entries.insert(right_id.clone(),e);}
    let index=state.clip_order.iter().position(|name|name==id).ok_or("clip order missing")?;
    state.clip_order.insert(index+1,right_id.clone());
    state.clips.insert(id.to_owned(),left);state.clips.insert(right_id.clone(),right);
    let mut result=state.changed();result["selected_clip"]=json!(id);result["right_clip"]=json!(right_id);Ok(result)
}

pub(crate) fn dispatch(state: &mut State, hook: &str, input: &Value) -> Result<Value, String> {
    super::undo::dispatch(state, hook, input)
}

pub(super) fn dispatch_inner(state: &mut State, hook: &str, input: &Value) -> Result<Value, String> {
    if hook=="SetTrackMix" && state.buses.rows.iter().any(|b|b.kind=="channel"&&input["id"]==b.id){return super::buses::dispatch(state,"SetBus",input);}
    if hook.starts_with("TrackFreeze"){return super::track_freeze::dispatch(state,hook,input);}
    if matches!(hook,"PutBus"|"SetBus"|"RemoveBus"){return super::buses::dispatch(state,hook,input);}
    let read_only=matches!(hook,"MixPreview"|"StageLighting"|"Snapshot"|"RenderRange"|"ExportStatus"|"CaptureStatus"|"CaptureInput"|"PianoRoll"|"LocatePanel"|"ProjectPanel"|"PumpWarp");
    if hook=="CaptureInput" && state.freeze.busy() && input["recording"]==true{return Err("Finish or flatten frozen tracks before recording".into());}
    if hook=="SetTrackMix" && state.freeze.frozen(input["id"].as_str().unwrap_or("")) && ["arm_mode","input_monitor_mode","clear_midi_contents"].iter().any(|key|input.get(key).is_some()){return Err("Frozen tracks cannot record or monitor input".into());}
    if state.freeze.busy() && !read_only{return Err("Finish or cancel track freeze first".into());}
    if !read_only && !matches!(hook,"SetTrackMix"|"ReorderTracks"|"ExportBegin"|"ExportStep"|"ExportCancel"){
        let id=input["id"].as_str().unwrap_or("");
        let track=input["track"].as_str().or_else(||state.clips.get(id).map(|c|c.track.as_str())).unwrap_or(id);
        if state.freeze.frozen(track){return Err("Frozen track content is locked until flattening".into());}
        if matches!(hook,"RebaseTimeline"|"PutSource"|"SetMusicalTime"|"DrumRack") && state.freeze.any(){return Err("Flatten frozen tracks before changing shared source or timing data".into());}
    }
    if matches!(
        hook,
        "ImportAudio"
            | "PutTrack"
            | "RemoveTrack"
            | "PutClip"
            | "CreateRangeClips"
            | "RemoveClip"
            | "TrimAudioClip"
            | "SetClipGain"
            | "SetClipRouting"
            | "SplitClip"
            | "SetClipFades"
            | "MoveClip"
            | "RebaseTimeline"
            | "SetMusicalTime"
            | "SetMidiTempo"
            | "SetTrackMix"
            | "SetCcTarget"
            | "ReorderTracks"
            | "PutSource"
            | "RebaseTimeline" | "SetMusicalTime" | "SetClipWarp" | "SetWarpMap" | "AddSpectralMask" | "UndoSpectralMask" | "ClearSpectralMasks" | "ClipEditorPreview"
    ) {
        state.require_revision(input)?;
    }
    match hook {
        "MixPreview" => super::mix_preview::step(state,input),
        "TrimAudioClip"=>{
            let id=input["id"].as_str().ok_or("clip id required")?;
            let a=number(input,"start_fraction",0.,1.)?;let b=number(input,"end_fraction",0.,1.)?;
            if a>=b{return Err("Select a nonempty waveform range".into());}
            if state.warp.entries.get(id).is_some_and(|e|e.map.enabled){return Err("Disable warp before trimming".into());}
            let c=state.clips.get_mut(id).or_else(||state.audio_playlists.clip_mut(id)).ok_or("unknown clip")?;
            let Content::Audio{offset,duration,..}=&mut c.content else{return Err("waveform trim requires audio".into())};
            let removed=*duration*a;let length=*duration*(b-a);
            *offset+=removed;c.start+=removed;*duration=length;
            c.fade_in=(c.fade_in-removed).max(0.).min(length);c.fade_out=c.fade_out.min(length);
            Ok(state.changed())
        },
        "SetClipGain" => {
            let id=input["id"].as_str().ok_or("clip id required")?;
            let gain=number(input,"gain",0.,16.)?;
            let c=state.clips.get_mut(id).or_else(||state.audio_playlists.clip_mut(id)).ok_or("unknown audio clip")?;
            if !matches!(c.content,Content::Audio{..}){return Err("clip gain requires audio".into());}
            c.gain=gain;Ok(state.changed())
        },
        "SplitClip" => split_audio_clip(state,input),
        "SetClipFades" => super::clip_fades::edit(state,input),
        "ImportAudio" => state.import_audio(input),
        "DrumRack" => {state.require_revision(input)?;super::drum_rack::command(state,input)?;Ok(state.changed())},
        "BeginRetrospectiveAudio"|"AppendRetrospectiveAudio"|"CommitRetrospectiveAudio"|"AbortRetrospectiveAudio"|"InsertRetrospectiveMidi"|"UndoRetrospective"|"RedoRetrospective"=>super::retrospective_insert::dispatch(state,hook,input),
        "ExportBegin" => {
            if state.tracks.keys().any(|id|!state.freeze.frozen(id)&&state.plugins.has_track(id,false)) || state.buses.rows.iter().any(|b|b.kind=="channel"&&state.plugins.has_track(&b.id,false)) { return Err("Export with native plugins requires an isolated render instance; plugin state is not silently omitted".into()); }
            state.require_revision(input)?;
            if state.export.active() {
                return Err("an export is already running".into());
            }
            super::warp_treatment::ready_for_export(state)?;
            let snapshot = state.freeze_for_export();
            state.export.begin(snapshot, input)
        }
        "ExportStep" => state.export.step(input),
        "ExportCancel" => state.export.cancel(input),
        "ExportStatus" => Ok(state.export.status()),
        "StageLighting" => {
            let seconds=input["seconds"].as_f64().filter(|n|n.is_finite()).ok_or("finite lighting seconds required")?;
            Ok(json!({"levels":super::stage_lighting::evaluate(state,seconds)}))
        },
        "Snapshot" => Ok(state.snapshot()),
        "SetMusicalTime"=>{
            let next=state.musical_time.updated(input)?;
            if next==state.musical_time{return Ok(json!({"revision":state.revision,"musical_time":next.snapshot()}));}
            state.revision.checked_add(1).ok_or("document revision exhausted")?;
            let prepared=super::warp_timing::prepare(state,&next,state.timeline_zero_seconds,0.)?;
            super::warp_treatment::apply_clock_change(state,prepared);
            state.musical_time=next;let mut result=state.changed();result["musical_time"]=state.musical_time.snapshot();Ok(result)
        },
        "RebaseTimeline"=>{
            let shift=number(input,"shift_seconds",f64::MIN_POSITIVE,MAX_SECONDS)?;
            if state.timeline_zero_seconds+shift>MAX_SECONDS||state.clips.values().any(|c|c.start+c.duration()+shift>MAX_SECONDS){return Err("timeline rebase exceeds the document's one-day domain".into());}
            state.revision.checked_add(1).ok_or("document revision exhausted")?;
            let mut next=state.musical_time.clone();next.revision=next.revision.checked_add(1).ok_or("musical time revision exhausted")?;
            let zero=state.timeline_zero_seconds+shift;next.map.zero_sample=(zero*next.map.sample_rate).round()as i64;
            // Read old endpoints at OLD starts/origin before any mutation. The
            // preparation validates durations at candidate starts/new origin.
            let prepared=super::warp_timing::prepare(state,&next,zero,shift)?;
            state.audio_playlists.rebase(shift)?;
            for clip in state.clips.values_mut(){clip.start+=shift;}
            super::warp_treatment::apply_clock_change(state,prepared);
            state.timeline_zero_seconds=zero;state.musical_time=next;
            let mut result=state.changed();result["shift_seconds"]=json!(shift);result["timeline_zero_seconds"]=json!(zero);Ok(result)
        },
        "SetClipWarp"|"SetWarpMap"=>{super::warp_timing::edit(state,hook,input)?;Ok(state.changed())},
        "PumpWarp"=>{super::warp_timing::ensure(state)?;let changed=super::warp_treatment::pump(state);if changed{state.changed();}Ok(json!({"changed":changed,"warp":super::warp_treatment::snapshot(state)}))},
        "AddSpectralMask" | "UndoSpectralMask" | "ClearSpectralMasks" => {
            super::clip_treatment::edit(state,hook,input)?; Ok(state.changed())
        }
        "ClipEditorPreview" => super::clip_treatment::preview(state,input),
        "AudioPlaylist" => {state.require_revision(input)?;super::audio_playlists::command(state,input)},
        "CaptureStatus" => Ok(state.recording.status()),
        "CycleMidiTake" => {state.require_revision(input)?;super::midi_capture::cycle(state,input)},
        "CaptureInput" => {state.require_revision(input)?;super::recording::capture(state,input)},
        "CreateRangeClips" => state.create_range_clips(input),
        "RemoveTrack" => {
            let id=name(input,"id")?;
            if !state.tracks.contains_key(&id){return Err("unknown track".into());}
            if state.recording.busy(&id){return Err("Stop recording before deleting this track".into());}
            let removed:std::collections::BTreeSet<_>=state.clips.values().filter(|c|c.track==id).map(|c|c.id.clone()).chain(state.audio_playlists.track_clip_ids(&id)).collect();
            state.tracks.remove(&id);state.track_order.retain(|old|old!=&id);
            state.clips.retain(|key,_|!removed.contains(key));state.clip_order.retain(|key|!removed.contains(key));
            state.clip_treatments.retain(|key,_|!removed.contains(key));state.warp.entries.retain(|key,_|!removed.contains(key));
            state.audio_playlists.remove_track(&id);
            state.drum_rack.remove_track(&id,&removed);
            for track in state.tracks.values_mut(){
                if track.gain_target.as_deref()==Some(&id){track.gain_target=None;}
                if track.cc_lane==super::track_io::CcLane::Track(id.clone()){track.cc_lane=super::track_io::CcLane::None;}
            }
            // Keep source data and inactive plugin instances available for Undo.
            // Playback only visits live tracks; deletion never processes this chain.
            Ok(state.changed())
        }
        "PutTrack" => {
            let id = name(input, "id")?;
            if id==super::mix_preview::ID{return Err("identity is reserved for the read-only master preview".into());}
            let label = name(input, "name")?;
            if state.buses.contains(&id)||(!state.tracks.contains_key(&id)&&state.plugins.has_track(&id,false)){return Err("identity already belongs to a bus".into());}
            let kind = input["kind"]
                .as_str()
                .filter(|s| matches!(*s, "audio" | "midi" | "cc"))
                .ok_or("track kind must be audio, midi or cc")?;
            if state.tracks.len() >= 64 && !state.tracks.contains_key(&id) {
                return Err("track capacity reached".into());
            }
            if state.tracks.get(&id).is_some_and(|old| old.kind != kind) {
                return Err("track kind cannot change in place".into());
            }
            // A browser drop creates and orders the track in one revision.
            // Validate every insertion constraint before changing either owner.
            let insert_index=if input.get("insert_index").is_some(){
                if state.tracks.contains_key(&id){return Err("insert_index is only valid for a new track".into());}
                input["insert_index"].as_u64().and_then(|n|usize::try_from(n).ok())
                    .filter(|n|*n<=state.track_order.len()).ok_or("invalid track insertion index")?
            }else{state.track_order.len()};
            if let Some(track) = state.tracks.get_mut(&id) {
                track.name = label;
            } else {
                state.track_order.insert(insert_index,id.clone());
                state.tracks.insert(
                    id.clone(),
                    Track {
                        output: String::new(),
                        id,
                        name: label,
                        kind: kind.into(),
                        gain: 1.,
                        mute: false,
                        solo: false,
                        default_sound_muted: false,
                        route: Route::default(),
                        io: super::track_io::TrackIo::new(kind),
                        gain_target: None,
                        cc_lane: super::track_io::CcLane::None,
                        cc_device_slot: 0,
                        cc_parameter_slot: 0,
                    },
                );
            }
            Ok(state.changed())
        }
        "SetTrackMix" => {
            let id = name(input, "id")?;
            let mut track = state.tracks.get(&id).ok_or("unknown track")?.clone();
            if let Some(out)=input.get("output"){let out=out.as_str().ok_or("invalid output")?;super::channels::validate_output(state,&id,out,true)?;track.output=out.into();}
            if track.kind == "cc" {
                // The automation row has binary A and three target
                // selectors, not the audio track's six routing controls.
                if input.as_object().is_some_and(|fields|fields.keys().any(|key|!matches!(key.as_str(),"document_revision"|"id"|"cc_action"|"cc_parameter_slot"))) {
                    return Err("CC row actions cannot change audio routing".into());
                }
                match input["cc_action"].as_str(){
                    Some("arm")=>track.io.arm_mode=if track.io.arm_mode>0{0}else{1},
                    Some("target_lane")=>{
                        let lanes=state.track_order.iter().map(|id|state.tracks[id].snapshot()).collect::<Vec<_>>();
                        track.cc_lane=track.cc_lane.next(&lanes);
                        track.cc_device_slot=0;track.cc_parameter_slot=0;
                        // Never silently map a Cardinal device selector onto the
                        // older Rust-only built-in gain adapter.
                        track.gain_target=None;
                    },
                    Some("target_device")=>{
                        let catalog=if track.cc_lane==super::track_io::CcLane::Workspace{crate::theatre::lighting::catalog().clone()}else{state.plugins.automation_catalog(true)?};
                        let count=super::cc_automation::devices(&track,&catalog).len();
                        if count==0{return Ok(state.snapshot());}
                        track.cc_device_slot=(track.cc_device_slot.min(count-1)+1)%count;track.cc_parameter_slot=0;
                    },
                    Some("target_param")=>{return Ok(state.snapshot());}, // UI-owned picker toggle.
                    Some("target_param_pick")=>{
                        let catalog=if track.cc_lane==super::track_io::CcLane::Workspace{crate::theatre::lighting::catalog().clone()}else{state.plugins.automation_catalog(true)?};
                        let target=super::cc_automation::resolved(&track,&catalog);
                        let count=target["cc_parameters"].as_array().map_or(0,Vec::len);
                        track.cc_parameter_slot=input["cc_parameter_slot"].as_u64().and_then(|n|usize::try_from(n).ok())
                            .filter(|n|*n<count).ok_or("automation parameter selection disappeared")?;
                    },
                    _=>return Err("unknown CC row action".into()),
                }
                state.tracks.insert(id,track);return Ok(state.changed());
            }
            if input.get("gain").is_some() {
                track.gain = number(input, "gain", 0., 16.)? as f32;
            }
            let boolean = |key: &str| -> Result<Option<bool>, String> {
                input
                    .get(key)
                    .map(|v| v.as_bool().ok_or_else(|| format!("{key} must be boolean")))
                    .transpose()
            };
            if let Some(muted)=boolean("default_sound_muted")? {
                if track.kind!="midi" {return Err("Default sound belongs to MIDI tracks".into());}
                track.default_sound_muted=muted;
            }
            let mute = boolean("mute")?;
            let solo = boolean("solo")?;
            if mute == Some(true) && solo == Some(true) {
                return Err("mute and solo cannot both be enabled".into());
            }
            if let Some(value) = mute {
                track.mute = value;
                if value {
                    track.solo = false;
                }
            }
            if let Some(value) = solo {
                track.solo = value;
                if value {
                    track.mute = false;
                }
            }
            track.route = Route::from_patch(input, track.route)?;
            track.io = super::track_io::TrackIo::from_patch(input, track.io, &track.kind)?;
            let clear = input.get("clear_midi_contents").map(|v|v.as_bool().ok_or("clear_midi_contents must be boolean")).transpose()?.unwrap_or(false);
            if clear && track.kind != "midi" { return Err("clear contents is the original MIDI-only C control".into()); }
            if clear {
                state.recording.midi.clear(&id);
                state.clips.retain(|_,clip|clip.track!=id);
                state.clip_order.retain(|clip|state.clips.contains_key(clip));
            }
            if !track.io.records(){state.recording.disarm(&id);}
            state.tracks.insert(id, track);
            Ok(state.changed())
        }
        "SetCcTarget" => {
            let id = name(input, "id")?;
            let track = state.tracks.get(&id).ok_or("unknown track")?;
            if track.kind != "cc" {
                return Err("automation target requires a CC track".into());
            }
            let target = input
                .get("target")
                .ok_or("target is required; null disconnects")?;
            let target = if target.is_null() {
                None
            } else {
                if target["parameter"] != "gain" {
                    return Err("only the built-in track gain target is connected".into());
                }
                let dest = name(target, "track")?;
                if !state.tracks.get(&dest).is_some_and(|t| t.kind != "cc") {
                    return Err("gain target must name an audio or MIDI track".into());
                }
                Some(dest)
            };
            // The legacy Rust gain adapter must never also address the first hosted parameter.
            state.tracks.get_mut(&id).unwrap().cc_lane=super::track_io::CcLane::None;
            state.tracks.get_mut(&id).unwrap().gain_target = target;
            Ok(state.changed())
        }
        "ReorderTracks" => {
            let order: Vec<String> = serde_json::from_value(input["ids"].clone())
                .map_err(|_| "ids must be a track ID array")?;
            let unique: BTreeSet<_> = order.iter().collect();
            if order.len() != state.tracks.len()
                || unique.len() != order.len()
                || order.iter().any(|id| !state.tracks.contains_key(id))
            {
                return Err("order must contain every track exactly once".into());
            }
            state.track_order = order;
            Ok(state.changed())
        }
        "PutSource" => {
            let id = name(input, "id")?;
            let source = playback::Source::parse(input)?;
            if state.sources.len() >= 256 && !state.sources.contains_key(&id) {
                return Err("source capacity reached".into());
            }
            let existing = state.sources.get(&id).map_or(0, |s| s.samples.len());
            if state
                .sources
                .values()
                .map(|s| s.samples.len())
                .sum::<usize>()
                - existing
                + source.samples.len()
                > 16_777_216
            {
                return Err("decoded source memory capacity reached".into());
            }
            super::warp_treatment::validate_source_replacement(state,&id,&source)?;
            let treatments=super::clip_treatment::replace_source(state,&id,&source)?;
            state.sources.insert(id, source);
            state.clip_treatments=treatments;
            Ok(state.changed())
        }
        "PutClip" => {let result=state.put_clip(input)?;if let Some(id)=input["id"].as_str(){state.drum_rack.instances.remove(id);}Ok(result)},
        "SetClipRouting" => {
            state.require_revision(input)?;
            let id=name(input,"id")?;let mut clip=state.clips.get(&id).ok_or("unknown clip")?.clone();
            if !clip.track.is_empty(){return Err("Clip routing is only available for trackless audio".into());}
            clip.output=name(input,"output")?;super::channels::validate_clip(state,&clip)?;
            state.clips.insert(id,clip);Ok(state.changed())
        }
        "RemoveClip" => {
            let id = name(input, "id")?;
            state.clips.remove(&id).ok_or("unknown clip")?;
            state.drum_rack.instances.remove(&id);
            state.clip_treatments.remove(&id);
            state.warp.entries.remove(&id);
            state.clip_order.retain(|old| old != &id);
            Ok(state.changed())
        }
        "MoveClip" | "SetMidiTempo" => {
            let id = name(input, "id")?;
            let mut clip = state.clips.get(&id).ok_or("unknown clip")?.clone();
            if hook == "MoveClip" {
                let was_assigned=!clip.track.is_empty();
                if input.get("track").is_some(){clip.track=if input["track"].is_null(){String::new()}else{input["track"].as_str().ok_or("invalid track")?.into()};}
                if was_assigned&&clip.track.is_empty()&&input.get("output").is_none()&&(clip.output.is_empty()||clip.output=="OFF"){clip.output=super::channels::default_output(state);}
                if let Some(lane)=input.get("lane"){clip.lane=lane.as_f64().filter(|n|n.is_finite()&&(0. ..=511.).contains(n)).ok_or("invalid free position")?;}
                if let Some(output)=input.get("output"){clip.output=output.as_str().ok_or("invalid output")?.into();}
                super::channels::validate_clip(state,&clip)?;
                clip.start = number(input, "start_seconds", 0., MAX_SECONDS)?;
                if let Some(next)=super::warp_treatment::moved_duration(state,&id,clip.start)? {if let Content::Audio{duration,..}=&mut clip.content{*duration=next;}}
            } else {
                let new_bpm = number(input, "bpm", 1., 1000.)?;
                let Content::Midi { bpm, .. } = &mut clip.content else {
                    return Err("tempo belongs to MIDI piano rolls only".into());
                };
                *bpm = new_bpm;
            }
            fits(&clip)?;
            state.clips.insert(id, clip);
            Ok(state.changed())
        }
        "SnapMidi" => state.snap(input),
        "SnapClip" => state.snap_clip(input),
        "RenderRange" => {
            state.require_revision(input)?;
            let publish = input
                .get("publish_visualization")
                .map(|flag| {
                    flag.as_bool()
                        .ok_or("publish_visualization must be boolean")
                })
                .transpose()?
                .unwrap_or(false);
            let mut rendered = playback::render(state, input)?;
            if publish {
                let sequence = state
                    .visualization_sequence
                    .checked_add(1)
                    .ok_or("visualization sequence exhausted")?;
                rendered["visualization"]["sequence"] = json!(sequence);
                state.visualization = Some(rendered["visualization"].clone());
                state.visualization_sequence = sequence;
            }
            Ok(rendered)
        }
        "PianoRoll" => state.piano_roll(input),
        "BeginMasterAnalysis" => state.begin_master(input),
        "AnalyzeMaster" => state.analyze_master(input),
        "LocatePanel" => super::panel::locate(input),
        "ProjectPanel" => super::panel::project(input),
        _ => Err(format!("unknown workstation data hook {hook}")),
    }
}

#[cfg(test)] mod shared_snapshot_tests {
 use super::*;
 fn edit(s:&mut State,hook:&str,mut input:Value)->Value {input["document_revision"]=json!(s.revision);dispatch(s,hook,&input).unwrap()}
 fn fixture()->State {let mut s=State::default();edit(&mut s,"PutTrack",json!({"id":"t","name":"Track","kind":"midi"}));edit(&mut s,"PutClip",json!({"id":"c","track":"t","kind":"midi","start_seconds":0.,"bpm":120.,"length_quarters":4.,"notes":[{"id":"n","quarter":0.,"length_quarters":1.,"pitch":60,"velocity":0.5}]}));s}
 #[test] fn unchanged_document_reuses_the_exact_owned_snapshot_value(){let mut s=fixture();let a=s.shared_snapshot();assert_eq!(a.owned(),s.snapshot());let b=s.shared_snapshot();assert!(Arc::ptr_eq(&a.document,&b.document));}
 #[test] fn edits_publish_a_new_snapshot_without_mutating_existing_readers(){let mut s=fixture();let a=s.shared_snapshot();edit(&mut s,"MoveClip",json!({"id":"c","start_seconds":2.}));let b=s.shared_snapshot();assert!(!Arc::ptr_eq(&a.document,&b.document));assert_eq!(a.document["clips"][0]["start_seconds"],0.);assert_eq!(b.document["clips"][0]["start_seconds"],2.);assert_eq!(b.owned(),s.snapshot());edit(&mut s,"RemoveClip",json!({"id":"c"}));assert_eq!(s.shared_snapshot().owned(),s.snapshot());assert_eq!(b.document["clips"].as_array().unwrap().len(),1);}
 #[test] fn rejected_edits_do_not_replace_valid_cached_data(){let mut s=fixture();let a=s.shared_snapshot();assert!(dispatch(&mut s,"MoveClip",&json!({"document_revision":0,"id":"c","start_seconds":9.})).is_err());assert!(Arc::ptr_eq(&a.document,&s.shared_snapshot().document));}
 #[test] fn fresh_nonrevision_metadata_does_not_reconstruct_the_document(){let mut s=fixture();let a=s.shared_snapshot();let revision=s.revision;edit(&mut s,"BeginMasterAnalysis",json!({"capture_epoch":"fresh","sample_rate":8000,"channels":1}));let b=s.shared_snapshot();assert_eq!(s.revision,revision);assert!(Arc::ptr_eq(&a.document,&b.document));assert_ne!(a.live,b.live);assert_eq!(b.owned(),s.snapshot());s.master_regions=vec![json!({"start_seconds":1.,"end_seconds":2.})];let c=s.shared_snapshot();assert!(Arc::ptr_eq(&b.document,&c.document));assert_ne!(b.live,c.live);assert_eq!(c.owned(),s.snapshot());for start in [0,8,16]{edit(&mut s,"RenderRange",json!({"sample_rate":8000,"start_frame":start,"frames":8,"publish_visualization":true}));let e=s.shared_snapshot();assert_eq!(s.revision,revision);assert!(Arc::ptr_eq(&a.document,&e.document));assert_eq!(e.owned(),s.snapshot());assert_ne!(e.live["visualization"],c.live["visualization"]);}}
 #[test] fn plugin_change_invalidates_and_reuse_never_crosses_document_owners(){let mut a=fixture();let old=a.shared_snapshot();a.plugin_changed();assert!(!Arc::ptr_eq(&old.document,&a.shared_snapshot().document));let mut b=fixture();let other=b.shared_snapshot();assert_eq!(old.owned(),other.owned());assert!(!Arc::ptr_eq(&old.document,&other.document));}
}

#[cfg(test)]mod timing_rebase_tests{
 use super::*;
 fn edit(s:&mut State,hook:&str,mut v:Value)->Result<Value,String>{v["document_revision"]=json!(s.revision);dispatch(s,hook,&v)}
 fn fixture()->State{let mut s=State::default();edit(&mut s,"PutTrack",json!({"id":"m","name":"MIDI","kind":"midi"})).unwrap();edit(&mut s,"PutTrack",json!({"id":"c","name":"CC","kind":"cc"})).unwrap();edit(&mut s,"PutClip",json!({"id":"midi","track":"m","kind":"midi","start_seconds":1.,"bpm":93.,"length_quarters":4.,"notes":[{"id":"n","quarter":1.,"length_quarters":1.,"pitch":60,"velocity":0.5}]})).unwrap();edit(&mut s,"PutClip",json!({"id":"cc","track":"c","kind":"cc","start_seconds":2.,"duration_seconds":4.,"points":[{"offset_seconds":1.,"value":0.25}]})).unwrap();s}
 #[test]fn rebase_moves_all_clips_and_map_origin_but_keeps_local_content_exact(){let mut s=fixture();let before=s.snapshot();let old=s.shared_snapshot();edit(&mut s,"RebaseTimeline",json!({"shift_seconds":4.})).unwrap();let after=s.snapshot();for(a,b)in before["clips"].as_array().unwrap().iter().zip(after["clips"].as_array().unwrap()){let mut a=a.clone();a["start_seconds"]=json!(a["start_seconds"].as_f64().unwrap()+4.);if let Some(notes)=a.get_mut("notes").and_then(Value::as_array_mut){for (n,other)in notes.iter_mut().zip(b["notes"].as_array().unwrap()){let expected=n["time_seconds"].as_f64().unwrap()+4.;assert!((other["time_seconds"].as_f64().unwrap()-expected).abs()<1e-12);n["time_seconds"]=other["time_seconds"].clone();}}assert_eq!(&a,b);}assert_eq!(after["timeline_zero_seconds"],4.);assert_eq!(after["musical_time"]["zero_sample"],192000);assert_eq!(after["musical_time"]["revision"],1);assert_eq!(old.owned(),before);assert_eq!(s.shared_snapshot().owned(),after);}
 #[test]fn rejected_rebase_has_no_partial_doc_map_or_revision_change(){let mut s=fixture();let before=s.snapshot();assert!(edit(&mut s,"RebaseTimeline",json!({"shift_seconds":86400.})).is_err());assert_eq!(s.snapshot(),before);assert!(dispatch(&mut s,"RebaseTimeline",&json!({"document_revision":0,"shift_seconds":1.})).is_err());assert_eq!(s.snapshot(),before);}
 #[test]fn map_publication_is_revisioned_without_retiming_existing_independent_clips(){let mut s=fixture();let before=s.snapshot();edit(&mut s,"SetMusicalTime",json!({"bpm":180.,"grid_choice":"fixed_1_8"})).unwrap();let after=s.snapshot();assert_eq!(before["clips"],after["clips"]);assert_eq!(after["musical_time"]["bpm"],180.);assert_eq!(after["musical_time"]["revision"],1);let revision=s.revision;edit(&mut s,"SetMusicalTime",json!({"bpm":180.,"grid_choice":"fixed_1_8"})).unwrap();assert_eq!(s.revision,revision);edit(&mut s,"MoveClip",json!({"id":"midi","start_seconds":3.})).unwrap();assert_eq!(s.snapshot()["musical_time"]["revision"],1);}
}

#[cfg(test)]mod cc_row_tests{
 use super::*;
 fn edit(s:&mut State,hook:&str,mut value:Value)->Value{value["document_revision"]=json!(s.revision);dispatch(s,hook,&value).unwrap()}
 #[test]fn cc_binary_arm_lane_cycle_and_retired_clear(){
  let mut s=State::default();for (id,kind) in [("m","midi"),("a","audio"),("cc","cc")]{edit(&mut s,"PutTrack",json!({"id":id,"name":id,"kind":kind}));}
  edit(&mut s,"PutClip",json!({"id":"automation","track":"cc","kind":"cc","start_seconds":0.,"duration_seconds":1.,"points":[{"offset_seconds":0.,"value":0.5}]}));
  edit(&mut s,"SetTrackMix",json!({"id":"cc","cc_action":"arm"}));assert_eq!(s.tracks["cc"].io.arm_mode,1);
  edit(&mut s,"SetTrackMix",json!({"id":"cc","cc_action":"arm"}));assert_eq!(s.tracks["cc"].io.arm_mode,0);
  edit(&mut s,"SetTrackMix",json!({"id":"cc","cc_action":"target_lane"}));assert_eq!(s.tracks["cc"].cc_lane,super::super::track_io::CcLane::Workspace);
  edit(&mut s,"SetTrackMix",json!({"id":"cc","cc_action":"target_lane"}));assert_eq!(s.tracks["cc"].cc_lane,super::super::track_io::CcLane::Track("a".into()));
  let revision=s.revision;edit(&mut s,"SetTrackMix",json!({"id":"cc","cc_action":"target_param"}));assert_eq!(s.revision,revision);
  assert!(dispatch(&mut s,"SetTrackMix",&json!({"document_revision":revision,"id":"cc","cc_action":"arm","mute":true})).is_err());assert_eq!(s.tracks["cc"].io.arm_mode,0);
  assert!(dispatch(&mut s,"SetTrackMix",&json!({"document_revision":revision,"id":"cc","cc_action":"clear"})).is_err());assert_eq!(s.clips.len(),1);assert_eq!(s.tracks.len(),3);
 }
}

#[cfg(test)]
mod audio_import_tests {
    use super::*;
    fn fixture() -> (State, std::path::PathBuf) {
        let mut state=State::default();
        dispatch(&mut state,"PutTrack",&json!({"document_revision":0,"id":"audio","name":"Audio","kind":"audio"})).unwrap();
        static SERIAL:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
        let serial=SERIAL.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
        let path=std::env::temp_dir().join(format!("wormwood-import-{}-{serial}.wav",std::process::id()));
        let mut bytes=Vec::new();
        bytes.extend(b"RIFF");bytes.extend(960036u32.to_le_bytes());bytes.extend(b"WAVEfmt ");
        bytes.extend(16u32.to_le_bytes());bytes.extend(1u16.to_le_bytes());bytes.extend(1u16.to_le_bytes());
        bytes.extend(48000u32.to_le_bytes());bytes.extend(96000u32.to_le_bytes());bytes.extend(2u16.to_le_bytes());bytes.extend(16u16.to_le_bytes());
        bytes.extend(b"data");bytes.extend(960000u32.to_le_bytes());
        for i in 0..480000 {let sample=(4096. * (i as f64*440.*std::f64::consts::TAU/48000.).sin()) as i16;bytes.extend(sample.to_le_bytes());}
        std::fs::write(&path,bytes).unwrap();
        (state,path)
    }
    fn request(state:&State,path:&std::path::Path)->Value {
        json!({"document_revision":state.revision,"track":"audio","path":path,"start_seconds":2.5,"sample_rate":48000})
    }
    #[test]
    fn imports_ten_second_tone_atomically_and_renders_at_drop_time() {
        let (mut state,path)=fixture(); let input=request(&state,&path);
        let result=dispatch(&mut state,"ImportAudio",&input).unwrap();
        assert_eq!(state.revision,2); assert_eq!(state.sources.len(),1); assert_eq!(state.clips.len(),1);
        let id=result["clip"].as_str().unwrap();let clip=&state.clips[id];
        assert_eq!(clip.start,2.5); assert_eq!(clip.duration(),10.);
        let source=&state.sources[result["source"].as_str().unwrap()];
        assert_eq!(source.samples.len(),480000);assert_eq!(source.channels,1);
        assert!(source.samples.iter().any(|s|s.abs()>0.12));
        let rendered=dispatch(&mut state,"RenderRange",&json!({"document_revision":2,"sample_rate":48000,"start_frame":120000,"frames":2048})).unwrap();
        let samples=rendered["lsffr"].as_array().unwrap();
        assert!(samples.iter().any(|frame|frame[2].as_f64().unwrap().abs()>0.12));
        assert!(samples.iter().all(|frame|[0,1,3].iter().all(|i|frame[*i]==0.)));
        let silence=dispatch(&mut state,"RenderRange",&json!({"document_revision":2,"sample_rate":48000,"start_frame":0,"frames":64})).unwrap();
        assert!(silence["lsffr"].as_array().unwrap().iter().all(|f|f==&json!([0.,0.,0.,0.])));
        assert!(dispatch(&mut state,"ImportAudio",&input).is_err());
        assert_eq!(state.clips.len(),1);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn failed_imports_leave_no_source_or_clip() {
        let (mut state,path)=fixture();
        for (key,value) in [("sample_rate",json!(44100)),("track",json!("missing")),("start_seconds",json!(86400.)),("path",json!("/nonexistent/no.wav"))] {
            let mut input=request(&state,&path); input[key]=value;
            assert!(dispatch(&mut state,"ImportAudio",&input).is_err(),"{key}");
            assert_eq!(state.revision,1);assert!(state.sources.is_empty());assert!(state.clips.is_empty());
        }
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(test)] mod track_delete_regressions {
 use super::*;
 fn call(s:&mut State,hook:&str,mut v:Value)->Result<Value,String>{v["document_revision"]=json!(s.revision);dispatch(s,hook,&v)}
 #[test] fn selected_track_delete_is_atomic_and_undo_restores_clips(){
  let mut s=State::default();call(&mut s,"PutTrack",json!({"id":"m","name":"MIDI","kind":"midi"})).unwrap();
  call(&mut s,"PutClip",json!({"id":"c","track":"m","kind":"midi","start_seconds":0.,"bpm":120.,"length_quarters":4.,"notes":[]})).unwrap();
  let before=s.snapshot();let revision=s.revision;
  assert!(dispatch(&mut s,"RemoveTrack",&json!({"document_revision":0,"id":"m"})).is_err());assert_eq!(before,s.snapshot());
  call(&mut s,"RemoveTrack",json!({"id":"m"})).unwrap();assert_eq!(s.revision,revision+1);assert!(s.tracks.is_empty());assert!(s.clips.is_empty());
  call(&mut s,"Undo",json!({})).unwrap();assert!(s.tracks.contains_key("m"));assert!(s.clips.contains_key("c"));
 }
 #[test] fn deleting_a_drum_track_remaps_surviving_pattern_instances(){
  let mut s=State::default();for id in ["kick","snare"]{call(&mut s,"DrumRack",json!({"action":"add","track":id,"name":id})).unwrap();}
  call(&mut s,"DrumRack",json!({"action":"steps","channel":1,"steps":1})).unwrap();
  call(&mut s,"DrumRack",json!({"action":"place","channel":1,"id":"pattern","start_seconds":0.})).unwrap();
  call(&mut s,"RemoveTrack",json!({"id":"kick"})).unwrap();assert_eq!(s.drum_rack.channels[0].track,"snare");assert_eq!(s.drum_rack.instances["pattern"],0);
 }
}
