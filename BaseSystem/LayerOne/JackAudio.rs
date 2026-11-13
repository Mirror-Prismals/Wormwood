//! Native JACK ownership and preallocated real-time processing. Private to the connection System.
use super::{Close, Frames, NO_START_SERVER, Open, candidates};
use libloading::Library;
use serde_json::{Value, json};
use std::{
    cell::UnsafeCell,
    ffi::{CStr, CString, c_char, c_int, c_ulong, c_void},
    path::Path,
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    time::{Duration, Instant},
};
type Action = unsafe extern "C" fn(*mut c_void) -> c_int;
type Register = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    *const c_char,
    c_ulong,
    c_ulong,
) -> *mut c_void;
type Buffer = unsafe extern "C" fn(*mut c_void, u32) -> *mut c_void;
type Process = unsafe extern "C" fn(u32, *mut c_void) -> c_int;
type SetProcess = unsafe extern "C" fn(*mut c_void, Option<Process>, *mut c_void) -> c_int;
type SetXrun = unsafe extern "C" fn(
    *mut c_void,
    Option<unsafe extern "C" fn(*mut c_void) -> c_int>,
    *mut c_void,
) -> c_int;
type Ports =
    unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, c_ulong) -> *mut *const c_char;
type PortName = unsafe extern "C" fn(*const c_void) -> *const c_char;
type Connect = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> c_int;
type Free = unsafe extern "C" fn(*mut c_void);
type Load = unsafe extern "C" fn(*mut c_void) -> f32;
struct Api {
    open: Open,
    close: Close,
    rate: Frames,
    size: Frames,
    activate: Action,
    deactivate: Action,
    register: Register,
    buffer: Buffer,
    process: SetProcess,
    xrun: SetXrun,
    ports: Ports,
    name: PortName,
    connect: Connect,
    free: Free,
    load: Load,
    realtime: Action,
    priority: Action,
    max_priority: Action,
}
impl Api {
    unsafe fn load(l: &Library) -> Result<Self, String> {
        macro_rules! sym {
            ($name:literal,$t:ty) => {
                unsafe {
                    *l.get::<$t>(concat!($name, "\0").as_bytes())
                        .map_err(|e| e.to_string())?
                }
            };
        }
        Ok(Self {
            open: sym!("jack_client_open", Open),
            close: sym!("jack_client_close", Close),
            rate: sym!("jack_get_sample_rate", Frames),
            size: sym!("jack_get_buffer_size", Frames),
            activate: sym!("jack_activate", Action),
            deactivate: sym!("jack_deactivate", Action),
            register: sym!("jack_port_register", Register),
            buffer: sym!("jack_port_get_buffer", Buffer),
            process: sym!("jack_set_process_callback", SetProcess),
            xrun: sym!("jack_set_xrun_callback", SetXrun),
            ports: sym!("jack_get_ports", Ports),
            name: sym!("jack_port_name", PortName),
            connect: sym!("jack_connect", Connect),
            free: sym!("jack_free", Free),
            load: sym!("jack_cpu_load", Load),
            realtime: sym!("jack_is_realtime", Action),
            priority: sym!("jack_client_real_time_priority", Action),
            max_priority: sym!("jack_client_max_real_time_priority", Action),
        })
    }
}
struct Rt {
    cursor: usize,
    tail: u32,
}
struct Callback {
    sample_rate: f64,
    stream: UnsafeCell<Option<crate::audio::chuck::Output>>,
    buffer: Buffer,
    outputs: [*mut c_void; 2],
    monitor: *mut c_void,
    physical_inputs: Box<[*mut c_void]>,
    samples: Box<[f32]>,
    rt: UnsafeCell<Rt>,
    go: AtomicBool,
    done: AtomicBool,
    callbacks: AtomicU64,
    produced: AtomicU64,
    xruns: AtomicU64,
    peak: AtomicU32,
    invalid: AtomicBool,
}
// JACK serializes this client's process callbacks. Only that callback accesses
// rt; the owner reads atomic telemetry, and closes the client before freeing it.
unsafe extern "C" fn process(n: u32, arg: *mut c_void) -> c_int {
    // SAFETY: pinned Box lives until jack_client_close succeeds; JACK owns callback scheduling.
    let s = unsafe { &*(arg as *const Callback) };
    let _performance = crate::host::performance_telemetry::AudioScope::new(n, s.sample_rate);
    crate::host::performance_telemetry::mark_xrun_supported();
    let a = unsafe { (s.buffer)(s.outputs[0], n) } as *mut f32;
    let b = unsafe { (s.buffer)(s.outputs[1], n) } as *mut f32;
    let input = unsafe { (s.buffer)(s.monitor, n) } as *const f32;
    if a.is_null() || b.is_null() || input.is_null() {
        s.invalid.store(true, Ordering::Relaxed);
        return 0;
    }
    let go = s.go.load(Ordering::Acquire);
    if go {
        if let Some(stream) = unsafe { &mut *s.stream.get() } {
            let left = unsafe { std::slice::from_raw_parts_mut(a,n as usize) };
            let right = unsafe { std::slice::from_raw_parts_mut(b,n as usize) };
            left.fill(0.);
            right.fill(0.);
            let mut inputs:[&[f32];crate::audio::input_capture::MAX_INPUTS]=[&[];crate::audio::input_capture::MAX_INPUTS];
            for (index,port) in s.physical_inputs.iter().enumerate(){
                let pointer=unsafe{(s.buffer)(*port,n)} as *const f32;
                if pointer.is_null(){s.invalid.store(true,Ordering::Relaxed);}else{inputs[index]=unsafe{std::slice::from_raw_parts(pointer,n as usize)};}
            }
            stream.render_with_inputs(left,right,&inputs[..s.physical_inputs.len()]);
            let peak=left.iter().chain(right.iter()).fold(0f32,|peak,value|peak.max(value.abs()));
            s.peak.fetch_max(peak.to_bits(),Ordering::Relaxed);
            s.produced.fetch_add(n as u64,Ordering::Relaxed);
            s.callbacks.fetch_add(1,Ordering::Relaxed);
            return 0;
        }
    }
    let rt = unsafe { &mut *s.rt.get() };
    let mut peak = f32::from_bits(s.peak.load(Ordering::Relaxed));
    let mut produced = 0;
    // One bounded pass over the buffer JACK supplied; no allocation, locks, I/O,
    // dispatch, JSON, destruction, waiting, or unbounded retry loops.
    for i in 0..n as usize {
        let value = if go && rt.cursor < s.samples.len() {
            let v = s.samples[rt.cursor];
            rt.cursor += 1;
            produced += 1;
            v
        } else {
            0.0
        };
        unsafe {
            *a.add(i) = value;
            *b.add(i) = value;
            peak = peak.max((*input.add(i)).abs())
        };
    }
    if go && rt.cursor == s.samples.len() {
        rt.tail = rt.tail.saturating_add(1);
        if rt.tail >= 4 {
            s.done.store(true, Ordering::Release)
        }
    }
    s.produced.fetch_add(produced, Ordering::Relaxed);
    s.callbacks.fetch_add(1, Ordering::Relaxed);
    s.peak.store(peak.to_bits(), Ordering::Relaxed);
    0
}
unsafe extern "C" fn xrun(arg: *mut c_void) -> c_int {
    let s = unsafe { &*(arg as *const Callback) };
    s.xruns.fetch_add(1, Ordering::Relaxed);
    crate::host::performance_telemetry::record_xrun();
    0
}

