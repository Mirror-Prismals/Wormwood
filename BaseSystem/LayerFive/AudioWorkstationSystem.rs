//! Workstation application and floating bubble-note editing. Layer five owns
//! input, document edits and Audicle lifetime; layer four owns mesh presentation.
use crate::{
    host::Host,
    workstation::{panel_motion::{Edge, MobilePage, State as Motion},
        panel_layout::{Options, Viewport}, piano_roll::Editor, piano_roll_scene, spatial_mix,
        arrangement::{self, Arrangement, Action}, browser},
};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[cfg(target_os="ios")]
static RETURN_TO_GAME:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
#[cfg(target_os="ios")]
static TRANSPORT_TAPS:std::sync::Mutex<Vec<[f64;2]>>=std::sync::Mutex::new(Vec::new());
#[cfg(target_os="ios")]
#[unsafe(no_mangle)]
pub extern "C" fn ww_workstation_transport_tap(x:f64,y:f64){
    if x.is_finite() && y.is_finite() {if let Ok(mut taps)=TRANSPORT_TAPS.lock(){if taps.len()<8{taps.push([x,y]);}}}
}
#[cfg(target_os="ios")]
#[unsafe(no_mangle)]
pub extern "C" fn ww_workstation_return_to_game(){RETURN_TO_GAME.store(true,std::sync::atomic::Ordering::Release);}
#[cfg(target_os="ios")]
fn take_return_to_game()->bool {RETURN_TO_GAME.swap(false,std::sync::atomic::Ordering::AcqRel)}
#[cfg(target_os="ios")]
unsafe extern "C" {fn ww_ios_workstation_focus(active:bool);}

// Match the authored ruby control poses, including Origami animation and
// overview scaling. A transparent point between buttons is never a hit.
fn transport_hit(state:&State,viewport:[f64;2],pointer:[f64;2])->Option<usize> {
    use crate::workstation::panel_layout;
    if !pointer.iter().all(|p|p.is_finite()) || pointer[0]<0. || pointer[1]<0. || pointer[0]>=viewport[0] || pointer[1]>=viewport[1]{return None;}
    let view=Viewport {width:viewport[0],height:viewport[1],canvas_scale:state.canvas_scale};
    let options=state.origami_control.mode.options(state.swap_sides,state.swap_horizontal);
    let surface=panel_layout::resolved_surface(view,&state.motion.snapshot(),options,0).ok()?;
    let mpp=1.3/view.height;
    let safe=surface.size;
    let scale=(safe[0]*0.88/0.205).min(safe[1]*0.80/0.04);
    let center=[view.width*0.5+surface.origin[0]/mpp,view.height*0.5-surface.origin[1]/mpp];
    (0..4).find(|i| {
        let x=center[0]+(*i as f64-1.5)*0.055*scale/mpp;
        (pointer[0]-x).abs()<=0.020*scale/mpp && (pointer[1]-center[1]).abs()<=0.020*scale/mpp
    })
}

// Desktop needs a visible pointer. On iOS InputMode is the gameplay pause
// switch, so the workstation must not request it at all.
fn workstation_pointer_mode(ios: bool) -> Option<bool> {
    (!ios).then_some(true)
}

fn wearable_player_view(kind:Option<&str>,frozen:&Value,live:&Value,passthrough:bool,cameras:bool,aspect:f64)->Option<Value>{
    let mut view=match kind {Some("ar")=>frozen.clone(),Some("vr") if passthrough && !cameras=>live.clone(),_=>return None};
    // The wearable may move its observer while the game world remains paused.
    if kind==Some("vr") {for key in ["time","time_override_minutes","water_instances","instances"]{if let Some(value)=frozen.get(key){view[key]=value.clone();}}}
    view["aspect"]=json!(aspect);view["id"]=json!("player");Some(view)
}

const STEP_STAGE_NAMES:[&str;9]=["input_motion_canvas","before_presentations","audio_status_document_snapshot",
    "arrangement_spatial_sync","browser_plugins","gestures_editor","after_presentation_caption_change",
    "view_lifecycle","status_and_value_cleanup"];
struct StepClock {last:Option<Instant>,values:[f64;9],next:usize}
impl StepClock {
    fn new(enabled:bool)->Self {Self{last:enabled.then(Instant::now),values:[0.;9],next:0}}
    fn mark(&mut self){if let Some(last)=&mut self.last {let now=Instant::now();
        self.values[self.next]=now.duration_since(*last).as_secs_f64()*1000.;*last=now;self.next+=1;}}
}

#[derive(Default)]
pub(crate) struct State {
    visual: Option<Value>,
    style_animated: bool,
    style_time: f64,
    style_next_draw: f64,
    bottom:crate::workstation::bottom_panel_runtime::Runtime,
    bottom_open_requested:bool,
    plugins: crate::workstation::plugin_inserts::Ui,
    track_io: crate::workstation::track_io::Ui,
    track_io_error: Option<String>,
    midi_sender:crate::workstation::midi_keyboard::Sender,
    midi_sender_error:Option<String>,
    midi_meter_enabled:Option<bool>,
    keyboard_controls:crate::workstation::keyboard_toggle::Ui,
    event: Option<Value>,
    motion: Motion,
    previous: [bool; 5],
    sfx_pointer_down: bool,
    sfx_started: bool,
    sfx_gains: Option<Value>,
    viewport: Option<[f64; 2]>,
    framebuffer: Option<[f64; 2]>,
    canvas_scale: f64,
    swap_sides: bool,
    swap_horizontal: bool,
    frames: u64,
    presentation_keys:bool,
    shared_snapshot:bool,
    step_timings:Option<Vec<[f64;9]>>,
    step_timings_omitted:u64,
    editor: Editor,
    editor_error: Option<String>,
    editor_left_held: bool,
    retained: bool,
    spatial_mix: spatial_mix::State,
    spatial_error: Option<String>,
    arrangement: Arrangement,
    track_freeze:crate::workstation::track_freeze::Workflow,
    arrangement_error: Option<String>,
    piano_workspace:crate::workstation::piano_workspace::Workspace,
    drum_ui:crate::workstation::drum_rack_ui::Ui,
    spatial_view: bool,
    view_held: bool,
    playhead_seconds: Option<f64>,
    browser_capture: bool,
    browser_selection: browser::Selection,
    plugin_browser: crate::workstation::plugin_browser::Ui,
    audio_import: crate::workstation::audio_import::Importer,
    plugin_surface: Value,
    browser_ghost: Value,
    range_overlay:Value,
    cc_picker_surface:Value,
    caption: String,
    transport_controls:crate::workstation::transport_controls::Controls,
    transport_notice:Option<String>,
    loop_control:crate::workstation::loop_control::Control,
    audio_startup_error:Option<String>,
    origami_control:crate::workstation::origami_control::Control,
    settings:crate::workstation::settings::Settings,
    tutorial:crate::workstation::tutorial::Tutorial,
    camera_view:Value,
    embedded:bool,
    pub(crate) audio_rays:bool,
    eyewear:Option<String>,
    frozen_view:Value,
    headset_camera:bool,
    computer_screen:bool,
    resident:bool,
    parked:bool,
    background_next:Option<Instant>,
    camera_cycle_down:bool,
}

impl State {
    fn browser_enabled(&self)->bool { !cfg!(target_os="macos") || self.plugin_browser.enabled }
    #[cfg(test)] pub(crate) fn frozen_observer(&self)->&Value { &self.frozen_view }
    #[cfg(test)] pub(crate) fn mobile_test_layout(&mut self){self.motion.set_mobile_page(Some(MobilePage::Timeline));self.viewport=None;}
    fn desktop_panel_navigation(&mut self, window:&Value, editor_input:&mut Value)->([bool;5],bool) {
        // A loaded clip survives Tab/Escape so its viewport can be restored.
        // Only the visible piano workspace owns the panel navigation keys.
        let editing=self.piano_workspace.active || self.drum_ui.open || self.plugin_browser.search_active;
        let pointer=editor_input["pointer"].as_array().and_then(|p|Some([p.first()?.as_f64()?,p.get(1)?.as_f64()?]));
        let bar_shift=self.arrangement.wants_bar_shift(pointer);
        let mut overview=self.canvas_scale<0.999;
        if !editing && !bar_shift && window["flight"]["workstation"]["overview"]==true {
            let wheel=editor_input["wheel"][1].as_f64().unwrap_or(0.);
            if (!overview && wheel<0.) || (overview && wheel>0.) {
                overview=wheel<0.;
            }
            // Own the complete Shift-wheel gesture, including repeated ticks
            // at either overview limit. Leaking those ticks pans the timeline
            // away from its clips after the first tick changes the layout.
            editor_input["wheel"]=json!([0.,0.]);
        }
        if editing {overview=false;}
        let keys=std::array::from_fn(|i| !editing && i<4 && window["flight"]["workstation"]["panels"][i]==true);
        // Space remains the shared audio transport.
        (keys,overview)
    }
    fn consumes_escape(&self)->bool {
        self.plugin_browser.search_active || self.motion.fullscreen_target().is_some() || self.motion.fullscreen_animating() || self.settings.open || self.drum_ui.open || self.piano_workspace.active || self.spatial_mix.is_interacting()
            || self.arrangement.is_interacting() || (!self.spatial_view && self.arrangement.has_range()) || (self.browser_selection.owns_pointer() || self.plugin_browser.owns_pointer())
    }
    fn status(&self) -> Value {
        let panels=Edge::ALL.map(|edge| json!({"edge":edge.name(),
            "open":self.motion.is_open(edge),"extension":self.motion.extension(edge),
            "visual_extension":self.motion.effective_extension(edge),"expanded":self.motion.is_expanded(edge)}));
        let mut status=json!({"open":self.visual.is_some(),"visual":self.visual,"animating":self.event.is_some(),
            "drum_rack_open":self.drum_ui.open,"drum_rack_notice":self.drum_ui.notice,
            "browser":self.browser_selection.status(),"browser_enabled":self.browser_enabled(),"viewports":self.arrangement.viewports.status(),"bottom_panel":self.bottom.packet,"track_io":self.track_io.presentation(),"track_io_error":self.track_io_error,
            "mix_preview":{"phase":self.piano_workspace.mix_preview["phase"],"revision":self.piano_workspace.mix_preview["revision"],"generation":self.piano_workspace.mix_preview["generation"],"progress":self.piano_workspace.mix_preview["progress"],"error":self.piano_workspace.mix_preview["error"]},
            "computer_midi_keyboard":self.midi_sender.status(),
            "computer_keyboard_controls":self.keyboard_controls.presentation(self.midi_sender.enabled()),"midi_sender_error":self.midi_sender_error,
            "tutorial":self.tutorial.status(),"security_camera":self.camera_view,"settings_open":self.settings.open,"lightbox":self.settings.packet(),
            "fullscreen_panel":self.motion.fullscreen_target().map(Edge::name),"panel_view":self.motion.panel_view(),"origami_mode":self.origami_control.mode.caption(),"mobile_page":self.motion.mobile_page().map(MobilePage::name),"mobile_animating":self.motion.mobile_animating(),
            "transport_pressure":self.transport_controls.pressure,
                "loop_control":{"enabled":self.arrangement.loop_enabled(),"pressure":self.loop_control.pressure},"transport_notice":self.transport_notice,
            "audio_startup_error":self.audio_startup_error,
            "frames":self.frames,"piano_roll":self.editor.status(),"piano_workspace":self.piano_workspace.status(),"editor_error":self.editor_error,
            "spatial_mix":self.spatial_mix.status(),"spatial_error":self.spatial_error,
            "arrangement":self.arrangement.status(),"arrangement_error":self.arrangement_error,
            "spatial_view":self.spatial_view,"playhead_seconds":self.playhead_seconds,
            "panels":panels});
        status["resident"]=json!(self.resident);status["parked"]=json!(self.parked);status["computer_screen"]=json!(self.computer_screen);status["audio_rays"]=json!(self.audio_rays);status["eyewear"]=json!(self.eyewear);status["headset_camera"]=json!(self.headset_camera);status
    }
    fn begin_event(&mut self, host: &mut Host) -> Result<(), String> {
        if let Some(event) = &self.event {
            host.invoke("AudicleSystem.Resume", event.clone())?;
        } else {
            let event = host.invoke("AudicleSystem.Create", Value::Null)?;
            self.event = Some(event.clone());
            host.invoke("AudicleSystem.Ready", event.clone())?;
            host.invoke("AudicleSystem.Go", event)?;
        }
        Ok(())
    }
    fn view(
        &mut self,
        host: &mut Host,
        viewport: [f64; 2],
        framebuffer: [f64; 2],
        capture: bool,
    ) -> Result<(), String> {
        let motion = self.motion.snapshot();
        let view=Viewport {width:viewport[0],height:viewport[1],canvas_scale:self.canvas_scale};
        let options=self.origami_control.mode.options(self.swap_sides,self.swap_horizontal);
        let rect=if self.piano_workspace.active || self.drum_ui.open {crate::workstation::piano_workspace::outer(view)}else{piano_roll_scene::render_canvas(view,&motion,options)?};
        if self.drum_ui.open {self.drum_ui.place(rect);}
        let bubbles=if self.drum_ui.open {self.drum_ui.bubbles(view)}else{self.piano_workspace.presentation(&self.editor,rect,view)};
        host.invoke(
            "WorkstationPanelSystem.View",
            json!({
                "entity":self.visual.as_ref().ok_or("workstation is closed")?["entity"],
                "preamp_split":motion.preamp_split,"fullscreen":motion.fullscreen,"extensions":motion.extensions,"open":motion.open,"opening_order":motion.opening_order,"mobile_page":motion.mobile_page.map(MobilePage::name),"mobile_progress":motion.mobile_progress,
                "left_width_scale":motion.left_width_scale,"bottom_height_scale":motion.bottom_height_scale,
                "overview_hide":motion.overview_hide,"canvas_scale":self.canvas_scale,
                "swap_sides":self.swap_sides,"swap_horizontal":self.swap_horizontal,"origami":options.origami,"fixed_top":options.fixed_top,
                "width":viewport[0],"height":viewport[1],"framebuffer":framebuffer,"capture":capture,"style_time":self.style_time,
                "camera_view":self.camera_view,"audio_rays":self.audio_rays,
                "computer_screen":self.computer_screen,"passthrough":self.settings.passthrough(),
                "piano_scene":if self.drum_ui.open {json!({"drum_workspace":true})}else{self.piano_workspace.scene(&self.editor,rect)},
                "lightbox":self.settings.packet(),
                "transport_pressure":self.transport_controls.pressure,
                "computer_keyboard_controls":self.keyboard_controls.presentation(self.midi_sender.enabled()),
                "browser_disabled":!self.browser_enabled(),
                "browser_labels":if !self.browser_enabled() {crate::workstation::plugin_browser::layout(view,&motion,options,&[],0,0.)?.locked_presentation()}else if self.arrangement.viewports.properties {self.arrangement.viewports.presentation(view,&motion,options,json!([]))?}else if cfg!(target_os="macos") {crate::workstation::plugin_browser::layout_filtered(view,&motion,options,&{let mut catalog=self.plugins.catalog();catalog.extend(self.plugin_browser.library.entries.iter().cloned());catalog},self.plugin_browser.category,self.plugin_browser.scroll,&self.plugin_browser.query)?.search_focus(self.plugin_browser.search_active).presentation(self.plugin_browser.category,self.plugin_browser.selected())} else {Value::Null},
                "track_io":self.track_io.presentation(),"loop_control":{"enabled":self.arrangement.loop_enabled(),"pressure":self.loop_control.pressure},"plugins":({let mut packet=self.plugins.presentation();packet["navigation"]=json!(false);packet["hide_mixer"]=json!(true);packet}),"bottom_panel":self.bottom.packet,"bubbles":bubbles,"spheres":if self.spatial_view && !self.piano_workspace.active {
                    self.spatial_mix.presentation(rect,view)
                } else { Value::Null },
                "timeline":if self.drum_ui.open {self.drum_ui.timeline()} else if self.piano_workspace.active {self.piano_workspace.timeline(&self.editor,rect)} else if self.spatial_view {Value::Null} else {
                    {let mut packet=self.arrangement.presentation(self.drum_ui.timeline_rect(rect),self.playhead_seconds,Value::Null);packet["rack_controls"]=self.drum_ui.packet();packet}
                }
            }),
        )?;
        Ok(())
    }
    fn flush_midi(&mut self,host:&mut Host)->Result<(),String>{
        let events=self.midi_sender.events();
        if !events.is_empty()&&!host.system_enabled("EnvironmentAudioSystem"){return Err("MIDI audio connection is unavailable".into());}
        for event in events{
            let mut request=event.request();
            // Keep the physical key as release identity; cents carry the clip's
            // retuning into the native-clock renderer and recording adapter.
            if self.piano_workspace.active {
                let edo=self.editor.status()["edo"].as_u64().unwrap_or(12)as u16;
                request["tuning_cents"]=json!((crate::workstation::piano_tuning::semitones(event.pitch,edo)-f64::from(event.pitch))*100.);
            }
            host.invoke("EnvironmentAudioSystem.MidiInput",request)?;
            self.midi_sender.acknowledge(&event);
        }
        Ok(())
    }
    fn configure_midi_meter(&mut self,host:&mut Host)->Result<(),String>{
        let enabled=self.midi_sender.enabled();
        if self.midi_meter_enabled!=Some(enabled)&&host.system_enabled("EnvironmentAudioSystem"){
            host.invoke("EnvironmentAudioSystem.MidiKeyboard",json!({"enabled":enabled}))?;
            self.midi_meter_enabled=Some(enabled);
        }
        Ok(())
    }
    fn close(&mut self, host: &mut Host, retain: bool) -> Result<(), String> {
        self.audio_import.close();
        self.track_freeze.close(host)?;
        self.drum_ui.cancel(retain);
        if host.system_enabled("WorkstationDataSystem") {
            let snapshot=host.workstation_snapshot_shared()?.document;
            if snapshot["drum_rack"]["preview"]==true {
                host.invoke("WorkstationDataSystem.DrumRack",json!({"document_revision":snapshot["revision"],"action":"preview","enabled":false}))?;
            }
        }
        self.bottom.cancel(false);
        self.midi_sender.release_all();
        let midi_cleanup=self.flush_midi(host);
        if host.system_enabled("EnvironmentAudioSystem") {
            let _=host.invoke("EnvironmentAudioSystem.MidiKeyboard",json!({"enabled":false}));
            let _=host.invoke("EnvironmentAudioSystem.PreviewAudio",json!({"stop":true}));
        }
        self.midi_meter_enabled=None;
        self.transport_controls.cancel_pointer();
        self.track_io.cancel();
        self.loop_control.cancel();
        self.origami_control.cancel();
        self.settings.cancel();
        self.tutorial.cancel_pointer();
        self.camera_view=Value::Null;
        self.keyboard_controls.cancel();
        // A plugin-window failure must not strand this scene's Entity/Audicle.
        // Report it after releasing the workstation's own resources.
        let plugin_cleanup=if cfg!(target_os="macos") {
            host.invoke("PluginConnectionSystem.Command",json!({"op":"hide"})).map(|_|())
        } else {Ok(())};
        if host.system_enabled("WorkstationExportSystem") {
            host.invoke("WorkstationExportSystem.Close", Value::Null)?;
        }
        // Retain handles on a failed release so Close can be retried.
        if let Some(visual) = self.visual.clone() {
            #[cfg(target_os="macos")]
            if !self.parked && !self.plugin_surface.is_null(){host.invoke("GraphicsConnectionSystem.PluginSurface",json!({"entity":visual["entity"],"surface":null}))?;}
            if retain && self.resident {
                if !self.parked {host.invoke("GraphicsConnectionSystem.ParkWorkstation",visual)?;}
                self.parked=true;
            }else{
                host.invoke("AppearanceSystem.Release", visual.clone())?;
                host.invoke("EntitySystem.Remove", visual)?;
                self.visual = None;self.resident=false;self.parked=false;
            }
        }
        host.invoke(if retain && self.resident {"WorkstationPanelSystem.Suspend"}else{"WorkstationPanelSystem.Release"}, Value::Null)?;
        if let Some(event) = self.event.clone() {
            host.invoke("AudicleSystem.Cancel", event)?;
            self.event = None;
        }
        self.previous = [false; 5];
        self.viewport = None;
        self.framebuffer = None;
        if retain {
            // The editor's zero-sized canvas path cancels an uncommitted
            // gesture while retaining the open clip, selection and viewport.
            self.editor.update(&Default::default(),crate::workstation::piano_roll::Rect {
                left:0.,top:0.,width:0.,height:0.},0.)?;
        } else {
            self.motion = Motion::default();
            self.canvas_scale = 1.;
            self.editor.close();
            self.piano_workspace=Default::default();
            self.arrangement=Arrangement::default();
            self.spatial_view=false;
        }
        self.arrangement.cancel_interaction();
        self.arrangement_error=None;
        self.view_held=false;
        self.playhead_seconds=None;
        self.audio_startup_error=None;
        self.browser_capture=false;
        self.browser_selection.cancel();
        self.plugin_browser.cancel();
        self.plugin_surface=Value::Null;
        self.browser_ghost=Value::Null;
        self.range_overlay=Value::Null;
        self.caption.clear();
        self.retained=retain;
        self.editor_error=None;
        self.editor_left_held=false;
        self.spatial_mix.close();
        self.spatial_error=None;
        if let Some(frames)=self.step_timings.take(){
            let report=json!({"stage_names":STEP_STAGE_NAMES,"frames_ms":frames,"omitted":self.step_timings_omitted,
                "presentation_keys":self.presentation_keys,"shared_snapshot":self.shared_snapshot,"scope":"Adjacent Step wall intervals through status and large Value cleanup; excludes timing append, remaining local drops and outer Host invocation."});
            std::fs::write(host.output_root().join("workstation-step-stages.json"),serde_json::to_vec(&report).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        }
        plugin_cleanup.and(midi_cleanup)
    }

    fn open_roll(&mut self, host:&mut Host, id:Option<&str>) -> Result<(),String> {
        self.open_roll_inner(host,id)?;
        self.arrangement.cancel_interaction();
        self.browser_capture |= self.editor_left_held;
        self.editor_error=None;
        self.arrangement_error=None;
        Ok(())
    }

    fn open_roll_inner(&mut self, host:&mut Host, id:Option<&str>) -> Result<(),String> {
        let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
        let id=id.or_else(||self.arrangement.selected_clip())
            .ok_or("Select and double-click an existing MIDI clip to edit it")?;
        self.editor.open(&snapshot,id)?;
        self.drum_ui.cancel(false);
        self.piano_workspace.focus_clip(&snapshot,id);
        self.editor.workspace_time();
        self.spatial_view=false;
        Ok(())
    }

    fn arrangement_action(&mut self, host:&mut Host, action:Action)->Result<(),String> {
        self.begin_event(host)?;
        match action {
            Action::Document{hook,request}=>{
                let selected=request["id"].as_str().map(str::to_owned);
                let result=host.invoke(hook,request);
                let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
                self.arrangement.sync(&snapshot)?;
                let response=result?;
                if hook=="WorkstationDataSystem.CreateRangeClips"{
                    if let Some(id)=response["selected_clip"].as_str(){self.arrangement.select_clip(id);}
                    let notice=response["rejected"].as_array().filter(|a|!a.is_empty()).map(|a|a.iter().map(|v|format!("{}: {}",v["kind"].as_str().unwrap_or("clip"),v["reason"].as_str().unwrap_or("range rejected"))).collect::<Vec<_>>().join("; "));
                    self.arrangement.set_notice(notice);
                }
                if let Some(id)=selected {
                    if hook=="WorkstationDataSystem.PutTrack" {self.arrangement.select_track(&id);}
                    if hook=="WorkstationDataSystem.PutClip" {self.arrangement.select_clip(&id);}
                }
            }
            Action::AudioClipMenu(id)=>{
                let choice=host.invoke("PluginConnectionSystem.Menu",json!({"items":[{"label":"OPEN AUDIO EDITOR...","value":0}]}))?;
                if choice["selection"]==0 {self.arrangement.select_clip(&id);self.bottom.controls.open_clip_editor(false);self.bottom_open_requested=true;}
            }
            Action::OpenAudioPlaylistMenu(id)=>{
                self.arrangement.cancel_interaction();
                let result=crate::workstation::audio_playlist_menu::open(host,&id);
                let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
                self.arrangement.sync(&snapshot)?;
                result?;
            }
            Action::OpenWaveform(id)=>{self.arrangement.select_clip(&id);self.bottom.controls.open_clip_editor(false);self.bottom_open_requested=true;}
            Action::OpenTrackPlugins(id)=>{
                self.arrangement.select_track(&id);
                self.plugins.selected=Some(id);self.plugins.scroll=0.;
                self.bottom.controls.page=crate::workstation::bottom_panel::Page::Plugins;
                self.plugins.set_bottom_page(self.bottom.controls.page.index());
                self.bottom_open_requested=true;
            }
            Action::OpenPianoRoll(id)=>{self.open_roll(host,Some(&id))?;}
            Action::Rebase{request,revision}=>{
                let audio=host.invoke("EnvironmentAudioSystem.Status",Value::Null)?;
                if let Some(rate)=audio["transport"]["sample_rate"].as_u64().filter(|r|*r>0){
                    let frame=|seconds:f64|(seconds*rate as f64).round() as u64;
                    let mut command=json!({"action":"rebase","shift_frames":frame(request.shift),
                        "document_revision":revision,"enabled":request.range.enabled,
                        "loop_start_frame":frame(request.range.start),"loop_end_frame":frame(request.range.end)});
                    if let Some(seek)=request.seek{command["start_frame"]=json!(frame(seek));}
                    host.invoke("EnvironmentAudioSystem.Transport",command)?;
                }else{
                    host.invoke("WorkstationDataSystem.RebaseTimeline",json!({"document_revision":revision,"shift_seconds":request.shift}))?;
                }
                let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
                self.arrangement.accept_rebase(&snapshot,request)?;
                self.playhead_seconds=request.seek.or_else(||self.playhead_seconds.map(|p|p+request.shift));
            }
            Action::SetLoop(range)=>{
                let audio=host.invoke("EnvironmentAudioSystem.Status",Value::Null)?;
                let rate=audio["transport"]["sample_rate"].as_u64().filter(|r|*r>0).ok_or("audio transport is not prepared")?;
                host.invoke("EnvironmentAudioSystem.Transport",json!({"action":"loop","enabled":range.enabled,
                    "loop_start_frame":(range.start*rate as f64).round() as u64,
                    "loop_end_frame":(range.end*rate as f64).round() as u64}))?;
            }
            Action::Seek(seconds)=>{
                let audio=host.invoke("EnvironmentAudioSystem.Status",Value::Null)?;
                let rate=audio["transport"]["sample_rate"].as_u64()
                    .filter(|r|*r>0).ok_or("audio transport is not prepared")?;
                host.invoke("EnvironmentAudioSystem.Transport",json!({"action":"seek",
                    "start_frame":(seconds*rate as f64).round() as u64}))?;
                self.playhead_seconds=Some(seconds);
            }
        }
        Ok(())
    }

    fn apply_piano_input(&mut self,host:&mut Host,input:&crate::workstation::piano_roll::Input,rect:crate::workstation::piano_roll::Rect,dt:f64)->Result<bool,String>{
        if let Some(request)=self.editor.update(input,rect,dt)? {
            self.begin_event(host)?;
            let result=host.invoke("WorkstationDataSystem.PutClip",request);
            let authoritative=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
            let sync_error=self.editor.sync(&authoritative).err();
            self.piano_workspace.sync(&authoritative,self.arrangement.selected_track());
            self.editor_error=result.err().or(sync_error);
            if self.editor_error.is_some(){return Ok(false);}
        }
        Ok(true)
    }
    fn browser_action(&mut self,host:&mut Host,action:browser::Action)->Result<(),String> {
        let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
        let action=match action {
            browser::Action::NewTrack(kind)=>browser::Action::InsertTrack(kind,self.arrangement.selected_insertion_row()),
            action=>action,
        };
        let action=match action {
            browser::Action::NewTrack(_)=>unreachable!(),
            browser::Action::InsertTrack(kind,_) if matches!(kind,"matrix"|"mix"|"channel")=>{
                let occupied:Vec<Value>=snapshot["buses"].as_array().into_iter().flatten().chain(snapshot["tracks"].as_array().into_iter().flatten()).cloned().chain(snapshot["plugins"]["inserts"].as_array().into_iter().flatten().map(|p|json!({"id":p["track"]}))).collect();
                let id=unused_id(&json!(occupied),kind);
                host.invoke("WorkstationDataSystem.PutBus",json!({"document_revision":snapshot["revision"],"id":id,"kind":kind,"name":format!("{} Bus {}",if kind=="matrix"{"Matrix"}else if kind=="channel"{"Channel"}else{"Mix"},snapshot["buses"].as_array().into_iter().flatten().filter(|b|b["kind"]==kind).count()+1)}))?;
                self.bottom.selected_bus=Some(id.clone());if kind!="matrix"{self.plugins.selected=Some(id);}self.bottom.controls.page=crate::workstation::bottom_panel::Page::Mixer;self.bottom_open_requested=true;
                return Ok(());
            }
            browser::Action::InsertTrack("comp",index)=>self.arrangement.create_comp_at(index)?,
            browser::Action::InsertTrack(kind,index)=>arrangement::create_track_at(&snapshot,kind,self.arrangement.parent_insertion_index(index))?,
        };
        self.arrangement_action(host,action)?;
        self.editor.close();self.piano_workspace=Default::default();self.editor_error=None;self.spatial_view=false;
        Ok(())
    }
}

fn unused_id(items:&Value,prefix:&str)->String {
    // Capacities are bounded; this always finds an unused caller-owned identity.
    (1..).map(|i|format!("{prefix}-{i}")).find(|id| !items.as_array()
        .is_some_and(|a|a.iter().any(|v|v["id"].as_str()==Some(id.as_str())))).unwrap()
}

fn feedback(state: &mut State, host: &mut Host, cue: &str) {
    if host.chuck.prepared_entity().is_none() { return; }
    let gains = state.sfx_gains.get_or_insert_with(|| {
        let path = host.root().join("Procedures/chuck/daw/gains.json");
        std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| serde_json::from_str(include_str!("../../Procedures/chuck/daw/gains.json")).unwrap())
    });
    let gain = gains[cue].as_f64().unwrap_or(1.0).clamp(0.,1.);
    if gain == 0.0 { return; }
    let result = state.begin_event(host).and_then(|()| host.invoke("ChuckConnectionSystem.Compile",
        json!({"script":format!("Procedures/chuck/daw/{cue}.ck"),"feedback_gain":gain})));
    if let Err(error) = result { eprintln!("Workstation SFX {cue}: {error}"); }
}

