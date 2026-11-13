//! Control-side elapsed-time track arrangement. Layer five dispatches proposals
//! through the document API and synchronizes after every accepted/rejected edit.
//! Picking and presentation share logical point dimensions; no audio callback,
//! file import or inferred tempo. Loop snapping uses the original explicit map.
use super::piano_roll::{Input, Rect};
use serde_json::{Value, json};
use std::{collections::{BTreeSet,BTreeMap},sync::Arc,time::Instant};
// Deterministic Host input tests only; production always uses monotonic wall time.
#[cfg(test)]thread_local!{static TEST_CLICK_CLOCK:std::cell::Cell<Option<f64>>=const{std::cell::Cell::new(None)};}
#[cfg(test)]pub(crate) fn test_click_clock(time:Option<f64>){TEST_CLICK_CLOCK.with(|c|c.set(time));}
struct FadeDrag{id:String,revision:u64,edge:usize,start:f64,duration:f64,values:[f64;2],canvas:Rect}
struct CcDrag {clip:Value,index:usize,revision:u64,canvas:Rect,top:f64,height:f64}
struct ClipClick {id:String,track:String,time:f64}
#[derive(Clone,Debug)]
struct TimeRange {anchor:f64,end:f64,first:usize,last:usize,dragging:bool}
// Cardinal MeterTrackSystem.cpp freeGridSeconds/snapFree, expressed in the
// current document's seconds clock. No inferred global musical tempo.
fn range_grid(seconds:f64)->f64{let target=(seconds/12.).max(0.000001);let decade=10f64.powf(target.log10().floor());[1.,2.,5.,10.].into_iter().map(|m|m*decade).find(|s|*s>=target).unwrap()}



pub(crate) const RULER_HEIGHT: f64 = super::timeline_bars::HEADER_HEIGHT;
pub(crate) const LANE_HEIGHT: f64 = 64.;
// Cardinal DawTrackHandleLayout: 13 px face, 19 px pick width, 12 px lane inset.
pub(crate) const HANDLE_FACE_WIDTH: f64 = 13.;
pub(crate) const HANDLE_WIDTH: f64 = 19.;
pub(crate) const HANDLE_INSET: f64 = 12.;
pub(crate) const BODY_OFFSET: f64 = HANDLE_FACE_WIDTH + HANDLE_INSET;
pub(crate) const MONITOR_GUTTER_WIDTH: f64 = 57.;
pub(crate) const CLIP_MARGIN: f64 = 8.;
pub(crate) const MIN_CLIP_WIDTH: f64 = 6.;
// The scene reserves 32 handle rows in its bounded dynamic geometry packet.
pub(crate) const MAX_VISIBLE_TRACKS: usize = 32;
const MAX_VISIBLE_CLIPS: usize = 128;
const MAX_SECONDS: f64 = 86_400.;

// View-only sizes follow stable track IDs and never touch the audio document.
pub(crate) fn lane_height(track:&Value)->f64 {if let Some(h)=track["display_height"].as_f64(){return h;}track["lane_height"].as_f64().filter(|h|h.is_finite()).unwrap_or(if track["kind"]=="audio"{96.}else{LANE_HEIGHT}).clamp(32.,256.)}
pub(crate) fn lane_top(tracks:&[Value],scroll:usize,row:usize)->f64 {if let Some(top)=tracks.get(row).and_then(|t|t["layout_top"].as_f64()){return top;}tracks.iter().take(row).skip(scroll).map(lane_height).sum::<f64>()-tracks.get(scroll).and_then(|t|t["vertical_remainder"].as_f64()).unwrap_or(0.)}
pub(crate) fn lane_count(tracks:&[Value],scroll:usize,height:f64)->usize {if tracks.iter().any(|t|t["layout_top"].is_number()){return tracks.iter().enumerate().skip(scroll).take(MAX_VISIBLE_TRACKS).filter(|(i,_)|lane_top(tracks,scroll,*i)<height).map(|(i,_)|i-scroll+1).max().unwrap_or(0);}if tracks.get(scroll).is_none_or(|t|t.get("vertical_remainder").is_none()){let mut used=0.;return tracks.iter().skip(scroll).take(MAX_VISIBLE_TRACKS).take_while(|t|{used+=lane_height(t);used<=height}).count();}let mut used=-tracks.get(scroll).and_then(|t|t["vertical_remainder"].as_f64()).unwrap_or(0.);tracks.iter().skip(scroll).take(MAX_VISIBLE_TRACKS).take_while(|t|{let visible=used<height;used+=lane_height(t);visible}).count()}
/// Shared elapsed-time drawing and picking bounds. The right gutter belongs to
/// visual lane monitors and does not represent additional elapsed time.
pub(crate) fn clip_body(canvas: Rect) -> Rect {
    Rect {
        left: canvas.left + BODY_OFFSET,
        top: canvas.top + RULER_HEIGHT,
        width: (canvas.width - BODY_OFFSET - MONITOR_GUTTER_WIDTH).max(0.),
        height: (canvas.height - RULER_HEIGHT).max(0.),
    }
}

#[derive(Debug)]
pub(crate) enum Action {
    Document { hook: &'static str, request: Value },
    OpenPianoRoll(String),
    OpenTrackPlugins(String),
    OpenWaveform(String),
    AudioClipMenu(String),
    OpenAudioPlaylistMenu(String),
    Seek(f64),
    SetLoop(super::timeline_bars::LoopRange),
    Rebase {request:super::timeline_bars::Rebase,revision:u64},
}

#[derive(Clone, Debug)]
struct Drag {
    id: String,
    revision: u64,
    origin_x: f64,
    origin_y: f64,
    click_moved: bool,
    original_start: f64,
    duration: f64,
    preview_start: f64,
    seconds_per_point: f64,
    moved: bool,
    target_row: Option<usize>,
    free_lane: Option<f64>,
    canvas: Rect,
}

#[derive(Clone, Debug)]
struct TrackDrag {
    id: String,
    revision: u64,
    origin_y: f64,
    origin_x: f64,
    original_height: f64,
    last_x: f64,
    last_time: f64,
    edge_direction: f64,
    resize: bool,
    original_index: usize,
    preview_index: usize,
    moved: bool,
    canvas: Rect,
}

fn reordered_tracks(tracks: &[Value], drag: &TrackDrag) -> Vec<Value> {
    if drag.original_index==drag.preview_index||tracks[drag.original_index]["comp_parent"].is_string(){return tracks.to_vec();}
    let mut ordered = tracks.to_vec();
    let parent=&tracks[drag.original_index]["id"];
    let group=ordered.iter().filter(|t|t["id"]==*parent||t["comp_parent"]==*parent).cloned().collect::<Vec<_>>();
    let target=&tracks[drag.preview_index];
    let target_id=target["comp_parent"].as_str().or(target["id"].as_str()).unwrap();
    if target_id==parent.as_str().unwrap(){return tracks.to_vec();}
    ordered.retain(|t|t["id"]!=*parent&&t["comp_parent"]!=*parent);
    let mut index=ordered.iter().position(|t|t["id"]==target_id).unwrap_or(ordered.len());
    if drag.preview_index>drag.original_index {while index<ordered.len()&&(ordered[index]["id"]==target_id||ordered[index]["comp_parent"]==target_id){index+=1;}}
    ordered.splice(index..index,group);ordered
}

/// Equality is conservative: identical keys guarantee identical presentation
/// with a null browser payload. Document identity changes on full content edits,
/// including same-revision replacements; keys retain the allocation while used.
#[derive(Debug)]
pub(crate) struct PresentationKey { document:Arc<()>, state:Value }
impl PartialEq for PresentationKey {
    fn eq(&self,other:&Self)->bool {
        Arc::ptr_eq(&self.document,&other.document)&&self.state==other.state
    }
}

pub(crate) struct Arrangement {
    pub(crate) viewports: super::viewports::State,
    mix_preview: Value,
    all_track_ids: Vec<Value>,
    pub(crate) lane_controls:[bool;3],
    pub(crate) lane_recording:bool,
    pub(crate) lane_time:f64,
    lane_record_tracks:Value,
    lane_fader:Option<(String,f64,u64,Rect,f64)>,
    waveform_key:Option<Value>,
    waveform_details:BTreeMap<String,Value>,
    heights:BTreeMap<String,f64>,
    pub lane_layout:usize,
    free_tops:BTreeMap<String,f64>,
    presentation_document:Arc<()>,
    revision: Option<u64>,
    ready: bool,
    tracks: Vec<Value>,
    clips: Vec<Value>,
    selected_track: Option<String>,
    selected_clip: Option<String>,
    start_seconds: f64,
    seconds_per_width: f64,
    scroll_tracks: usize,
    cc_drag:Option<CcDrag>,
    fade_drag:Option<FadeDrag>,
    drag: Option<Drag>,
    pending: Option<Drag>,
    track_drag: Option<TrackDrag>,
    pending_track: Option<TrackDrag>,
    pointer_capture: bool,
    previous: Input,
    notice: Option<String>,
    last_canvas: Option<Rect>,
    click_epoch: Instant,
    last_clip_click: Option<ClipClick>,
    plugin_press:Option<(String,String,[f64;2])>,
    plugin_click:Option<(String,String,f64)>,
    range:Option<TimeRange>,
    playhead:Option<f64>,
    timeline_zero_seconds:f64,
    musical_time:super::musical_time::Timing,
    bars:super::timeline_bars::Bars,
    bar_time:Option<f64>,
    transport_rate:f64,
    screen_height:f64,
    vertical_zoom:f64,
    vertical_offset:f64,
    vertical_rail:Option<Rect>,
    vertical_drag:Option<([f64;2],f64,f64)>,
}

impl Default for Arrangement {
    fn default() -> Self {
        Self {
            viewports:Default::default(),mix_preview:Value::Null,all_track_ids:Vec::new(),lane_controls:[false;3],lane_fader:None,lane_recording:false,lane_time:0.,lane_record_tracks:Value::Null,
            waveform_key:None,
            waveform_details:BTreeMap::new(),
            heights:BTreeMap::new(),lane_layout:0,free_tops:BTreeMap::new(),
            presentation_document:Arc::new(()),
            revision: None,
            ready: false,
            tracks: Vec::new(),
            clips: Vec::new(),
            selected_track: None,
            selected_clip: None,
            start_seconds: 0.,
            seconds_per_width: 16.,
            scroll_tracks: 0,
            cc_drag:None,
            fade_drag:None,
            drag: None,
            pending: None,
            track_drag: None,
            pending_track: None,
            pointer_capture: false,
            previous: Input::default(),
            notice: None,
            last_canvas: None,
            click_epoch:Instant::now(),
            last_clip_click:None,
            plugin_press:None,plugin_click:None,
            range:None,
            playhead:None,timeline_zero_seconds:0.,musical_time:Default::default(),
            bars:Default::default(),bar_time:None,transport_rate:44100.,screen_height:1080.,vertical_zoom:1.,vertical_offset:0.,vertical_rail:None,vertical_drag:None,
        }
    }
}

fn identity<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control))
        .ok_or_else(|| format!("arrangement: invalid {key}"))
}

fn number(value: &Value, key: &str, low: f64, high: f64) -> Result<f64, String> {
    value[key]
        .as_f64()
        .filter(|v| v.is_finite() && *v >= low && *v <= high)
        .ok_or_else(|| format!("arrangement: invalid {key}"))
}

fn read_snapshot(snapshot: &Value) -> Result<(u64, Vec<Value>, Vec<Value>), String> {
    let revision = snapshot["revision"]
        .as_u64()
        .ok_or("arrangement: missing revision")?;
    let tracks = snapshot["tracks"]
        .as_array()
        .filter(|t| t.len() <= 64)
        .ok_or("arrangement: invalid track list")?;
    let clips = snapshot["clips"]
        .as_array()
        .filter(|c| c.len() <= 512)
        .ok_or("arrangement: invalid clip list")?;
    let mut track_ids = BTreeSet::new();
    for track in tracks {
        if !track_ids.insert(identity(track, "id")?) {
            return Err("arrangement: duplicate track identity".into());
        }
        identity(track, "name")?;
        match track["kind"].as_str() {
            Some("cc") => {}
            Some("audio" | "midi") => {
                number(track, "gain", 0., 16.)?;
                if !track["mute"].is_boolean() || !track["solo"].is_boolean() {
                    return Err("arrangement: invalid track mute/solo".into());
                }
            }
            _ => return Err("arrangement: unsupported track kind".into()),
        }
    }
    let mut clip_ids = BTreeSet::new();
    for clip in clips {
        if !clip_ids.insert(identity(clip, "id")?) {
            return Err("arrangement: duplicate clip identity".into());
        }
        if !(clip["track"]=="" && clip["kind"]=="audio") {
            let track_id=identity(clip,"track")?;
            let track=tracks.iter().find(|t|t["id"]==track_id).ok_or("arrangement: clip refers to an absent track")?;
            if clip["kind"]!=track["kind"]{return Err("arrangement: clip and track kinds differ".into());}
        }
        let start = number(clip, "start_seconds", 0., MAX_SECONDS)?;
        let duration = number(clip, "duration_seconds", f64::MIN_POSITIVE, MAX_SECONDS)?;
        if start + duration > MAX_SECONDS + 1e-9 {
            return Err("arrangement: clip exceeds the document duration limit".into());
        }
    }
    Ok((revision, tracks.clone(), clips.clone()))
}

fn valid_rect(rect: Rect) -> bool {
    [rect.left, rect.top, rect.width, rect.height]
        .iter()
        .all(|v| v.is_finite())
        && clip_body(rect).width > 0.
        && rect.height > RULER_HEIGHT
}

fn same_rect(a: Rect, b: Rect) -> bool {
    [a.left, a.top, a.width, a.height].into_iter().zip([b.left,b.top,b.width,b.height]).all(|(a,b)|(a-b).abs()<=0.25)
}

pub(crate) fn visible_rows(rect: Rect) -> usize {
    (((rect.height - RULER_HEIGHT) / LANE_HEIGHT).floor().max(0.) as usize).min(MAX_VISIBLE_TRACKS)
}

fn contains(rect: Rect, point: [f64; 2]) -> bool {
    point.iter().all(|v| v.is_finite())
        && point[0] >= rect.left
        && point[0] < rect.left + rect.width
        && point[1] >= rect.top
        && point[1] < rect.top + rect.height
}

impl Arrangement {
    pub(crate) fn clip_body(&self,mut canvas:Rect)->Rect {
        canvas.width -= super::timeline_controls::reserve(self.lane_controls);
        let mut body=clip_body(canvas);
        if self.vertical_rail.is_some(){body.left+=32.;body.width=(body.width-32.).max(0.);}
        body
    }
    pub(crate) fn set_lane_controls(&mut self,flags:[bool;3]) {
        if self.lane_controls!=flags {self.cancel_interaction();self.lane_controls=flags;}
    }
    fn visible_rows(&self,rect:Rect)->usize {lane_count(&self.display_tracks(),self.scroll_tracks,self.clip_body(rect).height)}
    fn row_top(&self,row:usize)->f64 {lane_top(&self.display_tracks(),self.scroll_tracks,row)}
    fn height(&self,row:usize)->f64 {lane_height(&self.display_tracks()[row])}
    fn row_from_y(&self,y:f64)->usize {let tracks=self.display_tracks();if self.lane_layout!=0{return tracks.iter().enumerate().skip(self.scroll_tracks).rev().find(|(i,t)|{let top=lane_top(&tracks,self.scroll_tracks,*i);y>=top&&y<top+lane_height(t)}).map(|(i,_)|i).unwrap_or(tracks.len());}let mut top=-tracks.get(self.scroll_tracks).and_then(|t|t["vertical_remainder"].as_f64()).unwrap_or(0.);for (i,t) in tracks.iter().enumerate().skip(self.scroll_tracks){top+=lane_height(t);if y<top{return i;}}tracks.len()}

    pub(crate) fn cycle_lane_layout(&mut self){
        self.cancel_interaction();
        if (self.lane_layout+1)%3==2 {
            let tracks=self.display_tracks();
            for (i,t) in tracks.iter().enumerate(){if let Some(id)=t["id"].as_str(){let top=lane_top(&tracks,self.scroll_tracks,i)/self.vertical_zoom+self.vertical_origin();self.free_tops.entry(id.into()).or_insert(top);}}
        }
        self.lane_layout=(self.lane_layout+1)%3;
    }
    pub(crate) fn set_mix_preview(&mut self, preview:Value) { self.mix_preview=preview; }
    pub(crate) fn sync(&mut self, snapshot: &Value) -> Result<(), String> {
        let incoming = read_snapshot(snapshot).and_then(|parsed| {
            if self.revision.is_some_and(|revision| parsed.0 < revision) {
                Err("arrangement: stale document snapshot".into())
            } else {
                Ok(parsed)
            }
        });
        let (revision, mut tracks, mut clips) = match incoming {
            Ok(parsed) => parsed,
            Err(error) => {
                self.cancel_interaction();
                self.pending = None;
                self.pending_track = None;
                self.ready = false;
                self.notice = Some(error.clone());
                return Err(error);
            }
        };
        self.all_track_ids=tracks.iter().map(|t|t["id"].clone()).collect();
        if snapshot["buses"].as_array().is_some_and(|bs|bs.iter().any(|b|b["kind"]=="mix")) {
            let id=super::mix_preview::ID;
            tracks.push(json!({"id":id,"name":match self.mix_preview["phase"].as_str(){Some("rendering"|"preparing")=>"Master Mix (updating)",Some("failed")=>"Master Mix (unavailable)",_=>"Master Mix (preview)"},"kind":"audio","read_only":true,"gain":1.,"mute":false,"solo":false}));
            if self.mix_preview["revision"]==revision && self.mix_preview["phase"]=="ready" {
                clips.push(json!({"id":id,"track":id,"kind":"audio","read_only":true,"start_seconds":self.mix_preview["start_seconds"],"duration_seconds":self.mix_preview["duration_seconds"],"rainbow_waveform":self.mix_preview["rainbow_waveform"]}));
            }
        }
        self.viewports.sync(&tracks);
        tracks.retain(|t|self.viewports.visible(t["id"].as_str().unwrap_or("")));
        clips.retain(|c|c["track"]==""||self.viewports.visible(c["track"].as_str().unwrap_or("")));
        // View rows are attached to their real audio track, never mixer tracks.
        let parents=tracks.clone();tracks.clear();
        for parent in parents {
            let id=parent["id"].as_str().unwrap();tracks.push(parent.clone());
            if let Some(state)=snapshot["audio_playlists"].as_array().and_then(|a|a.iter().find(|t|t["track"]==id&&t["expanded"]==true)) {
                if let Some(lanes)=state["lanes"].as_array(){for (i,lane) in lanes.iter().enumerate().skip(1){
                    let mut row=parent.clone();let mut key=format!("{id}::take:{i}");
                    while snapshot["tracks"].as_array().unwrap().iter().any(|t|t["id"]==key){key.push(':');}
                    row["id"]=json!(key);row["comp_parent"]=json!(id);row["comp_index"]=json!(i);row["name"]=lane["name"].clone();tracks.push(row);
                    if let Some(takes)=lane["clips"].as_array(){for clip in takes {let mut clip=clip.clone();clip["track"]=json!(key);clip["comp_parent"]=json!(id);clip["comp_index"]=json!(i);clips.push(clip);}}
                }}
            }
        }
        self.lane_record_tracks=snapshot["recording"]["record_enabled_tracks"].clone();
        self.musical_time=super::musical_time::Timing::from_snapshot(&snapshot["musical_time"]);
        self.timeline_zero_seconds=snapshot["timeline_zero_seconds"].as_f64().unwrap_or(0.);
        let changed = self.revision.is_some_and(|old| old != revision)
            || self.tracks != tracks
            || self.clips != clips;
        if self.tracks.iter().map(|t|&t["id"]).ne(tracks.iter().map(|t|&t["id"])) {self.range=None;}
        if changed {self.waveform_key=None;self.waveform_details.clear();self.plugin_press=None;self.plugin_click=None;self.presentation_document=Arc::new(());self.last_clip_click=None;}
        let own_fader_ack=self.lane_fader.as_ref().is_some_and(|(id,_,_,_,gain)| {
            let mut expected=self.tracks.clone();
            for t in &mut expected {if t["id"]==*id||t["comp_parent"]==*id{t["gain"]=json!(gain);}}
            expected==tracks && self.clips==clips
        });
        if own_fader_ack {if let Some(d)=&mut self.lane_fader {d.2=revision;}}
        if self.is_interacting() && changed && !own_fader_ack {
            self.cancel_interaction();
            self.notice =
                Some("Arrangement gesture cancelled because the document changed.".into());
        }
        if let Some(pending) = self.pending.take() {
            let accepted = revision > pending.revision
                && clips.iter().any(|clip| {
                    clip["id"] == pending.id
                        && clip["start_seconds"]
                            .as_f64()
                            .is_some_and(|start| (start - pending.preview_start).abs() <= 1e-9)
                });
            self.notice = if accepted {
                None
            } else {
                Some(
                    "The clip move was rejected or superseded; the document position was restored."
                        .into(),
                )
            };
        }
        if let Some(pending) = self.pending_track.take() {
            let expected = reordered_tracks(&self.tracks, &pending);
            let accepted = revision > pending.revision
                && tracks.len() == expected.len()
                && tracks
                    .iter()
                    .zip(&expected)
                    .all(|(a, b)| a["id"] == b["id"]);
            self.notice = if accepted {
                None
            } else {
                Some("The track reorder was rejected or superseded; the document order was restored.".into())
            };
        }
        self.revision = Some(revision);
        self.ready = true;
        self.tracks = tracks;
        self.clips = clips;
        if self
            .selected_track
            .as_ref()
            .is_some_and(|id| !self.tracks.iter().any(|t| t["id"] == *id))
        {
            self.selected_track = None;
        }
        if self
            .selected_clip
            .as_ref()
            .is_some_and(|id| !self.clips.iter().any(|c| c["id"] == *id))
        {
            self.selected_clip = None;
        }
        self.scroll_tracks = self.scroll_tracks.min(self.tracks.len());
        Ok(())
    }

