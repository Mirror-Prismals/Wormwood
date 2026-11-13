//! Existing right-panel ORIGAMI placeholder, wired to Cardinal's exact cycle.
//! Reference: DawTransportSystem::UpdateDawTransport / DawOrigamiPanels toggle;
//! DawUiSystem::updateOrigamiPanelButtonVisuals supplies the status text.
use super::{panel_layout::{self,Options,Rect,Viewport},panel_motion::Snapshot};
use std::sync::OnceLock;
#[derive(Clone,Copy,Debug,Default,PartialEq,Eq)]
pub(crate) enum Mode {Off,On,#[default]FixedTop}
impl Mode {
 pub fn next(self)->Self{match self{Self::Off=>Self::On,Self::On=>Self::FixedTop,Self::FixedTop=>Self::Off}}
 pub fn caption(self)->&'static str{match self{Self::Off=>"ORIGAMI: OFF",Self::On=>"ORIGAMI: ON",Self::FixedTop=>"ORIGAMI: FIXED TOP"}}
 pub fn options(self,swap_sides:bool,swap_horizontal:bool)->Options{
  Options{origami:self!=Self::Off,fixed_top:self==Self::FixedTop,swap_sides,swap_horizontal,..Options::default()}
 }
}
#[derive(Default)]
pub(crate) struct Control {pub mode:Mode,pub changed_once:bool,captured:bool,previous_down:bool,pending:bool}
impl Control {
 pub fn owns_pointer(&self)->bool{self.captured}
 pub fn cancel(&mut self){self.captured=false;self.previous_down=false;self.pending=false;}
 pub fn update(&mut self,down:bool,inside:bool,blocked:bool)->bool{
  let activate=self.pending&&!blocked;self.pending=false;
  if activate{self.mode=self.mode.next();self.changed_once=true;}
  if blocked{self.captured=false;}else{
   if down&&!self.previous_down{self.captured=inside;}
   if !down&&self.previous_down{self.pending=self.captured&&inside;self.captured=false;}
  }
  self.previous_down=down;activate
 }
}
struct Spec {block:[f64;4],ink:[f64;4],margin:f64}
fn spec(text: &str)->Option<&'static Spec>{
 static SPEC:OnceLock<Option<Spec>>=OnceLock::new();
 static SETTINGS:OnceLock<Option<Spec>>=OnceLock::new();
 let cache=if text=="SETTINGS"{&SETTINGS}else{&SPEC};
 cache.get_or_init(||{
  let d:serde_json::Value=serde_json::from_str(include_str!("../Art/AudioWorkstation/LaserEngraving/engraving.json")).ok()?;
  let b=d["demo_blocks"].as_array()?.iter().find(|b|b["panel_index"]==3)?;
  let line=b["lines"].as_array()?.iter().find(|l|l["text"]==text)?;
  let block=serde_json::from_value(b["bbox_px"].clone()).ok()?;
  let margin=b["margin_px"].as_f64()?;let em=line["font_size_px"].as_f64()?;
  let baseline:[f64;2]=serde_json::from_value(line["baseline_px"].clone()).ok()?;
  let mut ink=[f64::INFINITY,f64::INFINITY,f64::NEG_INFINITY,f64::NEG_INFINITY];let mut x=baseline[0];
  let style=&d["style"];let pitch=d["pitch_em"].as_f64()?;
  let spread=style["radius_em"].as_f64()?*style["point_radius_range"][1].as_f64()?+style["point_jitter_cells"].as_f64()?*pitch;
  for byte in text.bytes() {let g=&d["glyphs"][(byte-32)as usize];let bounds:[f64;4]=serde_json::from_value(g["bounds_em"].clone()).ok()?;
   for shift in style["layer_shift_cells"].as_array()?{let sx=shift[0].as_f64()?*pitch;let sy=shift[1].as_f64()?*pitch;
    ink[0]=ink[0].min(x+(bounds[0]+sx-spread)*em);ink[1]=ink[1].min(baseline[1]-(bounds[3]+sy+spread)*em);
    ink[2]=ink[2].max(x+(bounds[2]+sx+spread)*em);ink[3]=ink[3].max(baseline[1]-(bounds[1]+sy-spread)*em);
   }x+=g["advance_em"].as_f64()?*em;
  }Some(Spec{block,ink,margin})
 }).as_ref()
}
pub(crate) fn target(view:Viewport,motion:&Snapshot,options:Options)->Option<Rect>{
 target_for(view,motion,options,"ORIGAMI")
}
pub(crate) fn target_for(view:Viewport,motion:&Snapshot,options:Options,text:&str)->Option<Rect>{
 let spec=spec(text)?;let surface=panel_layout::resolved_surface(view,motion,options,3).ok()?;
 let mpp=1.3/view.height;let b=spec.block;let safe=surface.size;
 let scale=(mpp*view.canvas_scale).min(safe[0]/(b[2]-b[0]+2.*spec.margin)).min(safe[1]/(b[3]-b[1]+2.*spec.margin));
 let anchor=[surface.origin[0]-safe[0]*0.5+(spec.margin-b[0])*scale,surface.origin[1]+safe[1]*0.5-(spec.margin-b[1])*scale];
 let i=spec.ink;let pad=6.;Some(Rect{x:view.width*0.5+(anchor[0]+(i[0]-pad)*scale)/mpp,y:view.height*0.5-(anchor[1]-(i[1]-pad)*scale)/mpp,width:(i[2]-i[0]+2.*pad)*scale/mpp,height:(i[3]-i[1]+2.*pad)*scale/mpp})
}
pub(crate) fn hit(view:Viewport,motion:&Snapshot,options:Options,p:[f64;2])->bool{
 hit_for(view,motion,options,p,"ORIGAMI")
}
pub(crate) fn hit_for(view:Viewport,motion:&Snapshot,options:Options,p:[f64;2],text:&str)->bool{
 if !p.iter().all(|x|x.is_finite())||p[0]<0.||p[1]<0.||p[0]>=view.width||p[1]>=view.height{return false;}
 if let Ok(Some(style))=super::user_style::selected(){if style.surface(3,view,motion,options).ok().flatten().is_some() && !style.hit_panel_point(3,view,motion,options,p).unwrap_or(false){return false;}}
 target_for(view,motion,options,text).is_some_and(|r|p[0]>=r.x&&p[0]<r.x+r.width&&p[1]>=r.y&&p[1]<r.y+r.height)
}
#[cfg(test)]mod tests{
 use super::*;use crate::workstation::panel_motion::{State,Edge};
 #[test]fn exact_cardinal_cycle_preserves_approved_initial_mode(){assert_eq!(Mode::default(),Mode::FixedTop);let mut m=Mode::Off;for next in [Mode::On,Mode::FixedTop,Mode::Off]{m=m.next();assert_eq!(m,next);}assert!(!Mode::Off.options(false,false).origami);assert!(!Mode::On.options(false,false).fixed_top);assert!(Mode::FixedTop.options(false,false).fixed_top);}
 #[test]fn release_inside_cycles_once_and_held_press_does_not_repeat(){let mut c=Control::default();assert!(!c.update(true,true,false));for _ in 0..10{assert!(!c.update(true,true,false));}assert!(!c.update(false,true,false));assert!(c.update(false,true,false));assert_eq!(c.mode,Mode::Off);assert!(!c.update(false,true,false));}
 #[test]fn cancelled_release_and_unrelated_drag_do_not_cycle(){for (start,end) in [(true,false),(false,true)]{let mut c=Control::default();c.update(true,start,false);c.update(false,end,false);assert!(!c.update(false,end,false));assert_eq!(c.mode,Mode::FixedTop);}}
 #[test]fn focus_loss_or_owned_gesture_cancels_pending_action(){let mut c=Control::default();c.update(true,true,false);c.update(false,true,false);assert!(!c.update(false,true,true));assert_eq!(c.mode,Mode::FixedTop);c.update(true,true,false);c.cancel();assert!(!c.update(false,true,false));}
 #[test]fn hit_target_follows_all_three_modes_overview_and_panel_swaps(){let mut m=State::default();m.set_all_open(true);m.advance(1.);for mode in [Mode::Off,Mode::On,Mode::FixedTop]{for scale in [0.5,1.]{for swap in [false,true]{let v=Viewport{width:960.,height:600.,canvas_scale:scale};let opts=mode.options(swap,false);let t=target(v,&m.snapshot(),opts).unwrap();assert!(hit(v,&m.snapshot(),opts,t.center()));assert!(!hit(v,&m.snapshot(),opts,[t.x,t.y-t.height*2.]));}}}}
 #[test]fn hidden_and_nonfinite_targets_do_not_activate(){let mut m=State::default();m.set_all_open(true);m.advance(1.);let v=Viewport{width:960.,height:600.,canvas_scale:1.};let opts=Mode::default().options(false,false);let p=target(v,&m.snapshot(),opts).unwrap().center();m.set_open(Edge::Right,false);m.advance(1.);assert!(!hit(v,&m.snapshot(),opts,p));for p in [[-1.,0.],[f64::NAN,0.],[960.,600.]]{assert!(!hit(v,&m.snapshot(),opts,p));}}
 #[test]fn settings_and_origami_have_separate_visible_targets(){let mut m=State::default();m.set_all_open(true);m.advance(1.);let v=Viewport{width:1920.,height:1200.,canvas_scale:1.};let opts=Mode::default().options(false,false);let settings=target_for(v,&m.snapshot(),opts,"SETTINGS").unwrap();assert!(hit_for(v,&m.snapshot(),opts,settings.center(),"SETTINGS"));assert!(!hit(v,&m.snapshot(),opts,settings.center()));}

}