fn step(state: &mut State, host: &mut Host, input: &Value) -> Result<Value, String> {
    let mut stages=StepClock::new(state.step_timings.is_some());
    if state.visual.is_none() {
        return Err("begin the workstation before advancing it".into());
    }
    let dt = input["dt"]
        .as_f64()
        .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
        .ok_or("workstation dt must be 0..1 seconds")?;
    let aspect = input["aspect"]
        .as_f64()
        .filter(|v| v.is_finite() && (0.2..=8.).contains(v))
        .ok_or("invalid workstation viewport aspect")?;
    let viewport: [f64; 2] = if input["viewport"].is_null() {
        [720. * aspect, 720.]
    } else {
        serde_json::from_value(input["viewport"].clone())
            .map_err(|_| "invalid workstation logical viewport")?
    };
    let framebuffer: [f64; 2] = if input["framebuffer"].is_null() {
        viewport
    } else {
        serde_json::from_value(input["framebuffer"].clone())
            .map_err(|_| "invalid workstation framebuffer")?
    };
    if viewport
        .iter()
        .chain(&framebuffer)
        .any(|v| !v.is_finite() || !(1.0..=16384.).contains(v))
    {
        return Err("workstation viewport dimensions must be within 1..16384".into());
    }
    state.settings.load();
    let placement = state.settings.panel_placement.unwrap_or([state.swap_sides, state.swap_horizontal]);
    let canvas_scale = input["canvas_scale"].as_f64().unwrap_or(state.canvas_scale);
    if !canvas_scale.is_finite() || !(0.25..=1.).contains(&canvas_scale) {
        return Err("canvas scale must be within 0.25..1".into());
    }
    let swap_sides = input["swap_sides"].as_bool().unwrap_or(placement[0]);
    let swap_horizontal = input["swap_horizontal"]
        .as_bool()
        .unwrap_or(placement[1]);
    // Tutorial input is consumed before every existing workstation controller.
    // Only card clicks are captured; the lesson stays nonmodal while practicing.
    if let Err(e)=state.track_freeze.pump(host){state.arrangement_error=Some(e);}
    let tutorial_document=host.workstation_snapshot_shared()?.document;
    let tutorial_view=Viewport{width:viewport[0],height:viewport[1],canvas_scale};
    let tutorial_blocked=state.settings.open || state.arrangement.is_interacting() || state.spatial_mix.is_interacting()
        || state.editor.status()["gesture_active"]==true || state.plugins.consumes_input()
        || state.browser_selection.owns_pointer() || state.plugin_browser.owns_pointer() || state.track_io.owns_pointer()
        || state.bottom.controls.consumes_pointer() || state.transport_controls.owns_pointer() || state.loop_control.owns_pointer()
        || host.workstation_export.blocks_timeline_commands();
    let tutorial_guard=state.tutorial.guard_input(input,tutorial_view,&tutorial_document,tutorial_blocked);
    let input=tutorial_guard.as_ref().unwrap_or(input);
    let search_before=(state.plugin_browser.query.clone(),state.plugin_browser.search_active);
    let search_layout=crate::workstation::plugin_browser::layout_filtered(tutorial_view,&state.motion.snapshot(),state.origami_control.mode.options(state.swap_sides,state.swap_horizontal),&[],state.plugin_browser.category,state.plugin_browser.scroll,&state.plugin_browser.query)?;
    let browser_disabled=!state.browser_enabled();
    let search_owns=cfg!(target_os="macos") && state.plugin_browser.search_input(&input["editor"],&search_layout,browser_disabled||state.settings.open||state.drum_ui.open||state.piano_workspace.active);
    if search_before!=(state.plugin_browser.query.clone(),state.plugin_browser.search_active){state.viewport=None;}
    let search_guard=if search_owns {
        let mut guarded=input.clone();
        guarded["keys"]=json!([false,false,false,false,false]);guarded["browser_search_active"]=json!(true);guarded["text_input_active"]=json!(true);
        let pointer=guarded["editor"]["pointer"].clone();let focused=guarded["editor"]["focused"].clone();
        let wheel=guarded["editor"]["wheel"].clone();
        guarded["editor"]=json!({"pointer":pointer,"focused":focused,"wheel":wheel});Some(guarded)
    }else{None};
    let input=search_guard.as_ref().unwrap_or(input);
    let keys = input["keys"]
        .as_array()
        .filter(|a| a.len() == 5 && a.iter().all(Value::is_boolean))
        .ok_or("workstation needs four edge keys and one all-panels key")?;
    let pressed: [bool; 5] = std::array::from_fn(|i| !state.drum_ui.open && !state.piano_workspace.active && !state.settings.open && keys[i] == true);
    // iOS navigation is resolved before any shared control sees this sample.
    // The native classifier withholds primary-down until a swipe/drag decision;
    // a navigation sample is cancelled, never a click through the new page.
    let mut mobile_changed=false;
    if let Some(edge)=input["editor"]["mobile_panel_request"].as_str().and_then(Edge::parse).filter(|_|!state.settings.open) {
        mobile_changed=state.motion.mobile_swipe(edge);
        if mobile_changed {
        state.arrangement.cancel_interaction();state.browser_selection.cancel();state.plugin_browser.cancel();
        state.bottom.cancel(false);state.track_io.cancel();state.transport_controls.cancel_pointer();
        state.loop_control.cancel();state.origami_control.cancel();state.keyboard_controls.cancel();
        state.browser_capture=false;
        }
    }
    // A full-screen browser has no simultaneous timeline drop region. Continue
    // its existing held component drag onto the timeline once it passes the
    // middle of the screen; release still performs the ordinary ordered drop.
    let mobile_component_transfer=state.motion.mobile_page()==Some(MobilePage::Panel(Edge::Left))
        && state.browser_selection.owns_pointer() && input["editor"]["left"]==true
        && input["editor"]["focused"]!=false
        && input["editor"]["pointer"].as_array().and_then(|p|p.first()?.as_f64()).is_some_and(|x|x>viewport[0]*0.55);
    if mobile_component_transfer {mobile_changed|=state.motion.set_mobile_page(Some(MobilePage::Timeline));}
    // Do not start an edit against a moving hit target. Navigation was already
    // consumed above, so an opposite swipe can still reverse an opening panel.
    let guarded_input=if state.motion.mobile_animating(){
        let mut guarded=input.clone();guarded["editor"]=json!({"focused":false,"transport_pointer_cancelled":true,"left":false});Some(guarded)
    }else{None};
    let input=guarded_input.as_ref().unwrap_or(input);
    let mut motion = state.motion.clone();
    let fullscreen_escape=!state.drum_ui.open && !state.piano_workspace.active && input["editor"]["escape"]==true && !state.settings.open && motion.fullscreen_target().is_some();
    if fullscreen_escape {motion.set_fullscreen(motion.fullscreen_target().unwrap(),false);}
    for edge in Edge::ALL {
        let index = edge as usize;
        if pressed[index] && !state.previous[index] {
            let logical = match edge {
                Edge::Left if swap_sides => Edge::Right,
                Edge::Right if swap_sides => Edge::Left,
                Edge::Top if swap_horizontal => Edge::Bottom,
                Edge::Bottom if swap_horizontal => Edge::Top,
                _ => edge,
            };
            if canvas_scale >= 0.999
                && input["expand"] == true
            {
                motion.cycle_panel_view(logical);
            } else if canvas_scale < 0.999 && !motion.is_open(logical) {
                let hidden = !motion.is_overview_hidden(logical);
                motion.set_overview_hidden(logical, hidden);
            } else if motion.fullscreen_target()==Some(logical) {
                motion.set_fullscreen(logical,false);motion.set_expanded(logical,false);
            } else if motion.is_expanded(logical) {
                motion.set_expanded(logical,false);
            } else {
                motion.toggle(logical);
            }
        }
    }
    if pressed[4] && !state.previous[4] {
        let open = !Edge::ALL.iter().all(|edge| motion.is_open(*edge));
        motion.set_all_open(open);
    }
    if pressed.iter().zip(state.previous).any(|(held,previous)| *held && !previous) { feedback(state,host,"panel_motion"); }
    // Overview follows the original silent Shift+wheel navigation.
    if motion.fullscreen_target()==Some(Edge::Bottom) && state.motion.fullscreen_target()!=Some(Edge::Bottom) {
        state.bottom.controls.page=crate::workstation::bottom_panel::Page::Mixer;
    }
    // Resolve one animation pose for layout, input, and uploaded geometry.
    // Advancing only at View left attached controls one frame behind panels.
    let preamp_track=state.arrangement.selected_track().or(state.plugins.selected.as_deref());
    motion.set_preamp(cfg!(target_os="macos") && state.bottom.controls.page==crate::workstation::bottom_panel::Page::Plugins && preamp_track.is_some_and(|id|tutorial_document["tracks"].as_array().is_some_and(|ts|ts.iter().any(|t|t["id"]==id&&t["kind"]=="audio"))));
    let motion_was_active = !motion.settled();
    motion.set_canonical_left_context(canvas_scale>=0.999 && crate::workstation::benchmark::motion_input::context(state.embedded,state.settings.passthrough(),state.style_animated,host.backend()));
    motion.advance(dt);
    let fullscreen_guard=if motion.fullscreen_animating() || fullscreen_escape {
        let mut guarded=input.clone();
        if motion.fullscreen_animating() {guarded["editor"]=json!({"focused":false,"transport_pointer_cancelled":true,"left":false});}
        else {guarded["editor"]["escape"]=json!(false);}
        Some(guarded)
    } else {None};
    let input=fullscreen_guard.as_ref().unwrap_or(input);
    let capture = input["capture"] == true;
    let view=Viewport {width:viewport[0],height:viewport[1],canvas_scale};
    let old_options=state.origami_control.mode.options(swap_sides,swap_horizontal);
    if let Some(page)=input["right_panel_page"].as_u64().filter(|p|*p<=2){state.track_io.page=page as usize;}
    state.settings.routing.sync(&tutorial_document);
    state.settings.theme_supported=state.embedded||host.backend()=="WebGPU";
    let settings_was_open=state.settings.open;
    let settings_surface_before=state.settings.surface(view,None);
    let lightbox_before=state.settings.packet();
    state.settings.load();
    let settings_input=piano_roll_scene::input(&input["editor"]);
    let settings_open_hit=|pointer:Option<[f64;2]>|cfg!(any(target_os="macos",target_os="ios")) && state.track_io.page==2 && pointer
        .is_some_and(|p|crate::workstation::origami_control::hit_for(view,&motion.snapshot(),old_options,p,"SETTINGS"));
    let settings_blocked=state.drum_ui.open||state.piano_workspace.active||state.arrangement.is_interacting()||state.plugins.consumes_input()
        ||state.browser_selection.owns_pointer()||state.plugin_browser.owns_pointer()||state.keyboard_controls.owns_pointer()
        ||state.track_io.owns_pointer()||state.transport_controls.owns_pointer()||state.loop_control.owns_pointer()
        ||state.spatial_mix.is_interacting()||host.workstation_export.blocks_timeline_commands();
    let settings_focused=input["editor"]["focused"]!=false && input["editor"]["transport_pointer_cancelled"]!=true;
    let mut modal_owns_input=state.settings.open;
    // Keep complete clicks even when an optical refresh spans both callbacks.
    for event in input["editor"]["transport_pointer_events"].as_array().into_iter().flatten().take(16){
        if let (Some(left),Ok(pointer))=(event["left"].as_bool(),serde_json::from_value::<[f64;2]>(event["pointer"].clone())){
            let mut sample=settings_input.clone();sample.left=left;sample.pointer=Some(pointer);sample.wheel=[0.;2];
            modal_owns_input|=state.settings.update(&sample,view,settings_open_hit(sample.pointer),settings_blocked,settings_focused);
        }
    }
    modal_owns_input|=state.settings.update(&settings_input,view,settings_open_hit(settings_input.pointer),settings_blocked,settings_focused);
    for mut patch in state.settings.routing.take_patches() {
        let current = host.workstation_snapshot_shared()?.document;
        patch["document_revision"] = current["revision"].clone();
        if let Err(error) = host.invoke("WorkstationDataSystem.SetBus", patch) {
            state.settings.notice = Some(error);
        } else {
            state.settings.notice = None;
        }
        state.viewport = None;
    }
    if !state.sfx_started && host.chuck.prepared_entity().is_some() {
        feedback(state,host, if state.embedded { "daw_open" } else { "daw_startup" });
        state.sfx_started = true;
    }
    if settings_focused {
        if input["editor"]["sfx_key_press"] == true { feedback(state,host,"keyswitch"); }
        let left = input["editor"]["left"] == true;
        let pressed = input["editor"]["transport_pointer_events"].as_array()
            .is_some_and(|events| events.iter().any(|event| event["left"] == true));
        if pressed || (left && !state.sfx_pointer_down) { feedback(state,host,"cursor_click"); }
        if mobile_changed { feedback(state,host,"workspace_swoosh"); }
        if state.settings.open != settings_was_open {
            feedback(state,host, if state.settings.open { "daw_open" } else { "daw_close" });
        }
    }
    state.sfx_pointer_down = input["editor"]["left"] == true;
    let lightbox_changed=lightbox_before!=state.settings.packet() || settings_surface_before!=state.settings.surface(view,None);
    let settings_hit=state.track_io.page==2 && serde_json::from_value::<[f64;2]>(input["editor"]["pointer"].clone()).ok()
        .is_some_and(|p|crate::workstation::origami_control::hit(view,&motion.snapshot(),old_options,p));
    let settings_was_captured=state.origami_control.owns_pointer();
    let settings_changed=state.origami_control.update(input["editor"]["left"]==true,settings_hit,
        modal_owns_input||state.drum_ui.open||state.piano_workspace.active||state.arrangement.is_interacting()||state.spatial_mix.is_interacting()
        ||state.keyboard_controls.owns_pointer()||state.track_io.owns_pointer()||state.plugins.owns_pointer()||state.transport_controls.owns_pointer()||state.loop_control.owns_pointer()||state.browser_capture||state.browser_selection.owns_pointer()||state.plugin_browser.owns_pointer()
        ||input["editor"]["focused"]==false||input["editor"]["transport_pointer_cancelled"]==true);
    let settings_owns_input=modal_owns_input||settings_was_captured||state.origami_control.owns_pointer();
    let options=state.origami_control.mode.options(swap_sides,swap_horizontal);
    let rect=if state.piano_workspace.active || state.drum_ui.open{crate::workstation::piano_workspace::outer(view)}else{piano_roll_scene::canvas(view,&motion.snapshot(),options)?};
    let rack_canvas=rect;
    let rack_shared=host.workstation_snapshot_shared()?;
    let mut rack_snapshot=rack_shared.document;
    state.drum_ui.sync(&rack_shared.live);
    let spectrum=if state.piano_workspace.active{state.piano_workspace.spectrum_request()}else{json!({"kind":"both"})};
    let mix_preview=host.invoke("WorkstationDataSystem.MixPreview",json!({"root":host.root().to_string_lossy(),"known_generation":state.piano_workspace.mix_preview["generation"],"enabled":state.piano_workspace.active||state.arrangement.viewports.visible(crate::workstation::mix_preview::ID),"track":spectrum["track"],"kind":spectrum["kind"]}))?;
    if mix_preview["unchanged"]!=true {
        if state.piano_workspace.mix_preview["phase"]!=mix_preview["phase"] || state.piano_workspace.mix_preview["generation"]!=mix_preview["generation"] {state.viewport=None;}
        state.arrangement.set_mix_preview(mix_preview.clone());
        state.piano_workspace.mix_preview=mix_preview;
    }
    state.arrangement.sync(&rack_snapshot)?;
    let rack_input=piano_roll_scene::input(&input["editor"]);
    let rack_blocked=modal_owns_input||host.workstation_export.blocks_timeline_commands()||state.piano_workspace.active||state.spatial_view
        ||state.arrangement.is_interacting()||state.editor.status()["gesture_active"]==true
        ||state.browser_selection.owns_pointer()||state.plugin_browser.owns_pointer()||state.plugins.owns_pointer()
        ||input["editor"]["focused"]==false||input["editor"]["transport_pointer_cancelled"]==true||(!state.drum_ui.open && !motion.snapshot().timeline_visible());
    let rack_before=state.drum_ui.packet();
    let (rack_owns_input,rack_commands,rack_drop)=state.drum_ui.update_events(&rack_input,rack_canvas,rack_blocked,&input["editor"]["transport_pointer_events"]);
    for mut command in rack_commands {
        state.begin_event(host)?;
        command["document_revision"]=rack_snapshot["revision"].clone();
        state.drum_ui.notice=host.invoke("WorkstationDataSystem.DrumRack",command).err();
        rack_snapshot=host.workstation_snapshot_shared()?.document;
        state.drum_ui.sync(&rack_snapshot);state.arrangement.sync(&rack_snapshot)?;
    }
    let rect=if !state.piano_workspace.active&&!state.spatial_view{state.drum_ui.timeline_rect(rect)}else{rect};
    if let Some((channel,point))=rack_drop {
        let result=(||->Result<(),String>{
            let (track,time)=state.arrangement.drum_pattern_drop(rect,point)?;
            if rack_snapshot["drum_rack"]["channels"][channel]["track"]!=track{return Err("Drop the pattern on its linked MIDI lane".into());}
            state.begin_event(host)?;
            host.invoke("WorkstationDataSystem.DrumRack",json!({"document_revision":rack_snapshot["revision"],"action":"place","channel":channel,
                "id":format!("rack-pattern-{}-{}",rack_snapshot["revision"],channel),"start_seconds":time}))?;
            Ok(())
        })();
        state.drum_ui.notice=result.err();rack_snapshot=host.workstation_snapshot_shared()?.document;
        state.drum_ui.sync(&rack_snapshot);state.arrangement.sync(&rack_snapshot)?;
    }
    let rack_changed=rack_before!=state.drum_ui.packet();
    if mobile_component_transfer {state.browser_selection.transfer_mobile_canvas(browser_gesture_canvas(view));}
    stages.mark();
    let before=state.piano_workspace.presentation(&state.editor,rect,view);
    let workspace_before=state.piano_workspace.status();
    let piano_scene_before=state.piano_workspace.scene(&state.editor,rect);
    let spheres_before=state.spatial_mix.presentation(rect,view);
    let (arrangement_before,arrangement_key)=if state.presentation_keys {
        (Value::Null,Some(state.arrangement.presentation_key(rect,state.playhead_seconds)))
    }else{(state.arrangement.presentation(rect,state.playhead_seconds,Value::Null),None)};
    stages.mark();
    let previous_view=state.spatial_view;
    let mut editor_input=piano_roll_scene::input(&input["editor"]);
    // Export owns timeline commands and gestures. Native cancellation also
    // covers a blur and regain delivered in the same callback batch.
    let timeline_input_blocked=state.track_freeze.active()||modal_owns_input||host.workstation_export.blocks_timeline_commands()
        || input["editor"]["focused"]==false || input["editor"]["transport_pointer_cancelled"]==true;
    editor_input.create_range_pressed &= !timeline_input_blocked;
    editor_input.split_clip_pressed &= !timeline_input_blocked;
    editor_input.loop_range_pressed &= !timeline_input_blocked;
    // In the source, musical Return belongs to QWERTY unless keyboard
    // navigation explicitly owns the piano roll.
    if state.midi_sender.enabled() && input["daw_keyboard_navigation"]!=true && !editor_input.command && !editor_input.alt {
        editor_input.enter=false;editor_input.enter_pressed=false;
    }
    if input["editor"]["freeze_track_pressed"]==true && input["text_input_active"]!=true && input["loading"]!=true && input["browser_search_active"]!=true && !timeline_input_blocked && !state.settings.open && !state.piano_workspace.active && !state.drum_ui.open && !state.spatial_view && !state.arrangement.is_interacting() && !state.plugins.consumes_input() && state.arrangement.selected_clip().is_none(){
        if let Some(id)=state.arrangement.selected_track().map(str::to_owned){state.arrangement_error=state.track_freeze.start(host,&id).err();}
    }
    if state.track_freeze.active(){editor_input=Default::default();}
    let raw_editor_input=editor_input.clone();
    if search_owns {editor_input.wheel=[0.;2];}
    let editor_was_open=state.piano_workspace.active;
    let raw_left=editor_input.left;
    let keyboard_controls_before=state.keyboard_controls.presentation(state.midi_sender.enabled());
    let keyboard_was_captured=state.keyboard_controls.owns_pointer();
    let keyboard_panel=crate::workstation::panel_layout::visual_bounds(view,&motion.snapshot(),options)?[3];
    let keyboard_blocked=state.drum_ui.open||state.piano_workspace.active||settings_owns_input||state.arrangement.is_interacting()||state.spatial_mix.consumes_pointer()
        ||state.editor.status()["gesture_active"]==true||state.plugins.consumes_input()||state.browser_selection.owns_pointer()
        ||state.transport_controls.owns_pointer()||state.track_io.owns_pointer()
        ||input["editor"]["focused"]==false||input["editor"]["transport_pointer_cancelled"]==true;
    if state.keyboard_controls.update(&input["editor"],keyboard_panel,&mut state.track_io.page,state.midi_sender.enabled(),keyboard_blocked,dt){
        state.midi_sender.set_enabled(!state.midi_sender.enabled());
        state.midi_sender_error=state.configure_midi_meter(host).and_then(|_|state.flush_midi(host)).err();
    }
    state.arrangement.set_lane_controls(state.keyboard_controls.timeline);
    let keyboard_owns_input=keyboard_was_captured||state.keyboard_controls.owns_pointer();
    let keyboard_controls_changed=keyboard_controls_before!=state.keyboard_controls.presentation(state.midi_sender.enabled());
    if settings_owns_input||keyboard_owns_input||rack_owns_input || state.drum_ui.open {
        let tab=editor_input.tab;let alt=editor_input.alt;
        editor_input=Default::default();
        if !settings_owns_input && !keyboard_owns_input {editor_input.tab=tab;editor_input.alt=alt;}
    }
    let transport_before=state.transport_controls.pressure;
    let loop_before=state.loop_control.pressure;
    if editor_input.tab && !state.view_held && !timeline_input_blocked {
        if editor_input.alt {
            state.spatial_view=!state.spatial_view;
            state.piano_workspace.active=false;
            state.drum_ui.open=false;
        } else {
            let was_piano=state.piano_workspace.active;
            state.piano_workspace.active=!state.drum_ui.open && !was_piano;
            state.drum_ui.open=was_piano;
            state.spatial_view=false;
        }
        state.viewport=None;
        // Suspend a draft on departure, retaining the accepted clip and viewport.
        state.editor.update(&Default::default(),crate::workstation::piano_roll::Rect{width:0.,height:0.,..rect},0.)?;
        state.arrangement.cancel_interaction();
    }
    state.view_held=editor_input.tab;
    editor_input.tab=false;
    if let Some(spatial)=input["spatial_view"].as_bool() {state.spatial_view=spatial;}
    if previous_view!=state.spatial_view {
        state.arrangement.cancel_interaction();
        state.spatial_mix.close();
        state.browser_capture|=raw_left;
    }
    let mut tutorial_transport=Value::Null;
    let mut playback_visualization=None;
    let mut keyboard_mic=crate::workstation::midi_keyboard::Mic::default();
    if host.system_enabled("EnvironmentAudioSystem") {
        let audio=host.invoke("EnvironmentAudioSystem.Status",Value::Null)?;
        keyboard_mic=crate::workstation::midi_keyboard::Mic::parse(&audio["computer_keyboard_mic"]);
        let transport=&audio["transport"];
        tutorial_transport=transport.clone();
        if transport.is_object() {
            state.arrangement.set_loop_from_transport(transport);
            state.transport_controls.sync_playing(transport["playing"]==true);
            // Consume metrics from the already rendered audio block. Replaying
            // a range for visualization would advance stateful plugins twice.
            playback_visualization=Some(transport["visualization"].clone());
        }
        state.playhead_seconds=transport["position_frame"].as_u64().zip(
            transport["sample_rate"].as_u64().filter(|r|*r>0))
            .map(|(frame,rate)|frame as f64/rate as f64);
    }
    // Loop is handled by the managed controls on right-panel page two.
    state.loop_control.cancel();
    let loop_owns_pointer=false;
    // Existing transport models now use Cardinal managed-button semantics.
    // Do not steal a note/clip/plugin gesture that began elsewhere.
    let transport_blocked=state.drum_ui.open || keyboard_owns_input || loop_owns_pointer || settings_owns_input || state.track_io.owns_pointer() || state.browser_selection.owns_pointer() || state.plugin_browser.owns_pointer() || state.piano_workspace.active || state.arrangement.is_interacting()
        || state.spatial_mix.is_interacting() || state.plugins.owns_pointer()
        || input["editor"]["focused"]==false || input["editor"]["transport_pointer_cancelled"]==true;
    let transport_events:Vec<_>=input["editor"]["transport_pointer_events"].as_array().into_iter().flatten().take(16)
        .filter_map(|e|Some((e["left"].as_bool()?,serde_json::from_value::<[f64;2]>(e["pointer"].clone()).ok()
            .filter(|p|p.iter().all(|v|v.is_finite())).and_then(|p|transport_hit(state,viewport,p))))) .collect();
    let transport_hit=editor_input.pointer.and_then(|p|transport_hit(state,viewport,p));
    let transport_was_captured=state.transport_controls.owns_pointer();
    let (transport_action,_)=state.transport_controls.update_events(raw_left,transport_hit,transport_blocked,dt,&transport_events);
    let transport_owns_pointer=loop_owns_pointer||transport_was_captured||state.transport_controls.owns_pointer()||state.loop_control.owns_pointer()||state.transport_controls.has_pending()||transport_action.is_some();
    if transport_owns_pointer{editor_input=Default::default();}
    if let Some(action)=transport_action {
        use crate::workstation::transport_controls::{self,Action as TransportAction};
        state.transport_notice=None;
        if let Some(request)=transport_controls::request(action) {
            if input["transport_available"]==true {
                match host.invoke("EnvironmentAudioSystem.Transport",request) {
                    Ok(_)=>{feedback(state,host,"button_click");state.transport_controls.accepted(action);if action==TransportAction::Rewind{state.arrangement.rewind_view();state.playhead_seconds=Some(state.arrangement.timeline_zero());}},
                    Err(error)=>state.transport_notice=Some(error),
                }
            }else{state.transport_notice=Some("Audio transport unavailable".into());}
        }else{state.transport_notice=Some("Transport action unavailable".into());}
    }
    // Cardinal DawTransportSystem:775-785 makes Space the final decision
    // after the managed-button action, including when both arrive together.
    let toggle_requested=input["editor"]["transport_toggle_pressed"]==true
        && (state.piano_workspace.active || input["editor"]["transport_toggle_timeline_pressed"].as_bool()
            .unwrap_or(input["editor"]["command"]!=true && input["editor"]["alt"]!=true));
    let toggle=state.transport_controls.take_toggle(toggle_requested&&input["transport_available"]==true,
        modal_owns_input || input["editor"]["focused"]==false || input["editor"]["transport_pointer_cancelled"]==true);
    if toggle && input["transport_available"]==true {
        let audio=host.invoke("EnvironmentAudioSystem.Status",Value::Null)?;
        let playing=audio["transport"]["playing"]==true;
        match host.invoke("EnvironmentAudioSystem.Transport",json!({"action":if playing{"pause"}else{"play"}})) {
            Ok(_)=>{state.transport_controls.sync_playing(!playing);state.transport_notice=None;},
            Err(error)=>state.transport_notice=Some(error),
        }
    }
    let (mut snapshot,live)=if state.shared_snapshot {let shared=host.workstation_snapshot_shared()?;(shared.document,Some(shared.live))}
        else{(std::sync::Arc::new(host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?),None)};
    let observation=if input["diagnostic_spheres"]!=true {playback_visualization.as_ref()}else{None}
        .or_else(||live.as_ref().map(|metadata|&metadata["visualization"]))
        .filter(|value|value.is_null() || value["revision"]==snapshot["revision"]);
    stages.mark();
    state.piano_workspace.sync(&snapshot,state.arrangement.selected_track());
    if let Err(error)=state.arrangement.sync(&snapshot) {state.arrangement_error=Some(error);}
    if let Err(error)=state.spatial_mix.sync_with_visualization(&snapshot,observation,dt) {state.spatial_error=Some(error);}
    stages.mark();
    // Cardinal components select on down; creation happens only at a valid drop.
    let component_was_captured=state.browser_selection.owns_pointer();
    let mut theatre_camera_requested=false;
    let io_before=state.track_io.presentation();
    let io_audio=if host.system_enabled("EnvironmentAudioSystem"){host.invoke("EnvironmentAudioSystem.Status",Value::Null)?}else{Value::Null};
    if let Some(error)=io_audio["browser_preview"]["error"].as_str(){state.plugin_browser.error=Some(error.into());}
    let physical_inputs=io_audio["physical_input_ports"].as_array().map_or(0,Vec::len);
    state.track_io.set_theatre((state.embedded && host.system_enabled("SecurityCameraSystem") && host.theatre.venue.is_some() && io_audio["transport"].is_object()).then_some(io_audio["transport"]["mode"]=="world" && io_audio["transport"]["speakers_enabled"]==true));
    state.track_io.set_loop_enabled(state.arrangement.loop_enabled());
    state.track_io.set_audio_rays(state.audio_rays);
    state.track_io.set_headset_view((state.eyewear.as_deref()==Some("vr")).then_some(state.headset_camera));
    state.track_io.set_virtual_mic_available(io_audio["virtual_mic_available"]==true);
    let io_blocked=state.drum_ui.open || rack_owns_input || keyboard_owns_input || settings_owns_input||transport_owns_pointer||state.piano_workspace.active||state.browser_capture||state.browser_selection.owns_pointer()||state.plugin_browser.owns_pointer()
        ||state.arrangement.is_interacting()||state.spatial_mix.consumes_pointer()||state.plugins.owns_pointer()||timeline_input_blocked;
    state.track_io.lane_layout=state.arrangement.lane_layout;
    state.arrangement.lane_recording=io_audio["transport"]["recording"]==true;
    state.arrangement.lane_time=state.style_time;
    let io_was_captured=state.track_io.owns_pointer();
    let io_events:Vec<_>=input["editor"]["transport_pointer_events"].as_array().into_iter().flatten().take(16)
        .filter_map(|event|Some((event["left"].as_bool()?,serde_json::from_value::<[f64;2]>(event["pointer"].clone()).ok()
            .filter(|point|point.iter().all(|v|v.is_finite()))?))).collect();
    if let Some(request)=state.track_io.update_events(&snapshot,&raw_editor_input,view,&motion.snapshot(),options,physical_inputs,io_blocked,
        io_audio["transport"]["recording"]==true,state.style_time,&io_events)? {
        state.begin_event(host)?;
        state.track_io_error=if request["lane_layout_cycle"]==true {
            state.arrangement.cycle_lane_layout();state.track_io.lane_layout=state.arrangement.lane_layout;None
        }else if request["import_audio_files"]==true {
            state.audio_import.open(crate::workstation::audio_import::Kind::Audio).err()
        }else if request["import_plugin_files"]==true {
            state.audio_import.open(crate::workstation::audio_import::Kind::Plugins).err()
        }else if request["learn_toggle"]==true {
            state.tutorial.toggle_open();state.viewport=None;None
        }else if request["loop_toggle"]==true {
            if input["transport_available"]==true {
                let action=state.arrangement.loop_toggle_action();
                let error=state.arrangement_action(host,action).err();
                state.track_io.set_loop_enabled(state.arrangement.loop_enabled());
                error
            }else{Some("Audio transport unavailable".into())}
        }else if let Some(enabled)=request["audio_rays"].as_bool(){
            state.audio_rays=enabled;state.viewport=None;None
        }else if let Some(camera)=request["headset_camera"].as_bool(){
            state.headset_camera=camera;state.settings.enable_camera_passthrough();state.viewport=None;None
        }else if request["drum_rack_open"]==true {
            state.drum_ui.open=!state.drum_ui.open;
            state.editor.update(&Default::default(),crate::workstation::piano_roll::Rect{left:0.,top:0.,width:0.,height:0.},0.)?;
            state.arrangement.cancel_interaction();
            state.piano_workspace.active=false;state.spatial_view=false;
            state.viewport=None;
            None
        }else if let Some(enabled)=request["theatre_output"].as_bool(){
            let result=host.invoke("EnvironmentAudioSystem.Transport",json!({"action":"speakers","enabled":enabled,"mode":if enabled{"world"}else{"stereo"}}));
            if result.is_ok(){
                state.track_io.set_theatre(Some(enabled));
                if enabled {state.settings.enable_camera_passthrough();theatre_camera_requested=state.eyewear.as_deref()!=Some("ar");if state.eyewear.as_deref()==Some("vr"){state.headset_camera=true;}}
            }
            result.err()
        }else if request["bus_edit"]==true{let mut patch=request;patch.as_object_mut().unwrap().remove("bus_edit");host.invoke("WorkstationDataSystem.SetBus",patch).err()}else{host.invoke("WorkstationDataSystem.SetTrackMix",request).err()};
        snapshot=std::sync::Arc::new(host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?);
        state.arrangement.sync(&snapshot)?;
        // The edit invalidates the pre-edit playback observation. Reconcile
        // from the authoritative snapshot until audio publishes its new revision.
        state.spatial_mix.sync(&snapshot,0.)?;
        state.track_io.refresh(&snapshot,view,&motion.snapshot(),options,physical_inputs,
            io_audio["transport"]["recording"]==true,state.style_time)?;
    }
    let io_owns_input=io_was_captured||state.track_io.owns_pointer();
    let io_changed=io_before!=state.track_io.presentation();
    if io_owns_input {editor_input=Default::default();}
    let browser_blocked=state.drum_ui.open || rack_owns_input || keyboard_owns_input || io_owns_input || settings_owns_input || transport_owns_pointer || state.browser_capture
        ||state.arrangement.consumes_pointer()||state.spatial_mix.consumes_pointer()
        ||state.plugins.owns_pointer()||timeline_input_blocked;
    // Finite graphics-only diagnostics do not open audio. Preserve discovery
    // there; normal audio startup has already prepared this catalog.
    if cfg!(target_os="macos") && !state.browser_enabled() {
        let gate=crate::workstation::plugin_browser::layout(view,&motion.snapshot(),options,&[],0,0.)?;
        if state.plugin_browser.enable_input(&input["editor"],&gate,browser_blocked||state.piano_workspace.active||state.spatial_view) {
            state.viewport=None;
        }
    }
    let browser_disabled=!state.browser_enabled();
    prepare_plugin_catalog(state, host);
    match state.audio_import.poll(host.root()) {
        Ok(Some(crate::workstation::audio_import::Kind::Audio))=>{
            state.plugin_browser.library.invalidate();state.plugin_browser.category=3;state.plugin_browser.scroll=0.;state.plugin_browser.query.clear();state.viewport=None;
        },
        Ok(Some(crate::workstation::audio_import::Kind::Plugins))=>{
            state.plugin_browser.cancel();
            match host.invoke("PluginConnectionSystem.Command",json!({"op":"scan","refresh":true})) {
                Ok(status)=>{state.plugins.refresh(status);state.plugin_browser.scanned=true;},
                Err(error)=>state.audio_import.notice=Some(format!("Plugins copied, but library refresh failed: {error}")),
            }
        },
        Ok(None)=>{},
        Err(error)=>state.audio_import.notice=Some(error),
    }
    let library_before=state.plugin_browser.library.entries.clone();
    if !browser_disabled {if let Err(error)=state.plugin_browser.library.refresh(host.root()){state.plugin_browser.error=Some(error);}}
    let mut catalog=state.plugins.catalog();
    catalog.extend(state.plugin_browser.library.entries.iter().cloned());
    let catalog_layout=crate::workstation::plugin_browser::layout_filtered(view,&motion.snapshot(),options,&catalog,state.plugin_browser.category,state.plugin_browser.scroll,&state.plugin_browser.query)?;
    let browser_before=(state.plugin_browser.category,state.plugin_browser.scroll);
    let catalog_was_captured=state.plugin_browser.owns_pointer();
    let (viewport_owns,viewport_changed)=state.arrangement.viewports.update(&raw_editor_input,view,&motion.snapshot(),options,browser_disabled||browser_blocked || state.piano_workspace.active || input["editor"]["focused"]==false,&io_events)?;
    if viewport_changed {state.arrangement.cancel_interaction();state.arrangement.sync(&snapshot)?;state.viewport=None;}
    let properties_page=state.arrangement.viewports.properties;
    let audio_target=raw_editor_input.pointer.and_then(|p|state.arrangement.audio_drop(rect,p).ok());
    let audio_lane=audio_target.as_ref().map(|(id,_,_)|json!({"id":id,"kind":"audio"}));
    let target=if state.plugin_browser.dragging_audio(){audio_lane.as_ref()}else{raw_editor_input.pointer.and_then(|p|if state.plugin_browser.dragging_stock(){state.arrangement.browser_stock_target(rect,p)}else{state.arrangement.browser_plugin_target(rect,p)})};
    let mix_target=state.plugins.selected.as_ref().and_then(|id|snapshot["buses"].as_array().into_iter().flatten().find(|b|b["id"]==*id&&(b["kind"]=="mix"||b["kind"]=="channel")));
    let bottom_drop=crate::workstation::panel_layout::visual_bounds(view,&motion.snapshot(),options)?[1];
    let target=if state.bottom.controls.page==crate::workstation::bottom_panel::Page::Plugins&&raw_editor_input.pointer.is_some_and(|p|p[0]>=bottom_drop.x&&p[0]<bottom_drop.x+bottom_drop.width&&p[1]>=bottom_drop.y&&p[1]<bottom_drop.y+bottom_drop.height){mix_target.or(target)}else{target};
    let plugin_drop=if cfg!(target_os="macos") && motion.mobile_page().is_none(){state.plugin_browser.update(&raw_editor_input,&catalog_layout,
        snapshot["revision"].as_u64().unwrap_or(0),{let c=browser_gesture_canvas(view);[c.left,c.top,c.width,c.height]},target,
        browser_disabled||browser_blocked||viewport_owns||properties_page||state.piano_workspace.active||state.spatial_view||state.browser_selection.owns_pointer())}else{None};
    let mut plugin_loaded=false;
    let plugin_drop=match plugin_drop {
        Some(command) if command["op"]=="choose_audio"||command["op"]=="choose_plugins"=>{
            let kind=if command["op"]=="choose_audio"{crate::workstation::audio_import::Kind::Audio}else{crate::workstation::audio_import::Kind::Plugins};
            if let Err(error)=state.audio_import.open(kind){state.audio_import.notice=Some(error);}
            None
        },other=>other
    };
    if let Some(mut command)=plugin_drop.clone(){
        let audio=host.invoke("EnvironmentAudioSystem.Status",Value::Null)?;
        if let Some(rate)=audio["transport"]["sample_rate"].as_u64().filter(|r|*r>=8000){command["sample_rate"]=json!(rate);}
        let imported=command["op"]=="import_audio";
        let preview=command["op"]=="preview_audio";
        let result=if preview {host.invoke("EnvironmentAudioSystem.PreviewAudio",command)}else if imported {
            state.begin_event(host)?;
            command["document_revision"]=snapshot["revision"].clone();
            command["start_seconds"]=json!(audio_target.as_ref().map(|(_,time,_)|*time).unwrap_or(0.));
            command["lane"]=json!(raw_editor_input.pointer.map(|p|state.arrangement.free_lane_at(rect,p)).unwrap_or(0.));
            command["comp_lane"]=json!(raw_editor_input.pointer.map(|p|state.arrangement.comp_lane_at(rect,p)).unwrap_or(0));
            if command.get("sample_rate").is_none(){command["sample_rate"]=json!(48000);}
            host.invoke("WorkstationDataSystem.ImportAudio",command)
        }else{host.invoke("PluginConnectionSystem.Command",command)};
        match result{
            Ok(status)=>{
                if imported {
                    snapshot=std::sync::Arc::new(host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?);
                    state.arrangement.sync(&snapshot)?;
                    if let Some(id)=status["clip"].as_str(){state.arrangement.select_clip(id);}
                    state.spatial_mix.sync(&snapshot,0.)?;
                }else if !preview{state.plugins.refresh(status);}
                plugin_loaded=true;state.plugin_browser.error=None;if !preview{state.plugin_browser.acknowledge_drop(raw_editor_input.pointer);}
            },
            Err(error)=>state.plugin_browser.error=Some(error),
        }
    }
    let catalog_owns_pointer=catalog_was_captured||state.plugin_browser.owns_pointer();
    let browser_hit=raw_editor_input.pointer.filter(|_|!browser_disabled&&!properties_page&&!viewport_owns).and_then(|p|if cfg!(target_os="macos") && motion.mobile_page().is_none(){
        state.plugin_browser.component_hit(&catalog_layout,p)
    }else{browser::hit(view,&motion.snapshot(),options,p)});
    let comp=matches!(browser_hit,Some(browser::Action::NewTrack("comp")))||state.browser_selection.selected_component()==Some("comp");
    let bus_kind=match browser_hit{Some(browser::Action::NewTrack(kind))=>Some(kind),_=>state.browser_selection.selected_component()};
    let bus_drop=matches!(bus_kind,Some("matrix"|"mix"|"channel"));
    let bus_panel=crate::workstation::panel_layout::visual_bounds(view,&motion.snapshot(),options)?[1];
    let slot=raw_editor_input.pointer.and_then(|p|if bus_drop{(p[0]>=bus_panel.x&&p[0]<bus_panel.x+bus_panel.width&&p[1]>=bus_panel.y&&p[1]<bus_panel.y+bus_panel.height).then_some(0)}else if comp{state.arrangement.comp_drop_slot(rect,p)}else{state.arrangement.browser_drop_slot(rect,p)});
    let component_action=state.browser_selection.update(&raw_editor_input,browser_hit,slot,
        snapshot["revision"].as_u64().unwrap_or(0),browser_gesture_canvas(view),
        browser_disabled||browser_blocked||viewport_owns||properties_page||catalog_owns_pointer||state.piano_workspace.active||state.spatial_view);
    let browser_owns_pointer=viewport_owns||catalog_owns_pointer||component_was_captured||state.browser_selection.owns_pointer()||state.plugin_browser.owns_pointer()
        ||state.browser_capture;
    state.browser_capture&=raw_left;
    let browser_action=component_action;
    if let Some(action)=browser_action {
        state.arrangement_error=state.browser_action(host,action).err();
        if state.arrangement_error.is_none() {state.arrangement.reveal_selected(rect);}
    }
    if browser_owns_pointer {editor_input=Default::default();}
    // Cardinal bottom pages share one owner; release frames stay consumed.
    let bottom_rect=crate::workstation::panel_layout::visual_bounds(view,&motion.snapshot(),options)?[1];
    let bottom_clip=state.arrangement.selected_clip().map(str::to_owned);
    let bottom_track=state.arrangement.selected_track().map(str::to_owned);
    let bottom_plugin=state.plugins.selected.clone();
    let bottom_blocked=input["text_input_active"]==true || input["browser_search_active"]==true || input["modal_open"]==true || input["loading"]==true || state.piano_workspace.active || state.drum_ui.open || rack_owns_input || io_owns_input || timeline_input_blocked || settings_owns_input || transport_owns_pointer || browser_owns_pointer
        || state.arrangement.is_interacting() || state.editor.status()["gesture_active"]==true
        || state.spatial_mix.consumes_pointer();
    if raw_editor_input.left&&raw_editor_input.pointer.is_some_and(|p|p[0]>=rect.left&&p[0]<rect.left+rect.width&&p[1]>=rect.top&&p[1]<rect.top+rect.height)&&!browser_owns_pointer{state.bottom.selected_bus=None;}
    let (bottom_owns_pointer,bottom_changed)=state.bottom.update(host,&input["editor"],
        crate::workstation::bottom_panel::Rect{x:bottom_rect.x,y:bottom_rect.y,w:bottom_rect.width,h:bottom_rect.height},
        bottom_clip.as_deref(),bottom_plugin.as_deref(),dt,bottom_blocked,observation,bottom_track.as_deref())?;
    if let Some(id)=state.bottom.recalled_clip.take(){let fresh=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;state.arrangement.sync(&fresh)?;state.arrangement.select_clip(&id);}
    for action in &state.bottom.pending {
        if let crate::workstation::bottom_panel::Action::SelectTrack(id)=action {state.arrangement.select_track(id);state.plugins.selected=Some(id.clone());}
        if let crate::workstation::bottom_panel::Action::SelectBus(id)=action{if snapshot["buses"].as_array().into_iter().flatten().any(|b|b["id"]==*id&&(b["kind"]=="mix"||b["kind"]=="channel")){state.plugins.selected=Some(id.clone());}}
        if let crate::workstation::bottom_panel::Action::CopyLoopback{audio,pointer,snap}=action {
            let result=(||->Result<Value,String>{
                let(track,raw,span)=state.arrangement.loopback_drop(rect,*pointer)?;
                let audio_status=host.invoke("EnvironmentAudioSystem.Status",Value::Null)?;
                let rate=audio_status["sample_rate"].as_u64().filter(|r|(8000..=192000).contains(r)).ok_or("Audio output is not prepared.")? as u32;
                let fresh=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
                let timing=crate::workstation::musical_time::Timing::from_snapshot(&fresh["musical_time"]);
                let map=timing.for_clock(rate as f64,fresh["timeline_zero_seconds"].as_f64().unwrap_or(0.));
                let time=if *snap{map.snap_seconds_signed(raw,span,0.,&timing.grid_choice,timing.quantum())}else{raw};
                crate::workstation::retrospective_runtime::insert_copy(host,audio,&track,(time.max(0.)*rate as f64).round() as u64,rate)
            })();
            match result{Ok(result)=>{let fresh=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;state.arrangement.sync(&fresh)?;if let Some(id)=result["selected_clip"].as_str(){state.arrangement.select_clip(id);}state.bottom.notice=Some("Copied loopback audio.".into());},Err(error)=>state.bottom.notice=Some(error)}
        }

        if let crate::workstation::bottom_panel::Action::SelectPluginTrack(index)=action {
            let tracks:Vec<_>=["audio","midi"].into_iter().flat_map(|kind|snapshot["tracks"].as_array().into_iter().flatten().filter(move|t|t["kind"]==kind).filter_map(|t|t["id"].as_str())).chain(snapshot["buses"].as_array().into_iter().flatten().filter(|b|(b["kind"]=="mix"||b["kind"]=="channel")).filter_map(|b|b["id"].as_str())).collect();
            if *index < tracks.len(){let next=*index;state.plugins.selected=Some(tracks[next].into());state.bottom.selected_bus=if snapshot["buses"].as_array().into_iter().flatten().any(|b|b["id"]==tracks[next]){Some(tracks[next].into())}else{None};state.plugins.scroll=0.;}
        }
    }
    state.plugins.set_bottom_page(state.bottom.controls.page.index());
    if bottom_owns_pointer {editor_input=Default::default();}
    let plugin_preferred=if state.bottom.selected_bus.as_ref().is_some_and(|id|snapshot["buses"].as_array().into_iter().flatten().any(|b|b["id"]==*id&&(b["kind"]=="mix"||b["kind"]=="channel"))){state.bottom.selected_bus.clone()}else if state.piano_workspace.active{
        let id=state.editor.status()["clip_id"].clone();
        snapshot["clips"].as_array().and_then(|a|a.iter().find(|c|c["id"]==id))
            .and_then(|c|c["track"].as_str()).map(str::to_owned)
    }else if state.spatial_view {
        state.spatial_mix.status()["selected_track"].as_str().map(str::to_owned)
    }else{state.arrangement.selected_track().map(str::to_owned)};
    let plugin_blocked=input["text_input_active"]==true || input["browser_search_active"]==true || input["modal_open"]==true || input["loading"]==true || state.piano_workspace.active || state.drum_ui.open || keyboard_owns_input || bottom_owns_pointer || state.bottom.controls.page!=crate::workstation::bottom_panel::Page::Plugins || io_owns_input || settings_owns_input || transport_owns_pointer || browser_owns_pointer || state.arrangement.is_interacting()
        || state.editor.status()["gesture_active"]==true || state.spatial_mix.consumes_pointer()
        || timeline_input_blocked;
    // Sync/picking above only read the document. Browser actions can mutate it
    // even before returning an error, so let the plugin UI re-read on that path.
    let plugin_snapshot=(browser_action.is_none()&&!bottom_changed&&!io_changed).then_some(&*snapshot);
    let plugin_changed=state.plugins.update(host,plugin_snapshot,if keyboard_owns_input || io_owns_input || settings_owns_input || transport_owns_pointer {&Value::Null}else{&input["editor"]},dt,view,&motion.snapshot(),options,
        plugin_preferred,plugin_blocked)? || plugin_loaded;

    let plugin_owns_input=state.plugins.consumes_input();
    stages.mark();
    // An insert Delete must not also delete a selected tape/note. A captured
    // insert drag retains ownership when the cursor crosses the arrangement.
    if plugin_owns_input {editor_input=Default::default();}
    let sphere_input=if io_owns_input || !state.spatial_view || state.piano_workspace.active || browser_owns_pointer || plugin_owns_input {
        crate::workstation::piano_roll::Input {left:raw_left,..Default::default()}
    }else{editor_input.clone()};
    if let Some(request)=state.spatial_mix.input(&sphere_input,rect,view) {
        state.begin_event(host)?;
        let result=host.invoke("WorkstationDataSystem.SetTrackMix",request);
        let authoritative=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
        let sync_error=state.spatial_mix.acknowledge(&authoritative).err();
        state.spatial_error=result.err().or(sync_error);
    }
    let sphere_owns_pointer=state.spatial_view && state.spatial_mix.consumes_pointer();
    if sphere_owns_pointer {editor_input=Default::default();}
    // Cardinal timeline gestures own clip creation and MIDI double-click opening.
    // Browser rows only create tracks; no fixed-duration clip shortcut.
    let mut arrangement_opened=false;
    let body=state.arrangement.clip_body(rect);
    state.arrangement.set_vertical_rail(crate::workstation::piano_roll::Rect{left:rect.left+crate::workstation::arrangement::HANDLE_FACE_WIDTH+4.,top:body.top,width:20.,height:body.height.max(1.)});
    state.arrangement.set_screen_height(viewport[1]);
    if !state.piano_workspace.active && !state.drum_ui.open && !state.spatial_view && !browser_owns_pointer
        && !io_owns_input && !settings_owns_input && !transport_owns_pointer && !plugin_owns_input && !timeline_input_blocked {
        state.arrangement.set_playhead(state.playhead_seconds);
        state.arrangement.set_screen_height(viewport[1]);
        let actions=state.arrangement.update(&editor_input,rect);
        for action in actions {
            arrangement_opened|=matches!(action,Action::OpenPianoRoll(_)|Action::OpenTrackPlugins(_)|Action::OpenWaveform(_));
            state.arrangement_error=state.arrangement_action(host,action).err();
        }
    } else {
        // Cancel captured gestures but retain the physical held-button history.
        // Focus/export/other-owner release must not manufacture a new press.
        state.arrangement.update(&crate::workstation::piano_roll::Input {
            left:raw_left,..Default::default()
        },rect);
    }
    if !state.piano_workspace.active && !state.drum_ui.open && !state.spatial_view {
        if let Err(error)=state.arrangement.refresh_waveforms(host,rect,state.settings.spectral_clips()){state.arrangement_error=Some(error);}
    }
    if state.bottom_open_requested {
        if motion.mobile_page().is_some(){mobile_changed|=motion.set_mobile_page(Some(MobilePage::Panel(Edge::Bottom)));}
        else{motion.set_expanded(Edge::Bottom,true);}
        state.bottom_open_requested=false;
    }
    if arrangement_opened {editor_input=Default::default();state.browser_capture&=raw_left;}
    state.editor_left_held=raw_left;
    let mut piano_edit_rect=rect;
    if state.piano_workspace.active {
        let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
        state.piano_workspace.sync(&snapshot,state.arrangement.selected_track());
        if let Err(error)=state.editor.sync(&snapshot) {state.editor_error=Some(error);}
        if editor_input.escape {
            state.piano_workspace.active=false;
        } else {
            editor_input.escape=false;
            state.piano_workspace.sync_transport(&tutorial_transport);
            state.piano_workspace.update(&mut editor_input,rect,&mut state.editor,dt)?;
            for action in std::mem::take(&mut state.piano_workspace.bar_actions){state.arrangement_action(host,action)?;}
            if let Some(mut request)=state.piano_workspace.sound_request.take(){
                request["document_revision"]=snapshot["revision"].clone();
                state.begin_event(host)?;
                host.invoke("WorkstationDataSystem.SetTrackMix",request)?;
                let fresh=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
                state.piano_workspace.sync(&fresh,state.arrangement.selected_track());
            }
            if let Some(request)=state.piano_workspace.settings.take(){
                state.midi_sender.release_all();let _=state.flush_midi(host);
                state.begin_event(host)?;
                let result=host.invoke("WorkstationDataSystem.PutClip",request);
                let authoritative=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
                state.editor_error=result.err().or(state.editor.sync(&authoritative).err());
                state.piano_workspace.sync(&authoritative,state.arrangement.selected_track());
            }
            if let Some(track)=state.piano_workspace.track.as_deref(){
                if state.arrangement.selected_track()!=Some(track){state.arrangement.select_track(track);}
            }
            piano_edit_rect=state.piano_workspace.edit_rect(&state.editor,rect);
            if state.editor.is_open() {
                let pointer_blocked=state.piano_workspace.pointer_consumed || timeline_input_blocked || settings_owns_input || keyboard_owns_input
                    || rack_owns_input || io_owns_input || bottom_owns_pointer || transport_owns_pointer || plugin_owns_input
                    || browser_owns_pointer || sphere_owns_pointer || arrangement_opened || !editor_was_open;
                let edges=piano_roll_scene::pointer_edges(&input["editor"]["transport_pointer_events"]);
                if pointer_blocked || edges.is_err() {
                    // Blur/cancellation/another owner must cancel a draft, not
                    // turn a masked held sample into an intentional release.
                    state.editor.update(&Default::default(),crate::workstation::piano_roll::Rect{width:0.,height:0.,..piano_edit_rect},dt)?;
                    state.editor.prime_input(&raw_editor_input);
                    if let Err(error)=edges {state.editor_error=Some(error);}
                } else {
                    let mut samples=Vec::new();
                    for (point,left) in edges.unwrap() {
                        // Toolbar/keyboard activations belong to their own
                        // controllers. A captured note may release outside.
                        if piano_roll_scene::contains(piano_edit_rect,point) || state.editor.status()["gesture_active"]==true
                            || samples.last().is_some_and(|(_,held):&([f64;2],bool)|*held) {
                            samples.push((point,left));
                        }
                    }
                    let mut rejected=false;
                    for (point,left) in samples {
                        let edge=state.editor.pointer_edge(&editor_input,point,left);
                        if !state.apply_piano_input(host,&edge,piano_edit_rect,0.)? {rejected=true;break;}
                    }
                    if !rejected {state.apply_piano_input(host,&editor_input,piano_edit_rect,dt)?;}
                }
            }
        }
    }
    if state.piano_workspace.active && (!editor_was_open
        || browser_owns_pointer || sphere_owns_pointer) {
        // Opening a tape consumes its activating pointer release. Retain
        // actual held input after a masked update so it cannot become a fresh
        // note action on the next frame or on release of a browser capture.
        state.editor.prime_input(&raw_editor_input);
    }
    stages.mark();
    let midi_snapshot=&*snapshot;
    let roll_id=state.editor.status()["clip_id"].clone();
    let roll_track=midi_snapshot["clips"].as_array().and_then(|clips|clips.iter().find(|c|c["id"]==roll_id))
        .and_then(|clip|clip["track"].as_str());
    let target=state.midi_sender.target(midi_snapshot,state.arrangement.selected_track(),roll_track);
    let keys=crate::workstation::midi_keyboard::Keys::parse(&input["editor"]);
    let midi_blocked=state.track_freeze.active() || host.workstation_export.blocks_timeline_commands() || input["daw_keyboard_navigation"]==true
        || input["loading"]==true || input["text_input_active"]==true || input["modal_open"]==true
        || modal_owns_input || input["settings_menu_open"]==true || input["browser_search_active"]==true;
    if let Some(enabled)=input["piano_roll_audition_enabled"].as_bool(){state.midi_sender.preview_enabled=enabled;}
    let preview=if plugin_owns_input||browser_owns_pointer||sphere_owns_pointer{None}else{if state.piano_workspace.active {state.piano_workspace.preview.clone().or_else(||state.editor.audition_note(&editor_input,piano_edit_rect))}else{None}};
    state.midi_sender.update(&keys,target.as_deref(),midi_blocked,dt,keyboard_mic,
        crate::workstation::midi_keyboard::Velocity::parse(&input["computer_midi_velocity"]),
        preview.as_ref().map(|(track,pitch,velocity)|(track.as_str(),*pitch,*velocity)));
    state.midi_sender_error=state.configure_midi_meter(host).and_then(|_|state.flush_midi(host)).err();
    let after=state.piano_workspace.presentation(&state.editor,rect,view);
    if !before.is_null() && after.is_null() {state.browser_capture|=raw_left;}
    if state.tutorial.status()["lesson"].is_string() {
        let committed=host.workstation_snapshot_shared()?.document;
        state.tutorial.observe(&committed,&state.editor.status(),&tutorial_transport);
    }
    let arrangement_status=state.arrangement.status();
    let caption=if let Some(notice)=&state.bottom.notice {format!("Mirror — {notice}")} else if let Some(notice)=&state.transport_notice {format!("Mirror — {notice}")} else if let Some(error)=state.arrangement_error.as_ref().or(state.editor_error.as_ref()) {
        format!("Mirror — {error}")
    } else if state.piano_workspace.active {
        state.piano_workspace.caption()
    } else if state.spatial_view {
        let spatial=state.spatial_mix.status();
        let notice=state.spatial_error.as_deref().or(spatial["message"].as_str());
        match notice {
            Some(notice)=>format!("Mirror — Spatial mix | {notice} | Alt-Tab returns to tracks"),
            None=>"Mirror — Spatial mix | Alt-Tab returns to tracks".into(),
        }
    } else if let Some(notice)=arrangement_status["notice"].as_str() {
        format!("Mirror — {notice}")
    } else {
        "Mirror — Tracks | Double-click MIDI clip: piano roll | Cmd-Shift-M: clip from selection | Tab: piano roll / drum rack / timeline | Alt-Tab: spatial mix".into()
    };
    let caption=if !state.piano_workspace.active && !state.drum_ui.open && !state.spatial_view && state.arrangement_error.is_none(){
        state.browser_selection.caption().map(|b|format!("{b} | {caption}")).unwrap_or(caption)
    }else{caption};
    let caption=if let Some(notice)=&state.audio_import.notice{format!("{caption} | {notice}")}else{caption};
    let caption=if let Some(error)=&state.plugin_browser.error{format!("{caption} | Browser: {error}")}else{caption};
    let caption=if state.origami_control.changed_once {format!("{} | {caption}",state.origami_control.mode.caption())}else{caption};
    let caption=if cfg!(target_os="macos") {format!("{caption} | {}",state.plugins.caption())}else{caption};
    let caption=state.drum_ui.caption().unwrap_or(caption);
    let caption=state.track_io.input_notice().map(|notice|format!("{notice} | {caption}")).unwrap_or(caption);
    let caption=match &state.audio_startup_error{Some(error)=>format!("Audio unavailable: {error} | {caption}"),None=>caption};
    if caption!=state.caption {
        host.invoke("GraphicsConnectionSystem.Caption",json!({"entity":state.visual.as_ref().unwrap()["entity"],"text":caption}))?;
        state.caption=caption;
    }
    // Baked drag artwork and native inertial motion never invalidate the scene.
    #[cfg(any(target_os="macos",target_os="ios"))]
    {
        let ghost_pointer=if state.computer_screen {
            host.theatre.computer_position().and_then(|computer|{
                let m=crate::workstation::computer_screen::map(&state.frozen_view,computer,view.width/view.height);
                raw_editor_input.pointer.and_then(|p|crate::workstation::computer_screen::project(m,p,[view.width,view.height]))
            })
        }else{raw_editor_input.pointer};
        let plugin_ghost=state.plugin_browser.ghost(ghost_pointer);
        let ghost=if state.piano_workspace.active {Value::Null}else if plugin_ghost.is_null(){state.browser_selection.ghost(ghost_pointer)}else{plugin_ghost};
        let mut catalog=state.plugins.catalog();
        catalog.extend(state.plugin_browser.library.entries.iter().cloned());
        let layout=crate::workstation::plugin_browser::layout_filtered(view,&motion.snapshot(),options,&catalog,state.plugin_browser.category,state.plugin_browser.scroll,&state.plugin_browser.query)?.search_focus(state.plugin_browser.search_active);
        let hover=if settings_owns_input||state.piano_workspace.active||state.drum_ui.open||state.spatial_view||input["editor"]["focused"]==false{None}else{raw_editor_input.pointer};
        let mut groups=if !state.browser_enabled(){layout.locked_presentation().as_array().unwrap().clone()}else if cfg!(target_os="macos"){layout.presentation_with_hover(state.plugin_browser.category,state.plugin_browser.selected(),hover).as_array().unwrap().clone()}else{Vec::new()};
        // Live catalog lettering is embedded in the gem optical frame.
        for group in &mut groups {
            if let Some(labels)=group["labels"].as_array_mut() {
                labels.retain(|label|label["icon"].is_array());
                for label in labels {label["text"]=json!("");}
            }
        }
        if state.browser_enabled(){groups=state.arrangement.viewports.presentation(view,&motion.snapshot(),options,json!(groups))?.as_array().unwrap().clone();}
        // Properties uses the same persistent engraved glyphs as the browser.
        // Keep native selection faces, but avoid drawing a second text layer.
        if state.arrangement.viewports.properties {
            for group in &mut groups {
                if let Some(labels)=group["labels"].as_array_mut() {
                    for label in labels { label["text"]=json!(""); }
                }
            }
        }
        state.plugin_browser.advance_feedback(dt);
        let sparkle=state.plugin_browser.drop_feedback(layout.screen);
        let inserts=if cfg!(target_os="macos"){state.plugins.surface(view,&motion.snapshot(),options)?}else{Value::Null};
        if let Some(inserts)=inserts.as_array(){groups.extend(inserts.iter().cloned());}
        if !sparkle.is_null(){groups.push(sparkle);}
        let bottom_packet=if motion.snapshot().panel_visible(Edge::Bottom){&state.bottom.packet}else{&Value::Null};
        let bottom_feedback=crate::workstation::bottom_overlay::surface(bottom_packet,
            if input["editor"]["focused"]==false{None}else{raw_editor_input.pointer});
        if !bottom_feedback.is_null(){groups.push(bottom_feedback);}
        // Bus rows use the white routing/mute engravings only, without name overlays.
        // Timeline panels do not exist in the separate piano room. Clear all
        // panel overlays together, including browser faces/icons.
        if state.piano_workspace.active || state.drum_ui.open { groups.clear(); }
        let settings_surface=state.settings.surface(view,raw_editor_input.pointer);
        if !settings_surface.is_null(){groups.push(settings_surface);}
        if !state.settings.open {
            groups.push(state.tutorial.surface(view));
        }
        let surface=if state.computer_screen {json!({"groups":[]})}else{json!({"groups":groups})};
        if surface!=state.plugin_surface {
            host.invoke("GraphicsConnectionSystem.PluginSurface",json!({"entity":state.visual.as_ref().unwrap()["entity"],"surface":surface}))?;
            state.plugin_surface=surface;
        }
        if ghost!=state.browser_ghost {
            host.invoke("GraphicsConnectionSystem.CursorLabel",json!({"entity":state.visual.as_ref().unwrap()["entity"],"label":ghost}))?;
            state.browser_ghost=ghost;
        }
    }
    #[cfg(any(target_os="macos",target_os="ios"))]{
        let overlay=if state.computer_screen||state.settings.open||state.piano_workspace.active||state.drum_ui.open||state.spatial_view||!motion.snapshot().timeline_visible(){Value::Null}else{state.arrangement.range_overlay(rect)};
        if overlay!=state.range_overlay{
            host.invoke("GraphicsConnectionSystem.SelectionRange",json!({"entity":state.visual.as_ref().unwrap()["entity"],"range":overlay}))?;
            state.range_overlay=overlay;
        }
    }
    #[cfg(any(target_os="macos",target_os="ios"))]{
        let picker=if state.computer_screen||state.settings.open||state.piano_workspace.active||state.drum_ui.open||state.spatial_view||!motion.snapshot().timeline_visible(){Value::Null}else{state.track_io.presentation()["parameter_picker"].clone()};
        if picker!=state.cc_picker_surface{
            host.invoke("GraphicsConnectionSystem.CcParameterPicker",json!({"entity":state.visual.as_ref().unwrap()["entity"],"picker":picker}))?;
            state.cc_picker_surface=picker;
        }
    }
    let camera_before=state.camera_view.clone();
    let cycle=input["editor"]["security_camera"]["cycle"]==true;
    if state.eyewear.as_deref()==Some("vr") && cycle && !state.camera_cycle_down && input["editor"]["focused"]!=false && !settings_owns_input && !state.midi_sender.enabled(){state.headset_camera=!state.headset_camera;state.settings.enable_camera_passthrough();}
    state.camera_cycle_down=cycle;
    if state.computer_screen {
        state.camera_view=state.frozen_view.clone();state.camera_view["id"]=json!("player");state.camera_view["aspect"]=json!(viewport[0]/viewport[1]);
    }else if state.eyewear.as_deref()==Some("ar") || (state.eyewear.as_deref()==Some("vr") && !state.headset_camera && state.settings.passthrough()) {
        if state.eyewear.as_deref()==Some("ar"){state.settings.enable_camera_passthrough();}
        state.camera_view=wearable_player_view(state.eyewear.as_deref(),&state.frozen_view,host.environment.view(),state.settings.passthrough(),state.headset_camera,viewport[0]/viewport[1]).unwrap();
    }else if host.system_enabled("SecurityCameraSystem") {
        state.camera_view=host.invoke("SecurityCameraSystem.View",json!({
            "enabled":state.settings.passthrough(),"embedded":state.embedded,
            "page":if state.piano_workspace.active{"piano_roll"}else if state.spatial_view{"mixer"}else{"timeline"},
            "keys":if theatre_camera_requested{json!({"theatre":true})}else{let mut k=input["editor"]["security_camera"].clone();if state.eyewear.as_deref()==Some("vr"){k["cycle"]=json!(input["editor"]["security_camera"]["next_camera"]==true);}k},"dt":dt,"aspect":viewport[0]/viewport[1],
            "blocked":settings_owns_input || state.midi_sender.enabled() || state.editor.status()["gesture_active"]==true
                || state.arrangement.is_interacting() || host.workstation_export.blocks_timeline_commands()
                || input["editor"]["focused"]==false || input["editor"]["command"]==true || input["editor"]["alt"]==true}))?;
    }else{state.camera_view=Value::Null;}
    crate::workstation::benchmark::camera::apply(&mut state.camera_view,&input["benchmark_camera_fraction"])?;
    let camera_changed=camera_before!=state.camera_view;
    state.style_time += dt;
    let style_due = state.style_animated && state.style_time >= state.style_next_draw;
    let changed_view = library_before!=state.plugin_browser.library.entries || browser_before!=(state.plugin_browser.category,state.plugin_browser.scroll) || piano_scene_before!=state.piano_workspace.scene(&state.editor,rect) || rack_changed || workspace_before!=state.piano_workspace.status() || mobile_changed || camera_changed || (state.audio_rays && state.camera_view.is_object()) || lightbox_changed || keyboard_controls_changed || bottom_changed || io_changed || loop_before!=state.loop_control.pressure || settings_changed || transport_before!=state.transport_controls.pressure || style_due || plugin_changed || before!=after || previous_view!=state.spatial_view
        || arrangement_key.as_ref().map_or_else(
            ||arrangement_before!=state.arrangement.presentation(rect,state.playhead_seconds,Value::Null),
            |key|*key!=state.arrangement.presentation_key(rect,state.playhead_seconds))
        || (state.spatial_view && !state.piano_workspace.active && (spheres_before!=state.spatial_mix.presentation(rect,view)
            || state.spatial_mix.is_animating())) || state.framebuffer != Some(framebuffer)
        || state.viewport != Some(viewport)
        || state.canvas_scale != canvas_scale
        || state.swap_sides != swap_sides
        || state.swap_horizontal != swap_horizontal
        || capture;
    stages.mark();
    if canvas_scale >= 0.999 {
        for edge in Edge::ALL {
            motion.set_overview_hidden(edge, false);
        }
    }
    if motion_was_active || !motion.settled() || changed_view || (state.event.is_some() && !state.style_animated) {
        state.begin_event(host)?;
        state.motion = motion;
        state.canvas_scale = canvas_scale;
        state.swap_sides = swap_sides;
        state.swap_horizontal = swap_horizontal;
        state.view(host, viewport, framebuffer, capture)?;
        state.style_next_draw = state.style_time + 1.0/30.0;
        state.viewport = Some(viewport);
        state.framebuffer = Some(framebuffer);
        if !state.style_animated && state.motion.settled() && (!state.spatial_view || state.piano_workspace.active || !state.spatial_mix.is_animating())
            && (!state.spatial_view || state.piano_workspace.active || !state.spatial_mix.is_interacting())
            && !state.arrangement.is_interacting()
            && !state.editor.notes().iter().any(|n|n.placement_age<1.35) {
            host.invoke("AudicleSystem.Finish", state.event.clone().unwrap())?;
            state.event = None;
        }
    }
    state.previous = pressed;
    state.frames += 1;
    stages.mark();
    let status=state.status();
    drop((snapshot,live,before,after,spheres_before,arrangement_before,arrangement_key));
    stages.mark();
    if let Some(frames)=&mut state.step_timings {
        if frames.len()<8192 {frames.push(stages.values);}else{state.step_timings_omitted+=1;}
    }
    Ok(status)
}

