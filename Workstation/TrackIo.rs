//! Cardinal page-zero track I/O and lane traffic-light interaction.
//! Source: DawUiSystem::updateDawUILayout, UIStampingSystem, DawTrackSystem,
//! MidiTrackSystem and DawInputMonitoringSystem. No device names are invented.
use super::{panel_layout::{self, Options, Viewport}, panel_motion::Snapshot, piano_roll::Input};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TrackIo {
    pub arm_mode: u8,
    pub input_monitor_mode: u8,
    pub physical_input_index: usize,
    pub stereo_input_pair_12: bool,
    pub use_virtual_input: bool,
}
impl TrackIo {
    pub fn new(kind: &str) -> Self {
        Self { arm_mode: 0, input_monitor_mode: if kind == "midi" {2} else {0},
            physical_input_index: 0, stereo_input_pair_12: false, use_virtual_input: false }
    }
    pub fn monitors_input(self) -> bool {
        self.input_monitor_mode == 2 || (self.input_monitor_mode == 1 && self.arm_mode > 0)
    }
    pub fn records(self) -> bool { matches!(self.arm_mode, 1 | 2) }
    pub fn snapshot(self) -> Value {
        json!({"arm_mode":self.arm_mode,"input_monitor_mode":self.input_monitor_mode,
            "physical_input_index":self.physical_input_index,"stereo_input_pair_12":self.stereo_input_pair_12,
            "use_virtual_input":self.use_virtual_input})
    }
    pub fn from_patch(input: &Value, old: Self, kind: &str) -> Result<Self,String> {
        let mut next=old;
        let mode=|key:&str,max:u64,previous:u8|->Result<u8,String>{
            input.get(key).map_or(Ok(previous),|v|v.as_u64().filter(|n|*n<=max)
                .map(|n|n as u8).ok_or_else(||format!("{key} must be an integer within 0..{max}")))
        };
        next.arm_mode=mode("arm_mode",3,old.arm_mode)?;
        next.input_monitor_mode=mode("input_monitor_mode",2,old.input_monitor_mode)?;
        for key in ["physical_input_index","stereo_input_pair_12","use_virtual_input"] {
            if input.get(key).is_some() && kind!="audio" {return Err("physical input selection requires an audio track".into());}
        }
        if let Some(v)=input.get("physical_input_index") {
            next.physical_input_index=v.as_u64().filter(|n|*n<=256).ok_or("physical_input_index exceeds 256")? as usize;
        }
        for (key,target) in [("stereo_input_pair_12",&mut next.stereo_input_pair_12),("use_virtual_input",&mut next.use_virtual_input)] {
            if let Some(v)=input.get(key){*target=v.as_bool().ok_or_else(||format!("{key} must be boolean"))?;}
        }
        if next.stereo_input_pair_12 && (next.use_virtual_input || next.physical_input_index!=0) {
            return Err("IN1+2 requires physical inputs 1/2 and cannot select VM1".into());
        }
        Ok(next)
    }
    pub fn cycle_input(self, physical_count:usize) -> Self {
        let count=physical_count.min(256);
        let stereo=count+1;
        let states=count+1+usize::from(count>=2);
        let current=if self.stereo_input_pair_12 {stereo} else {self.physical_input_index.min(count)};
        let next=(current+1)%states;
        Self { physical_input_index:if count>=2&&next==stereo {0}else{next},
            stereo_input_pair_12:count>=2&&next==stereo,
            use_virtual_input:next==count,..self }
    }
    pub fn input_label(self, physical_count:usize) -> String {
        if self.stereo_input_pair_12 && physical_count>=2 {"IN1+2".into()}
        else if !self.use_virtual_input && self.physical_input_index<physical_count {format!("IN{}",self.physical_input_index+1)}
        else {"VM1".into()}
    }
}
pub(crate) fn next_arm(current:u8)->u8 {if current==0 {3}else if current==3 {1}else{0}}
pub(crate) fn crosswalk_palette(arm:u8, recording:bool, seconds:f64)->usize {
    if arm==3 {1} else if matches!(arm,1|2) && (!recording || seconds.max(0.).rem_euclid(1.)<0.5) {2} else {0}
}
fn next_destination(track:&Value,tracks:&[Value])->String{
 let current=track["output"].as_str().unwrap_or("");
 let mut choices=if track["kind"]=="channel"{vec!["OFF".to_owned()]}else{vec![String::new(),"OFF".into()]};
 for target in tracks.iter().filter(|t|t["id"]!=track["id"]&&matches!(t["kind"].as_str(),Some("audio"|"midi"|"matrix"|"channel"))){
  let Some(id)=target["id"].as_str()else{continue;};
  let mut at=id;let mut seen=std::collections::BTreeSet::new();let mut feedback=false;
  while !at.is_empty()&&at!="OFF"{
   if track["id"]==at||!seen.insert(at){feedback=true;break;}
   let Some(next)=tracks.iter().find(|t|t["id"]==at&&t["kind"]!="matrix")else{break;};
   at=next["output"].as_str().unwrap_or("");
  }
  if !feedback{choices.push(id.into());}
 }
 let next=choices.iter().position(|s|s==current).map_or(0,|i|(i+1)%choices.len());choices[next].clone()
}
fn destination_label(track:&Value,tracks:&[Value])->String{let id=track["output"].as_str().unwrap_or("");if id.is_empty(){"BUS L/R".into()}else if id=="OFF"{"OFF".into()}else{tracks.iter().find(|t|t["id"]==id).map(|t|t["name"].as_str().unwrap_or(id).to_owned()).unwrap_or("MISSING".into())}}
pub(crate) fn next_bus(current:&str)->&'static str {
    match current {"OFF"=>"L","L"=>"S","S"=>"FF","FF"=>"R",_=>"OFF"}
}

