//! Mobile movement/look gestures and explicit held Dig / Put down controls.
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU32, Ordering};
use std::{sync::Mutex, time::Instant};

// Ordered pointer samples are consumed once per render/input poll. UIKit never
// invokes Host actions; all panel controls remain in AudioWorkstationSystem.
struct MobilePointerBridge {
 state:crate::workstation::mobile_workstation_input::State,
 clock:Instant,
 dimensions:Option<[f64;2]>,
}
fn mobile_pointer()->&'static Mutex<MobilePointerBridge>{
 static STATE:std::sync::OnceLock<Mutex<MobilePointerBridge>>=std::sync::OnceLock::new();
 STATE.get_or_init(||Mutex::new(MobilePointerBridge{state:Default::default(),clock:Instant::now(),dimensions:None}))
}
#[unsafe(no_mangle)]
pub extern "C" fn ww_mobile_workstation_touch(id:u64,phase:u32,x:f64,y:f64,width:f64,height:f64){
 if let Ok(mut bridge)=mobile_pointer().lock(){
  if ![x,y,width,height].iter().all(|v|v.is_finite())||width<=0.||height<=0.||width>16384.||height>16384.{bridge.state.cancel();return;}
  let dimensions=[width,height];
  if bridge.dimensions.is_some_and(|old|old!=dimensions){bridge.state.cancel();}
  bridge.dimensions=Some(dimensions);let now=bridge.clock.elapsed().as_secs_f64();
  bridge.state.event(id,phase,[x,y],now);
 }
}
fn cancel_mobile_pointer(){if let Ok(mut bridge)=mobile_pointer().lock(){bridge.state.cancel();bridge.dimensions=None;}}
fn mobile_pointer_snapshot()->Value{
 let Ok(mut bridge)=mobile_pointer().lock()else{return json!({"focused":false,"transport_pointer_cancelled":true,"left":false});};
 let now=bridge.clock.elapsed().as_secs_f64();let sample=bridge.state.next_sample(now);
 json!({"focused":!sample.cancelled,"pointer":sample.pointer,"left":sample.left,"right":false,
  "wheel":sample.wheel,"command":sample.command,"alt":false,"shift":false,
  "transport_pointer_events":[],"transport_pointer_cancelled":sample.cancelled,"pointer_cancelled":sample.cancelled,
  "mobile_panel_request":sample.panel.map(|p|p.name()),"mobile_pointer_queued":bridge.state.queued(),"mobile_pointer_overflows":bridge.state.overflows})
}