    /// Cardinal's ordered-lane insertion, projected through our current packed rows.
    /// The ruler, track handles, monitor gutter and hidden canvas are not targets.
    pub(crate) fn drum_pattern_drop(&self,rect:Rect,point:[f64;2])->Result<(String,f64),String>{
        if !self.ready||!valid_rect(rect)||!contains(self.clip_body(rect),point){return Err("Drop the pattern onto its linked MIDI lane".into());}
        let row=self.row_at(point,rect).ok_or("No MIDI lane under pattern")?;let track=&self.tracks[row];
        if track["kind"]!="midi"{return Err("Drum patterns require their MIDI lane".into());}
        Ok((track["id"].as_str().ok_or("MIDI identity missing")?.into(),self.time_at(point[0],rect).clamp(0.,MAX_SECONDS)))
    }
    pub(crate) fn loopback_drop(&self,rect:Rect,point:[f64;2])->Result<(String,f64,f64),String>{
        let target=self.audio_drop(rect,point)?;if target.0.is_empty(){return Err("Loopback copies require an audio track".into());}Ok(target)
    }
    pub(crate) fn audio_drop(&self,rect:Rect,point:[f64;2])->Result<(String,f64,f64),String>{
        if !self.ready||!valid_rect(rect)||!contains(self.clip_body(rect),point){return Err("Drop copied audio onto an audio lane.".into())}
        let Some(row)=self.row_at(point,rect)else{return Ok((String::new(),self.time_at(point[0],rect).clamp(0.,MAX_SECONDS),self.seconds_per_width));};
        let track=&self.tracks[row];if track["read_only"]==true||track["kind"]!="audio"{return Err("Loopback copies require an audio lane.".into())}
        if track["frozen"]==true||track["freeze_state"]=="frozen"{return Err("Frozen lanes cannot accept copied audio.".into())}
        Ok((if track["trackless"]==true{String::new()}else{track["comp_parent"].as_str().or(track["id"].as_str()).ok_or("audio lane identity missing")?.into()},self.time_at(point[0],rect).clamp(0.,MAX_SECONDS),self.seconds_per_width))
    }
    pub(crate) fn comp_drop_slot(&self,rect:Rect,point:[f64;2])->Option<usize>{
        let row=self.row_at(point,rect)?;
        (self.tracks[row]["read_only"]!=true&&self.tracks[row]["trackless"]!=true&&self.tracks[row]["kind"]=="audio").then_some(row)
    }
    pub(crate) fn free_lane_at(&self,rect:Rect,point:[f64;2])->f64 {
        ((self.vertical_origin()+(point[1]-self.clip_body(rect).top)/self.vertical_zoom)/96.).clamp(0.,511.)
    }
    fn vertical_origin(&self)->f64 {
        self.tracks.iter().take(self.scroll_tracks).map(|t|self.heights.get(t["id"].as_str().unwrap_or("")).copied().unwrap_or_else(||lane_height(t))).sum::<f64>()+self.vertical_offset
    }
    pub(crate) fn set_vertical_rail(&mut self,rect:Rect){self.vertical_rail=Some(rect);}
    fn free_bounds(&self,clip:&Value,rect:Rect)->(f64,f64) {
        let lane=self.drag.as_ref().or(self.pending.as_ref()).filter(|d|d.id==clip["id"]).and_then(|d|d.free_lane).unwrap_or_else(||clip["lane"].as_f64().unwrap_or(0.));
        (self.clip_body(rect).top+(lane*96.-self.vertical_origin())*self.vertical_zoom,96.*self.vertical_zoom)
    }
    fn hit_free(&self,point:[f64;2],rect:Rect)->Option<usize>{
        if !contains(self.clip_body(rect),point){return None;}
        self.visible_clip_indices(rect).0.into_iter().rev().find(|i|{
            let c=&self.clips[*i];if c["track"]!=""{return false;}
            let (top,h)=self.free_bounds(c,rect);let margin=CLIP_MARGIN*self.vertical_zoom;
            let x=self.clip_body(rect).left+(self.displayed_clip_start(c)-self.start_seconds)/self.seconds_per_width*self.clip_body(rect).width;
            let w=(c["duration_seconds"].as_f64().unwrap()/self.seconds_per_width*self.clip_body(rect).width).max(MIN_CLIP_WIDTH);
            point[0]>=x&&point[0]<x+w&&point[1]>=top+margin&&point[1]<top+h-margin
        })
    }
    pub(crate) fn comp_lane_at(&self,rect:Rect,point:[f64;2])->usize{self.row_at(point,rect).and_then(|row|self.tracks[row]["comp_index"].as_u64()).unwrap_or(0)as usize}
    pub(crate) fn create_comp_at(&self,row:usize)->Result<Action,String>{
        let t=self.tracks.get(row).filter(|t|t["read_only"]!=true&&t["trackless"]!=true&&t["kind"]=="audio").ok_or("Drop a Comp Lane onto an audio track")?;
        let parent=t["comp_parent"].as_str().or(t["id"].as_str()).unwrap();
        Ok(self.edit("WorkstationDataSystem.AudioPlaylist",json!({"id":parent,"action":"new"})))
    }
    pub(crate) fn selected_insertion_row(&self)->usize {
        self.tracks.iter().position(|t|t["id"].as_str()==self.selected_track()).unwrap_or(self.tracks.len())
    }
    pub(crate) fn parent_insertion_index(&self,index:usize)->usize {
        let Some(track)=self.tracks.get(index) else{return self.all_track_ids.len();};
        let parent=track["comp_parent"].as_str();
        let id=parent.map(Value::from).unwrap_or_else(||track["id"].clone());
        self.all_track_ids.iter().position(|v|*v==id)
            .map(|i|i+usize::from(parent.is_some())).unwrap_or(self.all_track_ids.len())
    }
    pub(crate) fn browser_drop_slot(&self,rect:Rect,point:[f64;2])->Option<usize>{
        if !self.ready || !valid_rect(rect) {return None;}
        let body=self.clip_body(rect);if !contains(body,point){return None;}
        if self.tracks.is_empty(){return Some(0);}
        // Preserve a usable upper insertion gap in the current packed-row layout.
        let row=if point[1]<body.top+CLIP_MARGIN {0}else{self.row_from_y(point[1]-body.top).saturating_sub(self.scroll_tracks)+1};
        Some((self.scroll_tracks+row).min(self.tracks.len()))
    }

    /// Hosted plug-ins attach to an existing timeline lane, not an insertion gap.
    pub(crate) fn browser_plugin_target(&self,rect:Rect,point:[f64;2])->Option<&Value>{
        if !self.ready||!valid_rect(rect)||!point.iter().all(|v|v.is_finite()){return None;}
        let body=self.clip_body(rect);if !contains(body,point){return None;}
        let index=self.row_from_y(point[1]-body.top);
        let target=self.tracks.get(index).or_else(||{
            let selected=self.selected_track.as_deref().or_else(||self.selected_clip.as_deref().and_then(|id|
                self.clips.iter().find(|c|c["id"]==id).and_then(|c|c["track"].as_str())));
            self.tracks.iter().find(|t|t["id"].as_str()==selected)
        });
        target.and_then(|t|t["comp_parent"].as_str().and_then(|id|self.tracks.iter().find(|p|p["id"]==id)).or(Some(t))).filter(|t|t["read_only"]!=true&&t["trackless"]!=true&&(t["kind"]=="audio"||t["kind"]=="midi"))
    }

    /// Cardinal's stock-specific release falls back to the selected lane even
    /// outside the timeline; hosted formats require a release over the timeline.
    pub(crate) fn browser_stock_target(&self,rect:Rect,point:[f64;2])->Option<&Value>{
        if !self.ready||!valid_rect(rect)||!point.iter().all(|v|v.is_finite()){return None;}
        let body=self.clip_body(rect);
        let direct=if contains(body,point){self.tracks.get(self.row_from_y(point[1]-body.top))}else{None};
        direct.or_else(||self.tracks.iter().find(|t|t["id"].as_str()==self.selected_track.as_deref()))
            .and_then(|t|t["comp_parent"].as_str().and_then(|id|self.tracks.iter().find(|p|p["id"]==id)).or(Some(t)))
            .filter(|t|t["read_only"]!=true&&t["trackless"]!=true&&(t["kind"]=="audio"||t["kind"]=="midi"))
    }

    pub(crate) fn rewind_view(&mut self) { self.start_seconds=self.timeline_zero_seconds;self.last_clip_click=None; }

    /// Visible-source detail is computed only on the serialized control side.
    /// Keep the fixed-size GPU packet, replacing overview peaks at close zoom.
    pub(crate) fn refresh_waveforms(&mut self,host:&mut crate::host::Host,canvas:Rect,spectral:bool)->Result<(),String>{
        let requests=self.visible_clip_indices(canvas).0.into_iter().filter_map(|i|{
            let clip=&self.clips[i];if clip["read_only"]==true || clip["kind"]!="audio" || (!spectral && self.seconds_per_width>0.1){return None;}
            let start=self.displayed_clip_start(clip);let duration=clip["duration_seconds"].as_f64()?;
            if duration<=0.{return None;}
            let lo=((self.start_seconds-start)/duration).clamp(0.,1.);
            let hi=((self.start_seconds+self.seconds_per_width-start)/duration).clamp(0.,1.);
            (hi>lo).then(||json!({"document_revision":self.revision,"id":clip["id"],"columns":128,"bins":32,"spectrogram":spectral,"wave_view":if spectral{[0.,1.]}else{[lo,hi]}}))
        }).collect::<Vec<_>>();
        let key=json!(requests);
        if self.waveform_key.as_ref()==Some(&key){return Ok(());}
        self.waveform_details.clear();
        for request in &requests{
            let preview=host.invoke("WorkstationDataSystem.ClipEditorPreview",request.clone())?;
            self.waveform_details.insert(request["id"].as_str().unwrap().into(),preview);
        }
        self.waveform_key=Some(key);Ok(())
    }

    pub(crate) fn selected_track(&self) -> Option<&str> {
        let id=self.selected_track.as_deref()?;
        self.tracks.iter().find(|t|t["id"]==id).and_then(|t|t["comp_parent"].as_str()).or(Some(id))
    }
    pub(crate) fn selected_clip(&self) -> Option<&str> {
        self.selected_clip.as_deref()
    }
    pub(crate) fn is_interacting(&self) -> bool {
        self.vertical_drag.is_some() || self.lane_fader.is_some() || self.cc_drag.is_some() || self.fade_drag.is_some() || self.bars.active() || self.drag.is_some() || self.track_drag.is_some() || self.range.as_ref().is_some_and(|r|r.dragging)
    }
    pub(crate) fn consumes_pointer(&self) -> bool {
        self.pointer_capture
    }

    pub(crate) fn has_range(&self)->bool{self.range.is_some()}
    pub(crate) fn set_playhead(&mut self,time:Option<f64>){self.playhead=time.filter(|t|t.is_finite()&&*t>=0.&&*t<=MAX_SECONDS);}
    pub(crate) fn range_action(&self)->Result<Action,String>{
        let r=self.range.as_ref().filter(|r|!r.dragging&&r.anchor!=r.end).ok_or("Drag an empty timeline area to select a time-and-track range first")?;
        if self.tracks[r.first]["read_only"]==true || self.tracks[r.last]["read_only"]==true {return Err("Master mix preview is read-only".into());}
        Ok(self.edit("WorkstationDataSystem.CreateRangeClips",json!({"first_track":self.tracks[r.first]["id"],"last_track":self.tracks[r.last]["id"],"start_seconds":r.anchor,"end_seconds":r.end})))
    }
    /// Wire only to Cardinal's existing loop_toggle managed control; dragging
    /// the range itself does not toggle playback looping.
    pub(crate) fn accept_rebase(&mut self,snapshot:&Value,request:super::timeline_bars::Rebase)->Result<(),String>{
        // The known rebase revision is the continuation of this captured gesture.
        // Other document changes still cancel capture through ordinary sync.
        let mut bars=std::mem::take(&mut self.bars);let range=self.range.take();
        self.sync(snapshot)?;
        bars.rebased(request.shift,request.range);self.bars=bars;
        self.range=range.map(|mut r|{r.anchor+=request.shift;r.end+=request.shift;r});
        self.start_seconds+=request.shift;
        self.playhead=request.seek.or_else(||self.playhead.map(|p|p+request.shift));
        self.pointer_capture=self.bars.active();self.notice=None;Ok(())
    }
    pub(crate) fn timeline_zero(&self)->f64{self.timeline_zero_seconds}
    pub(crate) fn loop_enabled(&self)->bool{self.bars.range.enabled}
    pub(crate) fn loop_toggle_action(&mut self)->Action {
        self.bars.range.enabled=!self.bars.range.enabled;
        Action::SetLoop(self.bars.range)
    }
    pub(crate) fn set_screen_height(&mut self,height:f64){if height.is_finite()&&height>0.{self.screen_height=height;}}
    pub(crate) fn set_loop_from_transport(&mut self, transport:&Value) {
        if let Some(rate)=transport["sample_rate"].as_f64().filter(|r|r.is_finite()&&*r>0.) {
            self.transport_rate=rate;
            if !self.bars.active() {
                if let (Some(a),Some(b),Some(enabled))=(transport["loop_start_frame"].as_f64(),transport["loop_end_frame"].as_f64(),transport["loop_enabled"].as_bool()) {
                    if a>=0.&&b>a&&b/rate<=MAX_SECONDS {self.bars.range=super::timeline_bars::LoopRange{start:a/rate,end:b/rate,enabled};}
                }
            }
        }
    }
    pub(crate) fn wants_bar_shift(&self, pointer:Option<[f64;2]>)->bool {
        self.vertical_drag.is_some() || self.vertical_rail.zip(pointer).is_some_and(|(r,p)|contains(r,p)) || self.bars.active() || self.last_canvas.zip(pointer).is_some_and(|(canvas,p)|
            self.bars.over_zoom(canvas,self.clip_body(canvas),self.start_seconds,self.seconds_per_width,p))
    }
    fn bar_presentation(&self,canvas:Rect)->Value {
        let g=self.bars.geometry(canvas,self.clip_body(canvas),self.start_seconds,self.seconds_per_width,self.playhead);
        let rect=|r:Rect|[r.left,r.top,r.width,r.height];
        json!({"zoom":rect(g.zoom),"loop":rect(g.loop_face),"playhead_handle":g.playhead.map(rect),
            "vertical":self.vertical_rail.map(rect),"vertical_zoom":self.vertical_zoom,"vertical_offset":self.vertical_origin(),"offset_y":self.bars.offset_y,"loop_start_seconds":self.bars.range.start,
            "loop_end_seconds":self.bars.range.end,"loop_enabled":self.bars.range.enabled})
    }
    pub(crate) fn set_notice(&mut self,text:Option<String>){self.notice=text;}
    pub(crate) fn cancel_interaction(&mut self) {
        self.lane_fader=None;self.vertical_drag=None;
        self.bars.cancel();self.bar_time=None;
        if self.range.as_ref().is_some_and(|r|r.dragging){self.range=None;}
        self.last_clip_click=None;
        self.plugin_press=None;self.plugin_click=None;
        self.cc_drag=None;
        self.fade_drag=None;
        self.drag = None;
        if let Some(d)=self.track_drag.take(){if d.resize{self.heights.insert(d.id,d.original_height);}}
        self.pointer_capture = false;
    }

    pub(crate) fn select_track(&mut self, id: &str) {
        self.cancel_interaction();
        self.selected_clip = None;
        self.selected_track = self
            .tracks
            .iter()
            .find(|t| t["id"] == id)
            .map(|_| id.to_owned());
        if self.selected_track.is_some() {
            self.notice = None;
        }
    }

    pub(crate) fn select_clip(&mut self, id: &str) {
        self.cancel_interaction();
        if let Some(clip) = self.clips.iter().find(|c| c["id"] == id) {
            self.selected_clip = Some(id.to_owned());
            self.selected_track = clip["track"].as_str().map(str::to_owned);
            self.notice = None;
        } else {
            self.selected_clip = None;
        }
    }

    /// Reveal once after an explicit browser selection or creation. Ordinary
    /// updates deliberately preserve the user's manual vertical scroll.
    pub(crate) fn reveal_selected(&mut self, rect: Rect) {
        if !valid_rect(rect) {
            return;
        }
        self.last_canvas = Some(rect);
        let Some(index) = self
            .tracks
            .iter()
            .position(|track| track["id"].as_str() == self.selected_track())
        else {
            return;
        };
        let count = self.visible_rows(rect);
        if count == 0 {
            return;
        }
        let old = self.scroll_tracks;
        if index < self.scroll_tracks {
            self.scroll_tracks = index;
        } else if index >= self.scroll_tracks.saturating_add(count) {
            self.scroll_tracks = index + 1 - count;
        }
        self.scroll_tracks = self
            .scroll_tracks
            .min(self.tracks.len().saturating_sub(count));
        if old != self.scroll_tracks {
            self.cancel_interaction();
        }
    }

    fn row_at(&self, point: [f64; 2], rect: Rect) -> Option<usize> {
        if !contains(rect, point) || point[1] < rect.top + RULER_HEIGHT {
            return None;
        }
        let row=self.row_from_y(point[1]-rect.top-RULER_HEIGHT);
        (row<self.scroll_tracks+self.visible_rows(rect)).then_some(row)
    }

