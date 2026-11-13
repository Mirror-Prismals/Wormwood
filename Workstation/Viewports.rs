//! Session-local timeline views. No document mutations or audio callback work.
use super::{
    panel_layout::{Options, Viewport},
    panel_motion::Snapshot,
    piano_roll::Input,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[derive(Default)]
pub(crate) struct State {
    pub properties: bool,
    selected: usize,
    hidden_global: BTreeSet<String>,
    shown_previews: BTreeSet<String>,
    views: Vec<BTreeSet<String>>,
    tracks: Vec<Value>,
    scroll: [f64; 2],
    down: bool,
    capture: bool,
    pressed: Option<Action>,
}
#[derive(Clone, Debug, PartialEq)]
enum Action {
    Page(bool),
    Track(String),
    Select(usize),
    Add,
}
struct Row {
    rect: [f64; 4],
    text: String,
    action: Option<Action>,
    selected: bool,
}
struct Layout {
    clip: [f64; 4],
    sections: [[f64; 4]; 2],
    rows: Vec<Row>,
    limits: [f64; 2],
    scale: f64,
}
fn inside(r: [f64; 4], p: [f64; 2]) -> bool {
    p.iter().all(|n| n.is_finite())
        && p[0] >= r[0]
        && p[0] < r[0] + r[2]
        && p[1] >= r[1]
        && p[1] < r[1] + r[3]
}
impl State {
    pub fn sync(&mut self, tracks: &[Value]) {
        self.tracks = tracks.to_vec();
    }
    pub fn visible(&self, id: &str) -> bool {
        if self.selected == 0 {
            if id==super::mix_preview::ID {self.shown_previews.contains(id)} else {!self.hidden_global.contains(id)}
        } else {
            self.views[self.selected - 1].contains(id)
        }
    }
    pub fn status(&self) -> Value {
        json!({"page":if self.properties{"properties"}else{"browser"},"selected":self.selected,
        "viewports":std::iter::once(json!({"name":"Global","locked":true})).chain(self.views.iter().enumerate().map(|(i,_)|json!({"name":format!("Viewport {}",i+1),"locked":false}))).collect::<Vec<_>>(),
        "tracks":self.tracks.iter().map(|t|json!({"id":t["id"],"name":t["name"],"visible":self.visible(t["id"].as_str().unwrap_or(""))})).collect::<Vec<_>>()})
    }
    fn apply(&mut self, a: Action) {
        match a {
            Action::Page(page) => self.properties = page,
            Action::Track(id) => {
                let set = if self.selected == 0 {
                    if id==super::mix_preview::ID {&mut self.shown_previews} else {&mut self.hidden_global}
                } else {
                    &mut self.views[self.selected - 1]
                };
                if !set.remove(&id) {
                    set.insert(id);
                }
            }
            Action::Select(i) => {
                self.selected = i;
                self.scroll[0] = 0.;
            }
            Action::Add => {
                self.views.push(BTreeSet::new());
                self.selected = self.views.len();
                self.scroll[0] = 0.;
                self.scroll[1] = -1.0e6;
            }
        }
    }
    fn layout(
        &self,
        view: Viewport,
        motion: &Snapshot,
        options: Options,
    ) -> Result<Layout, String> {
        let base = super::plugin_browser::layout(view, motion, options, &[], 0, 0.)?;
        let c = view.canvas_scale;
        let body = base.clip;
        let nav = 32. * c;
        let clip = [body[0], body[1] - nav, body[2], body[3] + nav];
        let mut rows = vec![];
        for (i, name) in ["Browser", "Properties"].iter().enumerate() {
            rows.push(Row {
                rect: [
                    clip[0] + i as f64 * clip[2] * 0.5,
                    clip[1],
                    clip[2] * 0.5,
                    nav,
                ],
                text: name.to_string(),
                action: Some(Action::Page(i == 1)),
                selected: self.properties == (i == 1),
            });
        }
        let h = 30. * c;
        let top_height = (body[3] * 0.62).max(0.);
        let lower = (body[3] - top_height).max(0.);
        let sections = [
            [body[0], body[1] + h, body[2], (top_height - h).max(0.)],
            [
                body[0],
                body[1] + top_height + h,
                body[2],
                (lower - h).max(0.),
            ],
        ];
        let limits = [
            (sections[0][3] - self.tracks.len() as f64 * h).min(0.),
            (sections[1][3] - (self.views.len() + 1) as f64 * h).min(0.),
        ];
        if self.properties {
            rows.push(Row {
                rect: [body[0], body[1], body[2], h],
                text: "TRACKS".into(),
                action: None,
                selected: false,
            });
            rows.push(Row {
                rect: [body[0], body[1] + top_height, body[2] - h, h],
                text: "VIEWPORTS".into(),
                action: None,
                selected: false,
            });
            rows.push(Row {
                rect: [body[0] + body[2] - h, body[1] + top_height, h, h],
                text: "+".into(),
                action: Some(Action::Add),
                selected: false,
            });
            for (i, t) in self.tracks.iter().enumerate() {
                let id = t["id"].as_str().unwrap_or("");
                let rect = [
                    sections[0][0],
                    sections[0][1] + self.scroll[0].clamp(limits[0], 0.) + i as f64 * h,
                    body[2],
                    h,
                ];
                if rect[1] >= sections[0][1] && rect[1] + h <= sections[0][1] + sections[0][3] {
                    rows.push(Row {
                        rect,
                        text: format!(
                            "[{}] {}",
                            if self.visible(id) { "x" } else { " " },
                            t["name"].as_str().unwrap_or(id)
                        ),
                        action: Some(Action::Track(id.into())),
                        selected: false,
                    });
                }
            }
            for i in 0..=self.views.len() {
                let rect = [
                    sections[1][0],
                    sections[1][1] + self.scroll[1].clamp(limits[1], 0.) + i as f64 * h,
                    body[2],
                    h,
                ];
                if rect[1] >= sections[1][1] && rect[1] + h <= sections[1][1] + sections[1][3] {
                    rows.push(Row {
                        rect,
                        text: if i == 0 {
                            "Global [locked]".into()
                        } else {
                            format!("Viewport {i}")
                        },
                        action: Some(Action::Select(i)),
                        selected: i == self.selected,
                    });
                }
            }
        }
        Ok(Layout {
            clip,
            sections,
            rows,
            limits,
            scale: c,
        })
    }
    pub fn update(
        &mut self,
        input: &Input,
        view: Viewport,
        motion: &Snapshot,
        options: Options,
        blocked: bool,
        events: &[(bool, [f64; 2])],
    ) -> Result<(bool, bool), String> {
        let layout = self.layout(view, motion, options)?;
        for i in 0..2 {
            self.scroll[i] = self.scroll[i].clamp(layout.limits[i], 0.);
        }
        let mut changed = false;
        let mut owned = self.capture;
        let valid = |p| inside([0., 0., view.width, view.height], p) && inside(layout.clip, p);
        if blocked || input.escape || input.pointer.is_none() {
            self.pressed = None;
            self.capture = false;
            self.down = input.left;
            return Ok((false, false));
        }
        for (down, p) in events
            .iter()
            .copied()
            .chain(std::iter::once((input.left, input.pointer.unwrap())))
        {
            if down && !self.down {
                let hit = valid(p)
                    .then(|| {
                        layout
                            .rows
                            .iter()
                            .find(|r| inside(r.rect, p))
                            .and_then(|r| r.action.clone())
                    })
                    .flatten();
                self.capture = hit.is_some() || (self.properties && valid(p));
                self.pressed = hit;
            }
            owned |= self.capture;
            if !down && self.down {
                if let Some(a) = self.pressed.take() {
                    if valid(p)
                        && layout
                            .rows
                            .iter()
                            .any(|r| r.action.as_ref() == Some(&a) && inside(r.rect, p))
                    {
                        self.apply(a);
                        changed = true;
                    }
                }
                self.capture = false;
            }
            self.down = down;
        }
        if self.properties && input.pointer.is_some_and(valid) {
            owned = true;
            for i in 0..2 {
                if input.pointer.is_some_and(|p| inside(layout.sections[i], p))
                    && input.wheel[1] != 0.
                {
                    self.scroll[i] = (self.scroll[i] + input.wheel[1] * 30. * layout.scale)
                        .clamp(layout.limits[i], 0.);
                    changed = true;
                }
            }
        }
        Ok((owned, changed))
    }
    pub fn presentation(
        &self,
        view: Viewport,
        motion: &Snapshot,
        options: Options,
        mut browser: Value,
    ) -> Result<Value, String> {
        let l = self.layout(view, motion, options)?;
        let labels=l.rows.iter().map(|r|json!({"text":r.text.to_uppercase(),"rect":[r.rect[0]+4.*l.scale,r.rect[1],(r.rect[2]-8.*l.scale).max(0.),r.rect[3]],"size":16.8*l.scale,"color":[244,241,232]})).collect::<Vec<_>>();
        let faces=l.rows.iter().filter(|r|r.selected).map(|r|json!({"rect":r.rect,"radius":4.*l.scale,"color":[51.,132.6,209.1],"alpha":0.5})).collect::<Vec<_>>();
        let group = json!({"clip":l.clip,"labels":labels,"browser_faces":faces});
        if self.properties {
            Ok(json!([group]))
        } else {
            if !browser.is_array() {
                browser = json!([]);
            }
            browser.as_array_mut().unwrap().push(group);
            Ok(browser)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn view() -> Viewport {
        Viewport {
            width: 1280.,
            height: 800.,
            canvas_scale: 1.,
        }
    }
    fn motion() -> Snapshot {
        let mut m = super::super::panel_motion::State::default();
        m.set_all_open(true);
        m.advance(1.);
        m.snapshot()
    }
    fn tracks() -> Vec<Value> {
        vec![
            json!({"id":"audio","name":"Audio 1"}),
            json!({"id":"midi","name":"Midi 1"}),
        ]
    }
    #[test]
    fn viewports_preserve_independent_visibility_and_new_track_defaults() {
        let mut s = State::default();
        s.sync(&tracks());
        assert!(s.visible("audio"));
        s.apply(Action::Track("audio".into()));
        assert!(!s.visible("audio"));
        s.apply(Action::Add);
        assert_eq!(s.selected, 1);
        assert!(!s.visible("audio"));
        assert!(!s.visible("midi"));
        s.apply(Action::Track("midi".into()));
        assert!(s.visible("midi"));
        s.apply(Action::Add);
        assert!(!s.visible("midi"));
        s.apply(Action::Select(1));
        assert!(s.visible("midi"));
        assert!(!s.visible("new"));
        s.apply(Action::Select(0));
        assert!(s.visible("new"));
        assert!(!s.visible("audio"));
        s.sync(&[]);
        s.sync(&tracks());
        assert!(!s.visible("audio"));
        assert_eq!(s.status()["viewports"][0]["locked"], true);
    }
    fn click(s: &mut State, action: Action) {
        let m = motion();
        let l = s.layout(view(), &m, Options::default()).unwrap();
        let r = l
            .rows
            .iter()
            .find(|r| r.action.as_ref() == Some(&action))
            .unwrap()
            .rect;
        let p = [r[0] + r[2] * 0.5, r[1] + r[3] * 0.5];
        let (owned, changed) = s
            .update(
                &Input {
                    pointer: Some(p),
                    ..Default::default()
                },
                view(),
                &m,
                Options::default(),
                false,
                &[(true, p), (false, p)],
            )
            .unwrap();
        assert!(owned && changed);
    }
    #[test]
    fn viewports_queued_clicks_drive_pages_checkboxes_and_selection() {
        let mut s = State::default();
        s.sync(&tracks());
        click(&mut s, Action::Page(true));
        click(&mut s, Action::Track("audio".into()));
        assert!(!s.visible("audio"));
        click(&mut s, Action::Add);
        assert_eq!(s.selected, 1);
        click(&mut s, Action::Track("midi".into()));
        click(&mut s, Action::Select(0));
        assert!(s.visible("midi"));
        assert!(!s.visible("audio"));
        let packet = s
            .presentation(view(), &motion(), Options::default(), Value::Null)
            .unwrap();
        assert!(super::super::plugin_surface::valid(
            &json!({"groups":packet})
        ));
    }
    #[test]
    fn viewports_cancelled_release_does_not_toggle() {
        let mut s = State::default();
        s.properties = true;
        s.sync(&tracks());
        let m = motion();
        let l = s.layout(view(), &m, Options::default()).unwrap();
        let r = l
            .rows
            .iter()
            .find(|r| r.action == Some(Action::Track("audio".into())))
            .unwrap()
            .rect;
        let p = [r[0] + 5., r[1] + 5.];
        s.update(
            &Input {
                pointer: Some(p),
                left: true,
                ..Default::default()
            },
            view(),
            &m,
            Options::default(),
            false,
            &[],
        )
        .unwrap();
        s.update(
            &Input {
                pointer: Some([1200., 790.]),
                ..Default::default()
            },
            view(),
            &m,
            Options::default(),
            false,
            &[],
        )
        .unwrap();
        assert!(s.visible("audio"));
    }
    #[test]
    fn viewports_scroll_lists_independently_and_keep_packet_valid() {
        let mut s = State::default();
        s.properties = true;
        s.sync(
            &(0..200)
                .map(|i| json!({"id":format!("t{i}"),"name":format!("Track {i}")}))
                .collect::<Vec<_>>(),
        );
        for _ in 0..30 {
            s.apply(Action::Add);
        }
        let m = motion();
        let l = s.layout(view(), &m, Options::default()).unwrap();
        let p = [l.sections[0][0] + 4., l.sections[0][1] + 4.];
        s.update(
            &Input {
                pointer: Some(p),
                wheel: [0., -1000.],
                ..Default::default()
            },
            view(),
            &m,
            Options::default(),
            false,
            &[],
        )
        .unwrap();
        assert!(s.scroll[0] < 0.);
        assert_eq!(s.scroll[1], l.limits[1]);
        assert!(super::super::plugin_surface::valid(
            &json!({"groups":s.presentation(view(),&m,Options::default(),Value::Null).unwrap()})
        ));
    }
}

#[cfg(test)]
mod arrangement_tests {
    use super::super::arrangement::Arrangement;
    use super::*;
    #[test]
    fn viewports_filter_timeline_keep_comp_rows_and_preserve_source_document() {
        let snapshot = json!({"revision":1,"tracks":[
            {"id":"a","name":"Audio 1","kind":"audio","gain":1.,"mute":false,"solo":false},
            {"id":"b","name":"Midi 1","kind":"midi","gain":1.,"mute":false,"solo":false}],
            "clips":[],"audio_playlists":[{"track":"a","expanded":true,"lanes":[{"name":"Main","clips":[]},{"name":"Take 1","clips":[]}]}]});
        let original = snapshot.clone();
        let mut a = Arrangement::default();
        a.sync(&snapshot).unwrap();
        let rect = super::super::piano_roll::Rect {
            left: 0.,
            top: 0.,
            width: 1000.,
            height: 800.,
        };
        assert_eq!(
            a.presentation(rect, None, Value::Null)["tracks"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
        assert_eq!(a.parent_insertion_index(1), 1); // Insert after the parent, before its take row.
        a.viewports.apply(Action::Track("a".into()));
        a.sync(&snapshot).unwrap();
        let packet = a.presentation(rect, None, Value::Null);
        assert_eq!(packet["tracks"].as_array().unwrap().len(), 1);
        assert_eq!(packet["tracks"][0]["id"], "b");
        assert_eq!(a.parent_insertion_index(0), 1);
        a.viewports.apply(Action::Add);
        a.sync(&snapshot).unwrap();
        assert_eq!(a.presentation(rect, None, Value::Null)["tracks"], json!([]));
        a.viewports.apply(Action::Track("a".into()));
        a.sync(&snapshot).unwrap();
        assert_eq!(
            a.presentation(rect, None, Value::Null)["tracks"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(snapshot, original);
    }
}

#[cfg(test)]
mod mix_preview_tests {
    use super::*;
    use super::super::{arrangement::Arrangement,piano_roll::{Input,Rect}};
    #[test]
    fn mix_preview_viewports_default_hidden_preserve_choice_and_refuse_edits() {
        let doc=json!({"revision":1,"tracks":[],"clips":[],"buses":[{"id":"mix-1","kind":"mix"}]});
        let mut a=Arrangement::default();a.sync(&doc).unwrap();
        let id=super::super::mix_preview::ID;
        assert!(!a.viewports.visible(id));assert_eq!(a.viewports.status()["tracks"][0]["id"],id);
        a.viewports.apply(Action::Track(id.into()));a.sync(&doc).unwrap();assert!(a.viewports.visible(id));
        let rect=Rect{left:0.,top:0.,width:1000.,height:500.};
        assert_eq!(a.presentation(rect,None,Value::Null)["tracks"][0]["read_only"],true);
        let point=[400.,80.];
        for left in [true,false,true,false] {
            assert!(a.update(&Input{pointer:Some(point),left,..Default::default()},rect).is_empty());
        }
        assert_eq!(a.selected_track(),None);
        assert!(a.browser_plugin_target(rect,point).is_none());
        assert!(a.browser_stock_target(rect,point).is_none());
        assert!(a.comp_drop_slot(rect,point).is_none());assert!(a.loopback_drop(rect,point).is_err());
        a.viewports.apply(Action::Add);assert!(!a.viewports.visible(id));
        a.viewports.apply(Action::Select(0));a.sync(&doc).unwrap();assert!(a.viewports.visible(id));
        a.sync(&json!({"revision":2,"tracks":[],"clips":[],"buses":[]})).unwrap();
        a.sync(&json!({"revision":3,"tracks":[],"clips":[],"buses":[{"id":"mix-1","kind":"mix"}]})).unwrap();
        assert!(a.viewports.visible(id));
    }
}
