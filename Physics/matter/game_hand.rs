//! Player-scale tool trajectories. All targets are rigid, reach-limited hand
//! requests; OGC accepts motion and the rendered wrists follow that result.
use super::*;
use crate::physics::{
    rigid_contact::{Grasp, Grip},
    rigid_fit,
};

#[derive(Clone)]
pub(in crate::physics::matter) struct Pose {
    pub origin: D,
    pub axes: [D; 3],
}
impl Pose {
    pub fn point(&self, p: V) -> D {
        addd(
            self.origin,
            std::array::from_fn(|k| (0..3).map(|j| self.axes[j][k] * p[j] as f64).sum()),
        )
    }
    pub fn vertices(&self, t: &Tool) -> Vec<V> {
        t.model
            .iter()
            .map(|&p| self.point(p).map(|v| v as f32))
            .collect()
    }
    pub fn fit(t: &Tool) -> Self {
        let (origin, axes) = rigid_fit::fit(&t.model, &t.p).unwrap();
        Self { origin, axes }
    }
    pub fn at(point: D, anchor: V, axes: [D; 3]) -> Self {
        Self {
            origin: subd(
                point,
                std::array::from_fn(|k| (0..3).map(|j| axes[j][k] * anchor[j] as f64).sum()),
            ),
            axes,
        }
    }
    pub fn blend(&self, b: &Self, t: f32) -> Self {
        let t = t.clamp(0., 1.) as f64;
        let a = quaternion(self.axes);
        let mut q = quaternion(b.axes);
        if (0..4).map(|i| a[i] * q[i]).sum::<f64>() < 0. {
            q = q.map(|v| -v);
        }
        let mut q: [f64; 4] = std::array::from_fn(|i| a[i] * (1. - t) + q[i] * t);
        let l = q.iter().map(|v| v * v).sum::<f64>().sqrt();
        q = q.map(|v| v / l);
        let [x, y, z, w] = q;
        Self {
            origin: addd(muld(self.origin, 1. - t), muld(b.origin, t)),
            axes: [
                [
                    1. - 2. * (y * y + z * z),
                    2. * (x * y + z * w),
                    2. * (x * z - y * w),
                ],
                [
                    2. * (x * y - z * w),
                    1. - 2. * (x * x + z * z),
                    2. * (y * z + x * w),
                ],
                [
                    2. * (x * z + y * w),
                    2. * (y * z - x * w),
                    1. - 2. * (x * x + y * y),
                ],
            ],
        }
    }
}
fn quaternion(a: [D; 3]) -> [f64; 4] {
    let trace = a[0][0] + a[1][1] + a[2][2];
    if trace > 0. {
        let s = (trace + 1.).sqrt() * 2.;
        return [
            (a[1][2] - a[2][1]) / s,
            (a[2][0] - a[0][2]) / s,
            (a[0][1] - a[1][0]) / s,
            s * 0.25,
        ];
    }
    let i = (0..3).max_by(|&i, &j| a[i][i].total_cmp(&a[j][j])).unwrap();
    let j = (i + 1) % 3;
    let k = (i + 2) % 3;
    let s = (1. + a[i][i] - a[j][j] - a[k][k]).max(0.).sqrt() * 2.;
    let mut q = [0.; 4];
    q[i] = s * 0.25;
    q[j] = (a[i][j] + a[j][i]) / s;
    q[k] = (a[i][k] + a[k][i]) / s;
    q[3] = (a[j][k] - a[k][j]) / s;
    q
}
fn smooth(t: f32, a: f32, b: f32) -> f32 {
    let u = ((t - a) / (b - a)).clamp(0., 1.);
    u * u * (3. - 2. * u)
}
fn axes(f: &Frame, world: [D; 3]) -> [D; 3] {
    world.map(|v| f.vector(v).map(f64::from))
}
fn pitch(a: [D; 3], angle: f64) -> [D; 3] {
    let (s, c) = angle.sin_cos();
    [
        a[0],
        addd(muld(a[1], c), muld(a[2], s)),
        addd(muld(a[1], -s), muld(a[2], c)),
    ]
}

