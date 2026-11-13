//! Cardinal DawChannelRackSystem: four monophonic sample channels, sixteen
//! sixteenth steps, root MIDI 48, half-step gates. Control/producer side only.
use super::{
    document::{Clip, Content, Note, State, Track},
    playback::Source,
};
use serde_json::{Value, json};
use std::cell::{Cell, RefCell};
#[derive(Clone, Default)]
struct History {
    next: Option<(u64, u64, u32)>,
    voices: Vec<Option<(u64, f64)>>,
}
#[derive(Clone)]
pub(super) struct Channel {
    pub track: String,
    pub name: String,
    pub source: Option<String>,
    pub steps: u16,
    pub muted: bool,
    pub volume: f32,
}
#[derive(Clone)]
pub(super) struct Rack {
    pub channels: Vec<Channel>,
    pub bpm: f64,
    pub preview: bool,
    origin: Cell<Option<u64>>,
    preview_step: Cell<i32>,
    history: RefCell<History>,
    pub instances: std::collections::BTreeMap<String, usize>,
}
impl Default for Rack {
    fn default() -> Self {
        Self {
            channels: Vec::new(),
            bpm: 120.,
            preview: false,
            origin: Cell::new(None),
            preview_step: Cell::new(-1),
            history: RefCell::new(History::default()),
            instances: Default::default(),
        }
    }
}
impl Rack {
    pub fn remove_track(&mut self,id:&str,removed:&std::collections::BTreeSet<String>){
        let map:Vec<_>=self.channels.iter().enumerate().filter(|(_,c)|c.track!=id).map(|(i,_)|i).collect();
        self.instances.retain(|key,index|{if removed.contains(key){return false;}if let Some(next)=map.iter().position(|old|old==index){*index=next;true}else{false}});
        self.channels.retain(|c|c.track!=id);
    }

