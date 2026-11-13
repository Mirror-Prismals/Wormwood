//! Small, bounded input buffers. No native calls, rendering, or audio work.
//! Virtual release samples preserve the existing rising-edge panel controller.
#[derive(Default)]
pub(crate) struct BufferedKeys {
    pending: [u8; 5],
    reported: [bool; 5],
    quit: bool,
}
impl BufferedKeys {
    pub(crate) fn record(&mut self, key: i32, action: i32, mods: i32, macos: bool) {
        if action != 1 { return; } // GLFW_PRESS; repeats/releases do not toggle.
        if let Some(index) = [265, 264, 263, 262, 256].iter().position(|k| *k == key) {
            self.pending[index] = self.pending[index].saturating_add(1).min(8);
        }
        if macos && key == 81 && mods & 0x0008 != 0 { self.quit = true; }
    }
    pub(crate) fn blur(&mut self) {
        self.pending = [0; 5];
        self.reported = [false; 5];
        // An explicit quit received before focus loss must still be honored.
    }
    fn sample(&mut self, index: usize, held: bool) -> (bool, bool) {
        let previous = self.reported[index];
        let value = if self.pending[index] > 0 {
            if previous { false } else { self.pending[index] -= 1; true }
        } else { held };
        self.reported[index] = value;
        (value, value && !previous)
    }
    pub(crate) fn panels(&mut self, held: [bool; 4]) -> [bool; 4] {
        std::array::from_fn(|i| self.sample(i, held[i]).0)
    }
    pub(crate) fn escape(&mut self, held: bool) -> (bool, bool) { self.sample(4, held) }
    pub(crate) fn take_quit(&mut self) -> bool { std::mem::take(&mut self.quit) }
}
/// Leave pending redraw state intact while the workstation handles a command.
pub(crate) fn input_preempts_draw(workstation: bool, quit: bool, escape_pressed: bool) -> bool {
    workstation && (quit || escape_pressed)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn short_arrow_press_and_release_survives_a_stall() {
        for (index, key) in [265,264,263,262].into_iter().enumerate() {
            let mut b=BufferedKeys::default(); b.record(key,1,0,true); b.record(key,0,0,true);
            let state=b.panels([false;4]); assert!(state[index]); assert_eq!(state.iter().filter(|v| **v).count(),1);
            assert_eq!(b.panels([false;4]),[false;4]);
        }
    }
    #[test] fn held_arrow_and_os_repeat_toggle_only_once() {
        let mut b=BufferedKeys::default();b.record(265,1,0,true);let mut previous=false;let mut toggles=0;
        for _ in 0..20 { b.record(265,2,0,true);let value=b.panels([true,false,false,false])[0];toggles+=usize::from(value&&!previous);previous=value; }
        assert_eq!(toggles,1);assert!(!b.panels([false;4])[0]);
    }
    #[test] fn repeated_short_taps_have_separate_rising_edges() {
        let mut b=BufferedKeys::default();for _ in 0..3 {b.record(263,1,0,true);b.record(263,0,0,true);}
        let states:Vec<_>=(0..6).map(|_|b.panels([false;4])[2]).collect();assert_eq!(states,vec![true,false,true,false,true,false]);
    }
    #[test] fn escape_press_is_delivered_once_even_after_release() {
        let mut b=BufferedKeys::default();b.record(256,1,0,true);b.record(256,0,0,true);
        assert_eq!(b.escape(false),(true,true));assert_eq!(b.escape(false),(false,false));
        b.record(256,1,0,true);assert_eq!(b.escape(true),(true,true));assert_eq!(b.escape(true),(true,false));
    }
    #[test] fn command_q_survives_release_and_focus_loss_without_affecting_other_keys() {
        let mut b=BufferedKeys::default();b.record(265,1,0,true);b.record(81,1,8,true);b.record(81,0,0,true);b.blur();
        assert!(b.take_quit());assert!(!b.take_quit());assert_eq!(b.panels([false;4]),[false;4]);
        b.record(81,1,0,true);b.record(81,2,8,true);b.record(81,1,8,false);assert!(!b.take_quit());
    }
    #[test] fn pending_input_is_bounded_and_focus_loss_discards_panel_actions() {
        let mut b=BufferedKeys::default();for _ in 0..100 {b.record(262,1,0,true);}
        assert_eq!((0..20).filter(|_|b.panels([false;4])[3]).count(),8);
        b.record(262,1,0,true);b.blur();assert_eq!(b.panels([false;4]),[false;4]);
    }
    #[test] fn quit_or_new_escape_skips_workstation_draw_without_changing_game_policy() {
        for quit in [false,true] {for escape in [false,true] {
            let mut draws=0;if !input_preempts_draw(true,quit,escape) {draws+=1;}
            assert_eq!(draws,usize::from(!quit&&!escape));assert!(!input_preempts_draw(false,quit,escape));
        }}
    }
}
