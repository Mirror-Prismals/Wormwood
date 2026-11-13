//! Cardinal right-page KEYBOARD switch and the numbered page selectors.
//! Layout: PanelSystem.cpp:updateTransportOffsets + DawSidePanelLayout::PackOptions.
//! Activation: ButtonSystem.cpp:updateButtonState; state: DawTransportSystem.cpp.
use super::panel_layout::{Rect, Viewport};
use serde_json::{Value, json};

pub(crate) const HEADER: usize = super::track_io_scene::END;
pub(crate) const POSE_BASE: usize = HEADER + 4;
pub(crate) const STRIDE: usize = 5;
pub(crate) const MAX_CONTROLS: usize = 7;
pub(crate) const END: usize = POSE_BASE + MAX_CONTROLS * STRIDE;
const SCALE: f64 = 1.2;
// Preserve every preceding cell and all rows for the original scroll extent.
const WIDTHS: [f64; 26] = [
    82., 82., 104., 104., 144., 78., 92., 92., 92., 68., 88., 48., 112., 112., 112., 112., 112.,
    144., 112., 112., 144., 144., 144., 112., 112., 112.,
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    PageOne,
    PageTwo,
    PageThree,
    Keyboard,
    Meters,
    Faders,
    Arming,
}
#[derive(Default)]
pub(crate) struct Ui {
    page: usize,
    scroll: f64,
    previous_down: bool,
    captured: Option<Action>,
    pending: Option<Action>,
    pub timeline: [bool; 3],
    pressure: [f32; 7],
    hold: [bool; 7],
    panel: Option<Rect>,
    targets: Vec<(Action, Rect)>,
}
fn contains(rect: Rect, p: [f64; 2]) -> bool {
    p.iter().all(|n| n.is_finite())
        && p[0] >= rect.x
        && p[0] <= rect.x + rect.width
        && p[1] >= rect.y
        && p[1] <= rect.y + rect.height
}
fn pack(width: f64) -> Vec<(f64, f64, usize)> {
    let available = width.max(1.);
    let gap = 12. * SCALE;
    let columns = if available >= 2. * 82. * SCALE + gap {
        2
    } else {
        1
    };
    let column_width = (available - (columns - 1) as f64 * gap) / columns as f64;
    let (mut row, mut column) = (0, 0);
    let mut cells = Vec::with_capacity(WIDTHS.len());
    for width in WIDTHS {
        let width = (width * SCALE).clamp(1., available);
        let span = if width > column_width { columns } else { 1 };
        if column + span > columns {
            row += 1;
            column = 0;
        }
        cells.push((column as f64 * (column_width + gap), width, row));
        column += span;
        if column == columns {
            row += 1;
            column = 0;
        }
    }
    cells
}
fn layout(panel: Rect, page: usize, scroll: f64) -> (Vec<(Action, Rect)>, f64) {
    let nav_y = panel.y + panel.height - 34. * SCALE;
    let button = |cx, cy, w| Rect {
        x: cx - w * 0.5,
        y: cy - 18. * SCALE,
        width: w,
        height: 36. * SCALE,
    };
    let mut targets = vec![];
    for (index, action) in [Action::PageOne, Action::PageTwo, Action::PageThree].into_iter().enumerate() {
        targets.push((action, Rect { x: panel.x + 12. + index as f64 * 48., y: nav_y - 18. * SCALE, width: 36. * SCALE, height: 36. * SCALE }));
    }
    let cells = pack((panel.width - 20. * SCALE).max(1.));
    // Only the lane switches scroll below the fixed page-two controls.
    let max_scroll = (panel.y+390.+2.*54.*SCALE+18.*SCALE-(nav_y-34.*SCALE)).max(0.);
    if page == 1 {
        let (x, w, _) = cells[5];
        let y = panel.y + 330.;
        if y - 18. * SCALE >= panel.y + 26. * SCALE && y + 18. * SCALE <= nav_y - 34. * SCALE {
            targets.push((
                Action::Keyboard,
                button(panel.x + 10. * SCALE + x + w * 0.5, y, w),
            ));
        }
    }
    if page == 1 {
        for (i, action) in [Action::Meters, Action::Faders, Action::Arming].into_iter().enumerate() {
            let y = panel.y + 390. + i as f64 * 54. * SCALE - scroll.clamp(0.,max_scroll);
            if y - 18.*SCALE >= panel.y+360. && y+18.*SCALE <= nav_y-34.*SCALE {
                targets.push((action,button(panel.x+panel.width*0.5,y,(144.*SCALE).min(panel.width-24.))));
            }
        }
    }
    (targets, max_scroll)
}
impl Ui {
    pub fn owns_pointer(&self) -> bool {
        self.captured.is_some() || self.pending.is_some()
    }
    pub fn cancel(&mut self) {
        self.captured = None;
        self.pending = None;
        self.previous_down = false;
        self.hold = [false; 7];
    }
    fn pointer(&mut self, down: bool, p: Option<[f64; 2]>) {
        let hit = p.and_then(|p| {
            self.targets
                .iter()
                .find(|(_, r)| contains(*r, p))
                .map(|(a, _)| *a)
        });
        if down && !self.previous_down {
            self.captured = hit;
        }
        if !down && self.previous_down {
            if self.captured.is_some() && self.captured == hit {
                self.pending = hit;
                if let Some(action) = hit {
                    self.hold[action as usize] = true;
                }
            }
            self.captured = None;
        }
        self.previous_down = down;
    }
    /// Return a keyboard toggle; numbered buttons select the shared existing page.
    pub fn update(
        &mut self,
        input: &Value,
        panel: Rect,
        page: &mut usize,
        enabled: bool,
        blocked: bool,
        dt: f64,
    ) -> bool {
        *page = (*page).min(2);
        if self.page != *page {
            self.scroll = 0.;
            self.pending = None;
            self.captured = None;
        }
        self.page = *page;
        self.panel = Some(panel);
        let point = serde_json::from_value::<[f64; 2]>(input["pointer"].clone()).ok();
        let wheel = input["wheel"][1]
            .as_f64()
            .filter(|v| v.is_finite())
            .unwrap_or(0.);
        let (_, max_scroll) = layout(panel, *page, self.scroll);
        if *page == 1 && !blocked && point.is_some_and(|p| contains(panel, p)) {
            self.scroll = (self.scroll - wheel * 30. * SCALE).clamp(0., max_scroll);
        }
        self.targets = layout(panel, *page, self.scroll).0;
        let activate = self
            .pending
            .take()
            .filter(|action| !blocked && self.targets.iter().any(|(a, _)| a == action));
        let toggle = activate == Some(Action::Keyboard);
        if let Some(action @ (Action::Meters | Action::Faders | Action::Arming)) = activate {
            self.timeline[action as usize - 4] ^= true;
        }
        match activate {
            Some(Action::PageOne) => *page = 0,
            Some(Action::PageTwo) => *page = 1,
            Some(Action::PageThree) => *page = 2,
            _ => {}
        }
        if self.page != *page {
            self.page = *page;
            self.scroll = 0.;
            self.targets = layout(panel, *page, 0.).0;
        }
        let down = input["left"] == true;
        if blocked {
            self.captured = None;
            self.previous_down = down;
        } else {
            for e in input["transport_pointer_events"]
                .as_array()
                .into_iter()
                .flatten()
                .take(16)
            {
                if let Some(left) = e["left"].as_bool() {
                    self.pointer(left, serde_json::from_value(e["pointer"].clone()).ok());
                }
            }
            self.pointer(down, point);
        }
        let step = (0.5 / 0.15 * dt.clamp(0., 1.)) as f32;
        for i in 0..7 {
            let target = if (i == 3 && (enabled ^ toggle)) || (i >= 4 && self.timeline[i-4])
                || self.hold[i]
                || (down && self.captured.is_some_and(|a| a as usize == i))
            {
                0.5
            } else {
                0.
            };
            self.pressure[i] = if self.pressure[i] < target {
                (self.pressure[i] + step).min(target)
            } else {
                (self.pressure[i] - step).max(target)
            };
            self.hold[i] = false;
        }
        toggle
    }
    pub fn presentation(&self, enabled: bool) -> Value {
        let Some(panel) = self.panel else {
            return Value::Null;
        };
        json!({"panel":[panel.x,panel.y,panel.width,panel.height],"enabled":enabled,
            "pressure":self.pressure,"page":self.page,
            "controls":self.targets.iter().map(|(action,rect)|json!({"rect":[rect.x,rect.y,rect.width,rect.height],
                "text":match action{Action::PageOne=>"1",Action::PageTwo=>"2",Action::PageThree=>"3",Action::Keyboard=>"KEYBOARD",Action::Meters=>"LANE METERS",Action::Faders=>"LANE FADERS",Action::Arming=>"LANE ARMING"},
                "active":if *action as usize>=4{self.timeline[*action as usize-4]}else{*action != Action::Keyboard && *action as usize == self.page},
                "pressure":self.pressure[*action as usize]})).collect::<Vec<_>>()})
    }
}
/// Reuse immutable GemButton_Mini geometry and the existing SSLE glyph atlas.
pub(crate) fn append(
    rows: &mut [[f32; 4]],
    view: Viewport,
    value: &Value,
    mini: ([usize; 3], [[f64; 3]; 2]),
) -> Result<(), String> {
    if rows.len() < END {
        return Err("keyboard control requires complete frame storage".into());
    }
    let mut packet = [[0.; 4]; END - HEADER];
    if value.is_null() {
        rows[HEADER..END].copy_from_slice(&packet);
        return Ok(());
    }
    let panel: [f64; 4] =
        serde_json::from_value(value["panel"].clone()).map_err(|e| e.to_string())?;
    if panel.iter().any(|n| !n.is_finite() || n.abs() > 1e6) {
        return Err("invalid keyboard panel".into());
    }
    let controls = value["controls"]
        .as_array()
        .filter(|v| v.len() <= MAX_CONTROLS)
        .ok_or("invalid keyboard controls")?;
    let mpp = 1.3 / view.height;
    let world = |x: f64, y: f64| [(x - view.width * 0.5) * mpp, (view.height * 0.5 - y) * mpp];
    let low = world(panel[0], panel[1] + panel[3]);
    let high = world(panel[0] + panel[2], panel[1]);
    packet[3] = [low[0] as f32, low[1] as f32, high[0] as f32, high[1] as f32];
    let (tree, [min, max]) = mini;
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    let mut pressure_key = 0.;
    for (index, control) in controls.iter().enumerate() {
        let rect: [f64; 4] =
            serde_json::from_value(control["rect"].clone()).map_err(|e| e.to_string())?;
        if rect.iter().any(|n| !n.is_finite() || n.abs() > 1e6) || rect[2] <= 0. || rect[3] <= 0. {
            return Err("invalid keyboard control rect".into());
        }
        let press = control["pressure"]
            .as_f64()
            .filter(|p| p.is_finite())
            .unwrap_or(0.)
            .clamp(0., 0.5);
        pressure_key += press * (1 + index * 4) as f64;
        let scale = (rect[2] * mpp / (max[0] - min[0])).min(rect[3] * mpp / (max[1] - min[1]));
        let center = world(
            rect[0] + rect[2] * 0.5 - 10. * press,
            rect[1] + rect[3] * 0.5,
        );
        let label = control["text"].as_str().unwrap_or("");
        let font = 13f64.min((rect[2] - 4.) / (0.55 * label.len().max(1) as f64));
        let at = 4 + index * STRIDE;
        packet[at] = [
            center[0] as f32,
            center[1] as f32,
            (-min[2] * scale + 0.0005) as f32,
            scale as f32,
        ];
        packet[at + 1] = [
            tree[0] as f32,
            tree[1] as f32,
            (font * mpp / scale) as f32,
            if control["active"] == true {12704.} else {tree[2] as f32},
        ];
        for (i, c) in label.bytes().take(12).enumerate() {
            packet[at + 2 + i / 4][i % 4] = c as f32;
        }
        for axis in 0..3 {
            lo[axis] = lo[axis].min(packet[at][axis] as f64 + min[axis] * scale);
            hi[axis] = hi[axis].max(packet[at][axis] as f64 + max[axis] * scale);
        }
    }
    if !controls.is_empty() {
        for axis in 0..3 {
            packet[1][axis] = ((lo[axis] + hi[axis]) * 0.5) as f32;
            packet[2][axis] = ((hi[axis] - lo[axis]) * 0.5 + 1e-6) as f32;
        }
    }
    packet[0] = [
        POSE_BASE as f32,
        controls.len() as f32,
        if value["enabled"] == true { 1. } else { 0. },
        pressure_key as f32,
    ];
    rows[HEADER..END].copy_from_slice(&packet);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn panel() -> Rect {
        Rect {
            x: 1200.,
            y: 18.,
            width: 320.,
            height: 900.,
        }
    }
    fn click(ui: &mut Ui, page: &mut usize, action: Action) -> bool {
        ui.update(&json!({}), panel(), page, false, false, 0.);
        let p = ui
            .targets
            .iter()
            .find(|(a, _)| *a == action)
            .unwrap()
            .1
            .center();
        assert!(!ui.update(
            &json!({"left":true,"pointer":p}),
            panel(),
            page,
            false,
            false,
            0.075
        ));
        assert!(!ui.update(
            &json!({"left":false,"pointer":p}),
            panel(),
            page,
            false,
            false,
            0.075
        ));
        ui.update(
            &json!({"left":false,"pointer":p}),
            panel(),
            page,
            false,
            false,
            0.,
        )
    }
    #[test]
    fn keyboard_sits_below_page_two_controls_and_numbered_navigation() {
        let (cells, _) = layout(panel(), 1, 0.);
        let r = cells[3].1;
        assert_eq!(r.width, 78. * 1.2);
        assert_eq!(r.y + 21.6, 18. + 330.);
        assert_eq!(layout(panel(), 0, 0.).0.len(), 3);
        for (_, selector) in &cells[..3] {
            assert_eq!(selector.width, 36. * SCALE);
            assert_eq!(selector.height, r.height);
        }
        assert!(cells[0].1.x + cells[0].1.width < cells[1].1.x);
        assert_eq!(layout(panel(), 2, 0.).0.len(), 3);
    }
    #[test]
    fn numbered_pages_select_directly_and_keyboard_activates_after_release() {
        let mut ui = Ui::default();
        let mut page = 0;
        assert!(!click(&mut ui, &mut page, Action::PageOne));
        assert_eq!(page, 0);
        assert!(!click(&mut ui, &mut page, Action::PageTwo));
        assert_eq!(page, 1);
        assert!(click(&mut ui, &mut page, Action::Keyboard));
        assert!(!click(&mut ui, &mut page, Action::PageThree));
        assert_eq!(page, 2);
        assert!(!click(&mut ui, &mut page, Action::PageThree));
        assert_eq!(page, 2);
    }
    #[test]
    fn callback_clicks_preserve_short_press_and_focus_cancels() {
        let mut ui = Ui::default();
        let mut page = 1;
        ui.update(&json!({}), panel(), &mut page, false, false, 0.);
        let p = ui.targets[3].1.center();
        let input = json!({"transport_pointer_events":[{"left":true,"pointer":p},{"left":false,"pointer":p}]});
        assert!(!ui.update(&input, panel(), &mut page, false, false, 0.1));
        assert!(ui.owns_pointer());
        assert!(!ui.update(&json!({}), panel(), &mut page, false, true, 0.1));
        assert!(!ui.update(&json!({}), panel(), &mut page, false, false, 0.1));
    }
    #[test]
    fn enabled_state_is_part_of_retained_packet_identity() {
        let mut ui = Ui::default();
        let mut page = 1;
        ui.update(&json!({}), panel(), &mut page, true, false, 0.15);
        let mut rows = vec![[99.; 4]; END + 1];
        append(
            &mut rows,
            Viewport {
                width: 1600.,
                height: 1000.,
                canvas_scale: 1.,
            },
            &ui.presentation(true),
            ([10, 20, 30], [[-0.02, -0.02, -0.01], [0.02, 0.02, 0.01]]),
        )
        .unwrap();
        assert_eq!(rows[HEADER][1], ui.targets.len() as f32);
        assert_eq!(rows[HEADER][2], 1.);
        assert!(rows[HEADER][3] > 0.);
        assert_eq!(rows[HEADER - 1], [99.; 4]);
        assert_eq!(rows[END], [99.; 4]);
    }
}