/// Cardinal automation target ordering includes NONE and WORKSPACE before
/// audio lanes, then MIDI lanes. A target selection does not invent a processor.
#[derive(Clone,Debug,Default,PartialEq,Eq)]
pub(super) enum CcLane { #[default] None, Workspace, Track(String) }
impl CcLane {
    pub fn snapshot(&self)->Value {match self {Self::None=>Value::Null,Self::Workspace=>json!({"kind":"workspace"}),Self::Track(id)=>json!({"track":id})}}
    pub fn next(&self,tracks:&[Value])->Self {
        let mut lanes=vec![Self::None,Self::Workspace];
        for kind in ["audio","midi"] {for track in tracks.iter().filter(|t|t["kind"]==kind){if let Some(id)=track["id"].as_str(){lanes.push(Self::Track(id.into()));}}}
        let index=lanes.iter().position(|lane|lane==self).unwrap_or(0);
        lanes[(index+1)%lanes.len()].clone()
    }
}
fn cc_lane_label(track:&Value,tracks:&[Value])->String {
    let lane=&track["cc_target_lane"];
    if lane["kind"]=="workspace" {return "WORKSPACE".into();}
    let Some(id)=lane["track"].as_str()else{return "NONE".into();};
    for (index,dest) in tracks.iter().filter(|t|t["kind"]=="audio").enumerate(){if dest["id"]==id{return format!("AUD{}",index+1);}}
    for (index,dest) in tracks.iter().filter(|t|t["kind"]=="midi").enumerate(){if dest["id"]==id{return dest["name"].as_str().filter(|name|!name.is_empty()).map(str::to_owned).unwrap_or_else(||format!("MID{}",index+1));}}
    "NONE".into()
}

pub(crate) const MAX_CONTROLS:usize=60;
#[derive(Clone,Debug)]
pub(crate) struct Control {pub rect:[f64;4],pub track:String,pub key:&'static str,pub text:String,pub active:bool,pub arm:u8,pub crosswalk:bool}
pub(crate) fn contains(r:[f64;4],p:[f64;2])->bool {
    p[0]>=r[0]&&p[0]<=r[0]+r[2]&&p[1]>=r[1]&&p[1]<=r[1]+r[3]
}
fn io(track:&Value)->TrackIo {
    TrackIo::from_patch(track,TrackIo::new(track["kind"].as_str().unwrap_or("audio")),track["kind"].as_str().unwrap_or("audio"))
        .unwrap_or_else(|_|TrackIo::new("audio"))
}
#[derive(Default)]
pub(crate) struct Ui {
    pub page:usize,
    pub lane_layout:usize,
    scroll:f64,
    previous_left:bool,
    captured:bool,
    pressed_control:Option<(String,&'static str)>,
    pending_control:Option<(String,&'static str)>,
    presentation:Value,
    last_context:Option<(usize,[f64;4])>,
    virtual_mic_available:bool,
    theatre:Option<bool>,
    audio_rays:bool,
    loop_enabled:bool,
    headset_view:Option<bool>,
    parameter_picker:super::cc_parameter_picker::State,
}
impl Ui {
    pub fn set_loop_enabled(&mut self,enabled:bool){self.loop_enabled=enabled;}
    pub fn set_audio_rays(&mut self,enabled:bool){self.audio_rays=enabled;}
    pub fn set_headset_view(&mut self,camera:Option<bool>){self.headset_view=camera;}
    pub fn set_theatre(&mut self,enabled:Option<bool>){self.theatre=enabled;}
    fn controls(&self,tracks:&[Value],rect:[f64;4],physical_inputs:usize)->Vec<Control>{
        if self.page==1 {
            let mut controls=vec![Control{rect:[rect[0]+rect[2]*0.5-54.,rect[1]+68.,108.,30.],track:String::new(),key:"learn",text:"LEARN".into(),active:false,arm:0,crosswalk:false},
                Control{rect:[rect[0]+rect[2]*0.5-54.,rect[1]+126.,108.,30.],track:String::new(),key:"loop",text:"LOOP".into(),active:self.loop_enabled,arm:0,crosswalk:false},
                Control{rect:[rect[0]+rect[2]*0.5-54.,rect[1]+184.,108.,30.],track:String::new(),key:"drum_rack",text:"DRUM RACK".into(),active:false,arm:0,crosswalk:false},
                Control{rect:[rect[0]+rect[2]*0.5-54.,rect[1]+242.,108.,30.],track:String::new(),key:"audio_rays",text:if self.audio_rays{"RAYS ON"}else{"RAYS OFF"}.into(),active:self.audio_rays,arm:0,crosswalk:false}];
            if let Some(enabled)=self.theatre {controls.push(Control{rect:[rect[0]+rect[2]*0.5-54.,rect[1]+300.,108.,30.],track:String::new(),key:"theatre",text:if enabled{"SPEAKERS ON"}else{"SPEAKERS OFF"}.into(),active:enabled,arm:0,crosswalk:false});}
            if let Some(camera)=self.headset_view {controls.push(Control{rect:[rect[0]+rect[2]*0.5-54.,rect[1]+358.,108.,30.],track:String::new(),key:"headset_view",text:if camera{"CAMERAS"}else{"PLAYER VIEW"}.into(),active:camera,arm:0,crosswalk:false});}
            return controls;
        }
        if self.page!=0{return Vec::new();}
        let mut rows_rect=rect;rows_rect[3]=(rect[3]-58.).max(0.);
        let mut controls=layout(tracks,rows_rect,self.scroll,physical_inputs);
        if rect[2]>=120. && rect[3]>=100. {
            controls.truncate(MAX_CONTROLS-1);
            controls.push(Control{rect:[rect[0]+rect[2]*0.5-54.,rect[1]+rect[3]-48.,108.,30.],track:String::new(),key:"lane_layout",text:format!("TRACK {}",["TOP","BOTTOM","FREE"][self.lane_layout.min(2)]),active:false,arm:0,crosswalk:false});
        }
        controls
    }

    pub fn owns_pointer(&self)->bool {self.captured||self.pending_control.is_some()||self.parameter_picker.is_open()}
    pub fn presentation(&self)->Value {self.presentation.clone()}
    pub fn set_virtual_mic_available(&mut self,available:bool){self.virtual_mic_available=available;}
    pub fn input_notice(&self)->Option<&str>{self.presentation["input_notice"].as_str()}
    pub fn cancel(&mut self){self.previous_left=false;self.captured=false;self.pressed_control=None;self.pending_control=None;self.last_context=None;self.parameter_picker.close();}
    pub fn update(&mut self,snapshot:&Value,input:&Input,view:Viewport,motion:&Snapshot,
        options:Options,physical_inputs:usize,blocked:bool,recording:bool,time:f64)->Result<Option<Value>,String> {
        self.update_events(snapshot,input,view,motion,options,physical_inputs,blocked,recording,time,&[])
    }
    pub fn update_events(&mut self,snapshot:&Value,input:&Input,view:Viewport,motion:&Snapshot,
        options:Options,physical_inputs:usize,blocked:bool,recording:bool,time:f64,events:&[(bool,[f64;2])])->Result<Option<Value>,String> {
        let pressed=input.left&&!self.previous_left;
        let panel=panel_layout::visual_bounds(view,motion,options)?[3];
        let rect=[panel.x,panel.y,panel.width,panel.height];
        let context=(self.page,rect);
        let blocked=blocked || self.page>1 || input.escape
            || !input.pointer.is_some_and(|p|p.iter().all(|v|v.is_finite()))
            // Panel easing is asymptotic: even a settled panel retains tiny
            // f64 changes. Those must not cancel a press, release or delayed
            // activation. Meaningful movement and page changes still cancel.
            || self.last_context.is_some_and(|old|old.0!=context.0
                || old.1.iter().zip(rect).any(|(a,b)|(a-b).abs()>0.001));
        self.last_context=Some(context);
        if blocked {self.captured=false;self.pressed_control=None;self.pending_control=None;}
        let rows:Vec<Value>=snapshot["tracks"].as_array().ok_or("track I/O snapshot requires tracks")?.iter().chain(snapshot["buses"].as_array().into_iter().flatten()).cloned().collect();
        let tracks=rows.as_slice();
        // UIStampingSystem clamps the independently scrolled I/O document.
        let minimum=(-(tracks.len().saturating_sub(1) as f64)*72.).min(0.);
        self.scroll=self.scroll.clamp(minimum,0.);
        let over=input.pointer.is_some_and(|p|contains(rect,p));
        if self.page==0 && over && !blocked && input.wheel[1].is_finite() && input.wheel[1]!=0. {
            self.scroll=(self.scroll+input.wheel[1]*24.).clamp(minimum,0.);
        }
        let controls=self.controls(tracks,rect,physical_inputs);
        let mut action=None;
        if self.page!=0||blocked{self.parameter_picker.close();}
        if let Some(id)=self.parameter_picker.track().map(str::to_owned){
            if let Some(track)=tracks.iter().find(|t|t["id"]==id){
                let labels=parameter_labels(track);
                self.parameter_picker.sync(&id,&labels,track["cc_parameter_slot"].as_u64().unwrap_or(0) as usize);
            }else{self.parameter_picker.close();}
        }
        let anchor=self.parameter_picker.track().and_then(|id|controls.iter().find(|c|c.track==id&&c.key=="target_param")).map(|c|c.rect);
        let picker=self.parameter_picker.update(input.pointer,pressed,self.pending_control.is_some(),anchor,[view.width,view.height]);
        if picker.consumed{self.captured|=input.left;}
        if let Some(selection)=picker.selection{
            action=Some(json!({"document_revision":snapshot["revision"],"id":selection.track,"cc_action":"target_param_pick","cc_parameter_slot":selection.slot}));
        }
        let activation=self.pending_control.take().filter(|_|!blocked);
        if blocked {self.pressed_control=None;}
        // Preserve press/release callbacks that both arrive between rendered
        // frames. Resolve pending actions only on the next update, as above.
        for (left,pointer) in events.iter().take(16).map(|(left,p)|(*left,Some(*p)))
            .chain(std::iter::once((input.left,input.pointer))) {
            let pressed=left&&!self.previous_left;let released=!left&&self.previous_left;
            self.previous_left=left;self.captured &= left;
            if pressed && pointer.is_some_and(|p|contains(rect,p)) && !blocked && (!picker.consumed||picker.button_hit) {
                self.captured=true;
                self.pressed_control=pointer.and_then(|p|controls.iter().find(|c|contains(c.rect,p))).map(|c|(c.track.clone(),c.key));
            }
            if released && !blocked {
                self.pending_control=pointer.and_then(|p|controls.iter().find(|c|contains(c.rect,p)))
                    .filter(|c|self.pressed_control.as_ref().is_some_and(|(track,key)|track==&c.track&&*key==c.key))
                    .map(|c|(c.track.clone(),c.key));
            }
            if released {self.pressed_control=None;}
        }
        // ButtonSystem::activateButton sets actionDelayFrames=1. Resolve the
        // original target on the following update using the current revision.
        if let Some((target,key))=activation {
            if let Some(control)=controls.iter().find(|c|c.track==target&&c.key==key) {
                if key=="lane_layout" {action=Some(json!({"lane_layout_cycle":true}));
                } else if key=="import_audio" {action=Some(json!({"import_audio_files":true}));
                } else if key=="import_plugins" {action=Some(json!({"import_plugin_files":true}));
                } else if key=="learn" {action=Some(json!({"learn_toggle":true}));
                } else if key=="loop" {action=Some(json!({"loop_toggle":true}));
                } else if key=="audio_rays" {action=Some(json!({"audio_rays":!self.audio_rays}));
                } else if key=="headset_view" {action=Some(json!({"headset_camera":!self.headset_view.unwrap_or(false)}));
                } else if key=="drum_rack" {
                    action=Some(json!({"drum_rack_open":true}));
                } else if key=="theatre" {
                    action=Some(json!({"theatre_output":!self.theatre.unwrap_or(false)}));
                } else {
                let track=tracks.iter().find(|t|t["id"]==control.track).ok_or("track I/O target disappeared")?;
                let mut patch=json!({"document_revision":snapshot["revision"],"id":control.track});
                if matches!(track["kind"].as_str(),Some("matrix"|"mix"|"channel")) {
                    patch["bus_edit"]=json!(true);
                    match control.key {
                        "channel_output"=>patch["output"]=json!(next_destination(track,&tracks)),
                        "bus_input"=>patch["input"]=json!(match track["input"].as_str().unwrap_or("L"){"L"=>"S","S"=>"FF","FF"=>"R",_=>"L"}),
                        "bus_output"=>patch["output"]=json!(if track["output"]=="OFF"{if track["kind"]=="matrix"{"flown_array"}else{"monitor"}}else{"OFF"}),
                        "mute"=>patch["mute"]=json!(track["mute"]!=true),_=>{}
                    }
                } else if track["kind"]=="cc" {
                    if control.key=="target_param"{
                        let labels=parameter_labels(track);
                        self.parameter_picker.toggle(&control.track,&labels,track["cc_parameter_slot"].as_u64().unwrap_or(0) as usize);
                        self.parameter_picker.update(input.pointer,false,false,Some(control.rect),[view.width,view.height]);
                    }else{self.parameter_picker.close();patch["cc_action"]=json!(control.key);}
                } else { match control.key {
                    "channel_output"=>patch["output"]=json!(next_destination(track,&tracks)),
                    "arm"=>patch["arm_mode"]=json!(next_arm(io(track).arm_mode)),
                    "input"=>{for (k,v) in io(track).cycle_input(physical_inputs).snapshot().as_object().unwrap(){patch[k]=v.clone();}},
                    "solo"=>patch["solo"]=json!(track["solo"]!=true),
                    "mute"=>patch["mute"]=json!(track["mute"]!=true),
                    "output_l"=>patch["output_bus_l"]=json!(next_bus(track["output_bus_l"].as_str().unwrap_or("OFF"))),
                    "output_r"=>patch["output_bus_r"]=json!(next_bus(track["output_bus_r"].as_str().unwrap_or("OFF"))),
                    // MIDI's source C is a clear-contents action, never a new clip button.
                    "clear"=>patch["clear_midi_contents"]=json!(true),
                    _=>return Err("unknown track I/O control".into()),
                }
                }
                if track["kind"]!="cc"||control.key!="target_param"{action=Some(patch);}
                }
            }
        }
        self.present(snapshot,rect,&controls,recording,time);
        Ok(action)
    }
    fn present(&mut self,snapshot:&Value,rect:[f64;4],controls:&[Control],recording:bool,time:f64) {
        let tracks=snapshot["tracks"].as_array();
        let input_notice=(self.page==0&&!self.virtual_mic_available&&tracks.is_some_and(|tracks|tracks.iter().any(|t|t["kind"]=="audio"&&io(t).input_label(0)=="VM1")))
            .then_some("VM1 unavailable: no virtual microphone input");
        self.presentation=json!({"page":self.page,"input_notice":input_notice,"panel":rect,"parameter_picker":self.parameter_picker.presentation(super::cc_parameter_picker::Theme::default()),"controls":controls.iter().map(|c|json!({
            "rect":c.rect,"track":c.track,"key":c.key,"text":c.text,"active":c.active,"crosswalk":c.crosswalk,
            "source_available":if c.key=="input"{Some(c.text!="VM1"||self.virtual_mic_available)}else{None},
            "unavailable_reason":if c.key=="input"&&c.text=="VM1"&&!self.virtual_mic_available{Some("No virtual microphone input")}else{None},
            "pressed":self.pressed_control.as_ref().is_some_and(|(track,key)|track==&c.track&&*key==c.key),
            "crosswalk_palette":crosswalk_palette(c.arm,recording&&snapshot["recording"]["record_enabled_tracks"].as_array().is_some_and(|ids|ids.iter().any(|id|id==&c.track)),time)
        })).collect::<Vec<_>>()});
    }
    // Refresh an accepted or rejected edit without sampling pointer edges twice
    // or cancelling a new press delivered alongside the prior activation.
    pub fn refresh(&mut self,snapshot:&Value,view:Viewport,motion:&Snapshot,options:Options,
        physical_inputs:usize,recording:bool,time:f64)->Result<(),String> {
        let panel=panel_layout::visual_bounds(view,motion,options)?[3];
        let rect=[panel.x,panel.y,panel.width,panel.height];
        let rows:Vec<Value>=snapshot["tracks"].as_array().ok_or("track I/O snapshot requires tracks")?.iter().chain(snapshot["buses"].as_array().into_iter().flatten()).cloned().collect();
        let tracks=rows.as_slice();
        let minimum=(-(tracks.len().saturating_sub(1) as f64)*72.).min(0.);
        self.scroll=self.scroll.clamp(minimum,0.);
        let controls=self.controls(tracks,rect,physical_inputs);
        self.present(snapshot,rect,&controls,recording,time);Ok(())
    }
}

fn parameter_labels(track:&Value)->Vec<String>{track["cc_parameters"].as_array().map_or_else(Vec::new,|rows|rows.iter().map(|row|row["label"].as_str().unwrap_or("NONE").to_owned()).collect())}

/// Original equal-slot horizontal layout and independently stamped 72px rows.
/// All controls retain their 36px pick boxes; authored 3-D geometry is fitted uniformly.
pub(crate) fn layout(tracks:&[Value],panel:[f64;4],scroll:f64,physical_inputs:usize)->Vec<Control> {
    let [left,top,width,height]=panel;
    let inset=(width*0.08).clamp(18.,32.);
    let step=((width-2.*inset)/5.).max(1.);
    let mut result=Vec::new();
    for (index,track) in tracks.iter().enumerate() {
        let y=top+64.+index as f64*72.+scroll;
        if y-18.<top || y+18.>top+height-68. {continue;}
        let kind=track["kind"].as_str().unwrap_or("");
        if matches!(kind,"matrix"|"mix"|"channel"){
            let keys=if kind=="matrix"{vec!["bus_input","mute"]}else if kind=="channel"{vec!["channel_output","mute"]}else{vec!["bus_output","mute"]};
            if result.len()+keys.len()>MAX_CONTROLS{break;}
            for (slot,key) in keys.into_iter().enumerate(){
                let text=match key{"channel_output"=>destination_label(track,tracks),"bus_input"=>format!("IN {}",track["input"].as_str().unwrap_or("L")),"bus_output"=>if track["output"]=="OFF"{"OFF".into()}else if kind=="matrix"{"ARRAY".into()}else{"MON".into()},_=>"M".into()};
                result.push(Control{rect:[left+inset+slot as f64*step-18.,y-18.,36.,36.],track:track["id"].as_str().unwrap_or("").into(),key,text,active:key=="mute"&&track["mute"]==true,arm:0,crosswalk:false});
            }
            continue;
        }
        if kind=="cc" {
            if result.len()+4>MAX_CONTROLS{break;}
            let usable=(width-64.).max(0.);
            let cc_step=if usable>0.{usable/3.}else{44.}.clamp(24.,44.);
            for (slot,key) in ["arm","target_lane","target_device","target_param"].into_iter().enumerate(){
                let text=match key {"arm"=>"A".into(),"target_lane"=>cc_lane_label(track,tracks),
                    "target_device"=>track["cc_target_device_label"].as_str().unwrap_or("NONE").into(),
                    _=>track["cc_target_parameter_label"].as_str().unwrap_or("NONE").into()};
                result.push(Control{rect:[left+32.+slot as f64*cc_step-18.,y-18.,36.,36.],track:track["id"].as_str().unwrap_or("").into(),key,text,
                    active:key=="arm"&&track["arm_mode"].as_u64().unwrap_or(0)>0,arm:0,crosswalk:false});
            }
            continue;
        }
        if !matches!(kind,"audio"|"midi"){continue;}
        if result.len()+7>MAX_CONTROLS {break;}
        if tracks.iter().any(|t|t["kind"]=="channel") || track["output"].as_str().is_some_and(|out|!out.is_empty()) {result.push(Control{rect:[left+inset-18.,y+20.,108.,24.],track:track["id"].as_str().unwrap_or("").into(),key:"channel_output",text:destination_label(track,tracks),active:false,arm:0,crosswalk:false});}
        let state=io(track);
        for (slot,key) in [if kind=="midi"{"clear"}else{"input"},"arm","solo","mute","output_l","output_r"].into_iter().enumerate() {
            let text=match key {"input"=>state.input_label(physical_inputs),"clear"=>"CLEAR".into(),"arm"=>"ARM".into(),"solo"=>"SOLO".into(),"mute"=>"MUTE".into(),
                "output_l"=>track["output_bus_l"].as_str().unwrap_or("OFF").into(),"output_r"=>track["output_bus_r"].as_str().unwrap_or("OFF").into(),_=>String::new()};
            result.push(Control {rect:[left+inset+slot as f64*step-18.,y-18.,36.,36.],track:track["id"].as_str().unwrap_or("").into(),key,text,
                active:match key {"solo"=>track["solo"]==true,"mute"=>track["mute"]==true,_=>false},arm:state.arm_mode,crosswalk:key=="arm"});
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn original_states_and_blink_boundaries(){
        assert_eq!((next_arm(0),next_arm(3),next_arm(1),next_arm(2)),(3,1,0,0));
        for mode in 0..3 {for arm in 0..4 {let s=TrackIo{arm_mode:arm,input_monitor_mode:mode,..TrackIo::new("audio")};
            assert_eq!(s.monitors_input(),mode==2||(mode==1&&arm>0));assert_eq!(s.records(),arm==1||arm==2);}}
        assert_eq!(TrackIo::new("audio").input_monitor_mode,0);assert_eq!(TrackIo::new("midi").input_monitor_mode,2);
        for time in [0.,0.499999,0.5,0.999999,1.] {assert_eq!(crosswalk_palette(3,true,time),1);assert_eq!(crosswalk_palette(0,true,time),0);}
        assert_eq!([0.,0.499999,0.5,0.999999,1.].map(|t|crosswalk_palette(1,true,t)),[2,2,0,0,2]);
        assert_eq!(crosswalk_palette(2,false,0.7),2);
    }
    #[test] fn physical_vm_stereo_cycle_and_bus_order(){
        let mut s=TrackIo::new("audio");let mut labels=Vec::new();for _ in 0..5{labels.push(s.input_label(2));s=s.cycle_input(2);}
        assert_eq!(labels,["IN1","IN2","VM1","IN1+2","IN1"]);
        let s=TrackIo::new("audio").cycle_input(0);assert!(s.use_virtual_input);assert_eq!(s.input_label(0),"VM1");
        assert!(!TrackIo::new("audio").cycle_input(1).stereo_input_pair_12);
        assert_eq!(["OFF","L","S","FF","R"].map(next_bus),["L","S","FF","R","OFF"]);
    }
    #[test] fn source_layout_stamps_all_six_controls_and_keeps_cc_row_spacing(){
        let tracks=vec![json!({"id":"a","kind":"audio"}),json!({"id":"cc","kind":"cc"}),json!({"id":"m","kind":"midi"})];
        let c=layout(&tracks,[1000.,18.,400.,700.],0.,2);
        assert_eq!(c.len(),16);assert_eq!(c[0].rect,[1014.,64.,36.,36.]);assert_eq!(c[1].key,"arm");
        assert_eq!(c[10].key,"clear");assert_eq!(c[10].rect[1]-c[0].rect[1],144.);
        assert_eq!(c[6..10].iter().map(|x|x.key).collect::<Vec<_>>(),["arm","target_lane","target_device","target_param"]);
        assert_eq!(c[6].text,"A");assert!(!c[6].crosswalk);assert_eq!(c[9].rect[0]-c[6].rect[0],132.);
        assert_eq!(layout(&tracks,[1000.,18.,400.,200.],0.,2).len(),6);
    }

    #[test] fn tutorial_ruby_uses_page_two_and_retained_release() {
        let mut ui=Ui::default();ui.page=1;
        let mut motion=super::super::panel_motion::State::default();motion.set_all_open(true);motion.advance(1.);
        let view=Viewport{width:1600.,height:1000.,canvas_scale:1.};let options=Options::default();
        let doc=json!({"revision":0,"tracks":[]});
        ui.update(&doc,&Input::default(),view,&motion.snapshot(),options,0,false,false,0.).unwrap();
        let c=&ui.presentation()["controls"][0];assert_eq!(c["text"],"LEARN");assert_eq!(c["active"],false);
        let r:[f64;4]=serde_json::from_value(c["rect"].clone()).unwrap();let p=[r[0]+54.,r[1]+18.];
        let input=Input{pointer:Some(p),..Default::default()};
        assert!(ui.update_events(&doc,&input,view,&motion.snapshot(),options,0,false,false,0.,&[(true,p),(false,p)]).unwrap().is_none());
        assert_eq!(ui.update(&doc,&input,view,&motion.snapshot(),options,0,false,false,0.).unwrap(),Some(json!({"learn_toggle":true})));
        ui.set_loop_enabled(true);
        ui.update(&doc,&Input::default(),view,&motion.snapshot(),options,0,false,false,0.).unwrap();
        let c=&ui.presentation()["controls"][1];assert_eq!(c["text"],"LOOP");assert_eq!(c["active"],true);
        let r:[f64;4]=serde_json::from_value(c["rect"].clone()).unwrap();let p=[r[0]+54.,r[1]+18.];
        let input=Input{pointer:Some(p),..Default::default()};
        assert!(ui.update_events(&doc,&input,view,&motion.snapshot(),options,0,false,false,0.,&[(true,p),(false,p)]).unwrap().is_none());
        assert_eq!(ui.update(&doc,&input,view,&motion.snapshot(),options,0,false,false,0.).unwrap(),Some(json!({"loop_toggle":true})));
        ui.page=2;ui.update(&doc,&input,view,&motion.snapshot(),options,0,false,false,0.).unwrap();
        assert!(ui.presentation()["controls"].as_array().unwrap().is_empty());
    }

    #[test] fn release_on_same_button_activates_and_other_page_cancels(){
        let mut ui=Ui::default();let motion=super::super::panel_motion::State::default().snapshot();
        let view=Viewport{width:1600.,height:1000.,canvas_scale:1.};let options=Options::default();
        let doc=json!({"revision":3,"tracks":[{"id":"a","kind":"audio","arm_mode":0,"input_monitor_mode":0}]});
        ui.update(&doc,&Input::default(),view,&motion,options,2,false,false,0.).unwrap();
        let rect:[f64;4]=serde_json::from_value(ui.presentation()["controls"][1]["rect"].clone()).unwrap();
        let mut input=Input{pointer:Some([rect[0]+18.,rect[1]+18.]),left:true,..Default::default()};
        assert!(ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap().is_none());
        input.left=false;
        assert!(ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap().is_none());
        let action=ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap().unwrap();
        assert_eq!(action,json!({"document_revision":3,"id":"a","arm_mode":3}));
        input.left=true;ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap();
        ui.page=1;input.left=false;assert!(ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap().is_none());
        assert!(ui.presentation()["controls"].as_array().unwrap().iter().all(|c|c["key"]!="theatre"));
    }
    #[test] fn automation_lane_order_and_original_labels(){
        let tracks=vec![json!({"id":"m","kind":"midi","name":"Keys"}),json!({"id":"a","kind":"audio"}),json!({"id":"cc","kind":"cc"})];
        let mut lane=CcLane::None;let mut order=Vec::new();for _ in 0..5{order.push(lane.clone());lane=lane.next(&tracks);}
        assert_eq!(order,vec![CcLane::None,CcLane::Workspace,CcLane::Track("a".into()),CcLane::Track("m".into()),CcLane::None]);
        assert_eq!(cc_lane_label(&json!({"cc_target_lane":{"track":"a"}}),&tracks),"AUD1");
        assert_eq!(cc_lane_label(&json!({"cc_target_lane":{"track":"m"}}),&tracks),"Keys");
        assert_eq!(cc_lane_label(&json!({"cc_target_lane":{"kind":"workspace"}}),&tracks),"WORKSPACE");
    }
    #[test] fn cc_parameter_button_opens_original_picker_and_press_selects_real_slot(){
        let mut ui=Ui::default();let motion=super::super::panel_motion::State::default().snapshot();
        let view=Viewport{width:1600.,height:1000.,canvas_scale:1.};let options=Options::default();
        let doc=json!({"revision":9,"tracks":[{"id":"cc","kind":"cc","cc_parameter_slot":0,"cc_parameters":[{"id":7,"label":"CUTOFF"},{"id":11,"label":"RESONANCE"}]}]});
        ui.update(&doc,&Input::default(),view,&motion,options,0,false,false,0.).unwrap();
        let rect:[f64;4]=serde_json::from_value(ui.presentation()["controls"][3]["rect"].clone()).unwrap();
        let mut input=Input{pointer:Some([rect[0]+18.,rect[1]+18.]),left:true,..Default::default()};
        ui.update(&doc,&input,view,&motion,options,0,false,false,0.).unwrap();input.left=false;
        ui.update(&doc,&input,view,&motion,options,0,false,false,0.).unwrap();
        assert!(ui.update(&doc,&input,view,&motion,options,0,false,false,0.).unwrap().is_none());
        assert!(ui.parameter_picker.is_open());
        let scene=ui.presentation()["parameter_picker"].clone();
        let selected=&scene["rows"][1]["rect"];
        input.pointer=Some([selected[0].as_f64().unwrap()+8.,selected[1].as_f64().unwrap()+9.]);input.left=true;
        let action=ui.update(&doc,&input,view,&motion,options,0,false,false,0.).unwrap().unwrap();
        assert_eq!(action,json!({"document_revision":9,"id":"cc","cc_action":"target_param_pick","cc_parameter_slot":1}));
        assert!(!ui.parameter_picker.is_open());
    }
    #[test] fn vm_availability_is_explicit_without_changing_input_label_or_cycle(){
        let mut ui=Ui::default();let motion=super::super::panel_motion::State::default().snapshot();
        let view=Viewport{width:1600.,height:1000.,canvas_scale:1.};let options=Options::default();
        let doc=json!({"revision":0,"tracks":[{"id":"a","kind":"audio","use_virtual_input":true,"physical_input_index":2}]});
        for available in [false,true,false]{
            ui.set_virtual_mic_available(available);
            ui.update(&doc,&Input::default(),view,&motion,options,2,false,false,0.).unwrap();
            let presentation=ui.presentation();let source=&presentation["controls"][0];
            assert_eq!(source["text"],"VM1");assert_eq!(source["source_available"],available);
            assert_eq!(source["unavailable_reason"].is_null(),available);assert_eq!(ui.input_notice().is_none(),available);
            assert_eq!(io(&doc["tracks"][0]).cycle_input(2).input_label(2),"IN1+2");
        }
        ui.page=1;ui.update(&doc,&Input::default(),view,&motion,options,2,false,false,0.).unwrap();assert!(ui.input_notice().is_none());
    }
    #[test] fn invalid_partial_io_patch_is_rejected(){
        let s=TrackIo::new("audio");
        for v in [json!({"arm_mode":4}),json!({"input_monitor_mode":-1}),json!({"stereo_input_pair_12":true,"use_virtual_input":true}),json!({"physical_input_index":999})] {assert!(TrackIo::from_patch(&v,s,"audio").is_err());}
        assert!(TrackIo::from_patch(&json!({"physical_input_index":0}),TrackIo::new("midi"),"midi").is_err());
    }
}

