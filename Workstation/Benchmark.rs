//! Fixed-input workstation replay and bounded rendered-frame measurements.
use serde_json::{Value, json};
#[path="BenchmarkCamera.rs"] pub(crate) mod camera;
#[path="BenchmarkMotionComparison.rs"] pub(crate) mod motion_comparison;
#[path="BenchmarkMotionInput.rs"] pub(crate) mod motion_input;

/// Only the explicit finite benchmark entry point consults these overrides.
/// Legacy replay remains four warmup frames and one four-phase sequence.
pub(crate) fn request_from_environment(frames: u32) -> Result<Value, String> {
    let mut input = request(frames,
        std::env::var("WORMWOOD_WORKSTATION_BENCHMARK_WARMUP").ok().as_deref(),
        std::env::var("WORMWOOD_WORKSTATION_BENCHMARK_CYCLE").ok().as_deref())?;
    input["benchmark"]["audio"] = json!(match std::env::var("WORMWOOD_WORKSTATION_BENCHMARK_AUDIO").ok().as_deref() {
        None | Some("0") => false,
        Some("1") => true,
        _ => return Err("WORMWOOD_WORKSTATION_BENCHMARK_AUDIO must be 0 or 1".into()),
    });
    input["benchmark"]["idle"] = json!(match std::env::var("WORMWOOD_WORKSTATION_BENCHMARK_SCENARIO").ok().as_deref() {
        None | Some("motion") => false,
        Some("idle") => true,
        _ => return Err("WORMWOOD_WORKSTATION_BENCHMARK_SCENARIO must be motion or idle".into()),
    });
    input["benchmark"]["bake_settle_replay"] = json!(match std::env::var("WORMWOOD_WORKSTATION_BAKE_SETTLE_REPLAY").ok().as_deref(){None|Some("0")=>false,Some("1")=>true,_=>return Err("WORMWOOD_WORKSTATION_BAKE_SETTLE_REPLAY must be 0 or 1".into())});
    if input["benchmark"]["idle"] == true { input["benchmark"]["cycle_frames"] = Value::Null; }
    let minimum_seconds = std::env::var("WORMWOOD_WORKSTATION_BENCHMARK_MIN_SECONDS").ok()
        .map(|value| value.parse::<f64>().ok().filter(|seconds| seconds.is_finite() && (0.0..=120.).contains(seconds))
            .ok_or("WORMWOOD_WORKSTATION_BENCHMARK_MIN_SECONDS must be finite within 0..120"))
        .transpose()?.unwrap_or(0.);
    apply_duration(&mut input, minimum_seconds);
    Ok(input)
}

fn apply_duration(input: &mut Value, minimum_seconds: f64) {
    input["benchmark"]["minimum_seconds"] = json!(minimum_seconds);
    if minimum_seconds > 0. {
        input["benchmark"]["maximum_measured_frames"] = json!(20_000);
        input["frames"] = json!(20_000 + input["benchmark"]["warmup_frames"].as_u64().unwrap());
        if input["benchmark"]["idle"] != true && input["benchmark"]["cycle_frames"].is_null() {
            input["benchmark"]["cycle_frames"] = json!(32);
        }
    }
}

fn request(frames: u32, warmup: Option<&str>, cycle: Option<&str>) -> Result<Value, String> {
    let warmup = match warmup {
        None => 4,
        Some(value) => value.parse::<u32>().ok().filter(|n| (1..=120).contains(n))
            .ok_or("WORMWOOD_WORKSTATION_BENCHMARK_WARMUP must be 1..120")?,
    };
    let cycle = cycle.map(|value| value.parse::<u32>().ok()
        .filter(|n| (8..=600).contains(n) && n % 4 == 0)
        .ok_or("WORMWOOD_WORKSTATION_BENCHMARK_CYCLE must be a multiple of four in 8..600"))
        .transpose()?;
    Ok(json!({"frames":frames+warmup,"benchmark":{
        "warmup_frames":warmup,"measured_frames":frames,"cycle_frames":cycle}}))
}

