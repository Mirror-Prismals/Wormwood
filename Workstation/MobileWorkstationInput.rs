//! Mobile gesture arbitration only. Existing workstation controllers own actions.
//! Times are monotonic seconds supplied by the bridge; positions are UIKit points.
use std::collections::VecDeque;
const HOLD:f64=0.18;
const SWIPE_TIME:f64=0.30;
const SWIPE_DISTANCE:f64=48.;
const SLOP:f64=8.;
const CAPACITY:usize=32;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum Panel { Top,Bottom,Left,Right }
impl Panel { pub fn name(self)->&'static str {match self{Self::Top=>"top",Self::Bottom=>"bottom",Self::Left=>"left",Self::Right=>"right"}} }
#[derive(Clone,Copy,Debug,PartialEq)]
pub(crate) struct Sample {
 pub pointer:Option<[f64;2]>,pub left:bool,pub wheel:[f64;2],pub command:bool,
 pub cancelled:bool,pub panel:Option<Panel>,
}
impl Default for Sample {fn default()->Self{Self{pointer:None,left:false,wheel:[0.;2],command:false,cancelled:false,panel:None}}}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]enum Kind{Down,Move,Up,Wheel,Cancel,Panel}
#[derive(Clone,Copy)]struct Queued{kind:Kind,sample:Sample}
#[derive(Clone,Copy)]struct Contact{id:u64,point:[f64;2]}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]enum TwoMode{Pending,Pan,Pinch}
#[derive(Clone,Copy)]enum Mode{
 Idle,Pending{start:[f64;2],at:f64,early_motion:bool},Drag,Suppressed,
 Two{mode:TwoMode,origin:[f64;2],center:[f64;2],span:f64,initial_span:f64},
}
pub(crate) struct State{contacts:[Option<Contact>;2],mode:Mode,queue:VecDeque<Queued>,current:Sample,last_time:f64,pub overflows:u64}
impl Default for State{fn default()->Self{Self{contacts:[None,None],mode:Mode::Idle,queue:VecDeque::with_capacity(CAPACITY),current:Sample::default(),last_time:0.,overflows:0}}}
fn distance(a:[f64;2],b:[f64;2])->f64{(a[0]-b[0]).hypot(a[1]-b[1])}
fn direction(d:[f64;2])->Option<Panel>{
 if d[0].abs().max(d[1].abs())<SWIPE_DISTANCE{return None;}
 if d[0].abs()>d[1].abs()*1.25{Some(if d[0]>0.{Panel::Left}else{Panel::Right})}
 else if d[1].abs()>d[0].abs()*1.25{Some(if d[1]>0.{Panel::Top}else{Panel::Bottom})}else{None}
}
impl State{
 fn clock(&mut self,now:f64)->Option<f64>{if !now.is_finite(){return None;}self.last_time=self.last_time.max(now);Some(self.last_time)}
 fn enqueue(&mut self,kind:Kind,sample:Sample){
  if kind==Kind::Move&&self.queue.back().is_some_and(|q|q.kind==Kind::Move){self.queue.back_mut().unwrap().sample=sample;return;}
  if self.queue.len()==CAPACITY{self.overflows+=1;self.cancel();return;}
  self.queue.push_back(Queued{kind,sample});
 }
 pub fn cancel(&mut self){
  self.contacts=[None,None];self.mode=Mode::Idle;self.current=Sample::default();self.queue.clear();
  self.queue.push_back(Queued{kind:Kind::Cancel,sample:Sample{cancelled:true,..Sample::default()}});
 }
 fn first(&self)->Option<[f64;2]>{self.contacts.iter().flatten().next().map(|c|c.point)}
 fn pair(&self)->Option<([f64;2],f64)>{let a=self.contacts[0]?.point;let b=self.contacts[1]?.point;Some(([(a[0]+b[0])*0.5,(a[1]+b[1])*0.5],distance(a,b).max(1.)))}
 fn begin_two(&mut self){
  let Some((center,span))=self.pair()else{return;};
  // Two-finger navigation must never complete a pending one-finger click.
  self.queue.clear();self.current=Sample::default();
  self.enqueue(Kind::Cancel,Sample{cancelled:true,..Sample::default()});
  self.mode=Mode::Two{mode:TwoMode::Pending,origin:center,center,span,initial_span:span};
 }
 fn advance(&mut self,now:f64){
  if let Mode::Pending{start,at,early_motion}=self.mode{
   if now-at>=HOLD&&!early_motion{
    let point=self.first().unwrap_or(start);self.enqueue(Kind::Down,Sample{pointer:Some(point),left:true,..Sample::default()});
    if self.contacts.iter().any(Option::is_some){self.mode=Mode::Drag;}
   }else if now-at>SWIPE_TIME&&early_motion{self.mode=Mode::Suppressed;}
  }
 }
 /// phase: 0 began, 1 moved, 2 ended, 3 cancelled. Unknown IDs/phases fail closed.
 pub fn event(&mut self,id:u64,phase:u32,point:[f64;2],now:f64){
  let Some(now)=self.clock(now)else{self.cancel();return;};
  if point.iter().any(|x|!x.is_finite()||x.abs()>1e6)||phase>3{self.cancel();return;}
  if phase==3{self.cancel();return;}
  let found=self.contacts.iter().position(|c|c.is_some_and(|c|c.id==id));
  if phase==0{
   if found.is_some(){self.cancel();return;}
   let Some(slot)=self.contacts.iter().position(Option::is_none)else{self.cancel();return;};
   self.contacts[slot]=Some(Contact{id,point});
   if self.contacts.iter().flatten().count()==2{self.begin_two();}
   else{self.mode=Mode::Pending{start:point,at:now,early_motion:false};}
   return;
  }
  let Some(slot)=found else{return;};
  // An already stationary-held contact remains a control drag, never a swipe.
  self.advance(now);
  // A full queue may cancel this contact when advance commits its deferred hold.
  let Some(contact)=self.contacts[slot].as_mut()else{return;};
  contact.point=point;
  match self.mode{
   Mode::Pending{start,at,mut early_motion}=>{
    let d=[point[0]-start[0],point[1]-start[1]];
    early_motion|=distance(point,start)>SLOP;
    if now-at<=SWIPE_TIME&&early_motion{
     if let Some(panel)=direction(d){
      self.enqueue(Kind::Panel,Sample{cancelled:true,panel:Some(panel),..Sample::default()});self.mode=Mode::Suppressed;
     }else{self.mode=Mode::Pending{start,at,early_motion};}
    }
    if phase==2&&matches!(self.mode,Mode::Pending{..}){
     if distance(point,start)<=SLOP{
      self.enqueue(Kind::Down,Sample{pointer:Some(start),left:true,..Sample::default()});
      self.enqueue(Kind::Up,Sample{pointer:Some(point),..Sample::default()});
     }
     self.mode=Mode::Suppressed;
    }
   },
   Mode::Drag=>{self.enqueue(if phase==2{Kind::Up}else{Kind::Move},Sample{pointer:Some(point),left:phase!=2,..Sample::default()});},
   Mode::Two{mut mode,origin,center,span,initial_span}=>{
    if phase==1{if let Some((new_center,new_span))=self.pair(){
     if mode==TwoMode::Pending{
      if (new_span/initial_span).ln().abs()>=0.04{mode=TwoMode::Pinch;}
      else if distance(new_center,origin)>=4.{mode=TwoMode::Pan;}
     }
     let wheel=match mode{TwoMode::Pan=>[(new_center[0]-center[0])/40.,(new_center[1]-center[1])/40.],TwoMode::Pinch=>[0.,if (new_span/span).ln().abs()>=1.18f64.ln(){-(new_span/span).ln().signum()}else{0.}],TwoMode::Pending=>[0.;2]};
     if wheel!=[0.;2]{self.enqueue(Kind::Wheel,Sample{pointer:Some(new_center),wheel,command:mode==TwoMode::Pinch,..Sample::default()});}
     self.mode=Mode::Two{mode,origin,center:if mode==TwoMode::Pending{center}else{new_center},span:if mode==TwoMode::Pending||wheel==[0.;2]{span}else{new_span},initial_span};
    }}else if phase==2{self.mode=Mode::Suppressed;}
   },Mode::Idle|Mode::Suppressed=>{},
  }
  if phase==2{self.contacts[slot]=None;if self.contacts.iter().all(Option::is_none){self.mode=Mode::Idle;}}
 }
 /// At most one complete sample per engine frame. Down/up edges never coalesce.
 pub fn next_sample(&mut self,now:f64)->Sample{
  if let Some(now)=self.clock(now){self.advance(now);}else{self.cancel();}
  if let Some(item)=self.queue.pop_front(){
   self.current=item.sample;
   if item.sample.cancelled{self.current=Sample::default();}
   let sample=item.sample;self.current.wheel=[0.;2];self.current.command=false;self.current.panel=None;self.current.cancelled=false;return sample;
  }
  self.current
 }
 pub fn queued(&self)->usize{self.queue.len()}
}
#[cfg(test)]mod tests{
 use super::*;
 #[test]fn quick_tap_keeps_down_and_up_on_distinct_frames(){let mut s=State::default();s.event(1,0,[50.,50.],0.);s.event(1,2,[50.,50.],0.05);assert_eq!(s.queued(),2);assert!(s.next_sample(0.06).left);assert!(!s.next_sample(0.07).left);assert!(!s.next_sample(0.08).left);}
 #[test]fn quick_swipe_anywhere_never_clicks(){for (delta,panel)in [([70.,0.],Panel::Left),([-70.,0.],Panel::Right),([0.,70.],Panel::Top),([0.,-70.],Panel::Bottom)]{let mut s=State::default();s.event(1,0,[300.,200.],0.);s.event(1,1,[300.+delta[0],200.+delta[1]],0.1);s.event(1,2,[300.+delta[0],200.+delta[1]],0.15);let a=s.next_sample(0.16);assert_eq!(a.panel,Some(panel));assert!(a.cancelled&&!a.left);assert_eq!(s.queued(),0);}}
 #[test]fn stationary_hold_then_drag_cannot_navigate(){let mut s=State::default();s.event(1,0,[30.,30.],0.);let down=s.next_sample(0.19);assert!(down.left);s.event(1,1,[120.,30.],0.21);assert_eq!(s.next_sample(0.22).panel,None);s.event(1,2,[130.,30.],0.25);assert!(!s.next_sample(0.26).left);}
 #[test]fn early_motion_can_finish_swipe_after_hold_threshold(){let mut s=State::default();s.event(1,0,[50.,50.],0.);s.event(1,1,[70.,50.],0.1);assert!(!s.next_sample(0.2).left);s.event(1,2,[120.,50.],0.25);assert_eq!(s.next_sample(0.26).panel,Some(Panel::Left));}
 #[test]fn diagonal_or_slow_motion_does_not_edit_or_switch(){for (point,time)in [([100.,100.],0.1),([130.,50.],0.4)]{let mut s=State::default();s.event(1,0,[50.,50.],0.);s.event(1,1,[65.,65.],0.1);s.event(1,2,point,time);let a=s.next_sample(time+0.01);assert!(!a.left&&a.panel.is_none());}}
 #[test]fn moves_coalesce_but_release_survives_stall(){let mut s=State::default();s.event(1,0,[50.,50.],0.);s.next_sample(0.2);for n in 0..100{s.event(1,1,[50.+n as f64,50.],0.21+n as f64*0.001);}s.event(1,2,[150.,50.],0.4);assert_eq!(s.queued(),2);assert_eq!(s.next_sample(0.41).pointer,Some([149.,50.]));assert!(!s.next_sample(0.42).left);}
 #[test]fn overflow_cancels_instead_of_losing_release(){let mut s=State::default();for n in 0..20{s.event(n,0,[50.,50.],n as f64);s.event(n,2,[50.,50.],n as f64+0.05);}assert!(s.queued()<=CAPACITY&&s.overflows>0);let first=s.next_sample(21.);assert!(first.cancelled&&!first.left);}
 #[test]fn overflow_during_deferred_hold_cancels_without_panicking(){let mut s=State::default();for n in 0..16{s.event(n,0,[50.,50.],n as f64);s.event(n,2,[50.,50.],n as f64+0.05);}assert_eq!(s.queued(),CAPACITY);s.event(100,0,[50.,50.],20.);s.event(100,1,[55.,50.],20.2);let a=s.next_sample(20.21);assert!(a.cancelled&&!a.left);assert_eq!(s.overflows,1);assert_eq!(s.queued(),0);}
 #[test]fn second_finger_cancels_unconsumed_click(){let mut s=State::default();s.event(1,0,[50.,50.],0.);s.next_sample(0.2);s.event(2,0,[100.,50.],0.21);let a=s.next_sample(0.22);assert!(a.cancelled&&!a.left);s.event(2,2,[100.,50.],0.23);s.event(1,2,[50.,50.],0.24);assert!(!s.next_sample(0.25).left);}
 #[test]fn two_finger_pan_does_not_switch_panels(){let mut s=State::default();s.event(1,0,[50.,50.],0.);s.event(2,0,[50.,100.],0.01);s.next_sample(0.02);s.event(1,1,[60.,50.],0.03);let a=s.next_sample(0.04);assert!(a.wheel[0]>0.&&!a.left&&a.panel.is_none()&&!a.command);}
 #[test]fn pinch_produces_zoom_wheel(){let mut s=State::default();s.event(1,0,[50.,50.],0.);s.event(2,0,[100.,50.],0.01);s.next_sample(0.02);s.event(2,1,[120.,50.],0.03);let a=s.next_sample(0.04);assert!(a.command&&a.wheel[1]<0.&&!a.left);assert_eq!(s.next_sample(0.05).wheel,[0.;2]);}
 #[test]fn interruption_clears_pending_clicks_and_hold(){let mut s=State::default();s.event(1,0,[50.,50.],0.);s.event(1,2,[50.,50.],0.05);s.cancel();let a=s.next_sample(0.06);assert!(a.cancelled&&!a.left);assert_eq!(s.queued(),0);assert!(!s.next_sample(1.).left);}
 #[test]fn invalid_events_cancel_without_action(){let mut s=State::default();s.event(1,0,[f64::NAN,0.],0.);assert!(s.next_sample(0.1).cancelled);s.event(1,0,[1.,1.],1.);s.event(1,4,[2.,2.],1.1);assert!(s.next_sample(1.2).cancelled);}
}