#[cfg(test)]mod integration_regressions {
 use super::*;
 fn fixture()->(Ui,Value,Viewport,Snapshot,Options,Input){
  let(mut ui,view,options)=(Ui::default(),Viewport{width:1600.,height:1000.,canvas_scale:1.},Options::default());
  let motion=super::super::panel_motion::State::default().snapshot();
  let doc=json!({"revision":3,"tracks":[{"id":"a","kind":"audio","arm_mode":0,"input_monitor_mode":0}]});
  ui.update(&doc,&Input::default(),view,&motion,options,2,false,false,0.).unwrap();
  let r:[f64;4]=serde_json::from_value(ui.presentation()["controls"][1]["rect"].clone()).unwrap();
  (ui,doc,view,motion,options,Input{pointer:Some([r[0]+18.,r[1]+18.]),left:true,..Default::default()})
 }
 #[test]fn settled_panel_easing_does_not_cancel_crosswalk_click(){
  let(mut ui,doc,view,_,options,mut input)=fixture();
  let mut motion=super::super::panel_motion::State::default();
  motion.set_all_open(false);
  for _ in 0..240 {motion.advance(1./60.);}
  motion.set_all_open(true);
  for _ in 0..600 {
   motion.advance(1./60.);
   ui.update(&doc,&input,view,&motion.snapshot(),options,2,false,false,0.).unwrap();
   if motion.settled(){break;}
  }
  assert!(motion.settled());
  let r:[f64;4]=serde_json::from_value(ui.presentation()["controls"][1]["rect"].clone()).unwrap();
  input.pointer=Some([r[0]+18.,r[1]+18.]);input.left=false;
  motion.advance(1./60.);ui.update(&doc,&input,view,&motion.snapshot(),options,2,false,false,0.).unwrap();
  input.left=true;
  motion.advance(1./60.);ui.update(&doc,&input,view,&motion.snapshot(),options,2,false,false,0.).unwrap();
  input.left=false;
  motion.advance(1./60.);ui.update(&doc,&input,view,&motion.snapshot(),options,2,false,false,0.).unwrap();
  motion.advance(1./60.);
  let action=ui.update(&doc,&input,view,&motion.snapshot(),options,2,false,false,0.).unwrap();
  assert_eq!(action,Some(json!({"document_revision":3,"id":"a","arm_mode":3})));
 }
 #[test]fn blocked_input_cancels_capture_and_pending_without_rearming_held_press(){
  for pending in [false,true] {
   let(mut ui,doc,view,motion,options,mut input)=fixture();
   ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap();assert!(ui.owns_pointer());
   if pending{input.left=false;ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap();assert!(ui.owns_pointer());}
   assert!(ui.update(&doc,&input,view,&motion,options,2,true,false,0.).unwrap().is_none());assert!(!ui.owns_pointer());
   assert!(ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap().is_none());assert!(!ui.owns_pointer());
   input.left=false;ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap();
   input.left=true;ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap();assert!(ui.owns_pointer());
  }
 }
 #[test]fn inactive_page_and_layout_change_cancel_without_intercepting_other_controls(){
  let(mut ui,doc,view,motion,options,mut input)=fixture();
  ui.page=1;ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap();assert!(!ui.owns_pointer());
  ui.page=0;ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap();assert!(!ui.owns_pointer());
  input.left=false;ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap();
  input.left=true;ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap();assert!(ui.owns_pointer());
  let resized=Viewport{width:1500.,..view};ui.update(&doc,&input,resized,&motion,options,2,false,false,0.).unwrap();assert!(!ui.owns_pointer());
 }
 #[test]fn authoritative_refresh_preserves_next_press_and_uses_new_revision(){
  let(mut ui,mut doc,view,motion,options,mut input)=fixture();
  ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap();input.left=false;
  ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap();
  let r:[f64;4]=serde_json::from_value(ui.presentation()["controls"][2]["rect"].clone()).unwrap();
  input.pointer=Some([r[0]+18.,r[1]+18.]);input.left=true;
  let arm=ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap().unwrap();assert_eq!(arm["arm_mode"],3);
  doc["revision"]=json!(4);doc["tracks"][0]["arm_mode"]=json!(3);
  ui.refresh(&doc,view,&motion,options,2,false,0.).unwrap();assert!(ui.owns_pointer());
  assert_eq!(ui.presentation()["controls"][2]["pressed"],true);assert_eq!(ui.presentation()["controls"][1]["crosswalk_palette"],1);
  input.left=false;assert!(ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap().is_none());
  let solo=ui.update(&doc,&input,view,&motion,options,2,false,false,0.).unwrap().unwrap();assert_eq!(solo,json!({"document_revision":4,"id":"a","solo":true}));
 }
}