pub(crate) fn augment_gpu(
    report: &mut Value,
    output: &std::path::Path,
    backend: &str,
) -> Result<(), String> {
    let path = output.join(format!("performance-gpu-{backend}.json"));
    if !path.exists() {
        report["gpu_measurement"] = json!({"supported":null,"complete":false,"reason":"No GPU query report was produced; CPU timing is not a substitute."});
        return Ok(());
    }
    let gpu: Value = serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let warmup = report["benchmark"]["warmup_frames"]
        .as_u64()
        .ok_or("missing benchmark warmup")?;
    let count = report["benchmark"]["measured_frames"].as_u64()
        .or_else(|| report["benchmark"]["requested_measured_frames"].as_u64())
        .ok_or("missing benchmark frame count")?;
    let offset=report["benchmark"]["gpu_draw_offset"].as_u64().unwrap_or(0);
    let mut indices = std::collections::BTreeSet::new();
    let mut values = Vec::new();
    if let Some(samples) = gpu["samples"].as_array() {
        for sample in samples {
            let draw = sample["draw"].as_u64().ok_or("invalid GPU draw index")?;
            if draw > offset + warmup && draw <= offset + warmup + count {
                if !indices.insert(draw) {
                    return Err("duplicate GPU draw index".into());
                }
                let ms = sample["gpu_ms"]
                    .as_f64()
                    .filter(|v| v.is_finite() && *v >= 0.)
                    .ok_or("invalid GPU duration")?;
                values.push(ms);
            }
        }
    }
    values.sort_by(f64::total_cmp);
    let percentile = |p: f64| {
        values
            .get(((values.len().saturating_sub(1)) as f64 * p).round() as usize)
            .copied()
    };
    report["gpu_measurement"] = json!({"supported":gpu["supported"],
        "complete":count>0 && gpu["supported"]==true && values.len() as u64==count,
        "measured_samples":values.len(),"expected_samples":count,"warmup_excluded":true,
        "scope":gpu["scope"],"method":gpu["method"],
        "gpu_ms_mean":(!values.is_empty()).then(||values.iter().sum::<f64>()/values.len() as f64),
        "gpu_ms_p50":percentile(0.5),"gpu_ms_p95":percentile(0.95),"gpu_ms_p99":percentile(0.99),
        "gpu_ms_worst":values.last()});
    report["gpu"] = gpu;
    Ok(())
}

pub(crate) struct Benchmark {
    pub warmup: u64,
    pub measured: u64,
    samples: Vec<Value>,
    warmup_samples: Vec<Value>,
    elapsed: Vec<f64>,
    last_phase: Option<u64>,
    dimensions: Option<[u64; 2]>,
    resized: bool,
    cycle: Option<u64>,
    device_first: Value,
    device_last: Value,
    last_window: Value,
    audio: bool,
    idle: bool,
    bake_settle_replay:bool,
    minimum_seconds: f64,
    maximum_measured: u64,
    measured_wall_seconds: Option<f64>,
}

impl Benchmark {
    pub fn parse(input: &Value, limit: Option<u64>) -> Result<Option<Self>, String> {
        let value = &input["benchmark"];
        if value.is_null() {
            return Ok(None);
        }
        let warmup = value["warmup_frames"]
            .as_u64()
            .filter(|n| (1..=120).contains(n))
            .ok_or("benchmark warmup must be 1..120 frames")?;
        let measured = value["measured_frames"]
            .as_u64()
            .filter(|n| (8..=600).contains(n))
            .ok_or("benchmark requires 8..600 measured frames")?;
        let minimum_seconds = if value["minimum_seconds"].is_null() {0.} else {
            value["minimum_seconds"].as_f64()
                .filter(|seconds| seconds.is_finite() && (0.0..=120.).contains(seconds))
                .ok_or("benchmark minimum seconds must be finite within 0..120")?
        };
        let maximum_measured = if minimum_seconds > 0. {
            value["maximum_measured_frames"].as_u64()
                .filter(|maximum| (measured..=20_000).contains(maximum))
                .ok_or("duration benchmark requires a bounded maximum of at most 20000 measured frames")?
        } else { measured };
        if limit != Some(warmup + maximum_measured) {
            return Err("benchmark frame limit does not match warmup plus measurements".into());
        }
        let cycle = if value["cycle_frames"].is_null() { None } else {
            Some(value["cycle_frames"].as_u64()
                .filter(|n| (8..=600).contains(n) && n % 4 == 0)
                .ok_or("benchmark cycle must be a multiple of four in 8..600")?)
        };
        if minimum_seconds > 0. && value["idle"] != true && cycle.is_none() {
            return Err("duration benchmark requires repeated motion or explicit idle mode".into());
        }
        let bake_settle_replay=value["bake_settle_replay"]==true;
        if bake_settle_replay&&(measured!=600||cycle.is_some()||minimum_seconds!=0.||value["idle"]==true){return Err("bake settle replay requires exactly 600 measured motion frames, no cycle or duration override".into());}
        Ok(Some(Self {
            warmup,
            measured,
            samples: Vec::with_capacity(maximum_measured as usize),
            warmup_samples: Vec::with_capacity(warmup as usize),
            elapsed: Vec::with_capacity(maximum_measured as usize),
            last_phase: None,
            dimensions: None,
            resized: false,
            cycle,
            device_first: Value::Null,
            device_last: Value::Null,
            last_window: Value::Null,
            audio: value["audio"] == true,
            idle: value["idle"] == true,
            bake_settle_replay,
            minimum_seconds,
            maximum_measured,
            measured_wall_seconds: None,
        }))
    }

