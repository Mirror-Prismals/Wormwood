//! Continuous environment sound lifetime and independently scheduled audio preparation.
use crate::{
    audio::{
        geometry::Geometry,
        line_array::{Cabinet, Routes},
        transport::Producer,
        vector,
    },
    host::Host,
};
use serde_json::{Value, json};
use std::collections::VecDeque;
#[path = "EnvironmentAudioWorker.rs"]
mod preparation;
#[path = "EnvironmentAmbience.rs"]
pub(crate) mod ambience;

/// Only document operations cross into the non-real-time preparation worker.
/// Both implementations retain the named WorkstationDataSystem boundary.
trait DocumentAccess {
    fn invoke(&mut self, target: &str, input: Value) -> Result<Value, String>;
    fn document_revision(&mut self) -> Result<u64, String> {
        self.invoke("WorkstationDataSystem.Snapshot", Value::Null)?["revision"].as_u64().ok_or_else(||"workstation revision required".into())
    }
}
impl DocumentAccess for Host {
    fn invoke(&mut self, target: &str, input: Value) -> Result<Value, String> { Host::invoke(self, target, input) }
}
impl DocumentAccess for crate::host::AudioPreparationTransaction<'_> {
    fn invoke(&mut self, target: &str, input: Value) -> Result<Value, String> { self.invoke(target, input) }
    fn document_revision(&mut self) -> Result<u64, String> { self.document_revision() }
}

const VISUALIZATION_PACKETS: usize = 128;
#[derive(Default)]
pub(crate) struct State {
    // The outer application owner holds this proxy; its inner State never does.
    worker: Option<preparation::Worker>,
    preparation_active: bool,
    preparation_ready: bool,
    preparation_block_frames: usize,
    preparation_ticks: u64,
    preparation_max_us: u64,
    preparation_qos: Option<i32>,
    ambience: ambience::State,
    event: Option<Value>,
    sound: Option<Value>,
    rate: u64,
    time: f64,
    source_count_max: usize,
    report: Value,
    geometry: Option<std::sync::Arc<Value>>,
    geometry_worker: Option<crate::base_system::layer_two::terrain_geometry_system::Worker>,
    geometry_batch_active: bool,
    next_geometry_capture: f64,
    microphone_geometry: Option<std::sync::Arc<Value>>,
    microphone_geometry_worker: Option<crate::base_system::layer_two::terrain_geometry_system::Worker>,
    microphone_batch_active: bool,
    next_microphone_geometry_capture: f64,
    transport: Option<Producer>,
    browser_preview:crate::workstation::audio_preview::Preview,
    capture: Option<crate::audio::input_capture::Reader>,
    capture_pending: VecDeque<crate::audio::input_capture::Frame>,
    capture_blocks: VecDeque<Value>,
    capture_observer_dropped: u64,
    capture_discarded_frames: u64,
    physical_input_ports: Vec<String>,
    virtual_mic_available: bool,
    recording: bool,
    retrospective: crate::workstation::retrospective_runtime::Service,
    retrospective_error: Option<String>,
    midi_pending: VecDeque<Value>,
    keyboard_mic: crate::workstation::midi_keyboard::MicMeter,
    cabinets: Vec<Cabinet>,
    // Exact geometry/listener cache: orientation changes still recompute pan,
    // but cannot change a segment's visibility.
    rig_visibility: Option<(u64, [u64;3], Vec<Option<crate::audio::geometry::Hit>>)>,
    render_revision: Option<u64>,
    render_calls_last_pump:usize,
    // Worker-produced metadata only; never retain PCM or rerun an instrument
    // to observe audio that the native callback has already consumed.
    visualizations: VecDeque<(u64, Value)>,
    audio_complete: bool,
    instrument_connected: bool,
    playback_error: Option<String>,
    theatre_audio: Value,
}

fn current_visualization(
    generation: u64,
    observation: &Value,
    transport: &Value,
    revision: Option<u64>,
    rate: u64,
) -> bool {
    let Some(revision) = revision else { return false; };
    let Some(start) = transport["generation_start_frame"].as_u64() else { return false; };
    let Some(first) = observation["start_frame"].as_u64() else { return false; };
    let Some(end) = observation["end_frame"].as_u64() else { return false; };
    transport["generation"].as_u64() == Some(generation)
        && transport["sample_rate"].as_u64() == Some(rate)
        && observation["revision"].as_u64() == Some(revision)
        && observation["sample_rate"].as_u64() == Some(rate)
        && observation["sequence"].as_u64().is_some_and(|sequence| sequence > 0)
        && (observation["transport_end_sequence"].is_u64() || first >= start) && end > first
}

fn consumed_position(transport: &Value) -> Option<u64> {
    let generation = transport["generation"].as_u64()?;
    (transport["played_generation"].as_u64() == Some(generation))
        .then(|| transport["position_frame"].as_u64()).flatten()
}

fn selected_visualization<'a>(
    packets: &'a VecDeque<(u64, Value)>,
    transport: &Value,
    revision: Option<u64>,
    rate: u64,
) -> Option<&'a Value> {
    if transport["playing"] != true { return None; }
    let consumed = consumed_position(transport)?;
    let consumed_sequence=transport["consumed_sequence"].as_u64();
    packets.iter().filter(|(generation, value)| {
        current_visualization(*generation, value, transport, revision, rate)
            && if let Some(end)=value["transport_end_sequence"].as_u64() {
                consumed_sequence.is_some_and(|sequence|end<=sequence)
            }else{value["end_frame"].as_u64().is_some_and(|end| end<=consumed)}
    }).max_by_key(|(_, value)| (
        value["transport_end_sequence"].as_u64().or_else(||value["end_frame"].as_u64()).unwrap_or(0),
        value["sequence"].as_u64().unwrap_or(0),
    )).map(|(_, value)| value)
}

