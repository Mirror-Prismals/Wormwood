//! Original button_world.json loop_toggle: managed release, independent latch.
use super::{panel_layout::{self,Options,Viewport},panel_motion::Snapshot,piano_roll::Rect};
#[derive(Default)]
pub(crate) struct Control {pub pressure:f64,captured:bool,previous:bool,pending:bool,hold:bool}
impl Control {
 pub fn owns_pointer(&self)->bool{self.captured||self.pending}
 pub fn cancel(&mut self){self.captured=false;self.pending=false;self.hold=false;self.previous=false;}
 pub fn update(&mut self,down:bool,hit:bool,blocked:bool,enabled:bool,dt:f64,events:&[(bool,bool)])->bool{
  let action=!blocked&&std::mem::take(&mut self.pending);
  if blocked {self.cancel();self.previous=down;}else{
   for &(down,hit) in events.iter().take(16){self.pointer(down,hit);}self.pointer(down,hit);
  }
  let target=if enabled||self.captured&&down||self.hold{0.5}else{0.};
  let delta=if dt.is_finite(){(0.5/0.15)*dt.clamp(0.,1.)}else{0.};
  self.pressure=if self.pressure<target{(self.pressure+delta).min(target)}else{(self.pressure-delta).max(target)};
  self.hold=false;action
 }
 fn pointer(&mut self,down:bool,hit:bool){
  if down&&!self.previous{self.captured=hit;}
  if !down&&self.previous{if self.captured&&hit{self.pending=true;self.hold=true;}self.captured=false;}
  self.previous=down;
 }
}
/// Preserve the original loop position relative to the unchanged four transport
/// centers: old centers [517.5,697.5,877.5,1057.5], loop [1165,68], row y=120.
/// Current approved transport centers have spacing .055m and remain untouched.
pub(crate) fn geometry(view:Viewport,motion:&Snapshot,options:Options)->Result<Rect,String>{
 let s=panel_layout::resolved_surface(view,motion,options,0)?;
 let scale=(s.size[0]*0.88/0.205).min(s.size[1]*0.80/0.04);
 let mpp=1.3/view.height;let original_unit=0.055/180.*scale/mpp;
 let x=view.width*0.5+s.origin[0]/mpp+(1165.-787.5)*original_unit;
 let y=view.height*0.5-s.origin[1]/mpp+(68.-120.)*original_unit;
 Ok(Rect{left:x-18.*original_unit,top:y-18.*original_unit,width:36.*original_unit,height:36.*original_unit})
}
pub(crate) fn hit(r:Rect,p:[f64;2])->bool{
 p.iter().all(|v|v.is_finite())&&p[0]>=r.left&&p[0]<=r.left+r.width&&p[1]>=r.top&&p[1]<=r.top+r.height
}
#[cfg(test)]mod tests {
 use super::*;
 #[test]fn short_click_is_managed_once_and_latch_is_external(){let mut c=Control::default();assert!(!c.update(false,true,false,false,0.075,&[(true,true),(false,true)]));assert_eq!(c.pressure,0.25);assert!(c.update(false,true,false,false,0.075,&[]));assert!(!c.update(false,true,false,true,0.15,&[]));assert_eq!(c.pressure,0.5);}
 #[test]fn outside_release_foreign_drag_and_focus_loss_cancel(){for ev in [[(true,false),(false,true)],[(true,true),(false,false)]]{let mut c=Control::default();c.update(false,true,false,false,0.1,&ev);assert!(!c.update(false,true,false,false,0.1,&[]));}let mut c=Control::default();c.update(true,true,false,false,0.1,&[]);assert!(!c.update(false,true,true,false,0.1,&[]));assert!(!c.owns_pointer());}
}