fn prepare_plugin_catalog(state: &mut State, host: &mut Host) {
    #[cfg(test)] let skip_plugin_scan=host.graphics.test_driver;
    #[cfg(not(test))] let skip_plugin_scan=false;
    if cfg!(target_os="macos") && state.browser_enabled() && !state.plugin_browser.scanned && !skip_plugin_scan {
        state.plugin_browser.scanned=true;
        match host.invoke("PluginConnectionSystem.Command",json!({"op":"scan"})) {
            Ok(status)=>state.plugins.refresh(status),
            Err(error)=>state.plugin_browser.error=Some(error),
        }
    }
}

pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    if hook=="PrimeWorld" {
        if !host.audio_workstation.resident {return Ok(Value::Null);}
        let game=json!({"entity":host.graphics.prepared_entity().ok_or("game preload scene missing")?});
        host.invoke("GraphicsConnectionSystem.Suspend",game.clone())?;
        let prepared=host.invoke("AudioWorkstationSystem.Resume",json!({"computer_screen":true})).and_then(|status| {
            let entity=&status["visual"]["entity"];
            host.invoke("GraphicsConnectionSystem.Pump",json!({"entity":entity,"hidden_scene":true}))?;
            // Complete the native window's first presentation while still loading,
            // before the application accepts gameplay input. Prewarm itself stays
            // hidden; this last startup view pays Cocoa/surface activation once.
            host.invoke("GraphicsConnectionSystem.Pump",json!({"entity":entity,"force_draw":true}))
        });
        let cleanup=if host.audio_workstation.resident && !host.audio_workstation.parked {host.invoke("AudioWorkstationSystem.Suspend",Value::Null)}else{Ok(Value::Null)};
        let restored=host.invoke("GraphicsConnectionSystem.Resume",game);
        return prepared.and(cleanup).and(restored).map(|_|Value::Null);
    }
    // Complete the first DAW frame before the game creates/presents its scene.
    // Subsequent entry borrows this resident Entity, GPU client and panel kit.
    if hook=="Prewarm" && cfg!(target_os="macos") && host.backend()=="WebGPU"
        && std::env::var("WORMWOOD_WORKSTATION_REFERENCE_OPTICS").as_deref()!=Ok("1") {
        if host.audio_workstation.resident {return Ok(Value::Null);}
        host.invoke("GraphicsConnectionSystem.PrewarmWorkstation",Value::Null)?;
        host.invoke("AudioWorkstationSystem.Begin",Value::Null)?;
        host.invoke("AudioWorkstationSystem.PreparePlugins",Value::Null)?;
        host.audio_workstation.resident=true;
        for _ in 0..120 {host.audio_workstation.motion.advance(1./60.);}
        let visual=host.audio_workstation.visual.clone().ok_or("resident DAW has no visual")?;
        let prepared=host.invoke("AudioWorkstationSystem.Step",json!({"dt":0.,"aspect":16./9.,"viewport":[1280.,720.],"framebuffer":[1280.,720.],"canvas_scale":1.,"keys":[false,false,false,false,false]}))
            .and_then(|_|host.invoke("GraphicsConnectionSystem.Pump",json!({"entity":visual["entity"],"background":true})));
        let cleanup=host.invoke(if prepared.is_ok(){"AudioWorkstationSystem.Suspend"}else{"AudioWorkstationSystem.Close"},Value::Null);
        return prepared.and(cleanup).map(|_|Value::Null);
    }
    let mut state = std::mem::take(&mut host.audio_workstation);
    let result = (|| match hook {
        "Status" => Ok(state.status()),
        "Prewarm" => {
            host.invoke("WorkstationPanelSystem.Load",Value::Null)?;
            host.invoke("GraphicsConnectionSystem.PrewarmWorkstation",Value::Null)?;
            Ok(Value::Null)
        },
        "Background" => {
            if !state.resident || !state.parked || state.background_next.is_some_and(|at|Instant::now()<at) {return Ok(Value::Null);}
            state.background_next=Some(Instant::now()+Duration::from_millis(100));
            state.style_time+=0.1;
            state.begin_event(host)?;
            let result=(|| {
                let snapshot=host.workstation_snapshot_shared()?.document;
                state.arrangement.sync(&snapshot)?;
                if host.system_enabled("EnvironmentAudioSystem") {
                    let audio=host.invoke("EnvironmentAudioSystem.Status",Value::Null)?;
                    let transport=&audio["transport"];
                    state.transport_controls.sync_playing(transport["playing"]==true);
                    state.arrangement.set_loop_from_transport(transport);
                    state.spatial_mix.sync_with_visualization(&snapshot,Some(&transport["visualization"]),0.1)?;
                    state.playhead_seconds=transport["position_frame"].as_u64().zip(transport["sample_rate"].as_u64().filter(|r|*r>0)).map(|(f,r)|f as f64/r as f64);
                }
                let camera=std::mem::take(&mut state.camera_view);
                let physical=state.computer_screen;state.computer_screen=false;
                let view=state.view(host,[1280.,720.],[1280.,720.],false);
                state.camera_view=camera;state.computer_screen=physical;
                view?;
                host.invoke("GraphicsConnectionSystem.Pump",json!({"entity":state.visual.as_ref().ok_or("resident visual missing")?["entity"],"background":true}))?;
                Ok::<_,String>(Value::Null)
            })();
            let cleanup=state.event.take().map(|event|host.invoke("AudicleSystem.Finish",event)).unwrap_or(Ok(Value::Null));
            result.and(cleanup)
        },
        "PreparePlugins" => {
            // Discovery owns the document while loading native factories. Run
            // before callbacks begin, retaining the existing visible error and
            // once-per-session behavior if discovery fails.
            prepare_plugin_catalog(&mut state, host);
            Ok(Value::Null)
        },
        "Begin" | "Resume" => {
            if (state.visual.is_some() && !(hook=="Resume" && state.resident && state.parked))
                || state.event.is_some()
                || host.graphics.prepared_entity().is_some()
                || host.active_audicle.is_some()
            {
                return Err("close the existing scene before opening the workstation".into());
            }
            // Initial assets are parked offscreen; this scene requests all four
            // open so the first view demonstrates the source slide animation.
            let had_retained=state.retained;
            let event = host.invoke("AudicleSystem.Create", Value::Null)?;
            state.event = Some(event.clone());
            let prepared = (|| {
                let visual=if state.resident && state.parked {
                    let visual=state.visual.clone().ok_or("resident DAW visual missing")?;
                    host.invoke("GraphicsConnectionSystem.FocusWorkstation",visual.clone())?;
                    state.parked=false;
                    visual
                }else{
                    let spec=host.invoke("WorkstationPanelSystem.Load",Value::Null)?;
                    let visual=host.invoke("EntitySystem.Create",spec)?;
                    state.visual=Some(visual.clone());
                    host.invoke("AppearanceSystem.Prepare",visual.clone())?;
                    visual
                };
                if let Some(pointer) = workstation_pointer_mode(cfg!(target_os = "ios")) {
                    host.invoke(
                        "GraphicsConnectionSystem.InputMode",
                        json!({"entity":visual["entity"],"pointer":pointer}),
                    )?;
                }
                host.invoke(
                    "GraphicsConnectionSystem.Caption",
                    json!({"entity":visual["entity"],"text":"Mirror — Audio workstation"}),
                )?;
                host.invoke("AudicleSystem.Ready", event.clone())?;
                host.invoke("AudicleSystem.Go", event)?;
                state.embedded=hook=="Resume";
                state.computer_screen=state.embedded && input["computer_screen"]==true;
                if state.computer_screen && (host.backend()!="WebGPU" || (!cfg!(target_os="macos") || std::env::var("WORMWOOD_WORKSTATION_REFERENCE_OPTICS").as_deref()==Ok("1"))) {return Err("Physical computer display requires the baked WebGPU renderer".into());}
                state.eyewear=if state.embedded {input["eyewear"].as_str().filter(|k|["ar","vr"].contains(k)).map(str::to_owned)}else{None};
                state.frozen_view=if state.eyewear.is_some() || state.computer_screen {host.environment.view().clone()}else{Value::Null};
                // Computer focus freezes the actual observer, independently of furniture.
                state.settings.ar_backdrop=state.eyewear.as_deref()==Some("ar");
                if state.computer_screen {state.camera_view=state.frozen_view.clone();state.camera_view["id"]=json!("player");}
                state.headset_camera=false;state.camera_cycle_down=true;
                let restore=hook=="Resume" && state.retained;
                if !restore {
                    state.motion = Motion::default();
                    if cfg!(target_os="ios") {
                        state.motion.set_mobile_page(Some(MobilePage::Timeline));
                    }
                    state.canvas_scale = 1.;
                    state.swap_sides = false;
                    state.swap_horizontal = false;
                }
                state.previous = [false; 5];
                state.sfx_pointer_down = false;
                state.sfx_started = false;
                state.frames = 0;
                state.presentation_keys=crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_PRESENTATION_KEYS");
                state.shared_snapshot=crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_SHARED_SNAPSHOT");
                state.step_timings=(std::env::var("WORMWOOD_WORKSTATION_CPU_STAGES").as_deref()==Ok("1")).then(Vec::new);
                state.step_timings_omitted=0;
                state.style_animated = crate::workstation::user_style::selected()?.is_some_and(|s|s.animated);
                state.style_time = 0.;
                state.style_next_draw = 0.;
                let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
                state.arrangement.sync(&snapshot)?;
                state.settings.load();
                // A saved theatre camera choice has no camera in standalone mode.
                // Select the existing plain studio backdrop for this session only.
                if !state.embedded {state.settings.use_plain_backdrop();}
                if state.eyewear.is_some(){state.settings.enable_camera_passthrough();}else if state.computer_screen{state.settings.use_plain_backdrop();}
                state.view(host, [1280., 720.], [1280., 720.], false)?;
                if !restore && state.motion.mobile_page().is_none() {state.motion.set_all_open(true);}
                state.motion.enable_canonical_left(crate::workstation::benchmark::motion_input::enabled() && crate::workstation::benchmark::motion_input::context(state.embedded,state.settings.passthrough(),state.style_animated,host.backend()));
                state.retained=false;
                Ok::<_, String>(state.status())
            })();
            if let Err(error) = prepared {
                let cleanup = state.close(host,hook=="Resume" && had_retained);
                return Err(format!(
                    "workstation preparation: {error}; cleanup: {:?}",
                    cleanup.err()
                ));
            }
            prepared
        }
        "ComputerKeyboard" => {
            let enabled=match input["action"].as_str(){
                Some("toggle")=>!state.midi_sender.enabled(),
                Some("set")=>input["enabled"].as_bool().ok_or("computer keyboard enabled must be boolean")?,
                _=>return Err("computer keyboard action must be toggle or set".into()),
            };
            state.midi_sender.set_enabled(enabled);
            state.configure_midi_meter(host)?;
            state.flush_midi(host)?;
            Ok(state.status())
        },
        "Step" => step(&mut state, host, input),
        "DrumRack"=>{
            if state.visual.is_none(){return Err("workstation is closed".into());}
            state.begin_event(host)?;
            let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
            if input["action"]=="open"{
                state.editor.update(&Default::default(),crate::workstation::piano_roll::Rect{left:0.,top:0.,width:0.,height:0.},0.)?;
                state.arrangement.cancel_interaction();
                state.drum_ui.open=true;state.piano_workspace.active=false;state.spatial_view=false;
            }else{
                let mut command=input.clone();command["document_revision"]=snapshot["revision"].clone();
                host.invoke("WorkstationDataSystem.DrumRack",command)?;
            }
            let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
            state.drum_ui.sync(&snapshot);state.arrangement.sync(&snapshot)?;state.viewport=None;
            Ok(state.status())
        },
        "OpenPianoRoll" => {
            if state.visual.is_none() {return Err("workstation is closed".into());}
            state.begin_event(host)?;
            state.open_roll(host,input["id"].as_str())?;
            state.editor_error=None;
            state.viewport=None;
            Ok(state.status())
        }
        "SetPanel" => {
            if state.visual.is_none() {
                return Err("workstation is closed".into());
            }
            let edge = input["edge"]
                .as_str()
                .and_then(Edge::parse)
                .ok_or("panel edge must be top, bottom, left or right")?;
            let open = input["open"].as_bool();
            let expanded = input["expanded"].as_bool();
            let fullscreen = input["fullscreen"].as_bool();
            if open.is_none() && expanded.is_none() && fullscreen.is_none() {
                return Err("panel requires an open, expanded or fullscreen boolean".into());
            }
            if expanded.is_some() && !matches!(edge, Edge::Left | Edge::Bottom) {
                return Err("only the browser and piano-roll panels expand".into());
            }
            let mut motion = state.motion.clone();
            let mut changed = false;
            if let Some(fullscreen)=fullscreen {
                changed|=motion.set_fullscreen(edge,fullscreen);
                if fullscreen && edge==Edge::Bottom {state.bottom.controls.page=crate::workstation::bottom_panel::Page::Mixer;}
            }
            if let Some(expanded) = expanded {
                changed |= motion.set_expanded(edge, expanded);
            }
            if let Some(open) = open {
                changed |= motion.set_open(edge, open);
            }
            if changed {
                state.begin_event(host)?;
                state.motion = motion;
            }
            Ok(state.status())
        }
        "Close" | "Suspend" => {
            state.close(host,hook=="Suspend")?;
            Ok(Value::Null)
        }
        _ => Err("unknown audio workstation hook".into()),
    })();
    host.audio_workstation = state;
    result
}