    pub fn deadline_seconds(&self) -> u64 { if self.minimum_seconds > 0. {240} else {120} }

    /// Duration runs end only after both minimums and a complete motion cycle.
    pub fn duration_complete(&self, seconds: f64) -> bool {
        self.minimum_seconds > 0. && seconds >= self.minimum_seconds
            && self.elapsed.len() as u64 >= self.measured
            && self.cycle.is_none_or(|cycle| self.elapsed.len() as u64 % cycle == 0)
    }

    pub fn measured_duration(&mut self, seconds: Option<f64>) {
        self.measured_wall_seconds = seconds;
    }

    pub fn phase(&self, frame: u64) -> u64 {
        if frame < self.warmup {
            0
        } else if self.bake_settle_replay {
            // Finite opt-in validation: let a refresh start, supersede it,
            // then keep the final layout unchanged long enough to publish.
            match frame-self.warmup {0..=119=>2,120..=159=>3,160..=199=>2,_=>1}
        } else if self.idle {
            1
        } else {
            let measured = frame - self.warmup;
            match self.cycle {
                Some(cycle) => 1 + (measured % cycle) * 4 / cycle,
                None => 1 + (measured * 4 / self.measured).min(3),
            }
        }
    }

    /// Return targets only at phase boundaries; real Origami easing still owns motion.
    pub fn targets(&mut self, frame: u64) -> Option<([bool; 4], bool, bool)> {
        let phase = self.phase(frame);
        if self.last_phase == Some(phase) {
            return None;
        }
        self.last_phase = Some(phase);
        // Edge order is Top, Bottom, Left, Right.
        Some(match phase {
            0 | 1 => ([true; 4], false, false),
            2 => ([true, true, false, true], false, false),
            3 => ([true; 4], true, false),
            _ => ([true, false, true, false], false, true),
        })
    }

    pub fn observe_window(&mut self, window: &Value) {
        // Preserve the terminal false-draw evidence too. Never substitute the
        // previous successful device dimensions for the current window state.
        self.last_window = window.clone();
    }

    pub fn record(&mut self, frame: u64, update: f64, render: f64, total: f64, window: &Value) {
        let size = [
            window["width"].as_u64().unwrap_or(0),
            window["height"].as_u64().unwrap_or(0),
        ];
        if self.device_first.is_null() { self.device_first = window["device"].clone(); }
        self.device_last = window["device"].clone();
        let mut sample = json!({"draw_index":frame+1,
            "measured_index":frame.checked_sub(self.warmup),
            "phase":self.phase(frame),"frame_ms":total*1000.,"update_ms":update*1000.,
            "render_present_ms":render*1000.,"dimensions":size,
            "logical_dimensions":[window["logical_width"],window["logical_height"]],
            "ruby_response":window["device"]["workstation_shader"]["ruby_response"]});
        if !window["device"]["frame_attribution"].is_null() {
            sample["webgpu_draw_wall"] = window["device"]["frame_attribution"].clone();
        }
        if frame < self.warmup {
            self.warmup_samples.push(sample);
            return;
        }
        if self.dimensions.is_some_and(|previous| previous != size) {
            self.resized = true;
        }
        self.dimensions.get_or_insert(size);
        self.elapsed.push(total);
        self.samples.push(sample);
    }

    // Attach only to the bounded sample just recorded; never create extra frames.
    pub fn attribute_last(&mut self, frame: u64, report: Value) {
        let sample = if frame < self.warmup { self.warmup_samples.last_mut() }
            else { self.samples.last_mut() };
        if let Some(sample) = sample {
            if sample["draw_index"].as_u64()==Some(frame+1) { sample["application_wall"] = report; }
        }
    }

