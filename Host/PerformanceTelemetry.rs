//! Opt-in adaptation of Cardinal's PerformanceTelemetry and DebugHud counters.
//! CPU means actual calling-thread CPU time, with exclusive nested attribution.
//! Fixed counters and a fixed TLS stack keep callback instrumentation allocation-
//! free and lock-free. JSON, files and environment access stay on the Host thread.
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    marker::PhantomData,
    rc::Rc,
    sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub(crate) enum Category {
    GameLogic,
    GameGraphics,
    DawGraphics,
    Audio,
}
const NAMES: [&str; 4] = ["GameLogic", "GameGraphics", "DawGraphics", "Audio"];
const BACKENDS: [&str; 3] = ["OpenGL", "Vulkan", "WebGPU"];
const STACK_LIMIT: usize = 64;
static ENABLED: AtomicBool = AtomicBool::new(false);
static CONFIGURED: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU64 = AtomicU64::new(1);
static CPU_NS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
static CPU_SCOPES: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
static CPU_FAILURES: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
static STACK_FAILURES: AtomicU64 = AtomicU64::new(0);
static AUDIO_NS: AtomicU64 = AtomicU64::new(0);
static AUDIO_BUDGET_NS: AtomicU64 = AtomicU64::new(0);
static AUDIO_BLOCKS: AtomicU64 = AtomicU64::new(0);
static AUDIO_LATE: AtomicU64 = AtomicU64::new(0);
static AUDIO_PEAK_PERMILLE: AtomicU64 = AtomicU64::new(0);
static XRUNS: AtomicU64 = AtomicU64::new(0);
static XRUN_GENERATION: AtomicU64 = AtomicU64::new(0);
static GPU: [GpuCounters; 3] = [const { GpuCounters::new() }; 3];

struct GpuCounters {
    ns: [AtomicU64; 4],
    samples: AtomicU64,
    incomplete: AtomicU64,
    unsupported: AtomicU64,
    support_generation: AtomicU64,
    method: AtomicU64,
}
impl GpuCounters {
    const fn new() -> Self {
        Self {
            ns: [const { AtomicU64::new(0) }; 4],
            samples: AtomicU64::new(0),
            incomplete: AtomicU64::new(0),
            unsupported: AtomicU64::new(0),
            support_generation: AtomicU64::new(0),
            method: AtomicU64::new(0),
        }
    }
}

pub(crate) fn set_enabled(value: bool) {
    CONFIGURED.store(true, Relaxed);
    if ENABLED.swap(value, Relaxed) != value {
        GENERATION.fetch_add(1, Relaxed);
    }
}
pub(crate) fn enabled() -> bool {
    ENABLED.load(Relaxed)
}
pub(crate) fn generation() -> u64 {
    GENERATION.load(Relaxed)
}

// Apple clock IDs are from the platform SDK's _time.h; Linux/Android use the
// public clock_gettime ABI. Other targets report unsupported, never wall time.
#[cfg(all(
    target_pointer_width = "64",
    any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android"
    )
))]
fn cpu_now() -> Option<u64> {
    use std::ffi::{c_int, c_long};
    #[repr(C)]
    struct Timespec {
        seconds: c_long,
        nanoseconds: c_long,
    }
    unsafe extern "C" {
        fn clock_gettime(clock: c_int, time: *mut Timespec) -> c_int;
    }
    let clock = if cfg!(any(target_os = "macos", target_os = "ios")) {
        16
    } else {
        3
    };
    let mut time = Timespec {
        seconds: 0,
        nanoseconds: 0,
    };
    // SAFETY: this matches clock_gettime's platform C ABI and writable timespec.
    if unsafe { clock_gettime(clock, &mut time) } != 0
        || time.seconds < 0
        || !(0..1_000_000_000).contains(&time.nanoseconds)
    {
        return None;
    }
    (time.seconds as u64)
        .checked_mul(1_000_000_000)?
        .checked_add(time.nanoseconds as u64)
}
#[cfg(not(all(
    target_pointer_width = "64",
    any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android"
    )
)))]
fn cpu_now() -> Option<u64> {
    None
}

