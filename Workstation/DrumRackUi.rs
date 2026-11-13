//! Full-screen drum workspace. Bubble steps and gem controls share pointer geometry.
use super::piano_roll::{Input, Rect};
use serde_json::{Value, json};
#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Toggle,
    Title,
    Body,
    Add,
    Preview,
    Tempo(i8),
    Mute(usize),
    Volume(usize),
    Sample(usize),
    Pattern(usize),
    Step(usize, usize),
}
struct Control {
    rect: [f64; 4],
    label: String,
    active: bool,
    hit: Hit,
}
pub(crate) struct Ui {
    pub open: bool,
    canvas: Rect,
    snapshot: Value,
    previous: Input,
    stroke: Option<(usize, usize, bool)>,
    drag: Option<usize>,
    volume: Option<usize>,
    pub notice: Option<String>,
    serial: u64,
    pending_sample: Option<usize>,
}
impl Default for Ui {
    fn default() -> Self {
        Self {
            open: false,
            canvas: Rect {
                left: 0.,
                top: 0.,
                width: 0.,
                height: 0.,
            },
            snapshot: Value::Null,
            previous: Input::default(),
            stroke: None,
            drag: None,
            volume: None,
            notice: None,
            serial: 0,
            pending_sample: None,
        }
    }
}
fn contains(r: [f64; 4], p: [f64; 2]) -> bool {
    p[0] >= r[0] && p[0] < r[0] + r[2] && p[1] >= r[1] && p[1] < r[1] + r[3]
}
impl Ui {
    pub fn cancel(&mut self, retain: bool) {
        self.stroke = None;
        self.drag = None;
        self.volume = None;
        self.pending_sample = None;
        if !retain {
            self.open = false;
        }
        #[cfg(target_os = "macos")]
        unsafe {
            ww_drum_sample_cancel();
        }
    }

    pub fn sync(&mut self, snapshot: &Value) {
        self.snapshot = snapshot["drum_rack"].clone();
    }
    pub fn timeline_rect(&self, r: Rect) -> Rect {
        r
    }
    pub fn place(&mut self, bounds: Rect) {
        self.canvas = bounds;
    }
    fn scales(&self) -> (f64, f64) {
        (self.canvas.width / 760., self.canvas.height / 225.)
    }