#[cfg(test)] mod drum_rack_launcher_tests {
 use super::*;
 #[test] fn empty_right_panel_opens_rack_after_release_delay() {
  let mut ui=Ui::default();ui.page=1;let doc=json!({"revision":0,"tracks":[]});
  let view=Viewport{width:1280.,height:800.,canvas_scale:1.};
  let mut motion=super::super::panel_motion::State::default();motion.set_all_open(true);motion.advance(1.);
  let motion=motion.snapshot();let options=Options::default();
  ui.refresh(&doc,view,&motion,options,0,false,0.).unwrap();
  let packet=ui.presentation();let c=packet["controls"].as_array().unwrap().iter().find(|c|c["key"]=="drum_rack").unwrap();
  let r: [f64;4]=serde_json::from_value(c["rect"].clone()).unwrap();
  let mut input=Input{pointer:Some([r[0]+r[2]*0.5,r[1]+r[3]*0.5]),left:true,..Default::default()};
  assert!(ui.update(&doc,&input,view,&motion,options,0,false,false,0.).unwrap().is_none());
  input.left=false;assert!(ui.update(&doc,&input,view,&motion,options,0,false,false,0.).unwrap().is_none());
  assert_eq!(ui.update(&doc,&input,view,&motion,options,0,false,false,0.).unwrap(),Some(json!({"drum_rack_open":true})));
 }
}