// Browser payloads are independent of animated panel geometry. Only a real
// viewport/scale change invalidates their coordinate space; drop targets still
// use the current timeline and panel bounds on every sample.
fn browser_gesture_canvas(view: Viewport) -> crate::workstation::piano_roll::Rect {
    crate::workstation::piano_roll::Rect {
        left: view.canvas_scale, top: 0., width: view.width, height: view.height,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn browser_drags_survive_panel_refresh_but_cancel_on_resize() {
        use super::*;
        use crate::workstation::{browser, plugin_browser, panel_motion, piano_roll};
        let view=Viewport{width:1280.,height:720.,canvas_scale:1.};
        for resize in [false,true] {
            let mut selection=browser::Selection::default();
            let mut samples=plugin_browser::Ui::default();
            samples.library.entries.push(json!({"name":"sample.wav","format":"WAV","path":"sample.wav"}));
            let catalog=samples.library.entries.clone();
            let mut motion=panel_motion::Snapshot::default();
            motion.extensions=[1.;4];
            let layout=plugin_browser::layout(view,&motion,Options::default(),&catalog,3,0.).unwrap();
            let row=layout.rows.iter().find(|row|matches!(row.item,Some(plugin_browser::Item::Audio(_)))).unwrap();
            let down=piano_roll::Input{pointer:Some([row.rect[0]+5.,row.rect[1]+5.]),left:true,..Default::default()};
            let canvas=browser_gesture_canvas(view);
            selection.update(&down,Some(browser::Action::NewTrack("audio")),None,1,canvas,false);
            samples.update(&down,&layout,1,[canvas.left,canvas.top,canvas.width,canvas.height],None,false);
            assert!(samples.dragging_audio());
            let before=piano_roll_scene::canvas(view,&motion,Options::default()).unwrap();
            motion.extensions=[0.8;4];
            let after=piano_roll_scene::canvas(view,&motion,Options::default()).unwrap();
            assert_ne!([before.left,before.top,before.width,before.height],[after.left,after.top,after.width,after.height]);
            let view=Viewport{width:if resize{1300.}else{1280.},..view};
            let canvas=browser_gesture_canvas(view);
            let layout=plugin_browser::layout(view,&motion,Options::default(),&catalog,3,0.).unwrap();
            let up=piano_roll::Input{pointer:Some([600.,300.]),..Default::default()};
            let component=selection.update(&up,None,Some(0),1,canvas,false);
            let sample=samples.update(&up,&layout,1,[canvas.left,canvas.top,canvas.width,canvas.height],Some(&json!({"id":"audio","kind":"audio"})),false);
            assert_eq!(component.is_some(),!resize);
            assert_eq!(sample.is_some(),!resize);
        }
    }

    #[test]
    fn embedded_startup_preserves_physical_screen_selection() {
        use super::*;
        let request=embedded_resume_input(&json!({"computer_screen":true}));
        assert_eq!(request["computer_screen"],true);
        assert!(request["eyewear"].is_null());
        let wearable=embedded_resume_input(&json!({"eyewear":"ar"}));
        assert_eq!(wearable["eyewear"],"ar");
        assert_eq!(wearable["computer_screen"],false);
    }

    #[test]
    fn timeline_panel_navigation_returns_with_retained_midi_clip() {
        use super::*;
        let mut state=State::default();
        let doc=json!({"revision":1,"tracks":[{"id":"keys","kind":"midi"}],"clips":[{"id":"solo","track":"keys","kind":"midi","start_seconds":0.,"bpm":120.,"length_quarters":8.,"meter":{"numerator":4,"denominator":4},"notes":[]}]});
        state.canvas_scale=1.;state.editor.open(&doc,"solo").unwrap();
        state.piano_workspace.focus_clip(&doc,"solo");
        let window=json!({"flight":{"workstation":{"panels":[true,true,true,true],"overview":true,"all":true}}});
        assert_eq!(state.desktop_panel_navigation(&window,&mut json!({"wheel":[0.,-1.]})),([false;5],false));
        // Both Tab and Escape leave the accepted clip loaded on departure.
        state.piano_workspace.active=false;
        assert!(state.editor.is_open());
        assert_eq!(state.desktop_panel_navigation(&window,&mut json!({"wheel":[0.,-1.]})),([true,true,true,true,false],true));
        state.piano_workspace.active=true;
        assert_eq!(state.desktop_panel_navigation(&window,&mut json!({"wheel":[0.,-1.]})),([false;5],false));
    }
    #[test]
    fn vr_observer_moves_without_advancing_the_paused_scene(){
        use super::*;
        let frozen=json!({"position":[0.,0.,0.],"time":7.,"instances":[1],"water_instances":[2]});
        let live=json!({"position":[2.,0.,0.],"time":9.,"instances":[3],"water_instances":[4]});
        let view=super::wearable_player_view(Some("vr"),&frozen,&live,true,false,1.6).unwrap();
        assert_eq!(view["position"],live["position"]);assert_eq!(view["time"],frozen["time"]);
        assert_eq!(view["instances"],frozen["instances"]);assert_eq!(view["water_instances"],frozen["water_instances"]);
    }
    #[test]
    fn overview_requires_wheel_and_persists_after_shift_release(){
        use super::*;
        let mut s=State::default();s.canvas_scale=1.;
        let held=json!({"flight":{"workstation":{"overview":true}}});
        assert!(!s.desktop_panel_navigation(&held,&mut json!({"wheel":[0.,0.]})).1);
        let mut down=json!({"wheel":[0.,-1.]});assert!(s.desktop_panel_navigation(&held,&mut down).1);assert_eq!(down["wheel"],json!([0.,0.]));
        s.canvas_scale=0.61;assert!(s.desktop_panel_navigation(&Value::Null,&mut Value::Null).1);
        assert!(!s.desktop_panel_navigation(&held,&mut json!({"wheel":[0.,1.]})).1);
    }
    #[test]
    fn repeated_shift_zoom_ticks_cannot_pan_the_timeline_at_either_limit(){
        use super::*;
        let held=json!({"flight":{"workstation":{"overview":true}}});
        for (scale,wheel,overview) in [(0.61,-1.,true),(1.,1.,false)] {
            let mut state=State::default();state.canvas_scale=scale;
            for _ in 0..8 {
                let mut editor=json!({"wheel":[0.,wheel]});
                assert_eq!(state.desktop_panel_navigation(&held,&mut editor).1,overview);
                assert_eq!(editor["wheel"],json!([0.,0.]),"Shift-wheel remains overview input even when already at its limit");
            }
        }
    }
    #[test]
    fn workstation_input_mode_does_not_activate_ios_pause_menu() {
        assert_eq!(super::workstation_pointer_mode(true), None);
        assert_eq!(super::workstation_pointer_mode(false), Some(true));
    }
    #[test]
    fn browser_drag_consumes_desktop_escape_through_cancelled_release() {
        use super::*;use crate::workstation::piano_roll::{Input,Rect};
        let mut state=State::default();let rect=Rect{left:0.,top:0.,width:500.,height:300.};
        assert!(!state.consumes_escape());
        let mut input=Input{pointer:Some([20.,30.]),left:true,..Default::default()};
        state.browser_selection.update(&input,Some(browser::Action::NewTrack("midi")),None,0,rect,false);
        assert!(state.consumes_escape());input.escape=true;
        state.browser_selection.update(&input,None,Some(0),0,rect,false);
        assert!(state.consumes_escape());input.left=false;
        state.browser_selection.update(&input,None,Some(0),0,rect,false);
        assert!(!state.consumes_escape());
    }
    #[test]
    fn transport_hits_follow_authored_panel_motion_and_leave_gaps_empty() {
        use super::*;
        let mut state=State::default();state.canvas_scale=1.;
        state.motion.set_all_open(true);state.motion.advance(1.);
        let viewport=[1280.,720.];
        let view=Viewport{width:viewport[0],height:viewport[1],canvas_scale:1.};
        let rect=crate::workstation::panel_layout::layout(view,&state.motion.snapshot(),Options::default()).unwrap()[0];
        let mpp=1.3/viewport[1];
        let safe=crate::workstation::panel_layout::optical_shape(rect.width*mpp,rect.height*mpp).unwrap().safe_rectangle_m;
        let scale=(safe[0]*0.88/0.205).min(safe[1]*0.80/0.04)/mpp;
        for i in 0..4 {
            assert_eq!(transport_hit(&state,viewport,[rect.center()[0]+(i as f64-1.5)*0.055*scale,rect.center()[1]]),Some(i));
        }
        assert_eq!(transport_hit(&state,viewport,rect.center()),None);
        let original=[rect.center()[0]+1.5*0.055*scale,rect.center()[1]];
        state.motion.set_all_open(false);state.motion.advance(1.);
        assert_eq!(transport_hit(&state,viewport,original),None);
    }
}

/// Desktop event loop; reusable Begin/Step/Close remain separate entry points
/// for native application adapters. No global scheduler or audio VM is started.
pub(crate) fn run(host: &mut Host, input: &Value) -> Result<Value, String> {
    run_mode(host,input,false)
}

// Preserve presentation selection through the shared embedded startup boundary.
fn embedded_resume_input(input: &Value) -> Value {
    json!({"eyewear":input["eyewear"],"computer_screen":input["computer_screen"]==true})
}

pub(crate) fn run_embedded(host: &mut Host, input: &Value) -> Result<Value, String> {
    run_mode(host,input,true)
}

// Opt-in finite benchmark only, on a fresh process-local document. It uses the
// normal MIDI renderer/native output and does not instrument the audio callback.
fn prepare_benchmark_audio(host: &mut Host, attempted: &mut bool) -> Result<Value, String> {
    if host.invoke("EnvironmentAudioSystem.Status",Value::Null)?["active"] == true {
        return Err("audio benchmark cannot take over an existing audio session".into());
    }
    let mut snapshot = host.invoke("WorkstationDataSystem.Snapshot", Value::Null)?;
    if snapshot["tracks"].as_array().is_none_or(|tracks| !tracks.is_empty())
        || snapshot["clips"].as_array().is_none_or(|clips| !clips.is_empty()) {
        return Err("audio benchmark requires a fresh empty document".into());
    }
    host.invoke("WorkstationDataSystem.PutTrack", json!({"document_revision":snapshot["revision"],
        "id":"benchmark-midi","name":"Benchmark MIDI","kind":"midi"}))?;
    snapshot = host.invoke("WorkstationDataSystem.Snapshot", Value::Null)?;
    let notes: Vec<_> = (0..512).map(|i| json!({"id":format!("benchmark-{i}"),
        "quarter":i as f64,"length_quarters":0.75,"pitch":60+i%12,"velocity":0.35})).collect();
    host.invoke("WorkstationDataSystem.PutClip", json!({"document_revision":snapshot["revision"],
        "id":"benchmark-midi-clip","track":"benchmark-midi","kind":"midi","start_seconds":0.,
        "bpm":120.,"length_quarters":512.,"notes":notes}))?;
    *attempted=true;
    let opened = host.invoke("EnvironmentAudioSystem.Begin",json!({"mode":"stereo"}))?;
    host.invoke("EnvironmentAudioSystem.Transport",json!({"action":"play","start_frame":0,
        "master_gain":0.05}))?;
    Ok(opened)
}

// Standalone DAW editing remains usable after a cleanly released audio-start
// failure. A failed cleanup is still fatal: do not hide retained native owners.
fn finish_optional_audio_start(start:Result<Value,String>,cleanup:impl FnOnce()->Result<Value,String>)->Result<(bool,Option<String>),String>{
    match start{
        Ok(_)=>Ok((true,None)),
        Err(error)=>{cleanup().map_err(|cleanup|format!("{error}; audio startup cleanup failed: {cleanup}"))?;Ok((false,Some(error)))}
    }
}
fn run_mode(host: &mut Host, input: &Value, embedded: bool) -> Result<Value, String> {
    let bubble_diagnostic=input["diagnostic_bubbles"]==true;
    let controls_diagnostic=input["diagnostic_controls"]==true;
    let limit = if input["frames"].is_null() {
        None
    } else {
        Some(
            input["frames"]
                .as_u64()
                .filter(|n| (1..=if input["benchmark"].is_null() {3600} else {20_120}).contains(n))
                .ok_or("frame limit must be 1..3600, or bounded by 20120 for a benchmark")?,
        )
    };
    let mut benchmark = crate::workstation::benchmark::Benchmark::parse(input, limit)?;
    // Explicit finite native verification of animated notes over the retained
    // game image. Normal application startup still opens an empty document.
    let benchmark_bubbles=benchmark.is_some() && std::env::var("WORMWOOD_WORKSTATION_BENCHMARK_BUBBLES").as_deref()==Ok("1");
    let mut motion_comparison=crate::workstation::benchmark::motion_comparison::Comparison::selected(benchmark.is_some())?;
    let mut motion_input=crate::workstation::benchmark::motion_input::Replay::selected(benchmark.is_some())?;
    let benchmark_audio = input["benchmark"]["audio"] == true;
    let require_passthrough = input["benchmark"]["require_embedded_passthrough"] == true;
    if require_passthrough && (!embedded || benchmark.is_none()) {return Err("pass-through benchmark requires a finite embedded session".into());}
    let benchmark_camera_fraction=crate::workstation::benchmark::camera::from_environment(require_passthrough)?;
    let benchmark_camera_animate=std::env::var("WORMWOOD_COMBINED_CAMERA_CLOCK_REPLAY").as_deref()==Ok("1");
    if benchmark_camera_animate&&benchmark_camera_fraction.is_none(){return Err("camera clock replay requires explicit fraction in a finite embedded benchmark".into());}
    let capture_validation=benchmark.is_some()&&std::env::var("WORMWOOD_COMBINED_CAPTURE").as_deref()==Ok("1");
    let camera_replay=require_passthrough&&std::env::var("WORMWOOD_COMBINED_CAMERA_REPLAY").as_deref()==Ok("1");
    let track_fixture=std::env::var("WORMWOOD_COMBINED_TRACK_FIXTURE").as_deref()==Ok("1");
    if track_fixture&&!require_passthrough{return Err("combined track fixture requires the finite pass-through benchmark".into());}
    // Finite verification uses the same pointer/controller route in standalone
    // and embedded sessions; embedded retains the real world and game audio.
    let fader_replay=benchmark.is_some() && std::env::var("WORMWOOD_FADER_REPLAY").as_deref()==Ok("1");
    let mut fader_values=Vec::new();
    let mut track_fixture_id=None;
    let playback_fixture=crate::workstation::benchmark::combined_playback_fixture(require_passthrough,std::env::var("WORMWOOD_COMBINED_TRANSPORT_PLAYBACK").ok().as_deref())?;
    let stage_lighting_replay=std::env::var("WORMWOOD_STAGE_LIGHTING_REPLAY").as_deref()==Ok("1");
    if stage_lighting_replay&&(!require_passthrough||!playback_fixture){return Err("stage lighting replay requires finite embedded transport playback".into());}
    let mut stage_lighting_evidence=Vec::new();
    let mut playback_fixture_started=false;let mut playback_fixture_final=Value::Null;
    let wearable_verify=input["wearable_verify"]==true;
    let observer_motion=input["benchmark"]["observer_motion"]==true;
    if observer_motion && (!require_passthrough || input["eyewear"]!="vr") {
        return Err("observer motion requires a finite embedded VR benchmark".into());
    }
    if wearable_verify && (!require_passthrough || !["ar","vr"].contains(&input["eyewear"].as_str().unwrap_or(""))){return Err("wearable verification requires a finite embedded benchmark".into());}
    let mut wearable_evidence=Vec::new();
    let mut startup_focus=Vec::new();
    let mut passthrough_frames=0u64;
    let mut loop_intervals=Vec::new();
    let mut previous_measured_loop:Option<Instant>=None;
    if benchmark_audio && (embedded || benchmark.is_none() || bubble_diagnostic || controls_diagnostic
        || input["diagnostic_spheres"]==true || input["diagnostic_arrangement"]==true) {
        return Err("audio benchmark requires a standalone finite benchmark without other diagnostics".into());
    }
    if controls_diagnostic && (embedded || limit != Some(4) || benchmark.is_some()
        || bubble_diagnostic || input["diagnostic_spheres"]==true || input["diagnostic_arrangement"]==true) {
        return Err("control diagnostic requires four standalone frames without another diagnostic or benchmark".into());
    }
    // Explicit finite capture runs can settle the same motion state on every
    // backend without letting shader-compilation time advance the animation.
    let capture_dt = if controls_diagnostic {Some(1.)} else if limit.is_some() && benchmark.is_none() {
        std::env::var("WORMWOOD_WORKSTATION_CAPTURE_DT").ok().map(|value| {
            value.parse::<f64>().ok().filter(|dt| dt.is_finite() && *dt>0. && *dt<=1.)
                .ok_or("WORMWOOD_WORKSTATION_CAPTURE_DT must be >0 and <=1 seconds")
        }).transpose()?
    } else { None };
    // Explicit bounded capture fixture, never admitted to interactive sessions.
    let capture_panel=std::env::var("WORMWOOD_WORKSTATION_CAPTURE_PANEL").ok().map(|name| {
        if embedded || limit.is_none() || benchmark.is_some() || capture_dt.is_none() {
            return Err("panel capture requires standalone finite frames and CAPTURE_DT".to_owned());
        }
        Edge::parse(&name)
            .ok_or_else(||"capture panel must be top, bottom, left or right".to_owned())
    }).transpose()?;
    let capture_panel_replay=std::env::var("WORMWOOD_WORKSTATION_CAPTURE_PANEL_REPLAY").as_deref()==Ok("1");
    if capture_panel_replay && (capture_panel.is_none() || limit.is_none_or(|n|n<112)
        || capture_dt.is_none_or(|dt|dt>0.05)) {
        return Err("panel replay requires CAPTURE_PANEL, at least112 finite frames and CAPTURE_DT<=0.05".into());
    }
    let mut capture_panel_frames=Vec::new();
    // A rejected Begin belongs to another live scene and must not close it.
    let opened = host.invoke(if embedded {"AudioWorkstationSystem.Resume"}else{"AudioWorkstationSystem.Begin"}, if embedded{embedded_resume_input(input)}else{Value::Null})?;
    // Select the real mixer page only for this finite optical fixture. Restore
    // its previous selection even when rendering fails or the window closes.
    let previous_mixer=(controls_diagnostic || benchmark_audio || fader_replay).then(||host.audio_workstation.plugins.set_mixer(true));
    // Standalone prewarm and physical-screen priming select a plain backdrop.
    // The explicit full-screen embedded benchmark must establish its required
    // observer itself, independently of that resident state or saved settings.
    // This changes only the process-local fixture; no preferences are saved.
    if require_passthrough && !host.audio_workstation.computer_screen {
        host.audio_workstation.settings.enable_camera_passthrough();
    }
    if wearable_verify{host.audio_workstation.audio_rays=true;}
    let visual = opened["visual"].clone();
    let gpu_draw_offset=if benchmark.is_some(){host.invoke("GraphicsConnectionSystem.Pump",json!({"entity":visual["entity"],"finish_only":true}))?["gpu_draw_offset"].as_u64().unwrap_or(0)}else{0};
    let mut nondrawable_frame=Value::Null;
    let mut frames = 0u64;
    let mut benchmark_started = None;
    let display_aware=(!embedded || crate::workstation::render_defaults::enabled("WORMWOOD_COMBINED_DISPLAY_CADENCE")) && host.backend()=="WebGPU" && crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_PRERENDER")
        && crate::workstation::render_defaults::enabled("WORMWOOD_WORKSTATION_DISPLAY_CADENCE");
    let benchmark_paced=benchmark.is_some() && std::env::var("WORMWOOD_WORKSTATION_BENCHMARK_PACED").as_deref()==Ok("1");
    let interactive_paced=benchmark.is_none()&&!bubble_diagnostic&&!controls_diagnostic&&capture_dt.is_none();
    let mut cadence=crate::workstation::frame_cadence::Stats::default();
    let frame_attribution = benchmark.is_some()
        && std::env::var("WORMWOOD_WORKSTATION_FRAME_ATTRIBUTION").as_deref() == Ok("1");
    let timeline_enabled=crate::workstation::frame_cadence::timeline_enabled();
    if timeline_enabled&&!frame_attribution{return Err("Frame timeline requires an explicit finite benchmark with FRAME_ATTRIBUTION=1".into());}
    let stamp=crate::workstation::frame_cadence::timeline_stamp;
    let mut benchmark_audio_start = Value::Null;
    let run_started = Instant::now();
    let audio_available=host.system_enabled("EnvironmentAudioSystem");
    let mut own_audio=false;
    let mut benchmark_audio_report=json!({"requested":benchmark_audio,"started":false,
        "fixture":if benchmark_audio{json!("512 quarter-spaced MIDI notes at120BPM, velocity0.35, master gain0.05")}else{Value::Null},
        "meter_geometry":"static authored mixer specimens; no audio-driven meter animation",
        "control_gestures_generated":false,"input_to_effect_latency_measured":false,
        "native_pumps":0,"consumed_visualization_observations":0});
    #[cfg(target_os="ios")]
    {
        take_return_to_game();
        if let Ok(mut taps)=TRANSPORT_TAPS.lock(){taps.clear();}
        if embedded {unsafe {ww_ios_workstation_focus(true);}}
    }
    let execution = (|| -> Result<&'static str, String> {
        if std::env::var("WORMWOOD_CHECK_PREAMP").as_deref()==Ok("1") {
            if limit.is_none(){return Err("Preamp diagnostic requires a finite frame limit".into());}
            let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
            let id=unused_id(&snapshot["tracks"],"preamp-check");
            host.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":snapshot["revision"],"id":id,"kind":"audio","name":"Preamp check"}))?;
            for chain in ["playback","playback","preamp","preamp"] {host.invoke("PluginConnectionSystem.Command",json!({"op":"add","track":id,"stock":"LShaping","chain":chain,"sample_rate":48000}))?;}
            host.audio_workstation.plugins.selected=Some(id);
            host.audio_workstation.bottom.controls.page=crate::workstation::bottom_panel::Page::Plugins;
            host.audio_workstation.plugins.set_bottom_page(1);
            host.audio_workstation.motion.set_all_open(true);
            host.audio_workstation.motion.set_preamp(true);
            for _ in 0..120{host.audio_workstation.motion.advance(1./60.);}
        }
        if track_fixture || (fader_replay && !benchmark_audio) {
            // One ordinary empty audio track exercises the real first I/O page
            // and its authored crosswalk. It is never armed or given a clip.
            let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
            let id=unused_id(&snapshot["tracks"],"combined-crosswalk-check");
            host.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":snapshot["revision"],
                "id":id,"kind":"audio","name":"Crosswalk performance"}))?;
            track_fixture_id=Some(id);
        }
        if playback_fixture {
            if !audio_available{return Err("combined playback fixture needs the existing game audio session".into());}
            let status=host.invoke("EnvironmentAudioSystem.Status",Value::Null)?;
            let document=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
            if status["active"]!=true||status["transport"]["playing"]==true||document["clips"].as_array().is_none_or(|v|!v.is_empty()){
                return Err("combined playback fixture requires active idle audio and a fresh document without clips".into());
            }
            host.invoke("EnvironmentAudioSystem.Transport",json!({"action":"play","start_frame":0}))?;
            playback_fixture_started=true;
            if stage_lighting_replay {
                host.audio_workstation.settings.enable_camera_passthrough();
                for (id,param) in [("stage-intensity-check",0),("stage-red-check",1),("stage-blue-check",3)] {
                    let revision=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?["revision"].clone();
                    host.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":revision,"id":id,"kind":"cc","name":id}))?;
                    for patch in [json!({"cc_action":"target_lane"}),json!({"cc_action":"target_param_pick","cc_parameter_slot":param})] {
                        let revision=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?["revision"].clone();
                        let mut patch=patch;patch["id"]=json!(id);patch["document_revision"]=revision;
                        host.invoke("WorkstationDataSystem.SetTrackMix",patch)?;
                    }
                    let points:Vec<_>=(0..=120).map(|i|json!({"offset_seconds":i as f64*0.5,"value":if (i+param)%2==0{0.1}else{0.95}})).collect();
                    let revision=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?["revision"].clone();
                    host.invoke("WorkstationDataSystem.PutClip",json!({"document_revision":revision,"id":id,"track":id,"kind":"cc","start_seconds":0.,"duration_seconds":60.,"points":points}))?;
                }
            }

        }
        if benchmark_audio {
            if audio_available {
                let mut attempted=false;
                match prepare_benchmark_audio(host,&mut attempted) {
                    Ok(opened) => {
                        own_audio=true;
                        benchmark_audio_report["started"]=json!(true);
                        benchmark_audio_report["opened"]=opened;
                    }
                    Err(error) => {
                        benchmark_audio_report["unavailable_reason"]=json!(error);
                        // Begin already attempts its own cleanup; retry the normal
                        // close route before continuing a graphics-only fallback.
                        if attempted {
                            benchmark_audio_report["failed_start_cleanup"]=host.invoke("EnvironmentAudioSystem.Close",Value::Null)?;
                        }
                    }
                }
            } else {
                benchmark_audio_report["unavailable_reason"]=json!("EnvironmentAudioSystem is disabled");
            }
        }
        if !embedded && audio_available && benchmark.is_none() && limit.is_none() {
            let result=host.invoke("EnvironmentAudioSystem.Begin",json!({"mode":"stereo"}));
            let (started,error)=finish_optional_audio_start(result,||host.invoke("EnvironmentAudioSystem.Close",Value::Null))?;
            own_audio=started;
            host.audio_workstation.audio_startup_error=error;
        }
        if input["diagnostic_bubbles"]==true || input["diagnostic_arrangement"]==true || benchmark_bubbles {
            // Explicit finite native diagnostic only; normal launch is empty.
            // Unique IDs keep a programmatic invocation from replacing user data.
            let diagnostic_edo=std::env::var("WORMWOOD_PIANO_CHECK_EDO").ok().map(|v|v.parse::<u16>().map_err(|_|"invalid diagnostic EDO")).transpose()?.unwrap_or(12);
            if !(1..=4096).contains(&diagnostic_edo){return Err("diagnostic EDO must be 1..4096".into());}
            let diagnostic_bend=std::env::var("WORMWOOD_PIANO_CHECK_BEND").ok().map(|s|s.parse::<f64>().map_err(|_|"invalid diagnostic bend")).transpose()?.unwrap_or(0.);
            crate::workstation::note_bend::read(&json!(diagnostic_bend))?;
            let diagnostic_notes=json!([{"id":"short","quarter":1.,"length_quarters":0.5,"pitch":60,"velocity":0.8},
                {"id":"medium","quarter":3.,"length_quarters":1.,"pitch":61,"velocity":0.8},
                {"id":"long","quarter":6.,"length_quarters":2.,"pitch":67,"velocity":0.8,"bend_semitones":diagnostic_bend},
                {"id":"sustained","quarter":10.,"length_quarters":4.,"pitch":72,"velocity":0.8,"bend_semitones":-diagnostic_bend}]);
            let diagnostic_notes=diagnostic_notes.as_array().unwrap().iter().map(|n|crate::workstation::piano_tuning::note(n,diagnostic_edo)).collect::<Vec<_>>();
            let mut snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
            let track=unused_id(&snapshot["tracks"],"bubble-check-track");
            let id=unused_id(&snapshot["clips"],"bubble-check-clip");
            host.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":snapshot["revision"],
                "id":track,"kind":"midi","name":"Bubble optical check"}))?;
            snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
            host.invoke("WorkstationDataSystem.PutClip",json!({"document_revision":snapshot["revision"],
                "id":id,"track":track,"kind":"midi","start_seconds":0.,"bpm":120.,"length_quarters":16.,
                "edo":diagnostic_edo,"notes":diagnostic_notes}))?;
            if input["diagnostic_bubbles"]==true || benchmark_bubbles {
                host.invoke("AudioWorkstationSystem.OpenPianoRoll",json!({"id":id}))?;
            } else {
                // Explicit finite scene check only. Normal launch remains empty.
                let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
                let cc=unused_id(&snapshot["tracks"],"arrangement-check-cc");
                host.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":snapshot["revision"],
                    "id":cc,"kind":"cc","name":"Automation"}))?;
                let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
                host.invoke("WorkstationDataSystem.PutClip",json!({"document_revision":snapshot["revision"],
                    "id":unused_id(&snapshot["clips"],"arrangement-check-curve"),"track":cc,"kind":"cc",
                    "start_seconds":3.25,"duration_seconds":5.,"points":[{"offset_seconds":0.,"value":0.5}]}))?;
            }
        }
        // Explicit finite arrangement fixture only; never seeds a user session.
        if input["diagnostic_arrangement"]==true&&std::env::var("WORMWOOD_WORKSTATION_COMP_CHECK").as_deref()==Ok("1") {
            let mut snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
            let track=unused_id(&snapshot["tracks"],"comp-check-audio");
            host.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":snapshot["revision"],"id":track,"kind":"audio","name":"Comp lane check"}))?;
            snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
            let source="comp-check-source";
            let samples=(0..192000).map(|i|((i as f64*440.*std::f64::consts::TAU/48000.).sin()*0.3)as f32).collect::<Vec<_>>();
            host.invoke("WorkstationDataSystem.PutSource",json!({"document_revision":snapshot["revision"],"id":source,"sample_rate":48000,"channels":1,"samples":samples}))?;
            snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
            host.invoke("WorkstationDataSystem.PutClip",json!({"document_revision":snapshot["revision"],"id":unused_id(&snapshot["clips"],"comp-check-clip"),"track":track,"kind":"audio","source":source,"start_seconds":2.,"source_offset_seconds":0.,"duration_seconds":4.}))?;
            for _ in 0..2 {
                snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
                host.invoke("WorkstationDataSystem.AudioPlaylist",json!({"document_revision":snapshot["revision"],"id":track,"action":"duplicate","index":0}))?;
            }
        }
        if input["diagnostic_arrangement"]==true&&std::env::var("WORMWOOD_WORKSTATION_FREE_CANVAS_CHECK").as_deref()==Ok("1") {
            let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
            let source=unused_id(&snapshot["sources"],"free-canvas-source");
            let samples=(0..192000).map(|i|((i as f64*330.*std::f64::consts::TAU/48000.).sin()*0.25)as f32).collect::<Vec<_>>();
            host.invoke("WorkstationDataSystem.PutSource",json!({"document_revision":snapshot["revision"],"id":source,"sample_rate":48000,"channels":1,"samples":samples}))?;
            for (i,lane) in [0.15,0.88,1.45].into_iter().enumerate(){
                let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
                host.invoke("WorkstationDataSystem.PutClip",json!({"document_revision":snapshot["revision"],"id":format!("free-canvas-{i}"),"track":"","kind":"audio","source":source,"lane":lane,"start_seconds":1.+i as f64*2.,"duration_seconds":4.,"source_offset_seconds":0.}))?;
            }
        }
        if input["diagnostic_spheres"]==true {
            crate::workstation::sphere_diagnostic::prepare(host)?;
            host.audio_workstation.spatial_view=true;
        }
        // Measure full-view transitions from the ordinary settled layout,
        // rather than mixing the unrelated startup slide into every sample.
        if capture_panel_replay {host.audio_workstation.motion.advance(1.);}
        if let Some(edge)=capture_panel.filter(|_|!capture_panel_replay) {
            host.invoke("AudioWorkstationSystem.SetPanel",json!({"edge":edge.name(),"fullscreen":true}))?;
        }
        let mut keys = [false; 5];
        let mut aspect = 16. / 9.;
        let mut viewport = [1280., 720.];
        let mut framebuffer = viewport;
        let mut expand = false;
        let mut overview = false;
        let mut editor_input=Value::Null;
        let mut escape_consumed=false;
        let mut transport_held=false;

        let mut headset_window=Value::Null;let mut headset_e_down=true;
        let mut last = Instant::now();
        loop {
            if controls_diagnostic && run_started.elapsed() >= Duration::from_secs(120) {
                return Ok("control_diagnostic_deadline");
            }
            if benchmark.as_ref().is_some_and(|b| frames == b.warmup) && benchmark_started.is_none()
            {
                // Drain the last warmup submission before the measured clock.
                // The completion-only Pump preserves input and redraw state.
                host.invoke("GraphicsConnectionSystem.Pump",json!({"entity":visual["entity"],"finish_only":true}))?;
                // Snapshot counters only after GPU work has completed.
                if (benchmark_audio && own_audio) || (require_passthrough && audio_available) {
                    benchmark_audio_start = benchmark_audio_counter_snapshot(host, run_started);
                }
                host.reset_performance();
                cadence=crate::workstation::frame_cadence::Stats::default();
                benchmark_started = Some(Instant::now());
            }
            let start = Instant::now();
            if benchmark_started.is_some() {
                if let Some(previous)=previous_measured_loop.replace(start){loop_intervals.push(start.duration_since(previous).as_secs_f64()*1000.);}
            }
            let mut frame_checkpoints = frame_attribution.then_some([start; 8]);
            let mut mark_frame = |at: usize| {
                if let Some(times) = &mut frame_checkpoints { times[at] = Instant::now(); }
            };
            let _frame_cpu = crate::host::performance_telemetry::Scope::new(
                crate::host::performance_telemetry::Category::DawGraphics,
            );
            if let Some(benchmark) = &mut benchmark {
                if run_started.elapsed() >= Duration::from_secs(benchmark.deadline_seconds()) {
                    return Ok("benchmark_deadline");
                }
                if let Some((open, expanded, show_overview)) = benchmark.targets(frames).filter(|_|motion_comparison.is_none()&&motion_input.is_none()&&(!fader_replay||frames==0)) {
                    for edge in Edge::ALL {
                        let mut target = json!({"edge":edge.name(),"open":open[edge as usize]});
                        if matches!(edge, Edge::Left | Edge::Bottom) {
                            target["expanded"] = json!(expanded);
                        }
                        host.invoke("AudioWorkstationSystem.SetPanel", target)?;
                    }
                    overview = show_overview;
                }
                keys = [false; 5];
                expand = false;
            }
            if benchmark.is_some()&&std::env::var("WORMWOOD_WORKSTATION_REFRESH_VALIDATION").as_deref()==Ok("content"){
                if let Some(mixer)=match frames{8=>Some(false),40=>Some(true),41=>Some(false),42=>Some(true),110=>Some(false),_=>None}{host.audio_workstation.plugins.set_mixer(mixer);}
            }
            if let Some(replay)=&mut motion_input {
                keys=replay.keys(frames,benchmark.as_ref().unwrap().warmup);overview=false;
                if replay.take_content_change(){
                    let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
                    host.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":snapshot["revision"],"id":"motion-input-content","kind":"midi","name":"Changed live content"}))?;
                }
                if let Some(factor)=replay.take_light_change(){host.audio_workstation.settings.benchmark_light_factor(factor)?;}
            }
            let dt = if motion_comparison.is_some(){0.}else if motion_input.as_ref().is_some_and(|r|r.gate){if frames<benchmark.as_ref().unwrap().warmup{1.}else{1./30.}}
            else if motion_input.is_some(){if frames<benchmark.as_ref().unwrap().warmup{1.}else{start.duration_since(last).as_secs_f64().min(1.)}}else if let Some(dt)=capture_dt {dt} else if input["diagnostic_bubbles"]==true || input["diagnostic_spheres"]==true || input["diagnostic_arrangement"]==true {1.} else if let Some(benchmark) = &benchmark {
                if frames < benchmark.warmup {
                    1.
                } else {
                    1. / 30.
                }
            } else if frames == 0 {
                1. / 60.
            } else {
                start.duration_since(last).as_secs_f64().min(1.)
            };
            last = start;
            if wearable_verify{
                headset_window=json!({"flight":{"focused":true,"move":[true,false,false,false,false,false],"pointer_look":[0.5,0.],"boost":false}});
                // Actual movement and camera controller routes, with deterministic finite input only.
                if frames==4 || frames==8 {editor_input=json!({"focused":true,"security_camera":{"cycle":true}});}else{editor_input=json!({"focused":true,"security_camera":{"pan":[true,false,false,false]}});}
            }
            if observer_motion && frames>=benchmark.as_ref().unwrap().warmup {
                // Repeat bounded forward/back input with a small look sweep.
                // This is normal VR navigation, without camera switching or rays.
                let forward=(run_started.elapsed().as_secs_f64()%8.)<4.;
                headset_window=json!({"flight":{"focused":true,"move":[forward,!forward,false,false,false,false],"pointer_look":[if forward{0.5}else{-0.5},0.],"boost":false}});
            }
            if host.audio_workstation.eyewear.as_deref()==Some("vr") && !host.audio_workstation.headset_camera && headset_window.is_object(){
                let f=&headset_window["flight"];
                let blocked=editor_input["left"]==true || editor_input["right"]==true || host.audio_workstation.bottom.controls.consumes_pointer() || host.audio_workstation.drum_ui.open || host.audio_workstation.track_io.owns_pointer() || host.audio_workstation.plugins.owns_pointer() || host.audio_workstation.browser_selection.owns_pointer() || f["focused"]==false || host.audio_workstation.arrangement.is_interacting() || host.audio_workstation.editor.status()["gesture_active"]==true || editor_input["command"]==true || editor_input["alt"]==true || host.audio_workstation.settings.open || host.audio_workstation.midi_sender.enabled() || host.workstation_export.blocks_timeline_commands();
                let input_motion=!blocked && (f["move"].as_array().is_some_and(|v|v.iter().any(|x|*x==true)) || f["pointer_look"].as_array().is_some_and(|v|v.iter().any(|x|x.as_f64().is_some_and(|x|x!=0.))));
                if input_motion || host.environment.observer_needs_step() {
                host.invoke("PlanetNavigationSystem.Step",json!({"entity":host.environment.view()["entity"],"dt":dt.min(0.05),"move":if blocked{json!([false,false,false,false,false,false])}else{f["move"].clone()},"look":if blocked{json!([0.,0.])}else{f["pointer_look"].clone()},"reset":false,"boost":!blocked && f["boost"]==true,"aspect":aspect,"defer_presentation":true}))?;
                }
            }
            if host.audio_workstation.eyewear.as_deref()==Some("vr") && host.audio_workstation.headset_camera {host.invoke("PlanetNavigationSystem.Pause",Value::Null)?;}
            let audio_producer_called = audio_available && (embedded || own_audio);
            mark_frame(1);
            if audio_available && (embedded || own_audio) {
                let pumped=host.invoke("EnvironmentAudioSystem.Pump",Value::Null);
                if benchmark_audio {
                    match pumped {
                        Ok(status) => {
                            let count=benchmark_audio_report["native_pumps"].as_u64().unwrap_or(0)+1;
                            benchmark_audio_report["native_pumps"]=json!(count);
                            if !status["visualization"].is_null() {
                                let count=benchmark_audio_report["consumed_visualization_observations"].as_u64().unwrap_or(0)+1;
                                benchmark_audio_report["consumed_visualization_observations"]=json!(count);
                            }
                            benchmark_audio_report["last_transport"]=status;
                        }
                        Err(error) => {
                            benchmark_audio_report["runtime_error"]=json!(error);
                            benchmark_audio_report["failed_pump_cleanup"]=host.invoke("EnvironmentAudioSystem.Close",Value::Null)?;
                            own_audio=false;
                        }
                    }
                } else {pumped?;}
            }
            mark_frame(2);
            if input["diagnostic_spheres"]==true {
                crate::workstation::sphere_diagnostic::publish(host)?;
            }
mark_frame(3);
            let capture_frame=capture_validation&&benchmark.as_ref().is_some_and(|b|frames+1==b.warmup || (frames>=b.warmup&&(frames-b.warmup+1)%30==0));
            let motion_capture=if let Some(comparison)=&mut motion_comparison{
                let warmup=benchmark.as_ref().unwrap().warmup;
                host.audio_workstation.motion=comparison.frame(frames,warmup);overview=false;
                comparison.capture_due(frames,warmup)
            }else{false};
            if camera_replay{let f=frames.saturating_sub(benchmark.as_ref().unwrap().warmup);editor_input=json!({"security_camera":{"pan":[false,(30..60).contains(&f),false,false],"cycle":f==75},"focused":true});}
            if fader_replay && frames>=benchmark.as_ref().unwrap().warmup {
                let b=benchmark.as_ref().unwrap();
                let n=frames-b.warmup;
                let packet=&host.audio_workstation.bottom.packet;
                let knob=&packet["faders"][0]["knob"];
                let x=knob[0].as_f64().ok_or("replay requires visible mixer fader")?+12.5;
                let area=&packet["content"];
                let travel=(area[3].as_f64().ok_or("missing mixer height")?-71.).max(1.);
                let progress=(n as f64/32.).fract();
                let value=0.15+0.7*(1.-(2.*progress-1.).abs());
                let y=if n==0{knob[1].as_f64().ok_or("missing fader knob")?+12.5}
                    else{area[1].as_f64().ok_or("missing mixer top")?+24.5+(1.-value)*travel};
                editor_input=json!({"left":n+1<b.measured,"pointer":[x,y],"focused":true});
            }
            if stage_lighting_replay {editor_input["security_camera"]=json!({"theatre":true});}
            // Finite arrangement capture: show the same document before and
            // after the whole-workstation Shift-wheel overview transition.
            if input["diagnostic_arrangement"]==true && std::env::var("WORMWOOD_WORKSTATION_OVERVIEW_CHECK").as_deref()==Ok("1") {
                if frames>=2 {
                    let view=Viewport{width:viewport[0],height:viewport[1],canvas_scale:host.audio_workstation.canvas_scale};
                    let rect=piano_roll_scene::canvas(view,&host.audio_workstation.motion.snapshot(),host.audio_workstation.origami_control.mode.options(false,false))?;
                    editor_input=json!({"pointer":[rect.left+rect.width*0.5,rect.top+rect.height*0.5],"wheel":[0.,-24.],"shift":true});
                    (keys,overview)=host.audio_workstation.desktop_panel_navigation(&json!({"flight":{"workstation":{"overview":true}}}),&mut editor_input);
                }
            }
            if input["diagnostic_arrangement"]==true && std::env::var("WORMWOOD_WORKSTATION_BROWSER_CHECK").as_deref()==Ok("1") && frames>=2 {
                let view=Viewport{width:viewport[0],height:viewport[1],canvas_scale:host.audio_workstation.canvas_scale};
                let options=host.audio_workstation.origami_control.mode.options(false,false);
                let gate=crate::workstation::plugin_browser::layout(view,&host.audio_workstation.motion.snapshot(),options,&[],0,0.)?;
                let r=gate.enable_button();editor_input=json!({"pointer":[r[0]+r[2]*0.5,r[1]+r[3]*0.5],"left":frames==2});
            }
            if capture_panel_replay {
                keys=[false;5];expand=true;
                keys[capture_panel.unwrap() as usize]=matches!(frames,16|48|80);
            }
            host.invoke(
                "AudioWorkstationSystem.Step",
                json!({"dt":dt,"aspect":aspect,"viewport":viewport,"framebuffer":framebuffer,"keys":keys,"expand":expand,
                "canvas_scale":if cfg!(target_os="ios") { mobile_canvas_scale() } else if overview { 0.61 } else { 1. },
                "capture":motion_input.as_ref().is_some_and(|r|r.capture_due()) || capture_frame || motion_capture || (benchmark.is_none() && limit.is_some_and(|n|frames+1==n)),
                "editor":editor_input,"transport_available":audio_available && (embedded || own_audio),
                "benchmark_camera_fraction":crate::workstation::benchmark::camera::frame_fraction(benchmark_camera_fraction,frames.saturating_sub(benchmark.as_ref().map_or(0,|b|b.warmup)),benchmark_camera_animate),
                "diagnostic_spheres":input["diagnostic_spheres"]==true,
                "spatial_view":if input["diagnostic_spheres"]==true {json!(true)} else {Value::Null}}),
            )?;
            if stage_lighting_replay {
                stage_lighting_evidence.push(json!({"frame":frames,"seconds":host.audio_workstation.playhead_seconds,"camera":host.audio_workstation.camera_view["id"],"levels":host.audio_workstation.camera_view["stage_lighting"]}));
            }
            if embedded && audio_available && (host.audio_workstation.camera_view.is_object() || (host.audio_workstation.eyewear.as_deref()==Some("vr") && !host.audio_workstation.headset_camera)) {
                let camera=if host.audio_workstation.camera_view.is_object(){&host.audio_workstation.camera_view}else{host.environment.view()};
                host.invoke("EnvironmentAudioSystem.Frame",json!({"position":camera["position"],"forward":camera["forward"],"up":if camera["up"].is_array(){camera["up"].clone()}else{json!([0.,1.,0.])},"dt":dt.min(0.1)}))?;
            }
            if fader_replay && frames>=benchmark.as_ref().unwrap().warmup {
                fader_values.push(host.audio_workstation.bottom.packet["faders"][0]["normalized"].as_f64().ok_or("missing applied fader value")?);
            }
            if wearable_verify || (observer_motion && frames%60==0){wearable_evidence.push(json!({"frame":frames,"player":host.environment.view()["position"],"camera":host.audio_workstation.camera_view["position"],"camera_id":host.audio_workstation.camera_view["id"],"eyewear":host.audio_workstation.eyewear,"camera_mode":host.audio_workstation.headset_camera,"rays":host.audio_workstation.audio_rays}));}
            if require_passthrough {
                if !host.audio_workstation.embedded || (!host.audio_workstation.settings.passthrough() && !host.audio_workstation.computer_screen)
                    || host.audio_workstation.camera_view["id"].as_str().is_none()
                    || host.theatre.venue.as_ref().is_none_or(|v|v.vertices.is_empty()) {
                    return Err("combined benchmark did not render an embedded theatre observer".into());
                }
                passthrough_frames+=1;
            }
            mark_frame(4);
            if std::env::var("WORMWOOD_WORKSTATION_INPUT_VALIDATION").as_deref()==Ok("1") && keys[..4].iter().any(|v|*v) {
                let motion=host.audio_workstation.motion.snapshot();
                host.emit(json!({"event":"panel_input_applied","keys":keys,"open":motion.open,"extensions":motion.extensions}))?;
            }
            if host.system_enabled("WorkstationExportSystem") {
                host.invoke("WorkstationExportSystem.Pump", json!({"open_pressed":!host.audio_workstation.settings.open && editor_input["export"]==true}))?;
            }
            let update_seconds = start.elapsed().as_secs_f64();
            // Count real rendered frames in finite diagnostics even after the
            // panels settle. This does not request additional readbacks.
            let mut pump = visual.clone();
            pump["force_draw"] = json!(limit.is_some());
            let render_start = Instant::now();
            mark_frame(5);
            let window = host.invoke("GraphicsConnectionSystem.Pump", pump)?;
            if startup_focus.len()<8 {startup_focus.push(window["flight"]["workstation"]["native_focus"].clone());}
            headset_window=window.clone();
            mark_frame(6);
            if capture_frame&&window["drawn"]==true{
                for (source,suffix) in [("triangle-WebGPU.ppm","ppm"),("workstation-optical-frame.json","json")]{
                    std::fs::copy(host.output_root().join(source),host.output_root().join(format!("combined-frame-{frames:05}.{suffix}"))).map_err(|e|format!("combined capture: {e}"))?;
                }
            }
            if motion_capture&&window["drawn"]==true{
                let file=format!("motion-{frames:05}.ppm");
                std::fs::copy(host.output_root().join("triangle-WebGPU.ppm"),host.output_root().join(&file)).map_err(|e|format!("motion footage: {e}"))?;
                let comparison=motion_comparison.as_mut().unwrap();comparison.record(file,frames+1);
                std::fs::write(host.output_root().join("motion-footage.json"),serde_json::to_vec_pretty(&comparison.report()).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            }
            if let Some(replay)=&mut motion_input{
                if replay.capture_due()&&window["drawn"]==true {std::fs::copy(host.output_root().join("triangle-WebGPU.ppm"),host.output_root().join(format!("input-{:05}.ppm",replay.measured_frame))).map_err(|e|e.to_string())?;}
                replay.record(&host.audio_workstation.motion, &window, start.elapsed().as_secs_f64()*1000.);
            }
            if capture_panel_replay && window["drawn"]==true {
                capture_panel_frames.push(json!({"frame":frames,"wall_ms":start.elapsed().as_secs_f64()*1000.,
                    "fullscreen":host.audio_workstation.motion.snapshot().fullscreen,
                    "retained":window["device"]["frame_attribution"]["retained_bake"]}));
            }
            cadence.observe(&window);
            if let Some(benchmark) = &mut benchmark {
                benchmark.observe_window(&window);
            }
            if window["drawn"] == true {
                if let Some(benchmark) = &mut benchmark {
                    benchmark.record(
                        frames,
                        update_seconds,
                        render_start.elapsed().as_secs_f64(),
                        start.elapsed().as_secs_f64(),
                        &window,
                    );
                }
                frames += 1;
            }
            mark_frame(7);
            if window["drawn"] == true {
                if let (Some(benchmark),Some(times)) = (&mut benchmark,frame_checkpoints) {
                    let wall_ms: [f64; 7] = std::array::from_fn(|i| (times[i+1]-times[i]).as_secs_f64()*1000.);
                    benchmark.attribute_last(frames-1,json!({"version":1,
                        "stage_names":["pre_audio_hooks","audio_producer_and_report","pre_step_hooks",
                            "step_update","post_step_hooks_and_pump_setup","pump","post_pump_reporting"],
                        "wall_ms":wall_ms,"total_wall_ms":(times[7]-times[0]).as_secs_f64()*1000.,
                        "audio_producer_called":audio_producer_called,
                        "performance_enabled":crate::host::performance_telemetry::enabled(),
                        "scope":"Adjacent CPU wall intervals through benchmark record/observe; excludes this attribution append, later input/exit handling and pacing. Complete-loop elapsed retains all overhead. Audio includes producer status bookkeeping."}));
                }
            }
            let mut loop_trace=if timeline_enabled{frame_checkpoints.map(|times|json!({
                "clock":"shared process-local monotonic milliseconds", "draw_index":frames,
                "stage_names":["loop_start","audio_start","audio_end","step_start","step_end","graphics_pump_start","graphics_pump_end","record_end"],
                "stage_ms":times.map(stamp),"cpu_update_end_ms":stamp(render_start),
                "pump_timeline":window["pump_timeline"],"pacing_enabled":interactive_paced||benchmark_paced,
                "pacing_completed":false,"terminal_before_pacing":true,"pacing":Value::Null
            }))}else{None};
            if let (Some(b),Some(t))=(&mut benchmark,&loop_trace){b.timeline_last(frames-1,t.clone());}
            let e=window["flight"]["glasses"]["open"]==true;
            if (host.audio_workstation.computer_screen || (host.audio_workstation.eyewear.is_some() && !host.audio_workstation.midi_sender.enabled())) && e && !headset_e_down {return Ok("return_to_game");}
            headset_e_down=e;
            let escape=!bubble_diagnostic && window["keys"][1]==true;
            if !escape {escape_consumed=false;}
            if escape && !host.audio_workstation.computer_screen && host.audio_workstation.consumes_escape() {escape_consumed=true;}
            if window["closed"] == true || (!bubble_diagnostic && window["flight"]["quit"] == true) {
                return Ok("window_closed");
            }
            if escape && !escape_consumed {
                #[cfg(not(target_os="ios"))]
                if embedded && !host.audio_workstation.resident && !host.audio_workstation.computer_screen {
                    if let Err(error) = host.invoke("GraphicsConnectionSystem.Pump",json!({"entity":visual["entity"],"capture_on_exit":true})) {
                        eprintln!("Theatre monitor capture: {error}");
                    }
                }
                return Ok(if embedded {"return_to_game"}else{"window_closed"});
            }
            #[cfg(target_os="ios")]
            if embedded && take_return_to_game() {return Ok("return_to_game");}
            if benchmark.is_some() && window["drawn"] != true {
                nondrawable_frame=window.clone();
                return Ok("benchmark_nondrawable");
            }
            if controls_diagnostic && window["drawn"] != true {
                return Ok("control_diagnostic_nondrawable");
            }
            if benchmark.as_ref().is_some_and(|b| benchmark_started
                .is_some_and(|started| b.duration_complete(started.elapsed().as_secs_f64()))) {
                return Ok("benchmark_duration_complete");
            }
            if limit.is_some_and(|n| frames >= n) {
                return Ok("frame_limit");
            }
            if let Some(value) = window["aspect"]
                .as_f64()
                .filter(|a| a.is_finite() && (0.2..=8.).contains(a))
            {
                aspect = value;
            }
            if let (Some(w), Some(h)) = (
                window["logical_width"].as_f64(),
                window["logical_height"].as_f64(),
            ) {
                viewport = [w, h];
            }
            if let (Some(w), Some(h)) = (window["width"].as_f64(), window["height"].as_f64()) {
                framebuffer = [w, h];
            }
            if benchmark.is_none() && !bubble_diagnostic && !controls_diagnostic && capture_dt.is_none() {
                let transport=window["flight"]["workstation"]["all"]==true;
                editor_input=window["flight"]["workstation"]["editor"].clone();
                if host.audio_workstation.computer_screen {
                    let m=crate::workstation::computer_screen::map(&host.audio_workstation.frozen_view,host.theatre.computer_position().ok_or("missing theatre computer")?,viewport[0]/viewport[1]);
                    let state=&host.audio_workstation;
                    let captured=state.arrangement.is_interacting() || state.plugins.owns_pointer()
                        || state.plugin_browser.owns_pointer() || state.browser_selection.owns_pointer()
                        || state.bottom.controls.consumes_pointer() || state.spatial_mix.is_interacting()
                        || state.editor.status()["gesture_active"]==true;
                    crate::workstation::computer_screen::input_for_drag(&mut editor_input,m,viewport,captured);
                }
                // Native callback events already carry press-time modifiers.
                // Retain sampled fallback only for backends without that path.
                if editor_input.is_object() && editor_input.get("transport_toggle_timeline_pressed").is_none() {
                    editor_input["transport_toggle_pressed"]=json!(editor_input["transport_toggle_pressed"]==true || (transport&&!transport_held));
                }
                transport_held=transport;
                expand = window["flight"]["workstation"]["expand"] == true;
                (keys,overview)=host.audio_workstation.desktop_panel_navigation(&window,&mut editor_input);
            }
            let pacing_entry=timeline_enabled.then(Instant::now);
            if interactive_paced||benchmark_paced {cadence.pace(display_aware,&window,start.elapsed());}
            if let Some(t)=&mut loop_trace{
                t["pacing_entry_ms"]=json!(pacing_entry.map(stamp));
                t["pacing"]=if interactive_paced||benchmark_paced{cadence.last_pace.clone()}else{json!({"requested_sleep_ms":0.,"sleep_ms":Value::Null,"disabled_by":"existing finite BENCHMARK_PACED=0 override; FIFO unchanged"})};
                t["next_loop_eligible_ms"]=json!(stamp(Instant::now()));t["pacing_completed"]=json!(true);t["terminal_before_pacing"]=json!(false);
                if let Some(b)=&mut benchmark{b.timeline_last(frames-1,t.clone());}
            }
        }
    })();
    // Include final GPU completion in throughput and audio counter windows;
    // otherwise an overlapped last submission would be counted prematurely.
    let completion_started=Instant::now();
    let completion=if benchmark_started.is_some() {
        host.invoke("GraphicsConnectionSystem.Pump",json!({"entity":visual["entity"],"finish_only":true})).map(|_|())
    }else{Ok(())};
    let completion_ms=completion_started.elapsed().as_secs_f64()*1000.;
    let execution=execution.and_then(|reason|completion.map(|_|reason));
    // Stop before status/cleanup so startup and teardown are not amortized into FPS.
    let benchmark_wall_seconds = benchmark_started.map(|started| started.elapsed().as_secs_f64());
    if let Some(previous)=previous_measured_loop {loop_intervals.push(previous.elapsed().as_secs_f64()*1000.);}
    if (benchmark_audio || require_passthrough) && benchmark_started.is_some() {
        let end = benchmark_audio_counter_snapshot(host, run_started);
        benchmark_audio_report["measured_counter_window"] = json!({
            "version":1,"start":benchmark_audio_start,"end":end,
            "transport_underruns":benchmark_counter_delta(&benchmark_audio_start,&end,
                "/environment/transport/underruns","EnvironmentAudioSystem.Status.transport.underruns"),
            "vm_underrun_periods":benchmark_counter_delta(&benchmark_audio_start,&end,
                "/chuck/underrun_periods","ChuckConnectionSystem.Telemetry.underrun_periods"),
            "boundary_mode":"explicit_completed_gpu_boundaries",
            "scope":"Read-only lifetime counters after explicit completion of warmup GPU work/before the measured clock and after completion of the last measured submission/before cleanup. Final completion is included in measured elapsed time. No atomic reset; sequential counter reads slightly enclose that interval."
        });
    }
    if playback_fixture_started {
        let status=host.invoke("EnvironmentAudioSystem.Status",Value::Null)?;
        playback_fixture_final=json!({"playing":status["transport"]["playing"],"position_frame":status["transport"]["position_frame"],"sample_rate":status["transport"]["sample_rate"]});
        host.invoke("EnvironmentAudioSystem.Transport",json!({"action":"pause"}))?;
    }
    if let Some(benchmark) = &mut benchmark {benchmark.measured_duration(benchmark_wall_seconds);}
    if let Some(comparison)=&motion_comparison{
        std::fs::write(host.output_root().join("motion-comparison.json"),serde_json::to_vec_pretty(&comparison.report()).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    }
    let benchmark_report = benchmark.as_ref().map(|b| {
        let mut report = b.report(execution.as_ref().copied().unwrap_or("error"));
        report["gpu_draw_offset"]=json!(gpu_draw_offset);
        report["nondrawable_frame"]=nondrawable_frame.clone();
        if let Some(comparison)=&motion_comparison{
            report["scenario"]=json!("finite-original-lighting-motion-comparison-v1");
            report["fixed_simulation_step_seconds"]=Value::Null;
            report["motion_comparison"]=comparison.report();
        }
        if let Some(replay)=&motion_input{report["motion_input"]=replay.report();report["scenario"]=json!("input-driven-stored-pose-candidate");report["fixed_simulation_step_seconds"]=if replay.gate{json!(1./30.)}else{Value::Null};}
        if fader_replay {report["scenario"]=json!("mixer-fader-drag");report["applied_fader_values"]=json!(fader_values);report["workload_limits"]=json!("Pointer down, continuous gain changes and release on a mixer track.");}
        report["combined_transport_playback"]=json!({"requested":playback_fixture,"started":playback_fixture_started,"final_before_pause":playback_fixture_final,"scope":"Existing game audio session and normal Transport play/pause; empty timeline, no synthesized sound, plugin/source/recording stress or persistent settings change."});
        if playback_fixture {report["audio_playback_requested"]=json!(true);}
        report["stage_lighting_replay"]=json!({"enabled":stage_lighting_replay,"samples":stage_lighting_evidence});
        report["capture_validation"]=json!(capture_validation);
        report["bubble_fixture"]=json!({"enabled":benchmark_bubbles,"notes":if benchmark_bubbles{4}else{0},"scope":"Finite process-local MIDI document and ordinary piano-roll rendering; no playback or persistent project edits."});
        report["wearable_verification"]=json!(wearable_evidence);
        report["camera_replay"]=json!(camera_replay);
        report["camera_day_night_fraction"]=json!(benchmark_camera_fraction);
        report["camera_clock_replay"]=json!(benchmark_camera_animate);
        report["track_fixture"]=json!({"enabled":track_fixture,"created_id":track_fixture_id,"scope":"One empty unarmed audio track through the normal document/track-I/O path; no clip, source, recording or playback stress fixture."});
        report["final_gpu_completion_ms"]=json!(completion_ms);
        let mut intervals=loop_intervals.clone();intervals.sort_by(f64::total_cmp);
        let percentile=|p:f64|intervals.get((intervals.len().saturating_sub(1) as f64*p).round() as usize).copied();
        report["complete_loop_timing"]=json!({"samples":intervals.len(),"p50_ms":percentile(0.5),"p95_ms":percentile(0.95),"p99_ms":percentile(0.99),"worst_ms":intervals.last(),
            "scope":"Consecutive measured loop starts, including pacing and reporting; last interval includes final GPU completion. Startup/teardown excluded."});
        report["gpu_completion_boundary"] = json!("warmup drained before clock; final drain included in measured elapsed");
        report["measured_loop_wall_seconds"] = json!(benchmark_wall_seconds);
        report["measured_loop_frames_per_second"] = json!(benchmark_wall_seconds
            .filter(|seconds| *seconds > 0.)
            .map(|seconds| frames.saturating_sub(b.warmup) as f64 / seconds));
        report
    });
    #[cfg(target_os="ios")]
    if embedded {unsafe {ww_ios_workstation_focus(false);}}
    let controls_settled=controls_diagnostic.then(||host.audio_workstation.motion.settled());
    let controls_mixer=previous_mixer.map(|previous|host.audio_workstation.plugins.set_mixer(previous));
    let embedded_evidence=require_passthrough.then(||json!({"embedded":host.audio_workstation.embedded,
        "computer_screen":host.audio_workstation.computer_screen,"eyewear":host.audio_workstation.eyewear,
        "observer_motion":observer_motion,"audio_rays":host.audio_workstation.audio_rays,
        "passthrough":host.audio_workstation.settings.passthrough(),"validated_frames":passthrough_frames,"visual":visual,
        "security_camera":host.audio_workstation.camera_view,"theatre_vertex_count":host.theatre.venue.as_ref().map(|v|v.vertices.len()),
        "scene_scope":"Shared live player scene at riverbank, mountain or theatre camera pose; normal native world passes and DAW overlay.",
        "audio_source":"Existing game EnvironmentAudioSystem session; no standalone MIDI stress fixture"}));
    let final_status = host.invoke("AudioWorkstationSystem.Status", Value::Null);
    if benchmark_audio && own_audio {
        benchmark_audio_report["final_status"]=host.invoke("EnvironmentAudioSystem.Status",Value::Null)
            .unwrap_or_else(|error|json!({"error":error}));
    }
    let audio_cleanup=if own_audio {host.invoke("EnvironmentAudioSystem.Close",Value::Null)} else {Ok(Value::Null)};
    if benchmark_audio {
        benchmark_audio_report["cleanup"]=audio_cleanup.as_ref().cloned()
            .unwrap_or_else(|error|json!({"error":error}));
    }
    let navigation_cleanup=if host.audio_workstation.eyewear.as_deref()==Some("vr"){host.invoke("PlanetNavigationSystem.Pause",Value::Null)}else{Ok(Value::Null)};
    let cleanup = host.invoke(if embedded {"AudioWorkstationSystem.Suspend"}else{"AudioWorkstationSystem.Close"}, Value::Null).and(audio_cleanup).and(navigation_cleanup);
    match (execution, final_status, cleanup) {
        (Ok(reason), Ok(status), Ok(_)) => Ok(
            json!({"mode":if controls_diagnostic {"workstation_controls_diagnostic"}else if input["diagnostic_spheres"]==true {"audio_sphere_diagnostic"}else if input["diagnostic_bubbles"]==true {"bubble_piano_roll_diagnostic"}else{"workstation"},"frames":frames,"end_reason":reason,
            "piano_roll":status["piano_roll"],"editor_error":status["editor_error"],
            "spatial_mix":status["spatial_mix"],"spatial_error":status["spatial_error"],
            "arrangement":status["arrangement"],"arrangement_error":status["arrangement_error"],
            "browser_enabled":status["browser_enabled"],"startup_focus":startup_focus,
            "panels":status["panels"],"entities_remaining":host.entities.len(),"capture_dt":capture_dt,
            "capture_panel_replay":if capture_panel_replay{json!(capture_panel_frames)}else{Value::Null},
            "controls_mixer":controls_mixer,"controls_settled":controls_settled,
            "benchmark":benchmark_report,"embedded_evidence":embedded_evidence,"benchmark_audio":benchmark_audio_report,"cadence":cadence.info(display_aware,benchmark_paced)}),
        ),
        (execution, status, cleanup) => Err(format!(
            "workstation: {:?}; status: {:?}; cleanup: {:?}",
            execution.err(),
            status.err(),
            cleanup.err()
        )),
    }
}

