//! Unlit settings patch bay. Changes use the document's ordinary SetBus/Undo path.
use super::{Rect, contains};
use crate::workstation::piano_roll::Input;
use serde_json::{Value, json};

#[derive(Default)]
pub(crate) struct Editor {
    buses: Vec<Value>,
    devices: Vec<Value>,
    selected: Option<String>,
    disconnect: Option<String>,
    patches: Vec<Value>,
    scroll: usize,
}
struct Port {
    rect: Rect,
    id: String,
    channel: String,
    name: String,
}
impl Editor {
    pub fn sync(&mut self, doc: &Value) {
        self.buses = doc["buses"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["kind"] == "matrix")
            .cloned()
            .collect();
        self.devices = doc["physical_outputs"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if self
            .selected
            .as_ref()
            .is_some_and(|id| !self.buses.iter().any(|b| b["id"] == *id))
        {
            self.cancel();
        }
        self.scroll = self.scroll.min(self.buses.len().saturating_sub(6));
    }
    pub fn cancel(&mut self) {
        self.selected = None;
        self.disconnect = None;
    }
    pub fn take_patches(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.patches)
    }
    fn rect(r: Rect, s: f64, x: f64, y: f64, w: f64, h: f64) -> Rect {
        [r[0] + x * s, r[1] + y * s, w * s, h * s]
    }
    fn sources(&self, r: Rect, s: f64) -> Vec<Port> {
        self.buses
            .iter()
            .skip(self.scroll)
            .take(6)
            .enumerate()
            .map(|(i, b)| Port {
                rect: Self::rect(r, s, 244., 151. + i as f64 * 42., 20., 20.),
                id: b["id"].as_str().unwrap_or("").into(),
                channel: String::new(),
                name: b["name"].as_str().unwrap_or("Matrix").into(),
            })
            .collect()
    }
    fn targets(&self, r: Rect, s: f64) -> Vec<Port> {
        self.devices
            .iter()
            .flat_map(|d| {
                d["channels"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(move |c| (d, c))
            })
            .take(6)
            .enumerate()
            .map(|(i, (d, c))| Port {
                rect: Self::rect(r, s, 432., 151. + i as f64 * 42., 20., 20.),
                id: d["id"].as_str().unwrap_or("").into(),
                channel: c.as_str().unwrap_or("").into(),
                name: format!(
                    "{} / {}",
                    d["name"].as_str().unwrap_or("Device"),
                    c.as_str().unwrap_or("")
                ),
            })
            .collect()
    }
    fn disconnect_rect(p: &Port, s: f64) -> Rect {
        [p.rect[0] - 36. * s, p.rect[1], 20. * s, 20. * s]
    }
    pub fn update(&mut self, input: &Input, r: Rect, s: f64, pressed: bool, released: bool) {
        let Some(point) = input.pointer else {
            if released {
                self.disconnect = None;
            }
            return;
        };
        if contains(Self::rect(r, s, 20., 130., 250., 280.), point)
            && input.wheel[1].is_finite()
            && input.wheel[1] != 0.
        {
            if input.wheel[1] < 0. {
                self.scroll = (self.scroll + 1).min(self.buses.len().saturating_sub(6));
            } else {
                self.scroll = self.scroll.saturating_sub(1);
            }
        }
        let sources = self.sources(r, s);
        if pressed {
            self.disconnect = sources
                .iter()
                .find(|p| contains(Self::disconnect_rect(p, s), point))
                .map(|p| p.id.clone());
            if let Some(p) = sources.iter().find(|p| contains(p.rect, point)) {
                self.selected = Some(p.id.clone());
            }
        }
        if released {
            if let Some(id) = self.disconnect.take() {
                if sources
                    .iter()
                    .any(|p| p.id == id && contains(Self::disconnect_rect(p, s), point))
                {
                    self.patches.push(
                        json!({"id":id,"output":"OFF","undo_label":"Disconnect matrix output"}),
                    );
                    self.selected = None;
                }
            } else if let Some(p) = self.targets(r, s).iter().find(|p| contains(p.rect, point)) {
                if let Some(id) = self.selected.take() {
                    self.patches.push(json!({"id":id,"output":p.id,"output_channel":p.channel,"undo_label":"Connect matrix output"}));
                }
            }
        }
    }
    pub fn draw(
        &self,
        r: Rect,
        s: f64,
        pointer: Option<[f64; 2]>,
        faces: &mut Vec<Value>,
        labels: &mut Vec<Value>,
    ) {
        let face = |r: Rect, c: [u8; 3]| json!({"rect":r,"color":c,"radius":0.,"alpha":1.});
        let label = |r: Rect, t: &str, size: f64| json!({"rect":r,"text":t,"size":size,"center":false,"color":[225,235,237]});
        faces.push(face(Self::rect(r, s, 20., 130., 250., 280.), [35, 43, 47]));
        faces.push(face(Self::rect(r, s, 426., 130., 254., 280.), [35, 43, 47]));
        labels.push(label(
            Self::rect(r, s, 30., 108., 230., 22.),
            "MATRIX BUSES",
            14. * s,
        ));
        labels.push(label(
            Self::rect(r, s, 436., 108., 230., 22.),
            "PHYSICAL OUTPUTS",
            14. * s,
        ));
        labels.push(label(
            Self::rect(r, s, 20., 414., 660., 20.),
            "Drag an output port to a device channel, or click the two ports.",
            12. * s,
        ));
        labels.push(label(
            Self::rect(r, s, 20., 438., 660., 20.),
            "X disconnects. Changes are immediate. Close Settings to use content Undo.",
            11. * s,
        ));
        let sources = self.sources(r, s);
        let targets = self.targets(r, s);
        // Wire geometry is inexpensive display content, never optical artwork.
        let wire = |a: [f64; 2], b: [f64; 2], color: [u8; 3], faces: &mut Vec<Value>| {
            let mut old = a;
            for i in 1..=64 {
                let t = i as f64 / 64.;
                let q = t * t * (3. - 2. * t);
                let next = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * q];
                faces.push(face(
                    [
                        old[0].min(next[0]) - s,
                        old[1].min(next[1]) - s,
                        (next[0] - old[0]).abs() + 2. * s,
                        (next[1] - old[1]).abs() + 2. * s,
                    ],
                    color,
                ));
                old = next;
            }
        };
        for p in &sources {
            let b = self.buses.iter().find(|b| b["id"] == p.id).unwrap();
            if let Some(target) = targets
                .iter()
                .find(|t| b["output"] == t.id && b["output_channel"] == t.channel)
            {
                wire(
                    [p.rect[0] + 10. * s, p.rect[1] + 10. * s],
                    [target.rect[0] + 10. * s, target.rect[1] + 10. * s],
                    [92, 178, 192],
                    faces,
                );
            }
        }
        if let (Some(p), Some(point)) = (
            sources
                .iter()
                .find(|p| self.selected.as_ref() == Some(&p.id)),
            pointer.filter(|p| contains(Self::rect(r, s, 20., 130., 660., 280.), *p)),
        ) {
            wire(
                [p.rect[0] + 10. * s, p.rect[1] + 10. * s],
                point,
                [239, 199, 112],
                faces,
            );
        }
        for p in &sources {
            let selected = self.selected.as_ref() == Some(&p.id);
            faces.push(face(
                p.rect,
                if selected {
                    [239, 199, 112]
                } else {
                    [92, 178, 192]
                },
            ));
            let x = Self::disconnect_rect(p, s);
            faces.push(face(x, [80, 57, 59]));
            labels.push(label(x, "X", 12. * s));
            labels.push(label(
                [r[0] + 30. * s, p.rect[1] - 6. * s, 168. * s, 20. * s],
                &p.name,
                12. * s,
            ));
            let b = self.buses.iter().find(|b| b["id"] == p.id).unwrap();
            let route = if b["output"] == "OFF" {
                "Disconnected".to_string()
            } else {
                format!(
                    "{} / {}",
                    self.devices
                        .iter()
                        .find(|d| d["id"] == b["output"])
                        .and_then(|d| d["name"].as_str())
                        .unwrap_or("Unavailable"),
                    b["output_channel"].as_str().unwrap_or("")
                )
            };
            labels.push(label(
                [r[0] + 30. * s, p.rect[1] + 12. * s, 168. * s, 18. * s],
                &route,
                9. * s,
            ));
        }
        for p in &targets {
            faces.push(face(p.rect, [92, 178, 192]));
            labels.push(label(
                [p.rect[0] + 30. * s, p.rect[1], 204. * s, 24. * s],
                &p.name,
                12. * s,
            ));
        }
        if sources.is_empty() {
            labels.push(label(
                Self::rect(r, s, 30., 155., 220., 50.),
                "Add a Matrix Bus from the browser.",
                12. * s,
            ));
        }
        if targets.is_empty() {
            labels.push(label(
                Self::rect(r, s, 436., 155., 220., 50.),
                "No physical outputs available.",
                12. * s,
            ));
        }
        if self.buses.len() > 6 {
            labels.push(label(
                Self::rect(r, s, 30., 390., 220., 20.),
                &format!(
                    "{}-{} of {}  (scroll)",
                    self.scroll + 1,
                    (self.scroll + 6).min(self.buses.len()),
                    self.buses.len()
                ),
                10. * s,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn doc() -> Value {
        json!({"buses":[{"id":"matrix-L","name":"Matrix L","kind":"matrix","output":"flown_array","output_channel":"L"},{"id":"mix-1","kind":"mix"}],"physical_outputs":[{"id":"flown_array","name":"Flown Array","channels":["L","S","FF","R"]}]})
    }
    #[test]
    fn drag_routes_and_disconnects_without_touching_mix_buses() {
        let mut e = Editor::default();
        e.sync(&doc());
        let r = [0., 0., 700., 520.];
        assert_eq!(e.sources(r, 1.).len(), 1);
        let mut input = Input {
            pointer: Some([254., 161.]),
            left: true,
            ..Default::default()
        };
        e.update(&input, r, 1., true, false);
        input.pointer = Some([442., 245.]);
        input.left = false;
        e.update(&input, r, 1., false, true);
        assert_eq!(
            e.take_patches(),
            vec![
                json!({"id":"matrix-L","output":"flown_array","output_channel":"FF","undo_label":"Connect matrix output"})
            ]
        );
        input.pointer = Some([218., 161.]);
        input.left = true;
        e.update(&input, r, 1., true, false);
        input.left = false;
        e.update(&input, r, 1., false, true);
        assert_eq!(e.take_patches()[0]["output"], "OFF");
    }
    #[test]
    fn cancellation_and_removed_sources_never_emit_routes() {
        let mut e = Editor::default();
        e.sync(&doc());
        let r = [0., 0., 700., 520.];
        let input = Input {
            pointer: Some([254., 161.]),
            ..Default::default()
        };
        e.update(&input, r, 1., true, false);
        e.cancel();
        e.update(
            &Input {
                pointer: Some([442., 161.]),
                ..Default::default()
            },
            r,
            1.,
            false,
            true,
        );
        assert!(e.take_patches().is_empty());
        e.update(&input, r, 1., true, false);
        e.sync(&json!({"buses":[]}));
        assert!(e.selected.is_none());
    }
}