    fn time_at(&self, x: f64, rect: Rect) -> f64 {
        let body = self.clip_body(rect);
        self.start_seconds + (x - body.left) / body.width * self.seconds_per_width
    }

    fn display_tracks(&self) -> Vec<Value> {
        let mut tracks=self.track_drag.as_ref().or(self.pending_track.as_ref()).map_or_else(||self.tracks.clone(),|d|reordered_tracks(&self.tracks,d));
        for t in &mut tracks {if let Some(h)=t["id"].as_str().and_then(|id|self.heights.get(id)){t["lane_height"]=json!(h);}}
        for t in &mut tracks {t["display_height"]=json!(lane_height(t)*self.vertical_zoom);if self.vertical_rail.is_some()||self.vertical_offset!=0.||self.vertical_zoom!=1.{t["vertical_remainder"]=json!(self.vertical_offset*self.vertical_zoom);}}
        if self.lane_layout!=0 {
            let total: f64=tracks.iter().skip(self.scroll_tracks).map(lane_height).sum();
            let available=self.last_canvas.map(|r|self.clip_body(r).height).unwrap_or(0.);
            let mut top=(available-total).max(0.)-tracks.iter().take(self.scroll_tracks).map(lane_height).sum::<f64>();
            let mut fallback=0.;
            let mut parent_delta=0.;
            for t in &mut tracks {
                let h=lane_height(t);
                let position=if self.lane_layout==1{let p=top;top+=h;p}else{
                    let id=t["id"].as_str().unwrap_or("");
                    let p=self.free_tops.get(id).copied().unwrap_or(fallback);
                    let p=if t["comp_parent"].is_string(){p+parent_delta}else{parent_delta=p-fallback;p};
                    fallback+=h/self.vertical_zoom;
                    (p-self.vertical_origin())*self.vertical_zoom
                };
                t["layout_top"]=json!(position);
            }
        }
        tracks
    }

    fn displayed_clip_start(&self, clip: &Value) -> f64 {
        self.drag
            .as_ref()
            .or(self.pending.as_ref())
            .filter(|drag| clip["id"] == drag.id)
            .map_or_else(
                || clip["start_seconds"].as_f64().unwrap(),
                |drag| drag.preview_start,
            )
    }