pub(super) fn grasp(tool: &Tool, f: &Frame, reach: &Value) -> Option<Grasp> {
    let points = tool.wrists();
    let shoulder = [
        d(&reach["shoulders"][0]).ok()?,
        d(&reach["shoulders"][1]).ok()?,
    ];
    Some(Grasp {
        grips: std::array::from_fn(|i| Grip {
            point: points[i].map(f64::from),
            shoulder: f.local(shoulder[i]).map(f64::from),
            minimum: reach["arm_min"][i].as_f64().unwrap_or(0.) + 0.005,
            maximum: (reach["arm_max"][i].as_f64().unwrap_or(0.65) - 0.025).max(0.1),
            slide: None,
        }),
    })
}
pub(super) fn feedback(tool: &Tool, f: &Frame) -> Value {
    let pose = Pose::fit(tool);
    let wrists = tool
        .wrists()
        .map(|p| f.world(pose.point(p).map(|v| v as f32)));
    let directions = tool.finger_directions().map(|v| {
        let local: V = std::array::from_fn(|k| {
            (0..3).map(|j| pose.axes[j][k] * v[j] as f64).sum::<f64>() as f32
        });
        unitd(subd(f.world(local), f.origin))
    });
    json!({"active":true,"locked":true,"left":wrists[0],"right":wrists[1],"left_fingers":directions[0],"right_fingers":directions[1]})
}
/// Project a requested hand pose, never the accepted contact geometry. A target
/// outside the arms' workspace cannot become a projectile destination.
pub(in crate::physics::matter) fn reachable(mut p: Pose, t: &Tool, g: Option<&Grasp>) -> Pose {
    if let Some(g) = g {
        for _ in 0..16 {
            let mut error: f64 = 0.;
            for grip in &g.grips {
                let hand = p.point(grip.point.map(|v| v as f32));
                let delta = subd(hand, grip.shoulder);
                let l = lend(delta);
                let want = l.clamp(grip.minimum + 0.008, grip.maximum - 0.008);
                p.origin = addd(p.origin, muld(unitd(delta), want - l));
                error = error.max((want - l).abs());
            }
            if error < 1e-6 {
                break;
            }
        }
    }
    let _ = t;
    p
}
pub(super) fn carry(tool: &Tool, f: &Frame, eye: D, dir: D, up: D, reach: &Value) -> Pose {
    let right = unitd(crossd(dir, up));
    let forward = unitd(crossd(up, right));
    let grips = tool.grips();
    let hand = if let Ok(s) = d(&reach["shoulders"][1]) {
        addd(addd(s, muld(forward, 0.32)), muld(up, -0.30))
    } else {
        addd(
            addd(addd(eye, muld(right, 0.22)), muld(forward, 0.30)),
            muld(up, -0.52),
        )
    };
    let basis = if tool.transverse_haft() {
        // Haft points back into the hands; blade faces forward/down.
        let x = unitd(addd(muld(forward, -0.37), muld(up, 0.93)));
        let z = right;
        let y = crossd(z, x);
        axes(f, [x, y, z])
    } else {
        pitch(axes(f, [muld(right, -1.), up, forward]), -0.38)
    };
    reachable(
        Pose::at(f.local(hand).map(f64::from), grips[1], basis),
        tool,
        grasp(tool, f, reach).as_ref(),
    )
}
pub(super) fn can_work(
    tool: &Tool,
    f: &Frame,
    point: D,
    normal: D,
    kind: usize,
    eye: D,
    dir: D,
    up: D,
    reach: &Value,
) -> bool {
    let Some(g) = grasp(tool, f, reach) else {
        return true;
    };
    let s = Stroke {
        point,
        normal,
        time: match kind {
            0 => 0.9,
            3 => 1.1,
            _ => 0.58,
        },
        power: 450.,
        pour: false,
        start: carry(tool, f, eye, dir, up, reach),
    };
    let raw = work(tool, f, &s, kind, eye, dir, up, &Value::Null);
    let projected = reachable(raw.clone(), tool, Some(&g));
    lend(subd(raw.origin, projected.origin)) < 0.09
}
pub(super) fn work(
    tool: &Tool,
    f: &Frame,
    s: &Stroke,
    kind: usize,
    eye: D,
    dir: D,
    up: D,
    reach: &Value,
) -> Pose {
    let rest = carry(tool, f, eye, dir, up, reach);
    let t = s.time;
    let right = unitd(crossd(dir, up));
    let forward = unitd(crossd(up, right));
    if s.pour {
        let p = Pose::at(
            rest.point(tool.grips()[1]),
            tool.grips()[1],
            pitch(rest.axes, -2.0),
        );
        return s
            .start
            .blend(&p, smooth(t, 0., 0.65))
            .blend(&rest, smooth(t, 1.15, 1.85));
    }
    let contact = f.local(s.point).map(f64::from);
    let mut poses: Vec<(f32, Pose)> = vec![(0., s.start.clone())];
    if kind == 0 {
        // Bowl faces away during insertion, then turns upward while the
        // handle levers toward the player. The same edge stays under the sod.
        let a = axes(f, [muld(right, -1.), up, forward]);
        let edge = [0., -0.29, 0.];
        let point = |depth: f64, advance: f64| {
            addd(
                contact,
                f.vector(addd(muld(up, depth), muld(forward, advance)))
                    .map(f64::from),
            )
        };
        poses.extend([
            (0.38, Pose::at(point(0.055, -0.03), edge, pitch(a, -0.20))),
            (0.90, Pose::at(point(-0.14, 0.), edge, pitch(a, -0.12))),
            (1.60, Pose::at(point(-0.14, 0.10), edge, pitch(a, -1.28))),
            (2.25, Pose::at(point(0.22, -0.12), edge, pitch(a, -1.28))),
            (3.0, rest.clone()),
        ]);
    } else if kind == 3 {
        let a = axes(f, [muld(right, -1.), up, forward]);
        let point = |h: f64, d: f64| {
            addd(
                contact,
                f.vector(addd(muld(up, h), muld(forward, d))).map(f64::from),
            )
        };
        poses.extend([
            (0.4, Pose::at(point(0.14, -0.16), [0.; 3], pitch(a, -1.05))),
            (1.1, Pose::at(point(0.035, 0.10), [0.; 3], pitch(a, -1.05))),
            (1.7, Pose::at(point(0.12, 0.02), [0.; 3], a)),
            (2.4, rest.clone()),
        ]);
    } else {
        // A diagonal chop: the haft extends back/up toward the player while
        // the cutting edge travels inward/down. Its plane crosses the fibres.
        let n = unitd(s.normal);
        let (basis, edge) = if kind == 2 {
            let x = unitd(addd(addd(muld(n, 0.60), muld(right, 0.75)), muld(up, 0.27)));
            let y = unitd(subd(n, muld(x, dotd(n, x))));
            (axes(f, [x, y, crossd(x, y)]), [0., -0.19, 0.])
        } else {
            let x = unitd(addd(muld(forward, -0.8), muld(up, 0.6)));
            let y = unitd(subd(up, muld(x, dotd(up, x))));
            (axes(f, [x, y, crossd(x, y)]), [0., -0.28, 0.08])
        };
        let cut = basis[1];
        poses.extend([
            (0.32, Pose::at(addd(contact, muld(cut, 0.32)), edge, basis)),
            (0.58, Pose::at(addd(contact, muld(cut, -0.09)), edge, basis)),
            (0.98, Pose::at(addd(contact, muld(cut, -0.09)), edge, basis)),
            (1.42, Pose::at(addd(contact, muld(cut, 0.16)), edge, basis)),
            (2.0, rest.clone()),
        ]);
    }
    for pair in poses.windows(2) {
        if t <= pair[1].0 {
            let a = reachable(pair[0].1.clone(), tool, grasp(tool, f, reach).as_ref());
            let b = reachable(pair[1].1.clone(), tool, grasp(tool, f, reach).as_ref());
            return reachable(
                a.blend(&b, smooth(t, pair[0].0, pair[1].0)),
                tool,
                grasp(tool, f, reach).as_ref(),
            );
        }
    }
    rest
}
