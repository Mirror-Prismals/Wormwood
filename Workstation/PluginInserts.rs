//! Cardinal's selected-track Edison chain, adapted to real Blender models.
//! Instrument first, effects in order. One layout drives drawing and picking.
use super::{
    panel_layout::{self, Options, Viewport},
    panel_motion::Snapshot,
};
use crate::host::Host;
use serde_json::{Value, json};
use std::sync::OnceLock;

/// Screen-space extent of the connected assembly, measured relative to the
/// socket face. The cable is real geometry now, not Cardinal's painted offset.
pub(crate) struct Hardware {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub footprint: [f64; 4],
}
pub(crate) fn hardware() -> Result<&'static Hardware, String> {
    static HARDWARE: OnceLock<Result<Hardware, String>> = OnceLock::new();
    HARDWARE
        .get_or_init(|| {
            let socket: Value = serde_json::from_str(include_str!(
                "../Art/AudioWorkstation/EdisonHardware/runtime.json"
            ))
            .map_err(|e| e.to_string())?;
            let plug: Value = serde_json::from_str(include_str!(
                "../Art/AudioWorkstation/EdisonHardware/connected.json"
            ))
            .map_err(|e| e.to_string())?;
            let bounds = |data: &Value| -> Result<([f64; 3], [f64; 3]), String> {
                let min: [f64; 3] =
                    serde_json::from_value(data["min"].clone()).map_err(|e| e.to_string())?;
                let max: [f64; 3] =
                    serde_json::from_value(data["max"].clone()).map_err(|e| e.to_string())?;
                if data["rotation_degrees_xyz"] != json!([0, 0, 0])
                    || min.iter().chain(&max).any(|n| !n.is_finite())
                    || (0..3).any(|i| min[i] >= max[i])
                {
                    return Err("Edison requires upright authored hardware bounds".into());
                }
                Ok((min, max))
            };
            let (min, max) = bounds(&socket)?;
            let (plug_min, plug_max) = bounds(&plug)?;
            let width = max[0] - min[0];
            Ok(Hardware {
                min,
                max,
                footprint: [
                    (plug_min[0].min(min[0]) - min[0]) / width,
                    (max[1] - plug_max[1].max(max[1])) / width,
                    (plug_max[0].max(max[0]) - min[0]) / width,
                    (max[1] - plug_min[1].min(min[1])) / width,
                ],
            })
        })
        .as_ref()
        .map_err(Clone::clone)
}
#[derive(Clone, Copy)]
pub(crate) struct Slot {
    pub rect: [f64; 4],
    pub index: usize,
}
pub(crate) fn contains(r: [f64; 4], p: [f64; 2]) -> bool {
    p[0] >= r[0] && p[0] < r[0] + r[2] && p[1] >= r[1] && p[1] < r[1] + r[3]
}
pub(crate) struct Layout {
    pub panel: [f64; 4],
    pub viewport: [f64; 4],
    pub navigation: [[f64; 4]; 4],
    pub size: f64,
    pub stride: f64,
    pub top: f64,
    pub scale: f64,
    pub depth: f64,
    pub footprint: [f64; 4],
}
impl Layout {
    pub(crate) fn navigation_visible(&self, r: [f64; 4]) -> bool {
        r[2] > 0.
            && r[3] > 0.
            && r[0] >= self.panel[0]
            && r[1] >= self.panel[1]
            && r[0] + r[2] <= self.panel[0] + self.panel[2]
            && r[1] + r[3] <= self.panel[1] + self.panel[3]
    }
    pub(crate) fn minimum_scroll(&self, count: usize) -> f64 {
        if count == 0 || self.size <= 0. {
            return 0.;
        }
        (self.viewport[2]
            - ((count - 1) as f64 * self.stride
                + self.size * (self.footprint[2] - self.footprint[0])))
            .min(0.)
    }
    pub(crate) fn slot(&self, index: usize, scroll: f64) -> Slot {
        Slot {
            rect: [
                self.viewport[0] - self.footprint[0] * self.size
                    + index as f64 * self.stride
                    + scroll,
                self.top,
                self.size,
                self.size,
            ],
            index,
        }
    }
    pub(crate) fn slots(&self, count: usize, scroll: f64) -> Vec<Slot> {
        if self.size <= 0. {
            return Vec::new();
        }
        let scroll = scroll.clamp(self.minimum_scroll(count), 0.);
        (0..count)
            .map(|i| self.slot(i, scroll))
            .filter(|s| {
                // Reserve the connected silhouette even with its editor shut,
                // so opening a plug never moves the chain or its scroll limit.
                s.rect[0] + self.size * self.footprint[2] > self.viewport[0]
                    && s.rect[0] + self.size * self.footprint[0]
                        < self.viewport[0] + self.viewport[2]
            })
            .take(16)
            .collect()
    }
    pub(crate) fn chain_slot(&self, inserts: &[Value], index: usize, scroll: f64) -> Slot {
        let mut x = self.viewport[0] - self.footprint[0] * self.size + scroll;
        for p in inserts.iter().take(index) {
            x += if p["stock"] == true {
                230. * self.scale + 20. * self.scale
            } else {
                self.stride
            };
        }
        let stock = inserts.get(index).is_some_and(|p| p["stock"] == true);
        let width = if stock { 230. * self.scale } else { self.size };
        let height = if stock { 132. * self.scale } else { self.size };
        Slot {
            rect: [x, self.top + self.size * 0.5 - height * 0.5, width, height],
            index,
        }
    }
    pub(crate) fn chain_minimum_scroll(&self, inserts: &[Value]) -> f64 {
        let Some(last) = inserts.len().checked_sub(1) else {
            return 0.;
        };
        let r = self.chain_slot(inserts, last, 0.).rect;
        let overhang = if inserts[last]["stock"] == true {
            0.
        } else {
            self.size * (self.footprint[2] - 1.)
        };
        (self.viewport[0] + self.viewport[2] - r[0] - r[2] - overhang).min(0.)
    }
    pub(crate) fn chain_slots(&self, inserts: &[Value], scroll: f64) -> Vec<Slot> {
        if self.size <= 0. {
            return Vec::new();
        }
        let scroll = scroll.clamp(self.chain_minimum_scroll(inserts), 0.);
        (0..inserts.len())
            .map(|i| self.chain_slot(inserts, i, scroll))
            .filter(|s| {
                let stock = inserts[s.index]["stock"] == true;
                s.rect[0]
                    + if stock {
                        s.rect[2]
                    } else {
                        self.size * self.footprint[2]
                    }
                    > self.viewport[0]
                    && s.rect[0]
                        + if stock {
                            0.
                        } else {
                            self.size * self.footprint[0]
                        }
                        < self.viewport[0] + self.viewport[2]
            })
            .take(16)
            .collect()
    }
}
fn custom_content_rect(
    view: Viewport,
    motion: &Snapshot,
    options: Options,
) -> Result<Option<panel_layout::Rect>, String> {
    let Some(style) = super::user_style::selected()? else {
        return Ok(None);
    };
    let Some(surface) = style.surface(1, view, motion, options)? else {
        return Ok(None);
    };
    let mpp = 1.3 / view.height;
    Ok(Some(panel_layout::Rect {
        x: view.width * 0.5 + (surface.origin[0] - surface.size[0] * 0.5) / mpp,
        y: view.height * 0.5 - (surface.origin[1] + surface.size[1] * 0.5) / mpp,
        width: surface.size[0] / mpp,
        height: surface.size[1] / mpp,
    }))
}
pub(crate) fn layout(
    view: Viewport,
    motion: &Snapshot,
    options: Options,
) -> Result<Layout, String> {
    let hardware = hardware()?;
    let custom = custom_content_rect(view, motion, options)?;
    let panel = custom.unwrap_or(panel_layout::layout(view, motion, options)?[1]);
    let mut c = view.canvas_scale;
    if custom.is_some() {
        c = c.min(panel.width / 400.).min(panel.height / 160.);
    }
    let height = panel.height - 60. * c;
    let socket_scale = (view.width / 1200.).min(view.height / 780.) * c;
    let size = if height > 76. * c {
        (124.7 * socket_scale).min(height)
    } else {
        0.
    };
    // Cardinal's navigation is beside the chain. The extra left/depth skew
    // belonged to its 2D painter; an upright 3D housing has no such overhang.
    let left = panel.x + (40. + 66. + 20.) * c;
    let right = panel.x + panel.width - 40. * c;
    let top = panel.y + 36. * c + height * 0.5 - size * 0.5;
    let navigation = std::array::from_fn(|i| {
        [
            panel.x + (40. + (i % 2) as f64 * 38.) * c,
            panel.y + (36. + (i / 2) as f64 * 36.) * c,
            28. * c,
            28. * c,
        ]
    });
    let surface = panel_layout::resolved_surface(view, motion, options, 1)?;
    Ok(Layout {
        panel: [panel.x, panel.y, panel.width, panel.height],
        viewport: [left, panel.y, (right - left).max(0.), panel.height],
        navigation,
        size,
        stride: size * (hardware.footprint[2] - hardware.footprint[0]) + 20. * c,
        top,
        scale: c,
        depth: if custom.is_some() {
            surface.origin[2]
        } else {
            // Mount hardware on the gem's actual top face, rather than hiding
            // the small navigation buttons inside its transparent volume.
            0.30 * panel_layout::panel_shape(view, motion, options, 1)?.local_unit_m
        },
        footprint: hardware.footprint,
    })
}
#[derive(Clone, Copy, PartialEq)]
enum Target {
    Insert(u64),
    Navigation(usize),
    StockBypass(u64),
    StockRemove(u64),
}
struct Press {
    target: Target,
    start: [f64; 2],
    dragging: bool,
    was_selected: bool,
}
#[derive(Default)]
pub(crate) struct Ui {
    pub selected: Option<String>,
    pub scroll: f64,
    status: Value,
    previous_left: bool,
    previous_right: bool,
    tick: f64,
    error: Option<String>,
    preferred: Option<String>,
    caption: String,
    mixer: bool,
    shared_navigation: bool,
    selected_insert: Option<u64>,
    hover: Option<u64>,
    press: Option<Press>,
    previous_delete: bool,
    // Cardinal DawFaderSystem::PluginOwnsDelete retains ownership until key-up.
    delete_consumed: bool,
    consumes_input: bool,
    drop_index: Option<usize>,
    drop_x: Option<f64>,
    stock_drag: Option<(u64, usize)>,
    last_stock_command: Option<Value>,
    preamp_scroll: f64,
    previous_z: bool,
    poses: std::collections::BTreeMap<u64, ([f64; 4], f64)>,
    rendered: Vec<Value>,
    drag_position: Option<[f64; 2]>,
    drag_origin: Option<[f64; 4]>,
    destination: Option<(&'static str, usize)>,
}
impl Ui {
    pub(crate) fn set_bottom_page(&mut self, page: usize) {
        self.shared_navigation = true;
        self.set_mixer(page != 1);
    }

