//! Mirror-owned acoustic propagation, ChucK transport and callback-only DSP.
pub(crate) mod acoustic;
pub(crate) mod birds;
pub(crate) mod chuck;
#[path = "InputCapture.rs"]
pub(crate) mod input_capture;
pub(crate) mod dsp;
pub(crate) mod geometry;
#[path = "LineArray.rs"]
pub(crate) mod line_array;
#[path = "Transport.rs"]
pub(crate) mod transport;
pub(crate) type V = [f64; 3];
pub(crate) fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] + b[i])
}
pub(crate) fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
pub(crate) fn mul(a: V, s: f64) -> V {
    a.map(|x| x * s)
}
pub(crate) fn dot(a: V, b: V) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
pub(crate) fn length(a: V) -> f64 {
    dot(a, a).sqrt()
}
pub(crate) fn unit(a: V) -> V {
    let n = length(a);
    if n > 1e-10 { mul(a, 1. / n) } else { [0.; 3] }
}
pub(crate) fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(crate) fn vector(x: &serde_json::Value) -> Result<V, String> {
    let v: V = serde_json::from_value(x.clone()).map_err(|e| format!("audio vector: {e}"))?;
    if v.iter().all(|n| n.is_finite()) {
        Ok(v)
    } else {
        Err("non-finite audio vector".into())
    }
}

#[path = "Volumes.rs"]
pub(crate) mod volumes;

#[path = "WorkerScheduling.rs"]
pub(crate) mod worker_scheduling;
