//! Embedded ChucK 1.5.5.8. VM compilation/rendering never executes on JACK/RemoteIO callbacks.
use super::dsp::{Mixer, Route};
use std::{
    cell::UnsafeCell,
    collections::VecDeque,
    ffi::{CString, c_char, c_void},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
#[derive(Clone,Copy,Default)]
struct MicrophoneRoutes { routes:[Route;17], available:bool }
const CAP: usize = 2048;
const BLOCK: usize = 128;
// Single producer VM worker and single serialized callback consumer. The indices
// publish ownership of initialized slots; neither side can access the other's slot.
struct Queue<T: Copy + Send> {
    slots: Box<[UnsafeCell<T>]>,
    read: AtomicUsize,
    write: AtomicUsize,
}

#[cfg(test)]
mod input_capture_tests {
    use super::*;
    #[test]
    fn callback_captures_physical_vm_and_player_audio_independently_while_stopped() {
        let shared=Arc::new(Shared {
            queue:Queue::new([0.;18]),routes:Queue::new([Route::default();17]),
            microphone_routes:Queue::new(MicrophoneRoutes::default()),stop:AtomicBool::new(false),
            worker_qos:AtomicI32::new(super::super::worker_scheduling::NOT_REQUESTED),
            underruns:AtomicU64::new(0),render_errors:AtomicU64::new(0),consumed:AtomicU64::new(0),
            listener_rms:AtomicU32::new(0),listener_peak:AtomicU32::new(0),source_peaks:std::array::from_fn(|_|AtomicU32::new(0)),
        });
        let (_,workstation)=super::super::transport::prepare(48000).unwrap();
        let (writer,mut reader)=super::super::input_capture::prepare(48000).unwrap();
        let mut output=Output {shared:shared.clone(),mixer:Mixer::new(48000).unwrap(),
            routes:[Route::default();17],workstation,input_capture:writer,
            microphone_mixer:Mixer::new(48000).unwrap(),microphone_routes:[Route::default();17],microphone_available:false,
            sample_rate:48000,listener_peak:0.};
        output.routes[0]=Route {active:true,gain:1.,..Route::default()};
        output.microphone_routes[0]=Route {active:true,gain:0.25,..Route::default()};
        for _ in 0..3 {let mut frame=[0.;18];frame[0]=1.;assert!(shared.queue.push(frame));}
        let mut left=[0.;3];let mut right=[0.;3];
        output.render_with_inputs(&mut left,&mut right,&[&[0.1,0.2,0.3],&[-0.1,-0.2,-0.3]]);
        let frames=reader.drain(3);assert_eq!(frames.len(),3);
        for (index,frame) in frames.iter().enumerate(){
            assert!(!frame.playing);assert_eq!(frame.device_frame,index as u64);
            assert_eq!(frame.physical_count,2);assert_eq!(frame.physical[0],[0.1,0.2,0.3][index]);
            assert_eq!(frame.physical[1],[-0.1,-0.2,-0.3][index]);
            assert_eq!(frame.player_audio,[0.5,0.5]);assert_eq!(frame.virtual_mic,0.25);
            assert!(frame.virtual_mic_available);
        }
        assert_eq!(left,[0.5;3]);assert_eq!(right,left);
        // A second receiver may be loud while the player mix is entirely muted.
        output.routes=[Route::default();17];
        for _ in 0..3 {let mut frame=[0.;18];frame[0]=1.;assert!(shared.queue.push(frame));}
        left.fill(0.);right.fill(0.);
        output.render(&mut left,&mut right);
        let mic_only=reader.drain(3);
        assert!(mic_only.iter().all(|f|f.virtual_mic>0. && f.player_audio==[0.,0.]));
        assert_eq!(left,[0.;3]);assert_eq!(right,[0.;3]);
        assert!(shared.microphone_routes.push(MicrophoneRoutes::default()));
        output.render(&mut left,&mut right);
        let frames=reader.drain(3);assert!(frames.iter().all(|f|!f.virtual_mic_available&&f.virtual_mic==0.&&f.physical_count==0));
        assert!(shared.microphone_routes.push(MicrophoneRoutes{routes:[Route::default();17],available:true}));
        output.render(&mut left,&mut right);
        assert!(reader.drain(3).iter().all(|f|f.virtual_mic_available&&f.virtual_mic==0.));
    }
}
unsafe impl<T: Copy + Send> Sync for Queue<T> {}
impl<T: Copy + Send> Queue<T> {
    fn new(initial: T) -> Self {
        Self {
            slots: (0..CAP).map(|_| UnsafeCell::new(initial)).collect(),
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
        }
    }
    fn free(&self) -> usize {
        let w = self.write.load(Ordering::Relaxed);
        let r = self.read.load(Ordering::Acquire);
        CAP - 1 - (w + CAP - r) % CAP
    }
    fn push(&self, frame: T) -> bool {
        let w = self.write.load(Ordering::Relaxed);
        let next = (w + 1) % CAP;
        if next == self.read.load(Ordering::Acquire) {
            return false;
        }
        unsafe { *self.slots[w].get() = frame }
        self.write.store(next, Ordering::Release);
        true
    }
    fn pop(&self) -> Option<T> {
        let r = self.read.load(Ordering::Relaxed);
        if r == self.write.load(Ordering::Acquire) {
            return None;
        }
        let p = unsafe { *self.slots[r].get() };
        self.read.store((r + 1) % CAP, Ordering::Release);
        Some(p)
    }
}
struct Shared {
    queue: Queue<[f32; 18]>,
    routes: Queue<[Route; 17]>,
    microphone_routes: Queue<MicrophoneRoutes>,
    stop: AtomicBool,
    worker_qos: AtomicI32,
    underruns: AtomicU64,
    render_errors: AtomicU64,
    consumed: AtomicU64,
    listener_rms: AtomicU32,
    listener_peak: AtomicU32,
    source_peaks:[AtomicU32;17],
}
/// Final stereo output, after environmental and workstation mixing. Cardinal's
/// phone meter uses mean channel energy and a 250 ms peak release, in dBFS.
#[derive(Default)]
struct MeterBlock {
    energy: f64,
    peak: f32,
    frames: usize,
}
impl MeterBlock {
    fn frame(&mut self, left: f32, right: f32) {
        self.frames += 1;
        if left.is_finite() && right.is_finite() {
            self.peak = self.peak.max(left.abs()).max(right.abs());
            let (left, right) = (f64::from(left), f64::from(right));
            self.energy += 0.5 * (left * left + right * right);
        }
    }
    fn finish(self, previous_peak: f32, sample_rate: u32) -> (f32, f32) {
        let rms = if self.frames == 0 {
            0.
        } else {
            (self.energy / self.frames as f64).sqrt() as f32
        };
        let release = (-(self.frames as f64) / (f64::from(sample_rate.max(1)) * 0.25)).exp();
        (rms, self.peak.max(previous_peak * release as f32))
    }
}
#[derive(Debug)]
pub(crate) enum Event {
    Compiled { request: u64, shred: u64 },
    Failed { request: u64, error: String },
    Completed { request: u64 },
    Cancelled { request: u64 },
}
enum Command {
    Compile {
        request: u64,
        path: PathBuf,
        argument: String,
        feedback: bool,
    },
    Remove {
        request: u64,
    },
}
unsafe extern "C" {
    fn mirror_chuck_open(rate: u32, error: *mut c_char, capacity: usize) -> *mut c_void;
    fn mirror_chuck_compile(
        vm: *mut c_void,
        path: *const c_char,
        args: *const c_char,
        error: *mut c_char,
        capacity: usize,
    ) -> u64;
    fn mirror_chuck_run(vm: *mut c_void, output: *mut f32, frames: u32) -> i32;
    fn mirror_chuck_close(vm: *mut c_void);
    fn mirror_chuck_active(vm: *mut c_void, id: u64) -> i32;
    fn mirror_chuck_remove(vm: *mut c_void, id: u64) -> i32;
}
fn message(error: &[u8]) -> String {
    String::from_utf8_lossy(error.split(|x| *x == 0).next().unwrap_or_default()).into_owned()
}
struct Vm(*mut c_void);
impl Drop for Vm {
    fn drop(&mut self) {
        unsafe { mirror_chuck_close(self.0) }
    }
}
/// Move this into the native callback's pinned state. Do not destroy it in callback.
pub(crate) struct Output {
    shared: Arc<Shared>,
    mixer: Mixer,
    routes: [Route; 17],
    workstation: super::transport::Output,
    input_capture: super::input_capture::Writer,
    microphone_mixer: Mixer,
    microphone_routes: [Route; 17],
    microphone_available: bool,
    sample_rate: u32,
    listener_peak: f32,
}
impl Output {
    pub fn set_routes(&mut self, routes: [Route; 17]) {
        self.routes = routes;
    }
    /// Add to host-owned output slices; caller clears them once per device period.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.render_with_inputs(left, right, &[]);
    }
    /// Physical inputs belong to this callback period. The independent microphone
    /// routes preserve VM1 as a spatial capture, separate from final player audio.
    pub fn render_with_inputs(&mut self,left:&mut [f32],right:&mut [f32],inputs:&[&[f32]]) {
        self.workstation.begin_block();
        for _ in 0..8 {
            if let Some(routes)=self.shared.microphone_routes.pop(){self.microphone_routes=routes.routes;self.microphone_available=routes.available;}else{break;}
        }
        let mic_available=self.microphone_available || self.microphone_routes.iter().any(|route|route.active);
        for _ in 0..8 {
            if let Some(routes) = self.shared.routes.pop() {
                self.routes = routes;
            } else {
                break;
            }
        }
        let mut starved = false;
        let mut meter = MeterBlock::default();
        let mut source_peaks=[0f32;17];
        for (index, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let frame = if let Some(frame) = self.shared.queue.pop() {
                self.shared.consumed.fetch_add(1, Ordering::Relaxed);
                frame
            } else {
                starved = true;
                [0.; 18]
            };
            // Device time advances even on starvation: decay/filter/echo state
            // must not pause and resume old tails at an unrelated later time.
            // VM channel 17 carries workstation feedback directly to the listener.
            let environment: &[f32; 17] = frame[..17].try_into().unwrap();
            for (peak,sample) in source_peaks.iter_mut().zip(environment){*peak=peak.max(sample.abs());}
            let out = self.mixer.frame(environment, &self.routes);
            let microphone = self.microphone_mixer.frame(environment, &self.microphone_routes);
            let (playback_position,playing)=self.workstation.capture_position();
            let playback_sequence=self.workstation.capture_sequence();
            let workstation = self.workstation.frame();
            *l += out[0] + workstation[0] + frame[17];
            *r += out[1] + workstation[1] + frame[17];
            let mut captured=super::input_capture::Frame {
                playback_position,playback_sequence,playing,recording:self.workstation.capture_recording(),generation:self.workstation.capture_generation(),
                physical_count:inputs.len().min(super::input_capture::MAX_INPUTS) as u8,
                virtual_mic:microphone[0]+microphone[1]+self.workstation.microphone_sample(),virtual_mic_available:mic_available,
                player_audio:[*l,*r],..super::input_capture::Frame::default()
            };
            for (channel,input) in inputs.iter().take(super::input_capture::MAX_INPUTS).enumerate(){captured.physical[channel]=input.get(index).copied().unwrap_or(0.);}
            self.input_capture.push(captured);
            meter.frame(*l, *r);
        }
        let (rms, peak) = meter.finish(self.listener_peak, self.sample_rate);
        self.listener_peak = peak;
        for (slot,peak) in self.shared.source_peaks.iter().zip(source_peaks){slot.store(peak.to_bits(),Ordering::Relaxed);}
        self.shared.listener_rms.store(rms.to_bits(), Ordering::Relaxed);
        self.shared.listener_peak.store(peak.to_bits(), Ordering::Relaxed);
        if starved {
            self.shared.underruns.fetch_add(1, Ordering::Relaxed);
        }
        self.workstation.end_block(left.len().min(right.len()));
    }
}
/// Application-thread owner. Release native output before calling close/drop.
pub(crate) struct Session {
    shared: Arc<Shared>,
    commands: SyncSender<Command>,
    events: Receiver<Event>,
    worker: Option<JoinHandle<()>>,
    root: PathBuf,
    next: u64,
    workstation: Option<super::transport::Producer>,
    input_capture: Option<super::input_capture::Reader>,
}
impl Session {
    pub fn prepare(root: &Path, rate: u32) -> Result<(Self, Output), String> {
        let mixer = Mixer::new(rate)?;
        let microphone_mixer = Mixer::new(rate)?;
        let (input_writer,input_reader)=super::input_capture::prepare(rate)?;
        let (workstation_producer, workstation_output) = super::transport::prepare(rate)?;
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let shared = Arc::new(Shared {
            queue: Queue::new([0.; 18]),
            routes: Queue::new([Route::default(); 17]),
            microphone_routes: Queue::new(MicrophoneRoutes::default()),
            stop: AtomicBool::new(false),
            worker_qos: AtomicI32::new(super::worker_scheduling::NOT_REQUESTED),
            underruns: AtomicU64::new(0),
            render_errors: AtomicU64::new(0),
            consumed: AtomicU64::new(0),
            listener_rms: AtomicU32::new(0f32.to_bits()),
            listener_peak: AtomicU32::new(0f32.to_bits()),
            source_peaks:std::array::from_fn(|_|AtomicU32::new(0)),
        });
        let copy = shared.clone();
        let (tx, rx) = mpsc::sync_channel::<Command>(32);
        let (etx, erx) = mpsc::sync_channel(64);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("mirror-chuck-vm".into())
            .spawn(move || {
                copy.worker_qos.store(super::worker_scheduling::request(), Ordering::Release);
                let mut error = [0u8; 512];
                let ptr =
                    unsafe { mirror_chuck_open(rate, error.as_mut_ptr().cast(), error.len()) };
                if ptr.is_null() {
                    let _ = ready_tx.send(Err(message(&error)));
                    return;
                }
                let vm = Vm(ptr);
                let _ = ready_tx.send(Ok(()));
                let mut pcm = [0f32; BLOCK * 18];
                let mut pending: Vec<(u64, u64, u64, Option<u64>, bool)> = Vec::with_capacity(64);
                let mut generated = 0u64;
                // Keep lifecycle messages until the application receives them.
                // A full event channel applies bounded control backpressure;
                // it must never silently drop a failed compile or cancellation.
                let mut outbox = VecDeque::with_capacity(64);
                while !copy.stop.load(Ordering::Acquire) {
                    while let Some(event) = outbox.pop_front() {
                        match etx.try_send(event) {
                            Ok(()) => {}
                            Err(mpsc::TrySendError::Full(event)) => {
                                outbox.push_front(event);
                                break;
                            }
                            Err(mpsc::TrySendError::Disconnected(_)) => {
                                copy.stop.store(true, Ordering::Release);
                                break;
                            }
                        }
                    }
                    for _ in 0..8 {
                        if outbox.len() >= 64 {
                            break;
                        }
                        let Ok(command) = rx.try_recv() else { break };
                        let (request, path, argument, feedback) = match command {
                            Command::Compile {
                                request,
                                path,
                                argument, feedback,
                            } => {
                                if pending.len() >= 64 {
                                    if feedback { eprintln!("Workstation SFX voice limit reached"); continue; }
                                    outbox.push_back(Event::Failed {
                                        request,
                                        error: "ChucK active shred limit reached".into(),
                                    });
                                    continue;
                                }
                                (request, path, argument, feedback)
                            }
                            Command::Remove { request } => {
                                if let Some(index) = pending.iter().position(|p| p.0 == request) {
                                    let id = pending[index].1;
                                    if unsafe { mirror_chuck_remove(vm.0, id) } == 0 {
                                        copy.render_errors.fetch_add(1, Ordering::Relaxed);
                                        outbox.push_back(Event::Failed {
                                            request,
                                            error: "ChucK shred removal failed".into(),
                                        });
                                        continue;
                                    }
                                    pending.swap_remove(index);
                                }
                                outbox.push_back(Event::Cancelled { request });
                                continue;
                            }
                        };
                        let result = (|| {
                            let path = CString::new(path.to_string_lossy().as_bytes())
                                .map_err(|e| e.to_string())?;
                            let args = CString::new(argument).unwrap();
                            error.fill(0);
                            let id = unsafe {
                                mirror_chuck_compile(
                                    vm.0,
                                    path.as_ptr(),
                                    args.as_ptr(),
                                    error.as_mut_ptr().cast(),
                                    error.len(),
                                )
                            };
                            if id == 0 {
                                Err(message(&error))
                            } else {
                                Ok(id)
                            }
                        })();
                        let event = match result {
                            Ok(shred) => {
                                pending.push((request, shred, generated, None, feedback));
                                Event::Compiled { request, shred }
                            }
                            Err(error) => Event::Failed { request, error },
                        };
                        if feedback {
                            if let Event::Failed {error,..} = event { eprintln!("Workstation SFX: {error}"); }
                        } else { outbox.push_back(event); }
                    }
                    if copy.queue.free() >= BLOCK && copy.queue.free() > CAP - 1 - 512 {
                        pcm.fill(0.);
                        if unsafe { mirror_chuck_run(vm.0, pcm.as_mut_ptr(), BLOCK as u32) } == 0 {
                            copy.render_errors.fetch_add(1, Ordering::Relaxed);
                            pcm.fill(0.);
                        }
                        for frame in pcm.chunks_exact(18) {
                            let mut out = [0.; 18];
                            for (c, x) in frame.iter().enumerate() {
                                out[c] = if x.is_finite() { *x } else { 0. }
                            }
                            let _ = copy.queue.push(out);
                        }
                        generated = generated.saturating_add(BLOCK as u64);
                    } else {
                        thread::sleep(Duration::from_millis(1));
                    }
                    pending.retain_mut(|(request, shred, compiled_at, finished_at, feedback)| {
                        if generated <= *compiled_at {
                            return true;
                        }
                        if finished_at.is_none()
                            && unsafe { mirror_chuck_active(vm.0, *shred) } == 0
                        {
                            *finished_at = Some(generated);
                        }
                        if finished_at
                            .is_some_and(|end| copy.consumed.load(Ordering::Acquire) >= end)
                        {
                            if *feedback { false } else if outbox.len() >= 64 {
                                true
                            } else {
                                outbox.push_back(Event::Completed { request: *request });
                                false
                            }
                        } else {
                            true
                        }
                    });
                }
            })
            .map_err(|e| e.to_string())?;
        match ready_rx.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let _ = worker.join();
                return Err(e);
            }
            Err(e) => {
                shared.stop.store(true, Ordering::Release);
                let _ = worker.join();
                return Err(e.to_string());
            }
        }
        let output = Output {
            shared: shared.clone(),
            mixer,
            routes: [Route::default(); 17],
            workstation: workstation_output,
            input_capture: input_writer,
            microphone_mixer,
            microphone_routes: [Route::default();17],microphone_available:false,
            sample_rate: rate,
            listener_peak: 0.,
        };
        Ok((
            Self {
                shared,
                commands: tx,
                events: erx,
                worker: Some(worker),
                root,
                next: 0,
                workstation: Some(workstation_producer),
                input_capture: Some(input_reader),
            },
            output,
        ))
    }
    pub fn compile(&mut self, relative: &str, channel: usize) -> Result<u64, String> {
        if channel >= 17 {
            return Err("ChucK channel outside 17-bus layout".into());
        }
        self.queue_script(relative, channel.to_string(), false)
    }
    pub fn compile_feedback(&mut self, relative: &str, gain: f64) -> Result<u64, String> {
        self.queue_script(relative, gain.to_string(), true)
    }
    fn queue_script(&mut self, relative: &str, argument: String, feedback: bool) -> Result<u64, String> {
        let path = self
            .root
            .join(relative)
            .canonicalize()
            .map_err(|e| e.to_string())?;
        if !path.starts_with(&self.root)
            || !path.is_file()
            || path.extension().and_then(|x| x.to_str()) != Some("ck")
        {
            return Err("ChucK script must be a local .ck file".into());
        }
        self.next = self
            .next
            .checked_add(1)
            .ok_or("ChucK request counter exhausted")?;
        self.commands
            .try_send(Command::Compile {
                request: self.next,
                path,
                argument, feedback,
            })
            .map_err(|e| format!("ChucK command queue: {e}"))?;
        Ok(self.next)
    }
    pub fn take_workstation_transport(&mut self) -> Result<super::transport::Producer,String> {
        self.workstation.take().ok_or_else(||"workstation transport already transferred".into())
    }
    pub fn take_input_capture(&mut self)->Result<super::input_capture::Reader,String>{
        self.input_capture.take().ok_or_else(||"input capture already transferred".into())
    }
    pub fn microphone_routes(&self,routes:[Route;17],available:bool)->Result<(),String>{
        if routes.iter().any(|r| !r.gain.is_finite() || !r.alpha.is_finite() || !r.echo_seconds.is_finite() || !r.echo_gain.is_finite() || r.pan!=0. || r.itd_ms!=0.) {return Err("invalid mono microphone route".into());}
        if self.shared.microphone_routes.push(MicrophoneRoutes{routes,available}){Ok(())}else{Err("microphone route queue is full".into())}
    }
    pub fn remove(&self, request: u64) -> Result<(), String> {
        self.commands
            .try_send(Command::Remove { request })
            .map_err(|e| format!("ChucK remove queue: {e}"))
    }
    pub fn routes(&self, routes: [Route; 17]) -> Result<(), String> {
        if self.shared.routes.push(routes) {
            Ok(())
        } else {
            Err("spatial route queue is full".into())
        }
    }
    pub fn events(&self) -> Vec<Event> {
        self.events.try_iter().collect()
    }
    pub fn telemetry(&self) -> serde_json::Value {
        serde_json::json!({"source_peaks":self.shared.source_peaks.iter().map(|p|f32::from_bits(p.load(Ordering::Relaxed))).collect::<Vec<_>>(),"worker_scheduling":super::worker_scheduling::status(self.shared.worker_qos.load(Ordering::Acquire)),"underrun_periods":self.shared.underruns.load(Ordering::Relaxed),"render_errors":self.shared.render_errors.load(Ordering::Relaxed),"listener_rms":f32::from_bits(self.shared.listener_rms.load(Ordering::Relaxed)),"listener_peak":f32::from_bits(self.shared.listener_peak.load(Ordering::Relaxed)),"vm":"embedded_chuck_1.5.5.8","channels":17})
    }
    pub fn close(&mut self) -> Result<(), String> {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(t) = self.worker.take() {
            t.join().map_err(|_| "ChucK worker panicked")?;
        }
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workstation_cues_reach_stereo_output_with_transport_stopped() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        for cue in ["cursor_click", "keyswitch", "panel_motion", "button_click",
                    "daw_open", "daw_close", "daw_startup", "workspace_swoosh"] {
            let (mut session, mut output) = Session::prepare(root, 48000).unwrap();
            session.compile_feedback(&format!("Procedures/chuck/daw/{cue}.ck"), 1.).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            let mut audible = false;
            while std::time::Instant::now() < deadline {
                let mut left = [0.; BLOCK];
                let mut right = [0.; BLOCK];
                output.render(&mut left, &mut right);
                assert_eq!(left, right);
                if left.iter().any(|sample| sample.abs() > 0.0001) { audible = true; break; }
                thread::sleep(Duration::from_millis(1));
            }
            assert!(audible, "{cue} did not reach the stereo output");
        }
    }
    #[test]
    fn listener_meter_uses_stereo_energy_without_clamping_output() {
        let mut block = MeterBlock::default();
        block.frame(-0.5, 1.5);
        block.frame(1.5, -0.5);
        let (rms, peak) = block.finish(0., 48_000);
        assert!((rms - 1.25f32.sqrt()).abs() < 1e-6);
        assert_eq!(peak, 1.5);
    }
    #[test]
    fn listener_meter_peak_releases_in_device_time_and_silence_is_zero() {
        let block = MeterBlock { frames: 12_000, ..MeterBlock::default() };
        let (rms, peak) = block.finish(0.8, 48_000);
        assert_eq!(rms, 0.);
        assert!((peak - 0.8 * (-1f32).exp()).abs() < 1e-6);
        let empty = MeterBlock::default().finish(peak, 48_000);
        assert_eq!(empty, (0., peak));
    }
    #[test]
    fn listener_meter_rejects_nonfinite_frames_without_poisoning_telemetry() {
        let mut block = MeterBlock::default();
        block.frame(f32::NAN, 0.5);
        block.frame(0.5, f32::INFINITY);
        block.frame(0.5, -0.5);
        let (rms, peak) = block.finish(0., 48_000);
        assert!((rms - (0.25f32 / 3.).sqrt()).abs() < 1e-6);
        assert_eq!(peak, 0.5);
    }
    #[test]
    fn pcm_queue_wraps_without_overwrite_or_reordering() {
        let q = Queue::new(0u64);
        assert_eq!(q.free(), CAP - 1);
        assert_eq!(q.pop(), None);
        for cycle in 0..3 {
            for i in 0..CAP - 1 {
                assert!(q.push((cycle * CAP + i) as u64));
            }
            assert_eq!(q.free(), 0);
            assert!(!q.push(u64::MAX));
            for i in 0..CAP - 1 {
                assert_eq!(q.pop(), Some((cycle * CAP + i) as u64));
            }
            assert_eq!(q.pop(), None);
        }
    }
}