    pub fn frozen(&self) -> Self {
        let mut r = self.clone();
        r.preview = false;
        r.origin.set(None);
        r.preview_step.set(-1);
        *r.history.borrow_mut() = History::default();
        r
    }
    pub fn snapshot(&self) -> Value {
        json!({"bpm":self.bpm,"preview":self.preview,"preview_step":self.preview_step.get(),"channels":self.channels.iter().map(|c|json!({"track":c.track,"name":c.name,"source":c.source,"steps":c.steps,"mute":c.muted,"volume":c.volume})).collect::<Vec<_>>(),"instances":self.instances})
    }
    pub fn channel(&self, track: &str) -> Option<&Channel> {
        self.channels.iter().find(|c| c.track == track)
    }
}
fn text(v: &Value, k: &str) -> Result<String, String> {
    v[k].as_str()
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .map(str::to_owned)
        .ok_or_else(|| format!("{k} needs 1..128 bytes"))
}
fn number(v: &Value, k: &str, lo: f64, hi: f64) -> Result<f64, String> {
    v[k].as_f64()
        .filter(|x| x.is_finite() && *x >= lo && *x <= hi)
        .ok_or_else(|| format!("invalid {k}"))
}
fn pattern(c: &Channel, bpm: f64) -> Content {
    Content::Midi { edo:12,scale:0,root:0,
        bpm,
        quarters: 4.,
        numerator: 4,
        denominator: 4,
        notes: (0..16)
            .filter(|i| c.steps & (1 << i) != 0)
            .map(|i| Note { bend:0.,tuning_cents:0.,
                id: format!("step-{i}"),
                quarter: i as f64 / 4.,
                length: 0.125,
                pitch: 48,
                velocity: 1.,
            })
            .collect(),
    }
}
fn capacity(s: &State, channel: usize, steps: u16, extra: bool) -> Result<(), String> {
    let mut count = 0;
    for clip in s.clips.values() {
        if s.drum_rack.instances.get(&clip.id) == Some(&channel) {
            count += steps.count_ones() as usize;
        } else if let Content::Midi { notes, .. } = &clip.content {
            count += notes.len();
        }
    }
    if extra {
        count += steps.count_ones() as usize;
    }
    if count > 8192 {
        return Err("document note capacity reached".into());
    }
    Ok(())
}
pub(super) fn command(s: &mut State, v: &Value) -> Result<(), String> {
    match v["action"].as_str().ok_or("rack action required")? {
        "add" => {
            if s.drum_rack.channels.len() >= 4 || s.tracks.len() >= 64 {
                return Err("rack or track capacity reached".into());
            }
            let id = text(v, "track")?;
            let name = text(v, "name")?;
            if s.tracks.contains_key(&id) {
                return Err("rack needs a new MIDI track identity".into());
            }
            let source = if v["source"].is_null() {
                None
            } else {
                let id = text(v, "source")?;
                if !s.sources.contains_key(&id) {
                    return Err("missing rack source".into());
                }
                Some(id)
            };
            s.tracks.insert(
                id.clone(),
                Track {
            output:String::new(),                    id: id.clone(),
                    name: name.clone(),
                    kind: "midi".into(),
                    gain: 1.,
                    mute: false,
                    solo: false,
                default_sound_muted: false,
                    route: Default::default(),
                    io: super::track_io::TrackIo::new("midi"),
                    gain_target: None,
                    cc_lane: Default::default(),
                    cc_device_slot: 0,
                    cc_parameter_slot: 0,
                },
            );
            s.track_order.push(id.clone());
            let volume = [0.82, 0.62, 0.62, 0.48][s.drum_rack.channels.len()];
            s.drum_rack.channels.push(Channel {
                track: id,
                name,
                source,
                steps: 0,
                muted: false,
                volume,
            });
        }
        "preview" => {
            let enabled = v["enabled"]
                .as_bool()
                .ok_or("preview enabled must be boolean")?;
            s.drum_rack.preview = enabled;
            s.drum_rack.preview_step.set(-1);
            s.drum_rack.origin.set(None);
        }
        "tempo" => {
            let bpm = number(v, "bpm", 1., 1000.)?;
            for (id, channel) in &s.drum_rack.instances {
                if let Some(clip) = s.clips.get(id) {
                    if clip.start + 240. / bpm > 86400. {
                        return Err("rack pattern exceeds timeline limit".into());
                    }
                    capacity(s, *channel, s.drum_rack.channels[*channel].steps, false)?;
                }
            }
            s.drum_rack.bpm = bpm;
            s.drum_rack.origin.set(None);
            for (id, ch) in &s.drum_rack.instances {
                if let Some(clip) = s.clips.get_mut(id) {
                    clip.content = pattern(&s.drum_rack.channels[*ch], bpm);
                }
            }
        }
        action => {
            let index = v["channel"]
                .as_u64()
                .filter(|i| (*i as usize) < s.drum_rack.channels.len())
                .ok_or("invalid rack channel")? as usize;
            let mut c = s.drum_rack.channels[index].clone();
            match action {
                "steps" => {
                    c.steps = v["steps"]
                        .as_u64()
                        .filter(|x| *x <= 65535)
                        .ok_or("steps must be a 16-bit mask")? as u16;
                    capacity(s, index, c.steps, false)?;
                }
                "mix" => {
                    if let Some(value) = v.get("mute") {
                        c.muted = value.as_bool().ok_or("mute must be boolean")?;
                    }
                    if v.get("volume").is_some() {
                        c.volume = number(v, "volume", 0., 1.)? as f32;
                    }
                }
                "source" => {
                    let source = text(v, "source")?;
                    if !s.sources.contains_key(&source) {
                        return Err("missing rack source".into());
                    }
                    c.source = Some(source);
                    if v.get("name").is_some() {
                        c.name = text(v, "name")?;
                    }
                }
                "import" => {
                    let path = v["path"]
                        .as_str()
                        .filter(|p| !p.is_empty() && p.len() <= 4096)
                        .ok_or("sample path needs 1..4096 bytes")?
                        .to_owned();
                    let id = text(v, "source")?;
                    if s.sources.contains_key(&id) {
                        return Err(
                            "sample source identity already exists; bind it with source instead"
                                .into(),
                        );
                    }
                    let source = super::drum_sample::read(std::path::Path::new(&path))?;
                    let total = s
                        .sources
                        .iter()
                        .filter(|(key, _)| **key != id)
                        .map(|(_, source)| source.samples.len())
                        .sum::<usize>()
                        + source.samples.len();
                    if (!s.sources.contains_key(&id) && s.sources.len() >= 256) || total > 16777216
                    {
                        return Err("decoded source capacity reached".into());
                    }
                    c.name = std::path::Path::new(&path)
                        .file_stem()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Sample")
                        .chars()
                        .take(32)
                        .collect();
                    c.source = Some(id.clone());
                    s.sources.insert(id, source);
                }
                "place" => {
                    let id = text(v, "id")?;
                    let start = number(v, "start_seconds", 0., 86400.)?;
                    if s.clips.contains_key(&id) || s.clips.len() >= 512 {
                        return Err("pattern identity exists or clip capacity reached".into());
                    }
                    if start + 240. / s.drum_rack.bpm > 86400. {
                        return Err("pattern exceeds timeline limit".into());
                    }
                    capacity(s, index, c.steps, true)?;
                    let clip = Clip { output:String::new(),lane:0., gain:1., fade_in:0., fade_out:0.,
                        id: id.clone(),
                        track: c.track.clone(),
                        start,
                        content: pattern(&c, s.drum_rack.bpm),
                    };
                    s.clips.insert(id.clone(), clip);
                    s.clip_order.push(id.clone());
                    s.drum_rack.instances.insert(id, index);
                }
                _ => return Err("unknown rack action".into()),
            }
            if action == "steps" {
                for (id, ch) in &s.drum_rack.instances {
                    if *ch == index {
                        if let Some(clip) = s.clips.get_mut(id) {
                            clip.content = pattern(&c, s.drum_rack.bpm);
                        }
                    }
                }
            }
            if let Some(t) = s.tracks.get_mut(&c.track) {
                if matches!(action, "source" | "import") {
                    t.name = c.name.clone();
                }
            }
            s.drum_rack.channels[index] = c;
        }
    }
    Ok(())
}
struct Voice<'a> {
    track: usize,
    channel: &'a Channel,
    source: &'a Source,
    hits: Vec<(u64, f64)>,
}
pub(super) struct Render<'a> {
    voices: Vec<Voice<'a>>,
    rate: u32,
    preview: Option<u64>,
    bpm: f64,
    loop_pcm: Vec<(usize, Vec<[f32; 2]>)>,
}
impl<'a> Render<'a> {
    pub fn prepare(
        s: &'a State,
        tracks: &[&Track],
        rate: u32,
        monitor: bool,
        capture: Option<u64>,
        clock: &super::render_clock::Clock,
    ) -> Result<Self, String> {
        let preview = if s.drum_rack.preview {
            let at = capture.unwrap_or(0);
            let origin = s.drum_rack.origin.get().unwrap_or(at);
            s.drum_rack.origin.set(Some(origin));
            Some(at.saturating_sub(origin))
        } else {
            None
        };
        s.drum_rack.preview_step.set(preview.map_or(-1, |elapsed| {
            let period = (240. / s.drum_rack.bpm * rate as f64).round().max(16.) as u64;
            ((elapsed % period) * 16 / period).min(15) as i32
        }));
        let mut voices = Vec::new();
        for (index, track) in tracks.iter().enumerate() {
            let Some(c) = s.drum_rack.channel(&track.id) else {
                continue;
            };
            let Some(id) = &c.source else { continue };
            let source = s.sources.get(id).ok_or("missing drum rack sample")?;
            let mut hits = Vec::new();
            if !monitor && preview.is_none() {
                for clip in s
                    .clip_order
                    .iter()
                    .map(|id| &s.clips[id])
                    .filter(|c| c.track == track.id)
                {
                    if let Content::Midi { bpm, notes, .. } = &clip.content {
                        for n in notes.iter().filter(|n| n.pitch == 48 && n.velocity > 0.) {
                            hits.push((
                                ((clip.start + n.quarter * 60. / bpm) * rate as f64).round() as u64,
                                n.velocity,
                            ));
                        }
                    }
                }
            }
            hits.sort_by_key(|h| h.0);
            voices.push(Voice {
                track: index,
                channel: c,
                source,
                hits,
            });
        }
        let mut result = Self {
            voices,
            rate,
            preview,
            bpm: s.drum_rack.bpm,
            loop_pcm: Vec::new(),
        };
        // A contiguous producer sequence carries the one-shot through natural wraps.
        // Direct/offline ranges remain pure absolute-clock reconstruction.
        if let Some(sequence) = clock.sequence.filter(|_| preview.is_none() && !monitor) {
            let mut history = s.drum_rack.history.borrow_mut();
            let continuous = history.next == Some((sequence.generation, sequence.frame, rate));
            history.voices.resize(s.drum_rack.channels.len(), None);
            if !continuous {
                history.voices.fill(None);
            }
            for v in &result.voices {
                let ch = s
                    .drum_rack
                    .channels
                    .iter()
                    .position(|c| c.track == tracks[v.track].id)
                    .unwrap();
                let mut phase = history.voices[ch];
                let mut pcm = Vec::with_capacity(clock.frames);
                for i in 0..clock.frames {
                    let at = clock.at(i);
                    let pos = v.hits.partition_point(|h| h.0 <= at);
                    if let Some(hit) = pos.checked_sub(1).map(|j| v.hits[j]) {
                        if hit.0 == at || (!continuous && i == 0) {
                            phase = Some((at - hit.0, hit.1));
                        }
                    }
                    pcm.push(phase.map_or([0.; 2], |(elapsed, velocity)| {
                        sample_source(
                            v.source,
                            elapsed,
                            rate,
                            v.channel.volume,
                            velocity,
                            v.channel.muted,
                        )
                    }));
                    if let Some((age, velocity)) = phase {
                        phase = Some((age.saturating_add(1), velocity));
                    }
                }
                history.voices[ch] = phase;
                result.loop_pcm.push((v.track, pcm));
            }
            history.next = Some((
                sequence.generation,
                sequence.frame + clock.frames as u64,
                rate,
            ));
        } else {
            *s.drum_rack.history.borrow_mut() = History::default();
        }
        Ok(result)
    }
    pub fn sample(&self, track: usize, at: u64, offset: usize) -> [f32; 2] {
        if let Some((_, pcm)) = self.loop_pcm.iter().find(|(index, _)| *index == track) {
            return pcm[offset];
        }
        let Some(v) = self.voices.iter().find(|v| v.track == track) else {
            return [0.; 2];
        };
        if v.channel.muted {
            return [0.; 2];
        }
        let hit = if let Some(start) = self.preview {
            let time = start + offset as u64;
            let period = (240. / self.bpm * self.rate as f64).round().max(16.) as u64;
            let cycle = time / period;
            let local = time % period;
            let mut latest = None;
            for step in 0..16 {
                if v.channel.steps & (1 << step) == 0 {
                    continue;
                }
                let position = (step as f64 * period as f64 / 16.).round() as u64;
                let on = if position <= local {
                    Some(cycle * period + position)
                } else if cycle > 0 {
                    Some((cycle - 1) * period + position)
                } else {
                    None
                };
                if let Some(on) = on {
                    if latest.is_none_or(|x| on > x) {
                        latest = Some(on);
                    }
                }
            }
            latest.map(|on| (time - on, 1.))
        } else {
            let i = v.hits.partition_point(|h| h.0 <= at);
            i.checked_sub(1).map(|i| (at - v.hits[i].0, v.hits[i].1))
        };
        let Some((elapsed, velocity)) = hit else {
            return [0.; 2];
        };
        sample_source(
            v.source,
            elapsed,
            self.rate,
            v.channel.volume,
            velocity,
            v.channel.muted,
        )
    }
}

