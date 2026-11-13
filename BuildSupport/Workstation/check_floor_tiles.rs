//! Standalone exact-output and CPU timing check for independent floor-panel cache.
//! Root compiles this with FLOOR_LIGHT_BASELINE_RS naming the frozen pre-change
//! module (its inner doc comments converted to ordinary comments for include!).
//! Usage: check-floor-tiles FRAME.bin [FRAME.bin ...]. Not a GPU/FPS benchmark.
#![allow(dead_code)]

#[path = "../../Workstation/FloorLight.rs"]
mod candidate;
mod baseline { include!(env!("FLOOR_LIGHT_BASELINE_RS")); }

fn load(path: &str) -> Vec<[f32; 4]> {
    let bytes = std::fs::read(path).expect("read optical frame");
    assert!(bytes.len() >= 256 * 16 && bytes.len() % 16 == 0);
    bytes.chunks_exact(16).take(256).map(|row| std::array::from_fn(|i| {
        f32::from_le_bytes(row[i * 4..i * 4 + 4].try_into().unwrap())
    })).collect()
}

fn compare(label: &str, frame: &[[f32; 4]], cache: &mut candidate::Cache) {
    let before = baseline::build(frame);
    let after = cache.field(frame);
    let mismatches = before.iter().zip(after).flat_map(|(a,b)| a.iter().zip(b))
        .filter(|(a,b)| a.to_bits() != b.to_bits()).count();
    let largest = before.iter().zip(after).flat_map(|(a,b)| a.iter().zip(b))
        .map(|(a,b)| (a - b).abs()).fold(0f32, f32::max);
    println!("{{\"case\":{label:?},\"mismatched_words\":{mismatches},\"max_error\":{largest}}}");
    assert_eq!(mismatches, 0, "field changed for {label}");
}

fn shifted_panel(frame: &[[f32; 4]], index: usize, delta: [f32; 3]) -> Vec<[f32; 4]> {
    let mut moved = frame.to_vec();
    let base = 32 + 40 * index;
    for axis in 0..3 { moved[base + 2][axis] += delta[axis]; }
    for plane in &mut moved[base + 6..base + 32] {
        plane[3] += plane[0] * delta[0] + plane[1] * delta[1] + plane[2] * delta[2];
    }
    moved
}

fn main() {
    let paths: Vec<_> = std::env::args().skip(1).collect();
    assert!(!paths.is_empty(), "supply an optical frame binary");
    for path in paths {
        let frame = load(&path);
        let mut cache = candidate::Cache::with_sample_reuse(true);
        compare(&path, &frame, &mut cache);
        for index in 0..4 {
            for (name, delta) in [("left",[-0.37,0.,0.]), ("up",[0.,0.29,0.]), ("depth",[0.,0.,0.07])] {
                compare(&format!("panel-{index}-{name}"), &shifted_panel(&frame,index,delta), &mut cache);
            }
        }
        for scale in [0.35, 1.4, 3.] {
            let mut resized = frame.clone();
            resized[2][3] *= scale;
            resized[3][3] *= scale;
            compare(&format!("domain-scale-{scale}"), &resized, &mut cache);
        }
        for floor in [-4., -0.05, 0., 0.4, 3.] {
            let mut moved = frame.clone();
            moved[5][3] = floor;
            compare(&format!("floor-{floor}"), &moved, &mut cache);
        }
        for light_index in 0..3 {
            for (name, delta) in [("side",[0.9,-0.45,0.]), ("below",[0.,0.,-8.])] {
                let mut moved = frame.clone();
                for axis in 0..3 { moved[16 + 5 * light_index][axis] += delta[axis]; }
                compare(&format!("light-{light_index}-{name}"), &moved, &mut cache);
            }
        }
        let mut degenerate = frame.clone();
        for index in 0..3 {
            degenerate[16 + 5 * index] = [0.,0.,frame[5][3],0.];
            degenerate[17 + 5 * index] = [0.,0.,0.,0.1];
            degenerate[18 + 5 * index] = [0.,0.,0.,0.1];
        }
        compare("degenerate-floor-lights", &degenerate, &mut cache);
        degenerate[249][3] = f32::NAN;
        compare("nonfinite-fallback", &degenerate, &mut cache);
        for scenario in ["one-panel", "two-panels", "floor-change"] {
            let mut motion_cache = candidate::Cache::with_sample_reuse(true);
            let cold = std::time::Instant::now();
            std::hint::black_box(motion_cache.field(&frame));
            let startup_ms = cold.elapsed().as_secs_f64() * 1000.;
            let mut old_ms = Vec::new();
            let mut new_ms = Vec::new();
            let mut changed_warmup_ms = Vec::new();
            for i in 0..18 {
                let mut moved = shifted_panel(&frame, 2, [0.002 * (i + 1) as f32, 0., 0.]);
                if scenario == "two-panels" { moved = shifted_panel(&moved, 1, [0., 0.003 * i as f32, 0.]); }
                if scenario == "floor-change" { moved[5][3] += 0.0002 * (i + 1) as f32; }
                // Alternate order and exclude three initial pairs. Every new
                // frame changes its scene, so these cannot become cache hits.
                for new in [i % 2 == 0, i % 2 != 0] {
                    let start = std::time::Instant::now();
                    if new { std::hint::black_box(motion_cache.field(&moved)); }
                    else { std::hint::black_box(baseline::build(&moved)); }
                    let elapsed = start.elapsed().as_secs_f64() * 1000.;
                    if i >= 3 { if new { new_ms.push(elapsed); } else { old_ms.push(elapsed); } }
                    else if new { changed_warmup_ms.push(elapsed); }
                }
                let reference = baseline::build(&moved);
                assert!(reference.iter().zip(motion_cache.field(&moved)).all(|(a,b)| a.map(f32::to_bits)==b.map(f32::to_bits)), "changed frame differs");
            }
            old_ms.sort_by(f64::total_cmp);
            new_ms.sort_by(f64::total_cmp);
            println!("{{\"timing_frame\":{path:?},\"scenario\":{scenario:?},\"samples\":15,\"startup_ms\":{startup_ms},\"candidate_changed_warmup_ms\":{changed_warmup_ms:?},\"baseline_ms_median\":{},\"candidate_ms_median\":{},\"speedup\":{},\"baseline_samples_ms\":{:?},\"candidate_samples_ms\":{:?}}}", old_ms[7], new_ms[7], old_ms[7]/new_ms[7],old_ms,new_ms);
        }
    }
}
