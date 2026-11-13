//! Layer-five-owned bridge. Applies real document edits and caches preview data.
//! Capture and warp ownership is explicit; unsupported actions report a notice
//! and remain in pending for the owning integration rather than pretending success.
use super::{
    bottom_panel::{self, Action, Page, Rect},
    bottom_panel_adapter,
};
use crate::host::Host;
use serde_json::{Value, json};
#[derive(Default)]
pub(crate) struct Runtime {
    pub controls: bottom_panel::State,
    pub packet: Value,
    pub notice: Option<String>,
    pub pending: Vec<Action>,
    pub recalled_clip:Option<String>,
    capture_status:Value,
    pub selected_bus:Option<String>,
    delete_held:bool,
    delete_consumed:bool,
    undo_held:bool,
    redo_held:bool,
    preview_key: Option<(u64, String, u8, [u64;2], usize)>,
    preview: Value,
    meters:super::mixer_meters::State,
}
impl Runtime {
    pub(crate) fn cancel(&mut self, left: bool) {
        self.controls.cancel(left);
    }
    pub(crate) fn update(
        &mut self,
        host: &mut Host,
        raw: &Value,
        panel: Rect,
        selected_clip: Option<&str>,
        plugin_track: Option<&str>,
        dt: f64,
        blocked: bool,
        metering:Option<&Value>,
        selected_track:Option<&str>,
    ) -> Result<(bool, bool), String> {
        self.recalled_clip=None;
        self.capture_status=match super::retrospective_runtime::update_display(host,&mut self.controls.loopback){Ok(status)=>status,Err(error)=>{self.notice=Some(error);Value::Null}};
        let undo=raw["undo"]==true;let redo=raw["redo"]==true;
        let keyboard_action=if self.controls.page==Page::Loopback&&!blocked {if undo&&!self.undo_held{Some("undo")}else if redo&&!self.redo_held{Some("redo")}else{None}}else{None};
        self.undo_held=undo;self.redo_held=redo;
        if let Some(action)=keyboard_action{match host.invoke("EnvironmentAudioSystem.Retrospective",json!({"action":action})){Ok(result)=>{self.recalled_clip=result["selected_clip"].as_str().map(str::to_owned);self.notice=None;},Err(error)=>self.notice=Some(error)}}
        let before = self.packet.clone();
        let was_captured = self.controls.consumes_pointer();
        let input = bottom_panel_adapter::input(raw);
        host.invoke("WorkstationDataSystem.PumpWarp",Value::Null)?;
        let mut snapshot = host.invoke("WorkstationDataSystem.Snapshot", Value::Null)?;
        let delete=raw["delete"]==true;
        let deleting=!blocked&&self.controls.page==Page::Mixer&&delete&&!self.delete_held&&self.selected_bus.is_some();
        self.delete_held=delete;
        if !delete{self.delete_consumed=false;}
        if deleting{self.delete_consumed=true;}
        if deleting {let id=self.selected_bus.clone().unwrap();self.notice=host.invoke("WorkstationDataSystem.RemoveBus",json!({"document_revision":snapshot["revision"],"id":id})).err();snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;}
        if self.selected_bus.as_ref().is_some_and(|id|!snapshot["buses"].as_array().is_some_and(|a|a.iter().any(|b|b["id"]==*id))){self.selected_bus=None;}
        let mut context = bottom_panel_adapter::context(&snapshot, selected_clip, 0);
        context.selected_bus=self.selected_bus.clone();
        context.plugin_track_index = context
            .tracks
            .iter()
            .position(|t| Some(t.id.as_str()) == plugin_track)
            .unwrap_or_else(|| snapshot["buses"].as_array().into_iter().flatten().filter(|b|b["kind"]=="mix").position(|b|b["id"].as_str()==plugin_track).map_or(0,|i|context.tracks.len()+i));
        let mut actions = Vec::new();
        if blocked {
            self.controls.cancel(input.left);
        } else {
            for event in raw["transport_pointer_events"].as_array().into_iter().flatten().take(16){
                let Some(point)=event["pointer"].as_array().and_then(|p|Some([p.first()?.as_f64()?,p.get(1)?.as_f64()?]))else{continue;};
                let mut edge=input.clone();edge.pointer=Some(point);edge.left=event["left"]==true;
                actions.extend(self.controls.update(&edge,panel,&context));
            }
            actions.extend(self.controls.update(&input, panel, &context));
        }
        let handled = self.delete_consumed || deleting || (delete&&self.selected_bus.is_some()&&self.controls.page==Page::Mixer) || keyboard_action.is_some() || was_captured || self.controls.consumes_pointer() || !actions.is_empty();
        self.pending.clear();
        for mut action in actions {
            if let Action::SelectTrack(id)=&action{if context.buses.iter().any(|b|b["id"]==*id&&b["kind"]=="channel"){action=Action::SelectBus(id.clone());}}
            if let Action::SelectTrack(_)=&action {self.selected_bus=None;self.pending.push(action);continue;}
            if let Action::SelectBus(id)=&action {self.selected_bus=Some(id.clone());self.notice=snapshot["buses"].as_array().into_iter().flatten().find(|b|b["id"]==*id).and_then(|b|b["name"].as_str()).map(|name|format!("{name} selected — Delete removes bus"));context.selected_bus=self.selected_bus.clone();self.pending.push(action);continue;}
            if let Action::EditorLoop{start,end}=&action {
                let status=host.invoke("EnvironmentAudioSystem.Status",Value::Null)?;
                if let Some(rate)=status["transport"]["sample_rate"].as_u64().filter(|r|*r>0){self.notice=host.invoke("EnvironmentAudioSystem.Transport",json!({"action":"loop","enabled":true,"loop_start_frame":(start*rate as f64).round()as u64,"loop_end_frame":(end*rate as f64).round()as u64})).err();}else{self.notice=Some("Audio transport is not prepared".into());}continue;
            }
            let capture_request=match &action {
                Action::RecallAudio{seconds}=>Some(json!({"action":"recall_audio","seconds":seconds,"track":selected_track})),
                Action::RecallMidi{seconds}=>Some(json!({"action":"recall_midi","seconds":seconds,"track":selected_track,"clip_id":selected_clip})),
                Action::SetCaptureSource(source)=>Some(json!({"action":"source","source":source.name()})),
                Action::ClearCapture=>Some(json!({"action":"clear"})),_=>None};
            if let Some(request)=capture_request {
                match host.invoke("EnvironmentAudioSystem.Retrospective",request){
                    Ok(result)=>{self.recalled_clip=result["selected_clip"].as_str().map(str::to_owned);self.notice=result["notice"].as_str().map(str::to_owned);if matches!(action,Action::ClearCapture){self.controls.loopback.clear();}},Err(error)=>self.notice=Some(error)
                }
                snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
                context=bottom_panel_adapter::context(&snapshot,selected_clip,context.plugin_track_index);
                self.controls.sync(&context);
            } else if let Some(mut request) = bottom_panel_adapter::request(&action) {
                if request.input.get("document_revision").is_some(){request.input["document_revision"]=snapshot["revision"].clone();}
                self.notice = host.invoke(request.hook, request.input).err();
                snapshot = host.invoke("WorkstationDataSystem.Snapshot", Value::Null)?;
                context = bottom_panel_adapter::context(
                    &snapshot,
                    selected_clip,
                    context.plugin_track_index,
                );
                self.controls.sync(&context);
            } else {
                self.notice = match &action {
                    Action::SetClipWarp { .. } => {
                        Some("Warp requires the Cardinal map and pitch-preserving renderer.".into())
                    }
                    Action::RecallAudio { .. }
                    | Action::RecallMidi { .. }
                    | Action::SetCaptureSource(_)
                    | Action::ClearCapture
                    | Action::CopyLoopback { .. } => {
                        Some("Retrospective capture owner has not been connected.".into())
                    }
                    Action::NextSoundtrack | Action::NextSpeaker => {
                        Some("Soundtrack service has not been connected.".into())
                    }
                    _ => None,
                };
                self.pending.push(action);
            }
        }
        self.controls.advance_pressure(dt);
        let spectral = self.controls.editor_tab == bottom_panel::EditorTab::Spectral;
        let wave_view=if self.controls.editor_tab==bottom_panel::EditorTab::Waveform {self.controls.wave_view}else{[0.,1.]};
        let columns=self.controls.layout(panel,&context).view.w.ceil().clamp(1.,if spectral {1000.}else{2048.}) as usize;
        let key = context
            .selected_audio
            .as_ref()
            .map(|clip| (context.revision, clip.id.clone(), self.controls.editor_tab as u8, wave_view.map(f64::to_bits),columns));
        if self.controls.page == Page::ClipEditor && key != self.preview_key {
            self.preview = Value::Null;
            if let Some((revision, id, _tab,_,_)) = &key {
                match host.invoke("WorkstationDataSystem.ClipEditorPreview",json!({"document_revision":revision,"id":id,"columns":columns,"wave_view":wave_view,"bins":100,"spectrogram":spectral,"transients":self.controls.editor_tab==bottom_panel::EditorTab::Warp})) {
                    Ok(preview)=>self.preview=preview,Err(error)=>self.notice=Some(error),
                }
            }
            self.preview_key = key;
        }
        let observation=metering.filter(|v|v.is_object()).unwrap_or(&snapshot["visualization"]);
        self.meters.apply(&mut context,&snapshot,observation,dt);
        let layout = self.controls.layout(panel, &context);
        self.packet = bottom_panel_adapter::presentation(&self.controls, &layout);
        self.packet["track_levels"]=json!(context.tracks.iter().map(|t|json!({"track":t.id,"levels":t.meter})).collect::<Vec<_>>());
        self.packet["selected_bus"]=json!(self.selected_bus);
        self.packet["fader_drag"] = json!(self.controls.dragging_fader());
        self.packet["preview"] = self.preview.clone();
        if let Some((id,markers))=self.controls.warp_preview(){if let Some(entry)=self.packet["preview"]["warp"].as_array_mut().and_then(|a|a.iter_mut().find(|e|e["id"]==id)){entry["markers"]=markers.clone();}}

        self.packet["loopback"]["waveform"] = json!(
            self.controls
                .loopback
                .waveform(layout.view.w.clamp(1., 1000.) as usize)
        );
        self.packet["loopback"]["capture_status"]=self.capture_status.clone();
        self.packet["notice"] = json!(self.notice);
        Ok((handled, before != self.packet))
    }
}
