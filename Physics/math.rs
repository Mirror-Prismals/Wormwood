pub(crate) type V = [f32; 3];
pub(crate) fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] + b[i])
}
pub(crate) fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
pub(crate) fn scale(a: V, s: f32) -> V {
    a.map(|x| x * s)
}
pub(crate) fn dot(a: V, b: V) -> f32 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
pub(crate) fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(crate) fn length(a: V) -> f32 {
    dot(a, a).sqrt()
}
pub(crate) fn unit(a: V) -> V {
    scale(a, 1. / length(a).max(1e-9))
}
pub(crate) fn rotate_x(p: V, center: V, angle: f32) -> V {
    let p = sub(p, center);
    let (s, c) = angle.sin_cos();
    add(center, [p[0], p[1] * c - p[2] * s, p[1] * s + p[2] * c])
}