    fn controls(&self) -> Vec<Control> {
        let r = self.canvas;
        if !self.open || r.width <= 1. || r.height <= 1. {
            return vec![];
        }
        let (sx, sy) = self.scales();
        let mut out = Vec::new();
        let mut put = |x: f64, y: f64, w: f64, h: f64, label: String, active: bool, hit: Hit| {
            let height=if matches!(hit,Hit::Body|Hit::Step(..)) {h*sy}else{(h*sx).min(42.)};
            out.push(Control {
                rect: [r.left + x * sx, r.top + y * sy+(h*sy-height)*0.5, w * sx, height],
                label,
                active,
                hit,
            });
        };
        put(4., 3., 90., 25., "TIMELINE".into(), false, Hit::Toggle);
        put(402., 3., 350., 25., "DRUMRACK".into(), false, Hit::Title);
        put(102., 3., 85., 25., "+ ROW".into(), false, Hit::Add);
        put(
            194.,
            3.,
            85.,
            25.,
            if self.snapshot["preview"] == true {
                "STOP"
            } else {
                "PLAY"
            }
            .into(),
            self.snapshot["preview"] == true,
            Hit::Preview,
        );
        put(286., 3., 50., 25., "BPM -".into(), false, Hit::Tempo(-1));
        put(343., 3., 50., 25., "BPM +".into(), false, Hit::Tempo(1));
        for (ch, c) in self.snapshot["channels"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
        {
            let y = 40. + ch as f64 * 42.;
            put(
                4.,
                y,
                100.,
                29.,
                c["name"]
                    .as_str()
                    .unwrap_or("SAMPLE")
                    .chars()
                    .filter(|c| c.is_ascii())
                    .take(8)
                    .collect(),
                c["source"].is_string(),
                Hit::Sample(ch),
            );
            put(
                111.,
                y,
                30.,
                29.,
                "M".into(),
                c["mute"] == true,
                Hit::Mute(ch),
            );
            put(
                148.,
                y,
                55.,
                29.,
                format!("{}%", (c["volume"].as_f64().unwrap_or(1.) * 100.).round()),
                false,
                Hit::Volume(ch),
            );
            put(
                210.,
                y,
                36.,
                29.,
                "::".into(),
                self.drag == Some(ch),
                Hit::Pattern(ch),
            );
            let mask = c["steps"].as_u64().unwrap_or(0);
            for step in 0..16 {
                let active = mask & (1 << step) != 0;
                let current = self.snapshot["preview"] == true
                    && self.snapshot["preview_step"].as_u64() == Some(step as u64);
                put(
                    253. + step as f64 * 31.,
                    y,
                    26.,
                    29.,
                    if current {
                        ">".into()
                    } else if active {
                        "+".into()
                    } else {
                        format!("{}", step + 1)
                    },
                    active || current,
                    Hit::Step(ch, step),
                );
            }
        }
        // Transparent field owns input without producing a panel image.
        put(0., 0., 760., 225., String::new(), false, Hit::Body);
        out
    }
    pub fn packet(&self) -> Value {
        json!(
            self.controls()
                .into_iter()
                .filter(|c| !matches!(c.hit, Hit::Body | Hit::Step(..)))
                .map(|c| json!({"rect":c.rect,"label":c.label,"active":c.active,"popup":false,"background":false}))
                .collect::<Vec<_>>()
        )
    }
    pub fn bubbles(&self, view: super::panel_layout::Viewport) -> Value {
        if !self.open || self.canvas.width <= 1. || self.canvas.height <= 1. { return Value::Null; }
        let mpp = 1.3 / view.height;
        let world = |x: f64, y: f64| [(x-view.width*0.5)*mpp,(view.height*0.5-y)*mpp];
        let lo=world(self.canvas.left,self.canvas.top+self.canvas.height);
        let hi=world(self.canvas.left+self.canvas.width,self.canvas.top);
        let notes: Vec<Value> = self.controls().into_iter().filter_map(|c| {
            let Hit::Step(ch,step)=c.hit else {return None;};
            // Empty steps keep their input targets; only enabled steps draw small bubbles.
            let enabled=self.snapshot["channels"][ch]["steps"].as_u64().unwrap_or(0)&(1<<step)!=0;
            if !enabled { return None; }
            let width=c.rect[2]*0.38;
            let height=(c.rect[3]*0.65).min(c.rect[2]*0.65);
            let xy=world(c.rect[0]+(c.rect[2]-width)*0.5,c.rect[1]+c.rect[3]*0.5);
            Some(json!({"id":format!("drum-{}-{step}",self.snapshot["channels"][ch]["track"].as_str().unwrap_or("row")),
                "pitch":ch,"x":xy[0],"y":xy[1],"length":width*mpp,"height_m":height*mpp,
                "age":2.,"selected":c.active}))
        }).collect();
        json!({"height_m":0.018,"notes":notes,"clip_rect":[lo[0],lo[1],hi[0],hi[1]]})
    }
    pub fn timeline(&self) -> Value {
        let r=self.canvas;
        json!({"canvas":[r.left,r.top,r.width,r.height],"seconds_per_width":4.,"start_seconds":0.,
            "scroll_tracks":0,"tracks":[],"clips":[],"rack_controls":self.packet()})
    }
    pub fn caption(&self) -> Option<String> {
        self.open.then(||format!("Drum rack | {} BPM | sample name: load WAV | left: draw | right: erase | drag :: then Tab to drop on linked MIDI lane | Tab: timeline{}",self.snapshot["bpm"],self.notice.as_ref().map(|s|format!(" | {s}")).unwrap_or_default()))
    }
    pub fn update_events(&mut self,input:&Input,canvas:Rect,blocked:bool,events:&Value)->(bool,Vec<Value>,Option<(usize,[f64;2])>){
        let mut owned=false;let mut commands=Vec::new();let mut drop=None;
        for event in events.as_array().into_iter().flatten().take(16){
            let Some(point)=event["pointer"].as_array().and_then(|p|Some([p.first()?.as_f64()?,p.get(1)?.as_f64()?]))else{continue;};
            let mut edge=input.clone();edge.pointer=Some(point);edge.left=event["left"]==true;
            let(o,c,d)=self.update(&edge,canvas,blocked);owned|=o;commands.extend(c);if d.is_some(){drop=d;}
        }
        let(o,c,d)=self.update(input,canvas,blocked);owned|=o;commands.extend(c);if d.is_some(){drop=d;}
        (owned,commands,drop)
    }
    pub fn update(
        &mut self,
        input: &Input,
        canvas: Rect,
        blocked: bool,
    ) -> (bool, Vec<Value>, Option<(usize, [f64; 2])>) {
        self.place(canvas);
        let old = std::mem::replace(&mut self.previous, input.clone());
        if input.escape && !blocked && self.open { self.cancel(false); return (true, vec![], None); }
        // A row can be carried through Tab into the timeline without losing its drag.
        if !self.open && !blocked && self.drag.is_some() && !input.left && old.left {
            let drop=self.drag.take().zip(input.pointer);
            return (drop.is_some(), vec![], drop);
        }
        if !self.open && self.drag.is_some() && input.left && !blocked {
            return (true, vec![], None);
        }
        if blocked || !self.open || input.pointer.is_none() {
            self.stroke = None;
            self.drag = None;
            self.volume = None;
            return (false, vec![], None);
        }
        let p = input.pointer.unwrap();
        let pressed = input.left && !old.left;
        let right = input.right && !old.right;
        let canvas = self.canvas;
        let hit = self.controls().into_iter().find(|c| contains(c.rect, p));
        let captured = self.stroke.is_some() || self.drag.is_some() || self.volume.is_some();
        let mut commands = Vec::new();
        let drop = None;
        if pressed || right {
            if let Some(c) = &hit {
                match c.hit{
   Hit::Body=>{},
   Hit::Title=>{},
   Hit::Toggle=>{self.cancel(false);},Hit::Add=>{self.serial+=1;commands.push(json!({"action":"add","track":format!("drum-{}-{}",std::process::id(),self.serial),"name":format!("Channel {}",self.snapshot["channels"].as_array().map_or(0,Vec::len)+1)}));},
   Hit::Preview=>commands.push(json!({"action":"preview","enabled":self.snapshot["preview"]!=true})),
   Hit::Tempo(d)=>commands.push(json!({"action":"tempo","bpm":(self.snapshot["bpm"].as_f64().unwrap_or(120.)+d as f64).clamp(1.,1000.)})),
   Hit::Mute(ch)=>commands.push(json!({"action":"mix","channel":ch,"mute":self.snapshot["channels"][ch]["mute"]!=true})),
   Hit::Volume(ch)=>self.volume=Some(ch),Hit::Pattern(ch)=>self.drag=Some(ch),
   Hit::Sample(ch)=>{match chooser_open(){Ok(())=>self.pending_sample=Some(ch),Err(e)=>self.notice=Some(e)}},
   Hit::Step(ch,step)=>self.stroke=Some((ch,step,!right)),
  }
            }
        }
        if let Some((ch, previous, enable)) = self.stroke {
            let (sx,sy) = self.scales();
            let y = canvas.top + (40. + ch as f64 * 42.) * sy;
            if p[1] >= y && p[1] < y + 29. * sy {
                let step = (((p[0] - canvas.left) / sx - 253.) / 31.)
                    .floor()
                    .clamp(0., 15.) as usize;
                let mut mask = self.snapshot["channels"][ch]["steps"].as_u64().unwrap_or(0);
                for i in previous.min(step)..=previous.max(step) {
                    if enable {
                        mask |= 1 << i
                    } else {
                        mask &= !(1 << i)
                    }
                }
                if mask != self.snapshot["channels"][ch]["steps"].as_u64().unwrap_or(0) {
                    commands.push(json!({"action":"steps","channel":ch,"steps":mask}));
                    self.snapshot["channels"][ch]["steps"]=json!(mask);
                }
                self.stroke = Some((ch, step, enable));
            }
            if (enable && !input.left) || (!enable && !input.right) {
                self.stroke = None;
            }
        }
        if let Some(ch) = self.volume {
            if let Some(c) = self.controls().iter().find(|c| c.hit == Hit::Volume(ch)) {
                commands.push(json!({"action":"mix","channel":ch,"volume":((p[0]-c.rect[0])/c.rect[2]).clamp(0.,1.)}));
            }
            if !input.left {
                self.volume = None;
            }
        }
        // Releasing inside this workspace must never place a pattern onto
        // the hidden arrangement. Carry it through Tab before dropping.
        if !input.left && old.left { self.drag = None; }
        if let Some(ch) = self.pending_sample {
            match chooser_poll() {
                Ok(Some(path)) => {
                    self.pending_sample = None;
                    self.serial += 1;
                    commands.push(json!({"action":"import","channel":ch,"path":path,"source":format!("drum-sample-{}-{}",std::process::id(),self.serial)}));
                }
                Err(e) => {
                    self.pending_sample = None;
                    self.notice = Some(e)
                }
                _ => {}
            }
        }
        (captured || hit.is_some(), commands, drop)
    }
}
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn ww_drum_sample_cancel();
    fn ww_drum_sample_open() -> i32;
    fn ww_drum_sample_poll(path: *mut std::ffi::c_char, capacity: usize) -> i32;
}
fn chooser_open() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if unsafe { ww_drum_sample_open() } == 0 {
            return Ok(());
        }
        return Err("sample chooser is already open".into());
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Load a WAV through the DrumRack import command on this platform".into())
    }
}
fn chooser_poll() -> Result<Option<String>, String> {
    #[cfg(target_os = "macos")]
    {
        let mut b = vec![0u8; 4096];
        match unsafe { ww_drum_sample_poll(b.as_mut_ptr().cast(), b.len()) } {
            1 => {
                let n = b.iter().position(|v| *v == 0).unwrap_or(b.len());
                return String::from_utf8(b[..n].to_vec())
                    .map(Some)
                    .map_err(|e| e.to_string());
            }
            -1 => return Err("Sample selection cancelled".into()),
            _ => return Ok(None),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(None)
    }
}

#[cfg(test)]
mod drum_rack_ui_tests {
    use super::*;
    fn fixture() -> Ui {
        let mut ui = Ui::default();
        ui.open = true;
        ui.canvas = Rect {
            left: 0.,
            top: 0.,
            width: 760.,
            height: 225.,
        };
        ui.sync(&json!({"drum_rack":{"bpm":120.,"preview":false,"channels":[{"name":"Kick","steps":0,"mute":false,"volume":0.82}]}}));
        ui
    }
    #[test]
    fn closed_workspace_has_no_controls_and_preserves_timeline_bounds() {
        let mut ui=Ui::default();
        let r=Rect {left:10.,top:20.,width:900.,height:600.};
        ui.update(&Input::default(),r,false);
        assert!(ui.packet().as_array().unwrap().is_empty());
        ui.open=true;
        let t=ui.timeline_rect(r);
        assert_eq!([t.left,t.top,t.width,t.height],[r.left,r.top,r.width,r.height]);
    }
    #[test]
    fn workspace_stays_fullscreen_and_escape_closes_without_document_edit() {
        let mut ui=fixture();let bounds=ui.canvas;
        ui.update(&Input {pointer:Some([500.,10.]),left:true,..Default::default()},bounds,false);
        ui.update(&Input {pointer:Some([500.,100.]),left:true,..Default::default()},bounds,false);
        assert_eq!(ui.canvas.top,0.);
        let (owned,commands,drop)=ui.update(&Input {escape:true,..Default::default()},bounds,false);
        assert!(owned);assert!(!ui.open);assert!(commands.is_empty());assert!(drop.is_none());
    }
    #[test]
    fn draw_and_erase_sweeps_include_release_position() {
        let mut ui = fixture();
        let canvas = ui.canvas;
        let mut input = Input {
            pointer: Some([260., 50.]),
            left: true,
            ..Default::default()
        };
        let (owned, commands, _) = ui.update(&input, canvas, false);
        assert!(owned);
        assert_eq!(commands[0]["steps"], 1);
        ui.snapshot["channels"][0]["steps"] = json!(1);
        input.pointer = Some([350., 50.]);
        input.left = false;
        let (_, commands, _) = ui.update(&input, canvas, false);
        assert_eq!(commands[0]["steps"], 15);
        ui.snapshot["channels"][0]["steps"] = json!(15);
        input.right = true;
        let (_, commands, _) = ui.update(&input, canvas, false);
        assert_eq!(commands[0]["steps"], 7);
        ui.snapshot["channels"][0]["steps"] = json!(7);
        input.right = false;
        input.pointer = Some([260., 50.]);
        let (_, commands, _) = ui.update(&input, canvas, false);
        assert_eq!(commands[0]["steps"], 0);
    }
    #[test]
    fn cancelled_pattern_drag_cannot_drop_after_focus_returns() {
        let mut ui = fixture();
        let canvas = ui.canvas;
        let mut input = Input {
            pointer: Some([220., 50.]),
            left: true,
            ..Default::default()
        };
        ui.update(&input, canvas, false);
        assert_eq!(ui.drag, Some(0));
        ui.update(&input, canvas, true);
        input.left = false;
        input.pointer = Some([500., 300.]);
        let (_, _, drop) = ui.update(&input, canvas, false);
        assert!(drop.is_none());
    }
    #[test]
    fn rack_geometry_uses_same_rectangles_as_pointer_targets() {
        let mut ui = fixture();
        let packet = ui.packet();
        assert_eq!(packet.as_array().unwrap().len(), 10);
        let c = ui
            .controls()
            .into_iter()
            .find(|c| c.hit == Hit::Step(0, 15))
            .unwrap();
        let input = Input {
            pointer: Some([c.rect[0] + c.rect[2] / 2., c.rect[1] + c.rect[3] / 2.]),
            left: true,
            ..Default::default()
        };
        let (_, commands, _) = ui.update(&input, ui.canvas, false);
        assert_eq!(commands[0]["steps"], 32768);
    }
}

#[cfg(test)] mod click_regressions {
 use super::*;
 #[test] fn buffered_click_draws_and_erases_a_drum_step(){
  let mut ui=Ui::default();ui.open=true;ui.sync(&json!({"drum_rack":{"bpm":120,"channels":[{"name":"Kick","steps":0}]}}));
  let bounds=Rect{left:0.,top:0.,width:900.,height:500.};ui.update(&Input::default(),bounds,false);
  let r=ui.controls().into_iter().find(|c|c.hit==Hit::Step(0,3)).unwrap().rect;
  let p=[r[0]+r[2]/2.,r[1]+r[3]/2.];
  let events=json!([{"pointer":p,"left":true},{"pointer":p,"left":false}]);
  let (owned,commands,_)=ui.update_events(&Input{pointer:Some(p),..Default::default()},bounds,false,&events);
  assert!(owned);assert_eq!(commands.len(),1);assert_eq!(commands[0]["steps"],8);
  let (_,commands,_)=ui.update(&Input{pointer:Some(p),right:true,..Default::default()},bounds,false);
  assert_eq!(commands[0]["steps"],0);
 }
}

