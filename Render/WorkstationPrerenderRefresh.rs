//! A retained moving image owes one fresh draw even if scene updates stop.
//! GPU submission, not merely preparing that draw, clears the debt.
#[derive(Default)]
pub(super) struct Refresh { dirty:bool, new_packet:bool }
impl Refresh {
 pub fn observe(&mut self,dirty:bool){self.dirty=dirty;self.new_packet=true;}
 pub fn needs_redraw(&self)->bool{self.dirty}
 pub fn idle_due(&self)->bool{self.dirty&&!self.new_packet}
 pub fn drawn(&mut self){self.new_packet=false;}
 pub fn submitted(&mut self){self.dirty=false;}
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn continuous_motion_retains_then_refreshes_without_new_input(){
  let mut r=Refresh::default();
  for _ in 0..12 {r.observe(true);assert!(!r.idle_due());r.drawn();assert!(r.needs_redraw());}
  assert!(r.idle_due());r.observe(true);r.submitted();r.drawn();assert!(!r.needs_redraw());assert!(!r.idle_due());
 }
 #[test] fn stable_new_packet_can_bake_immediately(){let mut r=Refresh::default();r.observe(true);assert!(!r.idle_due());r.submitted();r.drawn();assert!(!r.needs_redraw());}
 #[test] fn failed_or_suspended_draw_does_not_clear_refresh(){let mut r=Refresh::default();r.observe(true);r.drawn();assert!(r.idle_due());assert!(r.idle_due());r.observe(true);r.drawn();assert!(r.idle_due());}
 #[test] fn unchanged_and_unsupported_inputs_do_not_force_redraw(){let mut r=Refresh::default();r.observe(true);r.drawn();r.observe(false);r.drawn();assert!(!r.needs_redraw());assert!(!r.idle_due());}
}
