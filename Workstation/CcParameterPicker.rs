//! Cardinal's automation parameter menu, using the same geometry for drawing
//! and input. Source: DawLaneModalSystem.cpp and DawLaneRenderSystem.cpp.
//! Target identity/parameter enumeration remain owned by the document adapter.
use serde_json::{Value, json};

pub(crate) type Rect = [f64; 4];
const WIDTH: f64 = 180.;
const ROW: f64 = 18.;
const PADDING: f64 = 6.;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Selection {
    pub track: String,
    pub slot: usize,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub consumed: bool,
    /// Preserve the original button's press/release path so its activation
    /// toggles this menu; no other row control receives a consumed click.
    pub button_hit: bool,
    pub selection: Option<Selection>,
}
#[derive(Clone, Debug)]
struct Open {
    track: String,
    labels: Vec<String>,
    selected: usize,
    hover: Option<usize>,
    geometry: Option<Geometry>,
}
#[derive(Clone, Copy, Debug)]
struct Geometry {
    button: Rect,
    menu: Rect,
    viewport: [f64; 2],
}
#[derive(Default)]
pub(crate) struct State {
    open: Option<Open>,
}

#[derive(Clone, Copy)]
pub(crate) struct Theme {
    pub front: [f64; 3],
    pub top: [f64; 3],
    pub side: [f64; 3],
    pub text: [f64; 3],
}
impl Default for Theme {
    fn default() -> Self {
        // Exact fallback colors in DawLaneRenderSystem; a color library
        // overrides these, including the normal Cardinal cream theme.
        Self { front: [0.20, 0.23, 0.24], top: [0.30, 0.34, 0.35],
            side: [0.12, 0.13, 0.14], text: [0.88, 0.92, 0.93] }
    }
}
impl Theme {
    pub fn from_color_library(value: &Value) -> Self {
        let value = value.get("colors").unwrap_or(value);
        let parse = |key: &str, fallback: [f64; 3]| -> [f64; 3] {
            let v = &value[key];
            if let Some(text) = v.as_str().and_then(|s| s.strip_prefix('#')) {
                if text.len() == 6 && text.is_ascii() {
                    let mut color = [0.; 3];
                    for (i, channel) in color.iter_mut().enumerate() {
                        let Ok(byte) = u8::from_str_radix(&text[i*2..i*2+2], 16) else {return fallback;};
                        *channel = f64::from(byte) / 255.;
                    }
                    return color;
                }
            }
            if let Ok(color) = serde_json::from_value::<[f64; 3]>(v.clone()) {
                if color.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v)) {return color;}
            }
            fallback
        };
        let d = Self::default();
        Self { front: parse("ButtonFront", d.front), top: parse("ButtonTopHighlight", d.top),
            side: parse("ButtonSideShadow", d.side), text: parse("MiraText", d.text) }
    }
}

fn contains(rect: Rect, point: [f64; 2]) -> bool {
    point[0] >= rect[0] && point[0] <= rect[0]+rect[2]
        && point[1] >= rect[1] && point[1] <= rect[1]+rect[3]
}
fn layout(button: Rect, viewport: [f64; 2], count: usize) -> Option<Geometry> {
    if count == 0 || button.iter().chain(&viewport).any(|n| !n.is_finite())
        || button[2] <= 0. || button[3] <= 0. || viewport[0] <= 0. || viewport[1] <= 0. {return None;}
    let height = count as f64 * ROW + PADDING * 2.;
    let mut x = button[0];
    let mut y = button[1] + button[3] + 6.;
    if x + WIDTH > viewport[0] - 8. {x = viewport[0] - WIDTH - 8.;}
    x = x.max(8.);
    if y + height > viewport[1] - 8. {y = button[1] - 6. - height;}
    y = y.max(8.);
    Some(Geometry { button, menu: [x, y, WIDTH, height], viewport })
}

