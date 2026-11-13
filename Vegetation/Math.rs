pub type V = [f64; 3];
pub fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] + b[i])
}
pub fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
pub fn mul(a: V, s: f64) -> V {
    a.map(|v| v * s)
}
pub fn dot(a: V, b: V) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
pub fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn length(a: V) -> f64 {
    dot(a, a).sqrt()
}
pub fn unit(a: V) -> V {
    mul(a, 1. / length(a).max(1e-15))
}
pub fn basis(up: V) -> (V, V) {
    let e = unit(cross(
        if up[1].abs() > 0.95 {
            [0., 0., -1.]
        } else {
            [0., 1., 0.]
        },
        up,
    ));
    (e, cross(up, e))
}
pub fn hash(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58476d1ce4e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
pub fn random(seed: u64, stream: u64) -> f64 {
    (hash(seed.wrapping_add(stream.wrapping_mul(0x9e3779b97f4a7c15))) >> 11) as f64
        / (1u64 << 53) as f64
}