#[derive(Clone, Copy)]
struct Frame {
    start: u64,
    children: u64,
    generation: u64,
    category: Category,
}
const EMPTY: Frame = Frame {
    start: 0,
    children: 0,
    generation: 0,
    category: Category::GameLogic,
};
struct ThreadStack {
    frames: [Frame; STACK_LIMIT],
    len: usize,
}
#[derive(Debug, PartialEq, Eq)]
enum FinishError {
    Order,
    Clock,
}
impl ThreadStack {
    fn push(&mut self, start: u64, epoch: u64, category: Category) -> Option<usize> {
        let index = self.len;
        if index == STACK_LIMIT {
            return None;
        }
        self.frames[index] = Frame {
            start,
            children: 0,
            generation: epoch,
            category,
        };
        self.len += 1;
        Some(index)
    }
    fn finish(&mut self, index: usize, end: Option<u64>) -> Result<(u64, u64), FinishError> {
        if self.len != index + 1 {
            return Err(FinishError::Order);
        }
        let frame = self.frames[index];
        self.len -= 1;
        let inclusive = end
            .and_then(|end| end.checked_sub(frame.start))
            .ok_or(FinishError::Clock)?;
        if index > 0 {
            self.frames[index - 1].children =
                self.frames[index - 1].children.saturating_add(inclusive);
        }
        Ok((frame.generation, inclusive.saturating_sub(frame.children)))
    }
}
fn accepts_generation(active: bool, sample: u64, current: u64) -> bool {
    active && sample == current
}
thread_local! {
    static STACK: RefCell<ThreadStack> = const { RefCell::new(ThreadStack { frames: [EMPTY; STACK_LIMIT], len: 0 }) };
}

/// RAII scopes must remain on their creating thread and unwind in LIFO order.
pub(crate) struct Scope {
    index: Option<usize>,
    category: Category,
    marker: PhantomData<Rc<()>>,
}
impl Scope {
    pub(crate) fn new(category: Category) -> Self {
        let mut scope = Self {
            index: None,
            category,
            marker: PhantomData,
        };
        if !enabled() {
            return scope;
        }
        let Some(start) = cpu_now() else {
            CPU_FAILURES[category as usize].fetch_add(1, Relaxed);
            return scope;
        };
        let epoch = generation();
        STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let Some(index) = stack.push(start, epoch, category) else {
                STACK_FAILURES.fetch_add(1, Relaxed);
                return;
            };
            scope.index = Some(index);
        });
        scope
    }
    pub(crate) fn inclusive_cpu_ns(&self)->Option<u64>{
        let index=self.index?;let now=cpu_now()?;
        STACK.with(|cell|{
            let stack=cell.borrow();let frame=stack.frames.get(index)?;
            if stack.len<=index || frame.generation!=generation(){return None;}
            now.checked_sub(frame.start)
        })
    }

}
impl Drop for Scope {
    fn drop(&mut self) {
        let Some(index) = self.index else {
            return;
        };
        let end = cpu_now();
        STACK.with(|cell| {
            let mut stack = cell.borrow_mut();
            let (epoch, exclusive) = match stack.finish(index, end) {
                Ok(sample) => sample,
                Err(FinishError::Order) => {
                    STACK_FAILURES.fetch_add(1, Relaxed);
                    return;
                }
                Err(FinishError::Clock) => {
                    CPU_FAILURES[self.category as usize].fetch_add(1, Relaxed);
                    return;
                }
            };
            if accepts_generation(enabled(), epoch, generation()) {
                CPU_NS[self.category as usize].fetch_add(exclusive, Relaxed);
                CPU_SCOPES[self.category as usize].fetch_add(1, Relaxed);
            }
        });
    }
}

pub(crate) fn classify(target: &str, daw_only: bool) -> Category {
    if [
        "JackConnectionSystem",
        "IOSAudioConnectionSystem",
        "ChuckConnectionSystem",
        "RayTracedAudioSystem",
        "SoundPhysicsSystem",
        "EnvironmentAudioSystem",
        "BirdSystem",
    ]
    .iter()
    .any(|name| target.starts_with(name))
    {
        return Category::Audio;
    }
    if daw_only
        || [
            "AudioWorkstationSystem",
            "WorkstationPanelSystem",
            "WorkstationDataSystem",
        ]
        .iter()
        .any(|name| target.starts_with(name))
    {
        return Category::DawGraphics;
    }
    if [
        "Graphics",
        "Appearance",
        "Render",
        "Mesh",
        "Cloud",
        "ColorEmotion",
        "PlayerRig",
        "GameFrame",
    ]
    .iter()
    .any(|name| target.contains(name))
    {
        Category::GameGraphics
    } else {
        Category::GameLogic
    }
}

