//! Existing transport controls adapted from Cardinal's ButtonSystem::updateButtonState,
//! DawUiSystem::updateTransportButtonVisuals and DawTransportSystem::UpdateDawTransport.
//! Pointer release owns activation; visual pressure never dispatches an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action { Rewind, Stop, Record, Play }
impl Action {
    pub fn index(self)->usize {match self {Self::Rewind=>0,Self::Stop=>1,Self::Record=>2,Self::Play=>3}}
    fn from_index(i:usize)->Option<Self>{[Self::Rewind,Self::Stop,Self::Record,Self::Play].get(i).copied()}
}
#[derive(Default)]
pub(crate) struct Controls {
    pub pressure:[f32;4],
    captured:Option<usize>,previous_down:bool,pending:Option<Action>,hold:[bool;4],pending_toggle:bool,
    pub latch:Option<Action>,
}
impl Controls {
    pub fn owns_pointer(&self)->bool{self.captured.is_some()}
    pub fn sync_playing(&mut self,playing:bool){
        if playing {if self.latch!=Some(Action::Record){self.latch=Some(Action::Play);}}
        else if matches!(self.latch,Some(Action::Play|Action::Record)){self.latch=Some(Action::Stop);}
    }
    pub fn accepted(&mut self,action:Action){self.latch=match action {Action::Rewind=>None,_=>Some(action)};}
    /// Cardinal's managed controls delay release activation by one update.
    pub fn update(&mut self,down:bool,hit:Option<usize>,blocked:bool,dt:f64)->(Option<Action>,bool){
        self.update_events(down,hit,blocked,dt,&[])
    }
    pub fn update_events(&mut self,down:bool,hit:Option<usize>,blocked:bool,dt:f64,events:&[(bool,Option<usize>)])->(Option<Action>,bool){
        let before=self.pressure;let action=if blocked {self.pending=None;None}else{self.pending.take()};
        if blocked {self.captured=None;self.previous_down=down;} else {
            for &(event_down,event_hit) in events.iter().take(16) {self.pointer(event_down,event_hit);}
            self.pointer(down,hit);
        }
        let step=(0.5/0.15*dt.clamp(0.,1.)) as f32;
        for i in 0..4 {
            let target=if self.latch.is_some_and(|a|a.index()==i)||(down&&self.captured==Some(i))||self.hold[i]{0.5}else{0.};
            self.pressure[i]=if self.pressure[i]<target {(self.pressure[i]+step).min(target)}else{(self.pressure[i]-step).max(target)};
            self.hold[i]=false;
        }
        (action,before!=self.pressure)
    }
    fn pointer(&mut self,down:bool,hit:Option<usize>){
        if down&&!self.previous_down{self.captured=hit.filter(|i|*i<4);}
        if !down&&self.previous_down {
            if let Some(i)=self.captured {if hit==Some(i){self.pending=Action::from_index(i);self.hold[i]=true;}}
            self.captured=None;
        }
        self.previous_down=down;
    }
    pub fn has_pending(&self)->bool{self.pending.is_some()}
    /// A Space received with a newly released button follows that button on the
    /// next update. A keyboard-only request still takes effect immediately.
    pub fn take_toggle(&mut self,requested:bool,cancelled:bool)->bool {
        if cancelled {self.pending_toggle=false;return false;}
        let deferred=std::mem::take(&mut self.pending_toggle);
        self.pending_toggle=requested&&self.pending.is_some();
        deferred||(requested&&!self.pending.is_some())
    }
    #[cfg(target_os="ios")]
    pub fn tap(&mut self,index:usize){if let Some(action)=Action::from_index(index){self.pending=Some(action);self.hold[index]=true;}}
    pub fn cancel_pointer(&mut self){self.captured=None;self.previous_down=false;self.pending=None;self.hold=[false;4];self.pending_toggle=false;}
}
/// Existing audio API adapter: Cardinal Stop holds position; Rewind stops and
/// returns home. Record starts native capture; repeating Record is a no-op.
pub(crate) fn request(action:Action)->Option<serde_json::Value>{
    Some(match action {
        Action::Rewind=>serde_json::json!({"action":"stop"}),
        Action::Stop=>serde_json::json!({"action":"pause"}),
        Action::Play=>serde_json::json!({"action":"play"}),
        Action::Record=>serde_json::json!({"action":"record"}),
    })
}
#[cfg(test)]mod tests {
    use super::*;
    #[test]fn release_inside_activates_once_after_one_update(){let mut c=Controls::default();assert_eq!(c.update(true,Some(3),false,0.075).0,None);assert_eq!(c.pressure[3],0.25);assert_eq!(c.update(false,Some(3),false,0.075).0,None);assert_eq!(c.pressure[3],0.5);assert_eq!(c.update(false,Some(3),false,0.).0,Some(Action::Play));assert_eq!(c.update(false,Some(3),false,0.).0,None);}
    #[test]fn outside_release_or_foreign_drag_does_not_activate(){for (start,end) in [(Some(0),None),(None,Some(0)),(Some(0),Some(1))]{let mut c=Controls::default();c.update(true,start,false,0.1);c.update(false,end,false,0.1);assert_eq!(c.update(false,end,false,0.1).0,None);}}
    #[test]fn hold_and_latch_use_cardinals_150ms_half_pressure(){let mut c=Controls::default();c.update(true,Some(1),false,0.15);assert_eq!(c.pressure[1],0.5);c.update(true,None,false,0.15);assert_eq!(c.pressure[1],0.5);c.update(false,None,false,0.15);assert_eq!(c.pressure[1],0.);c.accepted(Action::Play);c.update(false,None,false,0.15);assert_eq!(c.pressure[3],0.5);c.accepted(Action::Stop);c.update(false,None,false,0.15);assert_eq!(c.pressure,[0.,0.5,0.,0.]);c.accepted(Action::Rewind);c.update(false,None,false,0.15);assert_eq!(c.pressure,[0.;4]);}
    #[test]fn focus_or_editor_capture_cancels_press(){let mut c=Controls::default();c.update(true,Some(3),false,0.1);c.update(false,Some(3),true,0.1);assert_eq!(c.update(false,None,false,0.1).0,None);c.update(true,Some(0),false,0.1);c.cancel_pointer();assert_eq!(c.update(false,Some(0),false,0.1).0,None);}
    #[test]fn audio_actions_preserve_cardinal_stop_rewind_and_non_toggle_play(){assert_eq!(request(Action::Stop).unwrap()["action"],"pause");assert_eq!(request(Action::Rewind).unwrap()["action"],"stop");assert_eq!(request(Action::Play).unwrap()["action"],"play");assert_eq!(request(Action::Record).unwrap()["action"],"record");}
    #[test]fn keyboard_and_external_transport_update_latches_without_actions(){let mut c=Controls::default();c.sync_playing(false);assert_eq!(c.latch,None);c.sync_playing(true);c.update(false,None,false,0.15);assert_eq!(c.pressure[3],0.5);c.sync_playing(false);assert_eq!(c.latch,Some(Action::Stop));assert_eq!(c.update(false,None,false,0.15).0,None);}
}

