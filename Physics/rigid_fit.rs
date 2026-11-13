//! Shape matching for a finite-mass rigid contact cluster; no graphics or event work.
use super::math::V;
type D = [f64; 3];
fn dot(a: D, b: D) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn cross(a: D, b: D) -> D {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn unit(a: D) -> D {
    let l = dot(a, a).sqrt();
    a.map(|v| v / l.max(1e-15))
}
/// Return the least-squares proper rigid transform of model vertices to projected
/// contact vertices. The polar factor preserves one rigid tool after node solving.
pub(super) fn fit(model: &[V], positions: &[V]) -> Option<(D, [D; 3])> {
    if model.len() != positions.len() || model.len() < 4 {
        return None;
    }
    let center = |p: &[V]| {
        std::array::from_fn::<_, 3, _>(|i| {
            p.iter().map(|p| p[i] as f64).sum::<f64>() / p.len() as f64
        })
    };
    let a = center(model);
    let b = center(positions);
    let mut r = [[0f64; 3]; 3];
    for (q, p) in model.iter().zip(positions) {
        for j in 0..3 {
            for i in 0..3 {
                r[j][i] += (p[i] as f64 - b[i]) * (q[j] as f64 - a[j]);
            }
        }
    }
    let scale = r.iter().flatten().map(|v| v * v).sum::<f64>().sqrt();
    if scale < 1e-15 {
        return None;
    }
    for c in &mut r {
        for x in c {
            *x /= scale;
        }
    }
    for _ in 0..24 {
        let det = dot(r[0], cross(r[1], r[2]));
        if !det.is_finite() || det <= 1e-15 {
            return None;
        }
        let inv = [
            cross(r[1], r[2]).map(|v| v / det),
            cross(r[2], r[0]).map(|v| v / det),
            cross(r[0], r[1]).map(|v| v / det),
        ];
        let mut error = 0f64;
        for j in 0..3 {
            for i in 0..3 {
                let next = (r[j][i] + inv[j][i]) * 0.5;
                error = error.max((next - r[j][i]).abs());
                r[j][i] = next;
            }
        }
        if error < 1e-12 {
            break;
        }
    }
    let right = unit(r[0]);
    let projection = dot(r[1], right);
    let up = unit(std::array::from_fn(|i| r[1][i] - right[i] * projection));
    let forward = cross(right, up);
    let axes = [right, up, forward];
    let origin = std::array::from_fn(|i| b[i] - (0..3).map(|j| axes[j][i] * a[j]).sum::<f64>());
    origin
        .iter()
        .chain(axes.iter().flatten())
        .all(|x| x.is_finite())
        .then_some((origin, axes))
}