fn benchmark_audio_counter_snapshot(host: &mut Host, run_started: Instant) -> Value {
    let before = run_started.elapsed().as_secs_f64();
    let environment = host.invoke("EnvironmentAudioSystem.Status",Value::Null)
        .unwrap_or_else(|error|json!({"error":error}));
    let chuck = host.invoke("ChuckConnectionSystem.Telemetry",Value::Null)
        .unwrap_or_else(|error|json!({"error":error}));
    json!({"environment":environment,"chuck":chuck,
        "snapshot_begin_seconds_since_run_start":before,
        "snapshot_end_seconds_since_run_start":run_started.elapsed().as_secs_f64()})
}
fn benchmark_counter_delta(start: &Value, end: &Value, path: &str, source: &str) -> Value {
    let before = start.pointer(path).and_then(Value::as_u64);
    let after = end.pointer(path).and_then(Value::as_u64);
    let delta = before.zip(after).and_then(|(before,after)|after.checked_sub(before));
    json!({"source":source,"snapshot_json_pointer":path,
        "unit":"output render blocks containing at least one missing queued frame",
        "start_lifetime_total":before,
        "end_lifetime_total":after,"delta":delta,"complete":delta.is_some(),
        "reason":if before.is_none() || after.is_none() {Some("counter unavailable at a boundary")}
            else if delta.is_none() {Some("counter decreased or reset")} else {None}})
}