    /// Keep the last eligible document clips: later clips own overlap priority.
    /// The same bounded set drives tape presentation and pointer picking.
    fn visible_clip_indices(&self, rect: Rect) -> (Vec<usize>, usize) {
        if !valid_rect(rect) {
            return (Vec::new(), 0);
        }
        let tracks = self.display_tracks();
        let track_ids = tracks
            .iter()
            .skip(self.scroll_tracks)
            .take(self.visible_rows(rect))
            .map(|track| track["id"].as_str().unwrap())
            .collect::<BTreeSet<_>>();
        let mut eligible = self
            .clips
            .iter()
            .enumerate()
            .filter_map(|(index, clip)| {
                if clip["track"]=="" {let (y,h)=self.free_bounds(clip,rect);let body=self.clip_body(rect);if y+h<=body.top||y>=body.top+body.height{return None;}} else if !track_ids.contains(clip["track"].as_str().unwrap()) {
                    return None;
                }
                let start = self.displayed_clip_start(clip);
                let end = start + clip["duration_seconds"].as_f64().unwrap();
                (end > self.start_seconds && start < self.start_seconds + self.seconds_per_width)
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        let hidden = eligible.len().saturating_sub(MAX_VISIBLE_CLIPS);
        eligible.drain(..hidden);
        (eligible, hidden)
    }

    fn display_notice(&self, hidden: usize) -> Option<String> {
        self.notice.clone().or_else(|| {
            (self.selected_clip.is_none() && self.tracks.iter().any(|t|t["id"].as_str()==self.selected_track.as_deref() && t["frozen"]==true)).then(||"Frozen track — press F to flatten.".into())
        }).or_else(|| {
            (hidden > 0)
                .then(|| format!("{hidden} clips hidden; zoom in to inspect a smaller time range."))
        })
    }

    fn hit_clip(&self, row: usize, point: [f64; 2], rect: Rect) -> Option<usize> {
        let y = rect.top + RULER_HEIGHT + self.row_top(row);
        if point[1] < y + CLIP_MARGIN.min(self.height(row)*0.25) || point[1] >= y + self.height(row) - CLIP_MARGIN.min(self.height(row)*0.25) {
            return None;
        }
        let body = self.clip_body(rect);
        let left = body.left;
        let width = body.width;
        self.visible_clip_indices(rect)
            .0
            .into_iter()
            .rev()
            .find_map(|index| {
                let clip = &self.clips[index];
                if clip["track"] != self.tracks[row]["id"] {
                    return None;
                }
                let start = clip["start_seconds"].as_f64()?;
                let end = start + clip["duration_seconds"].as_f64()?;
                if end <= self.start_seconds || start >= self.start_seconds + self.seconds_per_width
                {
                    return None;
                }
                let x = (left + (start - self.start_seconds) / self.seconds_per_width * width)
                    .max(left);
                let end_x = (left + (end - self.start_seconds) / self.seconds_per_width * width)
                    .max(x + MIN_CLIP_WIDTH)
                    .min(left + width);
                (point[0] >= x && point[0] < end_x).then_some(index)
            })
    }

    fn edit(&self, hook: &'static str, mut request: Value) -> Action {
        if let Some(clip)=request["id"].as_str().and_then(|id|self.clips.iter().find(|c|c["id"]==id)) {
            let action=match hook {"WorkstationDataSystem.SplitClip" if clip["comp_parent"].is_string()=>Some("split_clip"),"WorkstationDataSystem.MoveClip" if clip["comp_parent"].is_string()||(clip["trackless"]!=true&&request["track"]!=""&&request["lane"].as_u64().unwrap_or(0)>0)=>Some("move_clip"),"WorkstationDataSystem.RemoveClip" if clip["comp_parent"].is_string()=>Some("remove_clip"),"WorkstationDataSystem.SetClipFades" if clip["comp_parent"].is_string()=>Some("fade_clip"),_=>None};
            if let Some(action)=action {
                if clip["kind"]=="audio" {
                    request["clip_id"]=clip["id"].clone();request["id"]=clip["comp_parent"].as_str().map_or_else(||clip["track"].clone(),|p|json!(p));request["action"]=json!(action);
                    if action=="split_clip"{request["at_seconds"]=request["time_seconds"].clone();}
                    request["document_revision"]=json!(self.revision);
                    return Action::Document{hook:"WorkstationDataSystem.AudioPlaylist",request};
                }
            }
        }
        if hook=="WorkstationDataSystem.MoveClip" && request["track"]!="" && !self.clips.iter().any(|c|c["id"]==request["id"]&&c["trackless"]==true){request.as_object_mut().unwrap().remove("lane");}
        request["document_revision"] = json!(self.revision);
        Action::Document { hook, request }
    }

    /// Inputs contain held keys/buttons and wheel deltas. Release commits one
    /// move; pointer loss, Escape and any document revision cancel its preview.
    pub(crate) fn update(&mut self,input:&Input,rect:Rect)->Vec<Action>{
        let now=self.click_epoch.elapsed().as_secs_f64();
        #[cfg(test)]let now=TEST_CLICK_CLOCK.with(|c|c.get()).unwrap_or(now);
        self.update_at(input,rect,now)
    }
    fn update_at(&mut self, input: &Input, rect: Rect, now:f64) -> Vec<Action> {
        let right_pressed=input.right&&!self.previous.right;
        let pressed = input.left && !self.previous.left;
        let right_pressed=input.right&&!self.previous.right;
        let released = !input.left && self.previous.left;
        let delete = input.delete && !self.previous.delete;
        let home = input.home && !self.previous.home;
        let undo=input.undo&&!self.previous.undo;let redo=input.redo&&!self.previous.redo;
        self.previous = input.clone();
        let mut actions = Vec::new();
        let point = input.pointer.filter(|p| p.iter().all(|v| v.is_finite()));
        if point.is_none() {
            self.cancel_interaction();
        }
        if input.escape {self.range=None;}
        if !self.ready || !valid_rect(rect) || input.escape || !now.is_finite() || now<0. {
            self.cancel_interaction();
            return actions;
        }
        if self.last_canvas.is_some_and(|old|!same_rect(old,rect)){
            self.last_clip_click=None;self.plugin_press=None;self.plugin_click=None;
            if self.range.as_ref().is_some_and(|r|r.dragging){self.cancel_interaction();return actions;}
        }
        self.last_canvas = Some(rect);
        if let Some((id, offset, revision, original,previous_gain))=self.lane_fader.clone() {
            if !same_rect(original,rect)||input.pointer.is_none(){self.cancel_interaction();return actions;}
            let p=input.pointer.unwrap();
            if let Some(row)=self.tracks.iter().position(|t|t["id"]==id) {
                let y=rect.top+RULER_HEIGHT+self.row_top(row)+4.;
                let gain=super::timeline_controls::drag_gain(p[1]-offset,y,self.height(row)-8.);
                if (gain-previous_gain).abs()>1e-10 {
                    self.lane_fader.as_mut().unwrap().4=gain;
                    actions.push(Action::Document{hook:"WorkstationDataSystem.SetTrackMix",request:json!({"document_revision":revision,"id":id,"gain":gain})});
                }
            }
            if released {self.lane_fader=None;self.pointer_capture=false;}
            return actions;
        }
        if self
            .drag
            .as_ref()
            .is_some_and(|drag| !same_rect(drag.canvas, rect))
            || self
                .track_drag
                .as_ref()
                .is_some_and(|drag| !same_rect(drag.canvas, rect))
        {
            self.cancel_interaction();
            self.notice =
                Some("Arrangement gesture cancelled because the panel layout changed.".into());
            return actions;
        }
        let visible_rows = self.visible_rows(rect);
        self.scroll_tracks = self
            .scroll_tracks
            .min(self.tracks.len());
        if self.pending.is_some() || self.pending_track.is_some() {
            return actions;
        }
        if !self.is_interacting()&&(undo||redo) {
            actions.push(self.edit(if undo{"WorkstationDataSystem.Undo"}else{"WorkstationDataSystem.Redo"},json!({})));return actions;
        }
        if home && !self.is_interacting() {
            self.start_seconds = self.timeline_zero_seconds;
        }
        if !self.is_interacting() && delete {
            if let Some(id) = &self.selected_clip {
                actions.push(self.edit("WorkstationDataSystem.RemoveClip", json!({"id":id})));
                return actions;
            } else if let Some(id)=&self.selected_track {
                actions.push(self.edit("WorkstationDataSystem.RemoveTrack",json!({"id":id})));
                return actions;
            }
        }
        // Cardinal DawLaneShortcutSystem.cpp:838: creation is a timeline
        // selection command. Never turn it into a fixed clip or a browser button.
        if input.split_clip_pressed && !self.is_interacting() {
            if let (Some(id),Some(time))=(&self.selected_clip,self.range.as_ref().map(|r|r.anchor).or(self.playhead)) {
                actions.push(self.edit("WorkstationDataSystem.SplitClip",json!({"id":id,"time_seconds":time})));
            } else {self.notice=Some("Select an audio clip and place the edit cursor before splitting.".into());}
            return actions;
        }
        if input.create_range_pressed && point.is_some() && !self.is_interacting() {
            match self.range_action() {
                Ok(action)=>{self.notice=None;actions.push(action);},
                Err(error)=>self.notice=Some(error),
            }
            return actions;
        }
        let Some(point) = point else {
            return actions;
        };
        if self.vertical_drag.is_some() || (pressed&&self.vertical_rail.is_some_and(|r|contains(r,point))) {
            if pressed {self.vertical_drag=Some((point,self.vertical_zoom,self.vertical_origin()));self.pointer_capture=true;}
            if let Some((origin,zoom,offset))=self.vertical_drag {
                let body=self.clip_body(rect);let anchor=(origin[1]-body.top)/zoom+offset;
                self.vertical_zoom=(zoom*((point[0]-origin[0])*0.01).exp()).clamp(0.05,8.);
                let next=(anchor-(point[1]-body.top)/self.vertical_zoom).clamp(-49056.,49056.);
                self.scroll_tracks=0;self.vertical_offset=next;
                // Skip fully hidden real rows; the free canvas keeps its continuous origin.
                while self.scroll_tracks<self.tracks.len() {let t=&self.tracks[self.scroll_tracks];let h=self.heights.get(t["id"].as_str().unwrap_or("")).copied().unwrap_or_else(||lane_height(t));if self.vertical_offset<h{break;}self.vertical_offset-=h;self.scroll_tracks+=1;}
            }
            if released{self.vertical_drag=None;self.pointer_capture=false;}
            return actions;
        }
        // Display-only master rows never acquire edit, drag, record or plugin focus.
        if !self.is_interacting() && self.hit_free(point,rect).is_none() && self.row_at(point,rect).is_some_and(|row|self.tracks[row]["read_only"]==true) && input.wheel==[0.,0.] { if released {self.cancel_interaction();} return actions; }
        if !self.is_interacting() && self.row_at(point,rect).is_some_and(|r|self.tracks[r]["trackless"]==true)&&point[0]<rect.left+BODY_OFFSET{return actions;}
        // Release-based double-clicks share lane/clip picking with the arrangement.
        // A drag, modifier, focus loss or document/layout change breaks the pair.
        let plugin_target=self.hit_free(point,rect).map(|i|(String::new(),format!("clip:{}",self.clips[i]["id"].as_str().unwrap()))).or_else(||self.row_at(point,rect).filter(|_|point[0]<self.clip_body(rect).left+self.clip_body(rect).width).map(|row|{
            let track=self.tracks[row]["comp_parent"].as_str().or(self.tracks[row]["id"].as_str()).unwrap().to_owned();
            let target=if point[0]<rect.left+HANDLE_WIDTH {"handle".to_owned()}
                else if let Some(clip)=self.hit_clip(row,point,rect){format!("clip:{}",self.clips[clip]["id"].as_str().unwrap())}
                else {"lane".to_owned()};
            (track,target)
        }));
        if right_pressed || input.command || input.shift || input.alt || input.wheel!=[0.,0.] {
            self.plugin_press=None;self.plugin_click=None;
        } else {
            if pressed {
                self.plugin_press=plugin_target.clone().map(|(track,target)|(track,target,point));
                if self.plugin_press.is_none(){self.plugin_click=None;}
            }
            if self.plugin_press.as_ref().is_some_and(|(_,_,origin)|(point[0]-origin[0]).abs()>3.||(point[1]-origin[1]).abs()>3.) {
                self.plugin_press=None;self.plugin_click=None;
            }
            if released {
                if let Some((track,target,_))=self.plugin_press.take().filter(|(track,target,_)|plugin_target.as_ref()==Some(&(track.clone(),target.clone()))) {
                    let open=self.plugin_click.as_ref().is_some_and(|(t,k,time)|*t==track&&*k==target&&now>=*time&&now-*time<0.35);
                    if open {
                        self.cancel_interaction();self.select_track(&track);
                        if let Some(id)=target.strip_prefix("clip:").filter(|id|self.clips.iter().any(|c|c["id"]==*id&&c["kind"]=="audio")) {
                            let id=id.to_owned();self.select_clip(&id);actions.push(Action::OpenWaveform(id));
                        } else if !track.starts_with('\u{1f}') {actions.push(Action::OpenTrackPlugins(track));}return actions;
                    }
                    self.plugin_click=Some((track,target,now));
                } else {self.plugin_click=None;}
            }
        }
        if input.loop_range_pressed && !self.is_interacting() {
            if let Some(r)=&self.range {
                if self.bars.select_loop(r.anchor,r.end) {actions.push(Action::SetLoop(self.bars.range));}
            }
            return actions;
        }
        if let Some(d)=&mut self.cc_drag {
            if !same_rect(d.canvas,rect){self.cancel_interaction();return actions;}
            let body=super::timeline_controls::clip_body(rect,self.lane_controls);
            let start=d.clip["start_seconds"].as_f64().unwrap();
            let duration=d.clip["duration_seconds"].as_f64().unwrap();
            let time=(self.start_seconds+(point[0]-body.left)/body.width*self.seconds_per_width-start).clamp(0.,duration);
            d.clip["points"][d.index]=json!({"offset_seconds":time,"value":(1.-(point[1]-d.top)/d.height).clamp(0.,1.)});
            if released {let d=self.cc_drag.take().unwrap();self.pointer_capture=false;
                let mut request=d.clip;request["document_revision"]=json!(d.revision);
                actions.push(Action::Document{hook:"WorkstationDataSystem.PutClip",request});}
            return actions;
        }
        if let Some(d)=&mut self.fade_drag {
            if !same_rect(d.canvas,rect){self.cancel_interaction();return actions;}
            let body=super::timeline_controls::clip_body(rect,self.lane_controls);let time=self.start_seconds+(point[0]-body.left)/body.width*self.seconds_per_width;
            d.values[d.edge]=if d.edge==0{time-d.start}else{d.start+d.duration-time}.clamp(0.,d.duration);
            if released {let d=self.fade_drag.take().unwrap();self.pointer_capture=false;
                actions.push(self.edit("WorkstationDataSystem.SetClipFades",json!({"id":d.id,"fade_in_seconds":d.values[0],"fade_out_seconds":d.values[1]})));}
            return actions;
        }
        if !self.drag.is_some() && !self.track_drag.is_some() && !self.range.as_ref().is_some_and(|r|r.dragging) {
            let dt=self.bar_time.map_or(0.,|last|(now-last).max(0.));self.bar_time=Some(now);
            let map=self.musical_time.for_clock(self.transport_rate,self.timeline_zero_seconds);
            let offset=self.start_seconds;let span=self.seconds_per_width;
            self.bars.rebase_quantum_seconds=self.musical_time.quantum();
            let update=self.bars.update(input,pressed,rect,self.clip_body(rect),&mut self.start_seconds,
                &mut self.seconds_per_width,self.playhead,self.transport_rate,dt,
                self.screen_height,|time|map.snap_seconds_signed(time,span,offset,&self.musical_time.grid_choice,self.musical_time.quantum()));
            if update.consumed {
                self.pointer_capture=self.bars.active();self.last_clip_click=None;
                if let Some(request)=update.rebase {
                    actions.push(Action::Rebase{request,revision:self.revision.unwrap()});return actions;
                }
                if let Some(time)=update.seek {actions.push(Action::Seek(time));}
                if update.loop_changed {actions.push(Action::SetLoop(self.bars.range));}
                return actions;
            }
        }
        if self.range.as_ref().is_some_and(|r|r.dragging){
            let body=self.clip_body(rect);
            let raw=self.time_at(point[0].clamp(body.left,body.left+body.width),rect).clamp(0.,MAX_SECONDS);
            let time=if input.command||self.selected_clip.as_ref().is_some_and(|id|self.clips.iter().any(|c|c["id"]==*id&&c["kind"]=="audio")){raw}else{let grid=range_grid(self.seconds_per_width);(raw/grid).round()*grid}.clamp(0.,MAX_SECONDS);
            let row=self.row_from_y(point[1]-body.top).min(self.tracks.len().saturating_sub(1));
            let r=self.range.as_mut().unwrap();r.end=time;r.last=row;
            if released {r.dragging=false;self.pointer_capture=false;if r.anchor==r.end&&r.first==r.last{self.range=None;}}
            return actions;
        }
        let target_row=self.row_from_y(point[1]-rect.top-RULER_HEIGHT).min(self.tracks.len().saturating_sub(1));
        let free_drag_top=self.track_drag.as_ref().filter(|_|self.lane_layout==2).map(|d|self.free_tops.get(&d.id).copied().unwrap_or_else(||self.row_top(d.original_index)/self.vertical_zoom+self.vertical_origin()));
        if let Some(drag) = &mut self.track_drag {
            let frame_dx=point[0]-drag.last_x;
            if frame_dx.abs()>0.001 {drag.edge_direction=frame_dx.signum();}
            
            drag.last_x=point[0];drag.last_time=now;
            let dx=point[0]-drag.origin_x;let dy=point[1]-drag.origin_y;
            let child=self.tracks[drag.original_index]["comp_parent"].is_string();
            if !drag.moved&&!drag.resize {if dx.abs()>4.&&dx.abs()>dy.abs(){drag.resize=true;}else if dy.abs()>4.&&!child{drag.moved=true;}}
            if drag.resize {
                // Native virtual pointer coordinates already continue beyond every
                // screen edge. Do not add synthetic movement while it is stationary.
                let height=(drag.original_height+dx/self.vertical_zoom).clamp(32.,256.);
                self.heights.insert(drag.id.clone(),height);
                // Discard overshoot at a limit so reversing responds immediately.
                drag.origin_x=point[0]-(height-drag.original_height)*self.vertical_zoom;
            }
            if drag.moved {
                if let Some(top)=free_drag_top {
                    self.free_tops.insert(drag.id.clone(),(top+dy/self.vertical_zoom).max(0.));
                    drag.origin_y=point[1];
                }else{drag.preview_index=target_row;}
            }
            if released {
                let drag = self.track_drag.take().unwrap();
                self.pointer_capture = false;
                if drag.moved && drag.preview_index != drag.original_index {
                    let visible_ids = reordered_tracks(&self.tracks, &drag)
                        .iter()
                        .filter(|track|track["read_only"]!=true&&track["trackless"]!=true&&!track["comp_parent"].is_string())
                        .map(|track| track["id"].clone())
                        .collect::<Vec<_>>();
                    let mut visible=visible_ids.into_iter();
                    let ids=self.all_track_ids.iter().map(|id|if self.viewports.visible(id.as_str().unwrap_or("")){visible.next().unwrap_or_else(||id.clone())}else{id.clone()}).collect::<Vec<_>>();
                    actions.push(Action::Document {
                        hook: "WorkstationDataSystem.ReorderTracks",
                        request: json!({"document_revision":drag.revision,"ids":ids}),
                    });
                    self.pending_track = Some(drag);
                }
            }
            return actions;
        }
        let drop_target=self.row_at(point,rect).filter(|row|{
            let Some(d)=&self.drag else{return false;};let Some(c)=self.clips.iter().find(|c|c["id"]==d.id)else{return false;};
            
            let parent=c["comp_parent"].as_str().or(c["track"].as_str());let t=&self.tracks[*row];
            t["kind"]=="audio"&&(t["read_only"]!=true)&&((t["trackless"]==true&&!c["comp_parent"].is_string())||t["comp_parent"].as_str().or(t["id"].as_str())==parent||c["trackless"]==true)
        });
        let free_position=self.free_lane_at(rect,point);
        let vertical_zoom=self.vertical_zoom;
        if let Some(drag) = &mut self.drag {
            drag.target_row=drop_target;
            if let Some(lane)=&mut drag.free_lane {let original=self.clips.iter().find(|c|c["id"]==drag.id).unwrap();*lane=(original["lane"].as_f64().unwrap_or(0.)+(point[1]-drag.origin_y)/vertical_zoom/96.).clamp(0.,511.);}
            let delta = point[0] - drag.origin_x;
            // Cardinal MidiLaneSystem: movement >3 points in either axis is
            // not a click. Preserve our existing horizontal move threshold.
            if delta.abs()>3. || (point[1]-drag.origin_y).abs()>3. {drag.click_moved=true;}
            if delta.abs() >= 3. || (point[1]-drag.origin_y).abs()>3. {
                drag.moved = true;
            }
            if drag.moved {
                drag.preview_start = (drag.original_start + delta * drag.seconds_per_point)
                    .clamp(0., MAX_SECONDS - drag.duration);
            }
            if released {
                let drag = self.drag.take().unwrap();
                self.pointer_capture = false;
                if drag.moved {
                    self.last_clip_click=None;
                    let original=self.clips.iter().find(|c|c["id"]==drag.id).unwrap();
                    let target=drag.target_row.map(|row|&self.tracks[row]);
                    let free=target.is_none() && (original["track"]==""||(!original["comp_parent"].is_string()&&self.row_at(point,rect).is_none()));
                    let lane=if free{drag.free_lane.unwrap_or(free_position)}else{target.map(|t|t["comp_index"].as_u64().unwrap_or(0)as f64).unwrap_or(original["comp_index"].as_u64().unwrap_or(0)as f64)};
                    let track=if free{""}else{target.and_then(|t|t["comp_parent"].as_str().or(t["id"].as_str())).unwrap_or(original["track"].as_str().unwrap_or(""))};
                    if (drag.preview_start-drag.original_start).abs()>1e-9 || (lane-original[if free{"lane"}else{"comp_index"}].as_f64().unwrap_or(0.)).abs()>1e-9 || track!=original["track"].as_str().unwrap_or("") {
                        let mut request=json!({"id":drag.id,"start_seconds":drag.preview_start,"lane":if free{json!(lane)}else{json!(lane as u64)}});
                        if track!=original["track"].as_str().unwrap_or("")||free {request["track"]=json!(track);}
                        actions.push(self.edit("WorkstationDataSystem.MoveClip",request));
                        self.pending=Some(drag);
                    }
                }else if !drag.click_moved {
                    if let Some(clip)=self.clips.iter().find(|c|c["id"]==drag.id && c["kind"]=="midi") {
                        let track=clip["track"].as_str().unwrap().to_owned();
                        // Cardinal compares release times, same track and clip,
                        // with a strict <0.35s window (not an OS preference).
                        let open=self.last_clip_click.as_ref().is_some_and(|last|last.id==drag.id && last.track==track && now>=last.time && now-last.time<0.35);
                        self.last_clip_click=Some(ClipClick{id:drag.id.clone(),track,time:now});
                        if open && input.alt {actions.push(Action::OpenPianoRoll(drag.id));}
                    }else{self.last_clip_click=None;}
                }else{self.last_clip_click=None;}
            }
            return actions;
        }
        if released {
            self.pointer_capture = false;
        }
        if !contains(rect, point) {
            if pressed{self.last_clip_click=None;}
            return actions;
        }
        let body = self.clip_body(rect);
        if point[0] >= body.left + body.width {
            if pressed {
                if let Some(row)=self.row_at(point,rect) {
                    let track=&self.tracks[row];
                    if track["trackless"]!=true && !track["comp_parent"].is_string() && matches!(track["kind"].as_str(),Some("audio"|"midi")) {
                        let y=rect.top+RULER_HEIGHT+self.row_top(row)+4.;
                        let h=self.height(row)-8.;
                        let (fader,arm)=super::timeline_controls::columns(body.left+body.width+MONITOR_GUTTER_WIDTH,y,h,self.lane_controls);
                        if self.lane_controls[2] && super::track_io::contains(arm,point) {
                            actions.push(self.edit("WorkstationDataSystem.SetTrackMix",json!({"id":track["id"],"arm_mode":super::track_io::next_arm(track["arm_mode"].as_u64().unwrap_or(0) as u8)})));
                            self.pointer_capture=true;return actions;
                        }
                        if self.lane_controls[1] && super::track_io::contains(fader,point) {
                            let knob=super::timeline_controls::knob(fader,track["gain"].as_f64().unwrap_or(1.));
                            let offset=if super::track_io::contains(knob,point){point[1]-knob[1]-knob[3]*0.5}else{0.};
                            self.lane_fader=Some((track["id"].as_str().unwrap().into(),offset,self.revision.unwrap(),rect,track["gain"].as_f64().unwrap_or(1.)));
                            self.pointer_capture=true;return actions;
                        }
                    }
                }
            }
            if pressed {
                self.last_clip_click=None;
                if let Some(row)=self.row_at(point,rect) {
                    let track=&self.tracks[row];
                    if track["trackless"]!=true&&!track["comp_parent"].is_string()&&matches!(track["kind"].as_str(),Some("audio"|"midi")) {
                        let center=rect.top+RULER_HEIGHT+self.row_top(row)+self.height(row)*0.5;
                        if let Some(mode)=super::input_monitor::hit_mode(body.left+body.width,center,self.height(row),point) {
                            actions.push(self.edit("WorkstationDataSystem.SetTrackMix",json!({"id":track["id"],"input_monitor_mode":mode})));
                            self.pointer_capture=true;
                        }
                    }
                }
            }
            return actions;
        }
        let row = self.row_at(point, rect);
        let local_x = point[0] - rect.left;
        let local_y = row.map(|row| {
            point[1] - rect.top - RULER_HEIGHT - self.row_top(row)
        });
        // DawLaneInputSystem opens the track context menu from the lane header.
        if right_pressed&&!self.is_interacting()&&local_x<HANDLE_WIDTH {
            if let Some(row)=row.filter(|row|self.tracks[*row]["kind"]=="audio") {
                let id=self.tracks[row]["comp_parent"].as_str().or(self.tracks[row]["id"].as_str()).unwrap().to_owned();
                self.select_track(&id);self.last_clip_click=None;
                actions.push(self.edit("WorkstationDataSystem.AudioPlaylist",json!({"id":id,"action":"toggle_lanes"})));
                return actions;
            }
        }
        let wheel = input.wheel.map(|delta| {
            if delta.is_finite() {
                delta.clamp(-100., 100.)
            } else {
                0.
            }
        });
        if wheel != [0., 0.] && point[1] >= body.top {
            self.last_clip_click=None;
            if local_x < HANDLE_WIDTH {
                self.scroll_tracks = (self.scroll_tracks as f64 + wheel[1].signum())
                    .max(0.)
                    .min(self.tracks.len().saturating_sub(visible_rows) as f64)
                    as usize;
            } else if local_x >= BODY_OFFSET {
                if input.command && wheel[1] != 0. {
                    let anchor = self.time_at(point[0], rect);
                    let fraction = (point[0] - body.left) / body.width;
                    self.seconds_per_width =
                        (self.seconds_per_width * 1.18f64.powf(wheel[1])).clamp(super::timeline_bars::MIN_SECONDS, super::timeline_bars::MAX_SECONDS);
                    self.start_seconds = anchor - fraction * self.seconds_per_width;
                } else {
                    self.start_seconds +=
                        (wheel[0] + wheel[1]) * 40. / body.width * self.seconds_per_width;
                }
                self.start_seconds = self
                    .start_seconds
                    .clamp(-MAX_SECONDS, MAX_SECONDS - self.seconds_per_width);
            }
        }
        // DawLaneInputSystem:1310..1365: right-click selects an audio clip
        // and exposes its editor actions; the lane handle owns the track menu.
        if right_pressed && !self.is_interacting() && local_x >= HANDLE_WIDTH {
            if let Some(index) = self.hit_free(point,rect).or_else(||row.and_then(|row| self.hit_clip(row, point, rect))) {
                if self.clips[index]["kind"]=="audio" {
                    let id=self.clips[index]["id"].as_str().unwrap().to_owned();
                    let take=self.clips[index]["comp_parent"].is_string();
                    self.select_clip(&id);
                    if take{self.notice=Some("Option-click to slice a take; drag pieces to the main lane to use them.".into());}else{actions.push(Action::AudioClipMenu(id));}
                }
            }
            return actions;
        }
        if !pressed {
            return actions;
        }
        self.notice = None;
        self.range=None;
        if row.and_then(|r|self.hit_clip(r,point,rect)).is_none(){self.last_clip_click=None;}
        self.pointer_capture = true;
        if point[1] < rect.top + RULER_HEIGHT {return actions;}
        if let Some(index)=self.hit_free(point,rect) {
            let c=&self.clips[index];let id=c["id"].as_str().unwrap().to_owned();
            self.selected_track=None;self.selected_clip=Some(id.clone());
            let (top,_)=self.free_bounds(c,rect);let start=c["start_seconds"].as_f64().unwrap();let duration=c["duration_seconds"].as_f64().unwrap();
            let values=[c["fade_in_seconds"].as_f64().unwrap_or(0.),c["fade_out_seconds"].as_f64().unwrap_or(0.)];
            let x=|t:f64|body.left+(t-self.start_seconds)/self.seconds_per_width*body.width;
            let handles=[x(start+values[0]),x(start+duration-values[1])];
            let edge=if (point[0]-handles[0]).abs()<=(point[0]-handles[1]).abs(){0}else{1};
            if (point[1]-top-CLIP_MARGIN*self.vertical_zoom).abs()<=6.&&(point[0]-handles[edge]).abs()<=6. {
                self.fade_drag=Some(FadeDrag{id,revision:self.revision.unwrap(),edge,start,duration,values,canvas:rect});return actions;
            }
            self.drag=Some(Drag{id,revision:self.revision.unwrap(),origin_x:point[0],origin_y:point[1],click_moved:false,original_start:start,duration,preview_start:start,seconds_per_point:self.seconds_per_width/body.width,moved:false,target_row:None,free_lane:Some(c["lane"].as_f64().unwrap_or(0.)),canvas:rect});
            return actions;
        }
        let Some(row) = row else {
            self.selected_clip = None;
            if point[0]>=body.left {actions.push(Action::Seek(self.time_at(point[0],rect).clamp(0.,MAX_SECONDS)));}
            return actions;
        };
        let id = self.tracks[row]["id"].as_str().unwrap().to_owned();
        if local_x < HANDLE_WIDTH {
            let press=self.plugin_press.take();let click=self.plugin_click.take();
            self.select_track(&id);
            self.plugin_press=press;self.plugin_click=click;
            self.pointer_capture = true;
            self.track_drag = Some(TrackDrag {
                id: id.clone(),
                revision: self.revision.unwrap(),
                origin_y: point[1],
                origin_x:point[0],original_height:self.height(row)/self.vertical_zoom,last_x:point[0],last_time:now,edge_direction:0.,resize:false,
                original_index: row,
                preview_index: row,
                moved: false,
                canvas: rect,
            });
        } else {
            if let Some(index) = self.hit_clip(row, point, rect) {
                let clip = &self.clips[index];
                let id = clip["id"].as_str().unwrap().to_owned();
                let start = clip["start_seconds"].as_f64().unwrap();
                self.selected_track = Some(clip["track"].as_str().unwrap().into());
                self.selected_clip = Some(id.clone());
                if clip["kind"]=="cc" {
                    let top=body.top+self.row_top(row)+CLIP_MARGIN+10.;
                    let height=(self.height(row)-2.*CLIP_MARGIN-10.).max(1.);
                    if point[1]>=top {
                        let mut draft=clip.clone();
                        if !draft["points"].is_array(){draft["points"]=json!([]);}
                        let points=draft["points"].as_array_mut().unwrap();
                        let nearest=points.iter().enumerate().filter_map(|(i,p)|{
                            let x=body.left+(start+p["offset_seconds"].as_f64()? - self.start_seconds)/self.seconds_per_width*body.width;
                            let y=top+height*(1.-p["value"].as_f64()?);
                            let distance=(x-point[0]).hypot(y-point[1]);
                            (distance<=9.).then_some((i,distance))
                        }).min_by(|a,b|a.1.total_cmp(&b.1)).map(|p|p.0);
                        if input.alt {
                            if let Some(i)=nearest {points.remove(i);draft["document_revision"]=json!(self.revision.unwrap());
                                actions.push(Action::Document{hook:"WorkstationDataSystem.PutClip",request:draft});}
                            self.pointer_capture=false;return actions;
                        }
                        let index=if let Some(i)=nearest{i}else{
                            if points.len()>=2048{self.notice=Some("CC clip point limit reached.".into());self.pointer_capture=false;return actions;}
                            points.push(json!({"offset_seconds":(self.time_at(point[0],rect)-start).clamp(0.,clip["duration_seconds"].as_f64().unwrap()),"value":(1.-(point[1]-top)/height).clamp(0.,1.)}));points.len()-1
                        };
                        self.cc_drag=Some(CcDrag{clip:draft,index,revision:self.revision.unwrap(),canvas:rect,top,height});return actions;
                    }
                }
                if clip["kind"]=="audio" {
                    if input.alt {
                        let parent=clip["comp_parent"].as_str().or(clip["track"].as_str()).unwrap();
                        actions.push(self.edit("WorkstationDataSystem.AudioPlaylist",json!({"id":parent,"clip_id":id,"action":"split_clip","at_seconds":self.time_at(point[0],rect)})));
                        self.pointer_capture=false;return actions;
                    }
                    let duration=clip["duration_seconds"].as_f64().unwrap();
                    let values=[clip["fade_in_seconds"].as_f64().unwrap_or(0.),clip["fade_out_seconds"].as_f64().unwrap_or(0.)];
                    let top=body.top+self.row_top(row)+CLIP_MARGIN;
                    let x=|t:f64|body.left+(t-self.start_seconds)/self.seconds_per_width*body.width;
                    let handles=[x(start+values[0]),x(start+duration-values[1])];
                    if (point[1]-top).abs()<=9. {
                        let edge=if (point[0]-handles[0]).abs()<=(point[0]-handles[1]).abs(){0}else{1};
                        if (point[0]-handles[edge]).abs()<=9. {
                            self.fade_drag=Some(FadeDrag{id,revision:self.revision.unwrap(),edge,start,duration,values,canvas:rect});return actions;
                        }
                    }
                }
                if clip["kind"]=="audio" {
                    let y=point[1]-(body.top+self.row_top(row)+CLIP_MARGIN);
                    let height=self.height(row)-2.*CLIP_MARGIN;
                    let lip=12f64.min(height*0.25);
                    if y>lip && y<height-lip {
                        let raw=self.time_at(point[0],rect).clamp(0.,MAX_SECONDS);
                        let time=raw;
                        self.range=Some(TimeRange{anchor:time,end:time,first:row,last:row,dragging:true});
                        self.last_clip_click=None;
                        actions.push(Action::Seek(time));
                        return actions;
                    }
                }
                self.drag = Some(Drag {
                    id,
                    revision: self.revision.unwrap(),
                    origin_x: point[0],
                    origin_y: point[1],
                    click_moved:false,
                    original_start: start,
                    duration: clip["duration_seconds"].as_f64().unwrap(),
                    preview_start: start,
                    seconds_per_point: self.seconds_per_width / body.width,
                    moved: false,
                    target_row:Some(row),free_lane:None,
                    canvas: rect,
                });
            } else {
                self.selected_track = Some(id);
                self.selected_clip = None;
                // Existing tape/handle/ruler picking wins. Only the empty lane
                // body (within its current tape body) starts Cardinal range selection.
                if local_y.is_some_and(|y|(CLIP_MARGIN..self.height(row)-CLIP_MARGIN).contains(&y)){
                    let raw=self.time_at(point[0],rect).clamp(0.,MAX_SECONDS);
                    let grid=range_grid(self.seconds_per_width);
                    let snapped=((raw/grid).round()*grid).clamp(0.,MAX_SECONDS);
                    let anchor=if input.command{self.playhead.unwrap_or(0.)}else{snapped};
                    self.range=Some(TimeRange{anchor,end:if input.command{raw}else{snapped},first:row,last:row,dragging:true});
                    if !input.command&&self.playhead.is_some(){actions.push(Action::Seek(snapped));}
                }
            }
        }
        actions
    }

    pub(crate) fn status(&self) -> Value {
        let hidden = self
            .last_canvas
            .map_or(0, |rect| self.visible_clip_indices(rect).1);
        json!({"ready":self.ready,"revision":self.revision,
            "selected_track":self.selected_track,"selected_clip":self.selected_clip,
            "gesture_active":self.is_interacting(),"pending_commit":self.pending.is_some() || self.pending_track.is_some(),
            "start_seconds":self.start_seconds,"seconds_per_width":self.seconds_per_width,
            "timeline_bars":self.last_canvas.map(|canvas|self.bar_presentation(canvas)),
            "scroll_tracks":self.scroll_tracks,"hidden_clip_count":hidden,"notice":self.display_notice(hidden),
            "range":self.range.as_ref().map(|r|json!({"start_seconds":r.anchor.min(r.end),"end_seconds":r.anchor.max(r.end),"first_track":self.tracks[r.first]["id"],"last_track":self.tracks[r.last]["id"],"dragging":r.dragging}))})
    }

    pub(crate) fn range_overlay(&self,canvas:Rect)->Value{
        let mut overlay=self.selection_overlay(canvas);
        if !valid_rect(canvas)||!self.ready{return overlay;}
        let body=self.clip_body(canvas);let mut fades=Vec::new();
        for index in self.visible_clip_indices(canvas).0 {
            let c=&self.clips[index];if c["read_only"]==true||c["kind"]!="audio"{continue;}
            let bounds=if c["track"]=="" {self.free_bounds(c,canvas)}else{let Some(row)=self.tracks.iter().position(|t|t["id"]==c["track"])else{continue;};(body.top+self.row_top(row),self.height(row))};
            let start=self.displayed_clip_start(c);let duration=c["duration_seconds"].as_f64().unwrap();
            let mut values=[c["fade_in_seconds"].as_f64().unwrap_or(0.),c["fade_out_seconds"].as_f64().unwrap_or(0.)];
            if let Some(d)=self.fade_drag.as_ref().filter(|d|c["id"]==d.id){values=d.values;}
            let x=|t:f64|body.left+(t-self.start_seconds)/self.seconds_per_width*body.width;
            let margin=if c["track"]==""{CLIP_MARGIN*self.vertical_zoom}else{CLIP_MARGIN};let top=bounds.0+margin;
            let left=x(start).max(body.left);let right=x(start+duration).min(body.left+body.width);
            if right<=left{continue;}
            let h=bounds.1-2.*margin;
            let handles=[x(start+values[0]),x(start+duration-values[1])];
            let mut points=Vec::new();
            if values!=[0.,0.] {
                // Include fade knees exactly, then sample overlapping ramps.
                let mut times=(0..=32).map(|i|duration*i as f64/32.).collect::<Vec<_>>();
                times.extend([values[0],duration-values[1]]);times.sort_by(f64::total_cmp);
                for t in times {let a=if values[0]>0.{(t/values[0]).min(1.)}else{1.};let b=if values[1]>0.{((duration-t)/values[1]).min(1.)}else{1.};
                    points.push(json!([x(start+t).clamp(-32768.,32768.),top+h*(1.-a*b)]));}
            }
            fades.push(json!({"clip":[left,top,right-left,h],"points":points,"handles":handles.into_iter().filter(|x|*x>=left&&*x<=right).map(|x|json!([x,top])).collect::<Vec<_>>()}));
        }
        for index in self.visible_clip_indices(canvas).0 {
            let original=&self.clips[index];if original["kind"]!="cc"{continue;}
            let c=self.cc_drag.as_ref().filter(|d|d.clip["id"]==original["id"]).map_or(original,|d|&d.clip);
            let Some(row)=self.tracks.iter().position(|t|t["id"]==c["track"])else{continue;};
            let start=self.displayed_clip_start(c);let duration=c["duration_seconds"].as_f64().unwrap();
            let x=|t:f64|body.left+(t-self.start_seconds)/self.seconds_per_width*body.width;
            let left=x(start).max(body.left);let right=x(start+duration).min(body.left+body.width);if right<=left{continue;}
            let top=body.top+self.row_top(row)+CLIP_MARGIN+10.;let h=(self.height(row)-2.*CLIP_MARGIN-10.).max(1.);
            let mut values=c["points"].as_array().cloned().unwrap_or_default();values.sort_by(|a,b|a["offset_seconds"].as_f64().unwrap().total_cmp(&b["offset_seconds"].as_f64().unwrap()));
            let handles=values.iter().map(|p|json!([x(start+p["offset_seconds"].as_f64().unwrap()).clamp(-32768.,32768.),top+h*(1.-p["value"].as_f64().unwrap())])).collect::<Vec<_>>();
            let first=values.first().and_then(|p|p["value"].as_f64()).unwrap_or(0.5);
            let last=values.last().and_then(|p|p["value"].as_f64()).unwrap_or(0.5);
            let mut points=vec![json!([x(start).clamp(-32768.,32768.),top+h*(1.-first)])];points.extend(handles.clone());points.push(json!([x(start+duration).clamp(-32768.,32768.),top+h*(1.-last)]));
            fades.push(json!({"kind":"cc","clip":[left,top,right-left,h],"points":points,"handles":handles.into_iter().filter(|p|p[0].as_f64().unwrap()>=left&&p[0].as_f64().unwrap()<=right).collect::<Vec<_>>()}));
        }
        if !fades.is_empty(){if overlay.is_null(){overlay=json!({});}overlay["fades"]=json!(fades);}
        overlay
    }
    fn selection_overlay(&self,canvas:Rect)->Value{
        let Some(r)=&self.range else{return Value::Null;};
        if !valid_rect(canvas)||r.anchor==r.end{return Value::Null;}
        let body=self.clip_body(canvas);let row0=r.first.min(r.last);let row1=r.first.max(r.last);
        let first=row0.max(self.scroll_tracks);let last=row1.min(self.scroll_tracks+self.visible_rows(canvas).min(self.tracks.len().saturating_sub(self.scroll_tracks)).saturating_sub(1));
        if first>last||row1<self.scroll_tracks{return Value::Null;}
        let x=|t:f64|(body.left+(t-self.start_seconds)/self.seconds_per_width*body.width).clamp(body.left,body.left+body.width);
        let left=x(r.anchor.min(r.end));let right=x(r.anchor.max(r.end));if right<=left{return Value::Null;}
        let top=body.top+self.row_top(first)+CLIP_MARGIN;
        let bottom=body.top+self.row_top(last)+self.height(last)-CLIP_MARGIN;
        let mut holes=Vec::new();
        for index in self.visible_clip_indices(canvas).0{let c=&self.clips[index];let Some(row)=self.tracks.iter().position(|t|t["id"]==c["track"])else{continue;};
            if row<first||row>last{continue;}
            if c["kind"]=="audio" && self.selected_clip.as_deref()==c["id"].as_str(){continue;}
            let start=self.displayed_clip_start(c);let l=x(start);let end=x(start+c["duration_seconds"].as_f64().unwrap()).max(l+MIN_CLIP_WIDTH).min(body.left+body.width);
            holes.push(json!([l,body.top+self.row_top(row)+CLIP_MARGIN,end-l,self.height(row)-2.*CLIP_MARGIN]));
        }
        if let Some(t)=self.playhead.filter(|t|*t>=self.start_seconds&&*t<=self.start_seconds+self.seconds_per_width){
            // Keep the live foreground playhead readable through the range.
            holes.push(json!([x(t)-2.,top,4.,bottom-top]));
        }
        json!({"rect":[left,top,right-left,bottom-top],"holes":holes})
    }
    pub(crate) fn presentation_key(&self,canvas:Rect,playhead_seconds:Option<f64>)->PresentationKey {
        let preview=self.drag.as_ref().or(self.pending.as_ref());
        let track_preview=self.track_drag.as_ref().or(self.pending_track.as_ref());
        PresentationKey {document:self.presentation_document.clone(),state:json!({
            "canvas":[canvas.left,canvas.top,canvas.width,canvas.height],
            "revision":self.revision,"ready":self.ready,
            "start_seconds":self.start_seconds,"seconds_per_width":self.seconds_per_width,
            "bar_body":[self.clip_body(canvas).left,self.clip_body(canvas).top,self.clip_body(canvas).width,self.clip_body(canvas).height],"timeline_bars":self.bar_presentation(canvas),
            "lane_recording":self.lane_recording,"lane_time":if self.lane_recording{self.lane_time.rem_euclid(1.)<0.5}else{false},"lane_record_tracks":self.lane_record_tracks,"lane_controls":self.lane_controls,"lane_fader":self.lane_fader.as_ref().map(|d|(&d.0,d.1,self.previous.pointer)),"lane_layout":self.lane_layout,"free_tops":self.free_tops,"lane_heights":self.heights,"scroll_tracks":self.scroll_tracks,"selected_track":self.selected_track,
            "selected_clip":self.selected_clip,"notice":self.notice,
            "playhead_seconds":playhead_seconds.filter(|t|t.is_finite()&&*t>=0.),
            "vertical_view":[self.vertical_zoom,self.vertical_origin()],"vertical_rail":self.vertical_rail.map(|r|[r.left,r.top,r.width,r.height]),"clip_preview":preview.map(|d|(&d.id,d.preview_start,d.moved,d.target_row,d.free_lane)),
            "track_preview":track_preview.map(|d|(&d.id,d.original_index,d.preview_index,d.moved))
        })}
    }

    pub(crate) fn presentation(
        &self,
        canvas: Rect,
        playhead_seconds: Option<f64>,
        browser: Value,
    ) -> Value {
        let preview = self.drag.as_ref().or(self.pending.as_ref());
        let track_preview = self.track_drag.as_ref().or(self.pending_track.as_ref());
        let mut tracks = self.display_tracks();
        if let Some((id,offset,_,_,_))=&self.lane_fader {
            if let (Some(row),Some(p))=(tracks.iter().position(|t|t["id"]==*id),self.previous.pointer) {
                let y=canvas.top+RULER_HEIGHT+self.row_top(row)+4.;
                tracks[row]["gain"]=json!(super::timeline_controls::drag_gain(p[1]-offset,y,self.height(row)-8.));
            }
        }
        let (visible, hidden) = self.visible_clip_indices(canvas);
        let mut clips = visible
            .into_iter()
            .map(|index| self.clips[index].clone())
            .collect::<Vec<_>>();
        if let Some(drag) = preview {
            if let Some(clip) = clips.iter_mut().find(|c| c["id"] == drag.id) {
                clip["start_seconds"] = json!(drag.preview_start);
                clip["preview"] = json!(drag.moved);
                if let Some(row)=drag.target_row {clip["track"]=self.tracks[row]["id"].clone();clip["trackless"]=json!(false);}else if let Some(lane)=drag.free_lane{clip["lane"]=json!(lane);}
            }
        }
        for clip in &mut clips {if clip["track"]=="" {let (y,h)=self.free_bounds(clip,canvas);clip["free_top"]=json!(y);clip["free_height"]=json!(h);}
        if let Some(detail)=clip["id"].as_str().and_then(|id|self.waveform_details.get(id)){
            clip["clip_spectrogram"]=detail["spectrogram"].clone();
            clip["waveform_detail"]=detail["stereo_waveform"].clone();
            clip["waveform_view"]=detail["wave_view"].clone();
        }}
        json!({"canvas":{"left":canvas.left,"top":canvas.top,"width":canvas.width,"height":canvas.height},
            "revision":self.revision,"start_seconds":self.start_seconds,"seconds_per_width":self.seconds_per_width,
            "bar_body":[self.clip_body(canvas).left,self.clip_body(canvas).top,self.clip_body(canvas).width,self.clip_body(canvas).height],"timeline_bars":self.bar_presentation(canvas),
            "lane_recording":self.lane_recording,"lane_time":if self.lane_recording{self.lane_time.rem_euclid(1.)<0.5}else{false},"lane_record_tracks":self.lane_record_tracks,"lane_controls":self.lane_controls,"lane_fader":self.lane_fader.as_ref().map(|d|(&d.0,d.1,self.previous.pointer)),"lane_layout":self.lane_layout,"free_tops":self.free_tops,"lane_heights":self.heights,"scroll_tracks":self.scroll_tracks,"selected_track":self.selected_track,"selected_clip":self.selected_clip,
            "tracks":tracks,"clips":clips,"playhead_seconds":playhead_seconds.filter(|t| t.is_finite() && *t >= 0.),
            "preview_clip":preview.filter(|d| d.moved).map(|d| &d.id),
            "preview_track":track_preview.filter(|d| d.moved).map(|d| &d.id),"browser":browser,
            "hidden_clip_count":hidden,"notice":self.display_notice(hidden)})
    }
}

fn next_identity(items: &[Value], prefix: &str) -> String {
    // Among N+1 candidates at least one is absent from an N-item document.
    (1..=items.len() + 1)
        .map(|n| format!("{prefix}-{n}"))
        .find(|id| !items.iter().any(|item| item["id"] == *id))
        .unwrap()
}

pub(crate) fn create_track(snapshot: &Value, kind: &str) -> Result<Action, String> {
    let (revision, tracks, _) = read_snapshot(snapshot)?;
    let label = match kind {
        "audio" => "Audio",
        "midi" => "MIDI",
        "cc" => "CC",
        _ => return Err("Choose an audio, MIDI or CC track.".into()),
    };
    if tracks.len() >= 64 {
        return Err("The document already has 64 tracks.".into());
    }
    let id = next_identity(&tracks, &format!("arrangement-{kind}"));
    let count = tracks.iter().filter(|t| t["kind"] == kind).count() + 1;
    Ok(Action::Document {
        hook: "WorkstationDataSystem.PutTrack",
        request: json!({"document_revision":revision,"id":id,"name":format!("{label} {count}"),"kind":kind}),
    })
}

pub(crate) fn create_track_at(snapshot:&Value,kind:&str,index:usize)->Result<Action,String>{
    if index>snapshot["tracks"].as_array().ok_or("missing track list")?.len(){return Err("Track insertion position changed".into());}
    let mut action=create_track(snapshot,kind)?;
    if let Action::Document{request,..}=&mut action {request["insert_index"]=json!(index);}
    Ok(action)
}

pub(crate) fn create_clip(snapshot: &Value, track: &str, start: f64) -> Result<Action, String> {
    let (revision, tracks, clips) = read_snapshot(snapshot)?;
    let selected = tracks
        .iter()
        .find(|t| t["id"] == track)
        .ok_or("Select a track first.")?;
    if clips.len() >= 512 {
        return Err("The document already has 512 clips.".into());
    }
    let mut request = json!({"document_revision":revision,"id":next_identity(&clips,"arrangement-clip"),
        "track":track,"kind":selected["kind"],"start_seconds":start});
    let duration = match selected["kind"].as_str() {
        Some("midi") => {
            request["bpm"] = json!(120.);
            request["length_quarters"] = json!(16.);
            request["meter"] = json!({"numerator":4,"denominator":4});
            request["notes"] = json!([]);
            8.
        }
        Some("cc") => {
            request["duration_seconds"] = json!(4.);
            request["points"] = json!([{"offset_seconds":0.,"value":0.5},
                {"offset_seconds":4.,"value":0.5}]);
            4.
        }
        _ => {
            return Err(
                "Audio clips require a decoded source; file import is not connected yet.".into(),
            );
        }
    };
    if !start.is_finite() || start < 0. || start + duration > MAX_SECONDS {
        return Err("The new clip must fit within the document's 24-hour duration.".into());
    }
    Ok(Action::Document {
        hook: "WorkstationDataSystem.PutClip",
        request,
    })
}

pub(crate) fn create_midi_clip(
    snapshot: &Value,
    track: &str,
    start: f64,
) -> Result<Action, String> {
    if !snapshot["tracks"].as_array().is_some_and(|tracks| {
        tracks
            .iter()
            .any(|t| t["id"] == track && t["kind"] == "midi")
    }) {
        return Err("Select a MIDI track first.".into());
    }
    create_clip(snapshot, track, start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Value {
        json!({"revision":7,"tracks":[
            {"id":"midi","name":"Piano","kind":"midi","gain":1.,"mute":false,"solo":false},
            {"id":"cc","name":"Automation","kind":"cc","target":null}],
            "clips":[{"id":"clip","track":"midi","kind":"midi","start_seconds":2.,
                "duration_seconds":4.,"bpm":120.,"length_quarters":8.,"notes":[]}]})
    }
    fn rect() -> Rect {
        Rect {
            // Keep an 800 px body at x138 so elapsed-time fixtures stay exact.
            left: 138. - BODY_OFFSET,
            top: 20. - (RULER_HEIGHT - 32.),
            width: 800. + BODY_OFFSET + MONITOR_GUTTER_WIDTH,
            height: 288. + (RULER_HEIGHT - 32.),
        }
    }
    fn input(x: f64, y: f64, left: bool) -> Input {
        Input {
            pointer: Some([x, y]),
            left,
            ..Input::default()
        }
    }
    fn ready() -> Arrangement {
        let mut state = Arrangement::default();
        state.sync(&snapshot()).unwrap();
        state
    }
    fn document(action: &Action) -> (&str, &Value) {
        match action {
            Action::Document { hook, request } => (hook, request),
            _ => panic!("expected document edit"),
        }
    }

    fn key_covers_change(state:&mut Arrangement,change:impl FnOnce(&mut Arrangement)) {
        let before=state.presentation(rect(),Some(1.),Value::Null);
        let key=state.presentation_key(rect(),Some(1.));
        change(state);
        let after=state.presentation(rect(),Some(1.),Value::Null);
        if key==state.presentation_key(rect(),Some(1.)){assert_eq!(before,after);}
    }
    #[test]
    fn horizontal_handle_resize_preserves_order_and_updates_picking() {
        let mut s=ready();let r=rect();let y=r.top+RULER_HEIGHT+20.;let x=r.left+6.;
        let before=s.presentation_key(r,None);
        assert!(s.update(&input(x,y,true),r).is_empty());
        assert!(s.update(&input(x+48.,y,true),r).is_empty());
        assert_eq!(s.height(0),112.);
        assert_eq!(s.row_at([x,y+64.],r),Some(0));
        assert_ne!(before,s.presentation_key(r,None));
        assert!(s.update(&input(x+48.,y,false),r).is_empty());
        assert_eq!(s.height(0),112.);
        assert_eq!(s.display_tracks()[0]["id"],snapshot()["tracks"][0]["id"]);
        s.sync(&snapshot()).unwrap();assert_eq!(s.height(0),112.);
    }
    #[test]
    fn selected_handle_shrinks_at_left_edge_and_reverses_without_regrabbing() {
        let mut s=ready();let mut r=rect();r.left=0.;let y=r.top+RULER_HEIGHT+20.;
        s.update_at(&input(r.left+6.,y,true),r,1.);
        assert_eq!(s.selected_track(),Some("midi"));
        s.update_at(&input(0.,y,true),r,1.02);
        let first=s.height(0);
        s.update_at(&input(0.,y,true),r,1.04);
        assert_eq!(s.height(0),first,"a stationary pointer must not synthesize resizing");
        for i in 3..10 {s.update_at(&input(-(i as f64)*6.,y,true),r,1.+i as f64*0.02);}
        assert_eq!(s.height(0),32.);
        s.update_at(&input(-44.,y,true),r,1.2);
        assert_eq!(s.height(0),42.);
        s.update_at(&input(-44.,y,false),r,1.21);
        assert_eq!(s.height(0),42.);
        assert!(!s.is_interacting());
        assert_eq!(s.selected_track(),Some("midi"));
    }

    #[test]
    fn handle_resize_cancels_and_clamps() {
        let mut s=ready();let r=rect();let y=r.top+RULER_HEIGHT+20.;let x=r.left+6.;
        s.update(&input(x,y,true),r);s.update(&input(x+1000.,y,true),r);
        assert_eq!(s.height(0),256.);
        s.update(&Input{escape:true,..Default::default()},r);
        assert_eq!(s.height(0),LANE_HEIGHT);
        s.update(&input(x,y,true),r);s.update(&input(x-1000.,y,true),r);
        assert_eq!(s.height(0),32.);
        s.update(&Input::default(),r);assert_eq!(s.height(0),LANE_HEIGHT);
    }
    #[test]fn original_audio_header_right_click_opens_lane_menu_once(){let mut s=ready();let mut doc=snapshot();doc["tracks"][0]["kind"]=json!("audio");doc["clips"]=json!([]);s.sync(&doc).unwrap();let mut click=input(rect().left+6.5,76.,false);click.right=true;let a=s.update(&click,rect());assert!(matches!(a.as_slice(),[Action::Document{hook:"WorkstationDataSystem.AudioPlaylist",request}] if request["id"]=="midi"&&request["action"]=="toggle_lanes"));assert!(s.update(&click,rect()).is_empty());}
    #[test]
    fn presentation_key_reuses_document_identity_but_detects_same_revision_edits() {
        let mut state=ready();let mut doc=snapshot();
        let before=state.presentation_key(rect(),None);
        state.sync(&doc).unwrap();assert_eq!(before,state.presentation_key(rect(),None));
        doc["visualization"]=json!({"sequence":9});state.sync(&doc).unwrap();
        assert_eq!(before,state.presentation_key(rect(),None));
        for (field,value) in [("notes",json!([{"id":"n","quarter":0.}])),("duration_seconds",json!(6.))] {
            let before=state.presentation_key(rect(),None);doc["clips"][0][field]=value;
            state.sync(&doc).unwrap();assert_ne!(before,state.presentation_key(rect(),None));
        }
        let before=state.presentation_key(rect(),None);doc["tracks"][0]["gain"]=json!(0.5);
        state.sync(&doc).unwrap();assert_ne!(before,state.presentation_key(rect(),None));
    }
    #[test]
    fn presentation_key_covers_selection_pan_zoom_and_viewport_clock() {
        let mut state=ready();
        key_covers_change(&mut state,|s|s.select_clip("clip"));
        key_covers_change(&mut state,|s|s.select_track("cc"));
        key_covers_change(&mut state,|s|s.start_seconds=2.);
        key_covers_change(&mut state,|s|s.seconds_per_width=32.);
        key_covers_change(&mut state,|s|s.scroll_tracks=1);
        key_covers_change(&mut state,|s|s.notice=Some("notice".into()));
        let base=state.presentation_key(rect(),Some(1.));let mut resized=rect();resized.width-=20.;
        assert_ne!(base,state.presentation_key(resized,Some(1.)));
        assert_ne!(base,state.presentation_key(rect(),Some(2.)));
        assert_eq!(state.presentation_key(rect(),None),state.presentation_key(rect(),Some(f64::NAN)));
    }
    #[test]
    fn presentation_key_covers_drag_pending_rejection_and_track_order() {
        let mut state=ready();
        for (x,left) in [(248.,true),(348.,true),(348.,false)] {
            key_covers_change(&mut state,|s|{s.update(&input(x,76.,left),rect());});
        }
        key_covers_change(&mut state,|s|s.sync(&snapshot()).unwrap());
        key_covers_change(&mut state,|s|{
            s.track_drag=Some(TrackDrag{id:"midi".into(),revision:7,origin_y:40.,origin_x:0.,original_height:64.,last_x:0.,last_time:0.,edge_direction:0.,resize:false,original_index:0,
                preview_index:1,moved:true,canvas:rect()});
        });
        key_covers_change(&mut state,|s|{s.pending_track=s.track_drag.take();});
        key_covers_change(&mut state,|s|{let mut doc=snapshot();doc["revision"]=json!(8);
            doc["tracks"].as_array_mut().unwrap().reverse();s.sync(&doc).unwrap();});
        key_covers_change(&mut state,|s|{assert!(s.sync(&Value::Null).is_err());});
    }

    fn click_at(s:&mut Arrangement,x:f64,y:f64,release:f64)->Vec<Action>{
        assert!(s.update_at(&input(x,y,true),rect(),release-0.01).is_empty());
        s.update_at(&input(x,y,false),rect(),release)
    }
    #[test]
    fn cardinal_double_click_opens_only_on_second_release_inside_time_window(){
        for (delay,opens) in [(0.349,true),(0.35,false),(0.351,false)]{
            let mut s=ready();assert!(click_at(&mut s,248.,76.,1.).is_empty());
            assert_eq!(s.selected_clip(),Some("clip"));
            let actions=click_at(&mut s,248.,76.,1.+delay);
            assert_eq!(actions.iter().any(|a|matches!(a,Action::OpenTrackPlugins(id)if id=="midi")),opens);
        }
    }
    #[test]
    fn cardinal_double_click_excludes_movement_and_other_clip_kinds(){
        for (dx,dy) in [(4.,0.),(0.,3.01)]{
            let mut s=ready();click_at(&mut s,248.,76.,1.);
            s.update_at(&input(248.,76.,true),rect(),1.1);
            s.update_at(&input(248.+dx,76.+dy,true),rect(),1.15);
            assert!(!s.update_at(&input(248.+dx,76.+dy,false),rect(),1.2).iter().any(|a|matches!(a,Action::OpenPianoRoll(_))));
        }
        for kind in ["audio","cc"]{let mut d=snapshot();d["tracks"][0]["kind"]=json!(kind);d["clips"][0]["kind"]=json!(kind);let mut s=Arrangement::default();s.sync(&d).unwrap();click_at(&mut s,248.,62.,1.);assert!(!click_at(&mut s,248.,62.,1.2).iter().any(|a|matches!(a,Action::OpenPianoRoll(_))));}
        let mut s=ready();click_at(&mut s,248.,76.,1.);s.update_at(&input(248.,76.,true),rect(),1.1);
        assert!(matches!(s.update_at(&input(248.,79.,false),rect(),1.2).first(),Some(Action::OpenTrackPlugins(_))));
    }
    #[test]
    fn track_plugins_double_click_lane_handle_and_each_clip_kind(){
        for kind in ["midi","audio","cc"] {
            for x in [rect().left+6.,rect().left+HANDLE_WIDTH+1.,248.,800.] {
                let mut d=snapshot();d["tracks"][0]["kind"]=json!(kind);d["clips"][0]["kind"]=json!(kind);
                let mut s=Arrangement::default();s.sync(&d).unwrap();
                click_at(&mut s,x,62.,1.);
                let actions=click_at(&mut s,x,62.,1.2);
                assert!(actions.iter().any(|a|if kind=="audio"&&x==248. {matches!(a,Action::OpenWaveform(id) if id=="clip")}else{matches!(a,Action::OpenTrackPlugins(id) if id=="midi")}),"{kind} at {x}: {actions:?}");
            }
        }
    }
    #[test]
    fn cardinal_double_click_cancellation_and_document_changes_reset_candidate(){
        for mode in 0..4{let mut s=ready();click_at(&mut s,248.,76.,1.);match mode{
            0=>{s.update_at(&Input{escape:true,..input(248.,76.,false)},rect(),1.1);},
            1=>{s.update_at(&Input::default(),rect(),1.1);},
            2=>{let mut d=snapshot();d["revision"]=json!(8);s.sync(&d).unwrap();},
            _=>{let mut r=rect();r.width-=20.;s.update_at(&input(248.,76.,false),r,1.1);}
        }assert!(click_at(&mut s,248.,76.,1.2).is_empty());}
    }
    #[test]
    fn cardinal_double_click_requires_same_identity_and_ignores_timeline_enter(){
        let mut s=ready();let mut d=snapshot();d["clips"].as_array_mut().unwrap().push(json!({"id":"other","track":"midi","kind":"midi","start_seconds":6.,"duration_seconds":2.}));s.sync(&d).unwrap();
        click_at(&mut s,248.,76.,1.);assert!(click_at(&mut s,448.,76.,1.2).is_empty());
        assert_eq!(s.selected_clip(),Some("other"));
        let a=s.update_at(&Input{enter:true,..input(448.,76.,false)},rect(),1.3);assert!(a.is_empty());
    }

    #[test]
    fn drag_previews_elapsed_seconds_and_commits_once_on_release() {
        let mut state = ready();
        assert!(state.update(&input(248., 76., true), rect()).is_empty());
        assert!(state.update(&input(348., 76., true), rect()).is_empty());
        assert_eq!(
            state.presentation(rect(), None, Value::Null)["clips"][0]["start_seconds"],
            4.
        );
        let actions = state.update(&input(348., 76., false), rect());
        let (hook, request) = document(&actions[0]);
        assert_eq!(hook, "WorkstationDataSystem.MoveClip");
        assert_eq!(
            request,
            &json!({"document_revision":7,"id":"clip","start_seconds":4.})
        );
        assert!(state.update(&input(348., 76., false), rect()).is_empty());
        let mut accepted = snapshot();
        accepted["revision"] = json!(8);
        accepted["clips"][0]["start_seconds"] = json!(4.);
        state.sync(&accepted).unwrap();
        assert_eq!(state.status()["pending_commit"], false);
        assert!(state.status()["notice"].is_null());
    }

    #[test]
    fn pointer_loss_escape_and_external_revision_cancel_uncommitted_move() {
        for mode in 0..3 {
            let mut state = ready();
            state.update(&input(248., 76., true), rect());
            state.update(&input(348., 76., true), rect());
            match mode {
                0 => {
                    state.update(
                        &Input {
                            left: true,
                            ..Input::default()
                        },
                        rect(),
                    );
                }
                1 => {
                    state.update(
                        &Input {
                            escape: true,
                            ..input(348., 76., true)
                        },
                        rect(),
                    );
                }
                _ => {
                    let mut changed = snapshot();
                    changed["revision"] = json!(8);
                    state.sync(&changed).unwrap();
                }
            }
            assert!(!state.is_interacting());
            assert!(state.update(&input(348., 76., false), rect()).is_empty());
            assert_eq!(
                state.presentation(rect(), None, Value::Null)["clips"][0]["start_seconds"],
                2.
            );
        }
    }

    #[test]
    fn rejected_move_restores_authoritative_position_and_stale_snapshots_gate_edits() {
        let mut state = ready();
        state.update(&input(248., 76., true), rect());
        state.update(&input(348., 76., false), rect());
        state.sync(&snapshot()).unwrap();
        assert_eq!(
            state.presentation(rect(), None, Value::Null)["clips"][0]["start_seconds"],
            2.
        );
        assert!(state.status()["notice"].is_string());
        let mut stale = snapshot();
        stale["revision"] = json!(6);
        assert!(state.sync(&stale).is_err());
        assert!(state.update(&input(30., 96., true), rect()).is_empty());
        assert_eq!(state.status()["ready"], false);
    }

    #[test]
    fn selection_overlap_controls_and_keyboard_use_document_identities() {
        let mut state = ready();
        // Former M/S/gain positions cannot dispatch track mix edits.
        for x in [30., 58., 90.] {
            let mut empty = ready();
            empty.clips.clear();
            assert!(empty.update(&input(rect().left + x, 96., true), rect()).iter().all(|a|
                !matches!(a, Action::Document { hook: "WorkstationDataSystem.SetTrackMix", .. })));
        }
        let mut empty = ready();
        empty.clips.clear();
        let actions = empty.update(&Input {
            wheel: [0., 1.],
            ..input(rect().left + 90., 96., false)
        }, rect());
        assert!(actions.iter().all(|a| !matches!(a,
            Action::Document { hook: "WorkstationDataSystem.SetTrackMix", .. })));
        state.select_clip("clip");
        let open = state.update(
            &Input {
                enter: true,
                ..input(500., 100., false)
            },
            rect(),
        );
        assert!(open.is_empty());
        let delete = state.update(
            &Input {
                delete: true,
                ..input(500., 100., false)
            },
            rect(),
        );
        assert_eq!(document(&delete[0]).0, "WorkstationDataSystem.RemoveClip");
        assert!(
            state
                .update(
                    &Input {
                        delete: true,
                        ..input(500., 100., false)
                    },
                    rect()
                )
                .is_empty()
        );
        let mut overlap = snapshot();
        overlap["clips"].as_array_mut().unwrap().push(json!({
            "id":"top","track":"midi","kind":"midi","start_seconds":3.,"duration_seconds":2.}));
        overlap["revision"] = json!(8);
        state.sync(&overlap).unwrap();
        state.update(&input(313., 76., true), rect());
        assert_eq!(state.selected_clip(), Some("top"));
    }

    #[test]
    fn zoom_bar_grab_does_not_seek_and_wheel_preserves_pointer_time() {
        let mut state = ready();
        let y=rect().top+super::super::timeline_bars::LOOP_HEIGHT+super::super::timeline_bars::GAP+13.;
        let actions = state.update(&input(538., y, true), rect());
        assert!(actions.is_empty());
        assert_eq!(state.start_seconds,0.);assert_eq!(state.seconds_per_width,16.);
        state.update(&input(538., y, false), rect());
        state.update(
            &Input {
                command: true,
                wheel: [0., -1.],
                ..input(538., 100., false)
            },
            rect(),
        );
        let view = state.status();
        assert!(
            (view["start_seconds"].as_f64().unwrap()
                + view["seconds_per_width"].as_f64().unwrap() * 0.5
                - 8.)
                .abs()
                < 1e-9
        );
        assert!(view["seconds_per_width"].as_f64().unwrap() < 16.);
    }

    #[test]
    fn monitor_gutter_does_not_seek_select_or_zoom_the_arrangement() {
        let canvas = rect();
        let gutter_x = canvas.left + canvas.width - MONITOR_GUTTER_WIDTH * 0.5;
        for y in [36., 76.] {
            let mut state = ready();
            state.select_clip("clip");
            let before = state.status();
            let actions=state.update(&input(gutter_x, y, true), canvas);
            if y==76. {
                assert_eq!(actions.len(),1);
                assert!(matches!(&actions[0],Action::Document{hook:"WorkstationDataSystem.SetTrackMix",request} if request["input_monitor_mode"]==0));
                assert!(state.consumes_pointer());
            }else{assert!(actions.is_empty());assert!(!state.consumes_pointer());}
            assert_eq!(state.selected_clip(), Some("clip"));
            state.update(&input(gutter_x, y, false), canvas);
            for command in [false, true] {
                assert!(
                    state
                        .update(
                            &Input {
                                command,
                                wheel: [0., 1.],
                                ..input(gutter_x, y, false)
                            },
                            canvas,
                        )
                        .is_empty()
                );
            }
            assert_eq!(state.status()["start_seconds"], before["start_seconds"]);
            assert_eq!(
                state.status()["seconds_per_width"],
                before["seconds_per_width"]
            );
        }
    }

    #[test]
    fn vertical_handle_drag_reorders_mixed_tracks_with_clips_attached_and_revision_guard() {
        let mut state = ready();
        state.update(&input(rect().left + 6.5, 64., true), rect());
        assert!(state.is_interacting());
        assert!(state.update(&input(rect().left + 6.5, 140., true), rect()).is_empty());
        let preview = state.presentation(rect(), None, Value::Null);
        assert_eq!(preview["tracks"][0]["id"], "cc");
        assert_eq!(preview["tracks"][1]["id"], "midi");
        assert_eq!(preview["clips"][0]["track"], "midi");
        let actions = state.update(&input(rect().left + 6.5, 140., false), rect());
        let (hook, request) = document(&actions[0]);
        assert_eq!(hook, "WorkstationDataSystem.ReorderTracks");
        assert_eq!(request, &json!({"document_revision":7,"ids":["cc","midi"]}));
        let mut accepted = snapshot();
        accepted["revision"] = json!(8);
        accepted["tracks"].as_array_mut().unwrap().swap(0, 1);
        state.sync(&accepted).unwrap();
        assert_eq!(state.selected_track(), Some("midi"));
        assert_eq!(state.status()["pending_commit"], false);
        assert!(state.status()["notice"].is_null());

        state.update(&input(rect().left + 6.5, 140., true), rect());
        state.update(&input(rect().left + 6.5, 64., true), rect());
        accepted["revision"] = json!(9);
        state.sync(&accepted).unwrap();
        assert!(!state.is_interacting());
        assert!(state.update(&input(rect().left + 6.5, 64., false), rect()).is_empty());
        assert_eq!(
            state.presentation(rect(), None, Value::Null)["tracks"][0]["id"],
            "cc"
        );
    }

    #[test]
    fn selected_clip_enter_does_not_replace_cardinal_double_click() {
        let mut state = ready();
        state.select_clip("clip");
        let actions = state.update(
            &Input {
                enter: true,
                ..Input::default()
            },
            rect(),
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn explicit_reveal_fits_appended_track_using_whole_rows_and_preserves_manual_scroll() {
        let mut document = snapshot();
        for index in 0..6 {
            document["tracks"].as_array_mut().unwrap().push(json!({
                "id":format!("extra-{index}"),"name":format!("Extra {index}"),"kind":"cc","target":null}));
        }
        let mut state = Arrangement::default();
        state.sync(&document).unwrap();
        state.select_track("extra-5");
        let short = Rect {
            height: RULER_HEIGHT + LANE_HEIGHT * 2. + 31.,
            ..rect()
        };
        state.reveal_selected(short);
        assert_eq!(state.status()["scroll_tracks"], 6);
        let selected_row = 7 - state.scroll_tracks;
        assert!(RULER_HEIGHT + (selected_row + 1) as f64 * LANE_HEIGHT <= short.height);

        state.update(
            &Input {
                wheel: [0., -1.],
                ..input(rect().left + 6.5, 72., false)
            },
            short,
        );
        assert_eq!(state.status()["scroll_tracks"], 5);
        state.update(&input(rect().left + 6.5, 72., false), short);
        assert_eq!(state.status()["scroll_tracks"], 5);
        state.select_track("midi");
        state.reveal_selected(short);
        assert_eq!(state.status()["scroll_tracks"], 0);
    }

    #[test]
    fn panel_layout_changes_cancel_clip_and_track_drags_without_committing() {
        for title_drag in [false, true] {
            let mut state = ready();
            let (begin, moved) = if title_drag {
                ([rect().left + 6.5, 64.], [rect().left + 6.5, 140.])
            } else {
                ([248., 76.], [348., 76.])
            };
            state.update(&input(begin[0], begin[1], true), rect());
            state.update(&input(moved[0], moved[1], true), rect());
            assert!(state.is_interacting());
            let resized = Rect {
                left: rect().left + 10.,
                width: rect().width - 20.,
                ..rect()
            };
            assert!(
                state
                    .update(&input(moved[0], moved[1], true), resized)
                    .is_empty()
            );
            assert!(!state.is_interacting());
            assert!(
                state
                    .update(&input(moved[0], moved[1], false), resized)
                    .is_empty()
            );
            let presentation = state.presentation(resized, None, Value::Null);
            assert_eq!(presentation["tracks"][0]["id"], "midi");
            assert_eq!(presentation["clips"][0]["start_seconds"], 2.);
        }
    }

    #[test]
    fn unusually_tall_viewports_keep_picking_and_reveal_within_scene_capacity() {
        let mut document = snapshot();
        for index in 0..62 {
            document["tracks"].as_array_mut().unwrap().push(json!({
                "id":format!("extra-{index}"),"name":format!("Extra {index}"),"kind":"cc","target":null}));
        }
        let mut state = Arrangement::default();
        state.sync(&document).unwrap();
        let tall = Rect {
            height: 5000.,
            ..rect()
        };
        state.select_track("extra-61");
        state.reveal_selected(tall);
        assert_eq!(state.status()["scroll_tracks"], 32);
        assert_eq!(
            state.row_at(
                [rect().left + 6.5, tall.top + RULER_HEIGHT + 31. * LANE_HEIGHT + 8.],
                tall
            ),
            Some(63)
        );
        state.scroll_tracks = 0;
        assert!(
            state
                .row_at(
                    [rect().left + 6.5, tall.top + RULER_HEIGHT + 32. * LANE_HEIGHT + 8.],
                    tall
                )
                .is_none()
        );
        state.scroll_tracks = 1;
        assert_eq!(
            state.row_at(
                [rect().left + 6.5, tall.top + RULER_HEIGHT + 31. * LANE_HEIGHT + 8.],
                tall
            ),
            Some(32)
        );
    }

    #[test]
    fn bounded_visible_tapes_keep_late_overlap_priority_and_hidden_tapes_cannot_be_picked() {
        let mut document = snapshot();
        document["clips"] = json!(
            (0..130)
                .map(|index| json!({
                    "id":format!("clip-{index}"),"track":"midi","kind":"midi",
                    "start_seconds":if index<2 {1.} else {3.},"duration_seconds":1.
                }))
                .collect::<Vec<_>>()
        );
        let mut state = Arrangement::default();
        state.sync(&document).unwrap();
        state.update(&input(500., 200., false), rect());
        let presentation = state.presentation(rect(), None, Value::Null);
        assert_eq!(presentation["clips"].as_array().unwrap().len(), 128);
        assert_eq!(presentation["clips"][0]["id"], "clip-2");
        assert_eq!(presentation["clips"][127]["id"], "clip-129");
        assert_eq!(presentation["hidden_clip_count"], 2);
        assert_eq!(state.status()["hidden_clip_count"], 2);
        assert!(
            state.status()["notice"]
                .as_str()
                .unwrap()
                .contains("zoom in")
        );
        let body = clip_body(rect());
        let hidden_x = body.left + body.width * 1.5 / 16.;
        assert!(state.update(&input(hidden_x, 76., true), rect()).is_empty());
        assert_eq!(state.selected_clip(), None);
        // Empty visible space can start a range, but hidden tapes are never
        // selected, dragged or committed through that range gesture.
        assert!(state.drag.is_none());
        assert!(state.range.as_ref().is_some_and(|r|r.dragging));
        assert!(state.update(&input(hidden_x, 76., false), rect()).is_empty());
        let visible_x = body.left + body.width * 3.5 / 16.;
        state.update(&input(visible_x, 76., true), rect());
        assert_eq!(state.selected_clip(), Some("clip-129"));
        state.cancel_interaction();
        state.start_seconds = 0.;
        state.seconds_per_width = 2.;
        let smaller = state.presentation(rect(), None, Value::Null);
        assert_eq!(smaller["clips"].as_array().unwrap().len(), 2);
        assert_eq!(smaller["hidden_clip_count"], 0);
        assert!(
            state
                .hit_clip(0, [body.left + body.width * 0.75, 76.], rect())
                .is_some()
        );
        assert_eq!(document["clips"].as_array().unwrap().len(), 130);
    }

    #[test]
    fn partial_rows_are_not_displayed_or_picked_and_successful_selection_clears_old_notice() {
        let mut state = ready();
        let short = Rect {
            height: RULER_HEIGHT + LANE_HEIGHT + 20.,
            ..rect()
        };
        assert!(
            state
                .row_at([30., short.top + RULER_HEIGHT + LANE_HEIGHT + 8.], short)
                .is_none()
        );
        state.notice = Some("outdated rejection".into());
        state.select_track("midi");
        assert!(state.status()["notice"].is_null());
        state.notice = Some("outdated rejection".into());
        state.select_clip("clip");
        assert!(state.status()["notice"].is_null());
    }

    #[test]
    fn browser_drop_slot_is_body_only_and_scroll_aware(){
        let mut s=ready();let r=rect();let b=clip_body(r);let x=b.left+10.;
        assert_eq!(s.browser_drop_slot(r,[x,b.top]),Some(0));
        assert_eq!(s.browser_drop_slot(r,[x,b.top+1.]),Some(0));
        assert_eq!(s.browser_drop_slot(r,[x,b.top+CLIP_MARGIN-0.1]),Some(0));
        assert_eq!(s.browser_drop_slot(r,[x,b.top+CLIP_MARGIN]),Some(1));
        assert_eq!(s.browser_drop_slot(r,[x,b.top+LANE_HEIGHT]),Some(2));
        for p in [[r.left+5.,b.top+10.],[x,r.top+5.],[b.left+b.width+1.,b.top+10.],[x,b.top+b.height],[f64::NAN,b.top]]{assert_eq!(s.browser_drop_slot(r,p),None);}
        s.scroll_tracks=1;assert_eq!(s.browser_drop_slot(r,[x,b.top]),Some(1));
        s.tracks.clear();s.scroll_tracks=0;assert_eq!(s.browser_drop_slot(r,[x,b.top+10.]),Some(0));
        let a=create_track_at(&snapshot(),"cc",1).unwrap();assert_eq!(document(&a).1["insert_index"],1);assert!(create_track_at(&snapshot(),"cc",99).is_err());
    }

    #[test]
    fn creation_is_revision_checked_and_source_requirements_are_explicit() {
        let action = create_track(&snapshot(), "midi").unwrap();
        let (_, request) = document(&action);
        assert_eq!(request["document_revision"], 7);
        assert_eq!(request["kind"], "midi");
        let midi = create_midi_clip(&snapshot(), "midi", 5.).unwrap();
        assert_eq!(document(&midi).1["length_quarters"], 16.);
        let cc = create_clip(&snapshot(), "cc", 5.).unwrap();
        assert_eq!(document(&cc).1["points"][1]["offset_seconds"], 4.);
        assert!(create_clip(&snapshot(), "midi", 86_397.).is_err());
        let mut audio = snapshot();
        audio["tracks"][0]["kind"] = json!("audio");
        audio["clips"] = json!([]);
        assert!(
            create_clip(&audio, "midi", 0.)
                .unwrap_err()
                .contains("decoded source")
        );
    }
}

#[cfg(test)]mod range_tests{
 use super::*;
 fn rect()->Rect{Rect{left:128.-BODY_OFFSET,top:-(RULER_HEIGHT-32.),width:800.+BODY_OFFSET+MONITOR_GUTTER_WIDTH,height:288.+(RULER_HEIGHT-32.)}}
 fn input(x:f64,y:f64,left:bool)->Input{Input{pointer:Some([x,y]),left,..Default::default()}}
 fn ready()->Arrangement{let mut s=Arrangement::default();s.sync(&json!({"revision":1,"tracks":[{"id":"m","name":"M","kind":"midi","gain":1.,"mute":false,"solo":false},{"id":"c","name":"C","kind":"cc"}],"clips":[]})).unwrap();s}
 #[test]fn range_reverse_drag_normalizes_and_does_not_edit_document(){let mut s=ready();s.set_playhead(Some(0.));let a=s.update(&input(728.,128.,true),rect());assert!(matches!(a.first(),Some(Action::Seek(12.))));assert!(s.update(&input(228.,64.,true),rect()).is_empty());assert!(s.update(&input(228.,64.,false),rect()).is_empty());let Action::Document{request,..}=s.range_action().unwrap()else{panic!()};assert_eq!(request["start_seconds"],12.);assert_eq!(request["end_seconds"],2.);assert_eq!(request["first_track"],"c");assert_eq!(request["last_track"],"m");assert_eq!(s.status()["range"]["start_seconds"],2.);}
 #[test]fn range_command_anchor_and_grid_are_original_free_time_rules(){assert_eq!(range_grid(16.),2.);assert_eq!(range_grid(1.),0.1);let mut s=ready();s.set_playhead(Some(3.25));let mut i=input(300.,64.,true);i.command=true;assert!(s.update(&i,rect()).is_empty());i.left=false;s.update(&i,rect());let r=s.range.unwrap();assert_eq!(r.anchor,3.25);assert!((r.end-3.44).abs()<1e-12);}
 #[test]fn range_cancellation_release_and_outside_bounds_are_safe(){for cancel in 0..3{let mut s=ready();s.update(&input(228.,64.,true),rect());s.update(&input(328.,128.,true),rect());match cancel{0=>{let mut i=input(328.,128.,true);i.escape=true;s.update(&i,rect());},1=>{s.update(&Input{left:true,..Default::default()},rect());},_=>{let mut r=rect();r.width-=10.;s.update(&input(328.,128.,true),r);}}assert!(s.range_action().is_err());s.update(&input(328.,128.,false),rect());assert!(s.range_action().is_err());}
 let mut s=ready();s.update(&input(228.,64.,true),rect());s.update(&input(5000.,-100.,false),rect());assert_eq!(s.status()["range"]["end_seconds"],16.);assert!(!s.is_interacting());s.update(&Input::default(),rect());assert!(s.range_action().is_ok());}
 #[test]fn range_visual_is_clipped_and_has_tape_holes_without_optical_packet_changes(){let mut s=ready();s.update(&input(228.,64.,true),rect());let key=s.presentation_key(rect(),Some(2.));s.update(&input(728.,128.,true),rect());assert_eq!(key,s.presentation_key(rect(),Some(2.)));let o=s.range_overlay(rect());assert_eq!(o["rect"],json!([228.,40.,500.,112.]));assert_eq!(o["holes"],json!([]));s.update(&input(728.,128.,false),rect());s.clips.push(json!({"id":"x","track":"m","start_seconds":4.,"duration_seconds":2.}));assert_eq!(s.range_overlay(rect())["holes"][0],json!([328.,40.,100.,48.]));}
}

#[cfg(test)]mod timeline_bar_integration_tests {
 use super::*;
 fn ready()->Arrangement {let mut s=Arrangement::default();s.sync(&json!({"revision":1,"tracks":[{"id":"m","name":"M","kind":"midi","gain":1.,"mute":false,"solo":false}],"clips":[]})).unwrap();s}
 fn rect()->Rect {Rect{left:128.-BODY_OFFSET,top:0.,width:800.+BODY_OFFSET+MONITOR_GUTTER_WIDTH,height:400.}}
 #[test]fn command_l_enables_exact_selected_range_without_creating_a_clip(){
  let mut s=ready();let y=RULER_HEIGHT+32.;
  for(x,left)in[(228.,true),(428.,true),(428.,false)] {s.update(&Input{pointer:Some([x,y]),left,..Default::default()},rect());}
  let actions=s.update(&Input{pointer:Some([428.,y]),loop_range_pressed:true,..Default::default()},rect());
  assert!(matches!(actions.as_slice(),[Action::SetLoop(r)]if r.start==2.&&r.end==6.&&r.enabled));assert!(s.clips.is_empty());
 }
 #[test]fn bar_geometry_changes_invalidate_presentation_and_owns_shift_without_overview(){
  let mut s=ready();s.update(&Input{pointer:Some([400.,35.]),..Default::default()},rect());assert!(s.wants_bar_shift(Some([400.,35.])));
  let before=s.presentation_key(rect(),None);s.update(&Input{pointer:Some([400.,35.]),left:true,shift:true,..Default::default()},rect());
  s.update(&Input{pointer:Some([400.,100.]),left:true,shift:true,..Default::default()},rect());assert_ne!(before,s.presentation_key(rect(),None));assert!(s.consumes_pointer());
  let p=s.presentation(rect(),None,Value::Null);assert_eq!(p["timeline_bars"]["offset_y"],65.);
 }
 #[test]fn no_selection_means_command_l_has_no_effect_and_toggle_is_separate(){
  let mut s=ready();assert!(s.update(&Input{pointer:Some([400.,100.]),loop_range_pressed:true,..Default::default()},rect()).is_empty());
  assert!(matches!(s.loop_toggle_action(),Action::SetLoop(r)if r.enabled));assert!(matches!(s.loop_toggle_action(),Action::SetLoop(r)if !r.enabled));
 }
}

