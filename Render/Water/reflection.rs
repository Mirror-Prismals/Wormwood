//! Continuous reflection geometry for the parent's real opaque-scene redraw.
//! No screen-color mirroring, GPU resources or native calls are performed here.
use super::data::{Camera, ReflectionPlane};
#[derive(Clone, Debug)]
pub(crate) struct SceneView {
    pub camera: Camera,
    /// Keep n·local_position + d >= 0, using camera.origin-relative vertices.
    pub clip_plane: [f32; 4],
    /// Reflection reverses orientation; the opaque pass must reverse culling.
    pub reverse_winding: bool,
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
/// The view matrix is original_view * world_reflection, so points on the water
/// plane project to the same pixels in the main and reflection attachments.
/// Parent keeps its backend projection/depth convention and applies clip_plane
/// to scene geometry before rasterization. An ordinary look-at reconstruction
/// would discard the reflected handedness and break this pixel correspondence.
pub(crate) fn scene_view(source: &Camera, plane: ReflectionPlane) -> Result<SceneView, String> {
    if !source.valid()
        || plane
            .normal
            .iter()
            .chain(plane.point.iter())
            .any(|n| !n.is_finite())
    {
        return Err("invalid reflection camera/plane".into());
    }
    let length = dot(plane.normal, plane.normal).sqrt();
    if length <= 1e-12 {
        return Err("zero reflection plane normal".into());
    }
    let n = plane.normal.map(|x| x / length);
    let point_local = std::array::from_fn(|i| plane.point[i] - source.origin[i]);
    let d = -dot(n, point_local);
    // Column-major affine reflection in origin-relative metre coordinates.
    let mut reflection = [[0.; 4]; 4];
    for column in 0..3 {
        for row in 0..3 {
            reflection[column][row] =
                (if column == row { 1. } else { 0. }) - 2. * n[column] * n[row];
        }
    }
    for row in 0..3 {
        reflection[3][row] = -2. * d * n[row];
    }
    reflection[3][3] = 1.;
    let mut camera = source.clone();
    for column in 0..4 {
        for row in 0..4 {
            let value = (0..4)
                .map(|k| source.view[k][row] as f64 * reflection[column][k])
                .sum::<f64>();
            camera.view[column][row] = value as f32;
        }
    }
    let eye = source.relative_position.map(f64::from);
    let signed = dot(n, eye) + d;
    camera.relative_position = std::array::from_fn(|i| (eye[i] - 2. * signed * n[i]) as f32);
    let reflect_direction = |v: [f32; 3]| {
        let v = v.map(f64::from);
        let amount = 2. * dot(n, v);
        std::array::from_fn(|i| (v[i] - amount * n[i]) as f32)
    };
    camera.right = reflect_direction(source.right);
    camera.up = reflect_direction(source.up);
    camera.forward = reflect_direction(source.forward);
    let keep = if signed < 0. { -1. } else { 1. };
    let clip_plane = [n[0] * keep, n[1] * keep, n[2] * keep, d * keep].map(|x| x as f32);
    if !camera.valid() || clip_plane.iter().any(|n| !n.is_finite()) {
        return Err("reflection exceeds finite render coordinates".into());
    }
    Ok(SceneView {
        camera,
        clip_plane,
        reverse_winding: true,
    })
}