/// Actual GPU interval only. None + supported marks an incomplete observation;
/// None + unsupported marks absent capability. Submission epochs reject stale
/// query completions after a benchmark's warmup boundary.
pub(crate) fn gpu_record(
    backend: &str,
    epoch: u64,
    duration_ns: Option<u64>,
    supported: bool,
    method: &str,
    category: &str,
) {
    if !accepts_generation(enabled(), epoch, generation()) {
        return;
    }
    let Some(index) = BACKENDS
        .iter()
        .position(|name| name.eq_ignore_ascii_case(backend))
    else {
        return;
    };
    let counters = &GPU[index];
    if !supported {
        counters.unsupported.fetch_add(1, Relaxed);
        return;
    }
    counters.support_generation.store(epoch, Relaxed);
    counters.method.store(
        match method {
            "async_timestamp" => 2,
            "synchronous_timestamp" => 3,
            _ => 1,
        },
        Relaxed,
    );
    if let Some(duration) = duration_ns {
        let category = if category == "DawGraphics" {
            Category::DawGraphics
        } else {
            Category::GameGraphics
        };
        counters.ns[category as usize].fetch_add(duration, Relaxed);
        counters.samples.fetch_add(1, Relaxed);
    } else {
        counters.incomplete.fetch_add(1, Relaxed);
    }
}

/// Callback work and its deadline are separate from thread CPU usage. Clock
/// calls and fixed atomic operations are the only enabled-path instrumentation.
pub(crate) struct AudioScope {
    epoch: u64,
    budget: u64,
    start: Option<Instant>,
    _cpu: Scope,
}
impl AudioScope {
    pub(crate) fn new(frames: u32, sample_rate: f64) -> Self {
        let active = enabled();
        let budget = if active && sample_rate.is_finite() && sample_rate > 0. {
            (frames as f64 * 1e9 / sample_rate).min(u64::MAX as f64) as u64
        } else {
            0
        };
        Self {
            epoch: generation(),
            budget,
            start: (budget > 0).then(Instant::now),
            _cpu: Scope::new(Category::Audio),
        }
    }
}
impl Drop for AudioScope {
    fn drop(&mut self) {
        let Some(start) = self.start else {
            return;
        };
        if !accepts_generation(enabled(), self.epoch, generation()) {
            return;
        }
        let elapsed = start.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        AUDIO_NS.fetch_add(elapsed, Relaxed);
        AUDIO_BUDGET_NS.fetch_add(self.budget, Relaxed);
        AUDIO_BLOCKS.fetch_add(1, Relaxed);
        let (permille, late) = audio_load(elapsed, self.budget).unwrap();
        if late {
            AUDIO_LATE.fetch_add(1, Relaxed);
        }
        AUDIO_PEAK_PERMILLE.fetch_max(permille, Relaxed);
    }
}
pub(crate) fn mark_xrun_supported() {
    if enabled() {
        XRUN_GENERATION.store(generation(), Relaxed);
    }
}
pub(crate) fn record_xrun() {
    if enabled() {
        mark_xrun_supported();
        XRUNS.fetch_add(1, Relaxed);
    }
}

