//! Diagnostic only: cold floor recomputation, three vs six scoped workers.
//! Compile with FLOOR_LIGHT_BASELINE_RS and FLOOR_LIGHT_SIX_RS pointing at the
//! frozen modules; pass actual frame binaries. Does not measure GPU or FPS.
#![allow(dead_code)]
mod baseline { include!(env!("FLOOR_LIGHT_BASELINE_RS")); }
mod six { include!(env!("FLOOR_LIGHT_SIX_RS")); }

fn main() {
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).unwrap();
        let frame: Vec<[f32; 4]> = bytes.chunks_exact(16).take(256).map(|row| std::array::from_fn(|i| f32::from_le_bytes(row[4*i..4*i+4].try_into().unwrap()))).collect();
        let old = baseline::build(&frame);
        let new = six::build(&frame);
        assert!(old.iter().zip(&new).all(|(a,b)| a.map(f32::to_bits)==b.map(f32::to_bits)),"six-worker output changed");
        let mut times: [Vec<f64>; 2] = Default::default();
        for i in 0..18 {
            for variant in [i % 2, 1 - i % 2] {
                let start = std::time::Instant::now();
                if variant == 0 { std::hint::black_box(baseline::build(&frame)); }
                else { std::hint::black_box(six::build(&frame)); }
                if i >= 3 { times[variant].push(start.elapsed().as_secs_f64()*1000.); }
            }
        }
        for values in &mut times { values.sort_by(f64::total_cmp); }
        println!("{{\"frame\":{path:?},\"three_worker_ms\":{},\"six_worker_ms\":{},\"speedup\":{},\"samples_ms\":{:?}}}", times[0][7],times[1][7],times[0][7]/times[1][7],times);
    }
}
