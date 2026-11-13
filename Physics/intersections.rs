//! Independent triangle crossing monitor with an AABB sweep; excludes shared topology.
use super::{Scene, math::*};
fn segment_triangle(a: V, b: V, t: [V; 3]) -> bool {
    let direction = sub(b, a);
    let e1 = sub(t[1], t[0]);
    let e2 = sub(t[2], t[0]);
    let h = cross(direction, e2);
    let det = dot(e1, h);
    if det.abs() < 1e-8 {
        return false;
    }
    let inv = 1. / det;
    let s = sub(a, t[0]);
    let u = inv * dot(s, h);
    if !(1e-5..=0.99999).contains(&u) {
        return false;
    }
    let q = cross(s, e1);
    let v = inv * dot(direction, q);
    if v <= 1e-5 || u + v >= 0.99999 {
        return false;
    }
    let time = inv * dot(e2, q);
    (1e-5..0.99999).contains(&time)
}
fn coplanar_overlap(a: [V; 3], b: [V; 3]) -> bool {
    let n = cross(sub(a[1], a[0]), sub(a[2], a[0]));
    let norm = length(n);
    if norm < 1e-9 || b.iter().any(|p| dot(n, sub(*p, a[0])).abs() > norm * 1e-6) {
        return false;
    }
    let drop = (0..3)
        .max_by(|i, j| n[*i].abs().total_cmp(&n[*j].abs()))
        .unwrap();
    let axes: Vec<_> = (0..3).filter(|i| *i != drop).collect();
    let project = |t: [V; 3]| t.map(|p| [p[axes[0]], p[axes[1]]]);
    let a = project(a);
    let b = project(b);
    for t in [a, b] {
        for k in 0..3 {
            let edge = [t[(k + 1) % 3][0] - t[k][0], t[(k + 1) % 3][1] - t[k][1]];
            let axis = [-edge[1], edge[0]];
            let interval = |p: [[f32; 2]; 3]| {
                let values = p.map(|p| p[0] * axis[0] + p[1] * axis[1]);
                (
                    values.into_iter().fold(f32::INFINITY, f32::min),
                    values.into_iter().fold(f32::NEG_INFINITY, f32::max),
                )
            };
            let (lo, hi) = interval(a);
            let (other_lo, other_hi) = interval(b);
            if hi.min(other_hi) - lo.max(other_lo) <= 1e-7 {
                return false;
            }
        }
    }
    true
}

pub(super) fn count(scene: &Scene) -> usize {
    count_filtered(scene, |_, _| false)
}
pub(super) fn count_filtered(
    scene: &Scene,
    ignore: impl Fn([usize; 3], [usize; 3]) -> bool,
) -> usize {
    count_filtered_visit(scene, ignore, |_, _| {})
}
pub(super) fn count_filtered_visit(
    scene: &Scene,
    ignore: impl Fn([usize; 3], [usize; 3]) -> bool,
    mut hit: impl FnMut([usize; 3], [usize; 3]),
) -> usize {
    let faces = scene.faces();
    let mut boxes: Vec<_> = faces
        .iter()
        .enumerate()
        .map(|(id, f)| {
            let points = f.map(|i| scene.p[i]);
            let low: V =
                std::array::from_fn(|k| points.iter().map(|p| p[k]).fold(f32::INFINITY, f32::min));
            let high: V = std::array::from_fn(|k| {
                points
                    .iter()
                    .map(|p| p[k])
                    .fold(f32::NEG_INFINITY, f32::max)
            });
            (id, low, high)
        })
        .collect();
    boxes.sort_by(|a, b| a.1[0].total_cmp(&b.1[0]));
    let mut count = 0;
    for i in 0..boxes.len() {
        let (a, low, high) = boxes[i];
        for &(b, other_low, other_high) in &boxes[i + 1..] {
            if other_low[0] > high[0] {
                break;
            }
            if (1..3).any(|k| other_low[k] > high[k] || other_high[k] < low[k])
                || faces[a].iter().any(|v| faces[b].contains(v))
                || ignore(faces[a], faces[b])
            {
                continue;
            }
            let first = faces[a].map(|i| scene.p[i]);
            let second = faces[b].map(|i| scene.p[i]);
            if coplanar_overlap(first, second)
                || (0..3).any(|j| {
                    segment_triangle(first[j], first[(j + 1) % 3], second)
                        || segment_triangle(second[j], second[(j + 1) % 3], first)
                })
            {
                count += 1;
                hit(faces[a], faces[b]);
            }
        }
    }
    count
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coplanar_area_overlap_is_rejected_but_touching_edges_are_allowed() {
        let a = [[0., 0., 0.], [1., 0., 0.], [0., 0., 1.]];
        assert!(coplanar_overlap(
            a,
            [[0.1, 0., 0.1], [0.8, 0., 0.1], [0.1, 0., 0.8]]
        ));
        assert!(!coplanar_overlap(
            a,
            [[1., 0., 0.], [1., 0., 1.], [0., 0., 1.]]
        ));
    }
    #[test]
    fn catches_a_triangle_crossing() {
        let a = [[-1., 0., -1.], [1., 0., -1.], [0., 0., 1.]];
        assert!(segment_triangle([0., -1., 0.], [0., 1., 0.], a));
        assert!(!segment_triangle([2., -1., 0.], [2., 1., 0.], a));
    }
}