pub(super) struct Session {
    api: Api,
    library: Option<Library>,
    client: *mut c_void,
    callback: Option<Box<Callback>>,
    server: Option<Child>,
    pub entity: u64,
    pub audicle: u64,
    rate: u32,
    size: u32,
    started: bool,
    streaming: bool,
    owned_server: bool,
    server_name: String,
    start_xruns: u64,
    output_device: String,
    input_device: Option<String>,
    #[cfg(target_os="macos")]
    aggregate: Option<super::route::Aggregate>,
    follows_default_input: bool,
    physical_input_ports: Vec<String>,
    follows_default: bool,
    route_checked: Instant,
    route_changed: bool,
}
impl Session {
    pub(super) fn prepare(root: &Path, entity: u64, audicle: u64) -> Result<Self, String> {
        let mut errors = Vec::new();
        let mut loaded = None;
        for path in candidates() {
            match unsafe { Library::new(&path) } {
                Ok(l) => {
                    loaded = Some(l);
                    break;
                }
                Err(e) => errors.push(e.to_string()),
            }
        }
        let library = loaded.ok_or_else(|| format!("JACK library unavailable: {errors:?}"))?;
        let api = unsafe { Api::load(&library) }?;
        let explicit_output = std::env::var("WORMWOOD_JACK_OUTPUT").ok();
        let follows_default = cfg!(target_os = "macos")
            && explicit_output.is_none()
            && std::env::var_os("WORMWOOD_JACK_SERVER").is_none();
        let output_device = match explicit_output {
            Some(device) => device,
            None if cfg!(target_os = "macos") => super::route::default_uid()?,
            None => String::new(),
        };
        let explicit_server = std::env::var("WORMWOOD_JACK_SERVER").ok();
        let server_name = explicit_server.clone().unwrap_or_else(|| {
            if cfg!(any(target_os = "windows", target_os = "macos")) {
                format!("Wormwood-{}", std::process::id())
            } else {
                "Wormwood".into()
            }
        });
        let server_c =
            CString::new(server_name.as_bytes()).map_err(|_| "invalid JACK server name")?;
        let mut s = Self {
            api,
            library: Some(library),
            client: std::ptr::null_mut(),
            callback: None,
            server: None,
            entity,
            audicle,
            rate: 0,
            size: 0,
            started: false,
            streaming: false,
            owned_server: false,
            server_name,
            start_xruns: 0,
            output_device,
            input_device: if cfg!(target_os="macos") {std::env::var("WORMWOOD_JACK_INPUT").ok().or_else(||super::route::default_input_uid().ok())}else{None},
            #[cfg(target_os="macos")]
            aggregate: None,
            follows_default_input: cfg!(target_os="macos") && std::env::var_os("WORMWOOD_JACK_INPUT").is_none(),
            physical_input_ports:Vec::new(),
            follows_default,
            route_checked: Instant::now(),
            route_changed: false,
        };
        let mut status = 0;
        // JackServerName selects our isolated server; never auto-start libjack defaults.
        if !cfg!(any(target_os = "windows", target_os = "macos")) || explicit_server.is_some() {
            s.client = unsafe {
                (s.api.open)(
                    c"WormwoodCue".as_ptr(),
                    NO_START_SERVER | 0x04,
                    &mut status,
                    server_c.as_ptr(),
                )
            };
        }
        if s.client.is_null() {
            if !cfg!(any(target_os = "macos", target_os = "windows")) {
                return Err("managed server startup requires macOS or Windows".into());
            }
            std::fs::create_dir_all(root.to_path_buf()).map_err(|e| e.to_string())?;
            let log =
                std::fs::File::create(root.join("jack-server.log")).map_err(|e| e.to_string())?;
            let err = log.try_clone().map_err(|e| e.to_string())?;
            let exe = std::env::var_os("WORMWOOD_JACKD").unwrap_or_else(|| {
                if cfg!(target_os = "windows") {
                    r"C:\Program Files\JACK2\jackd.exe".into()
                } else {
                    "/opt/homebrew/bin/jackd".into()
                }
            });
            let mut command = Command::new(exe);
            if cfg!(target_os = "windows") {
                // Output-only PortAudio with an explicit device and period. No silent driver fallback.
                let device = std::env::var("WORMWOOD_JACK_OUTPUT").map_err(
                    |_| "Set WORMWOOD_JACK_OUTPUT to an explicit PortAudio playback device",
                )?;
                let period =
                    std::env::var("WORMWOOD_JACK_PERIOD").unwrap_or_else(|_| "1024".into());
                let frames = period.parse::<u32>().map_err(|_| "invalid JACK period")?;
                if !(64..=8192).contains(&frames) || !frames.is_power_of_two() {
                    return Err("JACK period must be a power of two from 64 to 8192".into());
                }
                // JACK2 Windows maps priorities 89..92 onto MMCSS Pro Audio.
                // A server priority of 92 gives its client callback priority 89.
                // Both settings are JACK's own scheduling policy, outside our callback.
                // Synchronous graph scheduling avoids the observed asynchronous
                // client deadline misses at the same ASIO rate and period.
                command.args([
                    "-n",
                    &s.server_name,
                    "-R",
                    "-P",
                    "92",
                    "-S",
                    "-d",
                    "portaudio",
                    "-P",
                    &device,
                    "-r",
                    "48000",
                    "-p",
                    &period,
                    "-i",
                    "0",
                    "-o",
                    "2",
                ]);
            } else {
                #[cfg(target_os="macos")]
                if let Some(input)=s.input_device.as_deref().filter(|input|*input!=s.output_device) {
                    s.aggregate=Some(super::route::Aggregate::create(input,&s.output_device)?);
                }
                command.args([
                    "-n",
                    &s.server_name,
                    "-d",
                    "coreaudio",
                    "-r",
                    "48000",
                    "-p",
                    "256",
                    "-o",
                    "2",
                ]);
                #[cfg(target_os="macos")]
                let aggregate=s.aggregate.as_ref().map(|a|(a.uid.as_str(),a.input_start,a.input_count));
                #[cfg(not(target_os="macos"))]
                let aggregate=None;
                command.args(coreaudio_device_args(&s.output_device,s.input_device.as_deref(),aggregate));
            }
            s.server = Some(
                command
                    .stdin(Stdio::null())
                    .stdout(log)
                    .stderr(err)
                    .spawn()
                    .map_err(|e| format!("start JACK: {e}"))?,
            );
            s.owned_server = true;
            std::thread::sleep(Duration::from_millis(200));
            let deadline = Instant::now() + Duration::from_secs(8);
            while Instant::now() < deadline {
                if let Some(code) = s
                    .server
                    .as_mut()
                    .unwrap()
                    .try_wait()
                    .map_err(|e| e.to_string())?
                {
                    return Err(server_exit_error(root,&code.to_string()));
                }
                s.client = unsafe {
                    (s.api.open)(
                        c"WormwoodCue".as_ptr(),
                        NO_START_SERVER | 0x04,
                        &mut status,
                        server_c.as_ptr(),
                    )
                };
                if !s.client.is_null() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if s.client.is_null() {
                return Err(format!("JACK startup timed out (status {status})"));
            }
        }
        s.rate = unsafe { (s.api.rate)(s.client) };
        s.size = unsafe { (s.api.size)(s.client) };
        if !(8000..=192000).contains(&s.rate) || s.size == 0 {
            return Err("unsupported JACK sample configuration".into());
        }
        let len = (s.rate as usize) * 3 / 4;
        let ramp = (s.rate as usize) / 100;
        let samples = (0..len)
            .map(|i| {
                let env = (i.min(len - 1 - i) as f32 / ramp as f32).min(1.0);
                (std::f32::consts::TAU * 440.0 * i as f32 / s.rate as f32).sin() * 0.025 * env
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let register = |name: &CStr, flags| unsafe {
            (s.api.register)(
                s.client,
                name.as_ptr(),
                c"32 bit float mono audio".as_ptr(),
                flags,
                0,
            )
        };
        let outputs = [register(c"out_L", 2), register(c"out_R", 2)];
        let monitor = register(c"verify_return", 1);
        if outputs.iter().any(|p| p.is_null()) || monitor.is_null() {
            return Err("JACK port registration failed".into());
        }
        let capture_ports=unsafe{(s.api.ports)(s.client,std::ptr::null(),c"32 bit float mono audio".as_ptr(),2|4)};
        let mut capture_sources=Vec::new();
        if !capture_ports.is_null(){
            for index in 0..crate::audio::input_capture::MAX_INPUTS{
                let name=unsafe{*capture_ports.add(index)};if name.is_null(){break;}
                capture_sources.push(unsafe{CStr::from_ptr(name)}.to_owned());
            }
            unsafe{(s.api.free)(capture_ports.cast())};
        }
        let mut physical_inputs=Vec::with_capacity(capture_sources.len());
        for (index,source) in capture_sources.iter().enumerate(){
            let name=CString::new(format!("in_{}",index+1)).unwrap();
            let input=register(&name,1);if input.is_null(){return Err("JACK capture port registration failed".into());}
            physical_inputs.push(input);
            s.physical_input_ports.push(source.to_string_lossy().into_owned());
        }
        s.callback = Some(Box::new(Callback {
            sample_rate: s.rate as f64,
            stream: UnsafeCell::new(None),
            buffer: s.api.buffer,
            outputs,
            monitor,
            physical_inputs:physical_inputs.into_boxed_slice(),
            samples,
            rt: UnsafeCell::new(Rt { cursor: 0, tail: 0 }),
            go: AtomicBool::new(false),
            done: AtomicBool::new(false),
            callbacks: AtomicU64::new(0),
            produced: AtomicU64::new(0),
            xruns: AtomicU64::new(0),
            peak: AtomicU32::new(0),
            invalid: AtomicBool::new(false),
        }));
        let data = (&**s.callback.as_ref().unwrap()) as *const Callback as *mut c_void;
        if unsafe { (s.api.process)(s.client, Some(process), data) } != 0
            || unsafe { (s.api.xrun)(s.client, Some(xrun), data) } != 0
        {
            return Err("JACK callback registration failed".into());
        }
        if unsafe { (s.api.activate)(s.client) } != 0 {
            return Err("JACK activation failed".into());
        }
        for (source,input) in capture_sources.iter().zip(s.callback.as_ref().unwrap().physical_inputs.iter()){
            if unsafe{(s.api.connect)(s.client,source.as_ptr(),(s.api.name)(*input))}!=0{return Err("JACK physical input connection failed".into());}
        }
        let ports = unsafe {
            (s.api.ports)(
                s.client,
                std::ptr::null(),
                c"32 bit float mono audio".as_ptr(),
                1 | 4,
            )
        };
        if ports.is_null() {
            return Err("JACK has no physical playback ports".into());
        }
        let mut destinations = Vec::new();
        for i in 0..2 {
            let p = unsafe { *ports.add(i) };
            if p.is_null() {
                break;
            }
            destinations.push(unsafe { CStr::from_ptr(p) }.to_owned())
        }
        unsafe { (s.api.free)(ports.cast()) };
        if destinations.len() != 2 {
            return Err("diagnostic requires two physical playback ports".into());
        }
        for (port, destination) in outputs.iter().zip(&destinations) {
            if unsafe { (s.api.connect)(s.client, (s.api.name)(*port), destination.as_ptr()) } != 0
            {
                return Err("JACK physical port connection failed".into());
            }
        }
        if unsafe { (s.api.connect)(s.client, (s.api.name)(outputs[0]), (s.api.name)(monitor)) }
            != 0
        {
            return Err("JACK verification loopback connection failed".into());
        }
        Ok(s)
    }
    pub(super) fn info(&self) -> Value {
        let realtime = unsafe { (self.api.realtime)(self.client) } != 0;
        let priority = unsafe { (self.api.priority)(self.client) };
        let max_priority = unsafe { (self.api.max_priority)(self.client) };
        let mut info=json!({"realtime":realtime,"client_priority":priority,"max_client_priority":max_priority,"sample_rate_hz":self.rate,"buffer_frames":self.size,"buffer_budget_ms":1000.0*self.size as f64/self.rate as f64,"owned_server":self.owned_server,"output_device_uid":self.output_device,"follows_system_default":self.follows_default,"server":self.server_name,"tone_hz":440,"duration_ms":750,"peak_amplitude":0.025,"entity":self.entity,"audicle":self.audicle});
        info["physical_input_ports"]=json!(self.physical_input_ports);
        info["input_device_uid"]=json!(self.input_device);
        #[cfg(target_os="macos")]
        {info["managed_duplex_device_uid"]=json!(self.aggregate.as_ref().map(|a|&a.uid));}
        if self.streaming {
            for key in ["tone_hz","duration_ms","peak_amplitude"] {info.as_object_mut().unwrap().remove(key);}
            let cb=self.callback.as_ref().unwrap();
            info["mode"]=json!("embedded_chuck");
            info["stream_output_peak"]=json!(f32::from_bits(cb.peak.load(Ordering::Relaxed)));
            info["samples_produced"]=json!(cb.produced.load(Ordering::Relaxed));
            info["callbacks"]=json!(cb.callbacks.load(Ordering::Relaxed));
        }
        info
    }
    pub(super) fn start(&mut self) -> Result<(), String> {
        if self.started {
            return Err("audio cue already started".into());
        }
        self.start_xruns = self
            .callback
            .as_ref()
            .ok_or("audio callback missing")?
            .xruns
            .load(Ordering::Relaxed);
        self.started = true;
        self.callback
            .as_ref()
            .ok_or("audio callback missing")?
            .go
            .store(true, Ordering::Release);
        Ok(())
    }
    pub(super) fn route_changed(&mut self) -> bool {
        if (self.follows_default || self.follows_default_input) && self.route_checked.elapsed() >= Duration::from_millis(100) {
            self.route_changed |=
                super::route::changed(self.follows_default, &self.output_device, super::route::default_uid());
            if self.follows_default_input{self.route_changed |= super::route::default_input_uid().ok()!=self.input_device;}
            self.route_checked = Instant::now();
        }
        self.route_changed
    }
    pub(super) fn complete(&self) -> bool {
        self.started
            && self
                .callback
                .as_ref()
                .is_some_and(|cb| cb.done.load(Ordering::Acquire))
    }
    pub(super) fn stop(&mut self) -> Result<Value, String> {
        if !self.started || self.client.is_null() {
            return Err("audio was not started".into());
        }
        let cb = self.callback.as_ref().ok_or("audio callback missing")?;
        let deadline = Instant::now() + Duration::from_secs(3);
        while !cb.done.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5))
        }
        let done = cb.done.load(Ordering::Acquire);
        let cpu = unsafe { (self.api.load)(self.client) };
        let mut report = self.info();
        self.close()?;
        let cb = self.callback.as_ref().unwrap();
        let peak = f32::from_bits(cb.peak.load(Ordering::Relaxed));
        report["callbacks"] = json!(cb.callbacks.load(Ordering::Relaxed));
        report["samples_produced"] = json!(cb.produced.load(Ordering::Relaxed));
        report["xruns"] = json!(cb.xruns.load(Ordering::Relaxed));
        report["active_xruns"] = json!(
            cb.xruns
                .load(Ordering::Relaxed)
                .saturating_sub(self.start_xruns)
        );
        report["loopback_peak"] = json!(peak);
        report["cpu_load_percent"] = json!(cpu);
        report["completed"] = json!(done);
        report["loopback_verified"] = json!(
            done && peak > 0.02
                && peak < 0.03
                && cb.produced.load(Ordering::Relaxed) == cb.samples.len() as u64
                && !cb.invalid.load(Ordering::Relaxed)
        );
        if report["loopback_verified"] != true {
            return Err(format!("audio verification failed: {report}"));
        }
        Ok(report)
    }
    pub(super) fn attach_output(&mut self, output: &mut Option<crate::audio::chuck::Output>) -> Result<(),String> {
        let cb = self.callback.as_ref().ok_or("JACK callback missing")?;
        if self.started || cb.go.load(Ordering::Acquire) {
            return Err("attach output before the first Go".into());
        }
        // Before Go, the callback never touches this separate cell. The Release
        // store in start publishes the completed attachment to its Acquire read.
        let slot = unsafe { &mut *cb.stream.get() };
        if slot.is_some() || output.is_none() { return Err("output attachment is not available".into()); }
        *slot = output.take();
        self.streaming=true;
        Ok(())
    }
    pub(super) fn release(&mut self) -> Result<(), String> {
        self.close()?;
        self.stop_server()?;
        #[cfg(target_os="macos")]
        if let Some(aggregate)=self.aggregate.as_mut(){aggregate.release()?;}
        Ok(())
    }
    fn stop_server(&mut self) -> Result<(), String> {
        if let Some(server) = self.server.as_mut() {
            if server.try_wait().map_err(|e| e.to_string())?.is_none() {
                #[cfg(unix)]
                {
                    let status = Command::new("/bin/kill")
                        .args(["-TERM", &server.id().to_string()])
                        .status()
                        .map_err(|e| e.to_string())?;
                    if !status.success() {
                        return Err("could not request owned JACK shutdown".into());
                    }
                }
                #[cfg(not(unix))]
                server.kill().map_err(|e| e.to_string())?;
                let deadline = Instant::now() + Duration::from_secs(2);
                while server.try_wait().map_err(|e| e.to_string())?.is_none() {
                    if Instant::now() >= deadline {
                        server.kill().map_err(|e| e.to_string())?;
                        server.wait().map_err(|e| e.to_string())?;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
        self.server.take();
        Ok(())
    }
    fn close(&mut self) -> Result<(), String> {
        if !self.client.is_null() {
            unsafe { (self.api.deactivate)(self.client) };
            let code = unsafe { (self.api.close)(self.client) };
            if code != 0 {
                return Err(format!("JACK close failed: {code}"));
            }
            self.client = std::ptr::null_mut();
        }
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if self.close().is_err() {
            // Preserve both callback data and native code if JACK cannot confirm release.
            if let Some(c) = self.callback.take() {
                Box::leak(c);
            }
            if let Some(l) = self.library.take() {
                std::mem::forget(l);
            }
        }
        if self.stop_server().is_err() {
            if let Some(mut server) = self.server.take() {
                let _ = server.kill();
                let _ = server.wait();
            }
        }
    }
}

fn server_exit_error(root:&Path,code:&str)->String{
    use std::io::Read;
    let path=root.join("jack-server.log");let mut text=String::new();
    if let Ok(file)=std::fs::File::open(&path){let _=file.take(8192).read_to_string(&mut text);}
    let detail=text.lines().find(|line|["Too many","Failed","Cannot","Error","ERROR"].iter().any(|word|line.contains(word)))
        .unwrap_or("no further startup detail").chars().take(240).collect::<String>();
    format!("JACK server exited {code}: {detail}; log: {}",path.display())
}
#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "C" fn buffer(port: *mut c_void, _n: u32) -> *mut c_void {
        port
    }
    #[test]
    fn callback_is_silent_before_go_and_finishes_finite_samples() {
        let mut left = [99.0f32; 8];
        let mut right = [99.0f32; 8];
        let mut input = [0.0f32; 8];
        let s = Callback {
            sample_rate: 48000.,
            stream: UnsafeCell::new(None),
            buffer,
            outputs: [left.as_mut_ptr().cast(), right.as_mut_ptr().cast()],
            monitor: input.as_mut_ptr().cast(),
            physical_inputs:Vec::new().into_boxed_slice(),
            samples: vec![0.01, 0.02, 0.0, -0.01, -0.02].into_boxed_slice(),
            rt: UnsafeCell::new(Rt { cursor: 0, tail: 0 }),
            go: AtomicBool::new(false),
            done: AtomicBool::new(false),
            callbacks: AtomicU64::new(0),
            produced: AtomicU64::new(0),
            xruns: AtomicU64::new(0),
            peak: AtomicU32::new(0),
            invalid: AtomicBool::new(false),
        };
        let p = &s as *const Callback as *mut c_void;
        unsafe { process(8, p) };
        assert_eq!(left, [0.0; 8]);
        assert_eq!(right, left);
        assert_eq!(s.produced.load(Ordering::Relaxed), 0);
        s.go.store(true, Ordering::Release);
        unsafe { process(8, p) };
        assert_eq!(&left[..5], &[0.01, 0.02, 0.0, -0.01, -0.02]);
        assert_eq!(&left[5..], &[0.0; 3]);
        assert_eq!(right, left);
        for _ in 0..4 {
            unsafe { process(8, p) };
        }
        assert!(s.done.load(Ordering::Acquire));
        assert_eq!(left, [0.0; 8]);
        assert_eq!(s.produced.load(Ordering::Relaxed), 5);
        unsafe { xrun(p) };
        assert_eq!(s.xruns.load(Ordering::Relaxed), 1);
    }
}

#[cfg(test)]mod launch_failure_tests{
 use super::*;
 #[test]fn startup_error_uses_actual_output_root_and_server_reason(){
  let p=std::env::temp_dir().join(format!("wormwood-jack-error-{}",std::process::id()));std::fs::create_dir_all(&p).unwrap();
  std::fs::write(p.join("jack-server.log"),"Too many servers already active\nFailed to open server\n").unwrap();
  let error=server_exit_error(&p,"255");assert!(error.contains("Too many servers already active"));assert!(error.contains(&p.join("jack-server.log").display().to_string()));
  std::fs::remove_file(p.join("jack-server.log")).unwrap();std::fs::remove_dir(&p).unwrap();
 }
}

