//! Application-worker recording. The native callback supplies bounded timestamped
//! input blocks; it never calls this module or edits the document.
//! Source: DawTransportSystem startRecording, DawClipSystem mergePendingRecords,
//! and Host/DawAudioPlaylists::Trim in the preserved Cardinal tree.
use super::{
    document::{Clip, Content, State as Document, Track},
    playback::Source,
    track_io::TrackIo,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
const MAX_VALUES: usize = 16_777_216;
const MAX_CLIPS: usize = 512;
const MAX_SOURCES: usize = 256;

#[derive(Clone)]
struct Take {
    track: String,
    playlist:usize,
    start: u64,
    frames: usize,
    channels: usize,
    route: TrackIo,
    loop_range: Option<[u64; 2]>,
    chunks: Vec<Arc<[f32]>>,
}
#[derive(Clone, Default)]
pub(super) struct State {
    rate: u32,
    next_capture: Option<u64>,
    recording: bool,
    playing: bool,
    enabled: BTreeSet<String>,
    destinations:BTreeMap<String,usize>,
    pending: Vec<Take>,
    serial: u64,
    midi_events: Vec<Value>,
    pub(super) midi:super::midi_capture::State,
}
impl State {
    pub fn status(&self) -> Value {
        json!({"recording":self.recording,"playing":self.playing,"sample_rate":self.rate,
            "next_capture_frame":self.next_capture,"record_enabled_tracks":self.enabled.iter().cloned().chain(self.midi.enabled()).collect::<BTreeSet<_>>(),
            "pending_frames":self.pending.iter().map(|t|t.frames).sum::<usize>(),
            "pending_samples":self.pending.iter().map(|t|t.frames*t.channels).sum::<usize>(),
            "sequence":self.serial,"midi_events":self.midi_events,
            "midi_recording_supported":true,"midi":self.midi.status()})
    }
    pub fn busy(&self,id:&str)->bool {(self.recording&&self.enabled.contains(id))||self.pending.iter().any(|take|take.track==id)}
    pub fn enabled(&self, id: &str) -> bool {
        self.enabled.contains(id)
    }
    pub fn disarm(&mut self, id: &str) {
        self.enabled.remove(id);self.midi.disarm(id);
    }
}
fn unsigned(input: &Value, key: &str) -> Result<u64, String> {
    input[key]
        .as_u64()
        .ok_or_else(|| format!("{key} must be an unsigned sample-frame integer"))
}
fn flag(input: &Value, key: &str) -> Result<bool, String> {
    input[key]
        .as_bool()
        .ok_or_else(|| format!("{key} must be boolean"))
}
fn scalar(value: &Value) -> Result<f32, String> {
    value
        .as_f64()
        .filter(|v| v.is_finite() && v.abs() <= 64.)
        .map(|v| v as f32)
        .ok_or_else(|| "input PCM must be finite within +/-64".into())
}
struct Inputs {
    physical: Vec<Vec<f32>>,
    virtual_mic: Option<Vec<f32>>,
}
impl Inputs {
    fn parse(v: &Value, frames: usize) -> Result<Self, String> {
        if frames == 0 && v.is_null() {
            return Ok(Self {
                physical: vec![],
                virtual_mic: None,
            });
        }
        let channel = |v: &Value| {
            v.as_array()
                .filter(|a| a.len() == frames)
                .ok_or("capture channels must match frames")?
                .iter()
                .map(scalar)
                .collect::<Result<Vec<_>, String>>()
        };
        let physical = v["physical"]
            .as_array()
            .filter(|a| a.len() <= 256)
            .ok_or("capture requires bounded physical channels")?
            .iter()
            .map(channel)
            .collect::<Result<Vec<_>, _>>()?;
        let virtual_mic = v
            .get("virtual_mic")
            .filter(|v| !v.is_null())
            .map(channel)
            .transpose()?;
        Ok(Self {
            physical,
            virtual_mic,
        })
    }
    fn select(&self, io: TrackIo, frames: usize) -> Option<(usize, Arc<[f32]>)> {
        if io.use_virtual_input {
            return self
                .virtual_mic
                .as_ref()
                .map(|v| (1, Arc::from(v.as_slice())));
        }
        if io.stereo_input_pair_12 {
            let l = self.physical.first()?;
            let r = self.physical.get(1).unwrap_or(l);
            Some((
                2,
                (0..frames)
                    .flat_map(|i| [l[i], r[i]])
                    .collect::<Vec<_>>()
                    .into(),
            ))
        } else {
            self.physical
                .get(io.physical_input_index)
                .map(|v| (1, Arc::from(v.as_slice())))
        }
    }
}
fn loop_range(input: &Value) -> Result<Option<[u64; 2]>, String> {
    if input.get("loop_start_frame").is_none() && input.get("loop_end_frame").is_none() {
        return Ok(None);
    }
    let a = unsigned(input, "loop_start_frame")?;
    let b = unsigned(input, "loop_end_frame")?;
    if b <= a {
        return Err("recording loop end must follow its start".into());
    }
    Ok(Some([a, b]))
}
fn validate_midi(
    input: &Value,
    capture_start: u64,
    frames: usize,
    tracks: &BTreeMap<String, Track>,
) -> Result<Vec<Value>, String> {
    let Some(events) = input.get("midi_events") else {
        return Ok(vec![]);
    };
    let events = events
        .as_array()
        .filter(|a| a.len() <= 4096)
        .ok_or("midi_events must be a bounded timestamped event array")?;
    let mut previous = capture_start;
    for event in events {
        let at = unsigned(event, "capture_frame")?;
        if at < previous || at < capture_start || at >= capture_start + frames as u64 {
            return Err("MIDI events must be ordered inside their capture block".into());
        }
        previous = at;
        let id = event["track"]
            .as_str()
            .ok_or("MIDI capture track required")?;
        if !tracks.get(id).is_some_and(|t| t.kind == "midi") {
            return Err("MIDI capture track must name a MIDI lane".into());
        }
        if !event["pitch"].as_u64().is_some_and(|v| v <= 127) {
            return Err("MIDI pitch must be 0..127".into());
        }
        if !event["velocity"]
            .as_f64()
            .is_some_and(|v| v.is_finite() && (0. ..=1.).contains(&v))
        {
            return Err("MIDI velocity must be 0..1 (zero is note off)".into());
        }
        if let Some(v) = event.get("tuning_cents") {
            if !v.as_f64().is_some_and(|v| v.is_finite()) {
                return Err("MIDI tuning must be finite".into());
            }
        }
    }
    Ok(events.clone())
}
/// Every validation and capacity check completes before publishing the staged
/// recorder or document. A rejected block can be retried without duplicate PCM.
pub(super) fn capture(document: &mut Document, input: &Value) -> Result<Value, String> {
    let rate = unsigned(input, "sample_rate")?;
    if !(8000..=192000).contains(&rate) {
        return Err("capture sample rate must be 8000..192000".into());
    }
    let frames = unsigned(input, "frames")? as usize;
    if frames > 4096 {
        return Err("capture blocks are limited to 4096 frames".into());
    }
    let capture_start = unsigned(input, "capture_start_frame")?;
    let capture_end = capture_start
        .checked_add(frames as u64)
        .ok_or("capture clock overflow")?;
    let timeline = unsigned(input, "timeline_start_frame")?;
    if timeline
        .checked_add(frames as u64)
        .is_none_or(|v| v > rate * 86_400)
    {
        return Err("capture exceeds the document's one-day timeline".into());
    }
    let playing = flag(input, "playing")?;
    let recording = flag(input, "recording")?;
    if recording && !playing {
        return Err("recording requires playing transport".into());
    }
    let loop_range = loop_range(input)?;
    if loop_range.is_some_and(|r| r[1] > rate * 86_400) {
        return Err("recording loop exceeds the document timeline".into());
    }
    let pcm = Inputs::parse(&input["live_inputs"], frames)?;
    if let Some(player) = input.get("player_audio") {
        let channels = player
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or("player_audio requires two planar channels")?;
        for values in channels {
            let values = values
                .as_array()
                .filter(|v| v.len() == frames)
                .ok_or("player_audio channels must match capture frames")?;
            for value in values {
                scalar(value)?;
            }
        }
    }
    let midi = validate_midi(input, capture_start, frames, &document.tracks)?;
    let mut next = document.recording.clone();
    if frames > 0 && next.next_capture.is_some_and(|n| n != capture_start) {
        return Err(
            "capture discontinuity: stop and commit before accepting a new capture clock".into(),
        );
    }
    if next.rate != 0 && next.rate != rate as u32 && !next.pending.is_empty() {
        return Err("stop and commit before changing capture sample rate".into());
    }
    next.rate = rate as u32;
    let starting = recording && !next.recording;
    if starting {
        if !next.pending.is_empty() {
            return Err(
                "previous recording must be committed while stopped before starting another".into(),
            );
        }
        next.enabled = document
            .tracks
            .values()
            .filter(|t| t.kind == "audio" && t.io.records())
            .map(|t| t.id.clone())
            .collect();
    }
    if starting {next.destinations=next.enabled.iter().map(|id|(id.clone(),document.audio_playlists.first_record_lane(id))).collect();}
    // Cardinal latches recordEnabled at the beginning of the take. Arming an
    // additional lane during a take does not silently join that recording.
    next.enabled
        .retain(|id| recording && document.tracks.get(id).is_some_and(|t| t.io.records()));
    next.recording = recording;
    next.playing = playing;
    // Validate the entire block before advancing any stateful native preamp.
    next.midi.advance(&document.tracks,input,rate as u32)?;
    let next_serial=next.serial.checked_add(1).ok_or("capture sequence exhausted")?;
    let mut selected=Vec::new();
    let mut unavailable = vec![];
    if frames>0 && recording {
        for id in &next.enabled {
            if let Some((channels,samples))=pcm.select(document.tracks[id].io,frames) {selected.push((id.clone(),channels,samples));}
            else {unavailable.push(id.clone());}
        }
    }
    let pending_values=next.pending.iter().map(|take|take.frames*take.channels).sum::<usize>();
    let incoming_values=selected.iter().map(|(_,_,samples)|samples.len()).sum::<usize>();
    let stored_values=document.sources.values().map(|source|source.samples.len()).sum::<usize>();
    if stored_values.checked_add(pending_values).and_then(|n|n.checked_add(incoming_values)).is_none_or(|n|n>MAX_VALUES) {
        return Err("capture would exceed the document PCM memory capacity; block was not consumed".into());
    }
    if frames > 0 && recording {
        for (id,channels,mut samples) in selected {
            let track = &document.tracks[&id];
            if document.plugins.has_preamp(&id) {
                let mut stereo=samples.chunks_exact(channels).map(|p|[p[0],p[if channels==2 {1}else{0}]]).collect::<Vec<_>>();
                document.plugins.process_preamp(&id,&mut stereo,rate as u32,capture_start,timeline)?;
                // Cardinal drains the processed right recording ring only for
                // stereo IN 1/2. Mono/VM records the processed left channel.
                samples=stereo.iter().flat_map(|p|p[..channels].iter().copied()).collect::<Vec<_>>().into();
            }
            let existing = next.pending.iter_mut().rev().find(|t| t.track == id);
            let append = existing.is_some_and(|take| {
                let expected = take.start + take.frames as u64;
                let mapped = take.loop_range.map_or(expected, |[a, b]| {
                    if expected < a {
                        expected
                    } else {
                        a + (expected - a) % (b - a)
                    }
                });
                if take.route != track.io || take.channels != channels || mapped != timeline {
                    return false;
                }
                take.chunks.push(Arc::clone(&samples));
                take.frames += frames;
                true
            });
            if !append {
                next.pending.push(Take {
                    track: id.clone(),
                    playlist:*next.destinations.get(&id).unwrap_or(&0),
                    start: timeline,
                    frames,
                    channels,
                    route: track.io,
                    loop_range,
                    chunks: vec![samples],
                });
            }
        }
    }
    let pending = next
        .pending
        .iter()
        .map(|t| t.frames * t.channels)
        .sum::<usize>();
    let stored = document
        .sources
        .values()
        .map(|s| s.samples.len())
        .sum::<usize>();
    if pending > MAX_VALUES || stored.checked_add(pending).is_none_or(|n| n > MAX_VALUES) {
        return Err(
            "capture would exceed the document PCM memory capacity; block was not consumed".into(),
        );
    }
    if frames > 0 {
        next.next_capture = Some(capture_end);
    } else {
        next.next_capture = None;
    }
    next.serial = next_serial;
    next.midi_events = midi;
    let created = if !playing {
        let created = commit(document, &mut next)?;
        next.next_capture = None;
        created
    } else {
        vec![]
    };
    document.recording = next;
    let mut result = document.recording.status();
    result["revision"] = json!(document.revision);
    result["created"] = json!(created);
    result["unavailable_inputs"] = json!(unavailable);
    Ok(result)
}
fn identity(prefix: &str, used: &BTreeSet<String>) -> String {
    (1..=used.len() + 1)
        .map(|n| format!("{prefix}-{n}"))
        .find(|id| !used.contains(id))
        .unwrap()
}
/// Same split/trim rules as DawAudioPlaylists::Trim. The source offset of the
/// retained right piece advances by exactly the removed timeline duration.
fn insert(
    clips: &mut BTreeMap<String, Clip>,
    order: &mut Vec<String>,
    incoming: Clip,
) -> Result<(), String> {
    let a = incoming.start;
    let b = a + incoming.duration();
    let affected = order
        .iter()
        .filter(|id| {
            clips[*id].track == incoming.track
                && clips[*id].start < b
                && clips[*id].start + clips[*id].duration() > a
        })
        .cloned()
        .collect::<Vec<_>>();
    for id in affected {
        let old = clips.remove(&id).unwrap();
        order.retain(|v| v != &id);
        let end = old.start + old.duration();
        let Content::Audio { source, offset, .. } = &old.content else {
            return Err("audio recording overlap has nonaudio content".into());
        };
        if old.start < a {
            let mut left = old.clone();
            left.content = Content::Audio {
                source: source.clone(),
                offset: *offset,
                duration: a - old.start,
            };
            order.push(left.id.clone());
            clips.insert(left.id.clone(), left);
        }
        if end > b {
            let id = if old.start < a {
                identity("record-split", &clips.keys().cloned().collect())
            } else {
                old.id.clone()
            };
            let right = Clip { output:String::new(),lane:0., gain:old.gain, fade_in:0., fade_out:0.,
                id: id.clone(),
                track: old.track,
                start: b,
                content: Content::Audio {
                    source: source.clone(),
                    offset: *offset + (b - old.start),
                    duration: end - b,
                },
            };
            order.push(id.clone());
            clips.insert(id, right);
        }
    }
    order.push(incoming.id.clone());
    clips.insert(incoming.id.clone(), incoming);
    if clips.len() > MAX_CLIPS {
        return Err("recording commit exceeds clip capacity; pending audio retained".into());
    }
    Ok(())
}
fn commit(document: &mut Document, next: &mut State) -> Result<Vec<String>, String> {
    if next.pending.is_empty() && !next.midi.pending() {
        return Ok(vec![]);
    }
    let mut playlists=document.audio_playlists.clone();
    let mut sources = document.sources.clone();
    let mut clips = document.clips.clone();
    let mut order = document.clip_order.clone();
    let mut created = next.midi.commit(&mut clips,&mut order,next.rate)?;
    let mut used_clip_ids = clips.keys().cloned().collect::<BTreeSet<_>>();
    let mut next_clip_id=1usize;
    used_clip_ids.extend(playlists.extra_ids());
    for take in &next.pending {
        if take.frames == 0 {
            continue;
        }
        let data = take
            .chunks
            .iter()
            .flat_map(|c| c.iter().copied())
            .collect::<Vec<_>>();
        let source_id=identity("record-source",&sources.keys().cloned().collect());
        sources.insert(source_id.clone(),Source {sample_rate:next.rate,channels:take.channels,samples:Arc::from(data)});
        if sources.len()>MAX_SOURCES{return Err("recording commit exceeds source capacity; pending audio retained".into());}
        let mut consumed = 0;
        let last_lane=playlists.lane_count(&take.track);
        while consumed < take.frames {
            let raw = take.start + consumed as u64;
            let (at, count) = match take.loop_range {
                Some([a, _b]) if raw < a => (raw, ((a - raw) as usize).min(take.frames - consumed)),
                Some([a, b]) => {
                    let position = (raw - a) % (b - a);
                    (
                        a + position,
                        ((b - a - position) as usize).min(take.frames - consumed),
                    )
                }
                None => (raw, take.frames - consumed),
            };
            let id=loop{let id=format!("record-clip-{next_clip_id}");next_clip_id+=1;if !used_clip_ids.contains(&id){break id;}};
            used_clip_ids.insert(id.clone());
            let clip = Clip { output:String::new(),lane:0., gain:1., fade_in:0., fade_out:0.,
                id: id.clone(),
                track: take.track.clone(),
                start: at as f64 / next.rate as f64,
                content: Content::Audio {
                    source: source_id.clone(),
                    offset: consumed as f64 / next.rate as f64,
                    duration: count as f64 / next.rate as f64,
                },
            };
            let pass=take.loop_range.map_or(0,|[a,b]| ((take.start+consumed as u64).saturating_sub(a)/(b-a)) as usize);
            let destination=if take.playlist==0{0}else{take.playlist.saturating_add(pass).min(last_lane)};
            if destination==0 {insert(&mut clips, &mut order, clip)?;}else{playlists.insert_capture(&take.track,destination,clip,&used_clip_ids)?;used_clip_ids.extend(playlists.extra_ids());}
            if clips.len()+playlists.extra_count()>MAX_CLIPS{return Err("recording exceeds combined playlist clip capacity".into());}
            created.push(id);
            consumed += count;
        }
    }
    // Preserve insertion grouping, but each recorded lane's clips are sorted by
    // elapsed time as the source's lane-local active playlist is.
    let targets = next
        .pending
        .iter()
        .map(|t| t.track.clone())
        .collect::<BTreeSet<_>>();
    for target in targets {
        let indices = order
            .iter()
            .enumerate()
            .filter_map(|(i, id)| (clips[id].track == target).then_some(i))
            .collect::<Vec<_>>();
        let mut ids = indices
            .iter()
            .map(|i| order[*i].clone())
            .collect::<Vec<_>>();
        ids.sort_by(|a, b| clips[a].start.total_cmp(&clips[b].start));
        for (i, id) in indices.into_iter().zip(ids) {
            order[i] = id;
        }
    }
    document.audio_playlists=playlists;
    document.sources = sources;
    document.clips = clips;
    document.clip_order = order;
    document.recording_changed();
    next.pending.clear();
    next.enabled.clear();
    // Some earlier loop-pass clip IDs have been punched out by later passes.
    created.retain(|id| document.clips.contains_key(id)||document.audio_playlists.contains(id));
    Ok(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn doc() -> Document {
        let mut d = Document::default();
        d.tracks.insert("a".into(), track("a", 1));
        d.tracks.insert("tracking".into(), track("tracking", 3));
        d
    }
    fn track(id: &str, arm: u8) -> Track {
        Track {
            output:String::new(),            id: id.into(),
            name: id.into(),
            kind: "audio".into(),
            gain: 1.,
            mute: false,
            solo: false,
                default_sound_muted: false,
            route: super::super::routing::Route::default(),
            io: TrackIo {
                arm_mode: arm,
                ..TrackIo::new("audio")
            },
            gain_target: None,
            cc_lane: super::super::track_io::CcLane::None,
            cc_device_slot:0,cc_parameter_slot:0,
        }
    }
    fn block(start: u64, timeline: u64, values: &[f32]) -> Value {
        json!({"sample_rate":8000,"capture_start_frame":start,"timeline_start_frame":timeline,"frames":values.len(),"playing":true,"recording":true,"live_inputs":{"physical":[values]}})
    }
    fn stop(d: &mut Document, at: u64) -> Value {
        capture(d,&json!({"sample_rate":8000,"capture_start_frame":at,"timeline_start_frame":at,"frames":0,"playing":false,"recording":false})).unwrap()
    }
    fn rendered_source(d: &Document, clip: &Clip) -> Vec<f32> {
        let Content::Audio {
            source,
            offset,
            duration,
        } = &clip.content
        else {
            panic!("not audio")
        };
        let s = &d.sources[source];
        let a = (*offset * s.sample_rate as f64).round() as usize * s.channels;
        let b = a + (*duration * s.sample_rate as f64).round() as usize * s.channels;
        s.samples[a..b].to_vec()
    }
    #[test]
    fn recording_latches_only_armed_tracks_and_commits_on_stop() {
        let mut d = doc();
        let result = capture(&mut d, &block(0, 80, &[1., 2., 3.])).unwrap();
        assert_eq!(result["record_enabled_tracks"], json!(["a"]));
        assert!(d.clips.is_empty());
        assert_eq!(d.revision, 0);
        d.tracks.get_mut("tracking").unwrap().io.arm_mode = 1;
        capture(&mut d, &block(3, 83, &[4.])).unwrap();
        stop(&mut d, 4);
        assert_eq!(d.clips.len(), 1);
        let clip = d.clips.values().next().unwrap();
        assert_eq!(clip.track, "a");
        assert_eq!(clip.start, 0.01);
        assert_eq!(rendered_source(&d, clip), [1., 2., 3., 4.]);
        assert_eq!(d.revision, 1);
        assert!(d.recording.enabled.is_empty());
    }
    #[test]
    fn recording_disarm_retains_pending_without_accepting_more_audio() {
        let mut d = doc();
        capture(&mut d, &block(0, 0, &[1., 2.])).unwrap();
        d.tracks.get_mut("a").unwrap().io.arm_mode = 0;
        d.recording.disarm("a");
        capture(&mut d, &block(2, 2, &[3., 4.])).unwrap();
        stop(&mut d, 4);
        assert_eq!(
            rendered_source(&d, d.clips.values().next().unwrap()),
            [1., 2.]
        );
    }
    #[test]
    fn recording_stereo_falls_back_to_one_physical_input_and_virtual_requires_source() {
        let mut d = doc();
        d.tracks.get_mut("a").unwrap().io.stereo_input_pair_12 = true;
        capture(&mut d, &block(0, 0, &[1., 2.])).unwrap();
        stop(&mut d, 2);
        let c = d.clips.values().next().unwrap();
        assert_eq!(rendered_source(&d, c), [1., 1., 2., 2.]);
        let mut d = doc();
        d.tracks.get_mut("a").unwrap().io.use_virtual_input = true;
        let r = capture(&mut d, &block(0, 0, &[1., 2.])).unwrap();
        assert_eq!(r["unavailable_inputs"], json!(["a"]));
        stop(&mut d, 2);
        assert!(d.clips.is_empty());
        let mut b = block(0, 0, &[1., 2.]);
        b["live_inputs"]["virtual_mic"] = json!([7., 8.]);
        capture(&mut d, &b).unwrap();
        stop(&mut d, 2);
        assert_eq!(
            rendered_source(&d, d.clips.values().next().unwrap()),
            [7., 8.]
        );
    }
    #[test]
    fn recording_replaces_overlap_and_preserves_right_source_offset() {
        let mut d = doc();
        d.sources.insert(
            "old".into(),
            Source {
                sample_rate: 8000,
                channels: 1,
                samples: vec![9.; 8].into(),
            },
        );
        let old = Clip { output:String::new(),lane:0., gain:1., fade_in:0., fade_out:0.,
            id: "old".into(),
            track: "a".into(),
            start: 0.,
            content: Content::Audio {
                source: "old".into(),
                offset: 0.,
                duration: 8. / 8000.,
            },
        };
        d.clip_order.push(old.id.clone());
        d.clips.insert(old.id.clone(), old);
        capture(&mut d, &block(0, 2, &[1., 2., 3.])).unwrap();
        stop(&mut d, 3);
        let all = d
            .clip_order
            .iter()
            .map(|id| rendered_source(&d, &d.clips[id]))
            .collect::<Vec<_>>();
        assert_eq!(all, [vec![9., 9.], vec![1., 2., 3.], vec![9., 9., 9.]]);
        let last = &d.clips[d.clip_order.last().unwrap()];
        let Content::Audio { offset, .. } = last.content else {
            panic!()
        };
        assert_eq!(offset, 5. / 8000.);
    }
    #[test]
    fn recording_loop_later_pass_punches_previous_without_take_stack() {
        let mut d = doc();
        let mut b = block(0, 6, &[1., 2., 3., 4., 5., 6., 7., 8.]);
        b["loop_start_frame"] = json!(4);
        b["loop_end_frame"] = json!(8);
        capture(&mut d, &b).unwrap();
        stop(&mut d, 8);
        let all = d
            .clip_order
            .iter()
            .map(|id| rendered_source(&d, &d.clips[id]))
            .collect::<Vec<_>>();
        assert_eq!(all, [vec![7., 8.], vec![5., 6.]]);
        assert_eq!(d.clips[&d.clip_order[0]].start, 4. / 8000.);
        assert_eq!(d.clips[&d.clip_order[1]].start, 6. / 8000.);
    }
    #[test]
    fn recording_rejects_gap_and_invalid_payload_atomically_and_allows_reset_after_stop() {
        let mut d = doc();
        capture(&mut d, &block(0, 0, &[1., 2.])).unwrap();
        let before = d.recording.status();
        assert!(capture(&mut d, &block(4, 4, &[8.])).is_err());
        assert_eq!(before, d.recording.status());
        let mut invalid = block(2, 2, &[3.]);
        invalid["player_audio"] = json!([[1.], []]);
        assert!(capture(&mut d, &invalid).is_err());
        assert_eq!(before, d.recording.status());
        capture(&mut d, &block(2, 2, &[3.])).unwrap();
        stop(&mut d, 3);
        capture(&mut d, &block(50, 80, &[4.])).unwrap();
        stop(&mut d, 51);
        assert_eq!(d.clips.len(), 2);
    }
    #[test]
    fn recording_commit_capacity_rejection_keeps_all_pending_and_document_unchanged() {
        let mut d = doc();
        for i in 0..MAX_SOURCES {
            d.sources.insert(
                format!("s{i}"),
                Source {
                    sample_rate: 8000,
                    channels: 1,
                    samples: vec![0.].into(),
                },
            );
        }
        capture(&mut d, &block(0, 0, &[1., 2.])).unwrap();
        let before = d.recording.status();
        let result = capture(
            &mut d,
            &json!({"sample_rate":8000,"capture_start_frame":2,"timeline_start_frame":2,"frames":0,"playing":false,"recording":false}),
        );
        assert!(result.is_err());
        assert_eq!(before, d.recording.status());
        assert_eq!(d.sources.len(), MAX_SOURCES);
        assert!(d.clips.is_empty());
        assert_eq!(d.revision, 0);
    }
}

#[cfg(test)]mod playlist_capture_tests {
 use super::*;
 fn setup()->Document {let mut d=Document::default();super::super::document::dispatch(&mut d,"PutTrack",&json!({"document_revision":0,"id":"a","name":"Audio","kind":"audio"})).unwrap();d.tracks.get_mut("a").unwrap().io.arm_mode=1;d}
 fn capture_block(d:&mut Document)->Value {capture(d,&json!({"sample_rate":8000,"capture_start_frame":0,"timeline_start_frame":0,"frames":2,"playing":true,"recording":true,"live_inputs":{"physical":[[0.1,0.2],[0.3,0.4]]}})).unwrap()}
 fn stop(d:&mut Document)->Value{capture(d,&json!({"sample_rate":8000,"capture_start_frame":2,"timeline_start_frame":2,"frames":0,"playing":false,"recording":false})).unwrap()}
 #[test]fn alternate_capture_is_silent_and_blocks_busy_lane_delete(){let mut d=setup();super::super::audio_playlists::command(&mut d,&json!({"id":"a","action":"new"})).unwrap();capture_block(&mut d);assert!(super::super::audio_playlists::command(&mut d,&json!({"id":"a","action":"delete","index":1})).is_err());let result=stop(&mut d);assert!(d.clips.is_empty());assert_eq!(d.audio_playlists.extra_count(),1);assert_eq!(result["created"].as_array().unwrap().len(),1);assert_eq!(d.audio_playlists.record_target("a"),1);}
}

#[cfg(test)]mod comp_lane_tests{
 use super::*;use super::super::document::dispatch;
 #[test]fn comp_lane_loop_advances_then_overwrites_last_and_keeps_main(){
  let mut d=Document::default();dispatch(&mut d,"PutTrack",&json!({"document_revision":0,"id":"a","name":"Audio","kind":"audio"})).unwrap();d.tracks.get_mut("a").unwrap().io.arm_mode=1;
  for _ in 0..2{super::super::audio_playlists::command(&mut d,&json!({"id":"a","action":"new"})).unwrap();}
  let input=|capture_start:u64,timeline:u64,values:Vec<f32>,recording:bool|json!({"sample_rate":8000,"capture_start_frame":capture_start,"timeline_start_frame":timeline,"frames":values.len(),"playing":recording,"recording":recording,"loop_start_frame":0,"loop_end_frame":2,"loop_enabled":true,"live_inputs":{"physical":[values]}});
  capture(&mut d,&input(0,0,vec![1.,2.,3.],true)).unwrap();capture(&mut d,&input(3,1,vec![4.,5.,6.,7.,8.,9.,10.],true)).unwrap();
  capture(&mut d,&input(10,0,vec![],false)).unwrap();assert!(d.clips.is_empty());
  let lanes=d.audio_playlists.snapshot(&d);let source_values=|lane:usize|{let c=&lanes[0]["lanes"][lane]["clips"][0];let s=&d.sources[c["source"].as_str().unwrap()];let offset=(c["source_offset_seconds"].as_f64().unwrap()*8000.).round()as usize;s.samples[offset..offset+2].to_vec()};
  assert_eq!(source_values(1),vec![1.,2.]);assert_eq!(source_values(2),vec![9.,10.]);assert_eq!(d.audio_playlists.extra_count(),2);
 }
}