#[cfg(test)]
mod benchmark_counter_window_tests {
    use super::benchmark_counter_delta;
    use serde_json::json;
    #[test]
    fn lifetime_totals_are_preserved_while_warmup_is_subtracted() {
        let result=benchmark_counter_delta(&json!({"x":17}),&json!({"x":23}),"/x","fixture");
        assert_eq!(result["delta"],6);
        assert_eq!(result["start_lifetime_total"],17);
        assert_eq!(result["end_lifetime_total"],23);
    }
    #[test]
    fn missing_or_decreasing_counts_do_not_claim_a_zero_delta() {
        assert_eq!(benchmark_counter_delta(&json!({}),&json!({"x":0}),"/x","fixture")["complete"],false);
        assert_eq!(benchmark_counter_delta(&json!({"x":5}),&json!({"x":4}),"/x","fixture")["complete"],false);
    }
}

fn mobile_canvas_scale() -> f64 {
    #[cfg(target_os = "ios")]
    {
        1. // full-screen pages; pinch belongs to the editor, not panel overview
    }
    #[cfg(not(target_os = "ios"))]
    {
        1.
    }
}

#[cfg(test)]mod range_escape_tests{
 use super::*;
 #[test]fn completed_hidden_range_does_not_consume_spatial_view_exit(){
  let mut s=State::default();s.arrangement.sync(&json!({"revision":1,"tracks":[{"id":"m","name":"MIDI","kind":"midi","gain":1.,"mute":false,"solo":false}],"clips":[]})).unwrap();
  let rect=crate::workstation::piano_roll::Rect{left:0.,top:0.,width:985.,height:288.};
  for (x,left) in [(228.,true),(428.,false)]{s.arrangement.update(&crate::workstation::piano_roll::Input{pointer:Some([x,crate::workstation::arrangement::RULER_HEIGHT+32.]),left,..Default::default()},rect);}
  assert!(s.consumes_escape());s.spatial_view=true;assert!(!s.consumes_escape());s.spatial_view=false;assert!(s.consumes_escape());
  s.arrangement.update(&crate::workstation::piano_roll::Input{escape:true,..Default::default()},rect);assert!(!s.consumes_escape());
 }
}

