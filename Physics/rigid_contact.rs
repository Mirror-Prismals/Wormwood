//! Six-degree-of-freedom Newton contact step for a rigid tool cluster.
//! OGC supplies gradients, Hessians and conservative per-vertex displacement bounds.
use super::math::V;
type D = [f64; 3];
fn dot(a: D, b: D) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn add(a: D, b: D) -> D {
    std::array::from_fn(|i| a[i] + b[i])
}
fn sub(a: D, b: D) -> D {
    std::array::from_fn(|i| a[i] - b[i])
}
fn scale(a: D, s: f64) -> D {
    a.map(|x| x * s)
}
fn cross(a: D, b: D) -> D {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn rotate(p: D, w: D) -> D {
    let angle = dot(w, w).sqrt();
    if angle < 1e-12 {
        return add(p, cross(w, p));
    }
    let n = scale(w, 1. / angle);
    add(
        add(scale(p, angle.cos()), scale(cross(n, p), angle.sin())),
        scale(n, dot(n, p) * (1. - angle.cos())),
    )
}
fn linear(mut a: [[f64; 6]; 6], mut b: [f64; 6]) -> Option<[f64; 6]> {
    for col in 0..6 {
        let pivot = (col..6).max_by(|i, j| a[*i][col].abs().total_cmp(&a[*j][col].abs()))?;
        if a[pivot][col].abs() < 1e-14 {
            return None;
        }
        a.swap(pivot, col);
        b.swap(pivot, col);
        let d = a[col][col];
        for j in col..6 {
            a[col][j] /= d
        }
        b[col] /= d;
        for row in 0..6 {
            if row == col {
                continue;
            }
            let f = a[row][col];
            for j in col..6 {
                a[row][j] -= f * a[col][j]
            }
            b[row] -= f * b[col];
        }
    }
    b.iter().all(|x| x.is_finite()).then_some(b)
}
#[derive(Clone)]
pub(super) struct Body {
    pub mass: f64,
    pub center: D,
    pub inertia: [[f64; 3]; 3],
    pub max_force: f64,
    pub max_torque: f64,
}
impl Body {
    pub(super) fn from_value(v: &serde_json::Value) -> Result<Self, String> {
        let body = Self {
            mass: v["mass_kg"].as_f64().ok_or("tool rigid mass required")?,
            center: serde_json::from_value(v["center"].clone()).map_err(|e| e.to_string())?,
            inertia: serde_json::from_value(v["inertia"].clone()).map_err(|e| e.to_string())?,
            max_force: v["max_hand_force_n"]
                .as_f64()
                .ok_or("tool hand force limit required")?,
            max_torque: v["max_hand_torque_nm"]
                .as_f64()
                .ok_or("tool hand torque limit required")?,
        };
        let i = body.inertia;
        let det = i[0][0] * (i[1][1] * i[2][2] - i[1][2] * i[2][1])
            - i[0][1] * (i[1][0] * i[2][2] - i[1][2] * i[2][0])
            + i[0][2] * (i[1][0] * i[2][1] - i[1][1] * i[2][0]);
        if !body.mass.is_finite()
            || !(0.1..=25.).contains(&body.mass)
            || body.center.iter().any(|v| !v.is_finite() || v.abs() > 3.)
            || i.iter().flatten().any(|v| !v.is_finite() || v.abs() > 100.)
            || (0..3).any(|a| (0..3).any(|b| (i[a][b] - i[b][a]).abs() > 1e-8))
            || i[0][0] <= 0.
            || i[0][0] * i[1][1] - i[0][1] * i[1][0] <= 1e-12
            || det <= 1e-12
            || !(1.0..=2000.).contains(&body.max_force)
            || !(0.1..=500.).contains(&body.max_torque)
        {
            return Err("invalid full-spade mass/inertia or hand wrench limits".into());
        }
        Ok(body)
    }
}
#[derive(Clone)]
pub(super) struct Grip {
    pub point: D,
    pub shoulder: D,
    pub minimum: f64,
    pub maximum: f64,
    pub slide: Option<[f64; 2]>,
}
#[derive(Clone)]
pub(super) struct Grasp {
    pub grips: [Grip; 2],
}
struct Binding {
    base: D,
    up: D,
    shoulder: D,
    minimum: f64,
    maximum: f64,
    slide: Option<[f64; 2]>,
}
impl Binding {
    fn sample(&self, c: D, q: [f64; 6]) -> (D, D) {
        let w = [q[3], q[4], q[5]];
        let mut r = self.base;
        if let Some(range) = self.slide {
            let base = add(add(c, [q[0], q[1], q[2]]), rotate(r, w));
            let up = rotate(self.up, w);
            let t = dot(sub(self.shoulder, base), up).clamp(range[0], range[1]);
            r = add(r, scale(self.up, t));
        }
        let rotated = rotate(r, w);
        (rotated, add(add(c, [q[0], q[1], q[2]]), rotated))
    }
    fn valid(&self, c: D, q: [f64; 6], tolerance: f64) -> bool {
        let distance = dot(
            sub(self.sample(c, q).1, self.shoulder),
            sub(self.sample(c, q).1, self.shoulder),
        )
        .sqrt();
        distance <= self.maximum + tolerance && distance + tolerance >= self.minimum
    }
}

pub(super) struct Projection {
    pub positions: Vec<V>,
    pub restricted: bool,
    pub grasp_feasible: bool,
}
/// Responses are 13 floats: force xyz, column-major 3x3 Hessian, conservative bound.
pub(super) fn project(
    model: &[V],
    current: &[V],
    goal: &[V],
    response: &[[f32; 13]],
    body: &Body,
    grasp: Option<&Grasp>,
    dt: f64,
) -> Result<Projection, String> {
    if current.len() < 4 || current.len() != goal.len() || response.len() != current.len() {
        return Err("rigid OGC response length mismatch".into());
    }
    let to_d = |p: &V| p.map(|x| x as f64);
    let (origin, axes) =
        super::rigid_fit::fit(model, current).ok_or("current rigid body frame is singular")?;
    let (goal_origin, goal_axes) =
        super::rigid_fit::fit(model, goal).ok_or("target rigid body frame is singular")?;
    let c = add(
        origin,
        std::array::from_fn(|i| (0..3).map(|j| axes[j][i] * body.center[j]).sum()),
    );
    let target = add(
        goal_origin,
        std::array::from_fn(|i| (0..3).map(|j| goal_axes[j][i] * body.center[j]).sum()),
    );
    let offsets: Vec<D> = current.iter().map(|p| sub(to_d(p), c)).collect();
    let bindings: Vec<Binding> = grasp.map_or_else(Vec::new, |g| {
        g.grips
            .iter()
            .map(|grip| {
                let point = if grip.slide.is_some() {
                    [grip.point[0], 0., grip.point[2]]
                } else {
                    grip.point
                };
                let offset = std::array::from_fn(|i| (0..3).map(|j| axes[j][i] * point[j]).sum());
                Binding {
                    base: sub(add(origin, offset), c),
                    up: axes[1],
                    shoulder: grip.shoulder,
                    minimum: grip.minimum,
                    maximum: grip.maximum,
                    slide: grip.slide,
                }
            })
            .collect()
    });

    let (_, rotation) =
        super::rigid_fit::fit(current, goal).ok_or("rigid target orientation is singular")?;
    let omega = [
        (rotation[1][2] - rotation[2][1]) * 0.5,
        (rotation[2][0] - rotation[0][2]) * 0.5,
        (rotation[0][1] - rotation[1][0]) * 0.5,
    ];
    let t = sub(target, c);
    let desired = [t[0], t[1], t[2], omega[0], omega[1], omega[2]];
    let mut inertia = [[0f64; 6]; 6];
    for i in 0..3 {
        inertia[i][i] = body.mass;
    }
    for i in 0..3 {
        for j in 0..3 {
            inertia[i + 3][j + 3] = (0..3)
                .map(|k| {
                    (0..3)
                        .map(|l| axes[k][i] * body.inertia[k][l] * axes[l][j])
                        .sum::<f64>()
                })
                .sum::<f64>();
        }
    }
    let mut a = inertia;
    for row in &mut a {
        for x in row {
            *x /= dt * dt;
        }
    }
    let mut rhs = std::array::from_fn::<_, 6, _>(|i| {
        (0..6)
            .map(|j| inertia[i][j] * desired[j] / (dt * dt))
            .sum::<f64>()
    });
    for (start, limit) in [(0, body.max_force), (3, body.max_torque)] {
        let magnitude = rhs[start..start + 3]
            .iter()
            .map(|x| x * x)
            .sum::<f64>()
            .sqrt();
        if magnitude > limit {
            for value in &mut rhs[start..start + 3] {
                *value *= limit / magnitude;
            }
        }
    }
    for (r, s) in offsets.iter().zip(response) {
        // Most vertices in an intact rock cell have no contact response. Their
        // displacement bounds still participate below, but adding a zero
        // force/Hessian through the full 6x6 Jacobian is pure wasted work.
        if s[..12].iter().all(|v| *v == 0.) { continue; }
        let mut jac = [[0f64; 6]; 3];
        for k in 0..3 {
            jac[k][k] = 1.;
            for j in 0..3 {
                let mut axis = [0.; 3];
                axis[j] = 1.;
                jac[k][j + 3] = cross(axis, *r)[k];
            }
        }
        for i in 0..6 {
            rhs[i] += (0..3).map(|k| jac[k][i] * s[k] as f64).sum::<f64>();
            for j in 0..6 {
                a[i][j] += (0..3)
                    .map(|k| {
                        (0..3)
                            .map(|l| jac[k][i] * s[3 + l * 3 + k] as f64 * jac[l][j])
                            .sum::<f64>()
                    })
                    .sum::<f64>();
            }
        }
    }
    let mut q = linear(a, rhs).ok_or("rigid contact Newton system is singular")?;
    let mut inverse_newton = [[0f64; 6]; 6];
    for col in 0..6 {
        let mut axis = [0.; 6];
        axis[col] = 1.;
        inverse_newton[col] =
            linear(a, axis).ok_or("rigid constrained Newton metric is singular")?;
    }

    let mut joint_restricted = false;
    // Project the SIX coupled freedoms onto all exact vertex displacement balls.
    // A blocked point may act as a pivot; it does not zero unrelated rigid freedoms.
    for _ in 0..32 {
        let mut violation = 0f64;
        for (r, s) in offsets.iter().zip(response) {
            let rotated = rotate(*r, [q[3], q[4], q[5]]);
            let movement = add([q[0], q[1], q[2]], sub(rotated, *r));
            let distance = dot(movement, movement).sqrt();
            let bound = (s[12] as f64).max(0.) * 0.9;
            if distance <= bound + 1e-10 {
                continue;
            }
            let n = scale(movement, 1. / distance.max(1e-20));
            let angular = cross(rotated, n);
            let gradient = [n[0], n[1], n[2], angular[0], angular[1], angular[2]];
            let direction = std::array::from_fn::<_, 6, _>(|i| {
                (0..6)
                    .map(|j| inverse_newton[j][i] * gradient[j])
                    .sum::<f64>()
            });
            let denominator = (0..6).map(|i| gradient[i] * direction[i]).sum::<f64>();
            if denominator <= 1e-20 {
                continue;
            }
            let error = distance - bound;
            for i in 0..6 {
                q[i] -= error * direction[i] / denominator;
            }
            violation = violation.max(error);
        }
        for grip in &bindings {
            let (rotated, hand) = grip.sample(c, q);
            let delta = sub(hand, grip.shoulder);
            let distance = dot(delta, delta).sqrt();
            let (error, sign) = if distance > grip.maximum {
                (distance - grip.maximum, 1.)
            } else if distance < grip.minimum {
                (grip.minimum - distance, -1.)
            } else {
                continue;
            };
            joint_restricted = true;
            let n = if distance > 1e-12 {
                scale(delta, sign / distance)
            } else {
                [sign, 0., 0.]
            };

            let angular = cross(rotated, n);
            let gradient = [n[0], n[1], n[2], angular[0], angular[1], angular[2]];
            let direction = std::array::from_fn::<_, 6, _>(|i| {
                (0..6)
                    .map(|j| inverse_newton[j][i] * gradient[j])
                    .sum::<f64>()
            });
            let denominator = (0..6).map(|i| gradient[i] * direction[i]).sum::<f64>();
            if denominator > 1e-20 {
                for i in 0..6 {
                    q[i] -= error * direction[i] / denominator;
                }
            }
            violation = violation.max(error);
        }
        if violation < 1e-9 {
            break;
        }
    }
    // Certification uses the reconstructed rigid points, not a chord approximation.
    for _ in 0..32 {
        let valid = offsets.iter().zip(response).all(|(r, s)| {
            let p = add([q[0], q[1], q[2]], sub(rotate(*r, [q[3], q[4], q[5]]), *r));
            dot(p, p).sqrt() <= (s[12] as f64).max(0.) + 1e-10
        });
        if valid && bindings.iter().all(|g| g.valid(c, q, 1e-8)) {
            break;
        }
        q = q.map(|x| x * 0.5);
    }
    let out = offsets
        .iter()
        .map(|r| add(add(c, [q[0], q[1], q[2]]), rotate(*r, [q[3], q[4], q[5]])).map(|v| v as f32))
        .collect::<Vec<_>>();
    let certified = out.iter().zip(current).zip(response).all(|((p, old), r)| {
        let delta = std::array::from_fn(|i| p[i] as f64 - old[i] as f64);
        dot(delta, delta).sqrt() <= (r[12] as f64).max(0.) + 1e-10
    });
    let grasp_certified =
        super::rigid_fit::fit(model, &out).is_some_and(|(new_origin, new_axes)| {
            let center = add(
                new_origin,
                std::array::from_fn(|i| (0..3).map(|j| new_axes[j][i] * body.center[j]).sum()),
            );
            grasp.is_none_or(|g| {
                g.grips.iter().all(|grip| {
                    let point = if grip.slide.is_some() {
                        [grip.point[0], 0., grip.point[2]]
                    } else {
                        grip.point
                    };
                    let offset =
                        std::array::from_fn(|i| (0..3).map(|j| new_axes[j][i] * point[j]).sum());
                    Binding {
                        base: sub(add(new_origin, offset), center),
                        up: new_axes[1],
                        shoulder: grip.shoulder,
                        minimum: grip.minimum,
                        maximum: grip.maximum,
                        slide: grip.slide,
                    }
                    .valid(center, [0.; 6], 2e-7)
                })
            })
        });
    if !certified || !grasp_certified {
        return Ok(Projection {
            positions: current.to_vec(),
            restricted: true,
            grasp_feasible: bindings.iter().all(|g| g.valid(c, [0.; 6], 2e-7)),
        });
    }
    if out.iter().flatten().any(|x| !x.is_finite()) {
        return Err("non-finite rigid contact pose".into());
    }
    Ok(Projection {
        positions: out,
        restricted: joint_restricted,
        grasp_feasible: true,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_contact_tip_preserves_rigid_pivot_motion_and_clearance() {
        let model: Vec<V> = [0f32, 0.3]
            .into_iter()
            .flat_map(|y| {
                [-0.1f32, 0.1]
                    .into_iter()
                    .flat_map(move |x| [-0.003f32, 0.003].map(move |z| [x, y, z]))
            })
            .collect();
        let pivot = model[0];
        let angle = 0.01f64;
        let goal: Vec<V> = model
            .iter()
            .map(|p| {
                let y = (p[1] - pivot[1]) as f64;
                let z = (p[2] - pivot[2]) as f64;
                [
                    p[0],
                    (pivot[1] as f64 + y * angle.cos() - z * angle.sin()) as f32,
                    (pivot[2] as f64 + y * angle.sin() + z * angle.cos()) as f32,
                ]
            })
            .collect();
        let mut response = vec![[0f32; 13]; model.len()];
        for r in &mut response {
            r[12] = 0.05;
        }
        response[0][12] = 0.00001;
        let body = Body {
            mass: 3.5,
            center: [0., 0.55, 0.],
            inertia: [[0.8, 0., 0.], [0., 0.009, 0.], [0., 0., 0.8]],
            max_force: 600.,
            max_torque: 90.,
        };
        let result = project(&model, &model, &goal, &response, &body, None, 1. / 60.)
            .unwrap()
            .positions;
        for ((p, old), r) in result.iter().zip(&model).zip(&response) {
            let delta = std::array::from_fn(|i| p[i] as f64 - old[i] as f64);
            assert!(dot(delta, delta).sqrt() <= r[12] as f64 + 1e-10);
        }
        let moving = result
            .iter()
            .zip(&model)
            .map(|(p, old)| {
                let d = std::array::from_fn(|i| p[i] as f64 - old[i] as f64);
                dot(d, d).sqrt()
            })
            .fold(0f64, f64::max);
        assert!(
            moving > 0.001,
            "one near-fixed tip must not freeze the entire rigid tool"
        );
        for i in 0..model.len() {
            for j in i + 1..model.len() {
                let before = std::array::from_fn(|k| model[i][k] as f64 - model[j][k] as f64);
                let after = std::array::from_fn(|k| result[i][k] as f64 - result[j][k] as f64);
                assert!((dot(before, before) - dot(after, after)).abs() < 1e-7);
            }
        }
    }
    fn grasp_fixture() -> (Vec<V>, Vec<[f32; 13]>, Body, Grasp) {
        let model: Vec<V> = [0f32, 0.3]
            .into_iter()
            .flat_map(|y| {
                [-0.1f32, 0.1]
                    .into_iter()
                    .flat_map(move |x| [-0.003f32, 0.003].map(move |z| [x, y, z]))
            })
            .collect();
        let mut responses = vec![[0f32; 13]; model.len()];
        for r in &mut responses {
            r[12] = 0.5;
        }
        let body = Body {
            mass: 3.5,
            center: [0., 0.55, 0.],
            inertia: [[0.8, 0., 0.], [0., 0.009, 0.], [0., 0., 0.8]],
            max_force: 600.,
            max_torque: 90.,
        };
        let grasp = Grasp {
            grips: [
                Grip {
                    point: [0., 1.1, 0.],
                    shoulder: [0., 1., 0.],
                    minimum: 0.,
                    maximum: 0.5,
                    slide: Some([0.85, 1.295]),
                },
                Grip {
                    point: [0., 1.45, 0.],
                    shoulder: [0., 1., 0.],
                    minimum: 0.01,
                    maximum: 0.5,
                    slide: None,
                },
            ],
        };
        (model, responses, body, grasp)
    }
    #[test]
    fn held_grasp_respects_reach_and_release_removes_the_joint() {
        let (model, responses, body, grasp) = grasp_fixture();
        let goal: Vec<V> = model.iter().map(|p| [p[0], p[1] + 0.2, p[2]]).collect();
        let held = project(&model, &model, &goal, &responses, &body, Some(&grasp), 0.05).unwrap();
        assert!(held.grasp_feasible);
        assert!(held.restricted);
        let (o, a) = super::super::rigid_fit::fit(&model, &held.positions).unwrap();
        let hand = add(o, scale(a[1], 1.45));
        assert!(dot(sub(hand, [0., 1., 0.]), sub(hand, [0., 1., 0.])).sqrt() <= 0.5000002);
        let free = project(&model, &model, &goal, &responses, &body, None, 0.05).unwrap();
        let (o, a) = super::super::rigid_fit::fit(&model, &free.positions).unwrap();
        let hand = add(o, scale(a[1], 1.45));
        assert!(
            hand[1] > 1.6,
            "released tool must not retain a hidden hand joint"
        );
    }
    #[test]
    fn infeasible_joint_with_blocked_contact_holds_and_reports_resistance() {
        let (model, mut responses, body, mut grasp) = grasp_fixture();
        for r in &mut responses {
            r[12] = 0.;
        }
        grasp.grips[1].shoulder = [10., 1., 0.];
        let result = project(
            &model,
            &model,
            &model,
            &responses,
            &body,
            Some(&grasp),
            1. / 120.,
        )
        .unwrap();
        assert!(result.restricted);
        assert!(!result.grasp_feasible);
        assert_eq!(result.positions, model);
    }
}