    pub(crate) fn set_mixer(&mut self, mixer: bool) -> bool {
        if self.mixer != mixer {
            self.clear_selection();
        }
        std::mem::replace(&mut self.mixer, mixer)
    }
    pub(crate) fn caption(&self) -> &str {
        &self.caption
    }
    pub(crate) fn owns_pointer(&self) -> bool {
        self.press.is_some() || self.stock_drag.is_some() || self.delete_consumed
    }
    pub(crate) fn consumes_input(&self) -> bool {
        self.consumes_input
    }
    pub(crate) fn presentation(&self) -> Value {
        let inserts = self.status["inserts"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|p| p["track"].as_str() == self.selected.as_deref())
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let pressed = self
            .press
            .as_ref()
            .filter(|p| !p.dragging)
            .and_then(|p| match p.target {
                Target::Insert(id) => Some(id),
                _ => None,
            });
        let nav_pressed = self.press.as_ref().and_then(|p| match p.target {
            Target::Navigation(i) => Some(i),
            _ => None,
        });
        json!({"enabled":cfg!(target_os="macos")&&!self.mixer,"navigation":cfg!(target_os="macos")&&!self.shared_navigation,
            "track":self.selected,"inserts":inserts,"scroll":self.scroll,"slots":self.rendered,
            "selected":self.selected_insert,"hover":self.hover,"pressed":pressed,
            "nav_pressed":nav_pressed,"drop_x":self.drop_x,
            "stock_bypass_pressed":self.press.as_ref().and_then(|p|if let Target::StockBypass(id)=p.target{Some(id)}else{None}),
            "stock_remove_pressed":self.press.as_ref().and_then(|p|if let Target::StockRemove(id)=p.target{Some(id)}else{None})})
    }
    pub(crate) fn update(
        &mut self,
        host: &mut Host,
        document_snapshot: Option<&Value>,
        input: &Value,
        dt: f64,
        view: Viewport,
        motion: &Snapshot,
        options: Options,
        preferred: Option<String>,
        blocked: bool,
    ) -> Result<bool, String> {
        if !cfg!(target_os = "macos") {
            return Ok(false);
        }
        let before = self.presentation();
        let owned;
        let snapshot = if let Some(v) = document_snapshot {
            v
        } else {
            owned = host.invoke("WorkstationDataSystem.Snapshot", Value::Null)?;
            &owned
        };
        let rows:Vec<Value>=snapshot["tracks"].as_array().ok_or("missing workstation tracks")?.iter().chain(snapshot["buses"].as_array().into_iter().flatten().filter(|b|(b["kind"]=="mix"||b["kind"]=="channel"))).cloned().collect();
        let tracks=&rows;
        let preferred =
            preferred.filter(|id| tracks.iter().any(|t| t["id"] == *id && t["kind"] != "cc"));
        let previous = self.selected.clone();
        if preferred != self.preferred {
            self.preferred = preferred.clone();
            if preferred.is_some() {
                self.selected = preferred;
            }
        }
        if !tracks
            .iter()
            .any(|t| t["id"].as_str() == self.selected.as_deref())
        {
            self.selected = tracks
                .iter()
                .find(|t| t["kind"] != "cc")
                .and_then(|t| t["id"].as_str())
                .map(str::to_owned);
        }
        if previous != self.selected {
            self.clear_selection();
            self.scroll = 0.;
            self.preamp_scroll = 0.;
            self.poses.clear();
        }
        let audio = tracks
            .iter()
            .any(|t| t["id"].as_str() == self.selected.as_deref() && t["kind"] == "audio");
        self.tick += dt;
        if self.tick >= 0.1 || self.status.is_null() {
            self.tick = 0.;
            match host.invoke("PluginConnectionSystem.Command", json!({"op":"tick"})) {
                Ok(s) => self.status = s,
                Err(e) => self.error = Some(e),
            }
        }
        let p = self.presentation();
        let inserts = p["inserts"].as_array().unwrap();
        if self
            .selected_insert
            .is_some_and(|id| !inserts.iter().any(|p| p["id"] == id))
        {
            self.clear_selection();
        }
        let main = layout(view, motion, options)?;
        let pre = preamp_layout(view, motion, options)?;
        let pointer = serde_json::from_value::<[f64; 2]>(input["pointer"].clone())
            .ok()
            .filter(|p| p.iter().all(|v| v.is_finite()));
        let left = input["left"] == true;
        let right = input["right"] == true;
        let delete = input["delete"] == true;
        let z = input["preamp_switch"] == true
            && input["command"] != true
            && input["alt"] != true
            && input["shift"] != true;
        let cancelled = blocked
            || input["escape"] == true
            || input["focused"] == false
            || input["pointer_cancelled"] == true
            || input["transport_pointer_cancelled"] == true
            || pointer.is_none()
            || self.mixer;
        if cancelled {
            self.drag_origin = None;
            self.press = None;
            self.stock_drag = None;
            self.destination = None;
            self.drag_position = None;
        }
        let chain_at = pointer.and_then(|point| {
            if contains(pre.viewport, point) && pre.panel[2] > 1. && audio {
                Some("preamp")
            } else if contains(main.viewport, point) {
                Some("playback")
            } else {
                None
            }
        });
        let enabled = !cancelled && !self.mixer;
        let mut action = None;
        if enabled && let Some(chain) = chain_at {
            let wheel =
                serde_json::from_value::<[f64; 2]>(input["wheel"].clone()).unwrap_or([0.; 2]);
            if wheel.iter().all(|v| v.is_finite()) {
                let scroll = if chain == "preamp" {
                    &mut self.preamp_scroll
                } else {
                    &mut self.scroll
                };
                *scroll += (wheel[0] + wheel[1]) * 40. * view.canvas_scale;
            }
        }
        // A drag begins only from the selected Edison. Keep its grab offset,
        // and preserve stable instance IDs while every neighbor moves around it.
        if left && let Some(press) = &mut self.press {
            if press.was_selected
                && matches!(press.target, Target::Insert(_))
                && pointer.is_some_and(|p| {
                    (p[0] - press.start[0]).hypot(p[1] - press.start[1]) > 6. * view.canvas_scale
                })
            {
                press.dragging = true;
            }
        }
        let moving = self.press.as_ref().filter(|p| p.dragging).and_then(|p| {
            if let Target::Insert(id) = p.target {
                inserts
                    .iter()
                    .find(|p| p["id"] == id && p["instrument"] != true)
                    .and_then(|p| p["id"].as_u64())
            } else {
                None
            }
        });
        self.destination = None;
        if enabled
            && moving.is_some()
            && let (Some(chain), Some(point)) = (chain_at, pointer)
        {
            let l = if chain == "preamp" { &pre } else { &main };
            let scroll = if chain == "preamp" {
                &mut self.preamp_scroll
            } else {
                &mut self.scroll
            };
            if left {
                let edge = 24. * view.canvas_scale;
                if point[0] < l.viewport[0] + edge {
                    *scroll += 260. * dt;
                } else if point[0] > l.viewport[0] + l.viewport[2] - edge {
                    *scroll -= 260. * dt;
                }
            }
            let others: Vec<_> = inserts
                .iter()
                .filter(|p| chain_name(p) == chain && p["id"].as_u64() != moving)
                .cloned()
                .collect();
            let index = others
                .iter()
                .enumerate()
                .filter(|(_, p)| p["instrument"] != true)
                .take_while(|(i, _)| {
                    let r = l.chain_slot(&others, *i, *scroll).rect;
                    point[0] >= r[0] + r[2] * 0.5
                })
                .count();
            self.destination = Some((chain, index));
        }
        let blend = 1. - (-dt.clamp(0., 0.05) * 22.).exp();
        let mut rendered = Vec::new();
        for (chain, l, scroll) in [
            ("playback", &main, &mut self.scroll),
            ("preamp", &pre, &mut self.preamp_scroll),
        ] {
            if chain == "preamp" && (!audio || l.panel[2] < 1.) {
                continue;
            }
            let mut order: Vec<_> = inserts
                .iter()
                .filter(|p| chain_name(p) == chain && p["id"].as_u64() != moving)
                .cloned()
                .collect();
            if let (Some(id), Some((destination, index))) = (moving, self.destination) {
                if destination == chain {
                    let offset =
                        usize::from(order.first().is_some_and(|p| p["instrument"] == true));
                    let at = (index + offset).min(order.len());
                    order.insert(at, inserts.iter().find(|p| p["id"] == id).unwrap().clone());
                }
            }
            *scroll = scroll.clamp(l.chain_minimum_scroll(&order), 0.);
            for (i, insert) in order.iter().enumerate() {
                let id = insert["id"].as_u64().unwrap_or(0);
                if Some(id) == moving {
                    continue;
                }
                let target = l.chain_slot(&order, i, *scroll).rect;
                let pose = self.poses.entry(id).or_insert((target, 0.));
                for j in 0..4 {
                    pose.0[j] += (target[j] - pose.0[j]) * blend;
                }
                pose.1 += (0. - pose.1) * blend;
                if pose.0[0] + pose.0[2] > l.viewport[0]
                    && pose.0[0] < l.viewport[0] + l.viewport[2]
                {
                    rendered.push(json!({"id":id,"rect":pose.0,"lift":pose.1,"clip":if pose.0.iter().zip(target).any(|(a,b)|(a-b).abs()>1.){[0.,0.,view.width,view.height]}else{l.viewport}}));
                }
            }
        }
        if moving.is_some() {
            rendered.truncate(15);
        }
        if let Some(id) = moving {
            let pose = self
                .poses
                .entry(id)
                .or_insert(([0., 0., main.size, main.size], 0.));
            let origin = *self.drag_origin.get_or_insert(pose.0);
            if let (Some(point), Some(press)) = (pointer, self.press.as_ref()) {
                let follow = self.drag_position.get_or_insert(press.start);
                for j in 0..2 {
                    follow[j] += (point[j] - follow[j]) * blend;
                    pose.0[j] = origin[j] + follow[j] - press.start[j];
                } // corrected below: anchored grab
            }
            pose.1 += (18. * view.canvas_scale - pose.1) * blend;
            rendered.push(
                json!({"id":id,"rect":pose.0,"lift":pose.1,"clip":[0.,0.,view.width,view.height]}),
            );
        } else {
            self.drag_position = None;
            self.drag_origin = None;
        }
        rendered.truncate(16);
        self.rendered = rendered;
        let hit = if enabled {
            pointer.and_then(|point| {
                self.rendered
                    .iter()
                    .rev()
                    .find(|s| {
                        serde_json::from_value::<[f64; 4]>(s["rect"].clone())
                            .is_ok_and(|r| contains(r, point))
                            && serde_json::from_value::<[f64; 4]>(s["clip"].clone())
                                .is_ok_and(|r| contains(r, point))
                    })
                    .and_then(|s| s["id"].as_u64())
            })
        } else {
            None
        };
        self.hover = hit;
        let mut target = hit.map(Target::Insert);
        if enabled && let (Some(id), Some(point)) = (hit, pointer) {
            if let Some(insert) = inserts.iter().find(|p| p["id"] == id && p["stock"] == true) {
                let r = serde_json::from_value(
                    self.rendered.iter().find(|s| s["id"] == id).unwrap()["rect"].clone(),
                )
                .unwrap();
                let controls = super::stock_eq::Controls::new(r, view.canvas_scale);
                if contains(controls.bypass, point) {
                    target = Some(Target::StockBypass(id));
                } else if contains(controls.remove, point) {
                    target = Some(Target::StockRemove(id));
                } else if left && !self.previous_left && point[1] > r[1] + 26. * view.canvas_scale {
                    if let Some((parameter, value)) = controls.press(point, &insert["state"]) {
                        match parameter {
                            super::stock_eq::Press::Slope(edge) => {
                                action = Some(super::stock_eq::slope_command(id, edge, value))
                            }
                            super::stock_eq::Press::Drag(pole) => {
                                self.stock_drag = Some((id, pole))
                            }
                        }
                    }
                    target = None;
                }
            }
        }
        if enabled && chain_at.is_some() && left && !self.previous_left {
            let was_selected = hit.is_some() && hit == self.selected_insert;
            self.selected_insert = hit;
            self.press = target.map(|target| Press {
                target,
                start: pointer.unwrap(),
                dragging: false,
                was_selected,
            });
        }
        if enabled
            && left
            && let (Some((id, pole)), Some(point)) = (self.stock_drag, pointer)
        {
            if let Some(slot) = self.rendered.iter().find(|s| s["id"] == id) {
                let r = serde_json::from_value(slot["rect"].clone()).unwrap();
                let insert = inserts.iter().find(|p| p["id"] == id).unwrap();
                action = Some(
                    super::stock_eq::Controls::new(r, view.canvas_scale).drag_command(
                        id,
                        pole,
                        point,
                        &insert["state"],
                        self.status["sample_rate"].as_f64().unwrap_or(48000.),
                    ),
                );
            }
        }
        if !left {
            self.stock_drag = None;
        }
        let captured = self.press.is_some() || self.stock_drag.is_some();
        if !left
            && self.previous_left
            && let Some(press) = self.press.take()
        {
            if enabled {
                if press.dragging {
                    if let (Target::Insert(id), Some((chain, index))) =
                        (press.target, self.destination)
                    {
                        action = Some(json!({"op":"transfer","id":id,"chain":chain,"index":index}));
                    }
                } else if Some(press.target) == target {
                    match press.target {
                        Target::Insert(id) => {
                            if press.was_selected
                                && inserts.iter().any(|p| p["id"] == id && p["stock"] != true)
                            {
                                action = Some(json!({"op":"toggle_editor","id":id}));
                            }
                        }
                        Target::StockBypass(id) => {
                            if let Some(p) = inserts.iter().find(|p| p["id"] == id) {
                                action = Some(
                                    json!({"op":"bypass","id":id,"value":p["bypassed"]!=true}),
                                );
                            }
                        }
                        Target::StockRemove(id) => action = Some(json!({"op":"remove","id":id})),
                        _ => {}
                    }
                }
            }
            self.destination = None;
        }
        if enabled
            && !left
            && right
            && !self.previous_right
            && hit == self.selected_insert
            && let Some(p) = inserts.iter().find(|p| p["id"].as_u64() == hit)
        {
            action = Some(json!({"op":"bypass","id":p["id"],"value":p["bypassed"]!=true}));
        }
        let mut shortcut = false;
        if enabled
            && !left
            && (input["preamp_switch_pressed"] == true || (z && !self.previous_z))
            && audio
            && let Some(p) = inserts
                .iter()
                .find(|p| p["id"].as_u64() == self.selected_insert && p["instrument"] != true)
        {
            let chain = if chain_name(p) == "preamp" {
                "playback"
            } else {
                "preamp"
            };
            action = Some(json!({"op":"transfer","id":p["id"],"chain":chain,"index":128}));
            shortcut = true;
        }
        if enabled
            && delete
            && !self.previous_delete
            && let Some(id) = self.selected_insert
        {
            action = Some(json!({"op":"remove","id":id}));
            self.delete_consumed = true;
            self.clear_selection();
        }
        if !delete {
            self.delete_consumed = false;
        }
        if let Some(command) = action {
            if self.last_stock_command.as_ref() != Some(&command) {
                match host.invoke("PluginConnectionSystem.Command", command.clone()) {
                    Ok(status) => {
                        self.status = status;
                        self.error = None;
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            self.last_stock_command = Some(command);
        } else {
            self.last_stock_command = None;
        }
        self.consumes_input = enabled
            && (captured
                || chain_at.is_some()
                || shortcut
                || (delete && (self.selected_insert.is_some() || self.delete_consumed)));
        self.previous_left = left;
        self.previous_right = right;
        self.previous_delete = delete;
        self.previous_z = z;
        self.caption=self.error.clone().unwrap_or_else(||"Edison: click to select; click selected to open; Z switches preamp; drag selected to reorder or move between chains".into());
        Ok(before != self.presentation())
    }
    fn clear_selection(&mut self) {
        self.selected_insert = None;
        self.press = None;
        self.hover = None;
        self.drop_index = None;
        self.drop_x = None;
        self.stock_drag = None;
        self.last_stock_command = None;
    }
    pub(crate) fn catalog(&self) -> Vec<Value> {
        self.status["catalog"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
    pub(crate) fn refresh(&mut self, status: Value) {
        self.status = status;
        self.tick = 0.;
    }
    pub(crate) fn surface(
        &self,
        view: Viewport,
        motion: &Snapshot,
        options: Options,
    ) -> Result<Value, String> {
        if self.mixer || !cfg!(target_os = "macos") {
            return Ok(Value::Null);
        }
        let p = self.presentation();
        let inserts = p["inserts"].as_array().unwrap();
        let mut groups: Vec<Value> = Vec::new();
        for slot in &self.rendered {
            let Some(insert) = inserts.iter().find(|p| p["id"] == slot["id"]) else {
                continue;
            };
            let mut r: [f64; 4] = serde_json::from_value(slot["rect"].clone()).unwrap();
            r[1] -= slot["lift"].as_f64().unwrap_or(0.);
            let clip = slot["clip"].clone();
            let index = groups
                .iter()
                .position(|g| g["clip"] == clip)
                .unwrap_or_else(|| {
                    groups.push(json!({"clip":clip,"labels":[],"graphs":[]}));
                    groups.len() - 1
                });
            if insert["stock"] == true {
                let mut graph = super::stock_eq::presentation(insert, r, view.canvas_scale);
                graph["selected"] = json!(insert["id"].as_u64() == self.selected_insert);
                groups[index]["graphs"].as_array_mut().unwrap().push(graph);
            } else {
                groups[index]["labels"].as_array_mut().unwrap().push(json!({"text":super::plugin_surface::label_text(insert["name"].as_str().unwrap_or(""),1024),"rect":[r[0],r[1]-30.*view.canvas_scale,r[2],24.*view.canvas_scale],"size":10.*view.canvas_scale,"center":true,"color":[38.25,38.25,38.25]}));
            }
        }
        let pre = preamp_layout(view, motion, options)?;
        if pre.panel[2] > 40. {
            groups.push(json!({"clip":pre.panel,"labels":[{"text":"PREAMP / RECORD FX","rect":[pre.panel[0]+12.,pre.panel[1]+6.,pre.panel[2]-24.,22.],"size":10.*view.canvas_scale,"center":true,"color":[38.25,38.25,38.25]}]}));
        }
        Ok(json!(groups))
    }
}

#[cfg(test)]
mod tests {
    use super::Ui;

    #[cfg(target_os = "macos")]
    #[test]
    fn edison_first_click_selects_and_second_click_requests_editor() {
        use super::*;
        let mut host = Host::load(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let mut ui = Ui::default();
        // An absent native instance makes an editor request fail explicitly;
        // the first selection must never issue that request.
        ui.refresh(json!({"inserts":[{"id":987654,"track":"a","name":"Fixture"}]}));
        let document = json!({"tracks":[{"id":"a","kind":"audio"}]});
        let view = Viewport {
            width: 1280.,
            height: 720.,
            canvas_scale: 1.,
        };
        let mut motion = super::super::panel_motion::State::default();
        motion.set_all_open(true);
        motion.advance(1.);
        let motion = motion.snapshot();
        let options = Options::default();
        let r = layout(view, &motion, options).unwrap().slot(0, 0.).rect;
        let point = [r[0] + r[2] * 0.5, r[1] + r[3] * 0.5];
        for left in [true, false] {
            ui.update(
                &mut host,
                Some(&document),
                &json!({"pointer":point,"left":left}),
                0.,
                view,
                &motion,
                options,
                Some("a".into()),
                false,
            )
            .unwrap();
        }
        assert_eq!(ui.selected_insert, Some(987654));
        assert!(ui.error.is_none(), "first click must only select");
        for left in [true, false] {
            ui.update(
                &mut host,
                Some(&document),
                &json!({"pointer":point,"left":left}),
                0.,
                view,
                &motion,
                options,
                Some("a".into()),
                false,
            )
            .unwrap();
        }
        assert!(
            ui.error.is_some(),
            "second click requests the absent editor"
        );
    }

    #[test]
    fn workstation_control_diagnostic_selects_existing_mixer_page() {
        let mut ui = Ui::default();
        let before = ui.presentation();
        let previous = ui.set_mixer(true);
        assert!(!previous);
        assert_eq!(ui.presentation()["enabled"], false);
        assert!(ui.set_mixer(previous));
        assert_eq!(ui.presentation(), before);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn preamp_transfer_preserves_instance_order_and_prints_only_capture() {
    use super::*;
    let mut h = Host::load(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
    h.invoke(
        "WorkstationDataSystem.PutTrack",
        json!({"document_revision":0,"id":"a","name":"Audio","kind":"audio"}),
    )
    .unwrap();
    let command = |h: &mut Host, q: Value| {
        crate::base_system::layer_one::plugin_connection_system::dispatch(h, "Command", &q).unwrap()
    };
    let mut ids = Vec::new();
    for _ in 0..3 {
        let s = command(
            &mut h,
            json!({"op":"add","track":"a","stock":"LShaping","sample_rate":8000}),
        );
        ids.push(
            s["inserts"].as_array().unwrap().last().unwrap()["id"]
                .as_u64()
                .unwrap(),
        );
    }
    command(
        &mut h,
        json!({"op":"stock_parameter","id":ids[1],"parameter":"gain_db","value":6.}),
    );
    let transferred = command(
        &mut h,
        json!({"op":"transfer","id":ids[1],"chain":"preamp","index":0}),
    );
    let row = transferred["inserts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == ids[1])
        .unwrap();
    assert_eq!(row["state"]["gain_db"], 6.);
    assert_eq!(row["chain"], "preamp");
    command(
        &mut h,
        json!({"op":"transfer","id":ids[2],"chain":"preamp","index":0}),
    );
    let s = command(&mut h, json!({"op":"status"}));
    assert_eq!(
        s["inserts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["chain"] == "preamp")
            .map(|p| p["id"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![ids[2], ids[1]]
    );
    command(
        &mut h,
        json!({"op":"transfer","id":ids[2],"chain":"playback","index":0}),
    );
    h.workstation.with(|d|{
        d.tracks.get_mut("a").unwrap().io.arm_mode=1;
        let samples:Vec<f32>=(0..512).map(|n|((n as f32)*0.21).sin()*0.1).collect();
        let q=json!({"sample_rate":8000,"capture_start_frame":0,"timeline_start_frame":0,"frames":512,"playing":true,"recording":true,"live_inputs":{"physical":[samples]}});
        crate::workstation::recording::capture(d,&q)?;
        crate::workstation::recording::capture(d,&json!({"sample_rate":8000,"capture_start_frame":512,"timeline_start_frame":512,"frames":0,"playing":false,"recording":false}))?;
        let recorded=d.sources.values().next().unwrap().samples.to_vec();
        assert!(recorded.iter().zip(&samples).any(|(a,b)|(a-b).abs()>0.01),"capture must print the EQ");
        // Source creation is the same path used by imported clips. Its PCM is
        // stored verbatim and must never pass through the capture chain.
        let rev=d.revision;
        crate::workstation::document::dispatch(d,"PutSource",&json!({"document_revision":rev,"id":"imported","sample_rate":8000,"channels":1,"samples":samples}))?;
        assert_eq!(&*d.sources["imported"].samples,samples.as_slice());
        Ok(())
    }).unwrap();
    let rev = h.workstation.with(|d| Ok(d.revision)).unwrap();
    h.invoke(
        "WorkstationDataSystem.PutTrack",
        json!({"document_revision":rev,"id":"m","name":"MIDI","kind":"midi"}),
    )
    .unwrap();
    let s = command(&mut h, json!({"op":"add","track":"m","stock":"LShaping"}));
    let id = s["inserts"].as_array().unwrap().last().unwrap()["id"].clone();
    assert!(
        crate::base_system::layer_one::plugin_connection_system::dispatch(
            &mut h,
            "Command",
            &json!({"op":"transfer","id":id,"chain":"preamp","index":0})
        )
        .is_err()
    );
}

fn chain_name(p: &Value) -> &str {
    p["chain"].as_str().unwrap_or("playback")
}
pub(crate) fn preamp_layout(
    view: Viewport,
    motion: &Snapshot,
    options: Options,
) -> Result<Layout, String> {
    let mut l = layout(view, motion, options)?;
    let p = panel_layout::preamp_rect(view, motion, options)?;
    l.panel = [p.x, p.y, p.width, p.height];
    l.viewport = [
        p.x + 18. * view.canvas_scale,
        p.y + 30. * view.canvas_scale,
        (p.width - 36. * view.canvas_scale).max(0.),
        (p.height - 42. * view.canvas_scale).max(0.),
    ];
    l.navigation = [[0.; 4]; 4];
    l.top = p.y + p.height * 0.5 - l.size * 0.5 + 6. * view.canvas_scale;
    Ok(l)
}

#[cfg(test)]
mod preamp_motion_tests {
    use super::*;
    #[test]
    fn preamp_split_tracks_browser_width_and_reverses_continuously() {
        let view = Viewport {
            width: 1280.,
            height: 720.,
            canvas_scale: 1.,
        };
        let mut motion = super::super::panel_motion::State::default();
        motion.set_all_open(true);
        motion.advance(1.);
        motion.set_preamp(true);
        motion.advance(0.016);
        let first = motion.snapshot();
        assert!(first.preamp_split > 0. && first.preamp_split < 1.);
        motion.set_preamp(false);
        motion.advance(0.016);
        assert!(motion.snapshot().preamp_split < first.preamp_split);
        motion.set_preamp(true);
        for _ in 0..90 {
            motion.advance(0.016);
        }
        let snapshot = motion.snapshot();
        let rects = panel_layout::layout(view, &snapshot, Options::default()).unwrap();
        let pre = panel_layout::preamp_rect(view, &snapshot, Options::default()).unwrap();
        assert!((pre.width - rects[2].width).abs() < 0.01);
        assert!((pre.x - rects[2].x).abs() < 0.01);
        assert!((rects[1].x - pre.x - pre.width - 8.).abs() < 0.01);
        assert!(rects[2].y + rects[2].height <= pre.y + 0.01);
        assert!(rects[1].width > 160.);
    }
    #[test]
    fn preamp_selected_drag_lifts_follows_and_opens_a_neighbor_gap_then_cancels() {
        let mut host = Host::load(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();
        let view = Viewport {
            width: 1600.,
            height: 900.,
            canvas_scale: 1.,
        };
        let mut motion = super::super::panel_motion::State::default();
        motion.set_all_open(true);
        motion.advance(1.);
        let motion = motion.snapshot();
        let options = Options::default();
        let document = json!({"tracks":[{"id":"a","kind":"audio"}]});
        let status =
            json!({"inserts":[{"id":1,"track":"a"},{"id":2,"track":"a"},{"id":3,"track":"a"}]});
        let mut ui = Ui::default();
        ui.refresh(status.clone());
        let l = layout(view, &motion, options).unwrap();
        let r = l
            .chain_slot(status["inserts"].as_array().unwrap(), 0, 0.)
            .rect;
        let point = [r[0] + r[2] * 0.5, r[1] + r[3] * 0.5];
        for left in [true, false, true] {
            ui.update(
                &mut host,
                Some(&document),
                &json!({"pointer":point,"left":left}),
                0.,
                view,
                &motion,
                options,
                Some("a".into()),
                false,
            )
            .unwrap();
        }
        let neighbor = ui.poses[&2].0;
        let destination = [point[0] + l.stride * 1.8, point[1] - 5.];
        for _ in 0..6 {
            ui.refresh(status.clone());
            ui.update(
                &mut host,
                Some(&document),
                &json!({"pointer":destination,"left":true}),
                0.016,
                view,
                &motion,
                options,
                Some("a".into()),
                false,
            )
            .unwrap();
        }
        assert!(ui.poses[&1].1 > 0.);
        assert!(ui.poses[&1].0[0] > r[0]);
        assert!(ui.poses[&1].0[0] < r[0] + l.stride * 1.8);
        assert!(
            ui.poses[&2].0[0] < neighbor[0],
            "neighbor must slide into the vacated slot"
        );
        assert!(ui.consumes_input());
        assert!(ui.destination.is_some());
        ui.update(
            &mut host,
            Some(&document),
            &json!({"pointer":destination,"left":true,"escape":true}),
            0.,
            view,
            &motion,
            options,
            Some("a".into()),
            false,
        )
        .unwrap();
        assert!(ui.press.is_none());
        assert!(ui.destination.is_none());
        assert!(ui.error.is_none());
        assert_eq!(ui.status, status);
    }
}