#[cfg(test)]mod timeline_controller_regressions {
 use super::*;
 fn rect()->Rect{Rect{left:128.-BODY_OFFSET,top:0.,width:800.+BODY_OFFSET+MONITOR_GUTTER_WIDTH,height:400.}}
 fn ready()->Arrangement{let mut s=Arrangement::default();s.sync(&json!({"revision":1,"tracks":[],"clips":[]})).unwrap();s}
 #[test]fn lane_zoom_and_pan_share_bar_limits_and_preserve_negative_start(){
  let mut s=ready();s.start_seconds=-10.;s.seconds_per_width=super::super::timeline_bars::MIN_SECONDS;
  let p=[528.,100.];s.update(&Input{pointer:Some(p),command:true,wheel:[0.,-100.],..Default::default()},rect());
  assert_eq!(s.seconds_per_width,super::super::timeline_bars::MIN_SECONDS);assert_eq!(s.start_seconds,-10.);
  s.seconds_per_width=super::super::timeline_bars::MAX_SECONDS;
  s.update(&Input{pointer:Some(p),command:true,wheel:[0.,100.],..Default::default()},rect());
  assert_eq!(s.seconds_per_width,super::super::timeline_bars::MAX_SECONDS);assert_eq!(s.start_seconds,-10.);
  s.seconds_per_width=16.;s.update(&Input{pointer:Some(p),wheel:[-1.,0.],..Default::default()},rect());
  assert_eq!(s.start_seconds,-10.8);
 }
 #[test]fn masked_held_input_cancels_capture_and_requires_release_before_new_grab(){
  let mut s=ready();let held=Input{pointer:Some([400.,35.]),left:true,..Default::default()};
  s.update(&held,rect());assert!(s.is_interacting());
  s.update(&Input{left:true,..Default::default()},rect());assert!(!s.is_interacting());
  s.update(&held,rect());assert!(!s.is_interacting());assert_eq!(s.start_seconds,0.);
  s.update(&Input{left:false,..held.clone()},rect());s.update(&held,rect());assert!(s.is_interacting());
 }
 #[test]fn loop_strip_wheel_does_not_pan_lane_even_beyond_visible_loop(){
  let mut s=ready();s.start_seconds=-4.;let before=s.seconds_per_width;
  for wheel in [[1.,0.],[0.,1.]] {s.update(&Input{pointer:Some([900.,6.]),wheel,..Default::default()},rect());}
  assert_eq!(s.start_seconds,-4.);assert_eq!(s.seconds_per_width,before);
 }
}

