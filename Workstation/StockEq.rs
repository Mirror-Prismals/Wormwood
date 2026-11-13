//! Cardinal DawFaderSystem's U SHAPING graph geometry and gestures.
use serde_json::{Value, json};
pub(crate) enum Press {
    Slope(usize),
    Drag(usize),
}
pub(crate) struct Controls {
    pub rect: [f64; 4],
    pub plot: [f64; 4],
    pub slopes: [[f64; 4]; 2],
    pub bypass: [f64; 4],
    pub remove: [f64; 4],
    scale: f64,
}
fn hit(r: [f64; 4], p: [f64; 2]) -> bool {
    p[0] >= r[0] && p[0] < r[0] + r[2] && p[1] >= r[1] && p[1] < r[1] + r[3]
}
fn hz(s: &Value, i: usize) -> f64 {
    s["edges"][i]["frequency_hz"]
        .as_f64()
        .unwrap_or(if i == 0 { 180. } else { 6000. })
}
fn gain(s: &Value) -> f64 {
    s["gain_db"].as_f64().unwrap_or(0.)
}
impl Controls {
    pub(crate) fn new(r: [f64; 4], c: f64) -> Self {
        let w = (r[2] - 24. * c) * 0.5;
        let size = 28. * c;
        let gap = 6. * c;
        let left = r[0] + (r[2] - 2. * size - gap) * 0.5;
        Self {
            rect: r,
            plot: [
                r[0] + 16. * c,
                r[1] + 30. * c,
                r[2] - 32. * c,
                r[3] - 64. * c,
            ],
            slopes: std::array::from_fn(|i| {
                [
                    r[0] + 10. * c + i as f64 * (w + 4. * c),
                    r[1] + r[3] - 24. * c,
                    w,
                    18. * c,
                ]
            }),
            bypass: [left, r[1] - 8. * c - size, size, size],
            remove: [left + size + gap, r[1] - 8. * c - size, size, size],
            scale: c,
        }
    }
    pub(crate) fn x(&self, hz: f64) -> f64 {
        self.plot[0] + (hz.clamp(20., 20000.) / 20.).ln() / 1000f64.ln() * self.plot[2]
    }
    pub(crate) fn y(&self, db: f64) -> f64 {
        self.plot[1] + (0.5 - db.clamp(-24., 24.) / 48.) * self.plot[3]
    }
    pub(crate) fn press(&self, p: [f64; 2], state: &Value) -> Option<(Press, f64)> {
        for edge in 0..2 {
            if hit(self.slopes[edge], p) {
                return Some((
                    Press::Slope(edge),
                    state["edges"][edge]["slope_db_oct"].as_f64().unwrap_or(12.),
                ));
            }
        }
        let r = self.plot;
        let c = self.scale;
        if !hit(
            [r[0] - 7. * c, r[1] - 7. * c, r[2] + 14. * c, r[3] + 14. * c],
            p,
        ) {
            return None;
        }
        let low = self.x(hz(state, 0));
        let high = self.x(hz(state, 1));
        let middle = (low + high) * 0.5;
        let pole =
            if (p[0] - middle).abs() <= 6. * c && (p[1] - self.y(gain(state))).abs() <= 7. * c {
                2
            } else if (p[0] - low).abs().min((p[0] - high).abs()) <= 8. * c {
                if (p[0] - low).abs() <= (p[0] - high).abs() {
                    0
                } else {
                    1
                }
            } else if p[0] >= low && p[0] <= high {
                2
            } else {
                return None;
            };
        Some((Press::Drag(pole), 0.))
    }
    pub(crate) fn drag_command(
        &self,
        id: u64,
        pole: usize,
        p: [f64; 2],
        s: &Value,
        rate: f64,
    ) -> Value {
        let (parameter, value) = if pole == 2 {
            (
                "gain_db",
                24. - 48. * ((p[1] - self.plot[1]) / self.plot[3]).clamp(0., 1.),
            )
        } else {
            let maximum = (rate.max(100.) * 0.475).min(20000.);
            let floor = if pole == 0 {
                20.
            } else {
                (hz(s, 0) * 1.01).min(maximum)
            };
            let ceiling = if pole == 0 {
                (hz(s, 1) / 1.01).min(maximum / 1.01)
            } else {
                maximum
            };
            (
                if pole == 0 { "low_hz" } else { "high_hz" },
                (20. * 1000f64.powf(((p[0] - self.plot[0]) / self.plot[2]).clamp(0., 1.)))
                    .clamp(floor, ceiling.max(floor)),
            )
        };
        json!({"op":"stock_parameter","id":id,"parameter":parameter,"value":value})
    }
}
pub(crate) fn slope_command(id: u64, edge: usize, current: f64) -> Value {
    let next = [6, 12, 18, 24, 30, 36, 48]
        .into_iter()
        .find(|n| *n as f64 > current)
        .unwrap_or(6);
    json!({"op":"stock_parameter","id":id,"parameter":if edge==0{"low_slope"}else{"high_slope"},"value":next})
}
pub(crate) fn presentation(insert: &Value, rect: [f64; 4], scale: f64) -> Value {
    let controls = Controls::new(rect, scale);
    let s = &insert["state"];
    let response = insert["response_db"]
        .as_array()
        .map(|a| {
            a.iter()
                .enumerate()
                .map(|(i, db)| {
                    [
                        controls.plot[0] + i as f64 / 96. * controls.plot[2],
                        controls.y(db.as_f64().unwrap_or(0.)),
                    ]
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({"gem_surface":true,"rect":rect,"plot":controls.plot,"slopes":controls.slopes,"bypass":controls.bypass,"remove":controls.remove,
        "bypassed":insert["bypassed"],"title":format!("U SHAPING  {:+0.1} dB",gain(s)),
        "slope_labels":std::array::from_fn::<_,2,_>(|i|format!("{:0.0} Hz / {} dB",hz(s,i),s["edges"][i]["slope_db_oct"].as_i64().unwrap_or(12))),
        "edges":[controls.x(hz(s,0)),controls.x(hz(s,1))],"gain_y":controls.y(gain(s)),"zero_y":controls.y(0.),"response":response,"scale":scale})
}
