//! RemoteIO connection ownership. Native setup/teardown never runs in the callback.
use serde_json::{Value, json};
use std::path::Path;
#[cfg(any(target_os = "ios", test))]
#[allow(dead_code, unused_imports)]
mod native {
    use super::*;
    use std::{
        cell::UnsafeCell,
        ffi::c_void,
        sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        time::{Duration, Instant},
    };
    type Render = unsafe extern "C" fn(*mut c_void, u32, *mut f32, *mut f32, f64, bool);
    #[cfg(target_os = "ios")]
    unsafe extern "C" {
        fn ww_audio_open(
            render: Render,
            interrupted: unsafe extern "C" fn(*mut c_void),
            data: *mut c_void,
            rate: *mut f64,
            frames: *mut u32,
            error: *mut u8,
            capacity: usize,
        ) -> *mut c_void;
        fn ww_audio_close(handle: *mut c_void, error: *mut u8, capacity: usize) -> i32;
    }
    struct Rt {
        cursor: usize,
        tail: u32,
        next_time: Option<f64>,
        command: u64,
        previous_command: u64,
        voice_cursor: usize,
        blend_from: f32,
        last: f32,
    }
    struct Callback {
        sample_rate: f64,
        stream: UnsafeCell<Option<crate::audio::chuck::Output>>,
        samples: Box<[f32]>,
        voices: Box<[f32]>,
        voice_len: usize,
        voice_ramp: usize,
        command: AtomicU64,
        note_done: AtomicU64,
        rt: UnsafeCell<Rt>,
        go: AtomicBool,
        done: AtomicBool,
        interrupted: AtomicBool,
        invalid: AtomicBool,
        callbacks: AtomicU64,
        produced: AtomicU64,
        gaps: AtomicU64,
        peak: AtomicU32,
        max_frames: AtomicU32,
    }
    unsafe extern "C" fn interrupted(data: *mut c_void) {
        unsafe { &*(data as *const Callback) }
            .interrupted
            .store(true, Ordering::Release);
    }
    unsafe extern "C" fn render(
        data: *mut c_void,
        n: u32,
        left: *mut f32,
        right: *mut f32,
        time: f64,
        valid: bool,
    ) {
        // RemoteIO serializes the output callback. The owning Box outlives disposal.
        let cb = unsafe { &*(data as *const Callback) };
        let _performance = crate::host::performance_telemetry::AudioScope::new(n, cb.sample_rate);
        let rt = unsafe { &mut *cb.rt.get() };
        if left.is_null() || right.is_null() {
            cb.invalid.store(true, Ordering::Relaxed);
            return;
        }
        if valid {
            if let Some(expected) = rt.next_time {
                if (time - expected).abs() > 0.5 {
                    cb.gaps.fetch_add(1, Ordering::Relaxed);
                }
            }
            rt.next_time = Some(time + n as f64);
        }
        let go = cb.go.load(Ordering::Acquire) && !cb.interrupted.load(Ordering::Acquire);
        if go {
            if let Some(stream) = unsafe { &mut *cb.stream.get() } {
                let left = unsafe { std::slice::from_raw_parts_mut(left,n as usize) };
                let right = unsafe { std::slice::from_raw_parts_mut(right,n as usize) };
                left.fill(0.);
                right.fill(0.);
                stream.render(left,right);
                let peak=left.iter().chain(right.iter()).fold(0f32,|peak,value|peak.max(value.abs()));
                cb.peak.fetch_max(peak.to_bits(),Ordering::Relaxed);
                cb.callbacks.fetch_add(1,Ordering::Relaxed);
                cb.produced.fetch_add(n as u64,Ordering::Relaxed);
                cb.max_frames.fetch_max(n,Ordering::Relaxed);
                return;
            }
        }
        if go && !cb.voices.is_empty() {
            let command = cb.command.load(Ordering::Acquire);
            if command != rt.command {
                rt.previous_command = rt.command;
                rt.command = command;
                rt.voice_cursor = 0;
                rt.blend_from = rt.last;
            }
        }
        let mut produced = 0;
        let mut peak = f32::from_bits(cb.peak.load(Ordering::Relaxed));
        for i in 0..n as usize {
            let v = if go && !cb.voices.is_empty() {
                let mut value = 0.;
                if rt.command != 0 && rt.voice_cursor < cb.voice_len {
                    value = cb.voices[(rt.command as usize & 31) * cb.voice_len + rt.voice_cursor];
                    let blend = (rt.voice_cursor as f32 / cb.voice_ramp as f32).min(1.);
                    value = rt.blend_from * (1. - blend) + value * blend;
                    rt.voice_cursor += 1;
                    produced += 1;
                    if rt.voice_cursor >= cb.voice_ramp && rt.previous_command != 0 {
                        cb.note_done
                            .store(rt.previous_command >> 5, Ordering::Release);
                        rt.previous_command = 0;
                    }
                    if rt.voice_cursor == cb.voice_len {
                        cb.note_done.store(rt.command >> 5, Ordering::Release);
                    }
                }
                rt.last = value;
                value
            } else if go && rt.cursor < cb.samples.len() {
                let v = cb.samples[rt.cursor];
                rt.cursor += 1;
                produced += 1;
                v
            } else {
                0.0
            };
            unsafe {
                *left.add(i) = v;
                *right.add(i) = v;
            }
            peak = peak.max(v.abs());
        }
        if go && cb.voices.is_empty() && rt.cursor == cb.samples.len() {
            rt.tail = rt.tail.saturating_add(1);
            if rt.tail >= 4 {
                cb.done.store(true, Ordering::Release);
            }
        }
        cb.callbacks.fetch_add(1, Ordering::Relaxed);
        cb.produced.fetch_add(produced, Ordering::Relaxed);
        cb.peak.store(peak.to_bits(), Ordering::Relaxed);
        cb.max_frames.fetch_max(n, Ordering::Relaxed);
    }
    #[cfg(target_os = "ios")]
    pub(super) struct Client {
        handle: *mut c_void,
        callback: Option<Box<Callback>>,
        rate: f64,
        frames: u32,
        started: bool,
        streaming: bool,
        start_gaps: u64,
    }
    #[cfg(target_os = "ios")]
    impl Client {
        pub(super) fn prepare(voice: bool) -> Result<Self, String> {
            // Reserve enough for the supported maximum before native activation;
            // native setup returns the actual rate, then we fill while Go is false.
            // The callback must not observe mutation, so AudioUnit starts only after
            // receiving prepared samples through a second native entry point.
            let cb = Box::new(Callback {
                sample_rate: 0.,
                stream: UnsafeCell::new(None),
                samples: Box::new([]),
                voices: Box::new([]),
                voice_len: 0,
                voice_ramp: 1,
                command: AtomicU64::new(0),
                note_done: AtomicU64::new(0),
                rt: UnsafeCell::new(Rt {
                    cursor: 0,
                    tail: 0,
                    next_time: None,
                    command: 0,
                    previous_command: 0,
                    voice_cursor: 0,
                    blend_from: 0.,
                    last: 0.,
                }),
                go: AtomicBool::new(false),
                done: AtomicBool::new(false),
                interrupted: AtomicBool::new(false),
                invalid: AtomicBool::new(false),
                callbacks: AtomicU64::new(0),
                produced: AtomicU64::new(0),
                gaps: AtomicU64::new(0),
                peak: AtomicU32::new(0),
                max_frames: AtomicU32::new(0),
            });
            let mut s = Self {
                handle: std::ptr::null_mut(),
                callback: Some(cb),
                rate: 0.,
                frames: 0,
                started: false,
                streaming: false,
                start_gaps: 0,
            };
            let mut error = [0u8; 1024];
            let data = (&**s.callback.as_ref().unwrap()) as *const Callback as *mut c_void;
            s.handle = unsafe {
                ww_audio_open(
                    render,
                    interrupted,
                    data,
                    &mut s.rate,
                    &mut s.frames,
                    error.as_mut_ptr(),
                    error.len(),
                )
            };
            if s.handle.is_null() {
                return Err(message(&error));
            }
            if !(8000.0..=192000.0).contains(&s.rate) {
                return Err("unsupported iOS sample rate".into());
            }
            s.callback.as_mut().unwrap().sample_rate = s.rate;
            let len = (s.rate * 0.75) as usize;
            let ramp = (s.rate / 100.) as usize;
            s.callback.as_mut().unwrap().samples = (0..len)
                .map(|i| {
                    let envelope = (i.min(len - 1 - i) as f32 / ramp as f32).min(1.);
                    (std::f32::consts::TAU * 440. * i as f32 / s.rate as f32).sin()
                        * 0.025
                        * envelope
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            if voice {
                let cb = s.callback.as_mut().unwrap();
                cb.samples = Box::new([]);
                cb.voice_len = (s.rate * 0.4) as usize;
                cb.voice_ramp = (s.rate * 0.005) as usize;
                cb.voices = (0..24)
                    .flat_map(|note| {
                        let rate = s.rate;
                        let len = cb.voice_len;
                        let hz = 440. * 2f64.powf((note as f64 + 60. - 69.) / 12.);
                        (0..len).map(move |i| {
                            let t = i as f64 / rate;
                            let envelope = (t / 0.005).min(1.) * ((len - 1 - i) as f64 / (rate * 0.08)).min(1.);
                            let phase = std::f64::consts::TAU * hz * t;
                            ((phase.sin() + 0.18 * (2. * phase).sin()) * 0.07 * envelope) as f32
                        })
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice();
            }
            unsafe extern "C" {
                fn ww_audio_activate(handle: *mut c_void, error: *mut u8, capacity: usize) -> i32;
            }
            if unsafe { ww_audio_activate(s.handle, error.as_mut_ptr(), error.len()) } != 0 {
                return Err(message(&error));
            }
            Ok(s)
        }
        pub(super) fn complete(&self) -> bool {
            self.callback
                .as_ref()
                .is_some_and(|c| c.done.load(Ordering::Acquire))
        }
        pub(super) fn route_changed(&self) -> bool {
            self.callback
                .as_ref()
                .is_none_or(|c| c.interrupted.load(Ordering::Acquire))
        }
        pub(super) fn note(&self, midi: u64) -> Result<u64, String> {
            let c = self.callback.as_ref().ok_or("audio released")?;
            if !self.started || c.voices.is_empty() || !(60..=83).contains(&midi) {
                return Err("temple voice requires MIDI 60..83 and running output".into());
            }
            let serial = (c.command.load(Ordering::Relaxed) >> 5) + 1;
            c.command
                .store((serial << 5) | (midi - 60), Ordering::Release);
            Ok(serial)
        }
        pub(super) fn note_complete(&self, serial: u64) -> bool {
            self.callback
                .as_ref()
                .is_some_and(|c| c.note_done.load(Ordering::Acquire) >= serial)
        }
        pub(super) fn info(&self) -> Value {
            let mut info=json!({"api":"Apple RemoteIO","sample_rate_hz":self.rate,"requested_buffer_frames":256,"nominal_buffer_frames":self.frames,"buffer_budget_ms":1000.*self.frames as f64/self.rate,"duration_ms":750,"tone_hz":440,"peak_amplitude":0.025,"acoustic_output_verified":false});
            if self.streaming {
                for key in ["tone_hz","duration_ms","peak_amplitude"] {info.as_object_mut().unwrap().remove(key);}
                let cb=self.callback.as_ref().unwrap();
                info["mode"]=json!("embedded_chuck");
                info["stream_output_peak"]=json!(f32::from_bits(cb.peak.load(Ordering::Relaxed)));
                info["samples_produced"]=json!(cb.produced.load(Ordering::Relaxed));
                info["callbacks"]=json!(cb.callbacks.load(Ordering::Relaxed));
                info["callback_discontinuities"]=json!(cb.gaps.load(Ordering::Relaxed));
            }
            info
        }
        pub(super) fn start(&mut self) -> Result<(), String> {
            if self.started {
                return Err("iOS audio already started".into());
            }
            let cb = self.callback.as_ref().unwrap();
            if cb.interrupted.load(Ordering::Acquire) {
                return Err("iOS audio interrupted before Go".into());
            }
            self.start_gaps = cb.gaps.load(Ordering::Relaxed);
            self.started = true;
            cb.go.store(true, Ordering::Release);
            Ok(())
        }
        pub(super) fn stop(&mut self) -> Result<Value, String> {
            if !self.started {
                return Err("iOS audio has not started".into());
            }
            let cb = self.callback.as_ref().unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            while !cb.done.load(Ordering::Acquire)
                && !cb.interrupted.load(Ordering::Acquire)
                && Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            self.close()?;
            let cb = self.callback.as_ref().unwrap();
            let mut report = self.info();
            let samples = cb.produced.load(Ordering::Relaxed);
            let peak = f32::from_bits(cb.peak.load(Ordering::Relaxed));
            let gaps = cb.gaps.load(Ordering::Relaxed);
            let complete = cb.done.load(Ordering::Acquire);
            let verified = complete
                && samples == cb.samples.len() as u64
                && peak > 0.02
                && peak < 0.03
                && !cb.invalid.load(Ordering::Relaxed)
                && !cb.interrupted.load(Ordering::Acquire);
            report["callbacks"] = json!(cb.callbacks.load(Ordering::Relaxed));
            report["samples_produced"] = json!(samples);
            report["completed"] = json!(complete);
            report["output_buffer_verified"] = json!(verified);
            report["output_peak"] = json!(peak);
            report["callback_discontinuities"] = json!(gaps);
            report["active_callback_discontinuities"] = json!(gaps.saturating_sub(self.start_gaps));
            report["max_callback_frames"] = json!(cb.max_frames.load(Ordering::Relaxed));
            // RemoteIO supplies no JACK-style xrun counter. Do not invent one.
            if !verified || gaps != self.start_gaps {
                return Err(format!("iOS audio verification failed: {report}"));
            }
            Ok(report)
        }
        fn close(&mut self) -> Result<(), String> {
            if !self.handle.is_null() {
                let mut error = [0; 1024];
                if unsafe { ww_audio_close(self.handle, error.as_mut_ptr(), error.len()) } != 0 {
                    return Err(message(&error));
                }
                self.handle = std::ptr::null_mut();
            }
            Ok(())
        }
        pub(super) fn attach_output(&mut self, output: &mut Option<crate::audio::chuck::Output>) -> Result<(),String> {
            let cb = self.callback.as_ref().ok_or("RemoteIO callback missing")?;
            if self.started || cb.go.load(Ordering::Acquire) {
                return Err("attach output before the first Go".into());
            }
            let slot = unsafe { &mut *cb.stream.get() };
            if slot.is_some() || output.is_none() { return Err("output attachment is not available".into()); }
            *slot = output.take();
            self.streaming=true;
            Ok(())
        }
        pub(super) fn release(&mut self) -> Result<(), String> {
            self.close()
        }
    }
    fn message(error: &[u8]) -> String {
        String::from_utf8_lossy(error.split(|b| *b == 0).next().unwrap_or(error)).into_owned()
    }
    #[cfg(target_os = "ios")]
    impl Drop for Client {
        fn drop(&mut self) {
            if self.close().is_err() {
                if let Some(cb) = self.callback.take() {
                    let _ = Box::into_raw(cb);
                }
            }
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        fn state() -> Callback {
            Callback {
                sample_rate: 48000.,
                stream: UnsafeCell::new(None),
                samples: vec![0.025, -0.025].into_boxed_slice(),
                voices: Box::new([]),
                voice_len: 0,
                voice_ramp: 1,
                command: AtomicU64::new(0),
                note_done: AtomicU64::new(0),
                rt: UnsafeCell::new(Rt {
                    cursor: 0,
                    tail: 0,
                    next_time: None,
                    command: 0,
                    previous_command: 0,
                    voice_cursor: 0,
                    blend_from: 0.,
                    last: 0.,
                }),
                go: AtomicBool::new(false),
                done: AtomicBool::new(false),
                interrupted: AtomicBool::new(false),
                invalid: AtomicBool::new(false),
                callbacks: AtomicU64::new(0),
                produced: AtomicU64::new(0),
                gaps: AtomicU64::new(0),
                peak: AtomicU32::new(0),
                max_frames: AtomicU32::new(0),
            }
        }
        #[test]
        fn remoteio_stream_consumes_real_vm_only_after_go_and_preserves_bounds() {
            use crate::audio::{chuck::{Session,Event},dsp::Route};
            let (mut vm,mut output)=Session::prepare(Path::new(env!("CARGO_MANIFEST_DIR")),48000).unwrap();
            let mut routes=[Route::default();17];
            routes[12]=Route {active:true,gain:0.3,alpha:1.,..Route::default()};
            output.set_routes(routes);
            let request=vm.compile("Procedures/chuck/birds/cardinal.ck",12).unwrap();
            let cb=state();
            unsafe { *cb.stream.get()=Some(output); }
            let data=&cb as *const Callback as *mut c_void;
            let mut left=[99f32;514];let mut right=[99f32;514];
            unsafe {render(data,512,left.as_mut_ptr().add(1),right.as_mut_ptr().add(1),0.,true);}
            assert!(left[1..513].iter().all(|s|*s==0.));
            assert_eq!(cb.produced.load(Ordering::Relaxed),0);
            cb.go.store(true,Ordering::Release);
            let deadline=Instant::now()+Duration::from_secs(3);
            let mut peak=0f32;let mut period=1;
            while Instant::now()<deadline && peak<1e-5 {
                unsafe {render(data,512,left.as_mut_ptr().add(1),right.as_mut_ptr().add(1),(period*512)as f64,true);}
                assert_eq!([left[0],left[513],right[0],right[513]],[99.;4]);
                for value in left[1..513].iter().chain(&right[1..513]) {assert!(value.is_finite());peak=peak.max(value.abs());}
                for event in vm.events() {
                    if let Event::Failed {request:id,error}=event {if id==request {panic!("native bird failed: {error}");}}
                }
                period+=1;
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(peak>1e-5,"native VM did not reach the RemoteIO render callback");
            assert!(!cb.done.load(Ordering::Acquire),"continuous stream must not finish as a finite tone");
            cb.interrupted.store(true,Ordering::Release);
            unsafe {render(data,512,left.as_mut_ptr().add(1),right.as_mut_ptr().add(1),(period*512)as f64,true);}
            assert!(left[1..513].iter().all(|s|*s==0.));
            drop(cb);
            vm.close().unwrap();
        }
        #[test]
        fn remoteio_callback_preserves_bounds_silence_and_finite_completion() {
            let cb = state();
            let p = &cb as *const Callback as *mut c_void;
            let mut left = [99.; 6];
            let mut right = [99.; 6];
            unsafe {
                render(
                    p,
                    4,
                    left.as_mut_ptr().add(1),
                    right.as_mut_ptr().add(1),
                    0.,
                    true,
                );
            }
            assert_eq!(left, [99., 0., 0., 0., 0., 99.]);
            assert_eq!(left, right);
            assert_eq!(cb.produced.load(Ordering::Relaxed), 0);
            cb.go.store(true, Ordering::Release);
            unsafe {
                render(
                    p,
                    4,
                    left.as_mut_ptr().add(1),
                    right.as_mut_ptr().add(1),
                    4.,
                    true,
                );
            }
            assert_eq!(left, [99., 0.025, -0.025, 0., 0., 99.]);
            assert_eq!(left, right);
            for i in 2..5 {
                unsafe {
                    render(
                        p,
                        4,
                        left.as_mut_ptr().add(1),
                        right.as_mut_ptr().add(1),
                        (i * 4) as f64,
                        true,
                    );
                }
            }
            assert!(cb.done.load(Ordering::Acquire));
            assert_eq!(cb.produced.load(Ordering::Relaxed), 2);
            assert_eq!(cb.gaps.load(Ordering::Relaxed), 0);
        }
        #[test]
        fn remoteio_interruption_silences_and_timestamp_gap_is_reported() {
            let cb = state();
            let p = &cb as *const Callback as *mut c_void;
            cb.go.store(true, Ordering::Release);
            let mut left = [1.; 4];
            let mut right = [1.; 4];
            unsafe {
                interrupted(p);
                render(p, 4, left.as_mut_ptr(), right.as_mut_ptr(), 0., true);
                render(p, 4, left.as_mut_ptr(), right.as_mut_ptr(), 12., true);
            }
            assert_eq!(left, [0.; 4]);
            assert_eq!(right, left);
            assert_eq!(cb.produced.load(Ordering::Relaxed), 0);
            assert_eq!(cb.gaps.load(Ordering::Relaxed), 1);
            assert!(!cb.done.load(Ordering::Acquire));
        }
    }
}
pub(super) struct Session {
    pub entity: u64,
    pub audicle: u64,
    #[cfg(target_os = "ios")]
    client: native::Client,
}
impl Session {
    pub(super) fn prepare(
        _: &Path,
        entity: u64,
        audicle: u64,
        voice: bool,
    ) -> Result<Self, String> {
        #[cfg(target_os = "ios")]
        {
            Ok(Self {
                entity,
                audicle,
                client: native::Client::prepare(voice)?,
            })
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = (entity, audicle, voice);
            Err("iOS audio requires an iOS host".into())
        }
    }
    pub(super) fn info(&self) -> Value {
        #[cfg(target_os = "ios")]
        {
            let mut r = self.client.info();
            r["entity"] = json!(self.entity);
            r["audicle"] = json!(self.audicle);
            r
        }
        #[cfg(not(target_os = "ios"))]
        {
            json!({"api":"unavailable"})
        }
    }
    pub(super) fn start(&mut self) -> Result<(), String> {
        #[cfg(target_os = "ios")]
        {
            self.client.start()
        }
        #[cfg(not(target_os = "ios"))]
        {
            Err("iOS audio requires an iOS host".into())
        }
    }
    pub(super) fn stop(&mut self) -> Result<Value, String> {
        #[cfg(target_os = "ios")]
        {
            let mut r = self.client.stop()?;
            r["entity"] = json!(self.entity);
            r["audicle"] = json!(self.audicle);
            Ok(r)
        }
        #[cfg(not(target_os = "ios"))]
        {
            Err("iOS audio requires an iOS host".into())
        }
    }
    pub(super) fn complete(&self) -> bool {
        #[cfg(target_os = "ios")]
        {
            self.client.complete()
        }
        #[cfg(not(target_os = "ios"))]
        {
            false
        }
    }
    pub(super) fn route_changed(&self) -> bool {
        #[cfg(target_os = "ios")]
        {
            self.client.route_changed()
        }
        #[cfg(not(target_os = "ios"))]
        {
            false
        }
    }
    pub(super) fn note(&self, midi: u64) -> Result<u64, String> {
        #[cfg(target_os = "ios")]
        {
            self.client.note(midi)
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = midi;
            Err("iOS audio requires iOS".into())
        }
    }
    pub(super) fn note_complete(&self, serial: u64) -> bool {
        #[cfg(target_os = "ios")]
        {
            self.client.note_complete(serial)
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = serial;
            false
        }
    }
    pub(super) fn attach_output(&mut self, output: &mut Option<crate::audio::chuck::Output>) -> Result<(),String> {
        #[cfg(target_os="ios")] { self.client.attach_output(output) }
        #[cfg(not(target_os="ios"))] { let _ = output; Err("iOS audio requires an iOS host".into()) }
    }
    pub(super) fn release(&mut self) -> Result<(), String> {
        #[cfg(target_os = "ios")]
        {
            self.client.release()
        }
        #[cfg(not(target_os = "ios"))]
        {
            Ok(())
        }
    }
}
