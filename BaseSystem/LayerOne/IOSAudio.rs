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
    }
    struct Callback {
        samples: Box<[f32]>,
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
        let mut produced = 0;
        let mut peak = f32::from_bits(cb.peak.load(Ordering::Relaxed));
        for i in 0..n as usize {
            let v = if go && rt.cursor < cb.samples.len() {
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
        if go && rt.cursor == cb.samples.len() {
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
        start_gaps: u64,
    }
    #[cfg(target_os = "ios")]
    impl Client {
        pub(super) fn prepare() -> Result<Self, String> {
            // Reserve enough for the supported maximum before native activation;
            // native setup returns the actual rate, then we fill while Go is false.
            // The callback must not observe mutation, so AudioUnit starts only after
            // receiving prepared samples through a second native entry point.
            let cb = Box::new(Callback {
                samples: Box::new([]),
                rt: UnsafeCell::new(Rt {
                    cursor: 0,
                    tail: 0,
                    next_time: None,
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
            unsafe extern "C" {
                fn ww_audio_activate(handle: *mut c_void, error: *mut u8, capacity: usize) -> i32;
            }
            if unsafe { ww_audio_activate(s.handle, error.as_mut_ptr(), error.len()) } != 0 {
                return Err(message(&error));
            }
            Ok(s)
        }
        pub(super) fn info(&self) -> Value {
            json!({"api":"Apple RemoteIO","sample_rate_hz":self.rate,"requested_buffer_frames":256,"nominal_buffer_frames":self.frames,"buffer_budget_ms":1000.*self.frames as f64/self.rate,"duration_ms":750,"tone_hz":440,"peak_amplitude":0.025,"acoustic_output_verified":false})
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
                samples: vec![0.025, -0.025].into_boxed_slice(),
                rt: UnsafeCell::new(Rt {
                    cursor: 0,
                    tail: 0,
                    next_time: None,
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
    pub(super) fn prepare(_: &Path, entity: u64, audicle: u64) -> Result<Self, String> {
        #[cfg(target_os = "ios")]
        {
            Ok(Self {
                entity,
                audicle,
                client: native::Client::prepare()?,
            })
        }
        #[cfg(not(target_os = "ios"))]
        {
            let _ = (entity, audicle);
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
