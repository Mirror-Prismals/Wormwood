//! Bounded kinematic deer behavior. Ground acceptance is supplied by DeerSystem.
use crate::audio::{V, add, cross, dot, length, mul, sub, unit};
pub(crate) const STARTLE_DISTANCE: f64 = 14.;
pub(crate) const SAFE_DISTANCE: f64 = 34.;
pub(crate) const MAX_SPEED: f64 = 15.;
#[derive(Clone, Copy, Debug)]
pub(crate) struct Ground {
    pub position: V,
    pub up: V,
}
#[derive(Clone, Debug)]
pub(crate) struct Animal {
    pub position: V,
    pub up: V,
    pub forward: V,
    pub speed: f64,
    pub fleeing: bool,
    pub safe_seconds: f64,
    pub stride: f64,
    pub time: f64,
    pub run_blend: f64,
    pub alert_blend: f64,
    pub look_yaw: f64,
    pub look_pitch: f64,
    pub watch_side: f64,
}
fn tangent(v: V, up: V) -> V {
    unit(sub(v, mul(up, dot(v, up))))
}
pub(crate) fn rotate(v: V, up: V, angle: f64) -> V {
    let (s, c) = angle.sin_cos();
    add(
        add(mul(v, c), mul(cross(up, v), s)),
        mul(up, dot(up, v) * (1. - c)),
    )
}
impl Animal {
    pub fn new(g: Ground, heading: V, phase: f64) -> Self {
        Self {
            position: g.position,
            up: g.up,
            forward: tangent(heading, g.up),
            speed: 0.,
            fleeing: false,
            safe_seconds: 0.,
            stride: phase,
            time: phase,
            run_blend: 0.,
            alert_blend: 0.,
            look_yaw: 0.,
            look_pitch: 0.,
            watch_side: if phase > 0.18 { -1. } else { 1. },
        }
    }
    pub fn advance<F>(&mut self, player: V, dt: f64, mut ground: F) -> Result<(), String>
    where
        F: FnMut(V, V, V) -> Result<Option<Ground>, String>,
    {
        if !dt.is_finite() || !(0. ..=0.25).contains(&dt) || player.iter().any(|v| !v.is_finite()) {
            return Err("bounded deer frame required".into());
        }
        let distance = length(sub(self.position, player));
        if distance < STARTLE_DISTANCE {
            if !self.fleeing {
                let side = dot(sub(player, self.position), cross(self.up, self.forward));
                if side.abs() > 0.3 {
                    self.watch_side = side.signum();
                }
            }
            self.fleeing = true;
            self.safe_seconds = 0.;
        }
        if self.fleeing {
            if distance > SAFE_DISTANCE {
                self.safe_seconds += dt;
            } else {
                self.safe_seconds = 0.;
            }
            if self.safe_seconds >= 4. {
                self.fleeing = false;
                self.safe_seconds = 0.;
            }
        }
        self.time += dt;
        if !self.fleeing && self.speed < 0.1 && distance < 24. {
            let toward = tangent(sub(player, self.position), self.up);
            if length(toward) > 0.9 {
                let angle =
                    dot(cross(self.forward, toward), self.up).atan2(dot(self.forward, toward));
                // Turn the body only enough to keep a comfortable head/neck aim.
                let amount = (angle.abs() - 0.8).max(0.).min(0.75 * dt) * angle.signum();
                self.forward = rotate(self.forward, self.up, amount);
            }
        }
        let steps = (dt / 0.08).ceil().max(1.) as usize;
        let step_dt = dt / steps as f64;
        let mut traveled = 0.;
        for _ in 0..steps {
            let target = if self.fleeing { MAX_SPEED } else { 0. };
            self.speed += (target - self.speed).clamp(-5. * step_dt, 12. * step_dt);
            if self.speed < 1e-6 {
                continue;
            }
            let away = tangent(sub(self.position, player), self.up);
            let away = if length(away) > 0.9 {
                away
            } else {
                self.forward
            };
            let escape = away;
            let current = tangent(self.forward, self.up);
            let turn = dot(cross(current, escape), self.up)
                .atan2(dot(current, escape))
                .clamp(-4. * step_dt, 4. * step_dt);
            let heading = if self.fleeing {
                rotate(current, self.up, turn)
            } else {
                current
            };
            self.forward = heading;
            let travel = self.speed * step_dt;
            let mut accepted = None;
            for angle in [0., 0.65, -0.65, 1.2, -1.2] {
                let direction = rotate(heading, self.up, angle);
                // Never move toward the threat, including when turning in place.
                if self.fleeing && dot(direction, away) < 0.15 {
                    continue;
                }
                let wanted = add(self.position, mul(direction, travel));
                if let Some(g) = ground(self.position, wanted, direction)? {
                    let delta = sub(g.position, self.position);
                    if dot(delta, self.up).abs() <= travel * 0.75 + 0.025
                        && length(delta) <= travel * 1.4 + 0.05
                    {
                        accepted = Some((g, direction));
                        break;
                    }
                }
            }
            if let Some((g, direction)) = accepted {
                traveled += length(sub(g.position, self.position));
                self.position = g.position;
                self.up = g.up;
                self.forward = tangent(direction, g.up);
            } else {
                self.speed = 0.;
            }
        }
        self.stride = (self.stride + traveled / 7.6).rem_euclid(1.);
        let target = if traveled > 1e-6 {
            (self.speed / 2.).min(1.)
        } else {
            0.
        };
        self.run_blend += (target - self.run_blend).clamp(-dt / 0.18, dt / 0.18);
        let alert = if !self.fleeing && self.speed < 0.1 && distance < 24. {
            1.
        } else {
            0.
        };
        self.alert_blend += (alert - self.alert_blend).clamp(-dt / 0.25, dt / 0.25);
        let toward = sub(sub(player, self.position), mul(self.up, 1.7));
        let right = unit(cross(self.up, self.forward));
        let mut yaw = dot(toward, right).atan2(dot(toward, self.forward));
        // Hold the chosen shoulder across the rear seam; tiny observer motion
        // behind the animal must not flip its neck from side to side.
        if yaw.abs() > 2.2 {
            yaw = self.watch_side * yaw.abs();
        } else if !self.fleeing && yaw.abs() > 0.2 {
            self.watch_side = yaw.signum();
        }
        let target_yaw = if alert > 0. {
            yaw.clamp(-2.443460953, 2.443460953)
        } else {
            0.
        };
        let horizontal = dot(toward, right).hypot(dot(toward, self.forward));
        let target_pitch = if alert > 0. {
            dot(toward, self.up).atan2(horizontal).clamp(-0.45, 0.4)
        } else {
            0.
        };
        let response = 1. - (-10. * dt).exp();
        self.look_yaw += ((target_yaw - self.look_yaw) * response).clamp(-2.6 * dt, 2.6 * dt);
        self.look_pitch += ((target_pitch - self.look_pitch) * response).clamp(-1.5 * dt, 1.5 * dt);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn deer() -> Animal {
        Animal::new(
            Ground {
                position: [10., 0., 0.],
                up: [0., 1., 0.],
            },
            [1., 0., 0.],
            0.,
        )
    }
    fn flat(_: V, p: V, _: V) -> Result<Option<Ground>, String> {
        Ok(Some(Ground {
            position: [p[0], 0., p[2]],
            up: [0., 1., 0.],
        }))
    }
    #[test]
    fn deer_flees_increases_distance_and_eventually_settles() {
        let mut d = deer();
        for _ in 0..20 {
            d.advance([0.; 3], 0.1, flat).unwrap();
        }
        assert!(d.fleeing);
        assert!(length(d.position) > 18.);
        assert!(d.run_blend > 0.9);
        for _ in 0..160 {
            d.advance([0.; 3], 0.1, flat).unwrap();
        }
        assert!(!d.fleeing);
        assert!(d.speed < 1e-6);
        assert!(d.run_blend < 1e-6);
    }
    #[test]
    fn deer_does_not_flee_at_a_distance_or_thrash_at_threshold() {
        let mut d = deer();
        d.advance([-20., 0., 0.], 0.1, flat).unwrap();
        assert!(!d.fleeing);
        assert_eq!(d.position, [10., 0., 0.]);
        d.advance([0.; 3], 0.1, flat).unwrap();
        d.advance([-10., 0., 0.], 0.1, flat).unwrap();
        assert!(d.fleeing);
    }
    #[test]
    fn deer_blocked_route_stops_without_teleport_or_running_in_place() {
        let mut d = deer();
        for _ in 0..40 {
            d.advance([0.; 3], 0.1, |_, _, _| Ok(None)).unwrap();
        }
        assert_eq!(d.position, [10., 0., 0.]);
        assert_eq!(d.stride, 0.);
        assert_eq!(d.run_blend, 0.);
        assert!(d.fleeing);
    }
    #[test]
    fn deer_rejects_cliffs_steers_and_handles_planetary_up() {
        let mut d = deer();
        d.advance([0.; 3], 0.1, |_, p, _| {
            Ok(Some(Ground {
                position: add(p, [0., 3., 0.]),
                up: [0., 1., 0.],
            }))
        })
        .unwrap();
        assert_eq!(d.position, [10., 0., 0.]);
        d.advance([0.; 3], 0.1, |_, p, heading| {
            if heading[2].abs() < 0.1 {
                Ok(None)
            } else {
                flat([0.; 3], p, heading)
            }
        })
        .unwrap();
        assert!(d.position[0] > 10. && d.position[2].abs() > 0.);
        let mut d = Animal::new(
            Ground {
                position: [0., 10., 0.],
                up: [0., 0., 1.],
            },
            [0., 1., 0.],
            0.,
        );
        d.advance([0.; 3], 0.1, |_, p, _| {
            Ok(Some(Ground {
                position: p,
                up: [0., 0., 1.],
            }))
        })
        .unwrap();
        assert!(d.position[1] > 10.);
        assert!(d.advance([0.; 3], 1., flat).is_err());
        assert!(d.advance([f64::NAN, 0., 0.], 0.1, flat).is_err());
    }
    #[test]
    fn deer_watches_only_while_alert_and_faces_forward_when_fleeing() {
        let mut d = deer();
        let start = d.position;
        // Begin with an alert head turn, then startle: gaze must release.
        for _ in 0..20 {
            d.advance([10., 1.7, 20.], 0.05, flat).unwrap();
        }
        assert!(d.look_yaw.abs() > 0.3);
        for _ in 0..40 {
            d.advance([0., 1.7, 0.], 0.05, flat).unwrap();
        }
        assert!(d.fleeing);
        assert!(length(d.position) > length(start));
        assert!(d.look_yaw.abs() < 0.01);
        assert!(d.look_pitch.abs() < 0.01);
        assert!(d.speed > 14. && d.speed <= MAX_SPEED);
        assert!(d.alert_blend < 0.01);
        assert!(dot(d.forward, unit(d.position)) > 0.99);
        // Even a blocked fleeing deer must not switch back to watching.
        for _ in 0..20 {
            d.advance([0., 1.7, 0.], 0.05, |_, _, _| Ok(None)).unwrap();
        }
        assert!(d.fleeing);
        assert!(d.look_yaw.abs() < 0.01);
        let mut alert = deer();
        for _ in 0..20 {
            alert.advance([10., 1.7, 20.], 0.05, flat).unwrap();
        }
        assert!(!alert.fleeing);
        assert!(alert.look_yaw.abs() > 0.3);
        assert_eq!(alert.position, start);
    }
}