#[cfg(test)] mod theatre_tests {
 use super::*;
 #[test] fn theatre_gem_click_uses_delayed_global_action_without_a_track(){
  let mut ui=Ui::default();ui.page=1;ui.set_theatre(Some(false));
  let doc=json!({"revision":1,"tracks":[]});let view=Viewport{width:1600.,height:1000.,canvas_scale:1.};
  let motion=super::super::panel_motion::State::default().snapshot();let options=Options::default();
  let mut input=Input::default();
  ui.update(&doc,&input,view,&motion,options,0,false,false,0.).unwrap();
  let c=ui.presentation()["controls"][4].clone();assert_eq!(c["text"],"SPEAKERS OFF");assert_eq!(c["crosswalk"],false);
  let r:[f64;4]=serde_json::from_value(c["rect"].clone()).unwrap();let point=[r[0]+r[2]/2.,r[1]+r[3]/2.];input.pointer=Some(point);
  assert!(ui.update_events(&doc,&input,view,&motion,options,0,false,false,0.,&[(true,point),(false,point)]).unwrap().is_none());
  assert!(ui.owns_pointer());
  assert_eq!(ui.update(&doc,&input,view,&motion,options,0,false,false,0.).unwrap(),Some(json!({"theatre_output":true})));
  ui.set_theatre(Some(true));ui.refresh(&doc,view,&motion,options,0,false,0.).unwrap();assert_eq!(ui.presentation()["controls"][4]["text"],"SPEAKERS ON");
  ui.set_theatre(None);ui.refresh(&doc,view,&motion,options,0,false,0.).unwrap();assert!(ui.presentation()["controls"].as_array().unwrap().iter().all(|c|c["key"]!="theatre"));
 }
}