impl State {
    pub fn is_open(&self) -> bool {self.open.is_some()}
    pub fn track(&self) -> Option<&str> {self.open.as_ref().map(|o| o.track.as_str())}
    pub fn close(&mut self) {self.open = None;}
    /// Called only when the authored target_param button activates. Empty
    /// parameter lists close it, as openParamMenuForTrack does in Cardinal.
    pub fn toggle(&mut self, track: &str, labels: &[String], selected: usize) {
        if self.track() == Some(track) || track.is_empty() || labels.is_empty() {self.close();return;}
        let selected = selected.min(labels.len()-1);
        self.open = Some(Open { track: track.into(), labels: labels.to_vec(), selected,
            hover: Some(selected), geometry: None });
    }
    /// Refresh an already-open target after catalog/device changes; this never
    /// creates a menu. A vanished track or empty parameter list closes it.
    pub fn sync(&mut self, track: &str, labels: &[String], selected: usize) {
        if self.track() != Some(track) || labels.is_empty() {self.close();return;}
        if let Some(open) = &mut self.open {
            open.labels = labels.to_vec();open.selected = selected.min(labels.len()-1);
            if open.hover.is_some_and(|slot| slot >= labels.len()) {open.hover = None;}
        }
    }
    /// Input is the original per-frame pressed edge, not a held-button level.
    /// No wheel, keyboard, search or Escape behavior is added to this picker.
    pub fn update(&mut self, pointer: Option<[f64; 2]>, left_pressed: bool,
        pending_action: bool, button: Option<Rect>, viewport: [f64; 2]) -> Outcome {
        let Some(open) = &mut self.open else {return Outcome::default();};
        let Some(geometry) = button.and_then(|button| layout(button, viewport, open.labels.len())) else {
            self.close();return Outcome::default();
        };
        open.geometry = Some(geometry);
        let button_hit = pointer.is_some_and(|p| contains(geometry.button, p));
        let menu_hit = pointer.is_some_and(|p| contains(geometry.menu, p));
        open.hover = None;
        if menu_hit {
            // C++ int(float) truncates toward zero. In the original top 6px
            // padding localY is negative but yields row0, intentionally retained.
            let local_y = pointer.unwrap()[1] - geometry.menu[1] - PADDING;
            let hover = (local_y / ROW) as isize;
            if hover >= 0 && (hover as usize) < open.labels.len() {open.hover = Some(hover as usize);}
        }
        let mut result = Outcome { consumed: menu_hit || button_hit, button_hit, selection: None };
        if left_pressed {
            result.consumed = true;
            if menu_hit && open.hover.is_some() {
                if !pending_action {result.selection = Some(Selection {track: open.track.clone(), slot: open.hover.unwrap()});}
                self.close();
            } else if !button_hit {
                self.close();
            }
        }
        result
    }
    /// This is the complete native overlay scene. Rectangles are in the same
    /// logical top-left pixel space used by hit testing; selected beats hover.
    pub fn presentation(&self, theme: Theme) -> Value {
        let Some(open) = &self.open else {return Value::Null;};
        let Some(g) = open.geometry else {return Value::Null;};
        json!({"track":open.track,"viewport":g.viewport,"button_rect":g.button,"rect":g.menu,
            "bevel":4.,"row_height":ROW,"padding":PADDING,
            "colors":{"front":theme.front,"top":theme.top,"side":theme.side,"text":theme.text,
                "selected":[0.20,0.42,0.68],"hover":[0.28,0.33,0.37]},
            "rows":open.labels.iter().enumerate().map(|(slot,label)| {
                let y = g.menu[1]+PADDING+slot as f64*ROW;
                json!({"slot":slot,"label":label,"rect":[g.menu[0]+2.,y,g.menu[2]-4.,ROW],
                    "text_origin":[g.menu[0]+8.,y+13.],"selected":slot==open.selected,"hovered":open.hover==Some(slot)})
            }).collect::<Vec<_>>()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn labels() -> Vec<String> {vec!["CUTOFF".into(),"RESONANCE".into(),"BYPASS".into()]}
    fn ready() -> State {let mut s=State::default();s.toggle("cc",&labels(),1);s}
    #[test] fn exact_layout_flips_above_and_clamps_screen_edges() {
        assert_eq!(layout([300.,100.,36.,36.],[800.,600.],3).unwrap().menu,[300.,142.,180.,66.]);
        assert_eq!(layout([780.,560.,36.,36.],[800.,600.],3).unwrap().menu,[612.,488.,180.,66.]);
        assert_eq!(layout([-30.,0.,36.,36.],[100.,20.],3).unwrap().menu,[8.,8.,180.,66.]);
        // Cardinal leaves a long list extending beyond the screen; no added scroll scheme.
        assert_eq!(layout([10.,10.,36.,36.],[800.,600.],100).unwrap().menu,[10.,8.,180.,1812.]);
    }
    #[test] fn pick_is_on_press_and_padding_matches_cpp_truncation() {
        let mut s=ready();let o=s.update(Some([310.,143.]),false,false,Some([300.,100.,36.,36.]),[800.,600.]);
        assert!(o.consumed);assert!(o.selection.is_none());
        let p=s.presentation(Theme::default());assert_eq!(p["rows"][0]["hovered"],true);
        assert_eq!(p["rows"][1]["selected"],true);
        let o=s.update(Some([310.,143.]),true,false,Some([300.,100.,36.,36.]),[800.,600.]);
        assert_eq!(o.selection,Some(Selection{track:"cc".into(),slot:0}));assert!(!s.is_open());
    }
    #[test] fn button_is_retained_for_toggle_and_outside_press_is_consumed() {
        let mut s=ready();let o=s.update(Some([310.,110.]),true,false,Some([300.,100.,36.,36.]),[800.,600.]);
        assert!(o.button_hit&&o.consumed);assert!(s.is_open());assert!(o.selection.is_none());
        s.toggle("cc",&labels(),1);assert!(!s.is_open());s.toggle("cc",&labels(),1);
        let o=s.update(Some([1.,1.]),false,false,Some([300.,100.,36.,36.]),[800.,600.]);assert!(!o.consumed&&s.is_open());
        let o=s.update(Some([1.,1.]),true,false,Some([300.,100.,36.,36.]),[800.,600.]);assert!(o.consumed&&!s.is_open());
    }
    #[test] fn pending_action_missing_anchor_and_catalog_removal_cancel_safely() {
        let mut s=ready();let o=s.update(Some([310.,170.]),true,true,Some([300.,100.,36.,36.]),[800.,600.]);assert!(o.consumed&&o.selection.is_none()&&!s.is_open());
        s.toggle("cc",&labels(),99);assert_eq!(s.open.as_ref().unwrap().selected,2);
        s.sync("cc",&["ONLY".into()],99);assert_eq!(s.open.as_ref().unwrap().selected,0);
        assert!(s.update(None,false,false,None,[800.,600.]).selection.is_none());assert!(!s.is_open());
        s.toggle("cc",&labels(),0);s.sync("other",&labels(),0);assert!(!s.is_open());
        s.toggle("cc",&[],0);assert!(!s.is_open());
    }
    #[test] fn scene_uses_source_bevel_rows_and_theme_library() {
        let mut s=ready();s.update(None,false,false,Some([300.,100.,36.,36.]),[800.,600.]);
        let theme=Theme::from_color_library(&json!({"colors":{"ButtonFront":"#E6E0D4","MiraText":"#3B342E"}}));
        let p=s.presentation(theme);assert_eq!(p["rect"],json!([300.,142.,180.,66.]));
        assert_eq!(p["rows"][0]["rect"],json!([302.,148.,176.,18.]));assert_eq!(p["rows"][0]["text_origin"],json!([308.,161.]));
        assert_eq!(p["bevel"],4.);assert_eq!(p["colors"]["front"],json!([230./255.,224./255.,212./255.]));
        assert_eq!(p["colors"]["text"],json!([59./255.,52./255.,46./255.]));
    }
}