    pub fn timeline_last(&mut self,frame:u64,timeline:Value){
        let sample=if frame<self.warmup{self.warmup_samples.last_mut()}else{self.samples.last_mut()};
        if let Some(sample)=sample{if sample["draw_index"].as_u64()==Some(frame+1){sample["loop_timeline"]=timeline;}}
    }

    pub fn report(&self, reason: &str) -> Value {
        let mut sorted = self.elapsed.clone();
        sorted.sort_by(f64::total_cmp);
        let percentile = |p: f64| {
            sorted
                .get(((sorted.len().saturating_sub(1)) as f64 * p).round() as usize)
                .copied()
                .map(|v| v * 1000.)
        };
        let elapsed: f64 = self.elapsed.iter().sum();
        let complete = !self.resized && if self.minimum_seconds > 0. {
            matches!(reason, "benchmark_duration_complete" | "frame_limit")
                && self.elapsed.len() as u64 <= self.maximum_measured
                && self.measured_wall_seconds.is_some_and(|seconds|self.duration_complete(seconds))
        } else { reason == "frame_limit" && self.elapsed.len() as u64 == self.measured };
        json!({"schema_version":2,
            "scenario":if self.bake_settle_replay {"incremental-bake-cancel-then-settle-v1"}else if self.idle {"workstation-settled-idle-v1"}else if self.cycle.is_some() {"origami-panel-motion-repeated-v1"}else{"origami-panel-motion-v1"},
            "cycle_frames":self.cycle,"benchmark_controls_version":2,"complete":complete,
            "end_reason":reason,"last_window":self.last_window,
            "warmup_frames":self.warmup,"requested_measured_frames":self.measured,"measured_frames":self.elapsed.len(),
            "minimum_measured_seconds":self.minimum_seconds,"maximum_measured_frames":self.maximum_measured,
            "deadline_seconds":self.deadline_seconds(),"measured_loop_wall_seconds":self.measured_wall_seconds,
            "duration_scope":"Actual measured loop after warmup; both frame minimum and time minimum required; motion ends at complete cycle boundary.",
            "fixed_simulation_step_seconds":1./30.,"dimensions":self.dimensions,"resized_during_measurement":self.resized,
            "frame_ms_mean":(!sorted.is_empty()).then(||elapsed*1000./sorted.len() as f64),
            "frame_ms_p50":percentile(0.5),"frame_ms_p95":percentile(0.95),"frame_ms_p99":percentile(0.99),
            "frame_ms_worst":sorted.last().map(|v|v*1000.),
            "rendered_frames_per_second":(elapsed>0.).then(||sorted.len() as f64/elapsed),
            "timing":"Wall time for panel update plus CPU render submission/present; actual GPU timestamps are reported separately. Warmup, idle ticks and intentional sleep are excluded.",
            "quality":"Existing resolution, authored geometry/materials and optical budgets; no benchmark quality override.",
            "audio_playback_requested":self.audio,
            "device_first":self.device_first,"device_last":self.device_last,
            "workload_limits":"Panel motion/expansion and overview, plus optional native MIDI playback; mixer meters/faders are static specimens, no control gestures or OS window resize is generated.",
            "warmup_samples":self.warmup_samples,"samples":self.samples})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workstation_benchmark_preserves_nondrawable_status_without_counting_a_frame() {
        let input = request(8, Some("4"), None).unwrap();
        let mut benchmark = Benchmark::parse(&input, Some(12)).unwrap().unwrap();
        benchmark.record(0, 0.001, 0.009, 0.01, &json!({"width":640,"height":480}));
        let stopped = json!({"closed":false,"drawn":false,"width":640,"height":480,
            "draw_status":{"reason":"zero_framebuffer","recoverable":true,"attempted":false,
                "drawable_dimensions":null},"dimensions_source":"previous"});
        benchmark.observe_window(&stopped);
        let report = benchmark.report("benchmark_nondrawable");
        assert_eq!(report["complete"], false);
        assert_eq!(report["end_reason"], "benchmark_nondrawable");
        assert_eq!(report["last_window"], stopped);
        assert_eq!(report["warmup_samples"].as_array().unwrap().len(), 1);
        assert_eq!(report["measured_frames"], 0);
    }

    #[test]
    fn workstation_benchmark_gpu_warmup_uses_submission_order_not_completion_order() {
        let directory = std::env::temp_dir().join(format!(
            "workstation-gpu-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let samples: Vec<_> = (1..=12)
            .rev()
            .map(|draw| json!({"draw":draw,"gpu_ms":if draw<=4 {900.}else{10.}}))
            .collect();
        let path = directory.join("performance-gpu-WebGPU.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({"supported":true,"samples":samples})).unwrap(),
        )
        .unwrap();
        let mut report = json!({"benchmark":{"warmup_frames":4,"requested_measured_frames":8}});
        augment_gpu(&mut report, &directory, "WebGPU").unwrap();
        assert_eq!(report["gpu_measurement"]["complete"], true);
        assert_eq!(report["gpu_measurement"]["gpu_ms_p50"], 10.);
        assert_eq!(report["gpu_measurement"]["measured_samples"], 8);
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({"supported":true,"samples":[{"draw":5,"gpu_ms":10.}]}))
                .unwrap(),
        )
        .unwrap();
        augment_gpu(&mut report, &directory, "WebGPU").unwrap();
        assert_eq!(report["gpu_measurement"]["complete"], false);
        let samples=(1..=29).map(|draw|json!({"draw":draw,"gpu_ms":if draw>21 {3.}else{900.}})).collect::<Vec<_>>();
        std::fs::write(&path,serde_json::to_vec(&json!({"supported":true,"samples":samples})).unwrap()).unwrap();
        report["benchmark"]["gpu_draw_offset"]=json!(17);
        augment_gpu(&mut report,&directory,"WebGPU").unwrap();
        assert_eq!(report["gpu_measurement"]["complete"],true);
        assert_eq!(report["gpu_measurement"]["gpu_ms_p50"],3.);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
    #[test]
    fn workstation_benchmark_excludes_warmup_and_rejects_partial_or_resized_runs() {
        let input = json!({"benchmark":{"warmup_frames":4,"measured_frames":8}});
        let mut b = Benchmark::parse(&input, Some(12)).unwrap().unwrap();
        for n in 0..12 {
            b.record(n, 0.002, 0.008, 0.01, &json!({"width":640,"height":480}));
        }
        let report = b.report("frame_limit");
        assert_eq!(report["complete"], true);
        assert_eq!(report["measured_frames"], 8);
        assert_eq!(report["samples"][0]["draw_index"], 5);
        assert!((report["rendered_frames_per_second"].as_f64().unwrap() - 100.).abs() < 1e-8);
        assert_eq!(b.report("window_closed")["complete"], false);
        b.record(12, 0.002, 0.008, 0.01, &json!({"width":800,"height":600}));
        assert_eq!(b.report("frame_limit")["complete"], false);
        assert!(Benchmark::parse(&input, Some(8)).is_err());
    }
    #[test]
    fn workstation_benchmark_replays_each_motion_phase() {
        let mut b = Benchmark::parse(
            &json!({"benchmark":{"warmup_frames":4,"measured_frames":32}}),
            Some(36),
        )
        .unwrap()
        .unwrap();
        let changes: Vec<_> = (0..36)
            .filter_map(|i| b.targets(i).map(|targets| (i, targets)))
            .collect();
        assert_eq!(
            changes.iter().map(|x| x.0).collect::<Vec<_>>(),
            vec![0, 4, 12, 20, 28]
        );
        assert!(!changes[2].1.0[2]);
        assert!(changes[3].1.1);
        assert!(changes[4].1.2);
    }

    #[test]
    fn workstation_benchmark_repeats_dirty_motion_with_bounded_opt_in_configuration() {
        let input = request(96, Some("120"), Some("32")).unwrap();
        let mut b = Benchmark::parse(&input, Some(216)).unwrap().unwrap();
        let changes: Vec<_> = (0..216).filter_map(|i| b.targets(i).map(|t| (i,t))).collect();
        assert_eq!(changes.len(), 13);
        assert_eq!(changes[1].1, changes[5].1);
        assert_eq!(changes[3].1, changes[7].1);
        assert_eq!(b.phase(151), 4);
        assert_eq!(b.phase(152), 1);
        for bad in ["0", "121", "invalid"] { assert!(request(32, Some(bad), None).is_err()); }
        for bad in ["0", "7", "33", "604"] { assert!(request(32, None, Some(bad)).is_err()); }
        assert_eq!(request(32, None, None).unwrap()["frames"], 36);
        b.record(0, 0.002, 0.008, 0.01, &json!({"width":1280,"height":960,"logical_width":640,"logical_height":480}));
        assert_eq!(b.report("frame_limit")["warmup_samples"][0]["logical_dimensions"], json!([640,480]));
        assert_eq!(b.report("frame_limit")["measured_frames"], 0);
    }

    #[test]
    fn bake_settle_replay_has_interruptions_and_a_complete_final_hold(){
        let mut input=request(600,Some("4"),None).unwrap();input["benchmark"]["bake_settle_replay"]=json!(true);
        let mut b=Benchmark::parse(&input,Some(604)).unwrap().unwrap();
        let changes:Vec<_>=(0..604).filter_map(|i|b.targets(i).map(|t|(i,t))).collect();
        assert_eq!(changes.iter().map(|x|x.0).collect::<Vec<_>>(),vec![0,4,124,164,204]);
        assert!(!changes[1].1.0[2]);assert!(changes[2].1.1);assert_eq!(changes.last().unwrap().1,([true;4],false,false));
        assert!((204..604).all(|f|b.phase(f)==1));
        assert_eq!(b.report("frame_limit")["scenario"],"incremental-bake-cancel-then-settle-v1");
    }
    #[test]
    fn bake_settle_replay_rejects_conflicting_or_truncated_configuration(){
        for (frames,cycle,idle) in [(32,None,false),(600,Some("32"),false),(600,None,true)]{
            let mut input=request(frames,Some("4"),cycle).unwrap();input["benchmark"]["bake_settle_replay"]=json!(true);input["benchmark"]["idle"]=json!(idle);
            assert!(Benchmark::parse(&input,Some(frames as u64+4)).is_err());
        }
    }

    #[test]
    fn workstation_benchmark_idle_keeps_panels_settled_separate_from_motion() {
        let mut input = request(600, Some("120"), None).unwrap();
        input["benchmark"]["idle"] = json!(true);
        let mut b = Benchmark::parse(&input, Some(720)).unwrap().unwrap();
        let targets: Vec<_> = (0..720).filter_map(|frame| b.targets(frame)).collect();
        assert_eq!(targets, vec![([true;4],false,false);2]);
        assert_eq!(b.report("frame_limit")["scenario"], "workstation-settled-idle-v1");
    }

    #[test]
    fn workstation_benchmark_duration_requires_both_minimums_and_complete_motion_cycles() {
        let mut input=request(8,Some("4"),Some("8")).unwrap();
        apply_duration(&mut input,30.);
        let mut b=Benchmark::parse(&input,Some(20_004)).unwrap().unwrap();
        for frame in 0..12 {b.record(frame,0.001,0.009,0.01,&json!({"width":640,"height":480}));}
        assert!(!b.duration_complete(29.9));
        assert!(b.duration_complete(30.));
        b.measured_duration(Some(29.9));
        assert_eq!(b.report("frame_limit")["complete"],false);
        b.measured_duration(Some(30.));
        assert_eq!(b.report("benchmark_duration_complete")["complete"],true);
        b.record(12,0.001,0.009,0.01,&json!({"width":640,"height":480}));
        assert!(!b.duration_complete(31.));
        assert_eq!(b.report("frame_limit")["complete"],false);
        assert_eq!(b.report("benchmark_deadline")["complete"],false);
        assert!(Benchmark::parse(&input,Some(12)).is_err());
    }
}

// Opt-in finite combined benchmark only; interactive launches stay unchanged.
pub(crate) fn combined_playback_fixture(embedded_benchmark:bool,value:Option<&str>)->Result<bool,String>{
 let enabled=match value{None|Some("0")=>false,Some("1")=>true,_=>return Err("WORMWOOD_COMBINED_TRANSPORT_PLAYBACK must be0 or1".into())};
 if enabled&&!embedded_benchmark{return Err("combined transport playback requires the finite embedded pass-through benchmark".into());}Ok(enabled)
}
#[cfg(test)]mod combined_playback_fixture_tests{
 use super::*;
 #[test]fn interactive_and_unset_do_not_start_playback(){assert!(!combined_playback_fixture(false,None).unwrap());assert!(!combined_playback_fixture(true,Some("0")).unwrap());}
 #[test]fn only_explicit_finite_combined_request_enables_it(){assert!(combined_playback_fixture(true,Some("1")).unwrap());assert!(combined_playback_fixture(false,Some("1")).is_err());}
 #[test]fn invalid_values_do_not_enable(){for v in ["true","", "2"]{assert!(combined_playback_fixture(true,Some(v)).is_err());}}
}