// A single CoreAudio device bypasses JACK2's legacy aggregate creation path.
// Explicit input mapping preserves the selected microphone even when the
// selected output interface also exposes its own input channels.
fn coreaudio_device_args(output:&str,input:Option<&str>,aggregate:Option<(&str,u32,u32)>)->Vec<String>{
    if let Some((uid,start,count))=aggregate {
        return vec!["-d".into(),uid.into(),"-i".into(),count.to_string(),"-n".into(),
            (start..start+count).map(|i|i.to_string()).collect::<Vec<_>>().join(" ")];
    }
    let mut args=vec!["-P".into(),output.into()];
    if let Some(input)=input{args.extend(["-C".into(),input.into()]);}
    else{args.extend(["-i".into(),"0".into()]);}
    args
}
#[cfg(test)]
mod duplex_routing_tests {
    use super::coreaudio_device_args;
    #[test]
    fn selected_microphone_skips_inputs_on_output_interface(){
        assert_eq!(coreaudio_device_args("speakers",Some("mic"),Some(("owned",4,2))),
            ["-d","owned","-i","2","-n","4 5"]);
    }
    #[test]
    fn single_device_and_output_only_preserve_explicit_selection(){
        assert_eq!(coreaudio_device_args("usb",Some("usb"),None),["-P","usb","-C","usb"]);
        assert_eq!(coreaudio_device_args("speakers",None,None),["-P","speakers","-i","0"]);
    }
}