#[cfg(test)] mod wearable_controls_tests {
 use super::*;
 #[test] fn ray_toggle_and_vr_view_use_real_delayed_button_actions(){
  let view=Viewport{width:1280.,height:1000.,canvas_scale:1.};let options=Options::default();let mut m=super::super::panel_motion::State::default();m.set_all_open(true);m.advance(1.);let motion=m.snapshot();let doc=json!({"revision":0,"tracks":[]});
  for (key,expected) in [("audio_rays",json!({"audio_rays":true})),("headset_view",json!({"headset_camera":true}))]{
   let mut ui=Ui::default();ui.page=1;ui.set_headset_view(Some(false));ui.refresh(&doc,view,&motion,options,0,false,0.).unwrap();let packet=ui.presentation();let c=packet["controls"].as_array().unwrap().iter().find(|c|c["key"]==key).unwrap();let r:[f64;4]=serde_json::from_value(c["rect"].clone()).unwrap();let mut input=Input{pointer:Some([r[0]+r[2]/2.,r[1]+r[3]/2.]),left:true,..Default::default()};
   assert!(ui.update(&doc,&input,view,&motion,options,0,false,false,0.).unwrap().is_none());input.left=false;assert!(ui.update(&doc,&input,view,&motion,options,0,false,false,0.).unwrap().is_none());assert_eq!(ui.update(&doc,&input,view,&motion,options,0,false,false,0.).unwrap(),Some(expected));
  }
  let ui=Ui::default();assert!(ui.controls(&[],[0.,0.,300.,800.],0).iter().all(|c|c.key!="headset_view"));
 }
}