#[cfg(test)] mod fullscreen_bubble_tests {
 use super::*;
 #[test] fn resize_fills_the_view_and_bubbles_share_step_targets() {
  let mut ui=Ui::default();ui.open=true;
  ui.sync(&json!({"drum_rack":{"channels":[{"track":"kick","steps":8}]}}));
  let view=super::super::panel_layout::Viewport{width:1280.,height:720.,canvas_scale:1.};
  let canvas=super::super::piano_workspace::outer(view);
  ui.update(&Input::default(),canvas,false);
  assert_eq!([ui.canvas.left,ui.canvas.top,ui.canvas.width,ui.canvas.height],[0.,0.,1280.,720.]);
  let packet=ui.packet();assert_eq!(packet.as_array().unwrap().len(),10);
  assert!(packet.as_array().unwrap().iter().all(|c|c["background"]==false));
  let bubbles=ui.bubbles(view);let notes=bubbles["notes"].as_array().unwrap();assert_eq!(notes.len(),1);
  assert_eq!(notes[0]["id"],"drum-kick-3");assert_eq!(notes[0]["selected"],true);
  let active=ui.controls().into_iter().find(|c|c.hit==Hit::Step(0,3)).unwrap().rect;
  assert!((notes[0]["length"].as_f64().unwrap()-active[2]*0.38*1.3/view.height).abs()<1e-10);
  ui.snapshot["preview"]=json!(true);ui.snapshot["preview_step"]=json!(2);
  assert_eq!(ui.bubbles(view)["notes"].as_array().unwrap().len(),1);
  let r=ui.controls().into_iter().find(|c|c.hit==Hit::Step(0,5)).unwrap().rect;
  let (_,commands,_)=ui.update(&Input{pointer:Some([r[0]+r[2]/2.,r[1]+r[3]/2.]),left:true,..Default::default()},canvas,false);
  assert_eq!(commands[0]["steps"],40);
 }
}

#[cfg(test)] mod pattern_workspace_tests {
 use super::*;
 #[test] fn drum_rack_pattern_only_drops_after_leaving_workspace() {
  let mut ui=Ui::default();ui.open=true;
  ui.sync(&json!({"drum_rack":{"channels":[{"track":"kick","steps":1}]}}));
  let rect=Rect{left:0.,top:0.,width:760.,height:225.};
  let down=Input{pointer:Some([220.,50.]),left:true,..Default::default()};
  let up=Input{pointer:Some([500.,50.]),..Default::default()};
  ui.update(&down,rect,false);
  assert!(ui.update(&up,rect,false).2.is_none());
  ui.update(&down,rect,false);ui.open=false;
  assert_eq!(ui.update(&up,rect,false).2,Some((0,[500.,50.])));
 }
}