#[cfg(test)]mod audio_editor_context_tests{
 use super::*;
 fn ready(kind:&str)->Arrangement{let mut a=Arrangement::default();a.sync(&json!({"revision":1,"tracks":[{"id":"t","kind":kind,"name":"Track","gain":1.,"mute":false,"solo":false}],"clips":[{"id":"c","track":"t","kind":kind,"start_seconds":0.,"duration_seconds":8.,"source":"s"}]})).unwrap();a}
 #[test]fn original_track_handle_opens_track_menu_not_selected_clip_editor(){let rect=Rect{left:0.,top:0.,width:900.,height:300.};let mut a=ready("audio");a.select_clip("c");let input=Input{pointer:Some([6.5,RULER_HEIGHT+32.]),right:true,..Default::default()};assert!(matches!(a.update(&input,rect).as_slice(),[Action::Document{hook:"WorkstationDataSystem.AudioPlaylist",request}]if request["id"]=="t"&&request["action"]=="toggle_lanes"));assert!(a.update(&input,rect).is_empty());}
 #[test]fn audio_clip_right_press_selects_clip_and_requests_its_menu_once(){let mut a=ready("audio");let rect=Rect{left:0.,top:0.,width:900.,height:300.};let input=Input{pointer:Some([200.,RULER_HEIGHT+32.]),right:true,..Default::default()};assert!(matches!(a.update(&input,rect).as_slice(),[Action::AudioClipMenu(id)]if id=="c"));assert!(a.update(&input,rect).is_empty());let mut midi=ready("midi");assert!(midi.update(&input,rect).is_empty());}
}