#[cfg(test)]mod channel_output_tests{
 use super::*;
 #[test]fn channel_destination_cycles_without_empty_output_or_feedback(){
  let mut channel=json!({"id":"c","kind":"channel","output":"matrix"});
  let source=json!({"id":"source","kind":"audio","output":"c"});
  let matrix=json!({"id":"matrix","kind":"matrix"});
  let tracks=vec![source,matrix,channel.clone()];
  assert_eq!(next_destination(&channel,&tracks),"OFF");
  channel["output"]=json!("OFF");assert_eq!(next_destination(&channel,&tracks),"matrix");
 }
}

#[cfg(all(test,target_os="macos"))]
mod audio_import_button_tests {
    use super::*;
    #[test]
    fn layout_button_replaces_right_panel_imports_and_requires_matching_release() {
        let mut ui=Ui::default();
        let mut motion=super::super::panel_motion::State::default();motion.set_all_open(true);motion.advance(1.);
        let motion=motion.snapshot();let view=Viewport{width:1600.,height:1000.,canvas_scale:1.};let options=Options::default();
        let doc=json!({"revision":7,"tracks":[]});
        ui.refresh(&doc,view,&motion,options,0,false,0.).unwrap();
        let p=ui.presentation();let controls=p["controls"].as_array().unwrap();
        assert!(!controls.iter().any(|c|c["key"]=="import_audio"||c["key"]=="import_plugins"));
        let button=controls.iter().find(|c|c["key"]=="lane_layout").unwrap();
        let r:[f64;4]=serde_json::from_value(button["rect"].clone()).unwrap();
        let mut input=Input{pointer:Some([r[0]+r[2]/2.,r[1]+r[3]/2.]),left:true,..Default::default()};
        assert!(ui.update(&doc,&input,view,&motion,options,0,false,false,0.).unwrap().is_none());
        input.left=false;assert!(ui.update(&doc,&input,view,&motion,options,0,false,false,0.).unwrap().is_none());
        assert_eq!(ui.update(&doc,&input,view,&motion,options,0,false,false,0.).unwrap(),Some(json!({"lane_layout_cycle":true})));
    }
}