fn prune_visualizations(
    packets: &mut VecDeque<(u64, Value)>,
    transport: &Value,
    revision: Option<u64>,
    rate: u64,
) {
    packets.retain(|(generation, value)|
        current_visualization(*generation, value, transport, revision, rate));
    let consumed = consumed_position(transport).map(|position|transport["consumed_sequence"].as_u64().unwrap_or(position));
    let latest = consumed.and_then(|position| packets.iter().enumerate()
        .filter(|(_, (_, value))| value["transport_end_sequence"].as_u64().or_else(||value["end_frame"].as_u64()).is_some_and(|end| end <= position))
        .max_by_key(|(_, (_, value))| (
            value["transport_end_sequence"].as_u64().or_else(||value["end_frame"].as_u64()).unwrap_or(0), value["sequence"].as_u64().unwrap_or(0)))
        .map(|(index, _)| index));
    if let (Some(position), Some(latest)) = (consumed, latest) {
        let mut index = 0;
        packets.retain(|(_, value)| {
            let keep = index == latest || value["transport_end_sequence"].as_u64().or_else(||value["end_frame"].as_u64()).is_some_and(|end| end > position);
            index += 1;
            keep
        });
    }
    while packets.len() > VISUALIZATION_PACKETS {
        // Preserve the latest completed packet even if a slow callback allows
        // many tiny future blocks to queue. Evict the oldest future packet.
        let completed = consumed.and_then(|position| packets.iter().position(|(_, value)|
            value["transport_end_sequence"].as_u64().or_else(||value["end_frame"].as_u64()).is_some_and(|end| end <= position)));
        packets.remove(if completed == Some(0) { 1 } else { 0 });
    }
}
fn backend() -> &'static str {
    if cfg!(target_os = "ios") {
        "IOSAudioConnectionSystem"
    } else {
        "JackConnectionSystem"
    }
}
impl State {
    #[cfg(test)]pub(crate) fn test_prepare_transport(&mut self,rate:u32,event:Value)->crate::audio::transport::Output {
        let(mut producer,output)=crate::audio::transport::prepare(rate).unwrap();producer.set_world(false);
        self.transport=Some(producer);self.rate=u64::from(rate);self.event=Some(event);output
    }
    #[cfg(test)]pub(crate) fn test_attach_capture(&mut self)->crate::audio::input_capture::Writer {
        let(writer,reader)=crate::audio::input_capture::prepare(self.rate as u32).unwrap();
        self.capture=Some(reader);writer
    }
    #[cfg(test)]pub(crate) fn test_enqueue_buses(&mut self,frames:&[[f32;4]]) {
        self.transport.as_mut().unwrap().enqueue_wrapped(frames).unwrap();
    }
    pub fn playback_status(&self) -> Value {
        if let Some(worker) = &self.worker {
            return worker.state.lock().map_or_else(|_|json!({"error":"audio preparation state poisoned"}), |state|state.playback_status());
        }
        let mut value = self
            .transport
            .as_ref()
            .map_or(Value::Null, Producer::snapshot);
        if value.is_object() {
            let visualization = selected_visualization(
                &self.visualizations, &value, self.render_revision, self.rate,
            ).cloned().unwrap_or(Value::Null);
            value["render_calls_last_pump"]=json!(self.render_calls_last_pump);
            value["preparation_worker"]=json!(self.preparation_active);
            value["preparation_ready"]=json!(self.preparation_ready);
            value["preparation_ticks"]=json!(self.preparation_ticks);
            value["preparation_max_us"]=json!(self.preparation_max_us);
            value["preparation_scheduling"]=crate::audio::worker_scheduling::status(self.preparation_qos.unwrap_or(crate::audio::worker_scheduling::NOT_REQUESTED));
            value["processing_quantum_frames"]=json!(self.render_quantum());
            value["audio_complete"] = json!(self.audio_complete);
            value["recording"] = json!(self.recording);
            value["capture_dropped_frames"] = json!(self.capture.as_ref().map_or(0, |r| r.dropped_frames()));
            value["capture_recovery_discarded_frames"]=json!(self.capture_discarded_frames);
            value["midi_instrument_connected"] = json!(self.instrument_connected);
            value["error"] = json!(self.playback_error);
            value["visualization"] = visualization;
        }
        value
    }
    pub fn theatre_audio(&self) -> Value {
        if let Some(worker) = &self.worker { return worker.state.lock().map_or(Value::Null, |state|state.theatre_audio.clone()); }
        self.theatre_audio.clone()
    }
    fn render_quantum(&self) -> usize { if self.preparation_block_frames == 0 { 4096 } else { self.preparation_block_frames } }
    pub(crate) fn stop_preparation_worker(&mut self) { if let Some(worker) = self.worker.as_mut() { worker.stop(); } }
    fn start_preparation_worker(&mut self, context: crate::host::AudioPreparationContext) -> Result<(), String> {
        if self.worker.is_some() { return Err("audio preparation worker already running".into()); }
        let state = std::mem::take(self);
        match preparation::Worker::start(state, context) {
            Ok(worker) => { self.worker = Some(worker); Ok(()) },
            Err((state, error)) => { *self = state; Err(error) }
        }
    }
    /// Start and acknowledge preparation before activating the native callback.
    /// The handshake runs after all state/document guards have been released.
    /// Failed activation recovers the joined inner State for normal Host cleanup.
    fn prepare_before_native_start(&mut self, context: crate::host::AudioPreparationContext,
        activate: impl FnOnce(&State)->Result<(),String>) -> Result<(),String> {
        self.start_preparation_worker(context)?;
        if let Err(error) = activate(self) {
            if let Some(mut worker) = self.worker.take() { *self = worker.take_state(); }
            return Err(error);
        }
        Ok(())
    }
    fn pump(&mut self, host: &mut impl DocumentAccess, force: bool) -> Result<(), String> {
        let result = self.pump_inner(host, force);
        if let Err(error) = &result {
            if let Some(transport) = self.transport.as_mut() {
                transport.set_playing(false);
                transport.set_recording(false);
            }
            self.recording=false;
            self.playback_error = Some(error.clone());
        }
        result
    }
    fn pump_inner(&mut self, host: &mut impl DocumentAccess, force: bool) -> Result<(), String> {
        self.render_calls_last_pump=0;
        if let Some(transport)=self.transport.as_mut(){self.browser_preview.pump(transport);}
        if self.capture.is_some() { return self.pump_capture(host,true); }
        let quantum = self.render_quantum();
        let Some(transport) = self.transport.as_mut() else {
            return Ok(());
        };
        if !force && !transport.playing() {
            return Ok(());
        }
        let revision = host.document_revision()?;
        if self.render_revision.is_some_and(|old| old != revision) {
            transport.seek(transport.position())?;
        }
        self.render_revision = Some(revision);
        // Each preparation pass is bounded to two native-sized blocks. The
        // callback consumes only prepared four-bus PCM and cabinet DSP.
        let mut budget=2 * quantum;
        // At most two full document renders, including one-sample loops.
        // RenderRange maps each output frame onto the loop inside that block.
        for _render_call in 0..2 {
            if budget==0{break;}
            let frames = transport.batch_needed().min(budget).min(quantum);
            if frames == 0 {
                break;
            }
            let mut request=json!({"document_revision":revision,"sample_rate":self.rate,"start_frame":transport.next_frame(),"frames":frames,"instrument":"cardinal_sine","publish_visualization":true});
            request.as_object_mut().unwrap().extend(transport.render_clock().as_object().unwrap().clone());
            self.render_calls_last_pump+=1;
            let rendered=host.invoke("WorkstationDataSystem.RenderRange",request)?;
            self.audio_complete = rendered["audio_complete"] == true;
            self.instrument_connected = rendered["midi_instrument_connected"] == true;
            let rows = rendered["matrix_pcm"]
                .as_array()
                .filter(|r| r.len() == frames)
                .ok_or("four-bus render frame count mismatch")?;
            let mut pcm = Vec::with_capacity(frames);
            for row in rows {
                let frame: [f32; 4] = serde_json::from_value(row.clone())
                    .map_err(|_| "four-bus render frame required")?;
                pcm.push(frame);
            }
            let monitor:Vec<[f32;2]>=serde_json::from_value(rendered["master_stereo"].clone()).map_err(|_|"monitor render requires stereo PCM")?;
            transport.enqueue_outputs(&pcm,Some(&monitor),true,true)?;
            budget-=frames;
            let status = transport.snapshot();
            let generation = status["generation"].as_u64().ok_or("transport generation required")?;
            let mut observation=rendered["visualization"].clone();
            if observation.is_object() {observation["transport_end_sequence"]=status["queued_sequence"].clone();}
            self.visualizations.push_back((generation, observation));
            prune_visualizations(&mut self.visualizations, &status, self.render_revision, self.rate);
        }
        prune_visualizations(&mut self.visualizations, &transport.snapshot(), self.render_revision, self.rate);
        Ok(())
    }
    /// The callback is the sole input producer. The preparation worker drains bounded
    /// blocks, records their consumed timeline, then renders the next output.
    /// No document, JSON, allocation, plugin host, or file write enters JACK.
    fn pump_capture(&mut self, host:&mut impl DocumentAccess, render:bool)->Result<(),String> {
        let quantum = self.render_quantum();
        let Some(transport)=self.transport.as_mut() else{return Ok(());};
        let reader=self.capture.as_mut().ok_or("native capture reader missing")?;
        if reader.sample_rate() as u64!=self.rate{return Err("capture rate differs from output rate".into());}
        self.capture_pending.extend(reader.drain(8192usize.saturating_sub(self.capture_pending.len())));
        transport.set_monitoring(true);
        transport.catch_up();
        for _ in 0..2 {
            let Some(first)=self.capture_pending.front().copied() else{break;};
            // A control transition explicitly resets the capture clock. Samples
            // from its preceding callback generation cannot enter the new take.
            if first.generation!=transport.generation() {
                while self.capture_pending.front().is_some_and(|f|f.generation!=transport.generation()){self.capture_pending.pop_front();self.capture_discarded_frames+=1;}
                continue;
            }
            let capture_clock=crate::workstation::capture_timeline::CaptureClock::new(&transport.snapshot(),first.playback_position,quantum,first.playing,first.generation,first.playback_sequence)?;
            let count=self.capture_pending.iter().take(if render {quantum.min(transport.capacity_frames())}else{quantum}).enumerate().take_while(|(i,f)|
                f.generation==first.generation && f.playing==first.playing && f.recording==first.recording &&
                f.physical_count==first.physical_count && f.virtual_mic_available==first.virtual_mic_available &&
                f.device_frame==first.device_frame+*i as u64 &&
                f.playback_position==capture_clock.at(*i)
            ).count();
            if count==0 {break;}
            let frames:Vec<_>=self.capture_pending.iter().take(count).copied().collect();
            let physical:Vec<Vec<f32>>=(0..first.physical_count as usize).map(|ch|frames.iter().map(|f|f.physical[ch]).collect()).collect();
            self.keyboard_mic.update(physical.first().map(Vec::as_slice),self.rate as u32);
            self.virtual_mic_available=first.virtual_mic_available;
            let virtual_mic=first.virtual_mic_available.then(||frames.iter().map(|f|f.virtual_mic).collect::<Vec<_>>());
            let live=json!({"physical":physical,"virtual_mic":virtual_mic});
            let capture_end=first.device_frame+count as u64;
            let midi:Vec<Value>=self.midi_pending.iter().take_while(|e|e["capture_frame"].as_u64().unwrap_or(0)<capture_end)
                .map(|event|{let mut event=event.clone();if event["capture_frame"].as_u64().unwrap_or(0)<first.device_frame{event["capture_frame"]=json!(first.device_frame);}event}).collect();
            let revision=host.document_revision()?;
            let mut capture=json!({"document_revision":revision,"sample_rate":self.rate,
                "capture_start_frame":first.device_frame,"timeline_start_frame":first.playback_position,"frames":count,
                "playing":first.playing,"recording":first.recording,"live_inputs":live,
                "player_audio":[frames.iter().map(|f|f.player_audio[0]).collect::<Vec<_>>(),frames.iter().map(|f|f.player_audio[1]).collect::<Vec<_>>()],
                "midi_events":midi});
            capture_clock.annotate(&mut capture);
            let accepted=host.invoke("WorkstationDataSystem.CaptureInput",capture.clone())?;
            self.midi_pending.drain(..midi.len());
            self.capture_pending.drain(..count);
            capture["capture_status"]=accepted.clone();
            // The bottom history sees the same accepted native block once,
            // even while the DAW/UI is closed. It is not another recorder.
            self.retrospective_error=self.retrospective.observe(&capture).err();
            if self.capture_blocks.len()==8 {self.capture_blocks.pop_front();self.capture_observer_dropped+=1;}
            self.capture_blocks.push_back(capture);
            if !render {continue;}
            let revision=accepted["revision"].as_u64().ok_or("capture revision required")?;
            self.render_revision=Some(revision);
            let mut request=json!({"document_revision":revision,"sample_rate":self.rate,
                "start_frame":transport.next_frame(),"frames":count,"instrument":"cardinal_sine","publish_visualization":true,
                "monitor_only":!first.playing,"capture_start_frame":first.device_frame,"live_inputs":live});
            let mut render_clock=transport.render_clock();
            if !first.playing{render_clock.as_object_mut().unwrap().remove("loop_range");}
            request.as_object_mut().unwrap().extend(render_clock.as_object().unwrap().clone());
            self.render_calls_last_pump+=1;
            let rendered=host.invoke("WorkstationDataSystem.RenderRange",request)?;
            let pcm:Vec<[f32;4]>=serde_json::from_value(rendered["matrix_pcm"].clone()).map_err(|_|"capture render needs four-bus PCM")?;
            if pcm.len()!=count{return Err("capture output frame count mismatch".into());}
            let monitor:Vec<[f32;2]>=serde_json::from_value(rendered["master_stereo"].clone()).map_err(|_|"capture monitor needs stereo PCM")?;
            transport.enqueue_outputs(&pcm,Some(&monitor),true,first.playing)?;
            self.audio_complete=rendered["audio_complete"]==true;
            self.instrument_connected=rendered["midi_instrument_connected"]==true;
            let status=transport.snapshot();
            let mut observation=rendered["visualization"].clone();
            if observation.is_object(){observation["transport_end_sequence"]=status["queued_sequence"].clone();}
            self.visualizations.push_back((transport.generation(),observation));
            prune_visualizations(&mut self.visualizations,&status,self.render_revision,self.rate);
        }
        Ok(())
    }
    fn capture_transition(&mut self,host:&mut Host,playing:bool,recording:bool,position:u64)->Result<(),String>{
        if self.capture.is_none(){return Ok(());}
        let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
        let clock=self.capture.as_ref().map_or(0,|r|r.next_device_frame());
        let mut request=json!({"document_revision":snapshot["revision"],"sample_rate":self.rate,
            "capture_start_frame":clock,"timeline_start_frame":position,"frames":0,"playing":playing,"recording":recording,"live_inputs":null});
        let status=self.transport.as_ref().ok_or("capture transport missing")?.snapshot();
        if playing&&status["loop_enabled"]==true{request["loop_start_frame"]=status["loop_start_frame"].clone();request["loop_end_frame"]=status["loop_end_frame"].clone();}
        host.invoke("WorkstationDataSystem.CaptureInput",request)?;
        if !playing{self.midi_pending.retain(|e|e["source"]=="preview");self.retrospective.buffer.release_keyboard(clock);}
        self.recording=recording;
        let transport=self.transport.as_mut().ok_or("transport missing during capture transition")?;
        transport.set_recording(recording);
        transport.set_playing(playing);
        transport.seek(position)?;
        self.render_revision=None;
        Ok(())
    }
    fn control_capture(&mut self,host:&mut Host,action:&str,start:Option<u64>)->Result<Value,String>{
        if self.recording && matches!(action,"play"|"record"){return Ok(self.playback_status());}
        // Stop must remain usable after an overflow or rejected capture
        // block so pending audio can be committed without a retry loop.
        if matches!(action,"pause"|"stop") && self.playback_error.is_some() {
            self.capture_discarded_frames+=self.capture_pending.len() as u64;
            self.capture_pending.clear();
            if let Some(reader)=self.capture.as_mut(){self.capture_discarded_frames+=reader.drain(32768).len() as u64;}
        } else {for _ in 0..4 {self.pump_capture(host,false)?;}}
        let transport=self.transport.as_ref().ok_or("workstation transport missing")?;
        let position=start.unwrap_or(transport.position());
        let (playing,recording,position)=match action {
            "record"=>(true,true,position),"play"=>(true,false,position),
            "pause"=>(false,false,position),"stop"=>(false,false,transport.timeline_zero()),
            // A recording seek closes the current take before moving.
            "seek"=>(transport.playing()&&!self.recording,false,position),_=>unreachable!()};
        if self.recording && !recording {self.capture_transition(host,false,false,transport.position())?;}
        self.capture_transition(host,playing,recording,position)?;
        self.playback_error=None;
        Ok(self.playback_status())
    }
    fn finish_capture_close(&mut self, host:&mut Host)->Result<(),String> {
        if self.capture.is_none(){return Ok(());}
        let mut first=None;
        // The native producer has been detached. Bound shutdown by queued input,
        // not six render quanta: small device periods can leave a full ring.
        let queued=|state:&Self|state.capture_pending.len()+state.capture.as_ref().map_or(0,|r|r.available());
        let drain_limit=queued(self).saturating_add(1);
        for _ in 0..drain_limit {
            let before=queued(self);
            if let Err(error)=self.pump_capture(host,false){first=Some(error);break;}
            let after=queued(self);
            if after==0 {break;}
            if after>=before {first=Some("capture shutdown made no input progress".into());break;}
        }
        // Commit already accepted audio even if a later block was rejected.
        // Rejected input and the document remain owned for an explicit retry.
        let position=self.transport.as_ref().map_or(0,Producer::position);
        if let Err(error)=self.capture_transition(host,false,false,position){
            first=Some(first.map_or(error.clone(),|old|format!("{old}; {error}")));
        }
        if let Some(error)=first{return Err(error);}
        if !self.capture_pending.is_empty()||self.capture.as_ref().is_some_and(|r|r.available()>0){
            return Err("capture shutdown still has uncommitted input".into());
        }
        self.capture=None;self.transport=None;self.recording=false;
        Ok(())
    }
    fn update_rig(
        &mut self,
        host: &Host,
        position: [f64; 3],
        forward: [f64; 3],
        up: [f64; 3],
    ) -> Result<(), String> {
        let Some(transport) = self.transport.as_mut() else {
            return Ok(());
        };
        let mut routes = Routes::default();
        let mut segments = Vec::new();
        let right = crate::audio::cross(forward, up);
        // Speaker mute gates the callback, not the prepared spatial routes, so
        // re-enabling while the computer owns input can use the current pose.
        if transport.world() {
            if let Some(geometry) = host.ray_audio.terrain_geometry.as_ref() {
                let key=(geometry.revision(),position.map(f64::to_bits));
                if host.ray_audio.terrain_worker.is_some() {
                    // Routing gain/pan follows the current listener. Visibility
                    // uses the latest completed bounded acoustic observation.
                    if host.ray_audio.terrain_rig.len()!=self.cabinets.len(){return Ok(());}
                    self.rig_visibility=Some((key.0,key.1,host.ray_audio.terrain_rig.clone()));
                } else if self.rig_visibility.as_ref().is_none_or(|(revision,pose,hits)|
                    (*revision,*pose)!=key || hits.len()!=self.cabinets.len()) {
                    let hits=self.cabinets.iter().map(|cabinet| {
                        let delta=crate::audio::sub(position,cabinet.position);
                        let distance=crate::audio::length(delta);
                        if distance>0.04 && distance<180. {
                            geometry.trace(cabinet.position,crate::audio::unit(delta),distance-0.02)
                        } else {None}
                    }).collect();
                    self.rig_visibility=Some((key.0,key.1,hits));
                }
                for (i, cabinet) in self.cabinets.iter().enumerate() {
                    let delta = crate::audio::sub(position, cabinet.position);
                    let distance = crate::audio::length(delta);
                    let hit = self.rig_visibility.as_ref().unwrap().2[i];
                    let visible = distance < 180. && hit.is_none();
                    routes.voices[i] = cabinet.route(position, right, visible);
                    if distance < 180. {
                        segments.push(json!({"from":cabinet.position,"to":hit.map_or(position,|h|h.position),"source":cabinet.id,"kind":if visible{"visible"}else{"blocked"}}));
                    }
                }
                routes.count = self.cabinets.len();
            }
        }
        // Full publication queues retain the previous complete routing snapshot.
        let _ = transport.routes(routes)?;
        let mut mic_routes=Routes::default();
        if let Some(listener)=host.theatre.microphone_listener() {
            let position=vector(&listener["position"])?;
            let forward=vector(&listener["forward"])?;
            if let Some(state)=host.ray_audio.microphone_terrain.as_ref() {
                if let Some(geometry)=state.terrain_geometry.as_ref() {
                    for (i,cabinet) in self.cabinets.iter().enumerate() {
                        let delta=crate::audio::sub(position,cabinet.position);
                        let distance=crate::audio::length(delta);
                        let hit=if state.terrain_worker.is_some() {
                            state.terrain_rig.get(i).copied().flatten()
                        }else if distance>0.04 && distance<180. {geometry.trace(cabinet.position,crate::audio::unit(delta),distance-0.02)}else{None};
                        let ready=state.terrain_worker.is_none()||state.terrain_rig.len()==self.cabinets.len();
                        let mut route=cabinet.route(position,[0.;3],ready&&distance<180.&&hit.is_none());
                        let direction=crate::audio::unit(crate::audio::sub(cabinet.position,position));
                        let cardioid=if distance<1e-4{1.}else{0.5*(1.+crate::audio::dot(direction,forward)).clamp(0.,2.)};
                        route.left=0.5;route.right=0.5;
                        for gain in &mut route.bands {*gain*=cardioid as f32;}
                        mic_routes.voices[i]=route;
                    }
                    mic_routes.count=self.cabinets.len();
                }
            }
        }
        let _=transport.microphone_routes(mic_routes)?;
        let peaks = transport.peaks();
        self.theatre_audio = json!({"enabled":transport.world()&&transport.speakers(),"playing":transport.playing(),"cabinets":self.cabinets.iter().map(|c|json!({"id":c.id,"bus":c.bus,"position":c.position,"forward":c.forward,"up":c.up,"horizontal":c.horizontal,"vertical":c.vertical,"omni":c.bus==1,"peak":peaks[c.bus],"gain":c.gain*c.band_gain[if c.bus==1{0}else{1}]})).collect::<Vec<_>>(),"segments":segments});
        Ok(())
    }
    fn close(&mut self, host: &mut Host, failure: Option<&str>) -> Result<(), String> {
        self.visualizations.clear();
        if self.event.is_none() && self.sound.is_none() && self.capture.is_none() {
            return Ok(());
        }
        if let Some(event) = &self.event {
            host.invoke("AudicleSystem.Resume", event.clone())?;
        }
        if let Some(transport) = self.transport.as_mut() {
            transport.set_playing(false);
            transport.set_recording(false);
        }
        let mut first = self.ambience.close(host).err();
        if let Err(e)=host.invoke("BirdSystem.Close", Value::Null) { first.get_or_insert(e); }
        if let Some(sound) = self.sound.clone() {
            let id = sound["entity"].as_u64().ok_or("sound Entity required")?;
            let native_owned = if cfg!(target_os = "ios") {
                host.ios_audio.prepared_entity() == Some(id)
            } else {
                host.audio.prepared_entity() == Some(id)
            };
            let native = if native_owned {
                host.invoke(&format!("{}.Info", backend()), sound.clone())
            } else {
                Ok(Value::Null)
            };
            let vm = if host.chuck.prepared_entity() == Some(id) {
                host.invoke("ChuckConnectionSystem.Telemetry", Value::Null)
            } else {
                Ok(Value::Null)
            };
            self.report = json!({"native":native.unwrap_or_else(|error|json!({"error":error})),"vm":vm.unwrap_or_else(|error|json!({"error":error})),"transport":self.playback_status(),"source_count_max":self.source_count_max,"simulated_seconds":self.time});
            // Preserve all ownership on failure; never drop a VM while the
            // native callback might still be using its transferred output.
            if native_owned {
                if let Err(error) = host.invoke(&format!("{}.Release", backend()), sound.clone()) {
                    return Err(first.map_or(error.clone(), |old| format!("{old}; {error}")));
                }
            }
            if let Err(error)=self.finish_capture_close(host) {
                self.report["capture_close_error"]=json!(error);
                first.get_or_insert(error);
            }
            if host.chuck.prepared_entity() == Some(id) {
                host.invoke(
                    "ChuckConnectionSystem.Release",
                    json!({"output_detached":true}),
                )?;
            }
            // Failed bird lifecycle cleanup retains its event for a retry;
            // a closed VM is treated as already cancelled by BirdSystem.
            host.invoke("EntitySystem.Remove", sound)?;
            self.sound = None;
            // Native ownership is released even if committing captured audio
            // failed. Keep its reader/transport/document until retry succeeds.
            if self.capture.is_none(){self.transport=None;self.capture_pending.clear();}
            self.recording=false;
            self.virtual_mic_available=false;
            self.cabinets.clear();
            self.theatre_audio = Value::Null;
        } else if let Err(error)=self.finish_capture_close(host) {
            first.get_or_insert(error);
        }
        self.geometry_worker=None;
        self.microphone_geometry_worker=None;
        self.microphone_geometry=None;
        self.microphone_batch_active=false;
        self.next_microphone_geometry_capture=0.;
        self.rig_visibility=None;
        if let Err(error) = host.invoke("RayTracedAudioSystem.Invalidate", Value::Null) {
            first.get_or_insert(error);
        }
        if let Some(error) = first {
            return Err(error);
        }
        if let Some(event) = &self.event {
            if let Some(reason) = failure {
                host.invoke(
                    "AudicleSystem.Fail",
                    json!({"audicle":event["audicle"],"reason":reason}),
                )?;
            } else {
                host.invoke("AudicleSystem.Finish", event.clone())?;
            }
            self.event = None;
        }
        self.geometry = None;
        self.geometry_batch_active = false;
        self.next_geometry_capture = 0.;
        Ok(())
    }
}
pub(crate) fn dispatch(host: &mut Host, hook: &str, input: &Value) -> Result<Value, String> {
    host.flush_audio_preparation_events()?;
    let parent = host.active_audicle;
    let mut owner = std::mem::take(&mut host.environment_audio);
    if hook == "Close" { owner.stop_preparation_worker(); }
    let shared = owner.worker.as_ref().map(|worker| worker.state.clone());
    let mut guard = match shared.as_ref() {
        Some(state) => Some(state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)),
        None => None,
    };
    let state = guard.as_deref_mut().unwrap_or(&mut owner);
    let was_active = state.event.is_some() || state.sound.is_some();
    let mut deferred_view = None;
    let mut deferred_native_start = None;
    let result: Result<Value, String> = (|| match hook {
        "Status" => Ok(
            json!({"active":state.event.is_some(),"ambient":state.ambience.snapshot(),"sample_rate":state.rate,"transport":state.playback_status(),"theatre_audio":state.theatre_audio(),"physical_input_ports":state.physical_input_ports,"virtual_mic_available":state.capture.is_some()&&state.virtual_mic_available,"virtual_microphone":host.theatre.microphone_listener(),"computer_keyboard_mic":state.keyboard_mic.snapshot(),"browser_preview":state.transport.as_ref().map(|t|state.browser_preview.status(t))}),
        ),
        "PreviewAudio" => {
            let transport=state.transport.as_mut().ok_or("Audio preview requires an active audio device")?;
            if input["stop"]==true{state.browser_preview.stop(transport);}else{let path=input["path"].as_str().ok_or("Preview file path required")?;state.browser_preview.toggle(path,state.rate as u32,transport)?;}
            Ok(state.browser_preview.status(transport))
        },
        "Retrospective" => {
            state.retrospective.buffer.native_dropped_frames=state.capture.as_ref().map_or(0,|r|r.dropped_frames());
            let playback=state.playback_status();
            let playhead=playback["position_frame"].as_u64().unwrap_or(0);
            state.retrospective.command(host,input,state.rate as u32,playhead,state.recording)
        },
        "DrainCapture" => Ok(json!({"blocks":state.capture_blocks.drain(..).collect::<Vec<_>>(),
            "dropped_blocks":state.capture_observer_dropped,"native_dropped_frames":state.capture.as_ref().map_or(0,|r|r.dropped_frames())})),
        "MidiKeyboard" => {
            let enabled=input["enabled"].as_bool().ok_or("computer MIDI keyboard enabled must be boolean")?;
            state.keyboard_mic.set_enabled(enabled);
            Ok(state.keyboard_mic.snapshot())
        },
        "MidiInput" => {
            let reader=state.capture.as_ref().ok_or("MIDI input requires the native sample clock")?;
            let id=input["track"].as_str().ok_or("MIDI input track required")?;
            let pitch=input["pitch"].as_u64().filter(|p|*p<=127).ok_or("MIDI input pitch must be 0..127")?;
            let velocity=input["velocity"].as_f64().filter(|v|v.is_finite()&&(0. ..=1.).contains(v)).ok_or("MIDI input velocity must be 0..1")?;
            let source=input["source"].as_str().unwrap_or("keyboard");
            if !matches!(source,"keyboard"|"preview"){return Err("MIDI input source must be keyboard or preview".into());}
            let tuning_cents=crate::workstation::piano_tuning::cents(&input["tuning_cents"],pitch as u8)?;
            let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
            if !snapshot["tracks"].as_array().is_some_and(|tracks|tracks.iter().any(|t|t["id"]==id&&t["kind"]=="midi")){return Err("MIDI input target must be an existing MIDI track".into());}
            if state.midi_pending.len()>=8192{return Err("MIDI input queue capacity reached".into());}
            let frame=reader.next_device_frame();
            state.midi_pending.push_back(json!({"capture_frame":frame,"track":id,"pitch":pitch,"tuning_cents":tuning_cents,"velocity":velocity,"source":source}));
            Ok(json!({"capture_frame":frame,"queued":true}))
        },
        "Begin" => {
            if state.event.is_some() || state.sound.is_some() {
                return Err("environment audio already prepared".into());
            }
            let world = match input["mode"].as_str() {
                None | Some("world") => true,
                Some("stereo") => false,
                _ => return Err("audio mode must be world or stereo".into()),
            };
            state.retrospective=Default::default();
            state.retrospective_error=None;
            state.time = 0.;
            state.rate = 0;
            state.source_count_max = 0;
            state.report = Value::Null;
            state.geometry = None;
            state.geometry_batch_active = false;
            state.next_geometry_capture = 0.;
            let event = host.invoke("AudicleSystem.Create", Value::Null)?;
            state.event = Some(event.clone());
            let sound = host.invoke(
                "EntitySystem.Create",
                json!({"kind":"sound","source":"embedded_chuck"}),
            )?;
            state.sound = Some(sound.clone());
            let data = host.invoke("EntitySystem.Get", sound.clone())?;
            host.invoke(&format!("{}.Prepare", backend()), data)?;
            let info = host.invoke(&format!("{}.Info", backend()), sound.clone())?;
            let rate = info["sample_rate_hz"]
                .as_f64()
                .filter(|n| n.is_finite() && (8000.0..=192000.0).contains(n))
                .ok_or("native audio sample rate required")?;
            if (rate - rate.round()).abs() > 0.01 {
                return Err("nonintegral native audio rate".into());
            }
            state.rate = rate.round() as u64;
            // The first accepted callback must not allocate/clear the entire
            // retrospective ring while live output waits for prepared PCM.
            state.retrospective.buffer.prepare(state.rate as u32)?;
            state.preparation_block_frames = info["buffer_frames"].as_u64().or_else(||info["nominal_buffer_frames"].as_u64()).unwrap_or(256).clamp(64,1024) as usize;
            if cfg!(target_os="macos") && host.system_enabled("PluginConnectionSystem") && host.system_enabled("AudioWorkstationSystem") {
                host.invoke("AudioWorkstationSystem.PreparePlugins", Value::Null)?;
            }
            if cfg!(target_os="macos") && host.system_enabled("PluginConnectionSystem") {
                host.invoke("PluginConnectionSystem.Command", json!({"op":"prepare_rate","sample_rate":state.rate}))?;
            }
            state.physical_input_ports=serde_json::from_value(info["physical_input_ports"].clone()).unwrap_or_default();
            host.invoke(
                "ChuckConnectionSystem.Prepare",
                json!({"entity":sound["entity"],"sample_rate":state.rate}),
            )?;
            let mut transport = host.chuck.take_workstation_transport()?;
            transport.set_preparation_quantum(state.preparation_block_frames);
            state.capture=Some(host.chuck.take_input_capture()?);
            state.virtual_mic_available=false;
            state.capture_pending.clear();state.capture_blocks.clear();state.midi_pending.clear();state.capture_observer_dropped=0;state.capture_discarded_frames=0;
            state.recording=false;
            transport.set_monitoring(true);
            transport.set_world(world);
            if host.system_enabled("WorkstationDataSystem"){
                let document=host.invoke("WorkstationDataSystem.Snapshot",Value::Null)?;
                let zero=document["timeline_zero_seconds"].as_f64().unwrap_or(0.);
                transport.initialize_origin((zero*state.rate as f64).round()as u64)?;
            }
            state.rig_visibility=None;
            state.cabinets = host.theatre.cabinets();
            if state.cabinets.len() > crate::audio::line_array::MAX_CABINETS {
                return Err("theatre cabinet budget exceeded".into());
            }
            for cabinet in &state.cabinets {
                cabinet.validate()?;
            }
            state.transport = Some(transport);
            state.render_revision = None;
            state.audio_complete = true;
            state.instrument_connected = false;
            state.playback_error = None;
            state.theatre_audio = Value::Null;
            host.invoke(&format!("{}.Attach", backend()), sound.clone())?;
            host.invoke("AudicleSystem.Ready", event.clone())?;
            host.invoke("AudicleSystem.Go", event)?;
            // Start only after the preparation worker has acknowledged readiness.
            // Otherwise input capture accumulates before there is a PCM producer.
            deferred_native_start = Some(sound);
            Ok(
                json!({"sample_rate":state.rate,"native_output_attached":true,"transport":state.playback_status()}),
            )
        }
        "Transport" => {
            let event = state
                .event
                .clone()
                .ok_or("environment audio not prepared")?;
            host.invoke("AudicleSystem.Resume", event)?;
            let action = input["action"]
                .as_str()
                .ok_or("transport action required")?;
            let mode = match input["mode"].as_str() {
                None => None,
                Some("world") => Some(true),
                Some("stereo") => Some(false),
                _ => return Err("audio mode must be world or stereo".into()),
            };
            let start = if let Some(v) = input.get("start_frame") {
                Some(
                    v.as_u64()
                        .filter(|n| *n <= 86400 * state.rate)
                        .ok_or("transport start outside one day")?,
                )
            } else {
                None
            };
            let gain = if let Some(v) = input.get("master_gain") {
                Some(
                    v.as_f64()
                        .filter(|v| v.is_finite() && (0.0..=16.).contains(v))
                        .ok_or("master gain must be within 0..16")? as f32,
                )
            } else {
                None
            };
            let speakers = if action == "speakers" {
                Some(
                    input["enabled"]
                        .as_bool()
                        .ok_or("speaker enabled flag required")?,
                )
            } else {
                None
            };
            if !matches!(
                action,
                "record" | "play" | "pause" | "stop" | "seek" | "mode" | "speakers" | "loop" | "rebase"
            ) {
                return Err("unknown transport action".into());
            }
            if action == "seek" && start.is_none() {
                return Err("seek start_frame required".into());
            }
            let loop_range=if action=="loop" || action=="rebase" {
                let enabled=input["enabled"].as_bool().ok_or("loop enabled flag required")?;
                let a=input["loop_start_frame"].as_u64().ok_or("loop_start_frame required")?;
                let b=input["loop_end_frame"].as_u64().ok_or("loop_end_frame required")?;
                if a>=b || b>state.rate*86400 {return Err("loop must have a positive range within one day".into());}
                Some((enabled,a,b))
            }else{None};
            if action=="rebase" && (mode.is_some()||gain.is_some()||speakers.is_some()){
                return Err("a timeline rebase cannot also change audio mode, gain or speakers".into());
            }
            if action=="record" && state.capture.is_none(){return Err("recording requires the native input capture owner".into());}
            let transport = state
                .transport
                .as_mut()
                .ok_or("workstation transport missing")?;
            if let Some(mode) = mode {
                transport.set_world(mode);
            }
            if let Some(gain) = gain {
                transport.set_gain(gain)?;
            }
            if let Some(enabled) = speakers {
                transport.set_speakers(enabled);
            }
            // Native capture is the normal device path. Apply the validated
            // monitor controls before its transport transition returns early.
            if state.capture.is_some() && matches!(action,"play"|"pause"|"stop"|"seek"|"record") {
                return state.control_capture(host,action,start);
            }
            match action {
                "rebase" => {
                    let shift=input["shift_frames"].as_u64().ok_or("rebase shift_frames required")?;
                    let(enabled,a,b)=loop_range.unwrap();
                    let plan=transport.prepare_rebase(shift,start,(a,b),enabled)?;
                    host.invoke("WorkstationDataSystem.RebaseTimeline",json!({
                        "document_revision":input["document_revision"],"shift_seconds":shift as f64/state.rate as f64}))?;
                    transport.commit_rebase(plan);
                    state.visualizations.clear();state.render_revision=None;
                    // Normal Pump refills next tick. A render failure must not
                    // turn a committed atomic rebase into an ambiguous result.
                }
                "loop" => {
                    let(enabled,start,end)=loop_range.unwrap();
                    transport.set_loop(enabled,start,end)?;
                    state.pump(host,false)?;
                }
                "play" => {
                    state.playback_error = None;
                    if let Some(start) = start {
                        transport.seek(start)?;
                    }
                    state.pump(host, true)?;
                    state.transport.as_mut().unwrap().set_playing(true);
                }
                "pause" => transport.set_playing(false),
                "stop" => {
                    transport.set_playing(false);
                    transport.seek(transport.timeline_zero())?;
                    state.render_revision = None;
                }
                "seek" => {
                    transport.seek(start.unwrap())?;
                    state.pump(host, false)?;
                }
                _ => {}
            }
            Ok(state.playback_status())
        }
        "Pump" => {
            let event = state
                .event
                .clone()
                .ok_or("environment audio not prepared")?;
            host.invoke("AudicleSystem.Resume", event)?;
            if host.invoke(
                &format!("{}.RouteChanged", backend()),
                state.sound.clone().ok_or("sound missing")?,
            )? == true
            {
                return Err("native audio route changed".into());
            }
            if !state.preparation_active { let _ = state.pump(host, false); }
            Ok(state.playback_status())
        }
        "RefreshMix" => {
            let event=state.event.clone().ok_or("environment audio not prepared")?;
            host.invoke("AudicleSystem.Resume",event)?;
            host.invoke("ChuckConnectionSystem.RefreshMix",Value::Null)
        },
        "Frame" => {
            let event = state
                .event
                .clone()
                .ok_or("environment audio not prepared")?;
            host.invoke("AudicleSystem.Resume", event)?;
            let dt = input["dt"]
                .as_f64()
                .filter(|n| n.is_finite() && (0.0..=0.1).contains(n))
                .ok_or("bounded audio frame time required")?;
            state.time += dt;
            let position = vector(&input["position"])?;
            let forward = vector(&input["forward"])?;
            let up = vector(&input["up"])?;
            let sound = state
                .sound
                .clone()
                .ok_or("environment sound Entity missing")?;
            if host.invoke(&format!("{}.RouteChanged", backend()), sound)? == true {
                return Err("native audio route changed".into());
            }
            let birds = host.invoke(
                "BirdSystem.Step",
                json!({"position":position,"up":up,"dt":dt,"time":state.time}),
            )?;
            host.invoke("AudicleSystem.Resume", state.event.clone().unwrap())?;
            let ambient = state.ambience.step(host, position, up, state.time)?;
            let mut sources = birds["sources"].as_array().ok_or("bird sources required")?.clone();
            sources.extend(ambient.iter().cloned());
            host.audio_visualizer.sources=json!(sources);
            state.source_count_max = state.source_count_max.max(sources.len());
            let async_geometry=cfg!(target_os="macos") && !cfg!(test)
                && std::env::var("WORMWOOD_GAME_ACOUSTICS_SYNC").as_deref()!=Ok("1");
            if async_geometry && state.geometry_worker.is_none(){
                state.geometry_worker=Some(crate::base_system::layer_two::terrain_geometry_system::Worker::start()?);
            }
            // A batch keeps its immutable geometry until publication, including
            // while a newer accepted snapshot is constructed in the background.
            if !state.geometry_batch_active {
                if let Some(worker)=state.geometry_worker.as_mut(){
                    if let Some(geometry)=worker.poll()?{state.geometry=Some(std::sync::Arc::new(geometry));}
                }
            }
            if state.geometry.is_none() {
                state.geometry=Some(std::sync::Arc::new(host.invoke("TerrainGeometrySystem.Capture",json!({"position":position,"time":state.time}))?));
                state.next_geometry_capture=state.time+0.4;
            } else if !state.geometry_batch_active && state.time>=state.next_geometry_capture {
                if let Some(worker)=state.geometry_worker.as_mut(){
                    if !worker.busy {
                        worker.submit(host.acoustic_capture_inputs(&json!({"position":position,"time":state.time}))?)?;
                        state.next_geometry_capture=state.time+0.4;
                    }
                } else {
                    state.geometry=Some(std::sync::Arc::new(host.invoke("TerrainGeometrySystem.Capture",json!({"position":position,"time":state.time}))?));
                    state.next_geometry_capture=state.time+0.4;
                }
            }
            let geometry = state
                .geometry
                .as_ref()
                .ok_or("environment geometry missing")?;
            let listener = json!({"position":position,"forward":forward,"up":up});
            let rays=host.advance_terrain_audio(geometry,&listener,&Value::Array(sources.clone()),state.time,&state.cabinets)?;
            let mic_listener=host.theatre.microphone_listener();
            if let Some(mic)=mic_listener.as_ref() {
                // Listener-relative invisible wind/insect sources are duplicated
                // at the capsule, never inserted into the player's source list.
                let mic_ambient=state.ambience.microphone_sources(host,vector(&mic["position"])?,vector(&mic["up"])?,state.time)?;
                let mut mic_sources=birds["sources"].as_array().ok_or("bird sources required")?.clone();
                mic_sources.extend(mic_ambient.iter().cloned());
                // The receiver keeps geometry around its own capsule even when
                // the player walks beyond the theatre's capture radius.
                if async_geometry && state.microphone_geometry_worker.is_none() {
                    state.microphone_geometry_worker=Some(crate::base_system::layer_two::terrain_geometry_system::Worker::start()?);
                }
                if !state.microphone_batch_active {
                    if let Some(worker)=state.microphone_geometry_worker.as_mut() {
                        if let Some(g)=worker.poll()? {state.microphone_geometry=Some(std::sync::Arc::new(g));}
                    }
                }
                if state.microphone_geometry.is_none() {
                    state.microphone_geometry=Some(std::sync::Arc::new(host.invoke("TerrainGeometrySystem.Capture",json!({"position":mic["position"],"time":state.time}))?));
                    state.next_microphone_geometry_capture=state.time+0.4;
                } else if !state.microphone_batch_active && state.time>=state.next_microphone_geometry_capture {
                    if let Some(worker)=state.microphone_geometry_worker.as_mut() {
                        if !worker.busy {worker.submit(host.acoustic_capture_inputs(&json!({"position":mic["position"],"time":state.time}))?)?;state.next_microphone_geometry_capture=state.time+0.4;}
                    } else {
                        state.microphone_geometry=Some(std::sync::Arc::new(host.invoke("TerrainGeometrySystem.Capture",json!({"position":mic["position"],"time":state.time}))?));
                        state.next_microphone_geometry_capture=state.time+0.4;
                    }
                }
                let mic_rays=host.advance_terrain_microphone(state.microphone_geometry.as_ref().unwrap(),mic,&Value::Array(mic_sources.clone()),state.time,&state.cabinets)?;
                state.microphone_batch_active=mic_rays["batch_active"].as_bool().ok_or("microphone batch state required")?;
                let mut mic_routes=host.invoke("SoundPhysicsSystem.ApplyMicrophone",json!({"listener":mic,"sources":mic_rays["sources"],"sample_rate":state.rate}))?;
                if let Some(rows)=mic_routes["sources"].as_array_mut() {
                    rows.retain(|r|mic_sources.iter().any(|s|s["id"]==r["id"]));
                    for row in rows {
                        if let Some(source)=mic_ambient.iter().find(|s|s["id"]==row["id"]) {
                            row["gain"]=json!(row["gain"].as_f64().unwrap_or(0.)*source["ambient_gain"].as_f64().unwrap_or(0.));
                        }
                    }
                }
                host.invoke("ChuckConnectionSystem.SetMicrophoneRoutes",mic_routes)?;
            } else {
                host.invoke("ChuckConnectionSystem.SetMicrophoneRoutes",json!({"sources":[],"available":false}))?;
            }
            let batch_active = rays["batch_active"]
                .as_bool()
                .ok_or("ray batch state required")?;
            if state.geometry_batch_active && !batch_active {
                state.next_geometry_capture = state.time + 0.4;
            }
            state.geometry_batch_active = batch_active;
            state.update_rig(host, position, forward, up)?;
            if !state.preparation_active { let _ = state.pump(host, false); }
            let mut routes = host.invoke(
                "SoundPhysicsSystem.Apply",
                json!({"listener":listener,"sources":rays["sources"],"sample_rate":state.rate}),
            )?;
            // Filter stale ray publications immediately when a water source leaves.
            if let Some(rows)=routes["sources"].as_array_mut() {
                rows.retain(|r| sources.iter().any(|s|s["id"]==r["id"]));
                for row in rows {
                    if state.ambience.submerged() && row["channel"] != 11 {
                        row["hf_alpha"]=json!(1. - (-2. * std::f64::consts::PI * 600. / state.rate as f64).exp());
                    }
                    if let Some(source)=ambient.iter().find(|s|s["id"]==row["id"]) {
                        row["gain"]=json!(row["gain"].as_f64().unwrap_or(0.) * source["ambient_gain"].as_f64().unwrap_or(0.));
                    }
                }
            }
            host.invoke("ChuckConnectionSystem.SetRoutes", routes)?;
            if input["view"].is_object() {
                let mut view = input["view"].clone();
                view["time"] = json!(state.time);
                view["theatre_audio"] = state.theatre_audio();
                let mut instances = view["instances"].as_array().cloned().unwrap_or_default();
                instances.extend(
                    birds["instances"]
                        .as_array()
                        .ok_or("bird instances required")?
                        .iter()
                        .cloned(),
                );
                view["instances"] = json!(instances);
                deferred_view = Some(view);
            }
            Ok(
                json!({"instances":birds["instances"],"time":state.time,"audio_version":rays["version"],"transport":state.playback_status(),"theatre_audio":state.theatre_audio()}),
            )
        }
        "Close" => {
            state.close(host, None)?;
            Ok(state.report.clone())
        }
        _ => Err("unknown environment audio hook".into()),
    })();
    // Native graphics submission can block for shader preparation. Release
    // audio ownership before invoking it so the preparation worker continues.
    drop(guard);
    let result = result.and_then(|value| {
        if let Some(view) = deferred_view { host.invoke("AppearanceSystem.View", view)?; }
        Ok(value)
    });
    let mut guard = shared.as_ref().map(|state|state.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
    let state = guard.as_deref_mut().unwrap_or(&mut owner);
    let result = match result {
        Err(error)
            if hook != "Close"
                && hook != "Status"
                && !matches!(hook,"Transport"|"MidiInput"|"MidiKeyboard"|"DrainCapture"|"Retrospective")
                && !(hook == "Begin" && was_active) =>
        {
            match state.close(host, Some(&error)) {
                Ok(()) => Err(error),
                Err(cleanup) => Err(format!("{error}; environment audio cleanup: {cleanup}")),
            }
        }
        result => result,
    };
    let closed = state.event.is_none() && state.sound.is_none() && state.capture.is_none();
    drop(guard);
    drop(shared);
    if closed {
        if let Some(mut worker) = owner.worker.take() { owner = worker.take_state(); }
    }
    let result = if hook == "Begin" && result.is_ok() {
        let started = host.audio_preparation_context().and_then(|context| {
            let sound = deferred_native_start.ok_or("native audio activation was not prepared")?;
            owner.prepare_before_native_start(context, |_|host.invoke(&format!("{}.Start", backend()), sound).map(|_|()))
        });
        match started {
            Ok(()) => result.map(|mut value| { value["transport"]=owner.playback_status(); value }),
            Err(error) => {
                let cleanup = owner.close(host, Some(&error));
                Err(cleanup.err().map_or(error.clone(), |cleanup|format!("{error}; {cleanup}")))
            }
        }
    } else { result };
    host.environment_audio = owner;
    host.active_audicle = parent.filter(|id| host.audicles.live(*id));
    result
}