// One-shot workshop controls; independent from game gestures and cheat state.
static WORKSHOP_ACTIONS: AtomicU32 = AtomicU32::new(0);
static WORKSTATION_PANELS: AtomicU32 = AtomicU32::new(0);
// Workstation gestures stay independent of movement and gameplay pause state.
#[derive(Clone, Copy)]
pub(crate) struct WorkstationGesture { pub start: [f64;2], pub delta: [f64;2], pub touches: u32 }
static WORKSTATION_GESTURES: Mutex<Vec<WorkstationGesture>> = Mutex::new(Vec::new());
static WORKSTATION_CANVAS: AtomicU32 = AtomicU32::new(1.0f32.to_bits());
#[unsafe(no_mangle)]
pub extern "C" fn ww_workstation_gesture(x:f64,y:f64,dx:f64,dy:f64,touches:u32) {
    if ![x,y,dx,dy].iter().all(|v| v.is_finite()) || !(1..=2).contains(&touches) { return; }
    if let Ok(mut gestures) = WORKSTATION_GESTURES.lock() {
        if gestures.len()<16 { gestures.push(WorkstationGesture {start:[x,y],delta:[dx,dy],touches}); }
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn ww_workstation_canvas_scale(scale:f64) {
    if scale.is_finite() { WORKSTATION_CANVAS.store((scale.clamp(0.25,1.) as f32).to_bits(), Ordering::Relaxed); }
}
pub(crate) fn workstation_canvas_scale() -> f64 { f32::from_bits(WORKSTATION_CANVAS.load(Ordering::Relaxed)) as f64 }
pub(crate) fn take_workstation_gestures() -> Vec<WorkstationGesture> {
    WORKSTATION_GESTURES.lock().map(|mut g| std::mem::take(&mut *g)).unwrap_or_default()
}
#[unsafe(no_mangle)]
pub extern "C" fn ww_workstation_panel_action(edge: u32) {
    if edge < 4 {
        WORKSTATION_PANELS.fetch_xor(1 << edge, Ordering::Relaxed);
    }
}
pub(crate) fn take_workstation_panel_actions() -> u32 {
    WORKSTATION_PANELS.swap(0, Ordering::Relaxed)
}
#[unsafe(no_mangle)]
pub extern "C" fn ww_workshop_action(action: u32) {
    if matches!(action, 1 | 2 | 4) {
        WORKSHOP_ACTIONS.fetch_or(action, Ordering::Relaxed);
    }
}
pub(crate) fn take_workshop_actions() -> u32 {
    WORKSHOP_ACTIONS.swap(0, Ordering::Relaxed)
}
#[derive(Clone, Copy, PartialEq)]
enum Role {
    Left,
    Right,
    Pause,
    Jump,
    Descend,
    Pointer,
}
#[derive(Clone, Copy)]
struct Contact {
    id: u64,
    role: Role,
    start: [f64; 2],
    at: [f64; 2],
    began: Instant,
    dragged: bool,
    handled: bool,
    return_candidate: bool,
}
struct TouchState {
    contacts: [Option<Contact>; 10],
    size: [f64; 2],
    look: [f64; 2],
    pointer: [f64; 2],
    tap: bool,
    target: bool,
    hold: f64,
    pause: bool,
    jump: bool,
    ui: bool,
    sprint: bool,
    last_left_tap: Option<Instant>,
    drop: bool,
    tool_equipped: bool,
    work: bool,
    material_tool: u32,
    work_feedback: Option<u32>,
    cheat_flags: u32,
    cheat_sequence: u64,
    cheat_request: Option<Value>,
    time_override_minutes: Option<u16>,
}
static INPUT: Mutex<TouchState> = Mutex::new(TouchState {
    contacts: [None; 10],
    size: [1.; 2],
    look: [0.; 2],
    pointer: [0.; 2],
    tap: false,
    target: false,
    hold: 0.,
    pause: false,
    jump: false,
    ui: false,
    sprint: false,
    last_left_tap: None,
    drop: false,
    tool_equipped: false,
    work: false,
    material_tool: 0,
    work_feedback: None,
    cheat_flags: 0,
    cheat_sequence: 0,
    cheat_request: None,
    time_override_minutes: None,
});
impl TouchState {
    fn clear(&mut self) {
        self.contacts = [None; 10];
        self.look = [0.; 2];
        self.tap = false;
        self.target = false;
        self.hold = 0.;
        self.pause = false;
        self.jump = false;
        self.sprint = false;
        self.last_left_tap = None;
        self.drop = false;
        self.work = false;
        self.work_feedback = None;
    }
    fn point(&mut self, p: [f64; 2]) {
        self.pointer = [
            (2. * p[0] / self.size[0] - 1.).clamp(-1., 1.),
            (1. - 2. * p[1] / self.size[1]).clamp(-1., 1.),
        ];
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn ww_touch_event(id: u64, phase: u32, x: f64, y: f64, width: f64, height: f64) {
    if ![x, y, width, height].iter().all(|v| v.is_finite()) || width <= 0. || height <= 0. {
        return;
    }
    let Ok(mut s) = INPUT.lock() else {
        return;
    };
    if s.size != [width, height] {
        s.clear();
        s.size = [width, height];
    }
    if phase == 0 {
        if s.contacts.iter().flatten().any(|c| c.id == id) {
            return;
        }
        let corner = 80f64.min(width.min(height) * 0.18);
        let mut role = if s.ui {
            Role::Pointer
        } else if x < width * 0.5 {
            Role::Left
        } else {
            Role::Right
        };
        if x > width - corner && y < corner {
            role = Role::Pause;
        }
        if !s.ui && x > width - corner && y > height - corner {
            role = Role::Jump;
        }
        if !s.ui && s.cheat_flags & 2 != 0 && x < corner && y > height - corner {
            role = Role::Descend;
        }
        if let Some(slot) = s.contacts.iter_mut().find(|c| c.is_none()) {
            *slot = Some(Contact {
                id,
                role,
                start: [x, y],
                at: [x, y],
                began: Instant::now(),
                dragged: false,
                handled: false,
                return_candidate: x > width - 28. && y > height * 0.35 && y < height * 0.82,
            });
        }
        s.point([x, y]);
        return;
    }
    let Some(index) = s
        .contacts
        .iter()
        .position(|c| c.is_some_and(|c| c.id == id))
    else {
        return;
    };
    let mut c = s.contacts[index].unwrap();
    let dragged = (x - c.start[0]).hypot(y - c.start[1]) > 12.;
    if phase == 1 {
        if c.role == Role::Right && (c.dragged || dragged) {
            s.look[0] += (x - c.at[0]) * 2.;
            s.look[1] += (y - c.at[1]) * 2.;
        }
        c.dragged |= dragged;
        c.at = [x, y];
        s.contacts[index] = Some(c);
        s.point(c.at);
        return;
    }
    if phase == 2 && !c.dragged && !dragged && !c.handled {
        let elapsed = c.began.elapsed().as_secs_f64();
        match c.role {
            Role::Pause => s.pause = true,
            Role::Jump => s.jump = true,
            Role::Descend => {}
            _ => {
                let moving = s
                    .contacts
                    .iter()
                    .flatten()
                    .any(|v| v.role == Role::Left && v.dragged);
                s.tap = true;
                s.target = !moving;
                s.hold = elapsed;
                s.point([x, y]);
                if c.role == Role::Left && elapsed < 0.3 {
                    let now = Instant::now();
                    if s.last_left_tap
                        .is_some_and(|t| now.duration_since(t).as_secs_f64() < 0.3)
                    {
                        s.sprint = !s.sprint;
                        s.last_left_tap = None;
                        s.tap = false;
                        s.target = false;
                    } else {
                        s.last_left_tap = Some(now);
                    }
                }
            }
        }
    }
    s.contacts[index] = None;
}
/// Gameplay routing supplies context; unequipped camera gestures never become tool gestures.
pub(crate) fn set_tool_equipped(equipped: bool) {
    let mut publish = None;
    if let Ok(mut s) = INPUT.lock() {
        if s.tool_equipped != equipped {
            s.tool_equipped = equipped;
            publish = Some(s.cheat_flags | u32::from(equipped) * 8);
            s.drop = false;
            s.work = false;
        }
    }
    if let Some(flags) = publish {
        unsafe {
            ww_ios_controls_state(flags);
        }
    }
}
/// Explicit UI controls do not share gesture timing with camera look.
#[unsafe(no_mangle)]
pub extern "C" fn ww_touch_work(held: bool) {
    if let Ok(mut s) = INPUT.lock() {
        s.work = held && s.tool_equipped && !s.ui;
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn ww_touch_put_down() {
    if let Ok(mut s) = INPUT.lock() {
        s.work = false;
        s.drop = s.tool_equipped && !s.ui;
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn ww_touch_reset() {
    cancel_mobile_pointer();
    if let Ok(mut s) = INPUT.lock() {
        s.clear();
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn ww_touch_tool(slot: u32) {
    if let Ok(mut s) = INPUT.lock() {
        s.material_tool = slot.min(3);
        s.work = false;
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn ww_touch_ui(active: bool) {
    if let Ok(mut s) = INPUT.lock() {
        if s.ui != active {
            s.clear();
            s.ui = active;
        }
    }
    unsafe {
        ww_ios_pause_state(active);
    }
}
#[cfg(not(test))]
unsafe extern "C" {
    fn ww_ios_controls_state(flags: u32);
    fn ww_ios_pause_state(paused: bool);
    fn ww_ios_dig_feedback(status: u32);
}
#[cfg(test)]
unsafe fn ww_ios_controls_state(_: u32) {}
#[cfg(test)]
unsafe fn ww_ios_pause_state(_: bool) {}
#[cfg(test)]
unsafe fn ww_ios_dig_feedback(_: u32) {}
pub(crate) fn set_work_feedback(status: u32) {
    let changed = if let Ok(mut s) = INPUT.lock() {
        if s.work_feedback == Some(status) {
            false
        } else {
            s.work_feedback = Some(status);
            true
        }
    } else {
        false
    };
    if changed {
        unsafe {
            ww_ios_dig_feedback(status);
        }
    }
}
/// UIKit sends complete desired flags, while the engine owns the accepted state.
#[unsafe(no_mangle)]
pub extern "C" fn ww_touch_cheats(flags: u32) {
    if flags & !7 != 0 {
        return;
    }
    if let Ok(mut s) = INPUT.lock() {
        if flags & 1 == 0 {
            s.time_override_minutes = None;
        }
        s.cheat_sequence = s.cheat_sequence.wrapping_add(1);
        let sequence = s.cheat_sequence;
        s.cheat_request = Some(
            json!({"enabled":flags&1!=0,"flight":flags&2!=0,"noclip":flags&4!=0,"sequence":sequence}),
        );
    }
}
/// A negative value restores the real clock; otherwise select a local minute.
#[unsafe(no_mangle)]
pub extern "C" fn ww_touch_time_override(minutes: i32) {
    if let Ok(mut s) = INPUT.lock() {
        let cheats_enabled = s
            .cheat_request
            .as_ref()
            .map(|r| r["enabled"] == true)
            .unwrap_or(s.cheat_flags & 1 != 0);
        s.time_override_minutes = if cheats_enabled && (0..1440).contains(&minutes) {
            Some(minutes as u16)
        } else {
            None
        };
    }
}
pub(crate) fn time_override_fraction() -> Option<f64> {
    INPUT
        .lock()
        .ok()?
        .time_override_minutes
        .map(|m| f64::from(m) / 1440.)
}
#[unsafe(no_mangle)]
pub extern "C" fn ww_touch_pause_request() {
    if let Ok(mut s) = INPUT.lock() {
        s.pause = true;
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn ww_touch_begin_session() {
    cancel_mobile_pointer();
    WORKSTATION_PANELS.store(0, Ordering::Relaxed);
    WORKSTATION_CANVAS.store(1.0f32.to_bits(), Ordering::Relaxed);
    if let Ok(mut gestures) = WORKSTATION_GESTURES.lock() { gestures.clear(); }
    WORKSHOP_ACTIONS.store(0, Ordering::Relaxed);
    if let Ok(mut s) = INPUT.lock() {
        s.clear();
        s.ui = false;
        s.tool_equipped = false;
        s.cheat_flags = 0;
        s.cheat_request = None;
        s.time_override_minutes = None;
    }
}
pub(crate) fn ack_cheats(sequence: Option<u64>, enabled: bool, flying: bool, noclip: bool) {
    let mut publish = None;
    if let Ok(mut s) = INPUT.lock() {
        if sequence.is_some()
            && s.cheat_request
                .as_ref()
                .and_then(|r| r["sequence"].as_u64())
                == sequence
        {
            s.cheat_request = None;
        }
        let flags = u32::from(enabled) | u32::from(flying) * 2 | u32::from(noclip) * 4;
        if s.cheat_flags != flags {
            s.cheat_flags = flags;
            publish = Some(flags | u32::from(s.tool_equipped) * 8);
        }
    }
    if let Some(flags) = publish {
        unsafe {
            ww_ios_controls_state(flags);
        }
    }
}
pub(crate) fn snapshot() -> Value {
    let Ok(mut s) = INPUT.lock() else {
        return json!({});
    };
    let mut axis = [0.; 2];
    let mut moving = false;
    let mut held = false;
    let mut quit = false;
    let mut jump = s.jump;
    let mut descend = false;
    let contacts = s.contacts;
    for c in contacts.into_iter().flatten() {
        if c.role == Role::Left && c.dragged {
            let radius = (s.size[0].min(s.size[1]) * 0.16).clamp(48., 90.);
            axis = [
                (c.at[0] - c.start[0]) / radius,
                (c.start[1] - c.at[1]) / radius,
            ];
            let length = axis[0].hypot(axis[1]).max(1.);
            axis = axis.map(|x| x / length);
            moving = true;
        }
        if c.role == Role::Jump && !c.dragged && c.began.elapsed().as_secs_f64() >= 0.15 {
            jump = true;
        }
        if c.role == Role::Descend {
            descend = true;
        }
        if s.ui && c.return_candidate && !c.dragged && c.began.elapsed().as_secs_f64() >= 0.5 {
            quit = true;
        }
    }
    if !s.ui && !moving && !s.tap {
        if let Some(c) = contacts.into_iter().flatten().find(|c| {
            matches!(c.role, Role::Left | Role::Right)
                && !c.dragged
                && !c.handled
                && !c.return_candidate
        }) {
            held = true;
            s.target = true;
            s.hold = c.began.elapsed().as_secs_f64();
            s.point(c.at);
        }
    }
    if s.ui {
        axis = [0.; 2];
        s.target = false;
    }
    let tool_input = json!({"held":false,"rotate":false,"drop":!s.ui&&s.drop,
        "delta":[0.,0.],"twist":0.,"reach":0.,"work":!s.ui&&s.tool_equipped&&s.work});
    let value = json!({"workstation":{"editor":mobile_pointer_snapshot()},"tools":{"slot":s.material_tool,"work":!s.ui&&s.work,"pour":!s.ui&&s.drop},"mobile":true,"cheats":s.cheat_request,"axis":axis,"move":[axis[1]>0.1,axis[1]< -0.1,axis[0]< -0.1,axis[0]>0.1,jump,descend],
        "look":s.look.map(|v|v.clamp(-1000.,1000.)),"mode_request":null,"reset":false,"replay":false,"dig":!s.ui&&s.tool_equipped&&s.work,"boost":s.sprint,
        "buttons":[false,false,false,false,false,false,false],"select":s.tap,"pointer":s.pointer,"pause":s.pause,"quit":quit,"step":false,
        "touch":{"active":s.target||s.work,"held":held,"tap":s.tap&&!s.ui,"seconds":s.hold.min(60.),"pointer":s.pointer},"tool_input":tool_input});
    s.look = [0.; 2];
    s.tap = false;
    s.target = false;
    s.hold = 0.;
    s.pause = false;
    s.jump = false;
    s.drop = false;
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    static SESSION: Mutex<()> = Mutex::new(());
    #[test]
    fn workshop_commands_are_one_shot_and_cleared_between_sessions() {
        let _session = SESSION.lock().unwrap();
        ww_touch_begin_session();
        ww_workshop_action(1);
        ww_workshop_action(4);
        ww_workshop_action(8);
        assert_eq!(take_workshop_actions(), 5);
        assert_eq!(take_workshop_actions(), 0);
        ww_workshop_action(2);
        ww_touch_begin_session();
        assert_eq!(take_workshop_actions(), 0);
    }
    #[test]
    fn cheats_request_survives_pause_snapshots_and_only_matching_ack_clears_it() {
        let _session = SESSION.lock().unwrap();
        ww_touch_begin_session();
        ww_touch_ui(true);
        ww_touch_cheats(1);
        let first = snapshot()["cheats"]["sequence"].as_u64().unwrap();
        ww_touch_cheats(7);
        let second = snapshot()["cheats"]["sequence"].as_u64().unwrap();
        assert_ne!(first, second);
        assert_eq!(snapshot()["cheats"]["sequence"], second);
        ww_touch_ui(false);
        ack_cheats(Some(first), true, false, false);
        assert_eq!(snapshot()["cheats"]["sequence"], second);
        ack_cheats(Some(second), true, true, true);
        assert!(snapshot()["cheats"].is_null());
        ww_touch_begin_session();
        assert_eq!(INPUT.lock().unwrap().cheat_flags, 0);
        assert!(snapshot()["cheats"].is_null());
    }
    #[test]
    fn time_override_requires_cheats_and_returns_to_real_time() {
        let _session = SESSION.lock().unwrap();
        ww_touch_begin_session();
        ww_touch_time_override(720);
        assert_eq!(time_override_fraction(), None);
        ww_touch_cheats(1);
        ww_touch_time_override(720);
        assert_eq!(time_override_fraction(), Some(0.5));
        ww_touch_time_override(1080);
        assert_eq!(time_override_fraction(), Some(0.75));
        ww_touch_time_override(-1);
        assert_eq!(time_override_fraction(), None);
        ww_touch_time_override(720);
        ww_touch_cheats(0);
        assert_eq!(time_override_fraction(), None);
        ww_touch_cheats(1);
        ww_touch_time_override(720);
        ww_touch_begin_session();
        assert_eq!(time_override_fraction(), None);
    }
    #[test]
    fn cheats_flight_has_distinct_descend_and_sprint_inputs() {
        let _session = SESSION.lock().unwrap();
        ww_touch_begin_session();
        ww_touch_event(1, 0, 20., 780., 400., 800.);
        assert_eq!(snapshot()["move"][5], false);
        ww_touch_event(1, 3, 20., 780., 400., 800.);
        ack_cheats(None, true, true, false);
        ww_touch_event(2, 0, 20., 780., 400., 800.);
        let s = snapshot();
        assert_eq!(s["move"][5], true);
        assert_eq!(s["boost"], false);
        ww_touch_event(2, 2, 20., 780., 400., 800.);
        let s = snapshot();
        assert_eq!(s["move"][5], false);
        assert_eq!(s["touch"]["tap"], false);
    }
    #[test]
    fn mobile_work_button_is_independent_of_look_and_releases_on_pause() {
        let _session = SESSION.lock().unwrap();
        ww_touch_begin_session();
        set_tool_equipped(true);
        ww_touch_work(true);
        ww_touch_event(91, 0, 280., 200., 400., 800.);
        {
            let mut s = INPUT.lock().unwrap();
            s.contacts
                .iter_mut()
                .flatten()
                .find(|c| c.id == 91)
                .unwrap()
                .began = Instant::now() - std::time::Duration::from_secs(1);
        }
        let held = snapshot();
        assert_eq!(held["tool_input"]["work"], true);
        assert_eq!(held["tool_input"]["held"], false);
        ww_touch_event(91, 1, 320., 200., 400., 800.);
        assert!(snapshot()["look"][0].as_f64().unwrap() > 0.);
        ww_touch_work(false);
        assert_eq!(snapshot()["tool_input"]["work"], false);
        ww_touch_work(true);
        ww_touch_ui(true);
        assert_eq!(snapshot()["tool_input"]["work"], false);
        ww_touch_ui(false);
        ww_touch_put_down();
        assert_eq!(snapshot()["tool_input"]["drop"], true);
        assert_eq!(snapshot()["tool_input"]["drop"], false);
        ww_touch_begin_session();
    }
}