#[derive(Clone, Copy, Default)]
struct GpuSnapshot {
    ns: [u64; 4],
    samples: u64,
    incomplete: u64,
    unsupported: u64,
}
#[derive(Clone, Copy, Default)]
struct Snapshot {
    cpu: [u64; 4],
    scopes: [u64; 4],
    failures: [u64; 4],
    stack_failures: u64,
    audio_ns: u64,
    audio_budget: u64,
    blocks: u64,
    late: u64,
    xruns: u64,
    gpu: [GpuSnapshot; 3],
}
fn snapshot() -> Snapshot {
    Snapshot {
        cpu: std::array::from_fn(|i| CPU_NS[i].load(Relaxed)),
        scopes: std::array::from_fn(|i| CPU_SCOPES[i].load(Relaxed)),
        failures: std::array::from_fn(|i| CPU_FAILURES[i].load(Relaxed)),
        stack_failures: STACK_FAILURES.load(Relaxed),
        audio_ns: AUDIO_NS.load(Relaxed),
        audio_budget: AUDIO_BUDGET_NS.load(Relaxed),
        blocks: AUDIO_BLOCKS.load(Relaxed),
        late: AUDIO_LATE.load(Relaxed),
        xruns: XRUNS.load(Relaxed),
        gpu: std::array::from_fn(|i| GpuSnapshot {
            ns: std::array::from_fn(|c| GPU[i].ns[c].load(Relaxed)),
            samples: GPU[i].samples.load(Relaxed),
            incomplete: GPU[i].incomplete.load(Relaxed),
            unsupported: GPU[i].unsupported.load(Relaxed),
        }),
    }
}
fn delta(value: u64, previous: u64) -> Option<u64> {
    value.checked_sub(previous)
}
fn seconds(ns: Option<u64>) -> Option<f64> {
    ns.map(|n| n as f64 / 1e9)
}
fn percent(numerator: Option<u64>, denominator: f64) -> Option<f64> {
    numerator
        .filter(|_| denominator > 0. && denominator.is_finite())
        .map(|n| 100. * n as f64 / denominator)
}
fn cpu_metrics(supported: bool, ns: Option<u64>, wall_ns: f64) -> Value {
    let ns = supported.then_some(ns).flatten();
    json!({"thread_cpu_supported":supported,"cpu_seconds":seconds(ns),"cpu_per_core_percent":percent(ns,wall_ns)})
}
fn audio_load(elapsed: u64, budget: u64) -> Option<(u64, bool)> {
    if budget == 0 {
        return None;
    }
    Some((
        ((elapsed as u128 * 1000) / budget as u128).min(u64::MAX as u128) as u64,
        elapsed > budget,
    ))
}

