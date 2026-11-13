//! Document adapter for source-bound, nondestructive spectral repair.
//! All derived PCM is built on the control thread and shared immutably by Arc.
use super::{
    document::{Content, State},
    playback::Source,
    spectral_repair::{self, Edits, Mask},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
const MAX_CACHE_VALUES: usize = 16_777_216;
#[derive(Clone)]
pub(super) struct Treatment {
    pub source_id: String,
    original: Source,
    pub edits: Edits,
    pub processed: Source,
}
impl Treatment {
    fn build(source_id: String, source: &Source, edits: Edits) -> Result<Self, String> {
        let channels: Vec<Vec<f32>> = (0..source.channels)
            .map(|c| {
                source
                    .samples
                    .chunks_exact(source.channels)
                    .map(|f| f[c])
                    .collect()
            })
            .collect();
        let rendered = edits.render(&channels, source.sample_rate)?;
        let samples: Vec<f32> = (0..channels[0].len())
            .flat_map(|i| rendered.iter().map(move |c| c[i]))
            .collect();
        Ok(Self {
            source_id,
            original: source.clone(),
            edits,
            processed: Source {
                sample_rate: source.sample_rate,
                channels: source.channels,
                samples: samples.into(),
            },
        })
    }
}
pub(super) fn render_source<'a>(
    state: &'a State,
    clip_id: &str,
    source_id: &str,
) -> Result<&'a Source, String> {
    let original = state
        .sources
        .get(source_id)
        .ok_or_else(|| format!("missing decoded audio source {source_id}"))?;
    if let Some(t) = state.clip_treatments.get(clip_id) {
        if t.source_id != source_id
            || t.original.sample_rate != original.sample_rate
            || t.original.channels != original.channels
            || !Arc::ptr_eq(&t.original.samples, &original.samples)
        {
            return Err("spectral cache is stale; rebuild before rendering".into());
        }
        Ok(&t.processed)
    } else {
        Ok(original)
    }
}
fn mask_json(m: &Mask) -> Value {
    json!({"source_start_frame":m.start,"source_end_frame":m.end,"low_hz":m.low_hz,"high_hz":m.high_hz,"attenuation_db":m.attenuation_db,"feather":m.feather})
}
pub(super) fn snapshot(state: &State) -> Value {
    json!(state.clip_treatments.iter().map(|(id,t)|json!({"id":id,"source":t.source_id,"revision":t.edits.revision(),"spectral_masks":t.edits.masks().iter().map(mask_json).collect::<Vec<_>>()})).collect::<Vec<_>>())
}
fn numeric(input: &Value, key: &str) -> Result<f32, String> {
    input[key]
        .as_f64()
        .filter(|v| v.is_finite() && v.abs() <= f32::MAX as f64)
        .map(|v| v as f32)
        .ok_or_else(|| format!("finite {key} required"))
}
fn audio_source<'a>(state: &'a State, input: &Value) -> Result<(&'a str, &'a str), String> {
    let id = input["id"].as_str().ok_or("audio clip id required")?;
    let clip = state.clips.get(id).or_else(||state.audio_playlists.clip(id)).ok_or("unknown audio clip")?;
    match &clip.content {
        Content::Audio { source, .. } => Ok((&clip.id, source)),
        _ => Err("spectral repair requires an audio clip".into()),
    }
}
pub(super) fn edit(state: &mut State, hook: &str, input: &Value) -> Result<(), String> {
    let (id, source_id) = audio_source(state, input)?;
    let id = id.to_owned();
    let source_id = source_id.to_owned();
    let source = state
        .sources
        .get(&source_id)
        .ok_or("decoded source required for spectral repair")?;
    let mut edits = state
        .clip_treatments
        .get(&id)
        .map(|t| t.edits.clone())
        .unwrap_or_default();
    match hook {
        "AddSpectralMask" => {
            let m = &input["mask"];
            let mask = Mask {
                start: m["source_start_frame"]
                    .as_u64()
                    .ok_or("source_start_frame required")?,
                end: m["source_end_frame"]
                    .as_u64()
                    .ok_or("source_end_frame required")?,
                low_hz: numeric(m, "low_hz")?,
                high_hz: numeric(m, "high_hz")?,
                attenuation_db: numeric(m, "attenuation_db")?,
                feather: numeric(m, "feather")?,
            }
            .validated()?;
            edits.add(mask)?;
        }
        "UndoSpectralMask" => {
            edits.undo();
        }
        "ClearSpectralMasks" => {
            edits.clear();
        }
        _ => return Err("unknown spectral edit".into()),
    }
    if edits.masks().is_empty() {
        state.clip_treatments.remove(&id);
        return Ok(());
    }
    let retained: usize = state
        .clip_treatments
        .iter()
        .filter(|(key, _)| **key != id)
        .map(|(_, t)| t.processed.samples.len())
        .sum();
    if retained + source.samples.len() > MAX_CACHE_VALUES {
        return Err("spectral cache memory capacity reached".into());
    }
    let next = Treatment::build(source_id, source, edits)?;
    state.clip_treatments.insert(id, next);
    Ok(())
}
/// Prepare every affected cache before replacing source PCM. Failure leaves the
/// source, edit lists, all caches and the document revision unchanged.
pub(super) fn replace_source(
    state: &State,
    id: &str,
    source: &Source,
) -> Result<BTreeMap<String, Treatment>, String> {
    let mut next = state.clip_treatments.clone();
    let total: usize = next
        .values()
        .map(|t| {
            if t.source_id == id {
                source.samples.len()
            } else {
                t.processed.samples.len()
            }
        })
        .sum();
    if total > MAX_CACHE_VALUES {
        return Err("spectral cache memory capacity reached".into());
    }
    for treatment in next.values_mut().filter(|t| t.source_id == id) {
        if treatment.original.sample_rate != source.sample_rate {
            return Err("clear spectral masks before changing their source sample rate".into());
        }
        *treatment = Treatment::build(id.into(), source, treatment.edits.clone())?;
    }
    Ok(next)
}
/// Waveform is the original source, exactly as the old WAVEFORM/WARP view;
/// spectrogram is processed PCM, exactly as EnsureSpectrogram.
pub(super) fn preview(state: &State, input: &Value) -> Result<Value, String> {
    let (id, source_id) = audio_source(state, input)?;
    let source = state
        .sources
        .get(source_id)
        .ok_or("decoded source required")?;
    let Content::Audio{offset,duration,..}=&state.clips.get(id).or_else(||state.audio_playlists.clip(id)).ok_or("unknown audio clip")?.content else{unreachable!()};
    let start=(*offset*source.sample_rate as f64).round() as usize;
    let clip_frames=(*duration*source.sample_rate as f64).round() as usize;
    let frames=clip_frames.min((source.samples.len()/source.channels).saturating_sub(start));
    if frames == 0
        || start
            .checked_add(frames)
            .is_none_or(|end| end > source.samples.len() / source.channels)
    {
        return Err("clip exceeds decoded source frames".into());
    }
    let columns = input["columns"].as_u64().unwrap_or(240).clamp(1, if input["spectrogram"]==true {1000}else{2048}) as usize;
    let view = input.get("wave_view").and_then(Value::as_array);
    let lo = view.and_then(|v|v.first()).and_then(Value::as_f64).unwrap_or(0.);
    let hi = view.and_then(|v|v.get(1)).and_then(Value::as_f64).unwrap_or(1.);
    if !lo.is_finite() || !hi.is_finite() || lo < 0. || hi > 1. || hi <= lo {
        return Err("invalid waveform view".into());
    }
    let first = (lo * clip_frames as f64).floor() as usize;
    let last = ((hi * clip_frames as f64).ceil() as usize).min(frames);
    let visible = last.saturating_sub(first);
    let mut wave = Vec::with_capacity(columns);
    let mut stereo = Vec::with_capacity(columns);
    for x in 0..columns {
        let begin = first + x * visible / columns;
        let end = (first + (x + 1) * visible / columns).max(begin + 1).min(frames);
        let mut lo = 1f32;
        let mut hi = -1f32;
        let mut channels = [1f32,-1.,1.,-1.];
        for frame in begin..end {
            let at = (start + frame) * source.channels;
            let sample = source.samples[at..at + source.channels].iter().sum::<f32>()
                / source.channels as f32;
            lo = lo.min(sample);
            hi = hi.max(sample);
            for channel in 0..2 {let sample=source.samples[at+channel.min(source.channels-1)].clamp(-1.,1.);channels[channel*2]=channels[channel*2].min(sample);channels[channel*2+1]=channels[channel*2+1].max(sample);}
        }
        wave.push(if begin<frames {[lo.clamp(-1., 1.), hi.clamp(-1., 1.)]} else {[0.,0.]});
        stereo.push(if begin<frames {channels} else {[0.;4]});
    }
    let masks = state
        .clip_treatments
        .get(id)
        .map(|t| t.edits.masks().iter().map(mask_json).collect::<Vec<_>>())
        .unwrap_or_default();
    let mut result = json!({"id":id,"revision":state.revision,"sample_rate":source.sample_rate,"source_start_frame":start,"frames":frames,"clip_frames":clip_frames,"waveform":wave,"spectral_masks":masks,"warp":super::warp_treatment::snapshot(state)});
    result["wave_view"]=json!([lo,hi]);
    result["stereo_waveform"]=json!(stereo);
    if input["transients"]==true{
        let marks=state.transient_cache.borrow_mut().marks(source)?;
        result["transients"]=json!(marks.iter().map(|m|json!({"source_frame":m.source_frame,"spectral_flux":m.spectral_flux,"strength":m.strength,"confidence":m.confidence})).collect::<Vec<_>>());
    }
    if input["spectrogram"] == true {
        let source = render_source(state, id, source_id)?;
        let channels: Vec<Vec<f32>> = (0..source.channels)
            .map(|c| {
                source
                    .samples
                    .chunks_exact(source.channels)
                    .map(|f| f[c])
                    .collect()
            })
            .collect();
        let bins = input["bins"].as_u64().unwrap_or(80).min(160) as usize;
        let spectrogram = spectral_repair::spectrogram(
            &channels,
            source.sample_rate,
            start,
            frames,
            columns,
            bins,
        )?;
        result["spectrogram"] = json!({"columns":spectrogram.columns,"bins":spectrogram.bins,"intensity":spectrogram.intensity});
    }
    Ok(result)
}