#[cfg(test)]mod pointer_event_tests {
 use super::*;
 #[test]fn short_complete_click_activates_next_update_once_and_keeps_visual_hold(){
  let mut c=Controls::default();assert_eq!(c.update_events(false,Some(3),false,0.075,&[(true,Some(3)),(false,Some(3))]).0,None);assert_eq!(c.pressure[3],0.25);assert!(c.has_pending());
  assert_eq!(c.update(false,Some(3),false,0.075).0,Some(Action::Play));assert_eq!(c.update(false,Some(3),false,0.075).0,None);
 }
 #[test]fn short_foreign_drag_outside_release_and_cancel_do_not_activate(){
  for events in [[(true,None),(false,Some(3))],[(true,Some(3)),(false,None)],[(true,Some(3)),(false,Some(2))]] {
   let mut c=Controls::default();c.update_events(false,None,false,0.1,&events);assert_eq!(c.update(false,None,false,0.1).0,None);
  }
  let mut c=Controls::default();c.update_events(false,Some(3),true,0.1,&[(true,Some(3)),(false,Some(3))]);assert!(!c.has_pending());
 }
}

#[cfg(test)]mod space_order_tests {
 use super::*;
 #[test]fn same_batch_space_waits_for_new_managed_release(){
  let mut c=Controls::default();
  assert_eq!(c.update_events(false,Some(3),false,0.075,&[(true,Some(3)),(false,Some(3))]).0,None);
  assert!(!c.take_toggle(true,false));
  assert_eq!(c.update(false,None,false,0.075).0,Some(Action::Play));
  assert!(c.take_toggle(false,false));assert!(!c.take_toggle(false,false));
 }
 #[test]fn keyboard_only_is_immediate_and_focus_cancels_deferred_space(){
  let mut c=Controls::default();assert!(c.take_toggle(true,false));
  c.update_events(false,Some(3),false,0.075,&[(true,Some(3)),(false,Some(3))]);
  assert!(!c.take_toggle(true,false));c.update(false,None,true,0.075);
  assert!(!c.take_toggle(false,true));c.update(false,None,false,0.075);
  assert!(!c.take_toggle(false,false));assert!(c.take_toggle(true,false));
 }
}