#[cfg(test)]
mod source_observation_tests {
    use super::*;

    #[test]
    fn preparation_worker_is_ready_before_native_activation() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let directory=std::env::temp_dir().join(format!("wormwood-preparation-ready-{}",std::process::id()));
        let mut host=Host::load_paths(root,&directory).unwrap();
        let mut state=State::default();let _output=state.test_prepare_transport(48000,json!({"audicle":99}));
        let _capture=state.test_attach_capture();
        let mut activated=false;
        state.prepare_before_native_start(host.audio_preparation_context().unwrap(),|state| {
            let status=state.playback_status();
            assert_eq!(status["preparation_worker"],true);
            assert_eq!(status["preparation_ready"],true);
            assert_eq!(status["queued_frames"],0);
            activated=true;Ok(())
        }).unwrap();
        assert!(activated);state.stop_preparation_worker();
        assert_eq!(state.playback_status()["preparation_ready"],false);
        drop(state);drop(host);let _=std::fs::remove_dir_all(directory);
    }

    #[test]
    fn native_activation_failure_joins_worker_and_preserves_accepted_capture() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let directory=std::env::temp_dir().join(format!("wormwood-preparation-activation-failure-{}",std::process::id()));
        let mut host=Host::load_paths(root,&directory).unwrap();
        let mut state=State::default();let event=json!({"audicle":99});let _output=state.test_prepare_transport(48000,event.clone());
        let mut capture=state.test_attach_capture();
        let result=state.prepare_before_native_start(host.audio_preparation_context().unwrap(),|state| {
            assert_eq!(state.playback_status()["preparation_ready"],true);
            // A native driver can deliver a callback before reporting failure.
            assert!(capture.push(crate::audio::input_capture::Frame{generation:1,..Default::default()}));
            Err("synthetic native activation failure".into())
        });
        assert_eq!(result.unwrap_err(),"synthetic native activation failure");
        assert!(state.worker.is_none());assert!(!state.preparation_active);assert!(!state.preparation_ready);
        assert_eq!(state.event,Some(event));assert!(state.transport.is_some());assert!(state.capture.is_some());
        let retained=state.capture.as_ref().unwrap().available()+state.capture_pending.len()+state.playback_status()["queued_frames"].as_u64().unwrap() as usize;
        assert_eq!(retained,1,"the callback frame remains owned for ordinary cleanup");
        drop(state);drop(host);let _=std::fs::remove_dir_all(directory);
    }

    #[test]
    fn preparation_worker_keeps_pcm_running_while_control_thread_is_blocked() {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let directory=std::env::temp_dir().join(format!("wormwood-independent-audio-{}",std::process::id()));
        let mut host=Host::load_paths(root,&directory).unwrap();
        let mut edit=|hook:&str,mut request:Value| {
            request["document_revision"]=host.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap()["revision"].clone();
            host.invoke(hook,request).unwrap();
        };
        edit("WorkstationDataSystem.PutTrack",json!({"id":"signal","name":"Signal","kind":"audio"}));
        edit("WorkstationDataSystem.SetTrackMix",json!({"id":"signal","output_bus_l":"L","output_bus_r":"R"}));
        edit("WorkstationDataSystem.PutSource",json!({"id":"constant","sample_rate":48000,"channels":1,"samples":vec![0.25f32;48000]}));
        edit("WorkstationDataSystem.PutClip",json!({"id":"clip","track":"signal","kind":"audio","source":"constant","start_seconds":0.,"source_offset_seconds":0.,"duration_seconds":1.}));
        let mut state=State::default();
        let mut output=state.test_prepare_transport(48000,Value::Null);
        state.preparation_block_frames=128;
        state.transport.as_mut().unwrap().set_preparation_quantum(128);
        state.transport.as_mut().unwrap().set_playing(true);
        state.start_preparation_worker(host.audio_preparation_context().unwrap()).unwrap();
        // Start from a fully prepared queue, then consume much more than its
        // capacity while the main/graphics thread makes no Host/Pump calls.
        for _ in 0..200 {
            if state.playback_status()["queued_frames"].as_u64().unwrap_or(0)>=256 {break;}
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let consumer=std::thread::spawn(move || {
            let mut heard=0usize;
            for _ in 0..100 {
                output.begin_block();
                for _ in 0..128 {
                    let frame=output.frame();
                    if (frame[0]-0.25).abs()<1e-6 && (frame[1]-0.25).abs()<1e-6 {heard+=1;}
                }
                output.end_block(128);
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            heard
        });
        // Joining the fake hardware intentionally blocks the control thread.
        // A graphics-thread producer would exhaust even the old 100ms queue.
        let heard=consumer.join().unwrap();
        state.stop_preparation_worker();
        assert_eq!(heard,12800,"every consumed frame must retain the source sample");
        let status=state.playback_status();
        assert_eq!(status["underruns"],0);
        assert!(status["preparation_ticks"].as_u64().unwrap()>10);
        assert!(status["error"].is_null(),"{status}");
        host.flush_audio_preparation_events().unwrap();
        drop(state);drop(host);let _=std::fs::remove_dir_all(directory);
    }

    #[test]
    fn preparation_worker_monitors_native_capture_while_control_thread_is_blocked() {
        use crate::audio::input_capture::{self,Frame};
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let directory=std::env::temp_dir().join(format!("wormwood-independent-capture-{}",std::process::id()));
        let mut host=Host::load_paths(root,&directory).unwrap();
        host.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":0,"id":"input","name":"Input","kind":"audio"})).unwrap();
        host.invoke("WorkstationDataSystem.SetTrackMix",json!({"document_revision":1,"id":"input","input_monitor_mode":2,"stereo_input_pair_12":true,"output_bus_l":"L","output_bus_r":"R"})).unwrap();
        let mut state=State::default();let mut output=state.test_prepare_transport(48000,Value::Null);
        state.preparation_block_frames=128;state.transport.as_mut().unwrap().set_preparation_quantum(128);
        let(mut writer,reader)=input_capture::prepare(48000).unwrap();state.capture=Some(reader);
        let sample=|i:usize|{let x=(i+1) as f32/32768.;[x,-x]};
        for i in 0..256 {let mut frame=Frame{generation:1,physical_count:2,..Frame::default()};frame.physical[..2].copy_from_slice(&sample(i));assert!(writer.push(frame));}
        state.start_preparation_worker(host.audio_preparation_context().unwrap()).unwrap();
        for _ in 0..200 {if state.playback_status()["queued_frames"].as_u64().unwrap_or(0)>=256{break;}std::thread::sleep(std::time::Duration::from_millis(1));}
        let device=std::thread::spawn(move||{
            for block in 0..100 {
                output.begin_block();
                for i in 0..128 {assert_eq!(output.frame(),sample(block*128+i));}
                output.end_block(128);
                for i in 0..128 {let mut frame=Frame{generation:output.capture_generation(),physical_count:2,playback_sequence:output.capture_sequence(),..Frame::default()};frame.physical[..2].copy_from_slice(&sample(256+block*128+i));assert!(writer.push(frame));}
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        });
        // The real capture branch must keep processing without application Pump.
        device.join().unwrap();state.stop_preparation_worker();
        let status=state.playback_status();assert_eq!(status["underruns"],0);assert_eq!(status["capture_dropped_frames"],0);assert_eq!(status["position_frame"],0);assert!(status["error"].is_null(),"{status}");
        drop(state);drop(host);let _=std::fs::remove_dir_all(directory);
    }

    #[test]
    fn native_capture_worker_monitors_stopped_and_commits_armed_pcm_at_consumed_clock() {
        use crate::audio::input_capture::{self,Frame};
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let output=std::env::temp_dir().join(format!("wormwood-track-io-capture-{}",std::process::id()));
        let mut host=Host::load_paths(root,&output).unwrap();
        let mut edit=|hook:&str,mut input:Value| {
            input["document_revision"]=host.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap()["revision"].clone();
            host.invoke(hook,input).unwrap();
        };
        edit("WorkstationDataSystem.PutTrack",json!({"id":"mic","name":"Mic","kind":"audio"}));
        edit("WorkstationDataSystem.SetTrackMix",json!({"id":"mic","input_monitor_mode":2,"arm_mode":1,"output_bus_l":"L","output_bus_r":"R"}));
        let mut state=State::default();let mut device=state.test_prepare_transport(8000,Value::Null);
        let(mut input,reader)=input_capture::prepare(8000).unwrap();state.capture=Some(reader);
        for sample in [0.25,-0.25] {let mut f=Frame{generation:1,physical_count:1,..Frame::default()};f.physical[0]=sample;assert!(input.push(f));}
        state.pump_capture(&mut host,true).unwrap();device.begin_block();
        assert_eq!(device.frame(),[0.25,0.25]);assert_eq!(device.frame(),[-0.25,-0.25]);device.end_block(2);
        assert_eq!(device.capture_position(),(0,false));
        state.capture_transition(&mut host,true,true,0).unwrap();
        let generation=state.transport.as_ref().unwrap().generation();
        for (at,sample) in [0.5,-0.5].into_iter().enumerate(){let mut f=Frame{generation,playback_position:at as u64,playing:true,recording:true,physical_count:1,..Frame::default()};f.physical[0]=sample;assert!(input.push(f));}
        state.pump_capture(&mut host,true).unwrap();device.begin_block();
        assert_eq!(device.frame(),[0.5,0.5]);assert_eq!(device.frame(),[-0.5,-0.5]);device.end_block(2);
        assert_eq!(device.capture_position(),(2,true));
        let status=host.invoke("WorkstationDataSystem.CaptureStatus",Value::Null).unwrap();
        assert_eq!(status["record_enabled_tracks"],json!(["mic"]));assert_eq!(status["pending_frames"],2);
        state.capture_transition(&mut host,false,false,2).unwrap();
        let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap();
        assert_eq!(snapshot["clips"].as_array().unwrap().len(),1);assert_eq!(snapshot["clips"][0]["start_seconds"],0.);
        let replay=host.invoke("WorkstationDataSystem.RenderRange",json!({"document_revision":snapshot["revision"],"sample_rate":8000,"start_frame":0,"frames":2})).unwrap();
        assert_eq!(replay["lsffr"],json!([[0.5,0.,0.,0.5],[-0.5,0.,0.,-0.5]]));
        assert_eq!(state.capture_blocks.len(),2);
        drop(host);let _=std::fs::remove_dir_all(output);
    }

    #[test]
    fn one_sample_loop_capture_is_one_worker_block_and_one_decoded_source() {
        use crate::audio::input_capture::{self,Frame};
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let directory=std::env::temp_dir().join(format!("wormwood-loop-capture-{}",std::process::id()));
        let mut host=Host::load_paths(root,&directory).unwrap();
        host.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":0,"id":"mic","name":"Mic","kind":"audio"})).unwrap();
        host.invoke("WorkstationDataSystem.SetTrackMix",json!({"document_revision":1,"id":"mic","arm_mode":1,"input_monitor_mode":2,"output_bus_l":"L","output_bus_r":"R"})).unwrap();
        let mut state=State::default();let mut output=state.test_prepare_transport(8000,Value::Null);
        let(mut writer,reader)=input_capture::prepare(8000).unwrap();state.capture=Some(reader);
        state.transport.as_mut().unwrap().set_loop(true,7,8).unwrap();
        state.capture_transition(&mut host,true,true,7).unwrap();let generation=state.transport.as_ref().unwrap().generation();
        for i in 0..4096 {let mut f=Frame{generation,playback_position:7,playback_sequence:i,playing:true,recording:true,physical_count:1,..Frame::default()};f.physical[0]=if i==4095{0.8}else{0.25};assert!(writer.push(f));}
        state.pump_capture(&mut host,true).unwrap();assert_eq!(state.render_calls_last_pump,1);assert_eq!(state.capture_blocks.len(),1);assert_eq!(state.capture_blocks[0]["frames"],4096);assert_eq!(state.capture_blocks[0]["loop_start_frame"],7);
        output.begin_block();for _ in 0..4096{output.frame();}output.end_block(4096);
        assert_eq!(output.capture_position(),(7,true));assert_eq!(output.capture_sequence(),4096);
        state.capture_transition(&mut host,false,false,7).unwrap();let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap();
        assert_eq!(snapshot["clips"].as_array().unwrap().len(),1);assert_eq!(snapshot["sources"].as_array().unwrap().len(),1);
        let replay=host.invoke("WorkstationDataSystem.RenderRange",json!({"document_revision":snapshot["revision"],"sample_rate":8000,"start_frame":7,"frames":1})).unwrap();
        assert!((replay["lsffr"][0][0].as_f64().unwrap()-0.8).abs()<1e-6);
        drop(host);let _=std::fs::remove_dir_all(directory);
    }
    #[test]
    fn stop_and_visible_pause_recover_rejected_capture_and_commit_accepted_audio() {
        use crate::audio::input_capture::{self,Frame};
        for action in ["pause","stop"] {
            let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
            let out=std::env::temp_dir().join(format!("wormwood-capture-recovery-{}-{action}",std::process::id()));
            let mut host=Host::load_paths(root,&out).unwrap();
            host.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":0,"id":"mic","name":"Mic","kind":"audio"})).unwrap();
            host.invoke("WorkstationDataSystem.SetTrackMix",json!({"document_revision":1,"id":"mic","arm_mode":1})).unwrap();
            host.invoke("WorkstationDataSystem.RebaseTimeline",json!({"document_revision":2,"shift_seconds":1.})).unwrap();
            let mut state=State::default();let _device=state.test_prepare_transport(8000,Value::Null);
            let plan=state.transport.as_ref().unwrap().prepare_rebase(8000,Some(8000),(8000,16000),false).unwrap();
            state.transport.as_mut().unwrap().commit_rebase(plan);
            let(mut writer,reader)=input_capture::prepare(8000).unwrap();state.capture=Some(reader);
            state.capture_transition(&mut host,true,true,8000).unwrap();
            let generation=state.transport.as_ref().unwrap().generation();
            let mut frame=Frame{generation,playing:true,recording:true,physical_count:1,playback_position:8000,..Frame::default()};frame.physical[0]=0.5;writer.push(frame);
            state.pump_capture(&mut host,false).unwrap();
            // A malformed later frame remains pending after a worker failure.
            state.capture_pending.push_back(Frame{generation,playing:true,recording:true,physical_count:1,playback_position:9000,device_frame:1,..Frame::default()});
            state.playback_error=Some("capture rejected".into());state.recording=false;
            let status=state.control_capture(&mut host,action,None).unwrap();
            assert_eq!(status["playing"],false);assert_eq!(status["recording"],false);assert_eq!(status["capture_recovery_discarded_frames"],1);
            if action=="stop"{assert_eq!(status["position_frame"],8000);}
            let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap();assert_eq!(snapshot["clips"].as_array().unwrap().len(),1);
            assert_eq!(host.invoke("WorkstationDataSystem.CaptureStatus",Value::Null).unwrap()["pending_frames"],0);
            drop(host);let _=std::fs::remove_dir_all(out);
        }
    }
    #[test]
    fn detached_capture_shutdown_drains_full_queue_and_commits_once() {
        use crate::audio::input_capture::{self,Frame};
        for virtual_input in [false,true] {
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));let out=std::env::temp_dir().join(format!("wormwood-capture-shutdown-{}",std::process::id()));
        let mut host=Host::load_paths(root,&out).unwrap();
        host.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":0,"id":"mic","name":"Mic","kind":"audio"})).unwrap();host.invoke("WorkstationDataSystem.SetTrackMix",json!({"document_revision":1,"id":"mic","arm_mode":1,"use_virtual_input":virtual_input,"output_bus_l":"L","output_bus_r":"R"})).unwrap();
        let mut state=State::default();let _output=state.test_prepare_transport(8000,Value::Null);state.preparation_block_frames=128;let(mut writer,reader)=input_capture::prepare(8000).unwrap();state.capture=Some(reader);state.capture_transition(&mut host,true,true,0).unwrap();let generation=state.transport.as_ref().unwrap().generation();
        for i in 0..32767 {let mut frame=Frame{generation,playing:true,recording:true,physical_count:1,virtual_mic_available:virtual_input,virtual_mic:0.5,playback_position:i,playback_sequence:i,..Frame::default()};frame.physical[0]=0.25;assert!(writer.push(frame));}
        drop(writer);state.finish_capture_close(&mut host).unwrap();assert!(state.capture.is_none());assert!(state.transport.is_none());
        let snapshot=host.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap();assert_eq!(snapshot["clips"].as_array().unwrap().len(),1);assert_eq!(snapshot["clips"][0]["duration_seconds"],32767./8000.);
        state.finish_capture_close(&mut host).unwrap();assert_eq!(host.invoke("WorkstationDataSystem.Snapshot",Value::Null).unwrap()["revision"],snapshot["revision"]);
        let replay=host.invoke("WorkstationDataSystem.RenderRange",json!({"document_revision":snapshot["revision"],"sample_rate":8000,"start_frame":0,"frames":2})).unwrap();
        let expected=if virtual_input{0.5}else{0.25};
        assert_eq!(replay["lsffr"],json!([[expected,0.,0.,expected],[expected,0.,0.,expected]]));
        drop(host);let _=std::fs::remove_dir_all(out);
        }
    }
    #[test]
    fn midi_input_adapter_uses_native_clock_and_does_not_close_on_invalid_event() {
        use crate::audio::input_capture::{self,Frame};
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let output=std::env::temp_dir().join(format!("wormwood-track-io-midi-{}",std::process::id()));
        let mut host=Host::load_paths(root,&output).unwrap();
        host.invoke("WorkstationDataSystem.PutTrack",json!({"document_revision":0,"id":"keys","name":"Keys","kind":"midi"})).unwrap();
        host.invoke("WorkstationDataSystem.SetTrackMix",json!({"document_revision":1,"id":"keys","output_bus_l":"L","output_bus_r":"R"})).unwrap();
        let mut state=State::default();let mut device=state.test_prepare_transport(8000,Value::Null);
        let(mut input,reader)=input_capture::prepare(8000).unwrap();state.capture=Some(reader);host.environment_audio=state;
        assert!(host.invoke("EnvironmentAudioSystem.MidiInput",json!({"track":"keys","pitch":999,"velocity":1.})).is_err());
        assert!(host.environment_audio.capture.is_some());
        let queued=host.invoke("EnvironmentAudioSystem.MidiInput",json!({"track":"keys","pitch":69,"velocity":1.})).unwrap();
        assert_eq!(queued["capture_frame"],0);
        let mut state=std::mem::take(&mut host.environment_audio);
        for _ in 0..512{assert!(input.push(Frame{generation:1,..Frame::default()}));}
        state.pump_capture(&mut host,true).unwrap();device.begin_block();
        let sounded=(0..512).fold(false,|heard,_|{let value=device.frame()[0];heard||value.abs()>0.01});
        assert!(sounded);device.end_block(512);
        assert_eq!(state.capture_blocks[0]["midi_events"][0]["capture_frame"],0);
        assert!(state.midi_pending.is_empty());
        host.environment_audio=state;
        assert_eq!(host.invoke("EnvironmentAudioSystem.MidiInput",json!({"track":"keys","pitch":69,"velocity":0.})).unwrap()["capture_frame"],512);
        drop(host);let _=std::fs::remove_dir_all(output);
    }

    fn packet(generation: u64, revision: u64, start: u64, end: u64, sequence: u64) -> (u64, Value) {
        (generation, json!({"revision":revision,"sample_rate":48000,
            "start_frame":start,"end_frame":end,"sequence":sequence,"tracks":[]}))
    }

    fn transport(generation: u64, start: u64, position: u64) -> Value {
        json!({"playing":true,"generation":generation,"played_generation":generation,
            "generation_start_frame":start,"position_frame":position,"sample_rate":48000})
    }

    #[test]
    fn source_observation_waits_for_complete_block_and_selects_latest_consumed() {
        let packets = VecDeque::from([
            packet(1,7,0,1024,1), packet(1,7,1024,2048,2), packet(1,7,2048,4096,3),
        ]);
        assert!(selected_visualization(&packets,&transport(1,0,1023),Some(7),48000).is_none());
        let first = selected_visualization(&packets,&transport(1,0,1024),Some(7),48000).unwrap();
        assert_eq!(first["sequence"],1);
        let second = selected_visualization(&packets,&transport(1,0,3000),Some(7),48000).unwrap();
        assert_eq!(second["sequence"],2);
        let last = selected_visualization(&packets,&transport(1,0,4096),Some(7),48000).unwrap();
        assert_eq!(last["sequence"],3);
    }

    #[test]
    fn source_observation_rejects_old_generation_pre_seek_history_revision_and_rate() {
        let packets = VecDeque::from([
            packet(1,7,0,1024,1), packet(2,7,3072,4096,2), packet(2,7,4096,5120,3),
        ]);
        let mut status = transport(2,4096,5120);
        status["played_generation"] = json!(1);
        assert!(selected_visualization(&packets,&status,Some(7),48000).is_none());
        status["played_generation"] = json!(2);
        status["position_frame"] = json!(4096);
        assert!(selected_visualization(&packets,&status,Some(7),48000).is_none());
        status["position_frame"] = json!(5120);
        assert_eq!(selected_visualization(&packets,&status,Some(7),48000).unwrap()["sequence"],3);
        assert!(selected_visualization(&packets,&status,Some(8),48000).is_none());
        assert!(selected_visualization(&packets,&status,None,48000).is_none());
        assert!(selected_visualization(&packets,&status,Some(7),44100).is_none());
        status["sample_rate"] = json!(44100);
        assert!(selected_visualization(&packets,&status,Some(7),44100).is_none());
    }

    #[test]
    fn source_observation_pause_suppresses_display_without_discarding_queued_future() {
        let mut packets = VecDeque::from([
            packet(1,7,0,1024,1), packet(1,7,1024,2048,2), packet(1,7,2048,4096,3),
        ]);
        let mut status = transport(1,0,2048);
        status["playing"] = json!(false);
        assert!(selected_visualization(&packets,&status,Some(7),48000).is_none());
        prune_visualizations(&mut packets,&status,Some(7),48000);
        assert_eq!(packets.len(),2);
        assert_eq!(packets[0].1["sequence"],2);
        assert_eq!(packets[1].1["sequence"],3);
        status["playing"] = json!(true);
        assert_eq!(selected_visualization(&packets,&status,Some(7),48000).unwrap()["sequence"],2);
    }

    #[test]
    fn source_observation_cache_prunes_old_metadata_and_bounds_future_packets() {
        let mut packets = VecDeque::from([
            packet(1,7,0,1,1), packet(2,6,0,1,2), packet(2,7,0,1,3), packet(2,7,1,2,4),
        ]);
        for frame in 2..202 { packets.push_back(packet(2,7,frame,frame+1,frame+3)); }
        let status = transport(2,0,2);
        prune_visualizations(&mut packets,&status,Some(7),48000);
        assert_eq!(packets.len(),VISUALIZATION_PACKETS);
        assert_eq!(packets[0].1["sequence"],4);
        assert!(packets.iter().all(|(generation,value)| *generation==2 && value["revision"]==7));
        assert_eq!(selected_visualization(&packets,&status,Some(7),48000).unwrap()["sequence"],4);
        assert!(packets.iter().all(|(_,value)| value.get("samples").is_none()));
        prune_visualizations(&mut packets,&transport(3,500,500),Some(7),48000);
        assert!(packets.is_empty());
    }
    #[test]
    fn loop_observations_use_consumed_sequence_across_wrapped_clock() {
        let mut first=packet(1,7,3,5,1);first.1["transport_end_sequence"]=json!(2);
        let mut second=packet(1,7,2,5,2);second.1["transport_end_sequence"]=json!(5);
        let mut future=packet(1,7,2,5,3);future.1["transport_end_sequence"]=json!(8);
        let mut packets=VecDeque::from([first,second,future]);
        let mut status=transport(1,3,2);status["consumed_sequence"]=json!(5);
        assert_eq!(selected_visualization(&packets,&status,Some(7),48000).unwrap()["sequence"],2);
        prune_visualizations(&mut packets,&status,Some(7),48000);
        assert_eq!(packets.len(),2);assert_eq!(packets[0].1["sequence"],2);
    }

}
