//! The first working component browser on the logical left panel.
//! Label placement and pointer targets share the resolved model surface.
use super::{
    panel_layout::{self, Options, Rect, Viewport},
    panel_motion::Snapshot,
};
use serde_json::Value;
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    NewTrack(&'static str),
    InsertTrack(&'static str, usize),
}


// Cardinal Vst3BrowserSystem.cpp:2659 / 2818 selects on down and inserts on
// release inside the timeline. Keep selection after a cancelled drop.
#[derive(Clone,Copy)]
struct ComponentDrag {origin:[f64;2],moved:bool,kind:&'static str,revision:u64,canvas:[f64;4]}
#[derive(Default)]
pub(crate) struct Selection {
 selected:Option<&'static str>, drag:Option<ComponentDrag>, previous_down:bool,
 drop_slot:Option<usize>, captured:bool,
 click:Option<(&'static str,std::time::Instant,u64,[f64;4])>,
}
impl Selection {
 pub fn selected_component(&self)->Option<&'static str>{self.drag.map(|d|d.kind).or(self.selected)}
 pub fn owns_pointer(&self)->bool {self.captured}
 pub fn cancel(&mut self){self.click=None;self.drag=None;self.drop_slot=None;self.captured=false;}
 /// Explicit mobile page handoff only: retain the same held component/revision
 /// while the full-screen browser reveals the timeline. Resize still cancels.
 pub(crate) fn transfer_mobile_canvas(&mut self,canvas:super::piano_roll::Rect){
  if let Some(d)=&mut self.drag {d.canvas=[canvas.left,canvas.top,canvas.width,canvas.height];}
 }
 pub fn status(&self)->Value {serde_json::json!({"selected_component":self.selected,"dragging":self.drag.is_some(),"captured":self.captured,"drop_slot":self.drop_slot})}
 /// Artwork identity and pointer anchor; native presentation owns inertial motion.
 pub fn ghost(&self,pointer:Option<[f64;2]>)->Value{
  let Some(d)=self.drag else{return Value::Null;};
  let Some(p)=pointer.filter(|p|p.iter().all(|x|x.is_finite())) else{return Value::Null;};
  let text=match d.kind{"audio"=>"Audio Track","midi"=>"Midi Track","comp"=>"Comp Lane","matrix"=>"Matrix Bus","mix"=>"Mix Bus","channel"=>"Channel Bus",_=>"CC Track"};
  serde_json::json!({"text":text,"position":[p[0]+12.,p[1]+8.],"preview":d.kind})
 }
 pub fn caption(&self)->Option<String>{
  let kind=self.selected?;let name=match kind{"audio"=>"Audio Track","midi"=>"Midi Track","comp"=>"Comp Lane","matrix"=>"Matrix Bus","mix"=>"Mix Bus","channel"=>"Channel Bus",_=>"CC Track"};
  Some(if self.drag.is_some(){match self.drop_slot{Some(i)=>format!("{name}: release to insert at row {}",i+1),None=>format!("{name}: drag into the destination panel; Escape cancels")}}
  else{format!("Browser: {name} selected")})
 }
 pub fn update(&mut self,input:&super::piano_roll::Input,hit:Option<Action>,slot:Option<usize>,revision:u64,canvas:super::piano_roll::Rect,blocked:bool)->Option<Action>{
  let pressed=input.left&&!self.previous_down;let released=!input.left&&self.previous_down;self.previous_down=input.left;
  let frame=[canvas.left,canvas.top,canvas.width,canvas.height];
  if blocked || input.escape || input.pointer.is_none() || self.drag.is_some_and(|d|d.revision!=revision||d.canvas!=frame){self.click=None;self.drag=None;self.drop_slot=None;self.captured&=input.left;return None;}
  if self.click.is_some_and(|(_,_,r,c)|r!=revision||c!=frame){self.click=None;}
  if pressed {if let Some(Action::NewTrack(kind))=hit{self.selected=Some(kind);self.captured=true;self.drag=Some(ComponentDrag{origin:input.pointer.unwrap(),moved:false,kind,revision,canvas:frame});}}
  if let Some(d)=&mut self.drag {let p=input.pointer.unwrap();d.moved|=(p[0]-d.origin[0]).abs()>3.||(p[1]-d.origin[1]).abs()>3.;if d.moved{self.click=None;}}
  self.drop_slot=self.drag.and(slot);
  if released {self.captured=false;let drag=self.drag.take();self.drop_slot=None;if let Some(d)=drag {
   if let Some(i)=slot {self.click=None;return Some(Action::InsertTrack(d.kind,i));}
   if !d.moved && hit==Some(Action::NewTrack(d.kind)) {
    let now=std::time::Instant::now();
    if self.click.is_some_and(|(kind,time,_,_)|kind==d.kind&&now.duration_since(time).as_secs_f64()<0.35){self.click=None;return Some(Action::NewTrack(d.kind));}
    self.click=Some((d.kind,now,revision,frame));
   }else{self.click=None;}
  }return None;}
  None
 }
}

#[derive(Clone, Copy)]
pub(crate) struct Label {
    pub text: &'static str,
    pub font_size_px: f64,
    pub baseline_px: [f64; 2],
    pub action: Option<Action>,
}

pub(crate) const MARGIN_PX: f64 = 9.6;
// Cardinal Vst3BrowserSystem.cpp:2072 and componentLabel:1392-1394.
// This is the supported component subset, not the full file/plugin library.
pub(crate) const LABELS: [Label; 8] = [
    Label { text: "DAW COMPONENTS", font_size_px: 14.4, baseline_px: [0.,14.4], action: None },
    Label { text: "Audio Track", font_size_px: 16.8, baseline_px: [0.,55.2], action: Some(Action::NewTrack("audio")) },
    Label { text: "Midi Track", font_size_px: 16.8, baseline_px: [0.,98.4], action: Some(Action::NewTrack("midi")) },
    Label { text: "CC Track", font_size_px: 16.8, baseline_px: [0.,141.6], action: Some(Action::NewTrack("cc")) },
    Label { text: "Comp Lane", font_size_px: 16.8, baseline_px: [0.,184.8], action: Some(Action::NewTrack("comp")) },
    Label { text: "Matrix Bus", font_size_px: 16.8, baseline_px: [0.,228.], action: Some(Action::NewTrack("matrix")) },
    Label { text: "Mix Bus", font_size_px: 16.8, baseline_px: [0.,271.2], action: Some(Action::NewTrack("mix")) },
    Label { text: "Channel Bus", font_size_px: 16.8, baseline_px: [0.,314.4], action: Some(Action::NewTrack("channel")) },
];

pub(crate) struct Spec {
    pub bbox_px: [f64; 4],
    // Actual ink, including the three displaced SSLE fracture layers.
    ink_px: [[f64; 4]; LABELS.len()],
}

/// The existing authored glyph metrics are embedded once so a hit test needs
/// no filesystem access, graphics ownership or rendering side effects.
pub(crate) fn spec() -> Result<&'static Spec, String> {
    static SPEC: OnceLock<Result<Spec, String>> = OnceLock::new();
    SPEC.get_or_init(|| {
        let font: Value = serde_json::from_str(include_str!(
            "../Art/AudioWorkstation/LaserEngraving/engraving.json"
        ))
        .map_err(|e| e.to_string())?;
        let number = |v: &Value| {
            v.as_f64()
                .filter(|n| n.is_finite())
                .ok_or_else(|| "invalid browser font metric".to_owned())
        };
        let style = &font["style"];
        let spread = number(&style["radius_em"])? * number(&style["point_radius_range"][1])?
            + number(&style["point_jitter_cells"])? * number(&font["pitch_em"])?;
        let shifts = &style["layer_shift_cells"];
        let pitch = number(&font["pitch_em"])?;
        let mut ink_px = [[0.; 4]; LABELS.len()];
        let mut bbox_px = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for (index, label) in LABELS.iter().enumerate() {
            let mut b = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            let mut x = label.baseline_px[0];
            let em = label.font_size_px;
            for byte in label.text.bytes() {
                let glyph = &font["glyphs"][(byte - 32) as usize];
                if glyph["bounds_em"].is_array() {
                    let bounds = &glyph["bounds_em"];
                    for layer in 0..3 {
                        let sx = number(&shifts[layer][0])? * pitch;
                        let sy = number(&shifts[layer][1])? * pitch;
                        b[0] = b[0].min(x + (number(&bounds[0])? + sx - spread) * em);
                        b[1] = b[1]
                            .min(label.baseline_px[1] - (number(&bounds[3])? + sy + spread) * em);
                        b[2] = b[2].max(x + (number(&bounds[2])? + sx + spread) * em);
                        b[3] = b[3]
                            .max(label.baseline_px[1] - (number(&bounds[1])? + sy - spread) * em);
                    }
                }
                x += number(&glyph["advance_em"])? * em;
            }
            if b.iter().any(|v| !v.is_finite()) {
                return Err("empty browser label".into());
            }
            ink_px[index] = b;
            bbox_px[0] = bbox_px[0].min(b[0]);
            bbox_px[1] = bbox_px[1].min(b[1]);
            bbox_px[2] = bbox_px[2].max(b[2]);
            bbox_px[3] = bbox_px[3].max(b[3]);
        }
        Ok(Spec { bbox_px, ink_px })
    })
    .as_ref()
    .map_err(Clone::clone)
}

pub(crate) struct Layout {
    pub scale_m_per_px: f64,
    pub anchor_m: [f64; 2],
    pub targets: [Rect; LABELS.len()],
}

pub(crate) fn layout(
    view: Viewport,
    motion: &Snapshot,
    options: Options,
) -> Result<Layout, String> {
    let surface = panel_layout::resolved_surface(view, motion, options, 2)?;
    let mpp = 1.3 / view.height;
    let safe = surface.size;
    let spec = spec()?;
    let b = spec.bbox_px;
    let scale = (mpp * view.canvas_scale)
        .min(safe[0] / (b[2] - b[0] + 2. * MARGIN_PX))
        .min(safe[1] / (b[3] - b[1] + 2. * MARGIN_PX));
    let anchor_m = [
        surface.origin[0] - safe[0] * 0.5 + (MARGIN_PX - b[0]) * scale,
        surface.origin[1] + safe[1] * 0.5 - (MARGIN_PX - b[1]) * scale,
    ];
    let targets = std::array::from_fn(|i| {
        let ink = spec.ink_px[i];
        // Small padding around each real label leaves the inter-row gaps inert.
        let padding = 6.;
        Rect {
            x: view.width * 0.5 + (anchor_m[0] + (ink[0] - padding) * scale) / mpp,
            y: view.height * 0.5 - (anchor_m[1] - (ink[1] - padding) * scale) / mpp,
            width: (ink[2] - ink[0] + 2. * padding) * scale / mpp,
            height: (ink[3] - ink[1] + 2. * padding) * scale / mpp,
        }
    });
    Ok(Layout {
        scale_m_per_px: scale,
        anchor_m,
        targets,
    })
}

pub(crate) fn hit(
    view: Viewport,
    motion: &Snapshot,
    options: Options,
    pointer: [f64; 2],
) -> Option<Action> {
    // Logical panel rectangles can be outside the physical screen while sliding
    // or in overview; their hidden content must never receive pointer events.
    if !pointer.iter().all(|v| v.is_finite())
        || pointer[0] < 0.
        || pointer[1] < 0.
        || pointer[0] >= view.width
        || pointer[1] >= view.height
    {
        return None;
    }
    if let Some(style) = super::user_style::selected().ok()? {
        if style.surface(2, view, motion, options).ok()?.is_some()
            && !style.hit_panel_point(2, view, motion, options, pointer).ok()?
        {
            return None;
        }
    }
    let placement = layout(view, motion, options).ok()?;
    placement
        .targets
        .iter()
        .zip(LABELS)
        .find_map(|(rect, label)| {
            (pointer[0] >= rect.x
                && pointer[0] < rect.x + rect.width
                && pointer[1] >= rect.y
                && pointer[1] < rect.y + rect.height)
                .then_some(label.action)
                .flatten()
        })
}

#[cfg(test)]
mod tests {
    use super::super::panel_motion::{Edge, State};
    use super::*;

    #[test]
    fn cardinal_component_rows_exclude_clip_commands() {
        assert_eq!(LABELS.map(|l|l.text), ["DAW COMPONENTS","Audio Track","Midi Track","CC Track","Comp Lane","Matrix Bus","Mix Bus","Channel Bus"]);
        assert!(LABELS[0].action.is_none());
        assert_eq!(LABELS[2].baseline_px[1]-LABELS[1].baseline_px[1],43.2);
    }
    #[test]
    fn browser_actions_follow_the_logical_left_gem_and_leave_gaps_inert() {
        let mut motion = State::default();
        motion.set_all_open(true);
        motion.advance(1.);
        for (width, height) in [(1280., 720.), (390., 844.), (844., 390.)] {
            for canvas_scale in [1., 0.5] {
                for swap_sides in [false, true] {
                    for swap_horizontal in [false, true] {
                        let view = Viewport {
                            width,
                            height,
                            canvas_scale,
                        };
                        let options = Options {
                            swap_sides,
                            swap_horizontal,
                            ..Options::default()
                        };
                        let placement = layout(view, &motion.snapshot(), options).unwrap();
                        for (target, label) in placement.targets.iter().zip(LABELS) {
                            assert_eq!(
                                hit(view, &motion.snapshot(), options, target.center()),
                                label.action
                            );
                        }
                        for pair in placement.targets.windows(2) {
                            let gap = [pair[0].x, (pair[0].y + pair[0].height + pair[1].y) * 0.5];
                            assert!(pair[0].y + pair[0].height < pair[1].y);
                            assert_eq!(hit(view, &motion.snapshot(), options, gap), None);
                        }
                    }
                }
            }
        }
        let view = Viewport {
            width: 1280.,
            height: 720.,
            canvas_scale: 1.,
        };
        let previous = layout(view, &motion.snapshot(), Options::default())
            .unwrap()
            .targets[1]
            .center();
        motion.set_open(Edge::Left, false);
        motion.advance(1.);
        assert_eq!(
            hit(view, &motion.snapshot(), Options::default(), previous),
            None
        );
        let hidden = layout(view, &motion.snapshot(), Options::default())
            .unwrap()
            .targets[1]
            .center();
        assert!(hidden[0] < 0.);
        assert_eq!(
            hit(view, &motion.snapshot(), Options::default(), hidden),
            None
        );
    }

    #[test]
    fn browser_rejects_invalid_and_physically_clipped_pointer_positions() {
        let view = Viewport {
            width: 390.,
            height: 844.,
            canvas_scale: 0.5,
        };
        let motion = State::default().snapshot();
        for pointer in [
            [f64::NAN, 10.],
            [-1., 10.],
            [390., 10.],
            [10., -1.],
            [10., 844.],
        ] {
            assert_eq!(hit(view, &motion, Options::default(), pointer), None);
        }
    }
}

#[cfg(test)]mod component_tests {
 use super::*;use crate::workstation::piano_roll::{Input,Rect};
 fn rect()->Rect{Rect{left:10.,top:20.,width:500.,height:300.}}
 fn input(down:bool)->Input{Input{pointer:Some([100.,100.]),left:down,..Default::default()}}
 #[test]fn selection_does_not_create_and_valid_release_commits_once(){let mut s=Selection::default();assert_eq!(s.update(&input(true),Some(Action::NewTrack("midi")),None,4,rect(),false),None);assert_eq!(s.status()["selected_component"],"midi");for _ in 0..4{assert_eq!(s.update(&input(true),None,Some(2),4,rect(),false),None);}assert_eq!(s.update(&input(false),None,Some(2),4,rect(),false),Some(Action::InsertTrack("midi",2)));assert_eq!(s.update(&input(false),None,Some(2),4,rect(),false),None);}
 #[test]fn double_click_inserts_each_component_once(){
  for kind in ["audio","midi","cc","comp","matrix","mix"] {
   let mut s=Selection::default();let hit=Some(Action::NewTrack(kind));
   s.update(&input(true),hit,None,4,rect(),false);
   assert_eq!(s.update(&input(false),hit,None,4,rect(),false),None);
   s.update(&input(true),hit,None,4,rect(),false);
   assert_eq!(s.update(&input(false),hit,None,4,rect(),false),hit);
   assert_eq!(s.update(&input(false),hit,None,4,rect(),false),None);
  }
 }
 #[test]fn double_click_rejects_motion_expiry_and_revision_change(){
  for mode in 0..3 {
   let mut s=Selection::default();let hit=Some(Action::NewTrack("audio"));
   s.update(&input(true),hit,None,4,rect(),false);s.update(&input(false),hit,None,4,rect(),false);
   if mode==0 {s.click.as_mut().unwrap().1=std::time::Instant::now()-std::time::Duration::from_secs(1);}
   let rev=if mode==1{5}else{4};
   s.update(&input(true),hit,None,rev,rect(),false);
   if mode==2{let mut moved=input(true);moved.pointer=Some([110.,100.]);s.update(&moved,hit,None,rev,rect(),false);}
   assert_eq!(s.update(&input(false),hit,None,rev,rect(),false),None);
  }
 }
 #[test]fn release_in_browser_keeps_selection_without_creation(){let mut s=Selection::default();s.update(&input(true),Some(Action::NewTrack("cc")),None,4,rect(),false);assert_eq!(s.update(&input(false),Some(Action::NewTrack("cc")),None,4,rect(),false),None);assert_eq!(s.status()["selected_component"],"cc");assert!(!s.owns_pointer());}
 #[test]fn foreign_held_press_cannot_begin_component_drag(){let mut s=Selection::default();s.update(&input(true),None,None,4,rect(),false);s.update(&input(true),Some(Action::NewTrack("audio")),None,4,rect(),false);assert!(!s.owns_pointer());assert_eq!(s.update(&input(false),None,Some(0),4,rect(),false),None);}
 #[test]fn focus_escape_revision_layout_and_pointer_loss_cancel(){for case in 0..5{let mut s=Selection::default();s.update(&input(true),Some(Action::NewTrack("audio")),None,4,rect(),false);let mut i=input(true);let mut r=rect();if case==1{i.escape=true;}if case==3{r.width+=1.;}if case==4{i.pointer=None;}s.update(&i,None,Some(0),if case==2{5}else{4},r,case==0);assert_eq!(s.status()["dragging"],false);assert!(s.owns_pointer());assert_eq!(s.update(&input(true),None,Some(0),4,rect(),false),None);assert!(s.owns_pointer());assert_eq!(s.update(&input(false),None,Some(0),4,rect(),false),None);assert!(!s.owns_pointer());}}
}

#[cfg(test)]mod ghost_tests {
 use super::*;use crate::workstation::piano_roll::{Input,Rect};
 #[test]fn ghost_tracks_pointer_and_disappears_when_actionable_drag_ends(){
  for (kind,text) in [("audio","Audio Track"),("midi","Midi Track"),("cc","CC Track")]{
   let mut s=Selection::default();let rect=Rect{left:0.,top:0.,width:500.,height:400.};let mut i=Input{pointer:Some([12.5,30.25]),left:true,..Default::default()};
   assert!(s.ghost(i.pointer).is_null());s.update(&i,Some(Action::NewTrack(kind)),None,0,rect,false);
   assert_eq!(s.ghost(i.pointer),serde_json::json!({"text":text,"position":[24.5,38.25],"preview":kind}));
   i.pointer=Some([400.,250.]);s.update(&i,None,Some(0),0,rect,false);assert_eq!(s.ghost(i.pointer)["position"],serde_json::json!([412.,258.]));
   assert!(s.ghost(Some([f64::NAN,0.])).is_null());i.escape=true;s.update(&i,None,Some(0),0,rect,false);
   assert!(s.owns_pointer());assert!(s.ghost(i.pointer).is_null());
  }
 }
}