fn sample_source(
    source: &Source,
    elapsed: u64,
    rate: u32,
    volume: f32,
    velocity: f64,
    muted: bool,
) -> [f32; 2] {
    if muted {
        return [0.; 2];
    }
    let position = elapsed as f64 * source.sample_rate as f64 / rate as f64;
    let frame = position.floor() as usize;
    let count = source.samples.len() / source.channels;
    if frame >= count {
        return [0.; 2];
    }
    let next = (frame + 1).min(count - 1);
    let fraction = (position - frame as f64) as f32;
    std::array::from_fn(|ch| {
        let ch = ch.min(source.channels - 1);
        let a = source.samples[frame * source.channels + ch];
        let b = source.samples[next * source.channels + ch];
        (a + (b - a) * fraction) * volume * velocity as f32
    })
}

#[cfg(test)]
mod drum_rack_tests {
    use super::super::{document::dispatch, playback};
    use super::*;
    fn edit(s: &mut State, mut v: Value) -> Result<Value, String> {
        v["document_revision"] = json!(s.revision);
        dispatch(s, "DrumRack", &v)
    }
    fn fixture() -> State {
        let mut s = State::default();
        dispatch(&mut s,"PutSource",&json!({"document_revision":0,"id":"kick","sample_rate":8000,"channels":2,"samples":[1.,0.5,0.75,0.25,0.5,0.125,0.25,0.0625]})).unwrap();
        edit(
            &mut s,
            json!({"action":"add","track":"drum","name":"Kick","source":"kick"}),
        )
        .unwrap();
        edit(&mut s, json!({"action":"steps","channel":0,"steps":3})).unwrap();
        edit(
            &mut s,
            json!({"action":"place","channel":0,"id":"pattern","start_seconds":0.}),
        )
        .unwrap();
        s
    }
    fn render(s: &State, start: u64, frames: u64) -> Value {
        playback::render(
            s,
            &json!({"sample_rate":8000,"start_frame":start,"frames":frames}),
        )
        .unwrap()
    }
    #[test]
    fn root_note_and_pattern_clock_match_cardinal() {
        let s = fixture();
        let Content::Midi {
            notes,
            bpm,
            quarters,
            ..
        } = &s.clips["pattern"].content
        else {
            panic!()
        };
        assert_eq!((*bpm, *quarters), (120., 4.));
        assert_eq!(
            (notes[0].pitch, notes[1].quarter, notes[1].length),
            (48, 0.25, 0.125)
        );
        assert_eq!(s.tracks["drum"].kind, "midi");
    }
    #[test]
    fn sampler_routes_stereo_to_shared_bus_without_sine_or_averaging() {
        let s = fixture();
        let v = render(&s, 0, 4);
        assert_eq!(v["audio_complete"], true);
        assert!((v["lsffr"][0][2].as_f64().unwrap() - 1.23).abs() < 1e-6);
        assert_eq!(v["lsffr"][0][0], 0.);
        assert_eq!(v["lsffr"][0][3], 0.);
    }
    #[test]
    fn split_ranges_and_seek_reconstruct_one_shot_tails() {
        let s = fixture();
        let whole = render(&s, 0, 8);
        let a = render(&s, 0, 2);
        let b = render(&s, 2, 6);
        let joined = a["lsffr"]
            .as_array()
            .unwrap()
            .iter()
            .chain(b["lsffr"].as_array().unwrap())
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(whole["lsffr"], json!(joined));
        assert_eq!(render(&s, 1000, 1)["lsffr"][0], whole["lsffr"][0]);
    }
    #[test]
    fn pattern_edits_sync_instances_and_preserve_positions() {
        let mut s = fixture();
        edit(
            &mut s,
            json!({"action":"place","channel":0,"id":"second","start_seconds":3.}),
        )
        .unwrap();
        edit(&mut s, json!({"action":"steps","channel":0,"steps":32768})).unwrap();
        for id in ["pattern", "second"] {
            let Content::Midi { notes, .. } = &s.clips[id].content else {
                panic!()
            };
            assert_eq!(notes.len(), 1);
            assert_eq!(notes[0].quarter, 3.75);
        }
        assert_eq!(s.clips["second"].start, 3.);
    }
    #[test]
    fn rejected_patch_and_stale_revision_are_atomic() {
        let mut s = fixture();
        let before = dispatch(&mut s, "Snapshot", &Value::Null).unwrap();
        assert!(
            edit(
                &mut s,
                json!({"action":"mix","channel":0,"mute":true,"volume":2.})
            )
            .is_err()
        );
        assert_eq!(dispatch(&mut s, "Snapshot", &Value::Null).unwrap(), before);
        assert!(
            dispatch(
                &mut s,
                "DrumRack",
                &json!({"document_revision":0,"action":"preview","enabled":true})
            )
            .is_err()
        );
    }
    #[test]
    fn rack_and_track_mute_both_silence_samples() {
        let mut s = fixture();
        edit(&mut s, json!({"action":"mix","channel":0,"mute":true})).unwrap();
        assert_eq!(render(&s, 0, 4)["lsffr"], json!(vec![[0.; 4]; 4]));
        edit(&mut s, json!({"action":"mix","channel":0,"mute":false})).unwrap();
        s.tracks.get_mut("drum").unwrap().mute = true;
        assert_eq!(render(&s, 0, 4)["lsffr"], json!(vec![[0.; 4]; 4]));
    }
    #[test]
    fn preview_uses_capture_clock_while_transport_is_stopped() {
        let mut s = fixture();
        edit(&mut s, json!({"action":"preview","enabled":true})).unwrap();
        let capture = |s: &mut State, at: u64| {
            let revision = s.revision;
            dispatch(s,"CaptureInput",&json!({"document_revision":revision,"sample_rate":8000,"capture_start_frame":at,"timeline_start_frame":0,"frames":2,"playing":false,"recording":false,"live_inputs":{"physical":[]},"midi_events":[]})).unwrap();
        };
        capture(&mut s, 0);
        let mut request = json!({"sample_rate":8000,"start_frame":0,"frames":2,"monitor_only":true,"capture_start_frame":0});
        let a = playback::render(&s, &request).unwrap();
        capture(&mut s, 2);
        request["capture_start_frame"] = json!(2);
        let b = playback::render(&s, &request).unwrap();
        let whole = render(&fixture(), 0, 4);
        assert_eq!(a["lsffr"][0], whole["lsffr"][0]);
        assert_eq!(b["lsffr"][0], whole["lsffr"][2]);
        assert_eq!(s.shared_snapshot().live["drum_rack"]["preview_step"], 0);
        assert!(!s.freeze_for_export().drum_rack.preview);
    }
    #[test]
    fn sample_rate_conversion_interpolates_without_changing_pitch() {
        let s = fixture();
        let v =
            playback::render(&s, &json!({"sample_rate":16000,"start_frame":0,"frames":2})).unwrap();
        let a = v["lsffr"][0][2].as_f64().unwrap();
        let b = v["lsffr"][1][2].as_f64().unwrap();
        assert!((a - 1.23).abs() < 1e-6);
        assert!((b - 1.025).abs() < 1e-6);
    }
    #[test]
    fn rack_capacity_does_not_add_an_orphan_track() {
        let mut s = State::default();
        for i in 0..4 {
            edit(
                &mut s,
                json!({"action":"add","track":format!("t{i}"),"name":"Row"}),
            )
            .unwrap();
        }
        let rev = s.revision;
        assert!(
            edit(
                &mut s,
                json!({"action":"add","track":"extra","name":"Extra"})
            )
            .is_err()
        );
        assert_eq!(s.tracks.len(), 4);
        assert_eq!(s.revision, rev);
    }
}

