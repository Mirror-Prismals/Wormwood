//! Limit motion at the declared model-space control point, then recover the rigid origin.
//! An omitted control point means the model origin; callers retain that trajectory.
use serde_json::{Value, json};
type V = [f64; 3];
fn dot(a: V, b: V) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] + b[i])
}
fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
fn mul(a: V, s: f64) -> V {
    a.map(|x| x * s)
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn length(a: V) -> f64 {
    dot(a, a).sqrt()
}
fn rotate(p: V, n: V, a: f64) -> V {
    add(
        add(mul(p, a.cos()), mul(cross(n, p), a.sin())),
        mul(n, dot(n, p) * (1. - a.cos())),
    )
}
fn vector(v: &Value) -> Result<V, String> {
    serde_json::from_value(v.clone()).map_err(|e| e.to_string())
}
pub(super) fn approach(
    current: &Value,
    target: &Value,
    control_point: V,
    dt: f64,
) -> Result<Value, String> {
    let p = vector(&current["position"])?;
    let end = vector(&target["position"])?;
    let keys = ["right", "up", "forward"];
    let old = [
        vector(&current[keys[0]])?,
        vector(&current[keys[1]])?,
        vector(&current[keys[2]])?,
    ];
    let goal = [
        vector(&target[keys[0]])?,
        vector(&target[keys[1]])?,
        vector(&target[keys[2]])?,
    ];
    let q: [[f64; 3]; 3] = std::array::from_fn(|i| {
        std::array::from_fn(|j| (0..3).map(|k| goal[k][i] * old[k][j]).sum())
    });
    let angle = ((q[0][0] + q[1][1] + q[2][2] - 1.) * 0.5)
        .clamp(-1., 1.)
        .acos();
    let axes = if angle < 1e-7 {
        old
    } else {
        let skew = [q[2][1] - q[1][2], q[0][2] - q[2][0], q[1][0] - q[0][1]];
        let mut axis = if length(skew) > 1e-6 {
            mul(skew, 1. / length(skew))
        } else {
            let k = (0..3)
                .max_by(|a, b| q[*a][*a].total_cmp(&q[*b][*b]))
                .unwrap();
            let mut n = [0.; 3];
            n[k] = ((q[k][k] + 1.) * 0.5).max(0.).sqrt();
            for j in 0..3 {
                if j != k {
                    n[j] = (q[k][j] + q[j][k]) / (4. * n[k].max(1e-12));
                }
            }
            mul(n, 1. / length(n).max(1e-12))
        };
        if dot(axis, skew) < 0. {
            axis = mul(axis, -1.);
        }
        old.map(|v| rotate(v, axis, angle.min(2. * dt)))
    };
    let offset = |basis: [V; 3]| {
        std::array::from_fn::<_, 3, _>(|i| (0..3).map(|j| basis[j][i] * control_point[j]).sum())
    };
    let from = add(p, offset(old));
    let to = add(end, offset(goal));
    let delta = sub(to, from);
    let point = add(
        from,
        mul(delta, (0.75 * dt / length(delta).max(1e-20)).min(1.)),
    );
    let position = sub(point, offset(axes));
    Ok(json!({"position":position,"right":axes[0],"up":axes[1],"forward":axes[2]}))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pair() -> (Value, Value, V) {
        let pivot = [0., 6_371_001.45, 0.];
        let angle = 1.1f64;
        let start = json!({"position":[0.,6_371_000.,0.],"right":[1.,0.,0.],"up":[0.,1.,0.],"forward":[0.,0.,1.]});
        let up = [0., angle.cos(), angle.sin()];
        let end = sub(pivot, mul(up, 1.45));
        let target = json!({"position":end,"right":[1.,0.,0.],"up":up,"forward":[0.,-angle.sin(),angle.cos()]});
        (start, target, pivot)
    }
    #[test]
    fn rigid_approach_preserves_the_upper_hand_pivot() {
        let (start, target, pivot) = pair();
        let next = approach(&start, &target, [0., 1.45, 0.], 1. / 120.).unwrap();
        let hand = add(
            vector(&next["position"]).unwrap(),
            mul(vector(&next["up"]).unwrap(), 1.45),
        );
        assert!(length(sub(hand, pivot)) < 2e-9, "pivot moved: {hand:?}");
        let angle = vector(&next["up"]).unwrap()[2].asin();
        assert!((angle - 2. / 120.).abs() < 1e-10);
    }
    #[test]
    fn origin_reference_retains_a_straight_origin_path() {
        let (start, mut target, _) = pair();
        target["position"] = json!([0., 6_370_999.8, 0.]);
        let next = approach(&start, &target, [0.; 3], 1. / 120.).unwrap();
        let p = vector(&next["position"]).unwrap();
        assert_eq!(p[0], 0.);
        assert_eq!(p[2], 0.);
        assert!((p[1] - (6_371_000. - 0.75 / 120.)).abs() < 1e-9);
    }
}