#[cfg(test)]mod fade_tests{
 use super::*;
 fn setup()->(Arrangement,Rect){let mut a=Arrangement::default();a.sync(&json!({"revision":1,"tracks":[{"id":"a","name":"Audio","kind":"audio","gain":1.,"mute":false,"solo":false}],"clips":[{"id":"c","track":"a","kind":"audio","start_seconds":2.,"duration_seconds":4.,"fade_in_seconds":0.,"fade_out_seconds":0.}]})).unwrap();(a,Rect{left:0.,top:0.,width:882.,height:300.})}
 fn pointer(x:f64,y:f64,left:bool)->Input{Input{pointer:Some([x,y]),left,..Default::default()}}
 #[test]fn fade_handles_preview_without_moving_clip_then_commit_once(){let(mut a,r)=setup();let b=clip_body(r);let x=b.left+2./16.*b.width;let y=b.top+CLIP_MARGIN+1.;
 assert!(a.update(&pointer(x+1.,y,true),r).is_empty());assert!(a.fade_drag.is_some());
 a.update(&pointer(x+b.width/16.,y,true),r);let overlay=a.range_overlay(r);assert!(!overlay["fades"][0]["points"].as_array().unwrap().is_empty());assert_eq!(a.clips[0]["start_seconds"],2.);
 let actions=a.update(&pointer(x+b.width/16.,y,false),r);assert_eq!(actions.len(),1);let Action::Document{hook,request}=&actions[0]else{panic!()};assert_eq!(*hook,"WorkstationDataSystem.SetClipFades");assert!((request["fade_in_seconds"].as_f64().unwrap()-1.).abs()<1e-9);assert_eq!(request["fade_out_seconds"],0.);assert!(a.update(&pointer(x,y,false),r).is_empty());}
 #[test]fn fade_drag_escape_pointer_loss_and_resize_cancel(){for mode in 0..3{let(mut a,r)=setup();let b=clip_body(r);let x=b.left+2./16.*b.width+1.;let y=b.top+CLIP_MARGIN+1.;a.update(&pointer(x,y,true),r);assert!(a.fade_drag.is_some());let mut next=pointer(x+30.,y,true);let mut size=r;if mode==0{next.escape=true;}if mode==1{next.pointer=None;}if mode==2{size.width+=1.;}assert!(a.update(&next,size).is_empty());assert!(a.fade_drag.is_none());}}
}

#[cfg(test)]mod cc_editor_tests {
 use super::*;
 fn setup()->(Arrangement,Rect){let mut a=Arrangement::default();a.sync(&json!({"revision":1,"tracks":[{"id":"cc","name":"CC","kind":"cc"}],"clips":[{"id":"c","track":"cc","kind":"cc","start_seconds":2.,"duration_seconds":4.,"points":[]}]})).unwrap();(a,Rect{left:0.,top:0.,width:882.,height:300.})}
 fn pointer(x:f64,y:f64,left:bool)->Input{Input{pointer:Some([x,y]),left,..Default::default()}}
 #[test]fn add_drag_preview_and_release_preserve_clip_and_commit_points_once(){
  let(mut a,r)=setup();let b=clip_body(r);let x=b.left+3./16.*b.width;let top=b.top+CLIP_MARGIN+10.;let h=LANE_HEIGHT-2.*CLIP_MARGIN-10.;
  assert!(a.update(&pointer(x,top+h*0.5,true),r).is_empty());assert!(a.cc_drag.is_some());
  assert!(a.update(&pointer(x+b.width/16.,top,true),r).is_empty());assert_eq!(a.clips[0]["points"],json!([]));
  assert_eq!(a.range_overlay(r)["fades"][0]["handles"].as_array().unwrap().len(),1);
  let actions=a.update(&pointer(x+b.width/16.,top,false),r);let [Action::Document{hook,request}]=actions.as_slice()else{panic!()};
  assert_eq!(*hook,"WorkstationDataSystem.PutClip");assert_eq!(request["start_seconds"],2.);assert_eq!(request["points"][0],json!({"offset_seconds":2.,"value":1.}));
  assert!(a.update(&pointer(x,top,false),r).is_empty());
 }
 #[test]fn cancellation_never_commits_and_option_removes_only_nearby_point(){
  for mode in 0..4{let(mut a,r)=setup();let b=clip_body(r);let x=b.left+3./16.*b.width;let y=b.top+CLIP_MARGIN+29.;a.update(&pointer(x,y,true),r);let mut p=pointer(x,y,true);let mut next=r;
   match mode{0=>p.escape=true,1=>p.pointer=None,2=>next.width+=1.,_=>{a.sync(&json!({"revision":2,"tracks":[{"id":"cc","name":"CC","kind":"cc"}],"clips":[]})).unwrap();p.left=false;}}
   assert!(a.update(&p,next).is_empty());assert!(a.cc_drag.is_none());
  }
  let(mut a,r)=setup();a.clips[0]["points"]=json!([{"offset_seconds":1.,"value":0.5},{"offset_seconds":2.,"value":1.}]);let b=clip_body(r);
  let mut p=pointer(b.left+3./16.*b.width,b.top+CLIP_MARGIN+29.,true);p.alt=true;
  let actions=a.update(&p,r);let [Action::Document{request,..}]=actions.as_slice()else{panic!()};assert_eq!(request["points"],json!([{"offset_seconds":2.,"value":1.}]));
 }
}