/// Control-thread reporting window. Counters are process-wide, matching the
/// single active Host application session; snapshots are not audio barriers.
pub(crate) struct Telemetry {
    started: Option<Instant>,
    baseline: Snapshot,
    epoch: u64,
    cpu_supported: bool,
    last_report: Cell<Option<Instant>>,
    pub(crate) daw_only: bool,
}
impl Telemetry {
    pub(crate) fn new() -> Self {
        if !CONFIGURED.load(Relaxed)
            && std::env::var("WORMWOOD_PERFORMANCE").is_ok_and(|v| v == "1")
        {
            set_enabled(true);
        }
        let mut telemetry = Self {
            started: None,
            baseline: Snapshot::default(),
            epoch: generation(),
            cpu_supported: false,
            last_report: Cell::new(None),
            daw_only: false,
        };
        if enabled() {
            telemetry.reset();
        }
        telemetry
    }
    pub(crate) fn reset(&mut self) {
        self.epoch = GENERATION.fetch_add(1, Relaxed).wrapping_add(1);
        self.baseline = snapshot();
        AUDIO_PEAK_PERMILLE.store(0, Relaxed);
        self.cpu_supported = cpu_now().is_some();
        self.started = enabled().then(Instant::now);
        self.last_report.set(None);
    }
    pub(crate) fn report_due(&self) -> bool {
        enabled()
            && self
                .last_report
                .get()
                .is_none_or(|last| last.elapsed().as_millis() >= 500)
    }
    pub(crate) fn report_written(&self) {
        self.last_report.set(Some(Instant::now()));
    }
    pub(crate) fn scope(&self, target: &str) -> Scope {
        if !enabled() {
            return Scope::new(Category::GameLogic);
        }
        let daw_context = self.daw_only
            || STACK.with(|cell| {
                let stack = cell.borrow();
                stack.len > 0 && stack.frames[stack.len - 1].category == Category::DawGraphics
            });
        Scope::new(classify(target, daw_context))
    }
    pub(crate) fn report(&self) -> Value {
        let current = snapshot();
        let wall = self.started.map(|start| start.elapsed().as_secs_f64());
        let valid = enabled() && self.started.is_some() && self.epoch == generation();
        let wall_ns = wall.unwrap_or(0.) * 1e9;
        let stack_failures = delta(current.stack_failures, self.baseline.stack_failures);
        let all_clocks_ok =
            (0..4).all(|i| delta(current.failures[i], self.baseline.failures[i]) == Some(0));
        let mut categories = serde_json::Map::new();
        for (i, name) in NAMES.iter().enumerate() {
            let failures = delta(current.failures[i], self.baseline.failures[i]);
            let supported =
                valid && self.cpu_supported && all_clocks_ok && stack_failures == Some(0);
            let mut metrics = cpu_metrics(
                supported,
                delta(current.cpu[i], self.baseline.cpu[i]),
                wall_ns,
            );
            metrics["completed_scopes"] = json!(delta(current.scopes[i], self.baseline.scopes[i]));
            metrics["clock_failures"] = json!(failures);
            categories.insert((*name).into(), metrics);
        }
        let mut gpu = serde_json::Map::new();
        for (i, backend) in BACKENDS.iter().enumerate() {
            let a = current.gpu[i];
            let b = self.baseline.gpu[i];
            let samples = delta(a.samples, b.samples);
            let incomplete = delta(a.incomplete, b.incomplete);
            let unavailable = delta(a.unsupported, b.unsupported);
            let observed = samples.is_some_and(|n| n > 0)
                || incomplete.is_some_and(|n| n > 0)
                || unavailable.is_some_and(|n| n > 0);
            let supported =
                (valid && observed).then(|| GPU[i].support_generation.load(Relaxed) == self.epoch);
            let mut times = serde_json::Map::new();
            for (category, name) in NAMES.iter().enumerate() {
                let ns = (supported == Some(true))
                    .then(|| delta(a.ns[category], b.ns[category]))
                    .flatten();
                times.insert((*name).into(),json!({"sampled_gpu_seconds":seconds(ns),
                    "timestamp_time_per_wall_percent":if incomplete==Some(0)&&unavailable==Some(0)&&samples.unwrap_or(0)>0 {percent(ns,wall_ns)}else{None}}));
            }
            gpu.insert((*backend).into(),json!({"supported":supported,"completed_samples":samples,"incomplete_samples":incomplete,
                "unsupported_observations":unavailable,"method":if supported==Some(true){match GPU[i].method.load(Relaxed){1=>Some("timestamp_query"),2=>Some("async_timestamp"),3=>Some("synchronous_timestamp"),_=>None}}else{None},"categories":times,"gpu_active_percent":Value::Null}));
        }
        let blocks = delta(current.blocks, self.baseline.blocks);
        let budget = delta(current.audio_budget, self.baseline.audio_budget);
        let fresh = valid && blocks.unwrap_or(0) > 0 && budget.unwrap_or(0) > 0;
        let xruns_supported = valid && XRUN_GENERATION.load(Relaxed) == self.epoch;
        json!({"schema":1,"enabled":enabled(),"generation":self.epoch,"window_valid":valid,"wall_seconds":wall,"measurement_window":"since_reset_completed_scopes",
            "categories":categories,"gpu":gpu,"stack_capacity":STACK_LIMIT,"stack_failures":stack_failures,
            "audio":{"observed_callbacks":blocks,"callback_wall_seconds":if fresh {seconds(delta(current.audio_ns,self.baseline.audio_ns))}else{None},
                "deadline_seconds":if fresh {seconds(budget)}else{None},"deadline_percent":if fresh {percent(delta(current.audio_ns,self.baseline.audio_ns),budget.unwrap() as f64)}else{None},
                "peak_deadline_percent":if fresh {Some(AUDIO_PEAK_PERMILLE.load(Relaxed) as f64/10.)}else{None},
                "late_blocks":if fresh {delta(current.late,self.baseline.late)}else{None},"xruns_supported":xruns_supported,
                "xruns":if xruns_supported {delta(current.xruns,self.baseline.xruns)}else{None}},
            "semantics":{"cpu":"Exclusive CPU seconds from CLOCK_THREAD_CPUTIME_ID for completed instrumented scopes; nested scopes are subtracted. One fully used core is 100%; multiple instrumented threads can exceed 100%. Sleeps and GPU waits are excluded.",
                "coverage":"Instrumented Host calls and native audio callbacks only; uninstrumented worker threads and process-wide CPU are not inferred.",
                "window":"Process-wide opt-in counters; snapshots are live atomic observations, not cross-thread barriers. Reset discards scopes/query completions started in older generations.",
                "gpu":"Actual completed timestamp intervals only. Summed interval/wall ratio is not shader occupancy or an overlap-unioned GPU utilization measurement; gpu_active_percent stays null.",
                "audio":"Callback wall time divided by frames/sample-rate budget; late blocks are not xruns. Xruns require an actual backend notification.",
                "bounded":"Fixed counters and a 64-scope TLS stack; no per-invocation history. Counter wrap or clock failure produces null metrics."}})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn stack() -> ThreadStack {
        ThreadStack {
            frames: [EMPTY; STACK_LIMIT],
            len: 0,
        }
    }

    #[test]
    fn performance_exclusive_nesting_counts_each_cpu_nanosecond_once() {
        let mut stack = stack();
        let parent = stack.push(10, 7, Category::DawGraphics).unwrap();
        let child = stack.push(20, 7, Category::Audio).unwrap();
        let (_, child_cpu) = stack.finish(child, Some(50)).unwrap();
        let sibling = stack.push(60, 7, Category::DawGraphics).unwrap();
        let (_, sibling_cpu) = stack.finish(sibling, Some(80)).unwrap();
        let (_, parent_cpu) = stack.finish(parent, Some(100)).unwrap();
        assert_eq!((parent_cpu, child_cpu, sibling_cpu), (40, 30, 20));
        assert_eq!(parent_cpu + child_cpu + sibling_cpu, 90);
        assert_eq!(stack.len, 0);
    }

    #[test]
    fn performance_generation_reset_rejects_outer_scope_and_delayed_gpu_sample() {
        let mut stack = stack();
        let old = stack.push(10, 3, Category::DawGraphics).unwrap();
        let measured = stack.push(30, 4, Category::DawGraphics).unwrap();
        let sample = stack.finish(measured, Some(60)).unwrap();
        let previous = stack.finish(old, Some(80)).unwrap();
        assert!(accepts_generation(true, sample.0, 4));
        assert_eq!(sample.1, 30);
        assert!(!accepts_generation(true, previous.0, 4));
        assert!(!accepts_generation(true, 3, 4));
        assert!(!accepts_generation(false, 4, 4));
    }

    #[test]
    fn performance_cpu_percent_means_one_core_and_unsupported_is_null() {
        let supported = cpu_metrics(true, Some(3_000_000_000), 1_000_000_000.);
        assert_eq!(supported["cpu_seconds"], 3.);
        assert_eq!(supported["cpu_per_core_percent"], 300.);
        let unsupported = cpu_metrics(false, Some(3_000_000_000), 1_000_000_000.);
        assert_eq!(unsupported["thread_cpu_supported"], false);
        assert!(unsupported["cpu_seconds"].is_null());
        assert!(unsupported["cpu_per_core_percent"].is_null());
        assert!(cpu_metrics(true, None, 1_000_000_000.)["cpu_seconds"].is_null());
        assert!(cpu_metrics(true, Some(12), 0.)["cpu_per_core_percent"].is_null());
        assert_eq!(delta(3, u64::MAX), None);
    }

    #[test]
    fn performance_audio_deadlines_keep_load_peak_and_lateness_distinct() {
        assert_eq!(audio_load(2_500_000, 10_000_000), Some((250, false)));
        assert_eq!(audio_load(10_000_000, 10_000_000), Some((1000, false)));
        assert_eq!(audio_load(12_500_000, 10_000_000), Some((1250, true)));
        assert_eq!(audio_load(12_500_000, 0), None);
        assert_eq!(percent(Some(15_000_000), 20_000_000.), Some(75.));
        assert_eq!(audio_load(u64::MAX, 1), Some((u64::MAX, true)));
    }

    #[test]
    fn performance_scope_stack_is_bounded_and_clock_errors_are_explicit() {
        let mut stack = stack();
        for i in 0..STACK_LIMIT {
            assert_eq!(stack.push(i as u64, 1, Category::GameLogic), Some(i));
        }
        assert_eq!(stack.push(65, 1, Category::GameLogic), None);
        assert_eq!(stack.len, STACK_LIMIT);
        assert_eq!(stack.finish(0, Some(100)), Err(FinishError::Order));
        assert_eq!(stack.finish(STACK_LIMIT - 1, None), Err(FinishError::Clock));
        assert_eq!(stack.len, STACK_LIMIT - 1);
    }

    #[test]
    fn performance_category_selection_keeps_audio_distinct_in_daw_mode() {
        assert_eq!(
            classify("GraphicsConnectionSystem.Pump", false),
            Category::GameGraphics
        );
        assert_eq!(
            classify("GraphicsConnectionSystem.Pump", true),
            Category::DawGraphics
        );
        assert_eq!(
            classify("JackConnectionSystem.Prepare", true),
            Category::Audio
        );
        assert_eq!(
            classify("PhysicsSystem.GameStep", false),
            Category::GameLogic
        );
    }
}