#[cfg(test)]mod audio_startup_failure_tests{
 use super::*;
 #[test]fn failed_audio_start_releases_partial_resources_and_keeps_visual_session(){
  let mut cleaned=0;let outcome=finish_optional_audio_start(Err("Too many servers already active".into()),||{cleaned+=1;Ok(Value::Null)}).unwrap();
  assert_eq!(cleaned,1);assert!(!outcome.0);assert_eq!(outcome.1.as_deref(),Some("Too many servers already active"));
  let mut state=State::default();state.visual=Some(json!({"entity":99}));state.audio_startup_error=outcome.1;
  assert_eq!(state.status()["open"],true);assert!(state.status()["audio_startup_error"].as_str().unwrap().contains("Too many"));
  assert!(!state.consumes_escape());
 }
 #[test]fn successful_audio_start_preserves_ownership_without_cleanup(){
  let outcome=finish_optional_audio_start(Ok(json!({"native_output_attached":true})),||panic!("successful startup must not be closed")).unwrap();assert!(outcome.0);assert!(outcome.1.is_none());
 }
 #[test]fn failed_audio_cleanup_is_not_silently_treated_as_offline(){
  let error=finish_optional_audio_start(Err("start failed".into()),||Err("callback still owned".into())).unwrap_err();assert!(error.contains("start failed"));assert!(error.contains("callback still owned"));
 }
}

