//! Bounded control/worker-side rendering, never invoked from an audio callback.
//! Cardinal's unwarped clip addressing, MIDI event clock and four-bus mixer.
//! MIDI schedules remain available; the explicit cardinal_sine adapter renders
//! the authored Cardinal ChucK-lane timbre without loading an external plugin.
use super::{
    automation,
    document::{Content, State, Track},
    routing,
    source_analysis::Observation,
};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct Source {
    pub sample_rate: u32,
    pub channels: usize,
    pub samples: Arc<[f32]>,
}
fn rate(input: &Value) -> Result<u32, String> {
    input["sample_rate"]
        .as_u64()
        .filter(|v| (8000..=192000).contains(v))
        .map(|v| v as u32)
        .ok_or_else(|| "sample_rate must be an integer within 8000..192000".into())
}
impl Source {
    pub fn parse(input: &Value) -> Result<Self, String> {
        let sample_rate = rate(input)?;
        let channels = input["channels"]
            .as_u64()
            .filter(|v| *v == 1 || *v == 2)
            .ok_or("source channels must be 1 or 2")? as usize;
        let values = input["samples"]
            .as_array()
            .filter(|v| !v.is_empty() && v.len() <= 393216 && v.len() % channels == 0)
            .ok_or("source needs complete interleaved frames, at most 393216 values")?;
        let samples = values
            .iter()
            .map(|v| {
                v.as_f64()
                    .filter(|v| v.is_finite() && v.abs() <= 64.)
                    .map(|v| v as f32)
                    .ok_or_else(|| "source samples must be finite within +/-64".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            sample_rate,
            channels,
            samples: samples.into(),
        })
    }
    pub fn snapshot(&self, id: &str) -> Value {
        json!({"id":id,"sample_rate":self.sample_rate,"channels":self.channels,
            "frames":self.samples.len()/self.channels})
    }
    pub(super) fn stereo(&self, frame: usize) -> [f32; 2] {
        let left = self.samples[frame * self.channels];
        [
            left,
            if self.channels == 1 {
                left
            } else {
                self.samples[frame * self.channels + 1]
            },
        ]
    }
}

fn frame(time: f64, rate: u32) -> u64 {
    (time * rate as f64).round() as u64
}
fn audible(track: &Track, any_solo: bool) -> bool {
    !track.mute && (!any_solo || track.solo)
}

// Cardinal DawFaderSystem::faderValueToGain: normalized control, not PCM gain.
fn fader_gain(value: f64) -> f32 {
    if value <= 0.02 {
        0.
    } else {
        10f64.powf((-60. + (value - 0.02) / 0.98 * 65.) / 20.) as f32
    }
}

struct AudioRegion<'a> {
    length:u64,gain:f32,fade_in:f64,fade_out:f64,
    track: usize,
    source: &'a Source,
    start: u64,
    end: u64,
    offset: usize,
}
struct ControlRegion {
    target: usize,
    owner: usize,
    start: u64,
    end: u64,
    points: Vec<automation::Point>,
}
struct MidiVoice {
    track: usize,
    on: u64,
    off: u64,
    frequency: f64,
    bend_phase: [f64;20],
    velocity: f64,
}
impl MidiVoice {
    fn sample(&self, at: u64, rate: u32) -> f32 {
        if at < self.on || self.frequency >= rate as f64 * 0.49 {
            return 0.;
        }
        let t = (at - self.on) as f64 / rate as f64;
        let held = (self.off - self.on) as f64 / rate as f64;
        // ChuckLaneSystem.cpp defaultChuckCode: SinOsc, gain .16,
        // ADSR (5ms, 40ms, .35, 120ms). MIDI supplies pitch and gate length.
        let attack_decay = |seconds: f64| {
            if seconds < 0.005 {
                seconds / 0.005
            } else if seconds < 0.045 {
                1. - 0.65 * (seconds - 0.005) / 0.04
            } else {
                0.35
            }
        };
        let envelope = if t < held {
            attack_decay(t)
        } else {
            attack_decay(held) * (1. - (t - held) / 0.12).clamp(0., 1.)
        };
        (0.16 * self.velocity * envelope * (std::f64::consts::TAU * self.frequency * held * super::note_bend::phase(&self.bend_phase,t/held)).sin())
            as f32
    }
}