#[cfg(test)]mod comp_lane_tests{
 use super::*;
 fn fixture()->Value{json!({"revision":1,"tracks":[{"id":"a","name":"Audio","kind":"audio","lane_height":64.,"gain":1.,"mute":false,"solo":false},{"id":"b","name":"Other","kind":"audio","lane_height":64.,"gain":1.,"mute":false,"solo":false}],"clips":[],"audio_playlists":[{"track":"a","expanded":true,"lanes":[{"name":"Main","clips":[]},{"name":"Take 1","clips":[{"id":"take","track":"a","kind":"audio","start_seconds":0.,"duration_seconds":2.,"source":"s","source_offset_seconds":0.}]}]}]})}
 fn rect()->Rect{Rect{left:0.,top:0.,width:900.,height:400.}}
 #[test]fn comp_lane_is_attached_resizes_independently_and_toggles_parent_once(){let mut a=Arrangement::default();a.sync(&fixture()).unwrap();assert_eq!(a.tracks.len(),3);assert_eq!(a.tracks[1]["comp_parent"],"a");assert_eq!(a.clips[0]["track"],a.tracks[1]["id"]);
  let y=RULER_HEIGHT+LANE_HEIGHT+32.;a.update(&Input{pointer:Some([6.,y]),left:true,..Default::default()},rect());a.update(&Input{pointer:Some([38.,y]),left:true,..Default::default()},rect());a.update(&Input{pointer:Some([38.,y]),..Default::default()},rect());assert_eq!(a.height(1),96.);assert_eq!(a.height(0),64.);
  let click=Input{pointer:Some([6.,RULER_HEIGHT+32.]),right:true,..Default::default()};let actions=a.update(&click,rect());assert!(matches!(actions.as_slice(),[Action::Document{hook:"WorkstationDataSystem.AudioPlaylist",request}]if request["id"]=="a"&&request["action"]=="toggle_lanes"));assert!(a.update(&click,rect()).is_empty());
 }
 #[test]fn selected_handle_lane_and_gutter_target_same_comp_parent(){
  for x in [6.,HANDLE_WIDTH+1.,500.] {
   let mut a=Arrangement::default();a.sync(&fixture()).unwrap();a.select_clip("take");
   a.update(&Input{pointer:Some([x,RULER_HEIGHT+32.]),left:true,..Default::default()},rect());
   assert_eq!(a.selected_track(),Some("a"));assert_eq!(a.selected_clip(),None);
   assert!(matches!(a.create_comp_at(a.selected_insertion_row()).unwrap(),Action::Document{hook:"WorkstationDataSystem.AudioPlaylist",request} if request["id"]=="a"&&request["action"]=="new"));
  }
  let mut a=Arrangement::default();a.sync(&fixture()).unwrap();
  assert!(a.create_comp_at(a.selected_insertion_row()).is_err());
  a.tracks[0]["kind"]=json!("midi");a.select_track("a");
  assert!(a.create_comp_at(a.selected_insertion_row()).is_err());
 }
 #[test]fn comp_lane_group_reorder_stays_attached_and_drop_requires_audio_row(){let mut a=Arrangement::default();a.sync(&fixture()).unwrap();let d=TrackDrag{id:"a".into(),revision:1,origin_y:0.,origin_x:0.,original_height:64.,last_x:0.,last_time:0.,edge_direction:0.,resize:false,original_index:0,preview_index:2,moved:true,canvas:rect()};let ordered=reordered_tracks(&a.tracks,&d);assert_eq!(ordered[0]["id"],"b");assert_eq!(ordered[1]["id"],"a");assert_eq!(ordered[2]["comp_parent"],"a");assert!(a.comp_drop_slot(rect(),[50.,RULER_HEIGHT+10.]).is_some());assert!(a.comp_drop_slot(rect(),[50.,350.]).is_none());}
}

#[cfg(test)]mod comp_lane_drag_tests{
 use super::*;
 #[test]fn comp_lane_clip_drag_and_slice_emit_revisioned_manual_edits(){
  let mut a=Arrangement::default();a.sync(&json!({"revision":4,"tracks":[{"id":"a","name":"Audio","kind":"audio","lane_height":64.,"gain":1.,"mute":false,"solo":false}],"clips":[],"audio_playlists":[{"track":"a","expanded":true,"lanes":[{"name":"Main","clips":[]},{"name":"Take 1","clips":[{"id":"take","track":"a","kind":"audio","start_seconds":0.,"duration_seconds":8.}]}]}]})).unwrap();
  let r=Rect{left:0.,top:0.,width:900.,height:400.};let y=RULER_HEIGHT+LANE_HEIGHT+CLIP_MARGIN+2.;
  a.update(&Input{pointer:Some([200.,y]),left:true,..Default::default()},r);
  a.update(&Input{pointer:Some([200.,y-LANE_HEIGHT]),left:true,..Default::default()},r);
  let actions=a.update(&Input{pointer:Some([200.,y-LANE_HEIGHT]),..Default::default()},r);
  assert!(matches!(actions.as_slice(),[Action::Document{hook:"WorkstationDataSystem.AudioPlaylist",request}]if request["action"]=="move_clip"&&request["clip_id"]=="take"&&request["lane"]==0&&request["document_revision"]==4));
 }
}

#[cfg(test)]mod audio_clip_editing {
 use super::*;
 fn setup()->(Arrangement,Rect){let mut a=Arrangement::default();a.sync(&json!({"revision":1,"tracks":[{"id":"a","name":"Audio","kind":"audio","gain":1.,"mute":false,"solo":false}],"clips":[{"id":"c","track":"a","kind":"audio","start_seconds":2.,"duration_seconds":8.}]})).unwrap();(a,Rect{left:0.,top:0.,width:882.,height:400.})}
 fn input(x:f64,y:f64,left:bool)->Input{Input{pointer:Some([x,y]),left,command:true,..Default::default()}}
 #[test]fn middle_selects_and_seeks_without_moving_audio(){let(mut a,r)=setup();let b=clip_body(r);let x=b.left+b.width*4./16.;let y=b.top+a.height(0)/2.;assert!(matches!(a.update(&input(x,y,true),r).as_slice(),[Action::Seek(t)]if *t==4.));assert_eq!(a.selected_clip(),Some("c"));assert!(a.drag.is_none());a.update(&input(x+50.,y,true),r);assert!(a.update(&input(x+50.,y,false),r).is_empty());assert_eq!(a.clips[0]["start_seconds"],2.);let actions=a.update(&Input{pointer:Some([x,y]),split_clip_pressed:true,..Default::default()},r);assert!(matches!(&actions[0],Action::Document{hook,request}if *hook=="WorkstationDataSystem.SplitClip"&&request["time_seconds"]==4.));}
 #[test]fn both_audio_lips_move_once_on_release(){for bottom in [false,true]{let(mut a,r)=setup();let b=clip_body(r);let x=b.left+b.width*4./16.;let y=b.top+if bottom{a.height(0)-CLIP_MARGIN-2.}else{CLIP_MARGIN+2.};a.update(&input(x,y,true),r);assert!(a.drag.is_some());assert!(a.update(&input(x+50.,y,true),r).is_empty());let actions=a.update(&input(x+50.,y,false),r);assert!(matches!(&actions[0],Action::Document{hook,..}if *hook=="WorkstationDataSystem.MoveClip"));assert!(a.update(&input(x+50.,y,false),r).is_empty());}}
}
#[cfg(test)]mod lane_control_tests{
 use super::*;
 fn setup()->(Arrangement,Value,Rect){let d=json!({"revision":1,"tracks":[{"id":"a","kind":"audio","name":"Audio","gain":1.,"arm_mode":0,"mute":false,"solo":false},{"id":"m","kind":"midi","name":"MIDI","gain":1.,"arm_mode":3,"mute":false,"solo":false}],"clips":[]});let mut a=Arrangement::default();a.sync(&d).unwrap();a.set_lane_controls([true;3]);(a,d,Rect{left:0.,top:0.,width:900.,height:400.})}
 fn pointer(p:[f64;2],left:bool)->Input{Input{pointer:Some(p),left,..Default::default()}}
 #[test]fn lane_fader_edits_live_and_retains_capture_through_own_ack(){let(mut a,mut d,r)=setup();let b=a.clip_body(r);let(h,_)=super::super::timeline_controls::columns(b.left+b.width+MONITOR_GUTTER_WIDTH,b.top+4.,88.,[true;3]);let k=super::super::timeline_controls::knob(h,1.);let p=[k[0]+k[2]*0.5,k[1]+k[3]*0.5];assert!(a.update(&pointer(p,true),r).is_empty());let q=[p[0],p[1]+20.];let edits=a.update(&pointer(q,true),r);let Action::Document{hook,request}=&edits[0]else{panic!()};assert_eq!(*hook,"WorkstationDataSystem.SetTrackMix");assert_eq!(request["id"],"a");assert!(request["gain"].as_f64().unwrap()<1.);d["revision"]=json!(2);d["tracks"][0]["gain"]=request["gain"].clone();a.sync(&d).unwrap();assert!(a.lane_fader.is_some());assert!(a.update(&pointer(q,false),r).is_empty());assert!(!a.is_interacting());}
 #[test]fn crosswalk_targets_lane_identity_and_does_not_seek(){let(mut a,_,r)=setup();let b=a.clip_body(r);let(_,arm)=super::super::timeline_controls::columns(b.left+b.width+MONITOR_GUTTER_WIDTH,b.top+96.+4.,56.,[true;3]);let edits=a.update(&pointer([arm[0]+arm[2]*0.5,arm[1]+arm[3]*0.5],true),r);assert!(matches!(edits.as_slice(),[Action::Document{hook,request}]if *hook=="WorkstationDataSystem.SetTrackMix"&&request["id"]=="m"&&request["arm_mode"]==1));}
 #[test]fn fader_cancels_on_escape_resize_missing_pointer_or_external_edit(){for mode in 0..4{let(mut a,mut d,r)=setup();let b=a.clip_body(r);let(h,_)=super::super::timeline_controls::columns(b.left+b.width+MONITOR_GUTTER_WIDTH,b.top+4.,88.,[true;3]);let p=[h[0]+12.,h[1]+10.];a.update(&pointer(p,true),r);assert!(a.lane_fader.is_some());let mut input=pointer(p,true);let mut size=r;match mode{0=>input.escape=true,1=>size.width+=1.,2=>input.pointer=None,_=>{d["revision"]=json!(2);d["tracks"][0]["mute"]=json!(true);a.sync(&d).unwrap();}}assert!(a.update(&input,size).is_empty());assert!(a.lane_fader.is_none());}}
}

#[cfg(test)]mod free_audio_tests {
 use super::*;
 #[test]fn empty_arrangement_accepts_audio_drop_without_creating_a_track(){
  let mut a=Arrangement::default();a.sync(&json!({"revision":0,"tracks":[],"clips":[]})).unwrap();
  let r=Rect{left:0.,top:0.,width:1000.,height:500.};let b=a.clip_body(r);
  let (id,time,_)=a.audio_drop(r,[b.left+b.width*0.5,b.top+30.]).unwrap();assert_eq!(id,"");assert!(time>0.);
  assert!(a.tracks.iter().all(|t|t["trackless"]==true));assert!(a.all_track_ids.is_empty());
 }
 #[test]fn free_lane_moves_are_document_edits_not_comp_edits(){
  let mut a=Arrangement::default();a.sync(&json!({"revision":7,"tracks":[],"clips":[{"id":"c","track":"","trackless":true,"lane":2,"kind":"audio","start_seconds":0.,"duration_seconds":1.}]})).unwrap();
  let action=a.edit("WorkstationDataSystem.MoveClip",json!({"id":"c","track":"","lane":3,"start_seconds":2.}));
  assert!(matches!(action,Action::Document{hook:"WorkstationDataSystem.MoveClip",request}if request["lane"]==3&&request["track"]==""));
  assert!(a.browser_plugin_target(Rect{left:0.,top:0.,width:1000.,height:500.},[200.,250.]).is_none());
 }
}

#[cfg(test)] mod free_canvas_tests {
 use super::*;
 fn rect()->Rect{Rect{left:100.,top:0.,width:900.,height:600.}}
 fn setup()->Arrangement{let mut a=Arrangement::default();a.sync(&json!({"revision":1,"tracks":[{"id":"real","name":"Real track","kind":"audio","gain":1.,"mute":false,"solo":false}],"clips":[{"id":"free","track":"","trackless":true,"lane":1.3125,"kind":"audio","start_seconds":2.,"duration_seconds":3.}]})).unwrap();a}
 fn input(p:[f64;2],left:bool)->Input{Input{pointer:Some(p),left,..Default::default()}}
 #[test] fn free_canvas_drag_retains_fractional_position_without_rows_or_monitors(){
  let mut a=setup();let r=rect();let b=a.clip_body(r);let p=[b.left+3./16.*b.width,b.top+126.+40.];
  assert_eq!(a.tracks.len(),1);assert_eq!(a.tracks[0]["id"],"real");assert_eq!(a.hit_free(p,r),Some(0));
  assert!(a.update(&input(p,true),r).is_empty());
  let q=[p[0]+19.,p[1]+117.25];a.update(&input(q,true),r);
  let preview=a.presentation(r,None,Value::Null);assert_eq!(preview["tracks"].as_array().unwrap().len(),1);assert_eq!(preview["clips"][0]["track"],"");
  assert!((preview["clips"][0]["free_top"].as_f64().unwrap()-(b.top+243.25)).abs()<1e-9);
  let actions=a.update(&input(q,false),r);let Action::Document{hook,request}=&actions[0]else{panic!("missing move")};
  assert_eq!(*hook,"WorkstationDataSystem.MoveClip");assert_eq!(request["track"],"");assert!((request["lane"].as_f64().unwrap()-243.25/96.).abs()<1e-9);
 }
 #[test] fn free_clip_drops_onto_audio_track_and_previews_assignment(){
  let mut a=setup();let r=rect();let b=a.clip_body(r);
  let p=[b.left+3./16.*b.width,b.top+166.];let q=[p[0]+19.,b.top+40.];
  a.update(&input(p,true),r);a.update(&input(q,true),r);
  assert_eq!(a.presentation(r,None,Value::Null)["clips"][0]["track"],"real");
  let actions=a.update(&input(q,false),r);
  assert!(matches!(&actions[0],Action::Document{hook:"WorkstationDataSystem.MoveClip",request} if request["track"]=="real"));
 }
 #[test] fn free_clip_double_click_opens_waveform(){
  let mut a=setup();let r=rect();let b=a.clip_body(r);let p=[b.left+3./16.*b.width,b.top+166.];
  a.update_at(&input(p,true),r,1.);a.update_at(&input(p,false),r,1.05);
  a.update_at(&input(p,true),r,1.15);
  assert!(a.update_at(&input(p,false),r,1.2).iter().any(|x|matches!(x,Action::OpenWaveform(id) if id=="free")));
 }
 #[test] fn empty_free_canvas_click_seeks_transport(){
  let mut a=setup();let r=rect();let b=a.clip_body(r);let p=[b.left+b.width*0.5,b.top+400.];
  assert!(a.update(&input(p,true),r).iter().any(|x|matches!(x,Action::Seek(t) if (*t-8.).abs()<1e-9)));
 }
 #[test] fn vertical_bar_anchors_zoom_and_pans_free_positions_without_editing_document(){
  let mut a=setup();let r=rect();let rail=Rect{left:65.,top:80.,width:26.,height:420.};a.set_vertical_rail(rail);
  let p=[78.,210.];let origin=a.free_lane_at(r,p);a.update(&input(p,true),r);
  let q=[p[0]-69.31471805599453,p[1]-35.];assert!(a.update(&input(q,true),r).is_empty());
  assert!((a.vertical_zoom-0.5).abs()<1e-9);assert!((a.free_lane_at(r,q)-origin).abs()<1e-9);
  a.update(&input(q,false),r);assert!(!a.is_interacting());assert_eq!(a.revision,Some(1));
  let view=a.presentation(r,None,Value::Null);assert_eq!(view["timeline_bars"]["vertical"][2],26.);assert_eq!(a.clips[0]["lane"],1.3125);
 }
 #[test]fn empty_canvas_drop_preserves_pointer_position_and_scroll(){let mut a=Arrangement::default();a.sync(&json!({"revision":1,"tracks":[],"clips":[]})).unwrap();a.vertical_zoom=0.5;a.vertical_offset=500.;let r=rect();let b=a.clip_body(r);assert!((a.free_lane_at(r,[b.left+20.,b.top+37.25])-(500.+74.5)/96.).abs()<1e-9);}
}

#[cfg(test)] mod requested_track_layout_tests {
 use super::*;
 fn document()->Value{json!({"revision":1,"tracks":[{"id":"a","kind":"audio","name":"A","gain":1.,"mute":false,"solo":false},{"id":"b","kind":"audio","name":"B","gain":1.,"mute":false,"solo":false}],"clips":[]})}
 fn canvas()->Rect{Rect{left:0.,top:0.,width:800.,height:600.}}
 #[test]fn settling_panel_does_not_cancel_edge_resize_and_reversal(){
  let mut a=Arrangement::default();a.sync(&document()).unwrap();let r=canvas();
  let input=|x,left|Input{pointer:Some([x,RULER_HEIGHT+20.]),left,..Default::default()};
  a.update_at(&input(5.,true),r,0.);
  let eased=Rect{left:0.00001,..r};
  a.update_at(&input(-12.,true),eased,0.01);
  assert!(a.track_drag.as_ref().unwrap().resize);assert_eq!(a.heights["a"],79.);
  a.update_at(&input(-7.,true),eased,0.02);assert_eq!(a.heights["a"],84.);
  a.update_at(&input(-7.,false),eased,0.03);assert!(a.track_drag.is_none());
 }
 #[test]fn top_bottom_free_share_render_and_pick_positions(){
  let mut a=Arrangement::default();a.sync(&document()).unwrap();a.last_canvas=Some(canvas());
  assert_eq!(a.row_top(0),0.);a.cycle_lane_layout();
  assert_eq!(a.row_top(1)+a.height(1),a.clip_body(canvas()).height);
  a.cycle_lane_layout();let original=a.row_top(0);a.free_tops.insert("a".into(),original+30.);
  assert_eq!(a.row_top(0),original+30.);assert_eq!(a.row_from_y(original+35.),0);
  let packet=a.presentation(canvas(),None,Value::Null);assert_eq!(packet["tracks"][0]["layout_top"],original+30.);
  a.cycle_lane_layout();assert_eq!(a.row_top(0),0.);
 }
}