#[cfg(test)]mod transport_routing_tests {
 use super::*;
 #[test]fn hit_positions_match_authored_transport_pose_across_layouts(){
  for (width,height) in [(960.,600.),(1280.,720.),(1920.,1200.)] {for scale in [1.,0.5]{for swap in [false,true]{
   let mut state=State::default();state.motion.set_all_open(true);state.motion.advance(1.);state.canvas_scale=scale;state.swap_horizontal=swap;
   let view=Viewport{width,height,canvas_scale:scale};let options=state.origami_control.mode.options(false,swap);
   let surface=crate::workstation::panel_layout::resolved_surface(view,&state.motion.snapshot(),options,0).unwrap();
   let scale=(surface.size[0]*0.88/0.205).min(surface.size[1]*0.80/0.04);let mpp=1.3/height;
   for i in 0..4 {let p=[width*0.5+(surface.origin[0]+(i as f64-1.5)*0.055*scale)/mpp,height*0.5-surface.origin[1]/mpp];assert_eq!(transport_hit(&state,[width,height],p),Some(i));}
  }}}
 }
}

#[cfg(test)] mod wearable_daw_tests {
 use super::*;
 #[test] fn ar_always_uses_frozen_view_while_vr_tracks_the_player_and_allows_other_backdrops(){
  let frozen=json!({"position":[1.,2.,3.],"forward":[0.,0.,1.]});let live=json!({"position":[8.,9.,10.],"forward":[1.,0.,0.]});
  for passthrough in [false,true]{for cameras in [false,true]{let ar=wearable_player_view(Some("ar"),&frozen,&live,passthrough,cameras,2.).unwrap();assert_eq!(ar["position"],frozen["position"]);assert_eq!(ar["forward"],frozen["forward"]);assert_eq!(ar["aspect"],2.);}}
  assert_eq!(wearable_player_view(Some("vr"),&frozen,&live,true,false,1.6).unwrap()["position"],live["position"]);
  assert!(wearable_player_view(Some("vr"),&frozen,&live,false,false,1.6).is_none());
  assert!(wearable_player_view(Some("vr"),&frozen,&live,true,true,1.6).is_none());
  assert!(wearable_player_view(None,&frozen,&live,true,false,1.6).is_none());
 }
}