/// All output channels use the requested audio sample clock. Export is always
/// the four pre-monitor buses. master_gain affects only the listening fold.
pub(super) fn render(state: &State, input: &Value) -> Result<Value, String> {
    let rate = rate(input)?;
    let flag = |name: &str| -> Result<bool, String> {
        input
            .get(name)
            .map(|value| {
                value
                    .as_bool()
                    .ok_or_else(|| format!("{name} must be boolean"))
            })
            .transpose()
            .map(|value| value.unwrap_or(false))
    };
    let analyze = flag("analyze_visualization")?;
    let publish = flag("publish_visualization")?;
    let observe = analyze || publish;
    let monitor_only = flag("monitor_only")?;
    let builtin = match input.get("instrument") {
        None | Some(Value::Null) => false,
        Some(Value::String(name)) if name == "cardinal_sine" => true,
        _ => return Err("unsupported instrument; available builtin is cardinal_sine".into()),
    };
    let start = input["start_frame"]
        .as_u64()
        .ok_or("start_frame required")?;
    let frames = input["frames"]
        .as_u64()
        .filter(|v| (1..=4096).contains(v))
        .ok_or("frames must be within 1..4096")? as usize;
    let live_inputs = super::track_input_block::Block::parse(&input["live_inputs"],frames)?;
    let live_midi = input.get("capture_start_frame").map(|v| {
        let at=v.as_u64().ok_or("capture_start_frame must be an integer")?;
        state.recording.midi.voices(at,frames,rate).map(|voices|(at,voices))
    }).transpose()?;
    let end = start
        .checked_add(frames as u64)
        .ok_or("render range overflows the sample clock")?;
    let render_clock=super::render_clock::Clock::parse(input,start,frames)?;
    let (source_start,source_end)=render_clock.bounds();
    let master_gain = match input.get("master_gain") {
        None => 1.,
        Some(v) => v
            .as_f64()
            .filter(|v| v.is_finite() && (0.0..=16.).contains(v))
            .ok_or("master_gain must be finite within 0..16")? as f32,
    };
    let tracks: Vec<_> = state
        .track_order
        .iter()
        .map(|id| &state.tracks[id])
        .collect();
    let rack=super::drum_rack::Render::prepare(state,&tracks,rate,monitor_only,input["capture_start_frame"].as_u64().or(Some(start)),&render_clock)?;
    let any_solo = tracks.iter().any(|t| t.kind != "cc" && t.solo);
    let mut audio = Vec::new();
    let mut controls = Vec::new();
    // (frame, off-before-on, track order, clip order, note order, payload)
    let mut events = Vec::new();
    let mut active_notes = Vec::new();
    let mut midi_notes = 0usize;
    let mut needs_instrument = false;
    let mut midi_voices = Vec::new();
    for (track_index, track) in tracks.iter().enumerate() {
        for (clip_index, id) in state.clip_order.iter().enumerate() {
            let clip = &state.clips[id];
            if monitor_only || clip.track != track.id {
                continue;
            }
            let first = frame(clip.start, rate);
            // Region extent belongs to its own source clock. Fractional moves
            // must not add a source frame through absolute endpoint rounding.
            let last = first + frame(clip.duration(), rate).max(1);
            match &clip.content {
                Content::Audio { source, offset, .. }
                    if audible(track, any_solo) && !(live_inputs.is_some() && track.io.monitors_input()) && last > source_start && first < source_end =>
                {
                    let Some((source,offset)) = super::warp_treatment::render_source(state, &clip.id, source,*offset)? else {continue};
                    if source.sample_rate != rate {
                        return Err(
                            "source sample rate must match render rate; resample before PutSource"
                                .into(),
                        );
                    }
                    let offset = frame(offset, rate) as usize;
                    // Cardinal's unwarped path copies only available source
                    // frames; clip duration and the silent tail remain intact.
                    let available=(source.samples.len()/source.channels).saturating_sub(offset) as u64;
                    let last=last.min(first.saturating_add(available));
                    if last<=start {continue;}
                    audio.push(AudioRegion {
                        length:frame(clip.duration(),rate).max(1),gain:clip.gain as f32,fade_in:clip.fade_in,fade_out:clip.fade_out,
                        track: track_index,
                        source,
                        start: first,
                        end: last,
                        offset,
                    });
                }
                Content::Midi { bpm, notes, .. } if audible(track, any_solo) => {
                    needs_instrument |= !track.default_sound_muted && state.drum_rack.channel(&track.id).is_none() && !state.plugins.has_track(&track.id,true) && notes.iter().any(|note| note.velocity > 0.);
                    for (note_index, note) in
                        notes.iter().enumerate().filter(|(_, n)| n.velocity > 0.)
                    {
                        let on = frame(clip.start + note.quarter * 60. / bpm, rate);
                        let off =
                            frame(clip.start + (note.quarter + note.length) * 60. / bpm, rate)
                                .max(on + 1);
                        if builtin && !track.default_sound_muted && state.drum_rack.channel(&track.id).is_none() && !state.plugins.has_track(&track.id,true) && on < source_end && off.saturating_add(frame(0.12, rate)) > source_start {
                            if midi_voices.len() >= 128 {
                                return Err(
                                    "builtin instrument exceeds 128 voices in render range".into(),
                                );
                            }
                            midi_voices.push(MidiVoice {
                                track: track_index,
                                on,
                                off,
                                frequency: super::piano_tuning::frequency(note.pitch,note.tuning_cents),
                                bend_phase: super::note_bend::phase_coefficients(note.bend),
                                velocity: note.velocity,
                            });
                        }
                        if on < source_end && off > source_start {
                            midi_notes += 1;
                        }
                        let note_data = json!({"track":track.id,"clip":clip.id,"note":note.id,
                            "pitch":note.pitch,"tuning_cents":note.tuning_cents,"velocity":note.velocity});
                        if on < start && off > start {
                            let mut held = note_data.clone();
                            held["off_frame"] = json!(off);
                            active_notes.push(held);
                        }
                        for (at, order, kind) in [(on, 1, "note_on"), (off, 0, "note_off")] {
                            for offset in render_clock.offsets(at){
                                let offset=offset as u64;
                                let mut event=note_data.clone();event["frame"]=json!(at);
                                event["offset_frame"]=json!(offset);event["kind"]=json!(kind);
                                // Bounded event reporting. DSP desired-note state
                                // uses its own span plan, never this metadata list.
                                if events.len()>=65536{return Err("loop MIDI metadata exceeds 65536 events per block".into());}
                                events.push((offset,order,track_index,clip_index,note_index,event));
                            }
                        }
                    }
                }
                Content::Cc { points, .. } if last > source_start && first < source_end => {
                    if let Some(target) = &track.gain_target {
                        let target = tracks
                            .iter()
                            .position(|t| &t.id == target)
                            .ok_or("missing CC target track")?;
                        // Quantize only at the selected audio rate, never to musical time.
                        let mut points: Vec<_> = points
                            .iter()
                            .map(|p| automation::Point {
                                offset_seconds: frame(p.offset_seconds, rate) as f64,
                                value: p.value,
                            })
                            .collect();
                        points.sort_by(|a, b| {
                            a.offset_seconds
                                .total_cmp(&b.offset_seconds)
                                .then(a.value.total_cmp(&b.value))
                        });
                        controls.push(ControlRegion {
                            target,
                            owner: track_index,
                            start: first,
                            end: last,
                            points,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    let mut live_voices=Vec::new();
    if let Some((capture_start,voices))=live_midi {
        for voice in voices {
            let Some((index,track))=tracks.iter().enumerate().find(|(_,t)|t.id==voice.track) else {continue;};
            if !audible(track,any_solo) || (!voice.preview&&!track.io.monitors_input()){continue;}
            let off=voice.off.unwrap_or(u64::MAX);
            needs_instrument |= !track.default_sound_muted && state.drum_rack.channel(&track.id).is_none() && !state.plugins.has_track(&track.id,true) && voice.on<capture_start+frames as u64 && off>capture_start;
            if builtin&&!track.default_sound_muted&&state.drum_rack.channel(&track.id).is_none()&&!state.plugins.has_track(&track.id,true){
                if live_voices.len()+midi_voices.len()>=128{return Err("builtin instrument exceeds 128 live/clip voices".into());}
                live_voices.push(MidiVoice{bend_phase:super::note_bend::phase_coefficients(0.),track:index,on:voice.on,off,frequency:super::piano_tuning::frequency(voice.pitch,voice.tuning_cents),velocity:voice.velocity});
            }
        }
    }
    events.sort_by(|a, b| (a.0, a.1, a.2, a.3, a.4).cmp(&(b.0, b.1, b.2, b.3, b.4)));
    let mut buses = vec![[0f32; 4]; frames];
    let mut master = vec![[0f32; 2]; frames];
    let mut preview_pcm=input["preview_track"].as_str().map(|_|vec![[0f32;2];frames]);
    let mut freeze_pcm= input["freeze_track"].as_str().map(|_|vec![[0f32;2];frames]);
    let mut track_gains = vec![vec![0f32; frames]; tracks.len()];
    let mut observations: Vec<_> = if observe {
        tracks
            .iter()
            .map(|track| {
                (track.kind == "audio" || ((builtin || state.drum_rack.channel(&track.id).is_some() || state.plugins.has_track(&track.id,true)) && track.kind == "midi"))
                    .then(|| Observation::new(frames))
            })
            .collect()
    } else {
        Vec::new()
    };
// Process hosted chains in blocks, retaining their tails across consecutive
// producer ranges. Muted/solo-excluded tracks receive silence and note-offs.
let mut raw_pcm=Vec::with_capacity(tracks.len());
for (j,track) in tracks.iter().enumerate() {
    let mut pcm=vec![[0.;2];frames];
    for (i,sample) in pcm.iter_mut().enumerate(){let at=render_clock.at(i);
        for region in audio.iter().filter(|r|r.track==j){if at>=region.start&&at<region.end{
            *sample=region.source.stereo(region.offset+(at-region.start) as usize).map(|s|s*region.gain*super::clip_fades::gain(at-region.start,region.length,region.fade_in,region.fade_out,rate));}}
        let drum=if audible(track,any_solo){rack.sample(j,at,i)}else{[0.;2]};sample[0]+=drum[0];sample[1]+=drum[1];
        for voice in midi_voices.iter().filter(|v|v.track==j){let v=voice.sample(at,rate);sample[0]+=v;sample[1]+=v;}
        if let Some((capture_start,_))=live_midi {for voice in live_voices.iter().filter(|v|v.track==j){let v=voice.sample(capture_start+i as u64,rate);sample[0]+=v;sample[1]+=v;}}
        if track.kind=="audio"&&track.io.monitors_input()&&audible(track,any_solo){if let Some(inputs)=&live_inputs{*sample=inputs.sample(track.io,i);}}
    }
    raw_pcm.push(pcm);
}
let mut graph_gains=vec![vec![0.;frames];tracks.len()];
for i in 0..frames{let at=render_clock.at(i);for(j,t)in tracks.iter().enumerate(){graph_gains[j][i]=t.gain;}let mut applied=[false;64];for c in &controls{if !applied[c.owner]&&at>=c.start&&at<c.end{graph_gains[c.target][i]=fader_gain(automation::evaluate(&c.points,(at-c.start)as f64));applied[c.owner]=true;}}}
let (plugin_pcm,graph_stems,direct_matrix,channel_meters)=super::channels::render(state,&tracks,raw_pcm,&graph_gains,rate,&render_clock,monitor_only,|j,pcm|{
 let t=tracks[j];if !state.freeze.frozen(&t.id)&&state.plugins.has_track(&t.id,false){super::plugin_render::process_clocked(state,&t.id,pcm,rate,&render_clock,!audible(t,any_solo),live_midi,!monitor_only)?;}
 if !audible(t,any_solo){pcm.fill([0.;2]);}Ok(())
})?;
    let mut meter_peaks=[0f32;64];
    let mut master_meter_peaks=[0f32;4];
    let mut monitor_meter_peaks=[0f32;2];
    let mut track_automated = [false; 64];
    let mut gains = [0f32; 64];
    for (i, output) in buses.iter_mut().enumerate() {
        let at = render_clock.at(i);
        for (j, track) in tracks.iter().enumerate() {
            gains[j] = track.gain;
        }
        // First active clip per CC track, last active CC track per target.
        let mut applied = [false; 64];
        let mut automated = [false; 64];
        for c in &controls {
            if !applied[c.owner] && at >= c.start && at < c.end {
                gains[c.target] =
                    fader_gain(automation::evaluate(&c.points, (at - c.start) as f64));
                applied[c.owner] = true;
                automated[c.target] = true;
            }
        }
        let mut track_pcm = [[0f32; 2]; 64];
        for (j, track) in tracks.iter().enumerate() {
            track_pcm[j]=plugin_pcm[j][i];
            track_gains[j][i] = gains[j];
            track_automated[j] |= automated[j];
            if track.kind == "audio" || ((builtin || state.drum_rack.channel(&track.id).is_some() || state.plugins.has_track(&track.id,true)) && track.kind == "midi") {
                if observe {
                    observations[j]
                        .as_mut()
                        .unwrap()
                        .push(track_pcm[j], gains[j], automated[j]);
                }
                if input["freeze_track"]==track.id {if let Some(pcm)=freeze_pcm.as_mut(){pcm[i]=track_pcm[j];}}
                let post_gain=track_pcm[j].map(|v|v*gains[j]);
                if input["preview_track"]==track.id {if let Some(pcm)=preview_pcm.as_mut(){pcm[i]=post_gain;}}
                if observe{let mut routed=[0.;4];track.route.add(post_gain,&mut routed);meter_peaks[j]=meter_peaks[j].max(routed.iter().map(|v|v.abs()).fold(0.,f32::max));}
                // Final output is accumulated by the stereo routing graph.
            }
        }
        *output=graph_stems[i];
        for b in state.buses.rows.iter().filter(|b|b.kind=="matrix"){if let Some(feed)=direct_matrix.get(&b.id){output[b.input]+=feed[i];}}
        master[i] = routing::listen(*output).map(|v| v * master_gain);
        if observe {for b in 0..4{master_meter_peaks[b]=master_meter_peaks[b].max(output[b].abs());}for c in 0..2{monitor_meter_peaks[c]=monitor_meter_peaks[c].max(master[i][c].abs());}}
    }
    let (matrix_pcm,mix_monitor,mut bus_meters)=super::buses::render(state,&graph_stems,&direct_matrix,rate,&render_clock,master_gain,input["stem_export"]!=true)?;
    if let (Some(all),Some(channels))=(bus_meters.as_array_mut(),channel_meters.as_array()){all.extend(channels.iter().cloned());}
    master=mix_monitor;
    if observe{monitor_meter_peaks=[0.;2];for f in &master{for ch in 0..2{monitor_meter_peaks[ch]=monitor_meter_peaks[ch].max(f[ch].abs());}}}
    let visualization = if observe {
        json!({"revision":state.revision,"sample_rate":rate,
        "start_frame":start,"end_frame":end,"timeline_end_frame":render_clock.end(),"sequence":0,"master_meter_peaks":master_meter_peaks,"monitor_meter_peaks":monitor_meter_peaks,
        "buses":bus_meters,"tracks":tracks.iter().enumerate().filter(|(_,t)|t.kind!="cc").map(|(i,t)| {
            let mut observation = observations[i].as_ref().map(|o|o.snapshot(rate)).unwrap_or_else(||
                json!({"rms_pre_gain":null,"rms_post_gain":null,"rms_l":null,"rms_r":null,
                    "stereo_balance":null,"stereo_width":null,"spectral_centroid_hz":null,
                    "effective_gain":null,"automated":track_automated[i]}));
            observation["track"]=json!(t.id);observation["meter_peak"]=json!(meter_peaks[i]);
            observation["name"]=json!(t.name);
            observation["kind"]=json!(t.kind);
            observation["available"]=json!(t.kind=="audio" || ((builtin || state.plugins.has_track(&t.id,true)) && t.kind=="midi"));
            observation["audible"]=json!(audible(t,any_solo));
            observation["manual_gain"]=json!(t.gain);
            observation
        }).collect::<Vec<_>>()})
    } else {
        Value::Null
    };
    Ok(
        json!({"revision":state.revision,"sample_rate":rate,"start_frame":start,"end_frame":end,"timeline_end_frame":render_clock.end(),
        "bus_order":["L","S","FF","R"],"export_tap":"pre_stereo_master",
        "matrix_pcm":matrix_pcm,"preview_pcm":preview_pcm,"freeze_pcm":freeze_pcm,"lsffr":buses,"master_stereo":master,"master_gain":master_gain,
        "midi_events":events.into_iter().map(|e|e.5).collect::<Vec<_>>(),
        "active_notes_at_start":active_notes,"midi_notes_in_range":midi_notes,
        "visualization":visualization,
        "track_mix":tracks.iter().enumerate().filter(|(_,t)|t.kind!="cc").map(|(i,t)|json!({
            "track":t.id,"audible":audible(t,any_solo),"routing":t.route.snapshot(),"gain":track_gains[i]
        })).collect::<Vec<_>>(),
        "audio_complete":!needs_instrument||builtin,
        "instrument":if builtin{Some("cardinal_sine")}else{None},
        "midi_instrument_connected":builtin||state.drum_rack.channels.iter().any(|c|c.source.is_some())||tracks.iter().any(|t|state.plugins.has_track(&t.id,true)),"external_plugin_host_connected":state.plugins.any(),"audio_device_connected":false}),
    )
}

#[cfg(test)]
mod instrument_tests {
    use super::*;
    fn project() -> State {
        let mut state = State::default();
        for (hook, mut input) in [
            (
                "PutTrack",
                json!({"id":"notes","name":"Notes","kind":"midi"}),
            ),
            (
                "SetTrackMix",
                json!({"id":"notes","output_bus_l":"L","output_bus_r":"R"}),
            ),
            (
                "PutClip",
                json!({"id":"roll","track":"notes","kind":"midi","start_seconds":0.,"bpm":120.,"length_quarters":1.,"notes":[{"id":"a","quarter":0.,"length_quarters":0.2,"pitch":69,"velocity":1.}]}),
            ),
        ] {
            input["document_revision"] = json!(state.revision);
            super::super::document::dispatch(&mut state, hook, &input).unwrap();
        }
        state
    }
    #[test]
    fn authored_builtin_sounds_notes_and_preserves_sample_clock_across_blocks() {
        let s = project();
        let request = |start, frames| json!({"sample_rate":8000,"start_frame":start,"frames":frames,"instrument":"cardinal_sine"});
        let whole = render(&s, &request(0, 2048)).unwrap();
        assert_eq!(whole["midi_instrument_connected"], true);
        assert_eq!(whole["audio_complete"], true);
        let samples = whole["lsffr"].as_array().unwrap();
        assert!(samples.iter().any(|v| v[0].as_f64().unwrap().abs() > 0.01));
        assert!(
            samples
                .iter()
                .all(|v| v[1] == 0. && v[2] == 0. && v[0] == v[3])
        );
        assert!(samples[1800..].iter().all(|v| v[0] == 0.));
        let left = render(&s, &request(0, 817)).unwrap();
        let right = render(&s, &request(817, 1231)).unwrap();
        let joined: Vec<_> = left["lsffr"]
            .as_array()
            .unwrap()
            .iter()
            .chain(right["lsffr"].as_array().unwrap())
            .cloned()
            .collect();
        assert_eq!(whole["lsffr"], json!(joined));
        let crossing = samples[300..700]
            .windows(2)
            .filter(|p| p[0][0].as_f64().unwrap() <= 0. && p[1][0].as_f64().unwrap() > 0.)
            .count();
        assert!((21..=23).contains(&crossing));
    }
    #[test]
    fn event_only_render_stays_explicit_and_unknown_plugins_are_rejected() {
        let s = project();
        let mut request = json!({"sample_rate":8000,"start_frame":0,"frames":128});
        let plain = render(&s, &request).unwrap();
        assert_eq!(plain["midi_instrument_connected"], false);
        assert!(
            plain["lsffr"]
                .as_array()
                .unwrap()
                .iter()
                .all(|v| v == &json!([0., 0., 0., 0.]))
        );
        request["instrument"] = json!("missing_vst");
        assert!(render(&s, &request).is_err());
    }
}

#[cfg(test)]mod natural_loop_tests{
 use super::*;
 fn fixture(instrument:bool)->State{let mut s=State::default();let edit=|s:&mut State,h:&str,mut v:Value|{v["document_revision"]=json!(s.revision);super::super::document::dispatch(s,h,&v).unwrap();};
  edit(&mut s,"PutTrack",json!({"id":"t","name":"Track","kind":if instrument{"midi"}else{"audio"}}));
  if instrument{edit(&mut s,"PutClip",json!({"id":"c","track":"t","kind":"midi","start_seconds":0.,"bpm":120.,"length_quarters":4.,"notes":[{"id":"n","quarter":0.,"length_quarters":1.,"pitch":60,"velocity":1.}]}));}
  else{edit(&mut s,"PutSource",json!({"id":"s","sample_rate":8000,"channels":1,"samples":[1.,0.]}));edit(&mut s,"PutClip",json!({"id":"c","track":"t","kind":"audio","source":"s","start_seconds":0.,"source_offset_seconds":0.,"duration_seconds":2./8000.}));}
  s.plugins.test_install_tail("t",instrument);s
 }
 fn request(generation:u64,sequence:u64,frames:usize,length:u64)->Value{json!({"sample_rate":8000,"start_frame":0,"frames":frames,"playback_clock":{"generation":generation,"sequence":sequence},"loop_range":{"start_frame":0,"end_frame":length}})}
 #[test]fn hosted_effect_tail_survives_wraps_and_blocks_but_seek_resets(){let s=fixture(false);
  let a=render(&s,&request(1,0,4,2)).unwrap();let b=render(&s,&request(1,4,4,2)).unwrap();
  for (i,expected)in [1.,0.5,1.25,0.625].iter().enumerate(){assert_eq!(a["master_stereo"][i][0],*expected);}
  assert_eq!(b["master_stereo"][0][0],1.3125);assert_eq!(s.plugins.test_tail_counters().0,1);
  let c=render(&s,&request(2,8,4,2)).unwrap();assert_eq!(c["master_stereo"][0][0],1.);assert_eq!(s.plugins.test_tail_counters().0,2);
 }
 #[test]fn one_sample_loop_is_one_document_render_and_stateful_audio_is_not_repeated_pcm(){let s=fixture(false);let v=render(&s,&request(1,0,4096,1)).unwrap();assert_eq!(v["lsffr"].as_array().unwrap().len(),4096);assert_eq!(v["timeline_end_frame"],0);assert_eq!(v["master_stereo"][0][0],1.);assert_eq!(v["master_stereo"][1][0],1.5);assert_eq!(v["master_stereo"][4095][0],2.);assert_eq!(s.plugins.test_tail_counters(),(1,4096));}
 #[test]fn hosted_instrument_state_and_held_note_survive_natural_wrap(){let s=fixture(true);let v=render(&s,&request(1,0,4,1)).unwrap();for(i,want)in[1.,1.5,1.75,1.875].into_iter().enumerate(){assert_eq!(v["master_stereo"][i][0],want);}assert_eq!(s.plugins.test_tail_counters().0,1);}
}
