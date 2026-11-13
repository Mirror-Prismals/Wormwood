//! JSON/Host boundary for the pure Cardinal bottom-panel controller.
//! Layer five applies returned requests in order, refreshes Snapshot after each,
//! and routes non-document actions to their existing owner. No effects are
//! silently marked successful by this adapter.
use super::bottom_panel::{self, Action, AudioClip, Context, Input, Layout, Track};
use serde_json::{Value, json};
#[derive(Debug)]
pub(crate) struct Request {
    pub hook: &'static str,
    pub input: Value,
}
pub(crate) fn input(value: &Value) -> Input {
    let pointer = value["pointer"]
        .as_array()
        .filter(|a| a.len() == 2)
        .and_then(|a| Some([a[0].as_f64()?, a[1].as_f64()?]))
        .filter(|p| p.iter().all(|x| x.is_finite()));
    let wheel = value["wheel"][1]
        .as_f64()
        .filter(|x| x.is_finite())
        .unwrap_or(0.);
    Input {
        pointer,
        left: value["left"] == true,
        wheel,
        command: value["command"] == true,
        focused: value["focused"] != false,
        cancelled: value["escape"] == true || value["pointer_cancelled"] == true || value["transport_pointer_cancelled"] == true,
    }
}
pub(crate) fn context(
    snapshot: &Value,
    selected_audio: Option<&str>,
    plugin_track_index: usize,
) -> Context {
    let mut tracks = Vec::new();
    // The original mixer groups audio strips before MIDI strips; CC lanes have
    // no audio faders. Each entry keeps its stable identity through reorder.
    for kind in ["audio", "midi"] {
        for track in snapshot["tracks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| t["kind"] == kind)
        {
            if let Some(id) = track["id"].as_str() {
                tracks.push(Track {
                    id: id.into(),
                    label: track["name"].as_str().unwrap_or(id).into(),
                    gain: track["gain"].as_f64().unwrap_or(1.),
                    meter: [0.; 2],
                });
            }
        }
    }
    for b in snapshot["buses"].as_array().into_iter().flatten().filter(|b|b["kind"]=="channel") {if let Some(id)=b["id"].as_str(){tracks.push(Track{id:id.into(),label:b["name"].as_str().unwrap_or(id).into(),gain:b["gain"].as_f64().unwrap_or(1.),meter:[0.;2]});}}
    let selected_audio = selected_audio
        .and_then(|id| {
            snapshot["clips"].as_array().into_iter().flatten()
                .chain(snapshot["audio_playlists"].as_array().into_iter().flatten().flat_map(|t|t["lanes"].as_array().into_iter().flatten().skip(1)).flat_map(|l|l["clips"].as_array().into_iter().flatten()))
                .find(|c| c["id"]==id && c["kind"]=="audio")
        })
        .and_then(|clip| {
            let source = snapshot["sources"]
                .as_array()?
                .iter()
                .find(|s| s["id"] == clip["source"])?;
            let rate = source["sample_rate"].as_u64()?;
            if !(8000..=192000).contains(&rate) {
                return None;
            }
            let offset = clip["source_offset_seconds"].as_f64()?;
            let duration = clip["duration_seconds"].as_f64()?;
            if !offset.is_finite() || !duration.is_finite() || offset < 0. || duration <= 0. {
                return None;
            }
            let id = clip["id"].as_str()?;
            let masks = snapshot["clip_treatments"]
                .as_array()
                .and_then(|a| a.iter().find(|t| t["id"] == id))
                .and_then(|t| t["spectral_masks"].as_array())
                .map_or(0, Vec::len);
            Some(AudioClip {
                trackless:clip["track"]=="",output:clip["output"].as_str().unwrap_or("OFF").into(),
                id: id.into(),
                source_offset: (offset * rate as f64).round() as u64,
                frames: (duration * rate as f64).round() as u64,
                sample_rate: rate as u32,
                gain:clip["gain"].as_f64().unwrap_or(1.),start_seconds:clip["start_seconds"].as_f64().unwrap_or(0.),
                warp:snapshot["warp"].as_array().and_then(|a|a.iter().find(|e|e["id"]==id)).cloned().unwrap_or(Value::Null),
                warp_enabled: clip["warp_enabled"] == true,
                spectral_masks: masks,
            })
        });
    Context {
        revision: snapshot["revision"].as_u64().unwrap_or(0),
        plugin_track_count: tracks.len()+snapshot["buses"].as_array().into_iter().flatten().filter(|b|b["kind"]=="mix").count(),
        tracks,
        selected_audio,
        plugin_track_index,
        buses:snapshot["buses"].as_array().cloned().unwrap_or_default(),
        selected_bus:None,
        master_meters: [0.; 6],
    }
}
pub(crate) fn request(action: &Action) -> Option<Request> {
    let (hook, input) = match action {
        Action::ClipEdit{revision,id,hook,input}=>{let mut input=input.clone();input["document_revision"]=json!(revision);input["id"]=json!(id);(*hook,input)},
        Action::SetTrackGain { revision, id, gain } => (
            "WorkstationDataSystem.SetTrackMix",
            json!({"document_revision":revision,"id":id,"gain":gain}),
        ),
        Action::AddSpectralMask { revision, id, mask } => (
            "WorkstationDataSystem.AddSpectralMask",
            json!({"document_revision":revision,"id":id,"mask":{"source_start_frame":mask.start,"source_end_frame":mask.end,"low_hz":mask.low_hz,"high_hz":mask.high_hz,"attenuation_db":mask.attenuation_db,"feather":mask.feather}}),
        ),
        Action::UndoSpectralMask { revision, id } => (
            "WorkstationDataSystem.UndoSpectralMask",
            json!({"document_revision":revision,"id":id}),
        ),
        Action::ClearSpectralMasks { revision, id } => (
            "WorkstationDataSystem.ClearSpectralMasks",
            json!({"document_revision":revision,"id":id}),
        ),
        Action::SetClipWarp{revision,id,enabled}=>("WorkstationDataSystem.SetClipWarp",json!({"document_revision":revision,"id":id,"enabled":enabled})),
        // Capture/plugin/music actions belong to those owners. Never emit
        // a made-up Host hook or claim success from a visual state toggle.
        _ => return None,
    };
    Some(Request { hook, input })
}
pub(crate) fn presentation(state: &bottom_panel::State, layout: &Layout) -> Value {
    let rect = |r: bottom_panel::Rect| [r.x, r.y, r.w, r.h];
    let id = |c: &bottom_panel::ControlId| format!("{c:?}");
    json!({"page":state.page.index(),"editor_tab":format!("{:?}",state.editor_tab),"panel":rect(layout.panel),"view":rect(layout.view),"content":rect(layout.content),
        "controls":layout.controls.iter().map(|c|json!({"id":id(&c.id),"label":c.label,"rect":rect(c.rect),"active":c.active,"enabled":c.enabled,"pressure":c.pressure})).collect::<Vec<_>>(),
        "faders":layout.faders.iter().map(|f|json!({"track":f.track,"housing":rect(f.housing),"knob":rect(f.knob),"normalized":f.normalized,"meters":f.meters.map(rect),"levels":f.levels})).collect::<Vec<_>>(),
        "master_meters":layout.master_meters.iter().map(|(label,r,level)|json!({"label":label,"rect":rect(*r),"level":level})).collect::<Vec<_>>(),
        "warp_selected":state.warp_selected,"wave_view":state.wave_view,"wave_loop":state.wave_loop,"wave_selection":state.wave_selection,"spectral_selection":state.spectral_selection.map(|s|[s.x0,s.x1,s.f0,s.f1]),"attenuation_db":state.attenuation_db,"feather":state.feather,
        "loopback":{"source":state.loopback.source().name(),"memory_seconds":state.loopback.memory_seconds(),"sample_rate":state.loopback.rate(),"oldest_frame":state.loopback.oldest(),"head_frame":state.loopback.head(),"selection":state.loopback.selection().map(|s|[s.start,s.end])}})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn strips_exclude_cc_and_keep_stable_ids_audio_first() {
        let snapshot = json!({"revision":8,"tracks":[{"id":"m","kind":"midi","name":"Piano"},{"id":"cc","kind":"cc"},{"id":"a","kind":"audio","name":"Voice","gain":0.5}]});
        let c = context(&snapshot, None, 0);
        assert_eq!(
            c.tracks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["a", "m"]
        );
        assert_eq!(c.plugin_track_count, 2);
    }
    #[test]
    fn unsupported_owners_never_become_fake_successful_requests() {
        assert!(
            request(&Action::SetClipWarp {
                revision: 1,
                id: "c".into(),
                enabled: true
            })
            .is_some()
        );
        assert!(request(&Action::RecallMidi { seconds: 30 }).is_none());
        let r = request(&Action::SetTrackGain {
            revision: 9,
            id: "t".into(),
            gain: 0.5,
        })
        .unwrap();
        assert_eq!(r.hook, "WorkstationDataSystem.SetTrackMix");
        assert_eq!(r.input["document_revision"], 9);
    }
}
