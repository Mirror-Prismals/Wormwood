//! Hands-on lessons observe committed document edits on the control thread.
//! This UIOnly overlay never edits a project or requests optical work.
use super::{panel_layout::Viewport, piano_roll::Input};
use serde_json::{Value, json};
use std::collections::BTreeSet;
type Rect = [f64; 4];
#[derive(Clone, Copy, Debug, PartialEq)]
enum Lesson { Audio, Midi }
#[derive(Clone, Copy, Debug, PartialEq)]
enum Action { Audio, Midi, Next, Card }
#[derive(Default)]
pub(crate) struct Tutorial {
    pub open: bool,
    lesson: Option<Lesson>,
    step: usize,
    ready: bool,
    track: Option<String>,
    clip: Option<String>,
    initial_tracks: BTreeSet<String>,
    baseline: Value,
    previous_down: bool,
    captured: Option<Action>,
    playback_start: Option<f64>,
    missing: bool,
}
fn items<'a>(doc: &'a Value, key: &str) -> impl Iterator<Item=&'a Value> {
    doc[key].as_array().into_iter().flatten()
}
fn find<'a>(doc: &'a Value, key: &str, id: &Option<String>) -> Option<&'a Value> {
    items(doc,key).find(|v| v["id"].as_str()==id.as_deref() && id.is_some())
}
fn inside(r: Rect, p: [f64;2]) -> bool {
    p.iter().all(|v|v.is_finite()) && p[0]>=r[0] && p[1]>=r[1] && p[0]<r[0]+r[2] && p[1]<r[1]+r[3]
}
impl Tutorial {
    fn last(&self)->usize {if self.lesson==Some(Lesson::Midi){7}else{6}}
    fn start(&mut self, lesson: Lesson, doc: &Value) {
        self.lesson=Some(lesson); self.step=0; self.ready=false; self.track=None; self.clip=None;
        self.initial_tracks=items(doc,"tracks").filter_map(|v|v["id"].as_str().map(str::to_owned)).collect();
        self.baseline=Value::Null; self.playback_start=None; self.missing=false;
    }
    pub fn status(&self)->Value {
        json!({"open":self.open,"lesson":self.lesson.map(|l|if l==Lesson::Audio{"audio"}else{"midi"}),
            "step":self.step,"ready":self.ready&&!self.missing,"track":self.track,"clip":self.clip,
            "target_missing":self.missing,"completed":self.lesson.is_some()&&self.step==self.last()&&!self.missing})
    }
    pub fn cancel_pointer(&mut self){self.captured=None;self.previous_down=false;}
    /// Observe only authoritative snapshots, never local previews. Targets are
    /// new lesson tracks; unrelated/pre-existing content cannot satisfy a step.
    pub fn observe(&mut self, doc:&Value, editor:&Value, transport:&Value) {
        let Some(lesson)=self.lesson else{return;};
        if self.step==0 && self.track.is_none() {
            let kind=if lesson==Lesson::Audio{"audio"}else{"midi"};
            self.track=items(doc,"tracks").find(|t|t["kind"]==kind && t["id"].as_str().is_some_and(|id|!self.initial_tracks.contains(id)))
                .and_then(|t|t["id"].as_str()).map(str::to_owned);
        }
        if self.step==1 && self.clip.is_none() {
            self.clip=items(doc,"clips").find(|c|c["track"].as_str()==self.track.as_deref() && self.track.is_some()
                && c["kind"]==if lesson==Lesson::Audio{"audio"}else{"midi"})
                .and_then(|c|c["id"].as_str()).map(str::to_owned);
        }
        let track=find(doc,"tracks",&self.track);
        let clip=find(doc,"clips",&self.clip);
        self.missing=(self.step>0 && track.is_none()) || (self.step>1 && (clip.is_none() || clip.is_some_and(|c|c["track"].as_str()!=self.track.as_deref())));
        if self.missing {self.ready=false;return;}
        let play_step=if lesson==Lesson::Audio{4}else{5};
        let done=match self.step {
            0=>track.is_some(),
            1=>clip.is_some(),
            2 if lesson==Lesson::Audio=>clip.is_some_and(|c|c["start_seconds"]!=self.baseline["start_seconds"]),
            2=>editor["open"]==true && editor["clip_id"].as_str()==self.clip.as_deref(),
            3 if lesson==Lesson::Audio=>track.is_some_and(|t|t["gain"]!=self.baseline["gain"]),
            3=>clip.is_some_and(|c|items(c,"notes").next().is_some()),
            4 if lesson==Lesson::Midi=>clip.is_some_and(|c| {
                // A committed change to an existing note, rather than merely
                // adding another note, teaches moving/resizing specifically.
                items(&self.baseline,"notes").any(|old|items(c,"notes").any(|n|n["id"]==old["id"] &&
                    (n["quarter"]!=old["quarter"] || n["pitch"]!=old["pitch"] || n["length_quarters"]!=old["length_quarters"])))
            }),
            n if n==play_step=> {
                let position=transport["position_frame"].as_f64().zip(transport["sample_rate"].as_f64().filter(|r|*r>0.)).map(|(p,r)|p/r);
                if transport["playing"]==true {
                    if let (Some(p),Some(c))=(position,clip) {
                        let a=c["start_seconds"].as_f64().unwrap_or(0.);
                        let b=a+c["duration_seconds"].as_f64().unwrap_or(0.);
                        if p>=a && p<b {
                            if self.playback_start.is_some_and(|previous|p>previous){true}
                            else {self.playback_start=Some(p);false}
                        } else {self.playback_start=None;false}
                    } else {false}
                } else {self.playback_start=None;false}
            },
            // Listening is deliberately a human confirmation, not a meter test.
            n if n==play_step+1=>true,
            _=>false,
        };
        self.ready|=done;
    }
    fn advance(&mut self,doc:&Value){
        if !self.ready || self.missing || self.step>=self.last(){return;}
        self.step+=1; self.ready=false; self.playback_start=None;
        self.baseline=if self.lesson==Some(Lesson::Audio) && self.step==3 {
            find(doc,"tracks",&self.track).cloned().unwrap_or(Value::Null)
        } else {find(doc,"clips",&self.clip).cloned().unwrap_or(Value::Null)};
    }
    fn geometry(view:Viewport)->(Rect,f64){
        let s=((view.width-16.)/470.).min((view.height-48.)/236.).clamp(0.01,1.);
        ([(view.width-470.*s)*0.5,42.*s,470.*s,236.*s],s)
    }
    pub fn toggle_open(&mut self){self.open=!self.open;}
    fn buttons(&self,view:Viewport)->Vec<(Rect,Action,String)>{
        let mut buttons=Vec::new();
        if self.open {
            let ([x,y,_,_],s)=Self::geometry(view);
            buttons.push(([x+12.*s,y+10.*s,210.*s,28.*s],Action::Audio,"Audio basics / restart".into()));
            buttons.push(([x+234.*s,y+10.*s,224.*s,28.*s],Action::Midi,"MIDI basics / restart".into()));
            if self.lesson.is_some() && self.step<self.last(){
                let text=if self.step==self.last()-1{"I heard it / finish"}else if self.ready{"Next"}else{"Waiting for your action"};
                buttons.push(([x+236.*s,y+196.*s,222.*s,28.*s],Action::Next,text.into()));
            }
        }
        buttons
    }
    /// Pass callback events and the final held sample in order. Capture release
    /// even outside the card; a foreign held press must never open the lesson.
    pub fn update(&mut self,input:&Input,view:Viewport,doc:&Value,blocked:bool,focused:bool)->bool {
        if !focused {self.captured=None;self.previous_down=input.left;return false;}
        let pressed=input.left&&!self.previous_down;
        let released=!input.left&&self.previous_down;
        self.previous_down=input.left;
        let card_hit=self.open&&input.pointer.is_some_and(|p|inside(Self::geometry(view).0,p));
        let hit=input.pointer.and_then(|p|self.buttons(view).into_iter().find(|(r,_,_)|inside(*r,p))).map(|(_,a,_)|a);
        let mut owns=self.captured.is_some();
        if pressed && !blocked {
            // Empty card space also owns its click, without dispatching a button.
            if hit.is_some()||card_hit {self.captured=Some(hit.unwrap_or(Action::Card));owns=true;}
        }
        if released {
            if let Some(action)=self.captured.take().filter(|a|Some(*a)==hit) {
                match action {
                    Action::Audio=>self.start(Lesson::Audio,doc),
                    Action::Midi=>self.start(Lesson::Midi,doc),
                    Action::Next=>self.advance(doc),
                    Action::Card=>{},
                }
            }
        }
        owns
    }
    // Keep callback parsing and guarded payload construction outside the large
    // application Step frame (including unoptimized desktop builds).
    #[inline(never)]
    pub fn guard_input(&mut self,raw:&Value,view:Viewport,doc:&Value,blocked:bool)->Option<Value> {
        let sample=super::piano_roll_scene::input(&raw["editor"]);
        let focused=raw["editor"]["focused"]!=false && raw["editor"]["transport_pointer_cancelled"]!=true;
        let mut owns=false;
        for event in raw["editor"]["transport_pointer_events"].as_array().into_iter().flatten().take(16){
            if let (Some(left),Ok(pointer))=(event["left"].as_bool(),serde_json::from_value::<[f64;2]>(event["pointer"].clone())) {
                let mut event_sample=sample.clone();event_sample.left=left;event_sample.pointer=Some(pointer);
                owns|=self.update(&event_sample,view,doc,blocked,focused);
            }
        }
        owns|=self.update(&sample,view,doc,blocked,focused);
        if owns {
            let mut guarded=raw.clone();
            guarded["editor"]=json!({"focused":false,"transport_pointer_cancelled":true});
            guarded["keys"]=json!([false,false,false,false,false]);Some(guarded)
        }else{None}
    }
    fn copy(&self)->(&str,[&str;4]) {
        if self.missing{return ("Lesson track or clip was removed",["Choose Audio basics or MIDI basics above to restart.","The tutorial will follow a new track.","Your project has not been reset or replaced.",""]);}
        let Some(lesson)=self.lesson else{return ("Learn by making something",["Choose Audio basics or MIDI basics above.","Work in your current project; your edits stay yours.","Each step checks your action before Next unlocks.","Hide the lesson any time, then LEARN to return."]);};
        match (lesson,self.step) {
            (_,0)=>("1. Create a track",["Open the left browser and choose Components.",if lesson==Lesson::Audio{"Drag Audio Track into the empty timeline area."}else{"Drag Midi Track into the empty timeline area."},"Release to create a new lane for this lesson.","Already have tracks? Make one new track for practice."]),
            (Lesson::Audio,1)=>("2. Add audio",["In the left browser choose Audio / USER_SAMPLES.","Drag a WAV onto the audio lane you just created.","Try the included sine_440Hz_10s_48k_mono.wav.","A tape appears when the import succeeds."]),
            (Lesson::Audio,2)=>("3. Move the audio clip",["Drag the new tape horizontally, then release.","Its position is elapsed time in seconds.","Moving the tape leaves the source recording intact.","Next unlocks after its position actually changes."]),
            (Lesson::Audio,3)=>("4. Set the track level",["Select your audio lane. Open the bottom Mixer page.","Move this track's volume fader a little, then release.","Start with a comfortable listening level.","Next checks the committed track gain."]),
            (Lesson::Midi,1)=>("2. Create a MIDI clip",["Drag across empty time on your new MIDI lane.","Release the range, then press Command-Shift-M.","An empty MIDI tape fills the selected interval.","Choose a few seconds so there is room for notes."]),
            (Lesson::Midi,2)=>("3. Enter the piano room",["Double-click the MIDI tape you just created.","The separate piano room edits this clip's notes.","Its BPM and snap belong to this clip.","Escape or < TIMELINE returns to the arrangement."]),
            (Lesson::Midi,3)=>("4. Draw your first note",["Click and release in the piano room's note area.","A bubble is a note: height is pitch, width is length.","Draw a few notes if you like. Free placement is fine.","Next checks that a note was saved in this clip."]),
            (Lesson::Midi,4)=>("5. Move or resize a note",["Drag a bubble to change its pitch or start time.","Or drag its right edge to change its length.","Release to commit the change.","Adding another note alone will not finish this step."]),
            (Lesson::Audio,4)|(Lesson::Midi,5)=>("Play your clip",["Return to the timeline if needed; click the ruler", "just before your clip. Press Space or the Play gem.","Let the playhead move through the lesson clip.","If silent, check mute, gain, routing and audio output."]),
            (Lesson::Audio,5)|(Lesson::Midi,6)=>("Listen and confirm",["Did you hear your clip? Stop playback with Space.","MIDI uses the built-in sound unless you add a plugin.","If silent, check mute, gain, routing and audio output.","Only choose I heard it when you actually heard it."]),
            _=>("Lesson complete",["Your practice track and clip remain in the project.","You created, edited and played your own material.","Choose the other lesson above, or hide this card.","More lessons can grow alongside the DAW."]),
        }
    }
    pub fn surface(&self,view:Viewport)->Value {
        let mut faces=Vec::new();let mut labels=Vec::new();
        let mut face=|r:Rect,c:[u8;3]|faces.push(json!({"rect":r,"color":c,"radius":3.,"alpha":1.}));
        let mut label=|r:Rect,t:&str,size:f64,c:[u8;3]|labels.push(json!({"rect":r,"text":t,"size":size,"center":false,"color":c}));
        if self.open {
            let ([x,y,w,h],s)=Self::geometry(view);face([x,y,w,h],[29,39,46]);
            let (title,lines)=self.copy();
            label([x+12.*s,y+50.*s,w-24.*s,26.*s],title,18.*s,[245,227,186]);
            for (i,line) in lines.iter().enumerate(){label([x+12.*s,y+(84.+i as f64*23.)*s,w-24.*s,22.*s],line,13.*s,[228,235,239]);}
            let progress=if self.lesson.is_some(){format!("{} / {}{}",self.step.min(self.last())+1,self.last()+1,if self.ready&&!self.missing{"  Ready"}else{""})}else{"Hands-on lessons".into()};
            label([x+12.*s,y+200.*s,215.*s,22.*s],&progress,12.*s,[167,205,196]);
        }
        for (r,a,text) in self.buttons(view){
            let disabled=a==Action::Next&&(!self.ready||self.missing);
            face(r,if disabled{[48,56,61]}else{[57,82,88]});
            label([r[0]+5.,r[1]+5.,(r[2]-5.).max(0.),r[3]],&text,12.*Self::geometry(view).1,if disabled{[151,160,165]}else{[239,244,247]});
        }
        json!({"clip":[0.,0.,view.width,view.height],"browser_faces":faces,"labels":labels})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{document, arrangement, piano_roll::{Editor, Rect as PianoRect}};
    fn snapshot(doc:&mut document::State)->Value {document::dispatch(doc,"Snapshot",&Value::Null).unwrap()}
    fn edit(doc:&mut document::State,hook:&str,mut request:Value)->Value {
        request["document_revision"]=snapshot(doc)["revision"].clone();
        document::dispatch(doc,hook,&request).unwrap()
    }
    fn track(doc:&mut document::State,kind:&str)->String {
        let action=arrangement::create_track(&snapshot(doc),kind).unwrap();
        let arrangement::Action::Document{hook,request:input}=action else{panic!("track command")};
        let id=input["id"].as_str().unwrap().to_owned();edit(doc,hook.rsplit('.').next().unwrap(),input);id
    }
    fn observe(t:&mut Tutorial,doc:&mut document::State){t.observe(&snapshot(doc),&Value::Null,&Value::Null);}
    fn next(t:&mut Tutorial,doc:&mut document::State){assert!(t.ready,"step {} not ready",t.step);t.advance(&snapshot(doc));}
    #[test]
    fn tutorial_audio_checks_real_track_clip_move_gain_and_playback_then_human_confirmation(){
        let mut doc=document::State::default();let old=track(&mut doc,"audio");
        let mut t=Tutorial::default();t.start(Lesson::Audio,&snapshot(&mut doc));
        observe(&mut t,&mut doc);assert!(!t.ready);assert!(t.track.is_none());
        let wrong=track(&mut doc,"midi");observe(&mut t,&mut doc);assert!(!t.ready);
        let id=track(&mut doc,"audio");observe(&mut t,&mut doc);assert_eq!(t.track.as_deref(),Some(id.as_str()));next(&mut t,&mut doc);
        edit(&mut doc,"PutSource",json!({"id":"tutorial-pcm","sample_rate":8000,"channels":1,"samples":vec![0.5;16]}));
        // Material on an unrelated track must not complete the import step.
        edit(&mut doc,"PutClip",json!({"id":"old-clip","kind":"audio","track":old,"source":"tutorial-pcm","start_seconds":0.,"source_offset_seconds":0.,"duration_seconds":0.002}));
        observe(&mut t,&mut doc);assert!(!t.ready);
        edit(&mut doc,"PutClip",json!({"id":"practice","kind":"audio","track":id,"source":"tutorial-pcm","start_seconds":0.,"source_offset_seconds":0.,"duration_seconds":0.002}));
        observe(&mut t,&mut doc);next(&mut t,&mut doc);observe(&mut t,&mut doc);assert!(!t.ready);
        edit(&mut doc,"MoveClip",json!({"id":"old-clip","start_seconds":3.}));observe(&mut t,&mut doc);assert!(!t.ready);
        edit(&mut doc,"MoveClip",json!({"id":"practice","start_seconds":1.}));observe(&mut t,&mut doc);next(&mut t,&mut doc);
        edit(&mut doc,"SetTrackMix",json!({"id":wrong,"gain":0.3}));observe(&mut t,&mut doc);assert!(!t.ready);
        edit(&mut doc,"SetTrackMix",json!({"id":id,"gain":0.4}));observe(&mut t,&mut doc);next(&mut t,&mut doc);
        // A play flag, a stationary playhead or playback outside the clip does not pass.
        let s=snapshot(&mut doc);
        for frame in [0,0,8000,8000] {t.observe(&s,&Value::Null,&json!({"playing":true,"position_frame":frame,"sample_rate":8000}));assert!(!t.ready);}
        t.observe(&s,&Value::Null,&json!({"playing":true,"position_frame":8001,"sample_rate":8000}));assert!(t.ready);next(&mut t,&mut doc);
        assert_eq!(t.step,5);assert!(!t.status()["completed"].as_bool().unwrap());
        observe(&mut t,&mut doc);assert!(t.ready);next(&mut t,&mut doc);assert_eq!(t.status()["completed"],true);
        let revision=snapshot(&mut doc)["revision"].clone();
        let pcm=document::dispatch(&mut doc,"RenderRange",&json!({"document_revision":revision,"sample_rate":8000,"start_frame":8000,"frames":8})).unwrap();
        assert!(pcm["lsffr"].as_array().unwrap().iter().any(|frame|frame.as_array().unwrap().iter().any(|v|v.as_f64().unwrap().abs()>0.)));
    }
    #[test]
    fn tutorial_midi_range_and_real_editor_commit_unlock_the_correct_steps(){
        let mut doc=document::State::default();let mut t=Tutorial::default();t.start(Lesson::Midi,&snapshot(&mut doc));
        let id=track(&mut doc,"midi");observe(&mut t,&mut doc);next(&mut t,&mut doc);
        edit(&mut doc,"CreateRangeClips",json!({"first_track":id,"last_track":id,"start_seconds":0.,"end_seconds":4.}));
        observe(&mut t,&mut doc);next(&mut t,&mut doc);let clip=t.clip.clone().unwrap();
        t.observe(&snapshot(&mut doc),&json!({"open":true,"clip_id":"other"}),&Value::Null);assert!(!t.ready);
        let mut editor=Editor::default();editor.open(&snapshot(&mut doc),&clip).unwrap();
        t.observe(&snapshot(&mut doc),&editor.status(),&Value::Null);next(&mut t,&mut doc);
        let rect=PianoRect{left:0.,top:0.,width:800.,height:400.};
        let payload=editor.update(&Input{enter:true,..Input::default()},rect,0.01).unwrap().unwrap();
        // Preview alone must not complete the lesson.
        t.observe(&snapshot(&mut doc),&editor.status(),&Value::Null);assert!(!t.ready);
        document::dispatch(&mut doc,"PutClip",&payload).unwrap();editor.sync(&snapshot(&mut doc)).unwrap();
        t.observe(&snapshot(&mut doc),&editor.status(),&Value::Null);next(&mut t,&mut doc);
        editor.update(&Input::default(),rect,0.01).unwrap();
        let payload=editor.update(&Input{arrows:[false,false,true,false],..Input::default()},rect,0.01).unwrap().unwrap();
        document::dispatch(&mut doc,"PutClip",&payload).unwrap();editor.sync(&snapshot(&mut doc)).unwrap();
        t.observe(&snapshot(&mut doc),&editor.status(),&Value::Null);assert!(t.ready);next(&mut t,&mut doc);
        let s=snapshot(&mut doc);
        for frame in [0,1] {t.observe(&s,&editor.status(),&json!({"playing":true,"position_frame":frame,"sample_rate":8000}));}
        assert!(t.ready);next(&mut t,&mut doc);observe(&mut t,&mut doc);next(&mut t,&mut doc);assert_eq!(t.status()["completed"],true);
        let pcm=document::dispatch(&mut doc,"RenderRange",&json!({"document_revision":s["revision"],"sample_rate":8000,"start_frame":0,"frames":2000,"instrument":"cardinal_sine"})).unwrap();
        assert!(pcm["lsffr"].as_array().unwrap().iter().any(|frame|frame.as_array().unwrap().iter().any(|v|v.as_f64().unwrap().abs()>0.)));
    }
    fn view()->Viewport{Viewport{width:960.,height:600.,canvas_scale:1.}}
    fn click(t:&mut Tutorial,doc:&Value,p:[f64;2]) {
        for left in [true,false]{assert!(t.update(&Input{left,pointer:Some(p),..Input::default()},view(),doc,false,true));}
    }
    #[test]
    fn tutorial_pointer_callbacks_hide_resume_restart_and_focus_never_edit_document(){
        let mut doc=document::State::default();let before=snapshot(&mut doc);let mut t=Tutorial::default();
        assert!(t.surface(view())["browser_faces"].as_array().unwrap().is_empty());
        assert!(!t.update(&Input{left:true,pointer:Some([920.,18.]),..Input::default()},view(),&before,false,true));
        assert!(!t.update(&Input{pointer:Some([920.,18.]),..Input::default()},view(),&before,false,true));
        assert!(!t.open);
        t.toggle_open();assert!(t.open);
        let ([x,y,_,_],_)=Tutorial::geometry(view());click(&mut t,&before,[x+25.,y+20.]);assert_eq!(t.lesson,Some(Lesson::Audio));
        click(&mut t,&before,[x+250.,y+205.]);assert_eq!(t.step,0); // gated Next
        t.toggle_open();assert!(!t.open);t.toggle_open();assert_eq!(t.lesson,Some(Lesson::Audio));
        t.update(&Input{left:true,pointer:Some([x+250.,y+20.]),..Input::default()},view(),&before,false,true);
        t.update(&Input::default(),view(),&before,false,false);
        t.update(&Input{pointer:Some([x+250.,y+20.]),..Input::default()},view(),&before,false,true);
        assert_eq!(t.lesson,Some(Lesson::Audio));assert_eq!(snapshot(&mut doc),before);
        // A foreign drag entering the button must not become a lesson click.
        t.update(&Input{left:true,pointer:Some([0.,400.]),..Input::default()},view(),&before,false,true);
        t.update(&Input{left:true,pointer:Some([920.,18.]),..Input::default()},view(),&before,false,true);
        assert!(!t.update(&Input{pointer:Some([920.,18.]),..Input::default()},view(),&before,false,true));assert!(t.open);
    }
    #[test]
    fn tutorial_deleted_target_requires_restart_and_overlay_is_bounded(){
        let mut doc=document::State::default();let mut t=Tutorial::default();t.start(Lesson::Midi,&snapshot(&mut doc));
        let id=track(&mut doc,"midi");observe(&mut t,&mut doc);next(&mut t,&mut doc);
        edit(&mut doc,"CreateRangeClips",json!({"first_track":id,"last_track":id,"start_seconds":0.,"end_seconds":4.}));
        observe(&mut t,&mut doc);next(&mut t,&mut doc);
        edit(&mut doc,"RemoveClip",json!({"id":t.clip}));observe(&mut t,&mut doc);assert!(t.missing);assert!(!t.ready);
        t.open=true;
        for (width,height) in [(960.,600.),(640.,480.),(390.,844.)] {
            let view=Viewport{width,height,canvas_scale:1.};
            assert!(super::super::plugin_surface::valid(&json!({"groups":[t.surface(view)]})));
            let [x,y,w,h]=Tutorial::geometry(view).0;assert!(x>=0.&&y>=0.&&x+w<=width&&y+h<=height);
        }
    }
}
