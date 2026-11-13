//! Pure rig mathematics, owned by the model/IK named hooks.
pub(crate) type V = [f64; 3];
pub(crate) fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] + b[i])
}
pub(crate) fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] - b[i])
}
pub(crate) fn mul(a: V, s: f64) -> V {
    a.map(|v| v * s)
}
pub(crate) fn dot(a: V, b: V) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
pub(crate) fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub(crate) fn len(a: V) -> f64 {
    dot(a, a).sqrt()
}
pub(crate) fn unit(a: V) -> V {
    mul(a, 1. / len(a).max(1e-12))
}
pub(crate) fn mix(a: V, b: V, t: f64) -> V {
    add(mul(a, 1. - t), mul(b, t))
}
pub(crate) fn vector(v: &serde_json::Value) -> Result<V, String> {
    serde_json::from_value::<V>(v.clone())
        .map_err(|e| e.to_string())
        .and_then(|v| {
            if v.iter().all(|x| x.is_finite()) {
                Ok(v)
            } else {
                Err("nonfinite rig vector".into())
            }
        })
}
pub(crate) fn rotate(v: V, axis: V, angle: f64) -> V {
    let (s, c) = angle.sin_cos();
    add(
        add(mul(v, c), mul(cross(axis, v), s)),
        mul(axis, dot(axis, v) * (1. - c)),
    )
}
pub(crate) fn between(v: V, from: V, to: V) -> V {
    let a = unit(from);
    let b = unit(to);
    let c = dot(a, b).clamp(-1., 1.);
    if c > 0.999999 {
        return v;
    }
    let axis = cross(a, b);
    if len(axis) < 1e-8 {
        return rotate(
            v,
            unit(cross(
                a,
                if a[0].abs() < 0.8 {
                    [1., 0., 0.]
                } else {
                    [0., 1., 0.]
                },
            )),
            std::f64::consts::PI,
        );
    }
    rotate(v, unit(axis), c.acos())
}
pub(crate) fn inverse(m: [f64; 16]) -> Result<[f64; 16], String> {
    let mut a = [[0.; 8]; 4];
    for r in 0..4 {
        for c in 0..4 {
            a[r][c] = m[c * 4 + r];
        }
        a[r][r + 4] = 1.;
    }
    for c in 0..4 {
        let pivot = (c..4)
            .max_by(|&x, &y| a[x][c].abs().total_cmp(&a[y][c].abs()))
            .unwrap();
        a.swap(c, pivot);
        if a[c][c].abs() < 1e-12 {
            return Err("singular inverse bind matrix".into());
        }
        let d = a[c][c];
        for j in 0..8 {
            a[c][j] /= d;
        }
        for r in 0..4 {
            if r != c {
                let d = a[r][c];
                for j in 0..8 {
                    a[r][j] -= d * a[c][j];
                }
            }
        }
    }
    Ok(std::array::from_fn(|i| a[i % 4][i / 4 + 4]))
}
