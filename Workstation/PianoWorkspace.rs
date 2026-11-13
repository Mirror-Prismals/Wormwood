//! A separate clip-clock instrument room. The arrangement's furniture never
//! participates in the piano roll's projection, drawing or input coordinates.
use super::{
    panel_layout::Viewport,
    piano_roll::{Editor, Input, Rect},
    piano_roll_scene, piano_tuning as tuning,
};
use serde_json::{Value, json};
#[derive(Default)]
pub(crate) struct Workspace {
    pub active: bool,
    pub track: Option<String>,
    snapshot: Value,
    previous: Input,
    pub settings: Option<Value>,
    pub preview: Option<(String, u8, f64)>,
    pub notice: Option<String>,
    pub mix_preview: Value,
    pub spectrum_track: Option<String>,
    pub spectrum_kind: u8,
    pub sound_request: Option<Value>,
    pub pointer_consumed: bool,
    pub bar_actions: Vec<super::arrangement::Action>,
    bars: super::timeline_bars::Bars,
    playhead: Option<f64>,
    rate: f64,
    spectral_cache:std::cell::RefCell<Option<(Value,Value)>>,
}
pub(crate) fn outer(view: Viewport) -> Rect {
    Rect {
        left: 0.,
        top: 0.,
        width: view.width,
        height: view.height,
    }
}
pub(crate) fn field(rect: Rect) -> Rect {
    let key = keyboard_width(rect);
    Rect {
        left: rect.left + key + 18.,
        top: rect.top + toolbar_height(rect) + super::timeline_bars::HEADER_HEIGHT + 18.,
        width: (rect.width - key - 42.).max(0.),
        height: (rect.height - toolbar_height(rect) - super::timeline_bars::HEADER_HEIGHT - 46.).max(0.),
    }
}
fn keyboard_width(rect: Rect) -> f64 {
    (rect.width * 0.14).clamp(88., 156.)
}
pub(crate) const CONTROL_COUNT: usize = 18;
fn columns(rect: Rect)->usize { ((rect.width / 106.).floor() as usize).clamp(4,9) }
pub(crate) fn toolbar_height(rect:Rect)->f64 { (CONTROL_COUNT.div_ceil(columns(rect)) as f64)*90.+8. }
pub(crate) fn control_rect(rect:Rect,i:usize)->Rect {
    let cols=columns(rect);let cell=rect.width/cols as f64;
    Rect{left:rect.left+(i%cols)as f64*cell+(cell-44.)*0.5,top:rect.top+(i/cols)as f64*90.+38.,width:44.,height:44.}
}
fn bar_canvas(rect:Rect)->Rect {let top=rect.top+toolbar_height(rect);Rect{top,height:(rect.height-(top-rect.top)).max(0.),..rect}}
impl Workspace {
    pub fn sync(&mut self, snapshot: &Value, preferred: Option<&str>) {
        self.snapshot = snapshot.clone();
        if self.spectrum_track.as_ref().is_some_and(|id|!snapshot["tracks"].as_array().into_iter().flatten().any(|t|t["id"]==*id&&t["kind"]!="cc")){self.spectrum_track=None;}

        let tracks: Vec<_> = snapshot["tracks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| t["kind"] == "midi")
            .filter_map(|t| t["id"].as_str())
            .collect();
        if !tracks.iter().any(|id| Some(*id) == self.track.as_deref()) {
            self.track = preferred
                .filter(|id| tracks.contains(id))
                .or_else(|| tracks.first().copied())
                .map(str::to_owned);
        }
    }
    pub fn focus_clip(&mut self, snapshot: &Value, id: &str) {
        if let Some(c) = snapshot["clips"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|c| c["id"] == id)
        {
            self.track = c["track"].as_str().map(str::to_owned);
        }
        self.active = true;
        self.sync(snapshot, None);
    }
    fn clips(&self) -> impl Iterator<Item = &Value> {
        self.snapshot["clips"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|c| c["kind"] == "midi" && c["track"].as_str() == self.track.as_deref())
    }
    fn clip(&self, editor: &Editor) -> Option<&Value> {
        let id = editor.status()["clip_id"].clone();
        self.clips().find(|c| c["id"] == id)
    }
    pub fn selected_id(&self, editor: &Editor) -> Option<String> {
        self.clip(editor)?["id"].as_str().map(str::to_owned)
    }
    pub fn edit_rect(&self, _: &Editor, rect: Rect) -> Rect {
        field(rect)
    }
    pub fn update(
        &mut self,
        input: &mut Input,
        rect: Rect,
        editor: &mut Editor,
        dt: f64,
    ) -> Result<bool, String> {
        let old = std::mem::replace(&mut self.previous, input.clone());
        self.preview = None;
        self.pointer_consumed=false;
        let mut changed = false;
        if !editor.is_open() {
            if let Some(id) = self
                .clips()
                .next()
                .and_then(|c| c["id"].as_str())
                .map(str::to_owned)
            {
                editor.open(&self.snapshot, &id)?;
                changed = true;
            }
        }
        if input.command {
            let delta = i32::from(input.arrows[3] && !old.arrows[3])
                - i32::from(input.arrows[2] && !old.arrows[2]);
            if delta != 0 && editor.status()["gesture_active"] != true {
                let tracks: Vec<String> = self.snapshot["tracks"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|t| t["kind"] == "midi")
                    .filter_map(|t| t["id"].as_str().map(str::to_owned))
                    .collect();
                if !tracks.is_empty() {
                    let i = tracks
                        .iter()
                        .position(|t| Some(t) == self.track.as_ref())
                        .unwrap_or(0);
                    let next = (i as i32 + delta).clamp(0, tracks.len() as i32 - 1) as usize;
                    self.track = Some(tracks[next].clone());
                    if let Some(id) = self
                        .clips()
                        .next()
                        .and_then(|c| c["id"].as_str())
                        .map(str::to_owned)
                    {
                        editor.open(&self.snapshot, &id)?;
                    } else {
                        editor.close();
                    }
                    changed = true;
                }
            }
            input.arrows[2] = false;
            input.arrows[3] = false;
        }
        let body = field(rect);
        if editor.status()["gesture_active"]!=true {
            let clock=editor.viewport();let clip=self.clip(editor);
            let bpm=clip.and_then(|c|c["bpm"].as_f64()).unwrap_or(120.);
            let origin=clip.and_then(|c|c["start_seconds"].as_f64()).unwrap_or(0.);
            let mut start=origin+clock.first_quarter*60./bpm;let mut span=clock.quarters_per_width*60./bpm;
            let snap=editor.status()["snap_quarters"].as_f64().map(|q|q*60./bpm);
            let update=self.bars.update(input,input.left&&!old.left,bar_canvas(rect),body,&mut start,&mut span,self.playhead,self.rate.max(48000.),dt,rect.height,|t|snap.map_or(t,|q|origin+((t-origin)/q).round()*q));
            if update.consumed {
                self.pointer_consumed=true;
                editor.set_workspace_time((start-origin)*bpm/60.,span*bpm/60.);
                if let Some(t)=update.seek{self.bar_actions.push(super::arrangement::Action::Seek(t));}
                if update.loop_changed{self.bar_actions.push(super::arrangement::Action::SetLoop(self.bars.range));}
                if let Some(request)=update.rebase{self.bars.rebased(request.shift,request.range);self.bar_actions.push(super::arrangement::Action::Rebase{request,revision:self.snapshot["revision"].as_u64().unwrap_or(0)});}
                *input=Input::default();return Ok(true);
            }
        }
        if let Some(p) = input.pointer {
            if p[1] >= rect.top
                && p[1] < rect.top + toolbar_height(rect)
                && editor.status()["gesture_active"] != true
            {
                let button = (0..CONTROL_COUNT).find(|&i|piano_roll_scene::contains(control_rect(rect,i),p));
                let pressed = input.left && !old.left;
                let wheel = input.wheel[1];
                if let Some(button)=button.filter(|_|pressed || wheel != 0.) {
                    let direction = if input.shift || wheel < 0. { -1 } else { 1 };
                    if (9..14).contains(&button) {
                        editor.arp_control(button-9, direction);
                    } else if button>=14 {
                        match button {
                            14=>{let tracks:Vec<_>=std::iter::once(None).chain(self.snapshot["tracks"].as_array().into_iter().flatten().filter(|t|t["kind"]!="cc").filter_map(|t|t["id"].as_str().map(|id|Some(id.to_owned())))).collect();let i=tracks.iter().position(|t|*t==self.spectrum_track).unwrap_or(0);self.spectrum_track=tracks[(i as i32+direction).rem_euclid(tracks.len()as i32)as usize].clone();},
                            15=>self.spectrum_kind=(self.spectrum_kind as i32+direction).rem_euclid(3)as u8,
                            16=>{if let Some(id)=&self.track{let muted=self.snapshot["tracks"].as_array().into_iter().flatten().find(|t|t["id"]==*id).is_some_and(|t|t["default_sound_muted"]==true);self.sound_request=Some(json!({"id":id,"default_sound_muted":!muted}));}},
                            17=>{self.bars.range.enabled=!self.bars.range.enabled;self.bar_actions.push(super::arrangement::Action::SetLoop(self.bars.range));},
                            _=>{}
                        }
                    } else { match button {
                        0 => {
                            if pressed {
                                self.active = false;
                            }
                        }
                        1 => {
                            input.draw = pressed;
                        }
                        2 => {
                            input.paint = pressed;
                        }
                        3 => {
                            if pressed {
                                input.snap_toggle = true;
                            } else {
                                let current =
                                    editor.status()["snap_quarters"].as_f64().unwrap_or(0.25);
                                let value = (current * if direction > 0 { 2. } else { 0.5 })
                                    .clamp(1. / 64., 4.);
                                editor.set_snap_quarters(Some(value))?;
                            }
                        }
                        _ => {
                            if let Some(c) = self.clip(editor) {
                                let mut bpm = c["bpm"].as_f64().unwrap_or(120.);
                                let mut edo = tuning::edo(&c["edo"])?;
                                let mut n = c["meter"]["numerator"].as_u64().unwrap_or(4);
                                let mut d = c["meter"]["denominator"].as_u64().unwrap_or(4);
                                let mut scale = c["scale"].as_u64().unwrap_or(0) as u8;
                                let mut root = c["root"].as_u64().unwrap_or(0) as u8;
                                match button {
                                    4 => {
                                        bpm = (bpm
                                            + f64::from(direction)
                                                * if input.alt { 0.1 } else { 1. })
                                        .clamp(1., 1000.)
                                    }
                                    5 => {
                                        if input.alt {
                                            let choices = [1, 2, 4, 8, 16, 32];
                                            let i =
                                                choices.iter().position(|x| *x == d).unwrap_or(2);
                                            d = choices
                                                [(i as i32 + direction).rem_euclid(6) as usize];
                                        } else {
                                            n = (n as i32 + direction).clamp(1, 32) as u64;
                                        }
                                    }
                                    6 => {
                                        if wheel != 0. {
                                            edo =
                                                (i32::from(edo) + direction).clamp(1, 4096) as u16;
                                        } else {
                                            let i = tuning::PRESETS
                                                .iter()
                                                .position(|v| *v == edo)
                                                .unwrap_or(2);
                                            edo = tuning::PRESETS
                                                [(i as i32 + direction).rem_euclid(10) as usize];
                                        }
                                    }
                                    7 => {
                                        if edo == 12 {
                                            scale =
                                                (i32::from(scale) + direction).rem_euclid(7) as u8;
                                        }
                                    }
                                    8 => root = (i32::from(root) + direction).rem_euclid(12) as u8,
                                    _ => {}
                                }
                                match editor.settings_request(bpm, edo, n, d, scale, root) {
                                    Ok(request) => {
                                        self.settings = Some(request);
                                        self.notice = None;
                                    }
                                    Err(error) => self.notice = Some(error),
                                }
                            }
                        }
                    } }
                    changed = true;
                }
                input.left = false;
                input.right = false;
                input.wheel = [0.; 2];
                input.pointer = None;
            } else if p[0] < body.left
                && p[1] >= body.top
                && p[1] < body.top + body.height
                && editor.status()["gesture_active"] != true
            {
                if input.left {
                    if let Some(c) = self.clip(editor) {
                        let position = editor.viewport().physical_at(p, body);
                        let black_region = p[0]
                            <= 10.
                                + (keyboard_width(rect) - 12.)
                                    * super::piano_key_layout::BLACK_LENGTH;
                        if let Some(key) = super::piano_key_layout::hit(position, black_region) {
                            self.preview =
                                Some((c["track"].as_str().unwrap_or("").into(), key, 0.8));
                        }
                    }
                }
                input.pitch_ruler = true;
                input.left = false;
                input.right = false;
            } else if !piano_roll_scene::contains(body, p)
                && editor.status()["gesture_active"] != true
            {
                input.left = false;
                input.right = false;
            }
        }
        Ok(changed)
    }
    pub fn presentation(&self, editor: &Editor, rect: Rect, view: Viewport) -> Value {
        if !self.active {
            return Value::Null;
        }
        piano_roll_scene::presentation(editor, field(rect), view)
    }
    pub fn sync_transport(&mut self,t:&Value) {
        self.rate=t["sample_rate"].as_f64().filter(|r|*r>0.).unwrap_or(48000.);
        self.playhead=t["position_frame"].as_f64().map(|f|f/self.rate);
        if !self.bars.active(){if let(Some(a),Some(b))=(t["loop_start_frame"].as_f64(),t["loop_end_frame"].as_f64()){self.bars.range=super::timeline_bars::LoopRange{start:a/self.rate,end:b/self.rate,enabled:t["loop_enabled"]==true};}}
    }
    pub fn spectrum_request(&self)->Value {json!({"track":self.spectrum_track,"kind":(["audio","midi","both"][self.spectrum_kind as usize])})}
    pub fn timeline(&self, editor: &Editor, rect: Rect) -> Value {
        if !self.active{return Value::Null;}
        let body=field(rect);let clock=editor.viewport();let c=self.clip(editor);let bpm=c.and_then(|c|c["bpm"].as_f64()).unwrap_or(120.);let origin=c.and_then(|c|c["start_seconds"].as_f64()).unwrap_or(0.);
        let start=origin+clock.first_quarter*60./bpm;let span=clock.quarters_per_width*60./bpm;
        let g=self.bars.geometry(bar_canvas(rect),body,start,span,self.playhead);let arr=|r:Rect|[r.left,r.top,r.width,r.height];
        json!({"canvas":{"left":rect.left,"top":rect.top,"width":rect.width,"height":rect.height},"bar_body":arr(body),"tracks":[],"clips":[],"scroll_tracks":0,"start_seconds":start,"seconds_per_width":span,"timeline_bars":{"zoom":arr(g.zoom),"loop":arr(g.loop_face),"playhead_handle":g.playhead.map(arr),"loop_start_seconds":self.bars.range.start,"loop_end_seconds":self.bars.range.end,"loop_enabled":self.bars.range.enabled}})
    }
    fn spectral(&self, clip:Option<&Value>, clock:super::piano_roll::Viewport)->Value {
        if self.mix_preview["revision"]!=self.snapshot["revision"] {return Value::Null;}
        let key=json!([self.mix_preview["generation"],self.mix_preview["phase"],clip.map(|c|(&c["id"],&c["start_seconds"],&c["bpm"],&c["edo"])),clock.first_quarter,clock.quarters_per_width,clock.lowest_pitch,clock.pitches_per_height]);
        let mut cache=self.spectral_cache.borrow_mut();
        if let Some((old,value))=cache.as_ref(){if *old==key{return value.clone();}}
        let value=super::mix_preview::project(&self.mix_preview,clip,clock);*cache=Some((key,value.clone()));value
    }
    pub fn scene(&self, editor: &Editor, rect: Rect) -> Value {
        if !self.active {
            return Value::Null;
        }
        let c = self.clip(editor);
        let status = editor.status();
        let body = field(rect);
        let clock = editor.viewport();
        let edo = c.map_or(12, |c| c["edo"].as_u64().unwrap_or(12));
        let scale = c.map_or(0, |c| c["scale"].as_u64().unwrap_or(0));
        let root = c.map_or(0, |c| c["root"].as_u64().unwrap_or(0));
        let bpm = c.map_or(120., |c| c["bpm"].as_f64().unwrap_or(120.));
        let n = c.map_or(4, |c| c["meter"]["numerator"].as_u64().unwrap_or(4));
        let d = c.map_or(4, |c| c["meter"]["denominator"].as_u64().unwrap_or(4));
        let names = [
            "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
        ];
        let snap = status["snap_quarters"].as_f64().map_or("FREE".into(), |s| {
            if s >= 1. {
                format!("SNAP {s}")
            } else {
                format!("SNAP 1/{}", (1. / s).round())
            }
        });
        let mut labels = vec![
            "< TIMELINE".into(),
            "DRAW".into(),
            "PAINT".into(),
            snap,
            format!("{bpm:0.1} BPM"),
            format!("{n}/{d}"),
            format!("{edo} EDO"),
            if edo == 12 {
                tuning::SCALES[scale as usize].into()
            } else {
                "MICROTONE".into()
            },
            format!("ROOT {}", names[root as usize]),
        ];
        labels.extend(["ARP".into(),["UP","DOWN","UP DOWN"][status["arp_direction"].as_u64().unwrap_or(0) as usize].into(),format!("NOTES {}",status["arp_count"]),["TRIAD","MAJ7","DOM7","MIN7","9TH","11TH","13TH"][status["arp_extension"].as_u64().unwrap_or(0) as usize].into(),if status["arp_chord"]==true{"CHORD".into()}else{"ARPEGGIO".into()}]);
        let source=self.spectrum_track.as_ref().and_then(|id|self.snapshot["tracks"].as_array().into_iter().flatten().find(|t|t["id"]==*id)).and_then(|t|t["name"].as_str()).unwrap_or("MIX BUS");
        let muted=self.snapshot["tracks"].as_array().into_iter().flatten().find(|t|t["id"].as_str()==self.track.as_deref()).is_some_and(|t|t["default_sound_muted"]==true);
        labels.extend([source.into(),["AUDIO","MIDI","BOTH"][self.spectrum_kind as usize].into(),if muted{"SILENT".into()}else{"ON".into()},if self.bars.range.enabled{"ON".into()}else{"OFF".into()}]);
        let title = c
            .and_then(|c| c["id"].as_str())
            .unwrap_or("Select a MIDI clip");
        json!({"spectral_grid":self.spectral(c,clock),"active":true,"canvas":[rect.left,rect.top,rect.width,rect.height],"field":[body.left,body.top,body.width,body.height],"keyboard_width":keyboard_width(rect),"lowest_pitch":clock.lowest_pitch,"pitches_per_height":clock.pitches_per_height,"first_quarter":clock.first_quarter,"quarters_per_width":clock.quarters_per_width,"edo":edo,"scale":scale,"root":root,"preview_key":self.preview.as_ref().map(|p|p.1),"arp_labels":["ARP", ((["UP","DOWN","UP DOWN"][status["arp_direction"].as_u64().unwrap_or(0) as usize])), &format!("NOTES {}",status["arp_count"]), ((["TRIAD","MAJ7","DOM7","MIN7","9TH","11TH","13TH"][status["arp_extension"].as_u64().unwrap_or(0) as usize])), if status["arp_chord"]==true {"CHORD"} else {"ARPEGGIO"}],"labels":labels,"control_rects":(0..CONTROL_COUNT).map(|i|{let r=control_rect(rect,i);[r.left,r.top,r.width,r.height]}).collect::<Vec<_>>(),"control_headings":["VIEW","TOOL","TOOL","GRID","TEMPO","METER","TUNING","SCALE","KEY","ARP","DIRECTION","COUNT","HARMONY","PLAY","SPECTRUM SOURCE","SPECTRUM TYPE","DEFAULT SOUND","LOOP"],"title":title,"notice":self.notice.as_deref().map(str::to_owned).or_else(||match self.mix_preview["phase"].as_str(){Some("rendering"|"preparing")=>Some("Updating mix spectrum…".into()),Some("failed")=>Some(format!("Mix preview: {}",self.mix_preview["error"].as_str().unwrap_or("render unavailable"))),_=>None}),"mode":status["mode"],"bpm":bpm,"meter":[n,d]})
    }
    pub fn caption(&self) -> String {
        "Mirror — Piano roll | Each clip has its own clock | Escape: timeline | Tab: drum rack".into()
    }
    pub fn status(&self) -> Value {
        json!({"active":self.active,"track":self.track,"separate_scene":true,"spectrum":self.spectrum_request(),"clips":self.clips().map(|c|c["id"].clone()).collect::<Vec<_>>()})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        json!({"revision":1,"tracks":[{"id":"keys","kind":"midi"}],"clips":[{"id":"solo","track":"keys","kind":"midi","start_seconds":7.,"bpm":93.,"length_quarters":8.,"meter":{"numerator":7,"denominator":8},"notes":[{"id":"n","quarter":1.,"length_quarters":1.,"pitch":61,"velocity":0.8}]}]})
    }
    #[test]
    fn piano_room_uses_full_window_and_local_clock_instead_of_a_tape_lane() {
        let doc = fixture();
        let mut editor = Editor::default();
        editor.open(&doc, "solo").unwrap();
        let mut room = Workspace::default();
        room.focus_clip(&doc, "solo");
        let view = Viewport {
            width: 960.,
            height: 600.,
            canvas_scale: 1.,
        };
        let rect = outer(view);
        let field = room.edit_rect(&editor, rect);
        assert!(field.height > 200.);
        assert!(field.left > 100.);
        assert!(room.timeline(&editor, rect)["timeline_bars"].is_object());
        let scene = room.scene(&editor, rect);
        assert_eq!(scene["bpm"], 93.);
        assert_eq!(scene["meter"], json!([7, 8]));
        assert!(scene["active"].as_bool().unwrap());
        assert_eq!(
            room.presentation(&editor, rect, view)["notes"][0]["pitch"],
            61
        );
    }
    #[test]
    fn edo_control_retunes_the_same_physical_keys_and_preserves_clip_clock() {
        let mut doc = fixture();
        let mut editor = Editor::default();
        editor.open(&doc, "solo").unwrap();
        let mut room = Workspace::default();
        room.focus_clip(&doc, "solo");
        let rect = Rect {
            left: 0.,
            top: 0.,
            width: 960.,
            height: 600.,
        };
        let mut input = Input {
            pointer: Some({let r=control_rect(rect,6);[r.left+10.,r.top+10.]}),
            left: true,
            ..Default::default()
        };
        room.update(&mut input, rect, &mut editor, 0.016).unwrap();
        let request = room.settings.take().unwrap();
        assert_eq!(request["edo"], 17);
        assert_eq!(request["bpm"], 93.);
        assert_eq!(request["start_seconds"], 7.);
        assert_eq!(request["meter"], json!({"numerator":7,"denominator":8}));
        let n = &request["notes"][0];
        let semitones = n["pitch"].as_f64().unwrap() + n["tuning_cents"].as_f64().unwrap() / 100.;
        assert!((semitones - (60. + 12. / 17.)).abs() < 1e-9);
        doc["revision"] = json!(2);
        doc["clips"][0] = request;
        editor.sync(&doc).unwrap();
        assert_eq!(editor.notes()[0].pitch, 61);
        assert!(!input.left);
    }
    #[test]
    fn keyboard_audition_never_creates_a_bubble() {
        let doc = fixture();
        let mut editor = Editor::default();
        editor.open(&doc, "solo").unwrap();
        let mut room = Workspace::default();
        room.focus_clip(&doc, "solo");
        let rect = Rect {
            left: 0.,
            top: 0.,
            width: 960.,
            height: 600.,
        };
        let body = field(rect);
        let p = editor.viewport().project(0., 60., body);
        let mut input = Input {
            pointer: Some([30., p[1]]),
            left: true,
            ..Default::default()
        };
        room.update(&mut input, rect, &mut editor, 0.016).unwrap();
        assert_eq!(room.preview.as_ref().unwrap().1, 60);
        assert!(!input.left);
        assert!(editor.update(&input, body, 0.1).unwrap().is_none());
        assert_eq!(editor.notes().len(), 1);
    }
}

#[cfg(test)] mod redesign_tests {
 use super::*;
 #[test] fn shared_bars_capture_pan_zoom_and_loop_without_note_input(){
  let doc=json!({"revision":1,"tracks":[{"id":"m","name":"Keys","kind":"midi"}],"clips":[{"id":"c","track":"m","kind":"midi","start_seconds":2.,"bpm":120.,"length_quarters":16.,"meter":{"numerator":4,"denominator":4},"notes":[]}]});
  let mut e=Editor::default();e.open(&doc,"c").unwrap();let mut w=Workspace::default();w.focus_clip(&doc,"c");let r=outer(Viewport{width:1280.,height:800.,canvas_scale:1.});
  assert_eq!(w.spectrum_request(),json!({"track":null,"kind":"audio"}));
  let t=w.timeline(&e,r);let z=&t["timeline_bars"]["zoom"];let x=z[0].as_f64().unwrap()+100.;let y=z[1].as_f64().unwrap()+13.;
  let mut press=Input{left:true,pointer:Some([x,y]),..Default::default()};w.update(&mut press,r,&mut e,0.016).unwrap();assert!(w.pointer_consumed);assert!(!press.left);
  let mut drag=Input{left:true,pointer:Some([x+60.,y+20.]),..Default::default()};let before=e.viewport();w.update(&mut drag,r,&mut e,0.016).unwrap();assert!(w.pointer_consumed);assert_ne!(e.viewport().quarters_per_width,before.quarters_per_width);assert!(!drag.left);
  for width in [640.,960.,1920.] {let rect=outer(Viewport{width,height:800.,canvas_scale:1.});for i in 0..CONTROL_COUNT{let b=control_rect(rect,i);assert!(b.left>=0.&&b.left+b.width<=width);assert!(b.top+b.height<field(rect).top);assert!(b.width/b.height<5.);}}
 }
 #[test] fn spectrum_controls_select_tracks_and_types_separately_from_sound(){
  let doc=json!({"revision":1,"tracks":[{"id":"a","name":"Audio","kind":"audio"},{"id":"m","name":"Keys","kind":"midi"}],"clips":[{"id":"c","track":"m","kind":"midi","start_seconds":0.,"bpm":120.,"length_quarters":4.,"meter":{"numerator":4,"denominator":4},"notes":[]}]});let mut e=Editor::default();e.open(&doc,"c").unwrap();let mut w=Workspace::default();w.focus_clip(&doc,"c");let r=outer(Viewport{width:1280.,height:800.,canvas_scale:1.});
  for i in [14,15,16]{let b=control_rect(r,i);let mut input=Input{pointer:Some([b.left+10.,b.top+10.]),left:true,..Default::default()};w.update(&mut input,r,&mut e,0.016).unwrap();w.update(&mut Input::default(),r,&mut e,0.016).unwrap();}
  assert_eq!(w.spectrum_request(),json!({"track":"a","kind":"midi"}));assert_eq!(w.sound_request.unwrap(),json!({"id":"m","default_sound_muted":true}));
 }
}