#[cfg(test)]
mod drum_rack_loop_tests {
    use super::super::{document::dispatch, playback};
    use super::*;
    #[test]
    fn contiguous_loop_wrap_carries_sample_tail_and_retrigger_restarts_it() {
        let mut s = State::default();
        let mut edit = |hook: &str, mut v: Value| {
            v["document_revision"] = json!(s.revision);
            dispatch(&mut s, hook, &v).unwrap();
        };
        edit(
            "PutSource",
            json!({"id":"hit","sample_rate":8000,"channels":1,"samples":[1.,0.75,0.5,0.25]}),
        );
        edit(
            "DrumRack",
            json!({"action":"add","track":"rack","name":"Hit","source":"hit"}),
        );
        edit(
            "PutClip",
            json!({"id":"note","track":"rack","kind":"midi","start_seconds":0.,"bpm":120.,"length_quarters":4.,"notes":[{"id":"n","quarter":0.0015,"length_quarters":0.1,"pitch":48,"velocity":1.}]}),
        );
        let render = |start, frames, seq| {
            playback::render(&s,&json!({"sample_rate":8000,"start_frame":start,"frames":frames,"playback_clock":{"generation":1,"sequence":seq},"loop_range":{"start_frame":4,"end_frame":8}})).unwrap()
        };
        let a = render(6, 2, 0);
        let b = render(4, 2, 2);
        let c = render(6, 1, 4);
        assert!(a["lsffr"][0][2].as_f64().unwrap() > 1.6);
        assert!((b["lsffr"][0][2].as_f64().unwrap() - 0.82).abs() < 1e-6);
        assert_eq!(a["lsffr"][0], c["lsffr"][0]);
    }
}
