//! Simple workstation preferences and controls reference.
//! References: DawSettingsLayout.h, DawLaneModalSystem, DawLaneRenderSystem.
use super::{lightbox::Rig, panel_layout::Viewport, piano_roll::Input};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
#[path = "RoutingEditor.rs"]
mod routing_editor;
type Rect = [f64; 4];
#[derive(Clone, Copy, Debug, PartialEq)]
enum Action {
    Routing,
    ClipDisplay,
    Backdrop,
    Theme,
    Sides,
    Vertical,
    Page(bool),
    Reset,
    Apply,
    Cancel,
}
#[derive(Default)]
pub(crate) struct Settings {
    pub open: bool,
    pub theme_supported:bool,
    pub(crate) ar_backdrop:bool,
    applied_passthrough: bool,
    waveform_clips: bool,
    draft_waveform_clips: bool,
    applied_theme:String,
    draft_theme:String,
    draft_passthrough: bool,
    applied: Rig,
    draft: Rig,
    pub panel_placement: Option<[bool; 2]>,
    draft_placement: [bool; 2],
    controls: bool,
    routing_page: bool,
    pub(crate) routing: routing_editor::Editor,
    previous_down: bool,
    pressed: Option<Action>,
    loaded: bool,
    path: Option<PathBuf>,
    pub notice: Option<String>,
}
struct Layout {
    rect: Rect,
    buttons: Vec<(Rect, String, Action)>,
    scale: f64,
}
fn contains(r: Rect, p: [f64; 2]) -> bool {
    p.iter().all(|v| v.is_finite())
        && p[0] >= r[0]
        && p[0] < r[0] + r[2]
        && p[1] >= r[1]
        && p[1] < r[1] + r[3]
}
fn preferences_path() -> Option<PathBuf> {
    std::env::var_os("WORMWOOD_SETTINGS_PATH")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| {
                PathBuf::from(home)
                    .join("Library/Application Support/MirrorPrismals/workstation-settings.json")
            })
        })
}
fn save(path: &Path, rig: &Rig, passthrough: bool, placement: [bool; 2], theme:&str, waveform_clips:bool) -> Result<(), String> {
    let parent = path.parent().ok_or("settings path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(&json!({"version":2,"lightbox":rig.value(),"passthrough":passthrough,"panel_placement":placement,"theme":theme,"spectral_clips":!waveform_clips}))
        .map_err(|e| e.to_string())?;
    let temporary = path.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())
}
impl Settings {
    pub fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        self.applied_theme=super::cardinal_theme::label(&super::cardinal_theme::root());
        self.draft_theme=self.applied_theme.clone();
        if !cfg!(any(target_os = "macos",target_os = "ios")) {
            return;
        }
        self.path = preferences_path();
        if let Some(path) = &self.path {
            match std::fs::read(path) {
                Ok(bytes) => {
                    let result = (|| {
                        if bytes.len() > 16384 {
                            return Err("settings file exceeds 16 KiB".to_string());
                        }
                        let value: Value =
                            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                        if value["version"] != 1 && value["version"] != 2 {
                            return Err("unsupported settings version".into());
                        }
                        self.waveform_clips=value["spectral_clips"].as_bool()==Some(false);
                        self.draft_waveform_clips=self.waveform_clips;
                        self.panel_placement = serde_json::from_value(value["panel_placement"].clone()).ok();
                        if let Some(theme)=value["theme"].as_str(){super::cardinal_theme::select_preference(&super::cardinal_theme::root(),theme)?;self.applied_theme=super::cardinal_theme::label(&super::cardinal_theme::root());self.draft_theme=self.applied_theme.clone();}
                        Rig::parse(&value["lightbox"]).map(|rig|(rig,value["passthrough"]==true))
                    })();
                    match result {
                        Ok((rig,passthrough)) => {
                            self.applied_passthrough=passthrough;self.draft_passthrough=passthrough;
                            self.applied = rig.clone();
                            self.draft = rig;
                        }
                        Err(e) => self.notice = Some(format!("Settings could not be loaded: {e}")),
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => self.notice = Some(format!("Settings could not be loaded: {e}")),
            }
        }
    }
    pub fn enable_camera_passthrough(&mut self){self.applied_passthrough=true;self.draft_passthrough=true;}
    pub fn use_plain_backdrop(&mut self){self.applied_passthrough=false;self.draft_passthrough=false;}
    // Timeline spectral display is temporarily disabled, including saved preferences.
    pub fn spectral_clips(&self)->bool{false}
    pub fn passthrough(&self)->bool{if self.open{self.draft_passthrough}else{self.applied_passthrough}}
    pub fn packet(&self) -> Value {
        if self.open {
            self.draft.value()
        } else {
            self.applied.value()
        }
    }
    /// Finite input validation only; applies the same lightbox packet without
    /// opening a modal or persisting a user preference. Never called normally.
    pub(crate) fn benchmark_light_factor(&mut self,factor:f64)->Result<(),String>{
        if std::env::var("WORMWOOD_WORKSTATION_MOTION_INPUT_TEST").is_err(){return Err("light fixture requires finite input test".into());}
        let mut rig=self.applied.clone();
        for light in &mut rig.lights{light[5]=factor;}
        self.applied=Rig::parse(&rig.value())?;self.draft=self.applied.clone();Ok(())
    }
    pub fn cancel(&mut self) {
        self.open = false;
        self.routing.cancel();
        self.draft_waveform_clips=self.waveform_clips;
        self.draft = self.applied.clone();
        self.draft_passthrough=self.applied_passthrough;
        self.draft_theme=self.applied_theme.clone();
        self.pressed = None;

    }
    fn show(&mut self) {
        if self.applied_theme.is_empty(){self.applied_theme=super::cardinal_theme::label(&super::cardinal_theme::root());}
        self.open = true;
        self.draft_placement = self.panel_placement.unwrap_or([false; 2]);
        self.controls = false;
        self.routing_page = false;
        self.routing.cancel();
        self.draft_waveform_clips=self.waveform_clips;
        self.draft = self.applied.clone();
        self.draft_passthrough=self.applied_passthrough;
        self.draft_theme=self.applied_theme.clone();
        self.pressed = None;

    }
    fn layout(&self, view: Viewport) -> Layout {
        let scale = ((view.width - 24.) / 700.)
            .min((view.height - 24.) / 520.)
            .min(1.)
            .max(0.1);
        let origin = [
            (view.width - 700. * scale) * 0.5,
            (view.height - 520. * scale) * 0.5,
        ];
        let rect = |x: f64, y: f64, w: f64, h: f64| {
            [
                origin[0] + x * scale,
                origin[1] + y * scale,
                w * scale,
                h * scale,
            ]
        };
        let mut buttons = vec![
            (rect(20., 62., 150., 32.), "Workspace".into(), Action::Page(false)),
            (rect(180., 62., 150., 32.), "Controls".into(), Action::Page(true)),
            (rect(20., 470., 150., 30.), "Reset workspace".into(), Action::Reset),
            (rect(456., 470., 100., 30.), "Cancel".into(), Action::Cancel),
            (rect(570., 470., 110., 30.), "Apply".into(), Action::Apply),
            (rect(650., 12., 30., 26.), "X".into(), Action::Cancel),
        ];
        buttons.push((rect(340., 62., 150., 32.), "Audio Routing".into(), Action::Routing));
        if self.routing_page {
            buttons.retain(|(_, _, a)| !matches!(a, Action::Reset | Action::Apply));
            for (_, text, a) in &mut buttons { if *a == Action::Cancel && text != "X" { *text = "Close".into(); } }
        }
        if !self.controls && !self.routing_page {
            buttons.extend([
                (rect(380., 274., 290., 30.), if self.draft_waveform_clips {"Clips: Waveform".into()}else{"Clips: Spectral".into()}, Action::ClipDisplay),
                (rect(240., 378., 430., 34.), if self.draft_theme=="ruby-3d"{"3D gem ruby (main)".into()}else{format!("Cardinal 2D: {}",self.draft_theme)}, Action::Theme),
                (rect(380., 144., 290., 36.), if self.ar_backdrop {"AR view (locked)".into()} else if self.draft_passthrough {"Passthrough".into()} else {"Plain".into()}, Action::Backdrop),
                (rect(380., 236., 290., 36.), if self.draft_placement[0] {"Swapped".into()} else {"Standard".into()}, Action::Sides),
                (rect(380., 310., 290., 36.), if self.draft_placement[1] {"Swapped".into()} else {"Standard".into()}, Action::Vertical),
            ]);
        }
        buttons.retain(|(_,_,a)| *a != Action::ClipDisplay);
        if !self.theme_supported {buttons.retain(|(_,_,a)|*a!=Action::Theme);}
        Layout {
            rect: rect(0., 0., 700., 520.),
            buttons,
            scale,
        }
    }
    // Returns whether this frame owns input, including opening/closing clicks.
    pub fn update(
        &mut self,
        input: &Input,
        view: Viewport,
        open_hit: bool,
        blocked: bool,
        focused: bool,
    ) -> bool {
        let pressed = input.left && !self.previous_down;
        let released = !input.left && self.previous_down;
        self.previous_down = input.left;
        if !focused {
            self.pressed = None;
            self.routing.cancel();
            return self.open;
        }
        if !self.open {
            if blocked {
                self.pressed = None;
                return false;
            }
            if pressed && open_hit {
                self.pressed = Some(Action::Apply);
            }
            let captured = self.pressed.is_some();
            if released {
                if captured && open_hit {
                    self.show();
                }
                self.pressed = None;
            }
            return captured || self.open;
        }
        if input.escape {
            self.cancel();
            return true;
        }
        let layout = self.layout(view);
        if self.routing_page { self.routing.update(input, layout.rect, layout.scale, pressed, released); }
        if pressed {
            self.pressed = input
                .pointer
                .and_then(|p| layout.buttons.iter().find(|(r, _, _)| contains(*r, p)))
                .map(|b| b.2);
        }
        if released {
    
            let action = self.pressed.take().filter(|a| {
                input.pointer.is_some_and(|p| {
                    layout
                        .buttons
                        .iter()
                        .any(|(r, _, b)| a == b && contains(*r, p))
                })
            });
            match action {
                Some(Action::Theme)=>{
                    let names=super::cardinal_theme::presets(&super::cardinal_theme::root());
                    let current=names.iter().position(|n|*n==self.draft_theme).unwrap_or(0);
                    self.draft_theme=names[(current+1)%names.len()].clone();
                },
                Some(Action::ClipDisplay)=>{self.draft_waveform_clips=!self.draft_waveform_clips;},
                Some(Action::Backdrop)=>{if !self.ar_backdrop{self.draft_passthrough=!self.draft_passthrough;}},
                Some(Action::Sides) => self.draft_placement[0] = !self.draft_placement[0],
                Some(Action::Vertical) => self.draft_placement[1] = !self.draft_placement[1],
                Some(Action::Page(controls)) => { self.controls = controls; self.routing_page = false; self.routing.cancel(); },
                Some(Action::Routing) => { self.routing_page = true; self.routing.cancel(); },
                Some(Action::Reset) => { self.draft_waveform_clips=false; self.draft_placement = [false; 2]; if !self.ar_backdrop { self.draft_passthrough = false; } },
                Some(Action::Cancel) => self.cancel(),
                Some(Action::Apply) => {
                    let result = self
                        .path
                        .as_ref()
                        .ok_or_else(|| "Settings location is unavailable".to_string())
                        .and_then(|p| {if self.draft_theme!="ruby-3d"&&super::user_style::selected()?.is_some(){return Err("Select the main theme when a custom model style is active".into());}super::cardinal_theme::validate_selection(&super::cardinal_theme::root(),&self.draft_theme)?;save(p, &self.applied,self.draft_passthrough,self.draft_placement,&self.draft_theme,self.draft_waveform_clips)});
                    match result {
                        Ok(()) => {
                            if let Err(e)=super::cardinal_theme::select(&super::cardinal_theme::root(),&self.draft_theme){self.notice=Some(e);return true;}
                            self.waveform_clips=self.draft_waveform_clips;
                            self.applied_theme=self.draft_theme.clone();
                            self.panel_placement = Some(self.draft_placement);
                            self.applied_passthrough=self.draft_passthrough;
                            self.open = false;
                            self.notice = None;
                        }
                        Err(e) => self.notice = Some(format!("Could not save: {e}")),
                    }
                }
                None => {}
            }
        }
        true
    }
    pub fn surface(&self, view: Viewport, pointer: Option<[f64; 2]>) -> Value {
        if !self.open {
            return Value::Null;
        }
        let l = self.layout(view);
        let s = l.scale;
        let [x, y, _, _] = l.rect;
        let mut faces = Vec::new();
        let mut labels = Vec::new();
        let face = |r: Rect, c: [u8; 3]| json!({"rect":r,"color":c,"radius":0.,"alpha":1.});
        let label = |r: Rect, text: &str, size: f64, center: bool| json!({"rect":r,"text":text,"size":size,"center":center,"color":[225,235,237]});
        faces.push(face(
            [
                x - 4. * s,
                y - 4. * s,
                l.rect[2] + 8. * s,
                l.rect[3] + 8. * s,
            ],
            [77, 87, 89],
        ));
        faces.push(face(l.rect, [51, 59, 61]));
        labels.push(label(
            [x + 20. * s, y + 12. * s, 600. * s, 26. * s],
            "Settings",
            20. * s,
            false,
        ));
        let lines: Vec<(f64, &str, f64)> = if self.routing_page {
            self.routing.draw(l.rect, s, pointer, &mut faces, &mut labels);
            vec![]
        } else if self.controls {
            vec![(122., "WORKSPACE NAVIGATION", 16.),
                (160., "Arrow keys: open or close panels", 14.),
                (194., "Option / Alt + arrow: normal / 3/4 / full", 14.),
                (228., "Escape: leave full view or cancel a dialog", 14.),
                (262., "Shift + scroll down/up: overview / return", 14.),
                (296., "Space: play / pause", 14.),
                (350., "Touch: swipe to open a panel; opposite swipe to close", 12.),
                (380., "Two fingers: scroll. Pinch: zoom.", 12.)]
        } else {
            vec![(116., "DISPLAY", 16.), (152., "Background", 14.),
                (190., "AR keeps the real view visible.", 12.),
                (214., "PANEL PLACEMENT", 16.), (246., "Left / right panels", 14.),
                (282., "Clip display", 14.),
                (320., "Top / bottom panels", 14.),
                (386., "Theme", 14.),
                (416., if self.theme_supported {"3D gem ruby is the main theme. Click to cycle 2D presets."}else{"3D gem ruby is the main theme. 2D themes require WebGPU."}, 12.)]
        };
        for (offset, text, size) in lines {
            labels.push(label([x + 20. * s, y + offset * s, 650. * s, 24. * s], text, size * s, false));
        }
        if let Some(message) = &self.notice {
            labels.push(label(
                [x + 20. * s, y + 432. * s, 660. * s, 28. * s],
                message,
                11. * s,
                false,
            ));
        }
        for (r, text, action) in &l.buttons {
            let hovered = pointer.is_some_and(|p| contains(*r, p));
            let color = if self.pressed == Some(*action) {
                [43, 65, 82]
            } else if matches!(action, Action::Page(page) if !self.routing_page && *page == self.controls) || (*action == Action::Routing && self.routing_page) {
                [71, 100, 122]
            } else if hovered {
                [71, 100, 122]
            } else {
                [66, 74, 77]
            };
            faces.push(face(*r, color));
            labels.push(label(*r, text, 12. * s, true));
        }
        json!({"clip":[0.,0.,view.width,view.height],"browser_faces":faces,"labels":labels})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn view() -> Viewport {
        Viewport {
            width: 960.,
            height: 600.,
            canvas_scale: 1.,
        }
    }
    #[test]
    fn timeline_waveforms_override_saved_spectral_choice() {
        let mut s=Settings::default();
        assert!(!s.spectral_clips());
        s.waveform_clips=false;
        s.show();
        assert!(!s.spectral_clips());
        assert!(!s.layout(view()).buttons.iter().any(|b|b.2==Action::ClipDisplay));
        s.cancel();
        assert!(!s.spectral_clips());
    }
    #[test]
    fn routing_tab_is_live_and_has_no_workspace_apply_or_reset() {
        let mut settings = Settings::default();
        settings.show();
        settings.routing.sync(&json!({"buses":(["L","S","FF","R"].iter().map(|c|json!({"id":format!("matrix-{c}"),"name":format!("Matrix {c}"),"kind":"matrix","output":"flown_array","output_channel":c})).collect::<Vec<_>>()),"physical_outputs":[{"id":"flown_array","name":"Flown Array","channels":["L","S","FF","R"]}]}));
        let r = settings.layout(view()).buttons.into_iter().find(|b| b.2 == Action::Routing).unwrap().0;
        let mut input = Input { pointer: Some([r[0]+1.,r[1]+1.]), left:true, ..Default::default() };
        settings.update(&input,view(),false,false,true);
        input.left=false;
        settings.update(&input,view(),false,false,true);
        assert!(settings.routing_page);
        assert!(settings.layout(view()).buttons.iter().all(|b| !matches!(b.2,Action::Apply|Action::Reset)));
        let surface=settings.surface(view(),None);
        assert!(surface["labels"].as_array().unwrap().iter().any(|l|l["text"]=="PHYSICAL OUTPUTS"));
        if let Some(path)=std::env::var_os("WORMWOOD_ROUTING_TEST_PREVIEW") {
            std::fs::write(path,serde_json::to_vec_pretty(&surface).unwrap()).unwrap();
        }
    }
    #[test]
    fn opening_release_cancel_and_focus_loss_do_not_leak() {
        let mut s = Settings::default();
        let mut i = Input::default();
        i.left = true;
        assert!(s.update(&i, view(), true, false, true));
        i.left = false;
        assert!(s.update(&i, view(), true, false, true));
        assert!(s.open);
        s.draft.lights[0][5] = 0.2;
        i.escape = true;
        assert!(s.update(&i, view(), false, false, true));
        assert!(!s.open);
        assert_eq!(s.draft, s.applied);
        i.escape = false;
        i.left = true;
        s.update(&i, view(), true, false, true);
        s.update(&i, view(), true, false, false);
        i.left = false;
        s.update(&i, view(), true, false, true);
        assert!(!s.open);
    }
    #[test]
    fn settings_roundtrip_keeps_light_choices() {
        let path = std::env::temp_dir().join(format!(
            "wormwood-settings-test-{}.json",
            std::process::id()
        ));
        let mut rig = Rig::default();
        rig.lights[1][0] = 35.;
        rig.lights[1][5] = 0.4;
        save(&path, &rig,false,[true, false],"ruby-3d",false).unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(Rig::parse(&v["lightbox"]).unwrap(), rig);
        assert_eq!(v["panel_placement"], json!([true, false]));
        assert_eq!(v["spectral_clips"],true);
        save(&path,&rig,false,[true,false],"ruby-3d",true).unwrap();
        let v:Value=serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v["spectral_clips"],false);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn layout_stays_inside_small_and_fullscreen_viewports() {
        let s = Settings::default();
        for (w, h) in [(640., 480.), (1920., 1200.), (320., 320.)] {
            let l = s.layout(Viewport {
                width: w,
                height: h,
                canvas_scale: 1.,
            });
            assert!(l.rect[0] >= 0. && l.rect[1] >= 0.);
            for (r, _, _) in l.buttons {
                assert!(
                    r[0] >= l.rect[0]
                        && r[0] + r[2] <= l.rect[0] + l.rect[2] + 0.01
                        && r[1] + r[3] <= l.rect[1] + l.rect[3] + 0.01
                );
            }
        }
    }
    #[test]
    fn settings_have_no_light_editor_and_cancel_discards_placement() {
        let mut s = Settings::default();
        s.show();
        let surface = s.surface(view(), None).to_string();
        assert!(!surface.to_lowercase().contains("lightbox"));
        let r = s.layout(view()).buttons.into_iter().find(|b| b.2 == Action::Sides).unwrap().0;
        let mut i = Input { pointer: Some([r[0]+1., r[1]+1.]), left: true, ..Default::default() };
        s.update(&i, view(), false, false, true);
        i.left = false;
        s.update(&i, view(), false, false, true);
        assert!(s.draft_placement[0]);
        assert_eq!(s.panel_placement, None);
        s.cancel();
        s.show();
        assert_eq!(s.draft_placement, [false; 2]);
    }
    #[test]
    fn apply_persists_placement_without_editing_legacy_lighting() {
        let path = std::env::temp_dir().join(format!("wormwood-settings-apply-{}.json", std::process::id()));
        let mut s = Settings::default();
        s.path = Some(path.clone());
        s.applied.lights[0][5] = 0.3;
        let original = s.packet();
        s.show();
        s.draft_placement = [true, true];
        let r = s.layout(view()).buttons.into_iter().find(|b| b.2 == Action::Apply).unwrap().0;
        let mut i = Input { pointer: Some([r[0]+1., r[1]+1.]), left: true, ..Default::default() };
        s.update(&i, view(), false, false, true);
        i.left = false;
        s.update(&i, view(), false, false, true);
        assert!(!s.open);
        assert_eq!(s.panel_placement, Some([true, true]));
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["lightbox"], original);
        assert_eq!(saved["panel_placement"], json!([true, true]));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn cardinal_theme_chooser_is_draft_until_apply_and_cancel_keeps_ruby_main() {
        let mut settings=Settings::default();settings.theme_supported=true;settings.applied_theme="ruby-3d".into();settings.show();
        let button=settings.layout(view()).buttons.into_iter().find(|b|b.2==Action::Theme).unwrap().0;
        let mut input=Input{pointer:Some([button[0]+2.,button[1]+2.]),left:true,..Default::default()};
        settings.update(&input,view(),false,false,true);input.left=false;settings.update(&input,view(),false,false,true);
        assert_ne!(settings.draft_theme,"ruby-3d");assert_eq!(settings.applied_theme,"ruby-3d");
        settings.cancel();assert_eq!(settings.draft_theme,"ruby-3d");
    }
    #[test]
    fn preview_fixture() {
        let Some(path) = std::env::var_os("WORMWOOD_SETTINGS_TEST_PREVIEW") else {
            return;
        };
        let mut s = Settings::default();
        s.show();
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&json!({"groups":[s.surface(view(),None)]})).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn release_outside_does_not_change_placement() {
        let mut s = Settings::default();
        s.show();
        let l = s.layout(view());
        let r = l
            .buttons
            .iter()
            .find(|(_, _, a)| *a == Action::Sides)
            .unwrap()
            .0;
        let mut i = Input {
            pointer: Some([r[0] + 1., r[1] + 1.]),
            left: true,
            ..Default::default()
        };
        s.update(&i, view(), false, false, true);
        i.left = false;
        i.pointer = Some([0., 0.]);
        s.update(&i, view(), false, false, true);
        assert_eq!(s.draft_placement, [false; 2]);
    }
}
