//! Presentation math for the initial closed convex mesh. No terrain or world storage.
use serde_json::Value;
pub(crate) fn valid(view: &Value) -> bool {
    ["right", "up", "forward"].iter().all(|key| {
        view[*key].is_null()
            || view[*key].as_array().is_some_and(|v| {
                v.len() == 3 && v.iter().all(|v| v.as_f64().is_some_and(f64::is_finite))
            })
    }) && view["entity"].as_u64().is_some_and(|n| n > 0)
        && view["position"].as_array().is_some_and(|v| {
            v.len() == 3 && v.iter().all(|v| v.as_f64().is_some_and(f64::is_finite))
        })
        && ["yaw", "pitch"]
            .iter()
            .all(|key| view[*key].as_f64().is_some_and(f64::is_finite))
}
pub(super) fn project(vertices: &[[f32; 6]], view: &Value) -> Vec<[f32; 24]> {
    let camera: Vec<f64> = view["position"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    let (sy, cy) = view["yaw"].as_f64().unwrap().sin_cos();
    let (sp, cp) = view["pitch"].as_f64().unwrap().sin_cos();
    let basis = |key: &str, fallback: [f64; 3]| {
        if view[key].is_array() {
            std::array::from_fn(|i| view[key][i].as_f64().unwrap())
        } else {
            fallback
        }
    };
    let right = basis("right", [cy, 0., sy]);
    let up = basis("up", [-sy * sp, cp, cy * sp]);
    let forward = basis("forward", [sy * cp, sp, -cy * cp]);
    let focal = 1. / (70f64.to_radians() / 2.).tan();
    let near = 0.05;
    let far = view["far"].as_f64().unwrap_or(1000.);
    let mut output = Vec::new();
    for triangle in vertices.chunks_exact(3) {
        let mut a = [0f64; 3];
        let mut b = [0f64; 3];
        let mut toward = [0f64; 3];
        for i in 0..3 {
            a[i] = (triangle[1][i] - triangle[0][i]) as f64;
            b[i] = (triangle[2][i] - triangle[0][i]) as f64;
            toward[i] = camera[i] - triangle[0][i] as f64;
        }
        let normal = [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ];
        if normal.iter().zip(toward).map(|(n, t)| n * t).sum::<f64>() <= 0. {
            continue;
        }
        let mut data = [0f32; 24];
        for (i, v) in triangle.iter().enumerate() {
            let relative = [
                v[0] as f64 - camera[0],
                v[1] as f64 - camera[1],
                v[2] as f64 - camera[2],
            ];
            let dot = |axis: [f64; 3]| axis.iter().zip(relative).map(|(a, b)| a * b).sum::<f64>();
            let z = dot(forward);
            data[i * 4..i * 4 + 4].copy_from_slice(&[
                (dot(right) * focal / (4. / 3.)) as f32,
                (dot(up) * focal) as f32,
                (far / (far - near) * z - far * near / (far - near)) as f32,
                z as f32,
            ]);
            data[12 + i * 4..16 + i * 4].copy_from_slice(&[v[3], v[4], v[5], 1.]);
        }
        output.push(data);
    }
    output
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn front_face_projects_and_reverse_winding_is_culled() {
        let tri = [
            [-1., -1., 0., 1., 0., 0.],
            [1., -1., 0., 1., 0., 0.],
            [0., 1., 0., 1., 0., 0.],
        ];
        let view = json!({"entity":1,"position":[0,0,4],"yaw":0,"pitch":0});
        let projected = project(&tri, &view);
        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0][3], 4.);
        assert!(projected[0][2] > 0. && projected[0][2] < 4.);
        assert!(project(&[tri[0], tri[2], tri[1]], &view).is_empty());
    }
}
